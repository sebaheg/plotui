//! plotui-protocol — turn an RGBA [`Framebuffer`] into terminal bytes.
//!
//! Pure: framebuffer + region size in → an escape-sequence `String` out. It
//! never writes to a terminal; the frontend decides where/when to emit it.
//!
//! Two encoders:
//! - [`kitty`]: the Kitty graphics protocol (also spoken by Ghostty/WezTerm).
//!   zlib-compressed RGBA, so it stays small enough for SSH.
//! - [`halfblock`]: a universal fallback using the `▀` half-block glyph — works
//!   in any truecolor terminal, no image protocol required.
//!
//! NOTE: this first version uses Kitty's *direct* placement (image drawn at the
//! cursor, sized to `cols`×`rows`). The flicker-free path for tight TUI
//! compositing is Kitty's Unicode-placeholder placement (virtual placement with
//! a fixed image id); that's the next iteration for the Textual widget.

use base64::Engine;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use plotui_core::Framebuffer;
use std::fmt::Write as _;
use std::io::Write as _;

const IMAGE_ID: u32 = 4242;

/// Encode a framebuffer as a Kitty graphics escape that draws the image at the
/// current cursor position, scaled to span `cols`×`rows` cells. Cursor is saved
/// and restored, so the caller's cursor position is preserved.
pub fn kitty(fb: &Framebuffer, cols: u16, rows: u16) -> String {
    let rgba = fb.rgba();

    let mut enc = ZlibEncoder::new(Vec::new(), Compression::fast());
    if enc.write_all(&rgba).is_err() {
        return String::new();
    }
    let compressed = match enc.finish() {
        Ok(b) => b,
        Err(_) => return String::new(),
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(&compressed);

    let w = fb.w;
    let h = fb.h;
    let mut out = String::with_capacity(b64.len() + 256);
    // Save cursor so placement doesn't move the caller's cursor.
    out.push_str("\x1b[s");

    const CHUNK: usize = 4096;
    let bytes = b64.as_bytes();
    let chunks: Vec<&[u8]> = bytes.chunks(CHUNK).collect();
    let n = chunks.len();
    for (i, chunk) in chunks.iter().enumerate() {
        out.push_str("\x1b_G");
        if i == 0 {
            // a=T transmit+display, f=32 RGBA, o=z zlib, s/v source px size,
            // c/r target cell span (scales the image to fill the region).
            // q=2 suppresses responses. Reusing IMAGE_ID replaces the prior image.
            let _ = write!(
                out,
                "q=2,i={IMAGE_ID},a=T,f=32,o=z,s={w},v={h},c={cols},r={rows},"
            );
        }
        let more = if i + 1 < n { 1 } else { 0 };
        let _ = write!(out, "m={more};");
        out.push_str(std::str::from_utf8(chunk).unwrap());
        out.push_str("\x1b\\");
    }
    // Restore cursor.
    out.push_str("\x1b[u");
    out
}

/// Escape sequence that deletes plotui's image from the terminal. Emit on exit.
pub fn kitty_cleanup() -> String {
    format!("\x1b_Ga=d,d=i,i={IMAGE_ID}\x1b\\")
}

/// Universal fallback: render into `▀` half-block cells (top pixel = fg, bottom
/// pixel = bg), two vertical pixels per character row. Returns `rows` lines.
///
/// The frontend should size the framebuffer to `cols*1 × rows*2` pixels for a
/// 1:2 cell so this maps cleanly.
pub fn halfblock(fb: &Framebuffer) -> String {
    let rgba = fb.rgba();
    let w = fb.w;
    let h = fb.h;
    let px = |x: usize, y: usize| -> Option<[u8; 3]> {
        if x >= w || y >= h {
            return None;
        }
        let i = (y * w + x) * 4;
        if rgba[i + 3] == 0 {
            None
        } else {
            Some([rgba[i], rgba[i + 1], rgba[i + 2]])
        }
    };
    let mut out = String::new();
    let rows = h / 2;
    for cy in 0..rows {
        for x in 0..w {
            let top = px(x, cy * 2);
            let bot = px(x, cy * 2 + 1);
            match (top, bot) {
                (None, None) => {
                    let _ = write!(out, "\x1b[0m ");
                }
                (Some(t), None) => {
                    let _ = write!(out, "\x1b[38;2;{};{};{}m\u{2580}", t[0], t[1], t[2]);
                }
                (None, Some(b)) => {
                    let _ = write!(out, "\x1b[38;2;{};{};{}m\u{2584}", b[0], b[1], b[2]);
                }
                (Some(t), Some(b)) => {
                    let _ = write!(
                        out,
                        "\x1b[38;2;{};{};{}m\x1b[48;2;{};{};{}m\u{2580}",
                        t[0], t[1], t[2], b[0], b[1], b[2]
                    );
                }
            }
        }
        out.push_str("\x1b[0m");
        if cy + 1 < rows {
            out.push('\n');
        }
    }
    out
}
