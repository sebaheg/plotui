//! Terminal capability detection, from the environment and the tty.

use crate::RenderMode;

/// Pick the best render path for this terminal, honoring the `PLOTUI_RENDER`
/// override (`placeholder` | `direct`, with `kitty` accepted as an alias for
/// `placeholder`). Reads the process environment; the `_from` variant takes
/// an injected lookup for tests.
pub fn detect_render_mode() -> RenderMode {
    detect_render_mode_from(|k| std::env::var(k).ok())
}

/// [`detect_render_mode`] over an injected environment lookup.
///
/// The tiering mirrors the terminal support matrix: Kitty and Ghostty do
/// Unicode-placeholder placement; iTerm2 (≥ 3.5), WezTerm, Konsole, Warp,
/// Rio, and VS Code speak the Kitty protocol but not placeholders; anything
/// else gets no pixels.
pub fn detect_render_mode_from(get: impl Fn(&str) -> Option<String>) -> RenderMode {
    let forced = get("PLOTUI_RENDER").unwrap_or_default().trim().to_lowercase();
    match forced.as_str() {
        "kitty" | "placeholder" => return RenderMode::Placeholder,
        "direct" => return RenderMode::Direct,
        _ => {}
    }

    let has = |k: &str| get(k).is_some_and(|v| !v.is_empty());
    let term = get("TERM").unwrap_or_default();
    let term_program = get("TERM_PROGRAM").unwrap_or_default();

    // Placeholder tier: Kitty sets KITTY_WINDOW_ID / TERM=xterm-kitty;
    // Ghostty speaks the same protocol (placeholders included).
    if has("KITTY_WINDOW_ID")
        || term.contains("kitty")
        || term.contains("ghostty")
        || term_program.to_lowercase() == "ghostty"
        || has("GHOSTTY_RESOURCES_DIR")
    {
        return RenderMode::Placeholder;
    }

    // Direct tier: Kitty graphics without Unicode placeholders.
    if term_program == "iTerm.app" || get("LC_TERMINAL").as_deref() == Some("iTerm2") {
        let version = get("TERM_PROGRAM_VERSION")
            .filter(|v| !v.is_empty())
            .or_else(|| get("LC_TERMINAL_VERSION"))
            .unwrap_or_default();
        return if version_at_least(&version, (3, 5)) {
            RenderMode::Direct
        } else {
            RenderMode::Unsupported
        };
    }
    if term_program == "WezTerm" || has("WEZTERM_EXECUTABLE") || has("KONSOLE_VERSION") {
        return RenderMode::Direct;
    }
    // Direct tier, partial implementations: Warp, Rio, and VS Code's xterm.js
    // terminal speak Kitty graphics without placeholders, with young decoders
    // (VS Code additionally gates images behind terminal.integrated.enableImages).
    if term_program == "WarpTerminal"
        || term_program.to_lowercase() == "rio"
        || term.starts_with("rio")
        || term_program == "vscode"
    {
        return RenderMode::Direct;
    }

    RenderMode::Unsupported
}

fn version_at_least(version: &str, minimum: (u32, u32)) -> bool {
    let mut parts = version.split('.');
    let major = parts.next().and_then(|p| p.parse::<u32>().ok());
    let minor = parts.next().map_or(Some(0), |p| p.parse::<u32>().ok());
    match (major, minor) {
        (Some(major), Some(minor)) => (major, minor) >= minimum,
        _ => false,
    }
}

/// True when `PLOTUI_KITTY_REPLACE` asks direct mode to skip the
/// delete-before-transmit (for terminals whose Kitty decoder replaces a
/// same-id image atomically, e.g. xterm.js's addon-image, where the delete
/// blanks the image between async redraws and flickers during interaction).
pub fn kitty_replace_env() -> bool {
    std::env::var("PLOTUI_KITTY_REPLACE").is_ok_and(|v| matches!(v.trim(), "1" | "true"))
}

/// The terminal's device pixels per cell, queried via the `TIOCGWINSZ` ioctl
/// (`ws_xpixel`/`ws_ypixel`). Kitty, Ghostty, iTerm2, and WezTerm all report
/// it — in *device* pixels, so this yields the true retina resolution. Probes
/// stdout, stderr, then stdin; returns `fallback` when no stream reports a
/// pixel size (or on platforms without termios).
pub fn detect_cell_px(fallback: (u16, u16)) -> (u16, u16) {
    detect_cell_px_impl(fallback)
}

/// The per-cell pixel size from a winsize report, or `None` when the terminal
/// reports no pixel size (any field zero) — the pure core of
/// [`detect_cell_px`], split out so it can be tested without a tty.
pub fn cell_px_from_winsize(rows: u16, cols: u16, xpix: u16, ypix: u16) -> Option<(u16, u16)> {
    if rows == 0 || cols == 0 || xpix == 0 || ypix == 0 {
        return None;
    }
    Some(((xpix / cols).max(1), (ypix / rows).max(1)))
}

#[cfg(unix)]
fn detect_cell_px_impl(fallback: (u16, u16)) -> (u16, u16) {
    use std::os::fd::BorrowedFd;
    // stdout, stderr, stdin — same probe order as the Python frontend.
    for raw in [1, 2, 0] {
        // SAFETY: the standard fds outlive this borrow for the process's life.
        let fd = unsafe { BorrowedFd::borrow_raw(raw) };
        if let Ok(ws) = rustix::termios::tcgetwinsize(fd) {
            if let Some(px) = cell_px_from_winsize(ws.ws_row, ws.ws_col, ws.ws_xpixel, ws.ws_ypixel)
            {
                return px;
            }
        }
    }
    fallback
}

#[cfg(not(unix))]
fn detect_cell_px_impl(fallback: (u16, u16)) -> (u16, u16) {
    fallback
}
