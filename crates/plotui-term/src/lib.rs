//! plotui-term — the terminal-environment glue every plotui frontend shares.
//!
//! `plotui-core` owns pixels and `plotui-protocol` owns escape bytes; both are
//! pure. What's left between them and a real widget is *policy*: which render
//! path this terminal supports, how many device pixels a cell has, how escapes
//! reach a terminal through tmux, when to drop to half resolution during
//! interaction, and how a frame request turns into terminal-ready output.
//! That policy used to live only in the Python Textual widget; this crate is
//! its single Rust home, so the Textual, Ratatui, and Bubble Tea frontends
//! (the last via the C ABI) cannot drift apart.
//!
//! Nothing here touches the terminal either — detection reads the environment
//! and a tty ioctl, and everything else is a pure function the frontend calls.

mod compose;
mod detect;
mod ids;
pub mod policy;
mod tmux;

pub use compose::{compose_frame, FrameOutput, FrameRequest};
pub use detect::{
    cell_px_from_winsize, detect_cell_px, detect_render_mode, detect_render_mode_from,
    kitty_replace_env,
};
pub use ids::next_image_id;
pub use tmux::{tmux_wrap, tmux_wrap_with};

/// Fallback device pixels per terminal cell, used when the terminal doesn't
/// report its own size. The terminal scales the image to the cell grid, so a
/// too-small guess renders below native resolution and upscales soft —
/// detection ([`detect_cell_px`]) avoids that.
pub const FALLBACK_CELL_PX: (u16, u16) = (12, 24);

/// How a frontend gets pixels onto this terminal.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RenderMode {
    /// Kitty graphics via Unicode placeholders (`U=1`) — Kitty, Ghostty.
    /// Flicker-free and splices with text overlays.
    Placeholder,
    /// Kitty graphics drawn at the widget origin — terminals that speak the
    /// protocol but not placeholders: iTerm2 ≥ 3.5, WezTerm, Konsole, and
    /// (with younger decoders) Warp, Rio, and VS Code.
    Direct,
    /// No Kitty graphics: show a notice naming supported terminals
    /// ([`policy::UNSUPPORTED_MESSAGE`]) rather than a degraded plot.
    Unsupported,
}
