//! crossterm event → plot interaction, with exact parity to the Textual
//! widget's mappings (same constants, via `plotui_term::policy`).

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use plotui_core::{DragScales, Element, RangeHit};
use plotui_term::policy::{
    pixel_geometry, DRAG_ZOOM_PER_CELL, EDGE_RADIUS_FACTOR, KEY_PAN_CELLS, KEY_ROTATE_STEP,
    KEY_WINDOW_STEP_FRAC, RANGE_GRAB_TOL_CELLS, ROTATE_PER_CELL, ZOOM_IN, ZOOM_OUT,
};
use plotui_term::RenderMode;
use ratatui::layout::Position;

use crate::state::PlotState;
use crate::{to_kind, PlotEvent};

impl PlotState {
    /// Feed a terminal event to the plot: drags rotate (shift-drags pan),
    /// the scroll wheel zooms, clicks pick, hovering drives the crosshair or
    /// element highlight, and the arrow/`+`/`-`/`r` keys mirror the mouse.
    /// Returns an event when an interaction produced one for the host.
    ///
    /// Mouse events are hit-tested against the last rendered area; key events
    /// apply unconditionally, so hosts with several focusable widgets should
    /// forward keys only while the plot has focus.
    pub fn handle_event(&mut self, ev: &Event) -> Option<PlotEvent> {
        if self.mode == RenderMode::Unsupported {
            return None;
        }
        match ev {
            Event::Mouse(m) => self.handle_mouse(m),
            Event::Key(k) => self.handle_key(k),
            _ => None,
        }
    }

    fn contains(&self, m: &MouseEvent) -> bool {
        self.area.contains(Position { x: m.column, y: m.row })
    }

    /// Cell coordinate relative to the widget's origin.
    fn rel(&self, m: &MouseEvent) -> (u16, u16) {
        (m.column.saturating_sub(self.area.x), m.row.saturating_sub(self.area.y))
    }

    fn handle_mouse(&mut self, m: &MouseEvent) -> Option<PlotEvent> {
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if self.contains(m) {
                    // The legend owns its rows: a press there shows or hides
                    // that series instead of grabbing the camera, so a click
                    // on a row can never smear into a rotation.
                    let (lx, ly) = self.rel(m);
                    let (pw, ph, px, py, _) = self.geometry(lx, ly);
                    if let Some(id) = self.plot.legend_hit(pw, ph, px, py) {
                        let shown = self.plot.toggle_muted(id).unwrap_or(true);
                        self.invalidate();
                        return Some(PlotEvent::LegendToggled(id, shown));
                    }
                    self.dragging = true;
                    self.moved = false;
                    self.last_pos = (m.column, m.row);
                    // A press on the range-slider strip grabs it instead of
                    // the camera; a track press jumps the window there first
                    // and then drags it as the window body.
                    if !self.plot.is_3d() && self.plot.range_slider {
                        let (x, y) = self.rel(m);
                        let (pw, ph, px, py, _) = self.geometry(x, y);
                        let tol = self.cell_px.0 as f32 * RANGE_GRAB_TOL_CELLS;
                        if let Some(hit) = self.plot.range_slider_hit(pw, ph, px, py, tol) {
                            self.range_drag = Some(if hit == RangeHit::Track {
                                if self.plot.jump_x_window(pw, ph, px) {
                                    self.invalidate();
                                }
                                RangeHit::Window
                            } else {
                                hit
                            });
                        }
                    }
                }
                None
            }
            MouseEventKind::Drag(MouseButton::Left) if self.dragging => {
                // Deltas from screen coordinates: drags keep arriving even
                // when the pointer leaves the widget (Textual's capture_mouse
                // equivalent comes for free).
                let dx = m.column as f64 - self.last_pos.0 as f64;
                let dy = m.row as f64 - self.last_pos.1 as f64;
                self.last_pos = (m.column, m.row);
                if dx != 0.0 || dy != 0.0 {
                    self.moved = true;
                }
                let dx_px = (dx * self.cell_px.0 as f64) as f32;
                let shift = m.modifiers.contains(KeyModifiers::SHIFT);
                if let Some(part) = self.range_drag {
                    let (pw, ph, ..) = self.geometry(0, 0);
                    if self.plot.drag_x_window(pw, ph, part, dx_px) {
                        self.invalidate();
                    }
                } else if !shift && !self.plot.is_3d() && self.plot.x_window.is_some() {
                    // With a window set, a plain plot-area drag slides the
                    // window (the camera is superseded).
                    let (pw, ph, ..) = self.geometry(0, 0);
                    if self.plot.pan_x_window(pw, ph, dx_px) {
                        self.invalidate();
                    }
                } else {
                    // Routed through the plot's input map: drag rotates
                    // (trackball — drag right turns the object right),
                    // shift-drag pans, unless the host remapped it. Pan is
                    // in full-resolution image pixels, so one dragged cell
                    // is one cell's worth of pixels and the plot stays
                    // under the pointer.
                    self.plot.apply_drag(
                        dx,
                        dy,
                        shift,
                        DragScales {
                            rotate: ROTATE_PER_CELL,
                            pan_x: self.cell_px.0 as f64,
                            pan_y: self.cell_px.1 as f64,
                            zoom: DRAG_ZOOM_PER_CELL,
                        },
                    );
                    self.invalidate();
                }
                None
            }
            MouseEventKind::Up(MouseButton::Left) if self.dragging => {
                let was_click = !self.moved;
                self.dragging = false;
                if self.range_drag.take().is_some() {
                    // The strip gesture ended: one event with the result.
                    self.invalidate();
                    return Some(PlotEvent::RangeChanged(self.plot.x_window));
                }
                if was_click {
                    self.click_at(m)
                } else {
                    // The gesture ended: repaint so a half-res interaction
                    // frame is replaced by a crisp full-res one.
                    self.invalidate();
                    None
                }
            }
            MouseEventKind::Moved => self.hover_at(m),
            MouseEventKind::ScrollUp if self.contains(m) => self.scroll(m, ZOOM_IN),
            MouseEventKind::ScrollDown if self.contains(m) => self.scroll(m, ZOOM_OUT),
            _ => None,
        }
    }

    /// Scroll: with an x window set on a 2D plot the wheel zooms the window
    /// about the cursor; otherwise it zooms the camera.
    fn scroll(&mut self, m: &MouseEvent, factor: f64) -> Option<PlotEvent> {
        if !self.plot.is_3d() && self.plot.x_window.is_some() {
            let (x, y) = self.rel(m);
            let (pw, ph, px, _, _) = self.geometry(x, y);
            if self.plot.zoom_x_window(pw, ph, px, factor) {
                self.invalidate();
                return Some(PlotEvent::RangeChanged(self.plot.x_window));
            }
            return None;
        }
        self.plot.camera.zoom_by(factor);
        self.invalidate();
        None
    }

    /// Click semantics: a press-and-release without movement picks and
    /// selects what's under the cursor.
    fn click_at(&mut self, m: &MouseEvent) -> Option<PlotEvent> {
        let (x, y) = self.rel(m);
        let (pw, ph, px, py, radius) = self.geometry(x, y);
        if self.pickable {
            let element =
                self.plot.pick_element(pw, ph, px, py, radius, radius * EDGE_RADIUS_FACTOR);
            self.plot.selected = element;
            self.invalidate();
            return Some(PlotEvent::ElementPicked(element.map(to_kind)));
        }
        let idx = self.plot.pick(pw, ph, px, py, radius);
        self.plot.selected = idx.map(Element::Node);
        self.invalidate();
        Some(PlotEvent::NodePicked(idx))
    }

    fn hover_at(&mut self, m: &MouseEvent) -> Option<PlotEvent> {
        if !self.contains(m) {
            // No leave event in crossterm: leaving the widget's area
            // substitutes for it — clear the crosshair and the highlight.
            self.set_hover2d(None);
            return if self.pickable { self.set_hover(None) } else { None };
        }
        let (x, y) = self.rel(m);
        if !self.plot.is_3d() {
            if self.crosshair {
                let (_, _, px, _, _) = self.geometry(x, y);
                self.set_hover2d(Some(px));
            }
            None
        } else if self.pickable {
            let (pw, ph, px, py, radius) = self.geometry(x, y);
            let element =
                self.plot.pick_element(pw, ph, px, py, radius, radius * EDGE_RADIUS_FACTOR);
            self.set_hover(element)
        } else {
            None
        }
    }

    fn handle_key(&mut self, k: &KeyEvent) -> Option<PlotEvent> {
        if !matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return None;
        }
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        let (cw, ch) = (self.cell_px.0 as f64, self.cell_px.1 as f64);
        match k.code {
            // `[`/`]` slide a set x window by a tenth of its span.
            KeyCode::Char('[') | KeyCode::Char(']') if self.plot.x_window.is_some() => {
                let dir = if k.code == KeyCode::Char('[') { -1.0 } else { 1.0 };
                if self.plot.shift_x_window(dir * KEY_WINDOW_STEP_FRAC) {
                    self.invalidate();
                    return Some(PlotEvent::RangeChanged(self.plot.x_window));
                }
                return None;
            }
            KeyCode::Char('+') | KeyCode::Char('=') => self.plot.camera.zoom_by(ZOOM_IN),
            KeyCode::Char('-') => self.plot.camera.zoom_by(ZOOM_OUT),
            KeyCode::Left if shift => self.plot.camera.pan(-KEY_PAN_CELLS * cw, 0.0),
            KeyCode::Right if shift => self.plot.camera.pan(KEY_PAN_CELLS * cw, 0.0),
            KeyCode::Up if shift => self.plot.camera.pan(0.0, -KEY_PAN_CELLS * ch),
            KeyCode::Down if shift => self.plot.camera.pan(0.0, KEY_PAN_CELLS * ch),
            // Arrows nudge like a drag in that direction (trackball: the
            // object follows — Left turns the object left).
            KeyCode::Left => self.plot.camera.rotate(KEY_ROTATE_STEP, 0.0),
            KeyCode::Right => self.plot.camera.rotate(-KEY_ROTATE_STEP, 0.0),
            KeyCode::Up => self.plot.camera.rotate(0.0, KEY_ROTATE_STEP),
            KeyCode::Down => self.plot.camera.rotate(0.0, -KEY_ROTATE_STEP),
            KeyCode::Char('r') => {
                // Reset restores both the camera and the full x extent.
                self.plot.camera.reset();
                if self.plot.x_window.take().is_some() {
                    self.invalidate();
                    return Some(PlotEvent::RangeChanged(None));
                }
            }
            _ => return None,
        }
        self.invalidate();
        None
    }

    /// Full-resolution pixel geometry for a widget-relative cell coordinate.
    fn geometry(&self, x: u16, y: u16) -> (usize, usize, f32, f32, f32) {
        pixel_geometry(self.area.width, self.area.height, self.cell_px.0, self.cell_px.1, x, y)
    }
}
