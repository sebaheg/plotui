//! Text for the rasterizer: Martian Mono outlines, filled by a small scanline
//! renderer.
//!
//! The engine used to carry a 5×7 GLCD bitmap font and a Hershey stroke font.
//! Both were legible and both looked like the decade they came from — a plot
//! is mostly numbers, so the numerals *are* the product's typography, and
//! pixel-grid digits made every chart read as a 1980s instrument panel.
//! Martian Mono is the face the rest of plotui is set in, so the charts now
//! match the docs and the site instead of arguing with them.
//!
//! Two things make the swap cheap. The face is monospace with a 700/1000
//! advance against an 800 cap, which is within 2% of the old 6×7 cell — so
//! every caller's `text_width` and margin arithmetic keeps working, and the
//! only thing that changes is the shape of the marks. And glyphs are filled
//! from quadratic outlines at whatever size is asked for, so the same data
//! serves a 7px tick label and a 24px legend row without a second font.
//!
//! Coverage decides how a glyph lands:
//!
//! * [`draw_text`] thresholds it. Tick labels sit on the *plot*, which is
//!   transparent — the terminal shows through — and this framebuffer has no
//!   alpha, so a soft edge would have to be blended against a background that
//!   does not exist. Hard pixels it is.
//! * [`draw_text_aa`] blends against a caller-supplied background. The legend
//!   and the crosshair readout paint an opaque panel first and then their
//!   text over it, so there the background is real and the edges can be soft.

use crate::glyphs::{self, ADV, CAP};
use crate::{Framebuffer, Rgb};

/// Horizontal advance per character in pixels, at scale 1.
///
/// Kept at the old bitmap cell's 6px so every margin solve and label
/// measurement in the layout is unaffected by the change of face.
pub const CHAR_W: i32 = 6;
/// Line height in pixels, at scale 1. Cap height is `800/700 · CHAR_W`, a
/// touch under this, which leaves the cell's last row as the gap that used to
/// be built into the bitmap.
pub const CHAR_H: i32 = 7;

/// The 5×7 bitmap that used to be the only font here, kept for one job.
///
/// At `scale == 1` a glyph gets a 6px advance and a ~7px cap, and hard-edged
/// outlines at that size are mush — there is no room for a stem to be one
/// pixel and a counter to be another. This face was *drawn* on that grid, so
/// it stays sharp where the outlines cannot. Above scale 1, and anywhere a
/// background is known and the marks can be antialiased, Martian Mono wins
/// and this is unused.
///
/// Printable ASCII 0x20..=0x7E, column-major, bit 0 = top row.
#[rustfmt::skip]
const FONT: [[u8; 5]; 95] = [
    [0x00, 0x00, 0x00, 0x00, 0x00], // ' '
    [0x00, 0x00, 0x5F, 0x00, 0x00], // '!'
    [0x00, 0x07, 0x00, 0x07, 0x00], // '"'
    [0x14, 0x7F, 0x14, 0x7F, 0x14], // '#'
    [0x24, 0x2A, 0x7F, 0x2A, 0x12], // '$'
    [0x23, 0x13, 0x08, 0x64, 0x62], // '%'
    [0x36, 0x49, 0x55, 0x22, 0x50], // '&'
    [0x00, 0x05, 0x03, 0x00, 0x00], // '\''
    [0x00, 0x1C, 0x22, 0x41, 0x00], // '('
    [0x00, 0x41, 0x22, 0x1C, 0x00], // ')'
    [0x14, 0x08, 0x3E, 0x08, 0x14], // '*'
    [0x08, 0x08, 0x3E, 0x08, 0x08], // '+'
    [0x00, 0x50, 0x30, 0x00, 0x00], // ','
    [0x08, 0x08, 0x08, 0x08, 0x08], // '-'
    [0x00, 0x60, 0x60, 0x00, 0x00], // '.'
    [0x20, 0x10, 0x08, 0x04, 0x02], // '/'
    [0x3E, 0x51, 0x49, 0x45, 0x3E], // '0'
    [0x00, 0x42, 0x7F, 0x40, 0x00], // '1'
    [0x42, 0x61, 0x51, 0x49, 0x46], // '2'
    [0x21, 0x41, 0x45, 0x4B, 0x31], // '3'
    [0x18, 0x14, 0x12, 0x7F, 0x10], // '4'
    [0x27, 0x45, 0x45, 0x45, 0x39], // '5'
    [0x3C, 0x4A, 0x49, 0x49, 0x30], // '6'
    [0x01, 0x71, 0x09, 0x05, 0x03], // '7'
    [0x36, 0x49, 0x49, 0x49, 0x36], // '8'
    [0x06, 0x49, 0x49, 0x29, 0x1E], // '9'
    [0x00, 0x36, 0x36, 0x00, 0x00], // ':'
    [0x00, 0x56, 0x36, 0x00, 0x00], // ';'
    [0x08, 0x14, 0x22, 0x41, 0x00], // '<'
    [0x14, 0x14, 0x14, 0x14, 0x14], // '='
    [0x00, 0x41, 0x22, 0x14, 0x08], // '>'
    [0x02, 0x01, 0x51, 0x09, 0x06], // '?'
    [0x32, 0x49, 0x79, 0x41, 0x3E], // '@'
    [0x7E, 0x11, 0x11, 0x11, 0x7E], // 'A'
    [0x7F, 0x49, 0x49, 0x49, 0x36], // 'B'
    [0x3E, 0x41, 0x41, 0x41, 0x22], // 'C'
    [0x7F, 0x41, 0x41, 0x22, 0x1C], // 'D'
    [0x7F, 0x49, 0x49, 0x49, 0x41], // 'E'
    [0x7F, 0x09, 0x09, 0x09, 0x01], // 'F'
    [0x3E, 0x41, 0x49, 0x49, 0x7A], // 'G'
    [0x7F, 0x08, 0x08, 0x08, 0x7F], // 'H'
    [0x00, 0x41, 0x7F, 0x41, 0x00], // 'I'
    [0x20, 0x40, 0x41, 0x3F, 0x01], // 'J'
    [0x7F, 0x08, 0x14, 0x22, 0x41], // 'K'
    [0x7F, 0x40, 0x40, 0x40, 0x40], // 'L'
    [0x7F, 0x02, 0x0C, 0x02, 0x7F], // 'M'
    [0x7F, 0x04, 0x08, 0x10, 0x7F], // 'N'
    [0x3E, 0x41, 0x41, 0x41, 0x3E], // 'O'
    [0x7F, 0x09, 0x09, 0x09, 0x06], // 'P'
    [0x3E, 0x41, 0x51, 0x21, 0x5E], // 'Q'
    [0x7F, 0x09, 0x19, 0x29, 0x46], // 'R'
    [0x46, 0x49, 0x49, 0x49, 0x31], // 'S'
    [0x01, 0x01, 0x7F, 0x01, 0x01], // 'T'
    [0x3F, 0x40, 0x40, 0x40, 0x3F], // 'U'
    [0x1F, 0x20, 0x40, 0x20, 0x1F], // 'V'
    [0x3F, 0x40, 0x38, 0x40, 0x3F], // 'W'
    [0x63, 0x14, 0x08, 0x14, 0x63], // 'X'
    [0x07, 0x08, 0x70, 0x08, 0x07], // 'Y'
    [0x61, 0x51, 0x49, 0x45, 0x43], // 'Z'
    [0x00, 0x7F, 0x41, 0x41, 0x00], // '['
    [0x02, 0x04, 0x08, 0x10, 0x20], // '\\'
    [0x00, 0x41, 0x41, 0x7F, 0x00], // ']'
    [0x04, 0x02, 0x01, 0x02, 0x04], // '^'
    [0x40, 0x40, 0x40, 0x40, 0x40], // '_'
    [0x00, 0x01, 0x02, 0x04, 0x00], // '`'
    [0x20, 0x54, 0x54, 0x54, 0x78], // 'a'
    [0x7F, 0x48, 0x44, 0x44, 0x38], // 'b'
    [0x38, 0x44, 0x44, 0x44, 0x20], // 'c'
    [0x38, 0x44, 0x44, 0x48, 0x7F], // 'd'
    [0x38, 0x54, 0x54, 0x54, 0x18], // 'e'
    [0x08, 0x7E, 0x09, 0x01, 0x02], // 'f'
    [0x0C, 0x52, 0x52, 0x52, 0x3E], // 'g'
    [0x7F, 0x08, 0x04, 0x04, 0x78], // 'h'
    [0x00, 0x44, 0x7D, 0x40, 0x00], // 'i'
    [0x20, 0x40, 0x44, 0x3D, 0x00], // 'j'
    [0x7F, 0x10, 0x28, 0x44, 0x00], // 'k'
    [0x00, 0x41, 0x7F, 0x40, 0x00], // 'l'
    [0x7C, 0x04, 0x18, 0x04, 0x78], // 'm'
    [0x7C, 0x08, 0x04, 0x04, 0x78], // 'n'
    [0x38, 0x44, 0x44, 0x44, 0x38], // 'o'
    [0x7C, 0x14, 0x14, 0x14, 0x08], // 'p'
    [0x08, 0x14, 0x14, 0x18, 0x7C], // 'q'
    [0x7C, 0x08, 0x04, 0x04, 0x08], // 'r'
    [0x48, 0x54, 0x54, 0x54, 0x20], // 's'
    [0x04, 0x3F, 0x44, 0x40, 0x20], // 't'
    [0x3C, 0x40, 0x40, 0x20, 0x7C], // 'u'
    [0x1C, 0x20, 0x40, 0x20, 0x1C], // 'v'
    [0x3C, 0x40, 0x30, 0x40, 0x3C], // 'w'
    [0x44, 0x28, 0x10, 0x28, 0x44], // 'x'
    [0x0C, 0x50, 0x50, 0x50, 0x3C], // 'y'
    [0x44, 0x64, 0x54, 0x4C, 0x44], // 'z'
    [0x00, 0x08, 0x36, 0x41, 0x00], // '{'
    [0x00, 0x00, 0x7F, 0x00, 0x00], // '|'
    [0x00, 0x41, 0x36, 0x08, 0x00], // '}'
    [0x08, 0x04, 0x08, 0x10, 0x08], // '~'
];

/// Sub-scanlines per pixel row. Four is the knee: it resolves the horizontal
/// edges that dominate digits, and past it the cost rises faster than the
/// look improves at these sizes.
const SUB: usize = 4;

/// Where `ch` lives in [`glyphs::GLYPHS`], or `None` if the face has no glyph
/// for it. Unknown characters advance without drawing, so a stray symbol
/// leaves a gap rather than shifting the rest of the line.
fn glyph_index(ch: char) -> Option<usize> {
    if (' '..='~').contains(&ch) {
        return Some(ch as usize - 0x20);
    }
    let ascii = '~' as usize - 0x20 + 1;
    glyphs::EXTRAS.iter().position(|c| *c == ch).map(|i| ascii + i)
}

/// Total advance of `text` at integer `scale`.
pub fn text_width(text: &str, scale: i32) -> i32 {
    text.chars().count() as i32 * CHAR_W * scale.max(1)
}

/// Total advance of `text` at a given cap height in pixels.
pub fn text_width_at(text: &str, cap_height: f32) -> i32 {
    let adv = ADV * (cap_height / CAP);
    (text.chars().count() as f32 * adv).round() as i32
}

/// One edge of a flattened outline, in pixel space.
struct Edge {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

/// Flatten one glyph's contours into `out`, positioned with its origin (the
/// baseline at the left sidebearing) at `(ox, oy)` and scaled by `upx` pixels
/// per font unit. Font y runs up and the framebuffer's runs down, hence the
/// sign flip.
///
/// With `rot`, the glyph turns a quarter turn counter-clockwise about that
/// origin: the advance walks up the frame and glyph tops face left. This is
/// the only place the transform lives — [`fill`] scanlines whatever polygon
/// it is handed, so rotated text costs no second rasterizer.
fn flatten(gi: usize, ox: f32, oy: f32, upx: f32, rot: bool, out: &mut Vec<Edge>) {
    let (first, first_end, n_ends) = glyphs::GLYPHS[gi];
    let (first, first_end, n_ends) = (first as usize, first_end as usize, n_ends as usize);
    let px = |p: (i16, i16, u8)| {
        let (dx, dy) = (p.0 as f32 * upx, p.1 as f32 * upx);
        if rot {
            (ox - dy, oy - dx)
        } else {
            (ox + dx, oy - dy)
        }
    };

    let mut start = 0usize;
    for e in 0..n_ends {
        let end = glyphs::ENDS[first_end + e] as usize;
        let pts = &glyphs::POINTS[first + start..first + end];
        start = end;
        if pts.is_empty() {
            continue;
        }

        // A contour may begin off-curve; then the start point is the implied
        // midpoint to the last point (or the last point itself, if that one
        // is on-curve).
        let n = pts.len();
        let on = |i: usize| pts[i % n].2 == 1;
        let pt = |i: usize| px(pts[i % n]);
        let mid = |a: (f32, f32), b: (f32, f32)| ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5);

        let (begin, i0) = if on(0) {
            (pt(0), 1)
        } else if on(n - 1) {
            (pt(n - 1), 0)
        } else {
            (mid(pt(n - 1), pt(0)), 0)
        };

        let mut cur = begin;
        let mut i = i0;
        let mut ctrl: Option<(f32, f32)> = None;
        let steps = i0 + n;
        while i < steps {
            let p = pt(i);
            if on(i) {
                match ctrl.take() {
                    Some(c) => quad(cur, c, p, out),
                    None => out.push(Edge { x0: cur.0, y0: cur.1, x1: p.0, y1: p.1 }),
                }
                cur = p;
            } else if let Some(c) = ctrl.replace(p) {
                // Two off-curve points in a row: the on-curve point between
                // them is implied at their midpoint.
                let m = mid(c, p);
                quad(cur, c, m, out);
                cur = m;
            }
            i += 1;
        }
        // Close the contour back onto its start.
        match ctrl {
            Some(c) => quad(cur, c, begin, out),
            None => out.push(Edge { x0: cur.0, y0: cur.1, x1: begin.0, y1: begin.1 }),
        }
    }
}

/// Flatten one quadratic into line segments. The step count comes from the
/// control polygon's pixel length, so a curve costs what its size on screen
/// warrants — a few segments in a 7px digit, more in a 24px legend row.
fn quad(a: (f32, f32), c: (f32, f32), b: (f32, f32), out: &mut Vec<Edge>) {
    let d = (c.0 - a.0).abs() + (c.1 - a.1).abs() + (b.0 - c.0).abs() + (b.1 - c.1).abs();
    let n = ((d * 0.4).sqrt().ceil() as usize).clamp(1, 16);
    let mut prev = a;
    for k in 1..=n {
        let t = k as f32 / n as f32;
        let u = 1.0 - t;
        let p = (
            u * u * a.0 + 2.0 * u * t * c.0 + t * t * b.0,
            u * u * a.1 + 2.0 * u * t * c.1 + t * t * b.1,
        );
        out.push(Edge { x0: prev.0, y0: prev.1, x1: p.0, y1: p.1 });
        prev = p;
    }
}

/// Scanline-fill `edges` with the nonzero winding rule, handing each covered
/// pixel's coverage in `0..=1` to `paint`.
fn fill(edges: &[Edge], mut paint: impl FnMut(i32, i32, f32)) {
    if edges.is_empty() {
        return;
    }
    let (mut ylo, mut yhi) = (f32::INFINITY, f32::NEG_INFINITY);
    let (mut xlo, mut xhi) = (f32::INFINITY, f32::NEG_INFINITY);
    for e in edges {
        ylo = ylo.min(e.y0).min(e.y1);
        yhi = yhi.max(e.y0).max(e.y1);
        xlo = xlo.min(e.x0).min(e.x1);
        xhi = xhi.max(e.x0).max(e.x1);
    }
    if !ylo.is_finite() || !xlo.is_finite() {
        return;
    }
    let y0 = ylo.floor() as i32;
    let y1 = yhi.ceil() as i32;
    let x0 = xlo.floor() as i32;
    let x1 = xhi.ceil() as i32;
    let w = (x1 - x0 + 1).max(1) as usize;

    let mut cov = vec![0.0f32; w];
    let mut xs: Vec<(f32, i32)> = Vec::new();
    for y in y0..=y1 {
        cov.iter_mut().for_each(|c| *c = 0.0);
        let mut any = false;
        for s in 0..SUB {
            let sy = y as f32 + (s as f32 + 0.5) / SUB as f32;
            xs.clear();
            for e in edges {
                let (ea, eb) = (e.y0, e.y1);
                // Half-open in y so a vertex shared by two edges is counted
                // once; horizontal edges never cross a sub-scanline.
                let (top, bot, dir) = if ea < eb { (ea, eb, 1) } else { (eb, ea, -1) };
                if sy < top || sy >= bot {
                    continue;
                }
                let t = (sy - e.y0) / (e.y1 - e.y0);
                xs.push((e.x0 + t * (e.x1 - e.x0), dir));
            }
            if xs.len() < 2 {
                continue;
            }
            xs.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut wind = 0;
            for i in 0..xs.len() - 1 {
                wind += xs[i].1;
                if wind == 0 {
                    continue;
                }
                let (sa, sb) = (xs[i].0, xs[i + 1].0);
                if sb <= sa {
                    continue;
                }
                any = true;
                // Accumulate the span's horizontal overlap per pixel, so the
                // left and right ends land as fractions rather than steps.
                let ia = (sa.floor() as i32).max(x0);
                let ib = (sb.ceil() as i32).min(x1);
                for px in ia..ib {
                    let l = sa.max(px as f32);
                    let r = sb.min(px as f32 + 1.0);
                    if r > l {
                        cov[(px - x0) as usize] += (r - l) / SUB as f32;
                    }
                }
            }
        }
        if !any {
            continue;
        }
        for (i, c) in cov.iter().enumerate() {
            if *c > 0.002 {
                paint(x0 + i as i32, y, c.min(1.0));
            }
        }
    }
}

/// Draw `text` with its cell's top-left at `(x, y)`, at a cap height of
/// `cap_height` pixels. `bg` is the background the glyphs are blended against
/// when `aa`; without it coverage is thresholded and the marks are hard.
#[allow(clippy::too_many_arguments)]
fn draw(
    fb: &mut Framebuffer,
    x: i32,
    y: i32,
    text: &str,
    cap_height: f32,
    z: f32,
    color: Rgb,
    bg: Option<Rgb>,
    rot: bool,
) {
    let upx = cap_height / CAP;
    let adv = ADV * upx;
    // Upright, `y` is the top of the line box and the baseline sits one cap
    // below it. Rotated, `(x, y)` is the box's *bottom*-left and the baseline
    // runs up its left edge, one cap in — so a caller reserves the same two
    // numbers either way, swapped.
    let (bx, by) =
        if rot { (x as f32 + cap_height, y as f32) } else { (x as f32, y as f32 + cap_height) };
    let mut edges: Vec<Edge> = Vec::new();
    for (i, ch) in text.chars().enumerate() {
        let Some(gi) = glyph_index(ch) else { continue };
        edges.clear();
        let (ox, oy) = if rot { (bx, by - i as f32 * adv) } else { (bx + i as f32 * adv, by) };
        flatten(gi, ox, oy, upx, rot, &mut edges);
        fill(&edges, |px, py, c| match bg {
            Some(bg) => {
                let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * c).round() as u8;
                let blended = [mix(bg[0], color[0]), mix(bg[1], color[1]), mix(bg[2], color[2])];
                if c > 0.02 {
                    fb.put_px(px, py, z, blended);
                }
            }
            None => {
                if c >= 0.5 {
                    fb.put_px(px, py, z, color);
                }
            }
        });
    }
}

/// Draw `text` with its top-left at `(x, y)` at integer `scale`, hard-edged.
/// For labels on the plot itself, where there is no background to blend into.
pub fn draw_text(fb: &mut Framebuffer, x: i32, y: i32, text: &str, scale: i32, z: f32, color: Rgb) {
    let s = scale.max(1);
    if s == 1 {
        draw_bitmap(fb, x, y, text, z, color);
        return;
    }
    draw(fb, x, y, text, CAP * (CHAR_W * s) as f32 / ADV, z, color, None, false);
}

/// The scale-1 path: one bitmap column per pixel, no scaling and no coverage.
///
/// The bitmap only ever covered ASCII. Anything outside it — the handful of
/// typographic marks the engine emits — falls through to the outline face for
/// that glyph alone, so coverage is the same at every scale even though the
/// face is not.
fn draw_bitmap(fb: &mut Framebuffer, x: i32, y: i32, text: &str, z: f32, color: Rgb) {
    for (i, ch) in text.chars().enumerate() {
        let cx = x + i as i32 * CHAR_W;
        let Some(g) = (' '..='~').contains(&ch).then(|| FONT[ch as usize - 0x20]) else {
            draw(fb, cx, y, &ch.to_string(), CAP * CHAR_W as f32 / ADV, z, color, None, false);
            continue;
        };
        for (col, bits) in g.iter().enumerate() {
            for row in 0..7 {
                if bits & (1 << row) != 0 {
                    fb.put_px(cx + col as i32, y + row, z, color);
                }
            }
        }
    }
}

/// [`draw_text`] turned a quarter turn counter-clockwise: the text reads
/// bottom to top with its tops facing left, and `(x, y)` is the *bottom*-left
/// corner of the rotated line box. The box is `CHAR_H * scale` wide and
/// [`text_width`] tall — the upright numbers, swapped — which is what a
/// y-axis title needs to reserve in the left margin.
pub fn draw_text_rot90(
    fb: &mut Framebuffer,
    x: i32,
    y: i32,
    text: &str,
    scale: i32,
    z: f32,
    color: Rgb,
) {
    let s = scale.max(1);
    if s == 1 {
        draw_bitmap_rot90(fb, x, y, text, z, color);
        return;
    }
    draw(fb, x, y, text, CAP * (CHAR_W * s) as f32 / ADV, z, color, None, true);
}

/// The scale-1 rotated path — [`draw_bitmap`]'s transpose. Rotating the
/// bitmap keeps small frames' titles as crisp as their tick labels; going
/// through the outline face at this size would blur them for no gain.
fn draw_bitmap_rot90(fb: &mut Framebuffer, x: i32, y: i32, text: &str, z: f32, color: Rgb) {
    for (i, ch) in text.chars().enumerate() {
        // Distance along the (upward) advance, before the rotation.
        let adv = i as i32 * CHAR_W;
        let Some(g) = (' '..='~').contains(&ch).then(|| FONT[ch as usize - 0x20]) else {
            draw(fb, x, y - adv, &ch.to_string(), CAP * CHAR_W as f32 / ADV, z, color, None, true);
            continue;
        };
        for (col, bits) in g.iter().enumerate() {
            for row in 0..7 {
                if bits & (1 << row) != 0 {
                    fb.put_px(x + row, y - (adv + col as i32), z, color);
                }
            }
        }
    }
}

/// [`draw_text`] blended against a known background — for text over a panel
/// the caller has already painted. Unlike [`draw_text`] this uses the real
/// face at every scale: with a background to blend into, coverage carries the
/// shape that thresholding at scale 1 would destroy.
#[allow(clippy::too_many_arguments)]
pub fn draw_text_aa(
    fb: &mut Framebuffer,
    x: i32,
    y: i32,
    text: &str,
    scale: i32,
    z: f32,
    color: Rgb,
    bg: Rgb,
) {
    let s = scale.max(1);
    draw(fb, x, y, text, CAP * (CHAR_W * s) as f32 / ADV, z, color, Some(bg), false);
}

/// [`draw_text_aa`] sized by cap height rather than by an integer scale, for
/// the legend and readout panels that size their rows to the text.
#[allow(clippy::too_many_arguments)]
pub fn draw_text_at(
    fb: &mut Framebuffer,
    x: i32,
    y: i32,
    text: &str,
    cap_height: f32,
    z: f32,
    color: Rgb,
    bg: Rgb,
) {
    draw(fb, x, y, text, cap_height.max(1.0), z, color, Some(bg), false);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(fb: &Framebuffer) -> usize {
        fb.rgba().chunks(4).filter(|px| px[3] != 0).count()
    }

    fn render(text: &str, scale: i32) -> Framebuffer {
        let mut fb = Framebuffer::new(40 + text.chars().count() * 6 * scale.max(1) as usize, 60);
        draw_text(&mut fb, 4, 4, text, scale, 0.0, [255, 255, 255]);
        fb
    }

    /// The whole reason the swap was cheap: callers measure text with
    /// `text_width`, and the new face keeps the old cell exactly. If this
    /// drifts, every margin solve in the layout drifts with it.
    #[test]
    fn the_advance_still_matches_the_old_cell() {
        for scale in 1..=4 {
            assert_eq!(text_width("12345", scale), 5 * CHAR_W * scale);
        }
        // The cap-height API agrees with the integer one at the same size.
        let cap = CAP * (CHAR_W * 3) as f32 / ADV;
        assert_eq!(text_width_at("12345", cap), text_width("12345", 3));
    }

    /// Eight glyphs in this face are composites — `-` among them — and the
    /// first cut of the generated table silently dropped every one, so minus
    /// signs vanished from every negative axis label. Nothing in the layout
    /// notices a glyph that draws nothing; only this does.
    #[test]
    fn every_covered_character_draws_something() {
        let printable = (0x20u8..=0x7E).map(|c| c as char);
        for ch in printable.chain(glyphs::EXTRAS) {
            if ch == ' ' {
                continue;
            }
            for scale in [1, 2, 3] {
                let fb = render(&ch.to_string(), scale);
                assert!(drawn(&fb) > 0, "{ch:?} drew nothing at scale {scale}");
            }
        }
    }

    #[test]
    fn glyphs_stay_inside_their_cell() {
        // A run of tall and deep glyphs must not bleed left of the origin or
        // past the last advance; descenders may dip below the line box.
        let text = "Ag|jQ";
        let scale = 3;
        let fb = render(text, scale);
        let w = text_width(text, scale);
        for (i, px) in fb.rgba().chunks(4).enumerate() {
            if px[3] == 0 {
                continue;
            }
            let x = (i % fb.w) as i32;
            assert!(x >= 4, "drew left of the origin at {x}");
            assert!(x < 4 + w, "drew past the advance at {x} (width {w})");
        }
    }

    #[test]
    fn unknown_characters_advance_without_drawing() {
        let mut fb = Framebuffer::new(80, 20);
        draw_text(&mut fb, 2, 2, "\u{4e2d}\u{6587}", 2, 0.0, [255, 255, 255]);
        assert_eq!(drawn(&fb), 0);
        // And they still take their slot, so the rest of a line stays put.
        let with = render("1\u{4e2d}2", 2);
        let without = render("1 2", 2);
        assert_eq!(text_width("1\u{4e2d}2", 2), text_width("1 2", 2));
        assert!(drawn(&with) > 0 && drawn(&without) > 0);
    }

    /// Scale 1 is the bitmap's job and scale 2+ is the outline's; both must
    /// draw, and the outline must actually scale rather than pixel-double.
    #[test]
    fn both_faces_render_across_scales() {
        let counts: Vec<usize> = (1..=4).map(|s| drawn(&render("0123456789", s))).collect();
        for (s, n) in counts.iter().enumerate() {
            assert!(*n > 0, "scale {} drew nothing", s + 1);
        }
        assert!(
            counts.windows(2).all(|w| w[1] > w[0]),
            "coverage must grow with scale, got {counts:?}"
        );
        // A pixel-doubled bitmap would land on exactly 4× its own coverage;
        // a resampled outline does not.
        assert_ne!(counts[1], counts[0] * 4, "scale 2 must be the outline, not a doubled bitmap");
    }

    #[test]
    fn antialiasing_blends_only_where_a_background_is_known() {
        let mut aa = Framebuffer::new(120, 30);
        aa.rect_fill(0, 0, 119, 29, 0.5, [0, 0, 0]);
        draw_text_aa(&mut aa, 4, 4, "48", 3, 0.0, [255, 255, 255], [0, 0, 0]);
        let mut full = 0;
        let mut partial = 0;
        for px in aa.rgba().chunks(4) {
            match px[0] {
                255 => full += 1,
                1..=254 => partial += 1,
                _ => {}
            }
        }
        assert!(full > 0, "glyph cores stay solid");
        assert!(partial > 0, "edges carry blended pixels");

        // The hard path has no background to blend into, so it must stay
        // strictly two-valued.
        let hard = render("48", 3);
        assert!(
            hard.rgba().chunks(4).all(|px| px[3] == 0 || px[0] == 255),
            "the plot-side path must not emit half-lit pixels"
        );
    }
}
