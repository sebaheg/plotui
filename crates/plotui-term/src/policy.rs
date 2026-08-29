//! The shared interaction/render policy: every tunable the Textual widget
//! established, named once so all frontends move together.

/// Above this 3D vertex count, plots drop to reduced resolution *while
/// interacting* (dragging or auto-rotating) and snap back when still.
/// Vertices, not nodes: line vertices and surface grids load the rasterizer
/// just as much as pickable nodes do.
pub const LARGE_VERTEX_COUNT: usize = 400;

/// Radians of yaw/pitch per cell of mouse drag.
pub const ROTATE_PER_CELL: f64 = 0.03;

/// Radians per arrow-key press.
pub const KEY_ROTATE_STEP: f64 = 0.1;

/// Cells of pan per shift+arrow press (multiplied by the cell pixel size —
/// pan is in framebuffer pixels).
pub const KEY_PAN_CELLS: f64 = 2.0;

/// Zoom factor per scroll-up / `+` press.
pub const ZOOM_IN: f64 = 1.1;
/// Zoom factor per scroll-down / `-` press.
pub const ZOOM_OUT: f64 = 0.9;

/// Default edge pick radius as a fraction of the node pick radius.
pub const EDGE_RADIUS_FACTOR: f32 = 0.75;

/// Radians of yaw per auto-rotate tick (frontends tick at ~30 Hz).
pub const AUTO_ROTATE_STEP: f64 = 0.02;

/// What to show, centered, instead of a degraded plot when the terminal has
/// no Kitty graphics. Line 1 is conventionally bold and line 4 dim; styling
/// is per-frontend, the strings are shared. (The Python and Go frontends keep
/// their own styled copies — change these only in lockstep with
/// `_UNSUPPORTED_MESSAGE` in python/plotui/textual.py and
/// `unsupportedMessage` in go/teaplot/teaplot.go.)
pub const UNSUPPORTED_MESSAGE: [&str; 4] = [
    "Plotting requires a terminal that supports the Kitty graphics protocol.",
    "",
    "Supported terminals include Kitty, Ghostty, iTerm2 (3.5+), WezTerm, and Konsole.",
    "If yours does support it, force a path with PLOTUI_RENDER=placeholder|direct.",
];

/// Framebuffer pixel size for `cols`×`rows` cells at `scale`, plus the pan
/// scale that keeps a reduced-resolution frame aligned with the full-res one
/// (the `pan_scale` contract of `Plot::render_at`). Scale clamps to
/// `0.05..=1.0`.
pub fn scaled_dims(
    cols: u16,
    rows: u16,
    cell_w: u16,
    cell_h: u16,
    scale: f64,
) -> (usize, usize, f64) {
    let s = scale.clamp(0.05, 1.0);
    let full_w = cols as usize * cell_w.max(1) as usize;
    let full_h = rows as usize * cell_h.max(1) as usize;
    let pw = ((full_w as f64 * s).round() as usize).max(1);
    let ph = ((full_h as f64 * s).round() as usize).max(1);
    (pw, ph, s)
}

/// Resolution multiplier for the next frame: `interactive_scale` only for
/// large 3D plots while interacting, else 1.0 — a still plot is always at
/// full resolution.
pub fn active_scale(
    interactive_scale: f64,
    is_3d: bool,
    vertex_count: usize,
    interacting: bool,
) -> f64 {
    if interactive_scale >= 1.0 || vertex_count < LARGE_VERTEX_COUNT || !is_3d {
        return 1.0;
    }
    if interacting {
        interactive_scale
    } else {
        1.0
    }
}

/// Map a cell coordinate into the full-resolution framebuffer's pixel space:
/// `(px_w, px_h, px, py, node_pick_radius)`. Picks and hover geometry always
/// use full resolution, whatever scale the last frame rendered at.
pub fn pixel_geometry(
    cols: u16,
    rows: u16,
    cell_w: u16,
    cell_h: u16,
    x: u16,
    y: u16,
) -> (usize, usize, f32, f32, f32) {
    let cw = cell_w.max(1) as f32;
    let ch = cell_h.max(1) as f32;
    (
        cols as usize * cell_w.max(1) as usize,
        rows as usize * cell_h.max(1) as usize,
        x as f32 * cw + cw / 2.0,
        y as f32 * ch + ch / 2.0,
        ch,
    )
}
