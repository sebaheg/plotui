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

mod diacritics;
use diacritics::DIACRITICS;

const IMAGE_ID: u32 = 4242;
const PLACEHOLDER: char = '\u{10EEEE}';

/// A Kitty image placed via Unicode placeholders, ready to composite inside a
/// bounded TUI widget (the flicker-free, compositor-friendly path).
pub struct KittyPlaceholder {
    /// The image-upload escape. Zero visible width; emit it once per frame
    /// (e.g. at the start of the first placeholder row). Reusing a fixed image
    /// id means the terminal atomically replaces the previous frame.
    pub transmit: String,
    /// Foreground color that encodes the image id — apply it to the
    /// placeholder cells so the terminal knows which image they show.
    pub id_rgb: (u8, u8, u8),
    /// One string of placeholder characters per row (each already carries the
    /// row/column diacritics). There are `rows` of them, each `cols` cells wide.
    pub rows: Vec<String>,
}

/// Encode `fb` as a Kitty image + placeholder cells for a `cols`×`rows` region.
pub fn kitty_placeholder(fb: &Framebuffer, cols: u16, rows: u16) -> KittyPlaceholder {
    let rgba = fb.rgba();
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::fast());
    let _ = enc.write_all(&rgba);
    let compressed = enc.finish().unwrap_or_default();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&compressed);
    let (w, h) = (fb.w, fb.h);

    // Image id is carried by the placeholder foreground color (low 24 bits) plus
    // one "extra" high-byte diacritic.
    let [extra, id_r, id_g, id_b] = IMAGE_ID.to_be_bytes();

    // Upload escape: U=1 = virtual placement (shown only via placeholders),
    // o=z zlib, c/r scale the source image to the cell region.
    let mut transmit = String::with_capacity(b64.len() + 128);
    const CHUNK: usize = 4096;
    let chunks: Vec<&[u8]> = b64.as_bytes().chunks(CHUNK).collect();
    let n = chunks.len().max(1);
    for (i, chunk) in chunks.iter().enumerate() {
        transmit.push_str("\x1b_G");
        if i == 0 {
            let _ = write!(
                transmit,
                "q=2,i={IMAGE_ID},a=T,U=1,f=32,o=z,t=d,s={w},v={h},c={cols},r={rows},"
            );
        }
        let more = if i + 1 < n { 1 } else { 0 };
        let _ = write!(transmit, "m={more};");
        transmit.push_str(std::str::from_utf8(chunk).unwrap_or(""));
        transmit.push_str("\x1b\\");
    }

    // Placeholder rows. The first cell of each row carries (row, col=0, extra);
    // the rest inherit position by contiguity.
    let dcol0 = DIACRITICS[0];
    let dextra = DIACRITICS[extra as usize % DIACRITICS.len()];
    let ncols = cols as usize;
    let mut out_rows = Vec::with_capacity(rows as usize);
    for y in 0..rows as usize {
        let drow = DIACRITICS[y % DIACRITICS.len()];
        let mut s = String::with_capacity(ncols * 4 + 4);
        s.push(PLACEHOLDER);
        s.push(drow);
        s.push(dcol0);
        s.push(dextra);
        for _ in 1..ncols {
            s.push(PLACEHOLDER);
        }
        out_rows.push(s);
    }

    KittyPlaceholder { transmit, id_rgb: (id_r, id_g, id_b), rows: out_rows }
}

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
