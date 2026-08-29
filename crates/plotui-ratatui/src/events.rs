//! crossterm event → plot interaction, with exact parity to the Textual
//! widget's mappings (same constants, via `plotui_term::policy`).

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use plotui_core::Element;
use plotui_term::policy::{
    pixel_geometry, EDGE_RADIUS_FACTOR, KEY_PAN_CELLS, KEY_ROTATE_STEP, ROTATE_PER_CELL, ZOOM_IN,
    ZOOM_OUT,
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
            Event::Key(k) => {
                self.handle_key(k);
                None
            }
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
                    self.dragging = true;
                    self.moved = false;
                    self.last_pos = (m.column, m.row);
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
                if m.modifiers.contains(KeyModifiers::SHIFT) {
                    // Pan is in full-resolution image pixels, so one dragged
                    // cell is one cell's worth of pixels: the plot stays
                    // under the pointer instead of lagging it.
                    self.plot.camera.pan(dx * self.cell_px.0 as f64, dy * self.cell_px.1 as f64);
                } else {
                    // Negated: dragging grabs the camera, not the object —
                    // drag right orbits the view right (website-example feel).
                    self.plot.camera.rotate(-dx * ROTATE_PER_CELL, -dy * ROTATE_PER_CELL);
                }
                self.invalidate();
                None
            }
            MouseEventKind::Up(MouseButton::Left) if self.dragging => {
                let was_click = !self.moved;
                self.dragging = false;
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
            MouseEventKind::ScrollUp if self.contains(m) => {
                self.plot.camera.zoom_by(ZOOM_IN);
                self.invalidate();
                None
            }
            MouseEventKind::ScrollDown if self.contains(m) => {
                self.plot.camera.zoom_by(ZOOM_OUT);
                self.invalidate();
                None
            }
            _ => None,
        }
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

    fn handle_key(&mut self, k: &KeyEvent) {
        if !matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return;
        }
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        let (cw, ch) = (self.cell_px.0 as f64, self.cell_px.1 as f64);
        match k.code {
            KeyCode::Char('+') | KeyCode::Char('=') => self.plot.camera.zoom_by(ZOOM_IN),
            KeyCode::Char('-') => self.plot.camera.zoom_by(ZOOM_OUT),
            KeyCode::Left if shift => self.plot.camera.pan(-KEY_PAN_CELLS * cw, 0.0),
            KeyCode::Right if shift => self.plot.camera.pan(KEY_PAN_CELLS * cw, 0.0),
            KeyCode::Up if shift => self.plot.camera.pan(0.0, -KEY_PAN_CELLS * ch),
            KeyCode::Down if shift => self.plot.camera.pan(0.0, KEY_PAN_CELLS * ch),
            KeyCode::Left => self.plot.camera.rotate(KEY_ROTATE_STEP, 0.0),
            KeyCode::Right => self.plot.camera.rotate(-KEY_ROTATE_STEP, 0.0),
            KeyCode::Up => self.plot.camera.rotate(0.0, KEY_ROTATE_STEP),
            KeyCode::Down => self.plot.camera.rotate(0.0, -KEY_ROTATE_STEP),
            KeyCode::Char('r') => self.plot.camera.reset(),
            _ => return,
        }
        self.invalidate();
    }

    /// Full-resolution pixel geometry for a widget-relative cell coordinate.
    fn geometry(&self, x: u16, y: u16) -> (usize, usize, f32, f32, f32) {
        pixel_geometry(self.area.width, self.area.height, self.cell_px.0, self.cell_px.1, x, y)
    }
}
