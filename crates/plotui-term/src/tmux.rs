//! tmux passthrough wrapping for image escapes.

/// Wrap a terminal escape for tmux passthrough when running inside tmux.
///
/// tmux intercepts control sequences it doesn't model (like the Kitty
/// graphics APC), so an image drawn by direct placement never reaches the
/// outer terminal. tmux's passthrough — `\ePtmux;<payload>\e\\` with every
/// ESC in the payload doubled — hands the raw bytes to the outer terminal.
/// Requires `set -g allow-passthrough on` in tmux. A no-op outside tmux
/// (`$TMUX` unset), so normal terminals are unaffected.
pub fn tmux_wrap(escape: &str) -> String {
    tmux_wrap_with(escape, std::env::var("TMUX").is_ok_and(|v| !v.is_empty()))
}

/// [`tmux_wrap`] with the in-tmux decision injected (pure, for tests and for
/// frontends that cache the environment check).
pub fn tmux_wrap_with(escape: &str, in_tmux: bool) -> String {
    if !in_tmux {
        return escape.to_string();
    }
    let mut out = String::with_capacity(escape.len() + escape.len() / 8 + 16);
    out.push_str("\x1bPtmux;");
    for c in escape.chars() {
        if c == '\x1b' {
            out.push('\x1b');
        }
        out.push(c);
    }
    out.push_str("\x1b\\");
    out
}
