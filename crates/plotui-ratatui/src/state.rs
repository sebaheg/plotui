//! The app-owned widget state: plot, options, frame cache, gesture state.

use plotui_core::{Element, Plot, TraceError};
use plotui_term::policy::active_scale;
use plotui_term::{
    compose_frame, detect_cell_px, detect_render_mode, kitty_replace_env, next_image_id,
    tmux_wrap_with, FrameOutput, FrameRequest, RenderMode, FALLBACK_CELL_PX,
};
use ratatui::layout::Rect;
use ratatui::style::Style;

/// Construction-time options for a [`PlotState`] (the runtime knobs live on
/// the state itself).
pub struct PlotOptions {
    /// Spin the plot when the host calls [`PlotState::tick`] (~30 Hz).
    pub auto_rotate: bool,
    /// Device pixels per terminal cell; `None` queries the terminal
    /// (`detect_cell_px`), which yields true retina resolution.
    pub cell_px: Option<(u16, u16)>,
    /// Interactive picking: hover lights up nodes/edges, clicks report them.
    /// Off by default so plots without click semantics pay no per-mouse-move
    /// cost.
    pub pickable: bool,
    /// 2D hover crosshair (a guide snapped to the nearest sample x, a marker
    /// per series, a value readout). 3D plots are unaffected.
    pub crosshair: bool,
    /// `None` detects the best path for this terminal, honoring
    /// `PLOTUI_RENDER`; set to force one.
    pub render_mode: Option<RenderMode>,
    /// Resolution multiplier for large 3D plots *while interacting*
    /// (dragging or auto-rotating); `1.0` disables it. Full resolution is
    /// restored the moment interaction stops.
    pub interactive_scale: f64,
    /// Kitty image id; `None` allocates a fresh one, so several plots in one
    /// app never clobber each other's images.
    pub image_id: Option<u32>,
    /// Re-emit the image on every draw, not just when the frame changes —
    /// for hosts that layer popups over the plot in direct mode (the popup
    /// destroys the image but not ratatui's diff state).
    pub always_retransmit: bool,
}

impl Default for PlotOptions {
    fn default() -> Self {
        Self {
            auto_rotate: false,
            cell_px: None,
            pickable: false,
            crosshair: true,
            render_mode: None,
            interactive_scale: 0.5,
            image_id: None,
            always_retransmit: false,
        }
    }
}

/// A text span drawn over the plot: `(row, col)` in widget cells. Spans
/// replace the image at the cells they cover (labels sit on the terminal
/// background). Overlapping or off-widget spans are clipped or dropped.
#[derive(Clone)]
pub struct OverlaySpan {
    pub row: u16,
    pub col: u16,
    pub text: String,
    pub style: Style,
}

/// The state behind [`crate::PlotWidget`]: owned by the app, it outlives
/// every frame and carries the plot, the frame cache, and gesture state.
pub struct PlotState {
    pub(crate) plot: Plot,
    pub(crate) mode: RenderMode,
    pub(crate) cell_px: (u16, u16),
    pub(crate) image_id: u32,
    pub(crate) pickable: bool,
    pub(crate) crosshair: bool,
    pub(crate) auto_rotate: bool,
    pub(crate) interactive_scale: f64,
    pub(crate) always_retransmit: bool,
    pub(crate) replace: bool,
    pub(crate) in_tmux: bool,

    pub(crate) version: u64,
    pub(crate) frame_key: Option<(u16, u16, u64, u64)>, // (cols, rows, version, scale bits)
    pub(crate) frame: Option<FrameOutput>,
    pub(crate) overlay: Vec<OverlaySpan>,
    pub(crate) area: Rect,

    pub(crate) dragging: bool,
    pub(crate) moved: bool,
    pub(crate) last_pos: (u16, u16),
    /// The strip part grabbed by the active drag, if the drag started on the
    /// range slider (then it never rotates/pans the camera).
    pub(crate) range_drag: Option<plotui_core::RangeHit>,
    pub(crate) hovered: Option<Element>,
    pub(crate) interacting_override: bool,
    pub(crate) needs_redraw: bool,
    pub(crate) force_retransmit: bool,
}

impl PlotState {
    pub fn new(plot: Plot, opts: PlotOptions) -> Self {
        Self {
            plot,
            mode: opts.render_mode.unwrap_or_else(detect_render_mode),
            cell_px: opts.cell_px.unwrap_or_else(|| detect_cell_px(FALLBACK_CELL_PX)),
            image_id: opts.image_id.unwrap_or_else(next_image_id),
            pickable: opts.pickable,
            crosshair: opts.crosshair,
            auto_rotate: opts.auto_rotate,
            interactive_scale: opts.interactive_scale.clamp(0.05, 1.0),
            always_retransmit: opts.always_retransmit,
            replace: kitty_replace_env(),
            in_tmux: std::env::var("TMUX").is_ok_and(|v| !v.is_empty()),
            version: 0,
            frame_key: None,
            frame: None,
            overlay: Vec::new(),
            area: Rect::ZERO,
            dragging: false,
            moved: false,
            last_pos: (0, 0),
            range_drag: None,
            hovered: None,
            interacting_override: false,
            needs_redraw: true,
            force_retransmit: false,
        }
    }

    /// The wrapped plot. For mutation use [`plot_mut`](Self::plot_mut).
    pub fn plot(&self) -> &Plot {
        &self.plot
    }

    /// Mutable access to the plot; marks the view dirty (the next render
    /// re-rasterizes), so camera calls or trace edits through it just show up.
    pub fn plot_mut(&mut self) -> &mut Plot {
        self.invalidate();
        &mut self.plot
    }

    /// The render path in use (detected at construction unless forced).
    pub fn render_mode(&self) -> RenderMode {
        self.mode
    }

    /// The area the widget last rendered into (for host-side layout math).
    pub fn area(&self) -> Rect {
        self.area
    }

    /// True while the user is actively dragging (rotating/panning) — a hook
    /// for hosts that want to defer expensive work mid-gesture.
    pub fn dragging(&self) -> bool {
        self.dragging && self.moved
    }

    /// Host-reported "interaction in progress" override for the reduced-
    /// resolution policy (e.g. while animating the camera from a timer).
    pub fn set_interacting(&mut self, on: bool) {
        if self.interacting_override != on {
            self.interacting_override = on;
            self.invalidate();
        }
    }

    /// Mark the view dirty and request a repaint (call after mutating the
    /// plot). Cheap; multiple calls between frames coalesce into one repaint.
    pub fn invalidate(&mut self) {
        self.version += 1;
        self.needs_redraw = true;
    }

    /// True when the plot needs another `terminal.draw` — a take-flag for
    /// event-driven hosts (it resets to false on read).
    pub fn needs_redraw(&mut self) -> bool {
        std::mem::take(&mut self.needs_redraw)
    }

    /// Re-emit the image on the next draw even if the frame is unchanged —
    /// call after something else painted over the plot's area (a dialog, a
    /// screen switch) in direct mode.
    pub fn force_retransmit(&mut self) {
        self.force_retransmit = true;
        self.needs_redraw = true;
    }

    /// Append points to a trace by handle and repaint: 2D traces take
    /// `(xs, ys)`, 3D traces `(xs, ys, zs)` (zipped to the shortest).
    pub fn extend(
        &mut self,
        handle: usize,
        xs: &[f32],
        ys: &[f32],
        zs: Option<&[f32]>,
    ) -> Result<(), TraceError> {
        match zs {
            None => self.plot.extend_xy(handle, xs, ys)?,
            Some(zs) => {
                let n = xs.len().min(ys.len()).min(zs.len());
                let pts: Vec<[f32; 3]> = (0..n).map(|i| [xs[i], ys[i], zs[i]]).collect();
                self.plot.extend_pts(handle, &pts)?;
            }
        }
        self.invalidate();
        Ok(())
    }

    /// Move every node of a graph trace at once and repaint — the per-frame
    /// call of a force-directed layout (pair with
    /// [`plotui_core::ForceLayout`]).
    pub fn set_graph_positions(
        &mut self,
        handle: usize,
        positions: Vec<[f32; 3]>,
    ) -> Result<(), TraceError> {
        self.plot.set_graph_positions(handle, positions)?;
        self.invalidate();
        Ok(())
    }

    /// Recolor a graph trace in place and repaint — dim everything, brighten
    /// a hovered dependency path, restore.
    pub fn set_graph_colors(
        &mut self,
        handle: usize,
        node_colors: Vec<[u8; 3]>,
        edge_colors: Option<Vec<[u8; 3]>>,
    ) -> Result<(), TraceError> {
        self.plot.set_graph_colors(handle, node_colors, edge_colors)?;
        self.invalidate();
        Ok(())
    }

    /// Append nodes and edges to a graph trace and repaint (pair with
    /// [`plotui_core::ForceLayout::add_node`]).
    pub fn extend_graph(
        &mut self,
        handle: usize,
        nodes: &[[f32; 3]],
        node_colors: &[[u8; 3]],
        edges: &[(u32, u32)],
    ) -> Result<(), TraceError> {
        self.plot.extend_graph(handle, nodes, node_colors, edges)?;
        self.invalidate();
        Ok(())
    }

    /// Show or hide a trace by handle; repaints only when the state actually
    /// changed. Returns true when it did.
    pub fn set_visible(&mut self, handle: usize, visible: bool) -> bool {
        let changed = self.plot.set_visible(handle, visible).unwrap_or(false);
        if changed {
            self.invalidate();
        }
        changed
    }

    /// Draw text over the plot (labels, badges). Spans replace the image at
    /// the cells they cover; repaints without re-rasterizing the image.
    pub fn set_overlay(&mut self, spans: Vec<OverlaySpan>) {
        self.overlay = spans;
        self.needs_redraw = true;
    }

    /// One auto-rotate step; call from the host's timer (~30 Hz) when
    /// `auto_rotate` is on. A no-op otherwise.
    pub fn tick(&mut self) {
        if self.auto_rotate {
            self.plot.camera.rotate(plotui_term::policy::AUTO_ROTATE_STEP, 0.0);
            self.invalidate();
        }
    }

    /// Write the escape that deletes this plot's image from the terminal.
    /// Call before restoring the terminal (leaving the alternate screen),
    /// or the last frame outlives the app on terminals that keep placements.
    pub fn cleanup(&self, w: &mut impl std::io::Write) -> std::io::Result<()> {
        if self.mode == RenderMode::Unsupported {
            return Ok(());
        }
        let escape = plotui_protocol::kitty_cleanup_with_id(self.image_id);
        w.write_all(tmux_wrap_with(&escape, self.in_tmux).as_bytes())?;
        w.flush()
    }

    /// Resolution multiplier for the next frame (see `policy::active_scale`).
    pub(crate) fn active_scale(&self) -> f64 {
        let interacting = self.dragging() || self.auto_rotate || self.interacting_override;
        active_scale(
            self.interactive_scale,
            self.plot.is_3d(),
            self.plot.vertex_count(),
            interacting,
        )
    }

    /// Rasterize + encode the current frame for `cols`×`rows` if the cached
    /// one is stale.
    pub(crate) fn ensure_frame(&mut self, cols: u16, rows: u16) {
        if self.mode == RenderMode::Unsupported {
            return;
        }
        let scale = self.active_scale();
        let key = (cols, rows, self.version, scale.to_bits());
        if self.frame_key == Some(key) {
            return;
        }
        self.frame_key = Some(key);
        self.frame = Some(compose_frame(
            &self.plot,
            &FrameRequest {
                cols,
                rows,
                cell_w: self.cell_px.0,
                cell_h: self.cell_px.1,
                scale,
                mode: self.mode,
                image_id: self.image_id,
                replace: self.replace,
                tmux: self.in_tmux,
            },
        ));
    }

    /// Update the hovered element with change detection; returns the event to
    /// surface when it changed.
    pub(crate) fn set_hover(&mut self, element: Option<Element>) -> Option<crate::PlotEvent> {
        if element == self.hovered {
            return None;
        }
        self.hovered = element;
        if self.plot.hovered != element {
            self.plot.hovered = element;
            self.invalidate();
        }
        Some(crate::PlotEvent::ElementHovered(element.map(crate::to_kind)))
    }

    /// Update the 2D crosshair position (framebuffer px) with change
    /// detection.
    pub(crate) fn set_hover2d(&mut self, px: Option<f32>) {
        if self.plot.hover2d_px != px {
            self.plot.hover2d_px = px;
            self.invalidate();
        }
    }

    /// Set (or clear) the 2D x window programmatically, with change
    /// detection and a repaint. Interactive changes arrive through
    /// `handle_event` instead and report as [`PlotEvent::RangeChanged`].
    pub fn set_x_window(&mut self, window: Option<(f64, f64)>) -> bool {
        if self.plot.x_window == window {
            return false;
        }
        self.plot.x_window = window;
        self.invalidate();
        true
    }
}
