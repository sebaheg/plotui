//! The per-frame pipeline: one plot + one request in, terminal-ready output
//! out. Every frontend (Textual via the Python bindings, Ratatui, and the C
//! ABI behind Bubble Tea) calls this instead of composing core + protocol
//! itself, so the rasterize/encode/wrap steps cannot drift between them.

use crate::policy::scaled_dims;
use crate::{tmux_wrap_with, RenderMode};
use plotui_core::Plot;

/// Everything that determines one frame's bytes.
pub struct FrameRequest {
    /// Cell region the image spans.
    pub cols: u16,
    pub rows: u16,
    /// Device pixels per cell (see `detect_cell_px`).
    pub cell_w: u16,
    pub cell_h: u16,
    /// Resolution multiplier, usually from `policy::active_scale`. The image
    /// still fills the same cell region; the terminal upscales it.
    pub scale: f64,
    pub mode: RenderMode,
    /// Kitty image id (see `next_image_id` for multi-plot hosts).
    pub image_id: u32,
    /// Direct tier only: skip the delete-before-transmit, for terminals whose
    /// Kitty decoder replaces a same-id image atomically (xterm.js).
    pub replace: bool,
    /// Direct tier only: wrap the escape for tmux passthrough.
    pub tmux: bool,
}

/// One frame, in the shape the requested render mode needs.
pub enum FrameOutput {
    /// Placeholder tier: emit `transmit` once (zero visible width), then draw
    /// `cells[y][x]` — each already carrying its position diacritics — with
    /// `id_rgb` as the foreground color.
    Placeholder { transmit: String, id_rgb: (u8, u8, u8), cells: Vec<Vec<String>> },
    /// Direct tier: emit with the cursor at the region's top-left (the escape
    /// saves and restores the cursor itself).
    Direct { escape: String },
    /// No pixels for this terminal — show `policy::UNSUPPORTED_MESSAGE`.
    Unsupported,
}

/// Rasterize `plot` and encode it for the terminal described by `req`.
pub fn compose_frame(plot: &Plot, req: &FrameRequest) -> FrameOutput {
    if req.mode == RenderMode::Unsupported {
        return FrameOutput::Unsupported;
    }
    let (pw, ph, pan_scale) = scaled_dims(req.cols, req.rows, req.cell_w, req.cell_h, req.scale);
    let fb = plot.render_at(pw, ph, pan_scale);
    match req.mode {
        RenderMode::Placeholder => {
            let p = plotui_protocol::kitty_placeholder_cells_with_id(
                &fb,
                req.cols,
                req.rows,
                req.image_id,
            );
            FrameOutput::Placeholder { transmit: p.transmit, id_rgb: p.id_rgb, cells: p.cells }
        }
        RenderMode::Direct => {
            // The direct tier exists for terminals (iTerm2) that need the
            // image id repeated on every data chunk, so it always uses the
            // compat framing.
            let escape = plotui_protocol::kitty_compat_with_id(
                &fb,
                req.cols,
                req.rows,
                !req.replace,
                req.image_id,
            );
            FrameOutput::Direct { escape: tmux_wrap_with(&escape, req.tmux) }
        }
        RenderMode::Unsupported => unreachable!(),
    }
}
