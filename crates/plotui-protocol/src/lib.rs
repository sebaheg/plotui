//! plotui-protocol — turn an RGBA [`Framebuffer`] into terminal bytes.
//!
//! Pure: framebuffer + region size in → an escape-sequence `String` out. It
//! never writes to a terminal; the frontend decides where/when to emit it.
//!
//! All encoders speak the Kitty graphics protocol (also spoken by Ghostty,
//! iTerm2, and WezTerm): zlib-compressed RGBA, so it stays small over SSH.
//! plotui only draws real pixels — there is deliberately no low-fi text
//! fallback; hosts show a "supported terminals" notice instead.
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

/// A Kitty image placed via per-cell Unicode placeholders. Unlike
/// [`KittyPlaceholder`] — whose rows only address their first cell and rely on
/// contiguity for the rest — every cell here carries its own row/column
/// diacritics, so a frontend can splice foreign text into a row (label
/// overlays) without breaking the addressing of the cells after the gap.
pub struct KittyPlaceholderCells {
    /// The image-upload escape; identical contract to [`KittyPlaceholder`].
    pub transmit: String,
    /// Foreground color encoding the image id; apply to every placeholder cell.
    pub id_rgb: (u8, u8, u8),
    /// `cells[y][x]` is the self-addressed placeholder string for cell (y, x).
    pub cells: Vec<Vec<String>>,
}

/// The virtual-placement upload escape shared by both placeholder encoders:
/// U=1 (shown only via placeholders), o=z zlib, c/r scale the source image to
/// the cell region. Returns the escape and the id split for the placeholders.
fn placeholder_transmit(fb: &Framebuffer, cols: u16, rows: u16) -> (String, (u8, u8, u8), u8) {
    let rgba = fb.rgba();
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::fast());
    let _ = enc.write_all(&rgba);
    let compressed = enc.finish().unwrap_or_default();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&compressed);
    let (w, h) = (fb.w, fb.h);

    // Image id is carried by the placeholder foreground color (low 24 bits)
    // plus one "extra" high-byte diacritic.
    let [extra, id_r, id_g, id_b] = IMAGE_ID.to_be_bytes();

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
    (transmit, (id_r, id_g, id_b), extra)
}

/// Encode `fb` as a Kitty image + placeholder cells for a `cols`×`rows` region.
pub fn kitty_placeholder(fb: &Framebuffer, cols: u16, rows: u16) -> KittyPlaceholder {
    let (transmit, id_rgb, extra) = placeholder_transmit(fb, cols, rows);

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

    KittyPlaceholder { transmit, id_rgb, rows: out_rows }
}

/// Encode `fb` as a Kitty image + a grid of individually addressed placeholder
/// cells — the splice-safe variant for frontends that overlay text.
pub fn kitty_placeholder_cells(fb: &Framebuffer, cols: u16, rows: u16) -> KittyPlaceholderCells {
    let (transmit, id_rgb, extra) = placeholder_transmit(fb, cols, rows);

    // Every cell is fully addressed: row diacritic, column diacritic, and —
    // only when the image id needs a fourth byte — the "extra" diacritic.
    // (Omitting a trailing zero diacritic is per the Kitty spec.)
    let dextra = (extra != 0).then(|| DIACRITICS[extra as usize % DIACRITICS.len()]);
    let mut cells = Vec::with_capacity(rows as usize);
    for y in 0..rows as usize {
        let drow = DIACRITICS[y % DIACRITICS.len()];
        let mut row = Vec::with_capacity(cols as usize);
        for x in 0..cols as usize {
            let mut s = String::with_capacity(4 * 4);
            s.push(PLACEHOLDER);
            s.push(drow);
            s.push(DIACRITICS[x % DIACRITICS.len()]);
            if let Some(d) = dextra {
                s.push(d);
            }
            row.push(s);
        }
        cells.push(row);
    }

    KittyPlaceholderCells { transmit, id_rgb, cells }
}

/// Encode a framebuffer as a Kitty graphics escape that draws the image at the
/// current cursor position, scaled to span `cols`×`rows` cells. Cursor is saved
/// and restored, so the caller's cursor position is preserved.
pub fn kitty(fb: &Framebuffer, cols: u16, rows: u16) -> String {
    kitty_with_framing(fb, cols, rows, false)
}

/// Like [`kitty`], but repeats `q=2,i=<id>` on every continuation chunk.
///
/// The Kitty spec says continuation chunks carry only `m` (and `q`) — and
/// that is exactly what iTerm2 fails to parse: it loses the association and
/// silently drops the whole image (empirically: spec framing → nothing drawn,
/// id-on-every-chunk → renders). Use this for terminals in the "direct" tier
/// (iTerm2/WezTerm/Konsole); keep [`kitty`] for Kitty/Ghostty.
pub fn kitty_compat(fb: &Framebuffer, cols: u16, rows: u16) -> String {
    kitty_with_framing(fb, cols, rows, true)
}

fn kitty_with_framing(fb: &Framebuffer, cols: u16, rows: u16, id_every_chunk: bool) -> String {
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
    // Every a=T creates a NEW placement — without this delete, each redraw
    // stacks another copy on screen (and they outlive the app). Deleting our
    // id first makes a frame REPLACE the previous one; inside a synchronized
    // update the swap is invisible.
    let _ = write!(out, "\x1b_Ga=d,d=i,i={IMAGE_ID},q=2\x1b\\");

    const CHUNK: usize = 4096;
    let bytes = b64.as_bytes();
    let chunks: Vec<&[u8]> = bytes.chunks(CHUNK).collect();
    let n = chunks.len();
    for (i, chunk) in chunks.iter().enumerate() {
        out.push_str("\x1b_G");
        if i == 0 {
            // a=T transmit+display, f=32 RGBA, o=z zlib, s/v source px size,
            // c/r target cell span (scales the image to fill the region).
            // q=2 suppresses responses. The fixed image id (freshly deleted
            // above) plus fixed placement id p=1 keep placements from ever
            // accumulating, whichever mechanism the terminal honors.
            // z=-1 draws the image below text glyphs (but above colored
            // backgrounds, per the Kitty spec), so hosts can print labels
            // over the plot — the direct-tier stand-in for the placeholder
            // path's text splicing.
            let _ = write!(
                out,
                "q=2,i={IMAGE_ID},p=1,a=T,f=32,o=z,z=-1,s={w},v={h},c={cols},r={rows},"
            );
        } else if id_every_chunk {
            let _ = write!(out, "q=2,i={IMAGE_ID},");
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
