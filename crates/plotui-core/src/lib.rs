//! plotui-core — the pure rendering engine.
//!
//! It owns *no* terminal state, *no* input, and *no* loop. Its entire job is:
//!
//! ```text
//! (data + camera + pixel size) -> RGBA framebuffer
//! ```
//!
//! The frontend (Textual today; Bubble Tea / Ratatui later) owns the event
//! loop and input, mutates the [`Camera`], and asks for a frame. Keeping this
//! crate free of I/O is what lets the same engine drive every frontend and be
//! unit-tested by hashing pixel buffers.

mod font;
mod glyphs;
mod layout;
mod marching;
mod ribbon;
mod ticks;

pub use font::{
    draw_text, draw_text_aa, draw_text_at, draw_text_rot90, text_width, text_width_at, CHAR_H,
    CHAR_W,
};
pub use layout::{reachable, Direction, ForceLayout, LayeredLayout, RankDir};
pub use marching::marching_cubes;
pub use ribbon::{catmull_rom, ribbon, tube};
pub use ticks::{
    civil_from_days, date_ticks, days_from_civil, format_datetime, format_log_tick, format_tick,
    log_ticks, nice_ticks,
};

pub type Rgb = [u8; 3];

/// Marker silhouette for a graph node. Every shape is proportioned to read
/// as its own glyph at the same nominal radius `r`, so a host can assign one
/// per node category and the category stays legible without a colour key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shape {
    #[default]
    Disc,
    /// Outline circle with a small filled centre.
    Ring,
    Square,
    /// Apex up.
    Triangle,
    Diamond,
    /// Outline diamond.
    DiamondOpen,
    /// A smaller disc (0.8 r) — for the least important category.
    Dot,
}

impl Shape {
    /// The wire names, in declaration order — what frontends accept.
    pub const NAMES: [&'static str; 7] =
        ["disc", "ring", "square", "triangle", "diamond", "diamond-open", "dot"];

    pub fn parse(name: &str) -> Option<Shape> {
        Some(match name {
            "disc" => Shape::Disc,
            "ring" => Shape::Ring,
            "square" => Shape::Square,
            "triangle" => Shape::Triangle,
            "diamond" => Shape::Diamond,
            "diamond-open" => Shape::DiamondOpen,
            "dot" => Shape::Dot,
            _ => return None,
        })
    }

    /// The solid silhouette behind an open shape — what hover/selection
    /// halos are drawn as, so the halo stays one readable blob.
    fn filled(self) -> Shape {
        match self {
            Shape::Ring | Shape::Dot => Shape::Disc,
            Shape::DiamondOpen => Shape::Diamond,
            other => other,
        }
    }
}

/// The silhouette of a [`Trace::Graph2d`] node. Unlike [`Shape`] — a marker
/// drawn at a nominal radius — these are boxes sized to the label inside
/// them, which is why a graph node needs its own small enum rather than
/// reusing the scatter markers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NodeShape {
    /// A box with rounded corners: the default, and what DOT's
    /// `style=rounded` asks for.
    #[default]
    Rounded,
    /// A box with square corners (DOT `shape=box`).
    Box,
    /// An ellipse inscribed in the label box. A `circle` sized to a label is
    /// an ellipse, so DOT's `circle` and `oval` land here too.
    Ellipse,
    Diamond,
}

impl NodeShape {
    /// The wire names, in declaration order — what frontends accept.
    pub const NAMES: [&'static str; 4] = ["rounded", "box", "ellipse", "diamond"];

    /// Parse a wire (or DOT `shape=`) name, including the DOT synonyms.
    /// Unknown names return `None`: core supplies the fact, the bindings
    /// phrase the error, so the message is identical in every language.
    pub fn parse(name: &str) -> Option<NodeShape> {
        Some(match name {
            "rounded" => NodeShape::Rounded,
            "box" | "rect" | "rectangle" | "square" => NodeShape::Box,
            "ellipse" | "oval" | "circle" => NodeShape::Ellipse,
            "diamond" => NodeShape::Diamond,
            _ => return None,
        })
    }
}

/// Half a box's width in category units. Boxes sit one unit apart (group `g`
/// at position `g`), so 0.3 leaves a visible gutter between neighbours the way
/// `bar_halfwidth`'s 40% does.
const BOX_HALF_WIDTH: f64 = 0.3;

/// Split the flat sample into its groups. A `group_starts` that runs past the
/// end, or backwards, yields an empty group rather than panicking — the
/// bindings validate it, but the renderer must not be the thing that trusts
/// them.
fn box_groups<'a>(
    values: &'a [f32],
    group_starts: &'a [u32],
) -> impl Iterator<Item = &'a [f32]> + 'a {
    (0..group_starts.len()).map(move |g| {
        let a = group_starts[g] as usize;
        let b = group_starts.get(g + 1).map_or(values.len(), |v| *v as usize);
        if a <= b && b <= values.len() {
            &values[a..b]
        } else {
            &[]
        }
    })
}

/// One group's five-number summary plus its outliers — everything a box
/// needs, solved once from the sample.
#[derive(Clone, Debug, PartialEq)]
struct BoxStats {
    q1: f64,
    median: f64,
    q3: f64,
    /// Whisker ends: the most extreme values still inside Tukey's fence, not
    /// the fence itself. A whisker that stopped at the fence would claim data
    /// exists where none does.
    lo: f64,
    hi: f64,
    outliers: Vec<f64>,
}

impl BoxStats {
    /// Every value the box occupies on the value axis: the whiskers, the
    /// quartiles, and each outlier. Bounds fold all of these in, so an
    /// outlier can never fall outside the frame that is supposed to show it.
    fn spans(&self) -> impl Iterator<Item = f64> + '_ {
        [self.lo, self.q1, self.median, self.q3, self.hi]
            .into_iter()
            .chain(self.outliers.iter().copied())
    }
}

/// Tukey's rule: quartiles by linear interpolation, whiskers to the furthest
/// points within 1.5·IQR of the box, everything beyond drawn individually.
///
/// Drawing outliers as points rather than extending the whiskers to the
/// extremes is the whole reason a box plot beats a min/max range: it
/// separates the bulk of a distribution from the handful of values arguing
/// with it.
fn box_stats(values: &[f32]) -> Option<BoxStats> {
    let mut v: Vec<f64> = values.iter().map(|&x| x as f64).filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    // Linear interpolation between order statistics (the "type 7" quantile
    // every mainstream stats package defaults to).
    let q = |p: f64| -> f64 {
        let h = (v.len() - 1) as f64 * p;
        let (lo, frac) = (h.floor(), h - h.floor());
        let i = lo as usize;
        v[i] + frac * (v[(i + 1).min(v.len() - 1)] - v[i])
    };
    let (q1, median, q3) = (q(0.25), q(0.5), q(0.75));
    let fence = 1.5 * (q3 - q1);
    let (lo_fence, hi_fence) = (q1 - fence, q3 + fence);
    let lo = *v.iter().find(|x| **x >= lo_fence).unwrap_or(&v[0]);
    let hi = *v.iter().rev().find(|x| **x <= hi_fence).unwrap_or(&v[v.len() - 1]);
    let outliers = v.iter().copied().filter(|x| *x < lo_fence || *x > hi_fence).collect();
    Some(BoxStats { q1, median, q3, lo, hi, outliers })
}

/// Per-point uncertainty on one axis: `plus` above (or right of) each point,
/// `minus` below. `minus: None` mirrors `plus`, which is the symmetric case
/// almost every measurement reports.
///
/// Shorter than the series means the remaining points simply carry no bar,
/// rather than the series being truncated — the same padding rule per-point
/// styling uses.
#[derive(Clone, Debug, PartialEq)]
pub struct ErrBars {
    pub plus: Vec<f32>,
    pub minus: Option<Vec<f32>>,
}

impl ErrBars {
    /// The `(low, high)` offsets at point `i`, or `None` where this point has
    /// no bar or a non-finite one.
    fn at(&self, i: usize) -> Option<(f64, f64)> {
        let up = *self.plus.get(i)? as f64;
        let down = match &self.minus {
            Some(m) => *m.get(i)? as f64,
            None => up,
        };
        (up.is_finite() && down.is_finite()).then_some((down.abs(), up.abs()))
    }
}

/// How several bar traces on one axis share their positions.
///
/// This is a plot-level setting, not a per-trace one, because the answer is
/// inherently about the *set*: a bar cannot know it is the second of three
/// without being told, and its width and offset both depend on how many
/// others there are.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BarMode {
    /// Every trace draws at full width on its own position. Two traces on the
    /// same positions overplot — the later one wins the z tie and hides the
    /// first — which is why this is rarely what you want with more than one
    /// bar series, but it is what plotui has always done.
    #[default]
    Overlay,
    /// Traces sit side by side within each position's slot, each taking
    /// `1/n` of the width.
    Group,
    /// Traces stack, each starting where the one below it ended. Only the
    /// same-signed part accumulates, so a mix of positive and negative values
    /// grows in both directions from the baseline rather than cancelling into
    /// a misleading total.
    Stack,
}

impl BarMode {
    /// The wire names, in declaration order — what frontends accept.
    pub const NAMES: [&'static str; 3] = ["overlay", "group", "stack"];

    pub fn parse(name: &str) -> Option<BarMode> {
        Some(match name {
            "overlay" => BarMode::Overlay,
            "group" => BarMode::Group,
            "stack" => BarMode::Stack,
            _ => return None,
        })
    }
}

/// Which axis a bar grows along.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Orient {
    /// Bars stand on the x axis and rise along y — the default.
    #[default]
    Vertical,
    /// Bars sit on the y axis and run along x. The roles of the two axes swap
    /// wholesale: the categories are on y, the measured values on x, and the
    /// baseline is a vertical line at zero. Worth it for long category names,
    /// which a horizontal axis has no room to write and no way to rotate.
    Horizontal,
}

impl Orient {
    /// The wire names, in declaration order — what frontends accept.
    pub const NAMES: [&'static str; 2] = ["vertical", "horizontal"];

    pub fn parse(name: &str) -> Option<Orient> {
        Some(match name {
            "vertical" | "v" => Orient::Vertical,
            "horizontal" | "h" => Orient::Horizontal,
            _ => return None,
        })
    }

    fn is_horizontal(self) -> bool {
        matches!(self, Orient::Horizontal)
    }
}

/// How a 2D line gets from one sample to the next.
///
/// `Linear` draws the straight segment. The three step modes draw the
/// right-angle path instead, which is the honest shape for anything that
/// *holds* a value between samples — a counter, a state machine, a price
/// between ticks. Drawing those linearly invents a ramp that never happened,
/// so the mode is a correctness choice more than a style one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Interp {
    #[default]
    Linear,
    /// The new value applies from the previous sample: step up, then across.
    Pre,
    /// The old value holds until the next sample: across, then step.
    Post,
    /// The step falls halfway between the two samples.
    Mid,
}

impl Interp {
    /// The wire names, in declaration order — what frontends accept.
    pub const NAMES: [&'static str; 4] = ["linear", "pre", "post", "mid"];

    pub fn parse(name: &str) -> Option<Interp> {
        Some(match name {
            "linear" => Interp::Linear,
            "pre" => Interp::Pre,
            "post" => Interp::Post,
            "mid" => Interp::Mid,
            _ => return None,
        })
    }

    /// The corner between two samples, or `None` for a straight segment.
    /// Two corners for `Mid` (the riser sits between the samples), one for
    /// `Pre`/`Post`.
    fn corners(self, a: (f64, f64), b: (f64, f64)) -> [Option<(f64, f64)>; 2] {
        match self {
            Interp::Linear => [None, None],
            Interp::Pre => [Some((a.0, b.1)), None],
            Interp::Post => [Some((b.0, a.1)), None],
            Interp::Mid => {
                let mx = (a.0 + b.0) * 0.5;
                [Some((mx, a.1)), Some((mx, b.1))]
            }
        }
    }
}

/// Height→color ramps for surfaces. Piecewise-linear between the standard
/// anchor stops — indistinguishable from the real ramps at terminal
/// resolutions.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Colormap {
    Viridis,
    Plasma,
}

impl Colormap {
    /// The wire names, in declaration order — what frontends accept.
    pub const NAMES: [&'static str; 2] = ["viridis", "plasma"];

    pub fn parse(name: &str) -> Option<Colormap> {
        match name {
            "viridis" => Some(Colormap::Viridis),
            "plasma" => Some(Colormap::Plasma),
            _ => None,
        }
    }

    /// Sample the ramp at `t` ∈ [0, 1] (clamped).
    pub fn sample(self, t: f32) -> Rgb {
        const VIRIDIS: [Rgb; 5] =
            [[68, 1, 84], [59, 82, 139], [33, 145, 140], [94, 201, 98], [253, 231, 37]];
        const PLASMA: [Rgb; 5] =
            [[13, 8, 135], [126, 3, 168], [204, 71, 120], [248, 149, 64], [240, 249, 33]];
        let stops: &[Rgb; 5] = match self {
            Colormap::Viridis => &VIRIDIS,
            Colormap::Plasma => &PLASMA,
        };
        let t = t.clamp(0.0, 1.0) * (stops.len() - 1) as f32;
        let i = (t.floor() as usize).min(stops.len() - 2);
        let f = t - i as f32;
        let (a, b) = (stops[i], stops[i + 1]);
        [
            (a[0] as f32 + (b[0] as f32 - a[0] as f32) * f).round() as u8,
            (a[1] as f32 + (b[1] as f32 - a[1] as f32) * f).round() as u8,
            (a[2] as f32 + (b[2] as f32 - a[2] as f32) * f).round() as u8,
        ]
    }
}

/// Built-in colorways: per-trace color sequences assigned in fixed order to
/// traces added without an explicit color (`set_colorway` swaps the active
/// one). Each is tuned for dark surfaces and ordered so adjacent slots stay
/// distinguishable under color-vision deficiency.
///
/// "plotui" — the default, leading with the brand trio from every demo.
pub const COLORWAY_PLOTUI: [Rgb; 8] = [
    [230, 60, 120],  // pink
    [69, 200, 209],  // cyan
    [240, 161, 60],  // orange
    [144, 133, 233], // violet
    [25, 158, 112],  // green
    [201, 133, 0],   // gold
    [57, 135, 229],  // blue
    [230, 103, 103], // red
];

/// "muted" — desaturated, for plots that sit behind busy chrome.
pub const COLORWAY_MUTED: [Rgb; 8] = [
    [196, 110, 140], // rose
    [110, 175, 180], // teal
    [206, 162, 110], // sand
    [150, 143, 202], // lavender
    [110, 160, 125], // sage
    [172, 150, 90],  // khaki
    [105, 140, 190], // steel
    [150, 150, 160], // slate
];

/// "vivid" — saturated, for plots that must carry the screen.
pub const COLORWAY_VIVID: [Rgb; 8] = [
    [255, 30, 120],  // magenta
    [0, 220, 230],   // aqua
    [255, 170, 0],   // amber
    [165, 100, 255], // purple
    [30, 210, 90],   // green
    [240, 220, 0],   // yellow
    [0, 140, 255],   // azure
    [255, 70, 60],   // red
];

/// Default per-trace colors — the "plotui" colorway.
pub const PALETTE: [Rgb; 8] = COLORWAY_PLOTUI;

/// A built-in colorway by name, or `None` for an unknown one. The valid
/// names are "plotui", "muted", and "vivid".
pub fn colorway_by_name(name: &str) -> Option<&'static [Rgb; 8]> {
    match name {
        "plotui" => Some(&COLORWAY_PLOTUI),
        "muted" => Some(&COLORWAY_MUTED),
        "vivid" => Some(&COLORWAY_VIVID),
        _ => None,
    }
}

// Chrome colors shared by the 2D and 3D paths: the frame/grid recede, ink is
// neutral (identity lives in the marks, never in the text).
const COLOR_BG: Rgb = [26, 30, 44];
const COLOR_FRAME: Rgb = [70, 78, 96];
const COLOR_GRID: Rgb = [45, 50, 66];
const COLOR_INK: Rgb = [150, 156, 170];
const COLOR_INK_BRIGHT: Rgb = [205, 210, 220];

/// The chrome palette: everything that is not data. Hosts with a theme of
/// their own (a Textual app with a dark surface) override it so the axes
/// recede into *their* background rather than ours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chrome {
    /// Legend box fill.
    pub bg: Rgb,
    /// Frame, tick marks, legend border.
    pub frame: Rgb,
    /// Grid lines.
    pub grid: Rgb,
    /// Tick labels.
    pub ink: Rgb,
    /// Legend text.
    pub ink_bright: Rgb,
}

impl Default for Chrome {
    fn default() -> Self {
        Self {
            bg: COLOR_BG,
            frame: COLOR_FRAME,
            grid: COLOR_GRID,
            ink: COLOR_INK,
            ink_bright: COLOR_INK_BRIGHT,
        }
    }
}

/// An RGBA framebuffer with a z-buffer for correct point/line occlusion.
pub struct Framebuffer {
    pub w: usize,
    pub h: usize,
    color: Vec<Rgb>,
    depth: Vec<f32>,
    drawn: Vec<bool>,
    /// Optional inclusive clip rectangle (x0, y0, x1, y1) applied by `put`.
    clip: Option<(i32, i32, i32, i32)>,
}

impl Framebuffer {
    pub fn new(w: usize, h: usize) -> Self {
        let n = w.max(1) * h.max(1);
        Self {
            w: w.max(1),
            h: h.max(1),
            color: vec![[0, 0, 0]; n],
            depth: vec![f32::INFINITY; n],
            drawn: vec![false; n],
            clip: None,
        }
    }

    /// Restrict subsequent drawing to the inclusive rectangle (x0, y0)–(x1, y1).
    pub fn set_clip(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        self.clip = Some((x0, y0, x1, y1));
    }

    pub fn clear_clip(&mut self) {
        self.clip = None;
    }

    #[inline]
    fn put(&mut self, x: i32, y: i32, z: f32, c: Rgb) {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return;
        }
        if let Some((cx0, cy0, cx1, cy1)) = self.clip {
            if x < cx0 || x > cx1 || y < cy0 || y > cy1 {
                return;
            }
        }
        let i = y as usize * self.w + x as usize;
        if z <= self.depth[i] {
            self.depth[i] = z;
            self.color[i] = c;
            self.drawn[i] = true;
        }
    }

    /// Single-pixel write honoring bounds, clip, and the z-buffer. Public for
    /// in-crate helpers like the bitmap font.
    #[inline]
    pub(crate) fn put_px(&mut self, x: i32, y: i32, z: f32, c: Rgb) {
        self.put(x, y, z, c);
    }

    /// Mix `c` into the pixel already there with coverage `a` (0..1), honoring
    /// bounds, clip, and the z-buffer. Unlike `put`, this reads the pixel back,
    /// so it only makes sense for overlays drawn after the geometry beneath
    /// them — the rounded panel edges are what need it.
    ///
    /// Coverage is per-pixel alpha we cannot actually export: [`Self::rgba`]
    /// has one bit of alpha, drawn or not. So over an *undrawn* pixel a
    /// partial write would blend toward stale black and fringe the panel dark
    /// against a light host background; there the edge snaps to a hard one
    /// instead. Antialiasing only happens where there is real colour beneath.
    #[inline]
    pub(crate) fn blend_px(&mut self, x: i32, y: i32, z: f32, c: Rgb, a: f32) {
        if a <= 0.0 {
            return;
        }
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return;
        }
        if let Some((cx0, cy0, cx1, cy1)) = self.clip {
            if x < cx0 || x > cx1 || y < cy0 || y > cy1 {
                return;
            }
        }
        let i = y as usize * self.w + x as usize;
        if z > self.depth[i] {
            return;
        }
        if a >= 1.0 || !self.drawn[i] {
            if a < 0.5 {
                return;
            }
            self.depth[i] = z;
            self.color[i] = c;
            self.drawn[i] = true;
            return;
        }
        let under = self.color[i];
        let mix =
            |u: u8, o: u8| (u as f32 + (o as f32 - u as f32) * a).round().clamp(0.0, 255.0) as u8;
        self.depth[i] = z;
        self.color[i] = [mix(under[0], c[0]), mix(under[1], c[1]), mix(under[2], c[2])];
        self.drawn[i] = true;
    }

    /// Filled axis-aligned rectangle over the inclusive pixel range.
    pub fn rect_fill(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, z: f32, c: Rgb) {
        let (x0, x1) = (x0.min(x1), x0.max(x1));
        let (y0, y1) = (y0.min(y1), y0.max(y1));
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.put(x, y, z, c);
            }
        }
    }

    /// Filled disc — the mark used for scatter/graph nodes.
    pub fn disc(&mut self, cx: f32, cy: f32, z: f32, r: f32, c: Rgb) {
        let r = r.max(0.5);
        let (x0, x1) = ((cx - r).floor() as i32, (cx + r).ceil() as i32);
        let (y0, y1) = ((cy - r).floor() as i32, (cy + r).ceil() as i32);
        let r2 = r * r;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let dx = x as f32 + 0.5 - cx;
                let dy = y as f32 + 0.5 - cy;
                if dx * dx + dy * dy <= r2 {
                    self.put(x, y, z, c);
                }
            }
        }
    }

    /// A node marker of the given [`Shape`] at nominal radius `r`. [`disc`]
    /// is the `Shape::Disc` case; the others share its bounding-box scan with
    /// a per-shape inside test. Open shapes get a stroke that never drops
    /// below one pixel, so they survive the smallest sizes.
    ///
    /// [`disc`]: Framebuffer::disc
    pub fn mark(&mut self, cx: f32, cy: f32, z: f32, r: f32, shape: Shape, c: Rgb) {
        let r = r.max(0.5);
        let reach = r * 1.3; // the widest any shape extends
        let (x0, x1) = ((cx - reach).floor() as i32, (cx + reach).ceil() as i32);
        let (y0, y1) = ((cy - reach).floor() as i32, (cy + reach).ceil() as i32);
        let stroke = (0.3 * r).max(1.0);
        let inside = |dx: f32, dy: f32| -> bool {
            match shape {
                Shape::Disc => dx * dx + dy * dy <= r * r,
                Shape::Dot => {
                    let d = 0.8 * r;
                    dx * dx + dy * dy <= d * d
                }
                Shape::Ring => {
                    let d2 = dx * dx + dy * dy;
                    let inner = (r - stroke).max(0.0);
                    let core = 0.38 * r;
                    d2 <= r * r && (d2 >= inner * inner || d2 <= core * core)
                }
                Shape::Square => dx.abs() <= r && dy.abs() <= r,
                Shape::Diamond => dx.abs() + dy.abs() <= 1.3 * r,
                Shape::DiamondOpen => {
                    // The L1 band matching a Euclidean stroke is sqrt(2) wider.
                    let m = dx.abs() + dy.abs();
                    m <= 1.3 * r && m >= 1.3 * r - stroke * std::f32::consts::SQRT_2
                }
                Shape::Triangle => {
                    // apex at -1.2 r, base at +0.9 r with half-width 1.1 r
                    let (top, bottom) = (-1.2 * r, 0.9 * r);
                    dy >= top && dy <= bottom && dx.abs() <= 1.1 * r * (dy - top) / (bottom - top)
                }
            }
        };
        for y in y0..=y1 {
            for x in x0..=x1 {
                let dx = x as f32 + 0.5 - cx;
                let dy = y as f32 + 0.5 - cy;
                if inside(dx, dy) {
                    self.put(x, y, z, c);
                }
            }
        }
    }

    /// Filled axis-aligned ellipse inscribed in the box centred on
    /// `(cx, cy)` with half-extents `(rx, ry)` — the [`NodeShape::Ellipse`]
    /// node body. Shares [`Self::disc`]'s bounding-box scan with the ellipse
    /// inside test, the way [`Self::mark`] shares it with the marker shapes.
    pub fn ellipse(&mut self, cx: f32, cy: f32, z: f32, rx: f32, ry: f32, c: Rgb) {
        let (rx, ry) = (rx.max(0.5), ry.max(0.5));
        let (x0, x1) = ((cx - rx).floor() as i32, (cx + rx).ceil() as i32);
        let (y0, y1) = ((cy - ry).floor() as i32, (cy + ry).ceil() as i32);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let dx = (x as f32 + 0.5 - cx) / rx;
                let dy = (y as f32 + 0.5 - cy) / ry;
                if dx * dx + dy * dy <= 1.0 {
                    self.put(x, y, z, c);
                }
            }
        }
    }

    /// Depth-interpolated line — used for axis boxes and graph edges.
    pub fn line(&mut self, a: [f32; 3], b: [f32; 3], c: Rgb) {
        let (x0, y0) = (a[0], a[1]);
        let (x1, y1) = (b[0], b[1]);
        let steps = (x1 - x0).abs().max((y1 - y0).abs()).ceil().max(1.0) as i32;
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let x = x0 + (x1 - x0) * t;
            let y = y0 + (y1 - y0) * t;
            let z = a[2] + (b[2] - a[2]) * t;
            self.put(x.round() as i32, y.round() as i32, z, c);
        }
    }

    /// Filled triangle with per-pixel interpolated depth — the surface
    /// primitive. Vertices are projected screen points carrying depth in
    /// `[2]`; either winding is accepted. Degenerate triangles draw nothing.
    pub fn tri(&mut self, a: [f32; 3], b: [f32; 3], c: [f32; 3], col: Rgb) {
        self.tri_shaded(a, b, c, col, col, col);
    }

    /// [`tri`](Self::tri) with a color per vertex, interpolated across the
    /// face — Gouraud shading, so adjacent surface cells blend into one
    /// smooth gradient instead of reading as flat facets.
    pub fn tri_shaded(&mut self, a: [f32; 3], b: [f32; 3], c: [f32; 3], ca: Rgb, cb: Rgb, cc: Rgb) {
        // Signed twice-area; dividing the edge functions by it normalizes the
        // barycentric weights for both windings at once.
        let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        if area.abs() < 1e-6 {
            return;
        }
        let inv = 1.0 / area;
        let x0 = a[0].min(b[0]).min(c[0]).floor().max(0.0) as i32;
        let x1 = a[0].max(b[0]).max(c[0]).ceil().min(self.w as f32 - 1.0) as i32;
        let y0 = a[1].min(b[1]).min(c[1]).floor().max(0.0) as i32;
        let y1 = a[1].max(b[1]).max(c[1]).ceil().min(self.h as f32 - 1.0) as i32;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let wa = ((b[0] - px) * (c[1] - py) - (b[1] - py) * (c[0] - px)) * inv;
                let wb = ((c[0] - px) * (a[1] - py) - (c[1] - py) * (a[0] - px)) * inv;
                let wc = 1.0 - wa - wb;
                if wa < 0.0 || wb < 0.0 || wc < 0.0 {
                    continue;
                }
                let z = a[2] * wa + b[2] * wb + c[2] * wc;
                let col = [
                    (ca[0] as f32 * wa + cb[0] as f32 * wb + cc[0] as f32 * wc) as u8,
                    (ca[1] as f32 * wa + cb[1] as f32 * wb + cc[1] as f32 * wc) as u8,
                    (ca[2] as f32 * wa + cb[2] as f32 * wb + cc[2] as f32 * wc) as u8,
                ];
                self.put(x, y, z, col);
            }
        }
    }

    /// Fill the ribbon between two boundaries — a confidence band, a stacked
    /// layer, one side of a violin. `cols` is the band in framebuffer pixels,
    /// one `(x, y_lo, y_hi)` per sample; the area between successive columns
    /// is filled by linearly interpolating both edges across it.
    ///
    /// A column with any non-finite component breaks the band the way a
    /// non-finite vertex breaks a line into runs: the runs either side of it
    /// are filled, the span across it is not.
    ///
    /// The sweep walks one pixel column at a time rather than emitting two
    /// triangles per quad, because a band is the one shape that must not
    /// vanish where it matters most: a confidence interval pinches toward
    /// zero exactly where the estimate is most certain, and a triangle whose
    /// height falls below a pixel centre covers nothing at all. Every column
    /// drawn here is at least one pixel tall, so a band stays continuous as
    /// it narrows. The sweep assumes x is monotonic between samples, which
    /// every caller guarantees by construction.
    pub fn fill_between(&mut self, cols: &[(f64, f64, f64)], z: f32, c: Rgb) {
        let finite = |v: &(f64, f64, f64)| v.0.is_finite() && v.1.is_finite() && v.2.is_finite();
        for pair in cols.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if !finite(&a) || !finite(&b) {
                continue;
            }
            let (l, r) = if a.0 <= b.0 { (a, b) } else { (b, a) };
            // Clamp the sweep to the framebuffer. An x-windowed plot can map a
            // column thousands of pixels off screen, and the clip test inside
            // `put` would reject each write only after we had walked to it.
            let x_start = l.0.floor().max(0.0) as i32;
            let x_end = r.0.ceil().min(self.w as f64 - 1.0) as i32;
            if x_end < x_start {
                continue;
            }
            let span = r.0 - l.0;
            for x in x_start..=x_end {
                let t = if span > 0.0 {
                    (((x as f64 + 0.5) - l.0) / span).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let lo = l.1 + (r.1 - l.1) * t;
                let hi = l.2 + (r.2 - l.2) * t;
                let (y0, y1) = if lo <= hi { (lo, hi) } else { (hi, lo) };
                // `rect_fill` is inclusive, so a pinched column is still 1px.
                self.rect_fill(x, y0.round() as i32, x, y1.round() as i32, z, c);
            }
        }
    }

    /// Flatten to RGBA8. Background pixels are transparent so the plot floats
    /// over the terminal's own background.
    pub fn rgba(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.w * self.h * 4);
        for i in 0..self.color.len() {
            let [r, g, b] = self.color[i];
            let a = if self.drawn[i] { 255 } else { 0 };
            out.extend_from_slice(&[r, g, b, a]);
        }
        out
    }
}

/// Orbit camera: two rotation angles, zoom, and screen-space pan.
#[derive(Clone, Copy)]
pub struct Camera {
    pub yaw: f64,
    pub pitch: f64,
    pub zoom: f64,
    pub pan_x: f64,
    pub pan_y: f64,
}

impl Default for Camera {
    fn default() -> Self {
        // A slight starting tilt so 3D reads as 3D immediately.
        Self { yaw: 0.6, pitch: 0.5, zoom: 1.0, pan_x: 0.0, pan_y: 0.0 }
    }
}

impl Camera {
    pub fn rotate(&mut self, d_yaw: f64, d_pitch: f64) {
        self.yaw += d_yaw;
        self.pitch = (self.pitch + d_pitch).clamp(-1.55, 1.55);
    }
    pub fn zoom_by(&mut self, f: f64) {
        self.zoom = (self.zoom * f).clamp(0.05, 50.0);
    }
    pub fn pan(&mut self, dx: f64, dy: f64) {
        self.pan_x += dx;
        self.pan_y += dy;
    }
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// The full camera state, for frontends that persist the view across
    /// plot rebuilds: `(yaw, pitch, zoom, pan_x, pan_y)`.
    pub fn state(&self) -> (f64, f64, f64, f64, f64) {
        (self.yaw, self.pitch, self.zoom, self.pan_x, self.pan_y)
    }

    /// Restore a camera state, applying the same clamps as the incremental
    /// mutators so a restored view is always a reachable one.
    pub fn set_state(&mut self, yaw: f64, pitch: f64, zoom: f64, pan_x: f64, pan_y: f64) {
        self.yaw = yaw;
        self.pitch = pitch.clamp(-1.55, 1.55);
        self.zoom = zoom.clamp(0.05, 50.0);
        self.pan_x = pan_x;
        self.pan_y = pan_y;
    }

    /// Rotate a normalized point and return (x, y, depth) in view space.
    ///
    /// The data frame is z-up (the scientific-plotting convention: surface
    /// heights and scatter elevations live in z), matching the website's
    /// hand-drawn hero. Turntable order: yaw about the data z-axis first —
    /// a horizontal drag spins the scene like a turntable around what the
    /// viewer sees as vertical — then pitch about the screen x-axis, so a
    /// vertical drag changes elevation only, with no sideways skew.
    /// (Pitch-first composition tumbled the scene around an axis that sits
    /// diagonally on screen at nonzero yaw.)
    #[inline]
    fn view(&self, p: [f32; 3]) -> (f64, f64, f64) {
        let (x, up, depth) = (p[0] as f64, p[2] as f64, p[1] as f64);
        let (sy, cy) = self.yaw.sin_cos();
        let x1 = x * cy + depth * sy;
        let z1 = -x * sy + depth * cy;
        let (sp, cp) = self.pitch.sin_cos();
        let y2 = up * cp - z1 * sp;
        let z2 = up * sp + z1 * cp;
        (x1, y2, z2)
    }
}

/// One camera degree of freedom a drag-gesture axis can drive.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CameraControl {
    /// Turntable spin about the data z-axis (what the viewer sees as
    /// vertical).
    Yaw,
    /// Elevation tilt about the screen x-axis.
    Pitch,
    /// Screen-space pan, horizontal.
    PanX,
    /// Screen-space pan, vertical.
    PanY,
    /// Exponential zoom about the view center.
    Zoom,
    /// The gesture axis does nothing.
    Off,
}

/// Which camera control each drag-gesture axis drives — hosts override
/// fields on [`Plot::input_map`] to remap gestures in code. The default is
/// the house feel: dragging rotates as a trackball (the drag grabs the
/// object), shift-dragging pans. A pan-first UI would set
/// `drag_x: PanX, drag_y: PanY`; axis-swapped rotation is
/// `drag_x: Pitch, drag_y: Yaw`. The `invert_*` flags flip the sign an
/// axis applies — `invert_drag_x: true, invert_drag_y: true` restores the
/// camera-grab rotation (drag right orbits the view right).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InputMap {
    pub drag_x: CameraControl,
    pub drag_y: CameraControl,
    pub shift_drag_x: CameraControl,
    pub shift_drag_y: CameraControl,
    pub invert_drag_x: bool,
    pub invert_drag_y: bool,
    pub invert_shift_drag_x: bool,
    pub invert_shift_drag_y: bool,
}

impl Default for InputMap {
    fn default() -> Self {
        Self {
            drag_x: CameraControl::Yaw,
            drag_y: CameraControl::Pitch,
            shift_drag_x: CameraControl::PanX,
            shift_drag_y: CameraControl::PanY,
            invert_drag_x: false,
            invert_drag_y: false,
            invert_shift_drag_x: false,
            invert_shift_drag_y: false,
        }
    }
}

/// Per-frontend gesture sensitivity for [`Plot::apply_drag`]: how much of
/// each camera control one dragged input unit (a pixel, a terminal cell)
/// applies. Pan is split per axis because terminal cells are not square.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct DragScales {
    /// Radians per unit.
    pub rotate: f64,
    /// Framebuffer pixels per horizontal unit.
    pub pan_x: f64,
    /// Framebuffer pixels per vertical unit.
    pub pan_y: f64,
    /// Log-zoom per unit.
    pub zoom: f64,
}

/// Projects data points to screen space. Built once per frame (or per pick) so
/// [`Plot::render`] and [`Plot::pick`] share identical geometry.
struct Projector {
    center: [f32; 3],
    inv_extent: f32,
    scale: f64,
    cx: f64,
    cy: f64,
    cam: Camera,
}

impl Projector {
    /// The camera-rotated point in normalized data space — before pixel
    /// scaling, so geometry computed here (facet normals for shading) is
    /// independent of resolution and zoom.
    #[inline]
    fn view_norm(&self, p: [f32; 3]) -> [f32; 3] {
        let n = [
            (p[0] - self.center[0]) * self.inv_extent,
            (p[1] - self.center[1]) * self.inv_extent,
            (p[2] - self.center[2]) * self.inv_extent,
        ];
        let (vx, vy, vz) = self.cam.view(n);
        [vx as f32, vy as f32, vz as f32]
    }

    #[inline]
    fn to_screen(&self, v: [f32; 3]) -> [f32; 3] {
        [
            (self.cx + v[0] as f64 * self.scale) as f32,
            (self.cy - v[1] as f64 * self.scale) as f32, // flip: +y is up on screen
            v[2],
        ]
    }

    #[inline]
    fn project(&self, p: [f32; 3]) -> [f32; 3] {
        self.to_screen(self.view_norm(p))
    }
}

/// Linear data→pixel transform for the 2D path, with the camera's zoom applied
/// about the plot-area center and its pan applied in pixels. Invertible, so
/// tick generation can ask what data range is visible.
#[derive(Default, Clone, Copy)]
struct Map2d {
    ax: f64,
    bx: f64,
    ay: f64,
    by: f64,
    /// Per-axis log₁₀ scaling, and the scale-space floor an off-scale value
    /// lands on; see [`Map2d::fwd`].
    lx: bool,
    ly: bool,
    xfloor: f64,
    yfloor: f64,
}

impl Map2d {
    fn new(
        data: (f64, f64, f64, f64),
        rect: (f64, f64, f64, f64),
        cam: &Camera,
        logs: (bool, bool),
    ) -> Self {
        let (lx, ly) = logs;
        let (dxlo, dxhi, dylo, dyhi) = data;
        // The affine solve happens in *scale* space, so a log axis is the
        // same straight line as any other — only the coordinate it is
        // straight in differs.
        let (dxlo, dxhi) = (Self::to_scale(dxlo, lx), Self::to_scale(dxhi, lx));
        let (dylo, dyhi) = (Self::to_scale(dylo, ly), Self::to_scale(dyhi, ly));
        let (x0, y0, x1, y1) = rect;
        let ax0 = (x1 - x0) / (dxhi - dxlo);
        let bx0 = x0 - ax0 * dxlo;
        let ay0 = -(y1 - y0) / (dyhi - dylo);
        let by0 = y1 - ay0 * dylo;
        let (rcx, rcy) = ((x0 + x1) * 0.5, (y0 + y1) * 0.5);
        let z = cam.zoom;
        Self {
            ax: ax0 * z,
            bx: rcx + (bx0 - rcx) * z + cam.pan_x,
            ay: ay0 * z,
            by: rcy + (by0 - rcy) * z + cam.pan_y,
            lx,
            ly,
            xfloor: Self::floor_of(dxlo, dxhi, lx),
            yfloor: Self::floor_of(dylo, dyhi, ly),
        }
    }

    /// Data → scale space: identity, or log₁₀ on a log axis.
    fn to_scale(v: f64, log: bool) -> f64 {
        if log {
            v.log10()
        } else {
            v
        }
    }

    /// Scale space → data, the inverse of [`Self::to_scale`].
    fn from_scale(v: f64, log: bool) -> f64 {
        if log {
            10f64.powf(v)
        } else {
            v
        }
    }

    /// Where an off-scale value (zero or negative on a log axis) is put: one
    /// full axis span below the bottom of the range. It has to go *somewhere*
    /// finite — the primitives walk their pixel span before the clip rejects
    /// each write, so a `-inf` coordinate would saturate into a span the size
    /// of the i32 range — and a span below is far enough to be clipped away
    /// at any sane zoom while keeping the segment that reaches it sloping
    /// off-scale the way a below-range point does on a linear axis.
    fn floor_of(lo: f64, hi: f64, log: bool) -> f64 {
        if log && lo.is_finite() && hi.is_finite() {
            lo - (hi - lo).abs().max(1.0)
        } else {
            f64::NEG_INFINITY
        }
    }

    fn fwd(v: f64, log: bool, floor: f64) -> f64 {
        if !log {
            return v;
        }
        if v > 0.0 {
            v.log10().max(floor)
        } else {
            floor
        }
    }

    fn sx(&self, x: f64) -> f64 {
        self.ax * Self::fwd(x, self.lx, self.xfloor) + self.bx
    }
    fn sy(&self, y: f64) -> f64 {
        self.ay * Self::fwd(y, self.ly, self.yfloor) + self.by
    }
    fn inv_x(&self, px: f64) -> f64 {
        Self::from_scale((px - self.bx) / self.ax, self.lx)
    }
    fn inv_y(&self, py: f64) -> f64 {
        Self::from_scale((py - self.by) / self.ay, self.ly)
    }
}

/// Autoscale padding: 5% of the span at each end, in the axis's own scale
/// space — a log axis pads by a fraction of a decade, not by a slice of a
/// number, which on a range like 1..1000 would otherwise push the low end
/// straight through zero. An axis nothing landed on falls back to a readable
/// unit range.
fn pad_range(lo: f64, hi: f64, log: bool) -> (f64, f64) {
    if !lo.is_finite() || !hi.is_finite() {
        return if log { (1.0, 10.0) } else { (-1.0, 1.0) };
    }
    if log {
        let (l0, l1) = (lo.log10(), hi.log10());
        let span = l1 - l0;
        let p = if span > 0.0 { span * 0.05 } else { 0.5 };
        return (10f64.powf(l0 - p), 10f64.powf(l1 + p));
    }
    let span = hi - lo;
    let p = if span > 0.0 { span * 0.05 } else { 1.0 };
    (lo - p, hi + p)
}

/// Force a range a log axis can actually solve in. Autoscale already only
/// counts positive samples, so what this catches is an explicit range: rather
/// than refuse the frame, a bad low end is lifted to a decade under the high
/// one and the plot draws what it can.
fn log_safe(lo: f64, hi: f64, log: bool) -> (f64, f64) {
    if !log {
        return (lo, hi);
    }
    let hi = if hi > 0.0 { hi } else { 10.0 };
    let lo = if lo > 0.0 && lo < hi { lo } else { hi / 10.0 };
    (lo, hi)
}

/// [`nice_ticks`] in the `(positions, labels)` shape [`date_ticks`] returns,
/// so every axis in [`Layout2d`] carries its labels rather than the step they
/// were formatted from. Labels are the only thing the renderer needs, and a
/// label list is the one shape a calendar, log or categorical axis can also
/// produce — a step cannot describe any of them.
fn numeric_ticks(lo: f64, hi: f64, target: usize) -> (Vec<f64>, Vec<String>) {
    let (t, step) = nice_ticks(lo, hi, target);
    let labels = t.iter().map(|v| format_tick(*v, step)).collect();
    (t, labels)
}

/// Ticks for a categorical axis: one per category at its own integer
/// position, labelled by name. Only categories inside the visible range are
/// emitted, and they thin by a whole stride when more would fit than `target`
/// — dropping every second label keeps the ones that remain where they
/// belong, which sub-sampling to exactly `target` would not.
fn category_ticks(names: &[String], lo: f64, hi: f64, target: usize) -> (Vec<f64>, Vec<String>) {
    if names.is_empty() || !lo.is_finite() || !hi.is_finite() {
        return (Vec::new(), Vec::new());
    }
    let first = lo.ceil().max(0.0);
    let last = hi.floor().min(names.len() as f64 - 1.0);
    if last < first {
        return (Vec::new(), Vec::new());
    }
    let (first, last) = (first as usize, last as usize);
    let stride = (last - first + 1).div_ceil(target.max(1));
    let mut pos = Vec::new();
    let mut labels = Vec::new();
    for i in (first..=last).step_by(stride) {
        pos.push(i as f64);
        labels.push(names[i].clone());
    }
    (pos, labels)
}

/// A colormap's legend: the ramp drawn as a labelled strip beside the plot,
/// so a colormapped trace says what its colors *mean*. Without one a heatmap
/// is decorative — the reader can see structure but cannot read a value.
///
/// `lo`/`hi` are the data range the ramp spans, which the caller sets from
/// whatever it mapped through the colormap; the strip's own ticks come from
/// the same ladder as an axis, so the numbers read alike.
#[derive(Clone, Debug, PartialEq)]
pub struct Colorbar {
    pub map: Colormap,
    pub lo: f64,
    pub hi: f64,
    /// A caption above the strip. There is no rotated text, so it sits over
    /// the ramp rather than alongside it, and the frame gives up a line of
    /// top margin to make room.
    pub label: Option<String>,
}

/// The colorbar's solved geometry: the gradient strip's rect and its ticks in
/// data units, positioned by the same top-is-`hi` convention as a y axis.
struct CbarLayout {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    ticks: Vec<f64>,
    labels: Vec<String>,
}

/// The solved 2D frame geometry: plot rect, per-axis maps, and ticks with
/// their rendered labels. Produced by `Plot::layout_2d` and consumed by the
/// renderer, so anything that must agree with what is on screen (hit tests,
/// future overlays) derives from the same solve instead of re-deriving it.
struct Layout2d {
    s: i32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    map: Map2d,
    maps_r: [Map2d; RIGHT_AXES],
    xticks: Vec<f64>,
    xlabels: Vec<String>,
    yticks: Vec<f64>,
    ylabels: Vec<String>,
    rticks: [Vec<f64>; RIGHT_AXES],
    rlabels: [Vec<String>; RIGHT_AXES],
    col_x: [i32; RIGHT_AXES], // label column offset from x1
    has_right: [bool; RIGHT_AXES],
    strip: Option<StripLayout>,
    cbar: Option<CbarLayout>,
    /// The titles the frame had room for, already filtered by
    /// [`Plot::layout_2d`] — the renderer draws what it is given.
    title: Option<String>,
    x_title: Option<String>,
    y_title: Option<String>,
}

/// The range-slider strip's solved geometry: its rect, the full-extent
/// overview maps (one per axis), the full x domain they cover, and the
/// window edges in strip pixels.
struct StripLayout {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    map: Map2d,
    maps_r: [Map2d; RIGHT_AXES],
    full: (f64, f64),
    wx0: f64,
    wx1: f64,
}

/// What `range_slider_hit` found under the pointer: a window-edge handle, the
/// window body (drag to slide), or the track outside it (click to jump).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RangeHit {
    LeftHandle,
    RightHandle,
    Window,
    Track,
}

/// The narrowest `x_window` the drag/zoom mutators allow, as a fraction of
/// the full x extent — a window can shrink this far and no further, so a
/// handle drag can never collapse it into an unrecoverable sliver.
pub const MIN_WINDOW_FRAC: f64 = 0.02;

/// Range-slider strip height and activation floor, in `s`-scaled pixels: the
/// strip drops out silently below `STRIP_MIN_H` frame height, where it would
/// crush the plot area.
const STRIP_H_S: i32 = 24;
const STRIP_MIN_H: i32 = 160;

/// Squared distance from screen point `(px, py)` to the segment `a`–`b`,
/// using only the projected x/y (depth is ignored for hit testing).
fn point_segment_d2(px: f32, py: f32, a: [f32; 3], b: [f32; 3]) -> f32 {
    let (abx, aby) = (b[0] - a[0], b[1] - a[1]);
    let (apx, apy) = (px - a[0], py - a[1]);
    let len2 = abx * abx + aby * aby;
    let t = if len2 > 0.0 { ((apx * abx + apy * aby) / len2).clamp(0.0, 1.0) } else { 0.0 };
    let (dx, dy) = (px - (a[0] + abx * t), py - (a[1] + aby * t));
    dx * dx + dy * dy
}

/// Draw a segment as a white glow pulled to the front — the hover/selection
/// treatment for graph edges.
fn edge_glow(fb: &mut Framebuffer, a: [f32; 3], b: [f32; 3], r: f32) {
    let front = -1.0e9;
    let steps = (b[0] - a[0]).abs().max((b[1] - a[1]).abs()).ceil().max(1.0) as i32;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        fb.disc(a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, front, r, [255, 255, 255]);
    }
}

#[inline]
pub(crate) fn vsub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[inline]
pub(crate) fn vcross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

#[inline]
fn shade(c: Rgb, f: f32) -> Rgb {
    [
        (c[0] as f32 * f).clamp(0.0, 255.0) as u8,
        (c[1] as f32 * f).clamp(0.0, 255.0) as u8,
        (c[2] as f32 * f).clamp(0.0, 255.0) as u8,
    ]
}

/// The shared geometry of the overlay panels — the legend and the crosshair
/// readout — so the two never drift apart. Proportions come from the website
/// hero's legend and are expressed against the text cell, so they hold at
/// every render scale: roomy side padding, a swatch a little shorter than a
/// line, and ~1.75 lines of leading between rows.
#[derive(Clone, Copy)]
struct PanelStyle {
    /// Padding inside the box: `x` at the sides, `y` above and below the rows.
    pad_x: i32,
    pad_y: i32,
    /// Swatch → label gap.
    gap: i32,
    /// Swatch side, and the baseline-to-baseline row pitch.
    swatch: i32,
    row_h: i32,
    /// Row pitch minus the text cell: the air between two rows.
    leading: i32,
    /// Corner radius of the box, and the border width.
    radius: f32,
    stroke: f32,
    /// How far the box sits from the edge it is anchored to — a touch more
    /// horizontally, like the hero's 16px/12px.
    inset: i32,
    inset_x: i32,
    /// Cap height for the stroke font, which takes over above scale 1.
    cap_height: f32,
}

impl PanelStyle {
    fn new(s: i32) -> Self {
        let ch = CHAR_H * s;
        let unit = |f: f32| (ch as f32 * f).round().max(1.0) as i32;
        let row_h = unit(1.75);
        Self {
            pad_x: unit(1.2),
            pad_y: unit(0.6),
            gap: unit(0.75),
            swatch: unit(0.75),
            row_h,
            leading: row_h - ch,
            radius: ch as f32 * 0.42,
            stroke: s as f32,
            inset: unit(1.2),
            inset_x: unit(1.5),
            // At scale 1 the 5×7 bitmap font is the crispest thing there is;
            // any larger, the Hershey stroke font renders smooth instead of
            // blocky.
            cap_height: ch as f32 - s as f32 * 0.5,
        }
    }

    /// Box width and height for `rows` rows of `text_w`-wide labels. The last
    /// row's leading is trimmed so the bottom padding matches the top.
    fn box_size(&self, rows: i32, text_w: i32) -> (i32, i32) {
        (
            self.pad_x + self.swatch + self.gap + text_w + self.pad_x,
            2 * self.pad_y + rows * self.row_h - self.leading,
        )
    }

    /// Left edge of the label column, relative to the box's left edge.
    fn text_dx(&self) -> i32 {
        self.pad_x + self.swatch + self.gap
    }

    /// The rounded box itself.
    fn frame(&self, fb: &mut Framebuffer, b: (i32, i32, i32, i32), z: f32, chrome: &Chrome) {
        rounded_panel(fb, b.0, b.1, b.2, b.3, self.radius, self.stroke, z, chrome.bg, chrome.frame);
    }

    /// A series chip, centred on the text cell whose top-left is `(bx0, ey)`.
    fn chip(&self, fb: &mut Framebuffer, bx0: i32, ey: i32, s: i32, z: f32, color: Rgb) {
        let sy = ey + (CHAR_H * s - self.swatch + s) / 2;
        let sx = bx0 + self.pad_x;
        let r = (self.swatch as f32 * 0.22).max(1.0);
        rounded_panel(
            fb,
            sx,
            sy,
            sx + self.swatch - 1,
            sy + self.swatch - 1,
            r,
            0.0,
            z,
            color,
            color,
        );
    }

    /// Width of `text` at this panel's cap height.
    fn measure(&self, text: &str) -> i32 {
        text_width_at(text, self.cap_height)
    }

    /// One label, drawn against the panel's own opaque fill — the one place
    /// text sits on a known background, so it can be antialiased.
    #[allow(clippy::too_many_arguments)]
    fn label(&self, fb: &mut Framebuffer, x: i32, y: i32, text: &str, z: f32, ink: Rgb, bg: Rgb) {
        draw_text_at(fb, x, y, text, self.cap_height, z, ink, bg);
    }
}

/// The top-left corner for the crosshair readout panel, in framebuffer
/// pixels: `px` is the guide's *snapped* x (never the raw hovered pixel, or
/// two hovers that snap to the same sample would render differently),
/// `markers` the marker y's currently on the frame, and `legend` the legend's
/// box when there is one.
///
/// Four slots — beside the guide on either side, in either half of the frame
/// — scored on how far they fall outside the plot rect, then on how much of
/// the legend they cover. The preferred half is the one the markers are
/// *not* in: the panel is opaque, and a tall multi-series readout sitting on
/// the values it names hides more than it explains. Chart.js and Plotly do
/// the opposite — they centre the label on the point and accept the occlusion
/// — but both animate between positions and draw a caret back to the point.
/// Here the guide line is already the tether, so the panel is free to stand
/// well clear of the data.
fn readout_slot(
    px: i32,
    box_w: i32,
    box_h: i32,
    rect: (i32, i32, i32, i32),
    gap: i32,
    markers: &[i32],
    legend: Option<(i32, i32, i32, i32)>,
) -> (i32, i32) {
    let (x0, y0, x1, y1) = rect;
    let (top, bottom) = (y0 + gap, y1 - gap - box_h);
    // Markers up top send the panel low, and the other way about. With no
    // marker on the frame there is nothing to dodge, so it keeps the top
    // corner the single-slot placement always used.
    let panel_low = !markers.is_empty() && {
        let sum: i64 = markers.iter().map(|&y| i64::from(y)).sum();
        sum / markers.len() as i64 <= i64::from((y0 + y1) / 2)
    };
    let (near, far) = if panel_low { (bottom, top) } else { (top, bottom) };
    let (right, left) = (px + gap, px - gap - box_w);
    let cands = [(right, near), (left, near), (right, far), (left, far)];

    let overlap = |a: (i32, i32, i32, i32), b: (i32, i32, i32, i32)| -> i64 {
        let w = i64::from((a.2.min(b.2) - a.0.max(b.0) + 1).max(0));
        let h = i64::from((a.3.min(b.3) - a.1.max(b.1) + 1).max(0));
        w * h
    };
    let full = i64::from(box_w + 1) * i64::from(box_h + 1);

    let (bx, by) = cands
        .iter()
        .min_by_key(|&&(bx, by)| {
            let b = (bx, by, bx + box_w, by + box_h);
            (full - overlap(b, rect), legend.map_or(0, |l| overlap(b, l)))
        })
        // `min_by_key` keeps the first of equal keys, so a tie — no legend, or
        // a legend none of the slots touch — falls to the preferred slot and
        // placement stays deterministic.
        .copied()
        .unwrap_or((right, near));

    // A panel wider or taller than the frame cannot be placed, only pushed
    // back inside it — the same guards the single-slot placement used.
    ((bx.min(x1 - box_w)).max(x0 + 1), by.clamp(y0, (y1 - box_h).max(y0)))
}

/// One legend row: the trace it stands for, its label and swatch colour, and
/// whether that trace is currently drawn.
struct LegendRow<'a> {
    trace: usize,
    name: &'a str,
    color: Rgb,
    visible: bool,
}

/// The legend's pixel box and the rows in it, for one render size. Built once
/// and used by both the drawing pass and [`Plot::legend_hit`], so what is on
/// screen and what a click resolves to can never disagree.
struct LegendBox<'a> {
    ps: PanelStyle,
    bx0: i32,
    by0: i32,
    bx1: i32,
    by1: i32,
    rows: Vec<LegendRow<'a>>,
}

impl LegendBox<'_> {
    /// The trace whose row covers `(px, py)`, if the point is in the box. The
    /// whole row pitch counts, leading included, so the gaps between rows are
    /// not dead zones for a mouse.
    fn row_at(&self, px: f32, py: f32) -> Option<usize> {
        let (x, y) = (px.round() as i32, py.round() as i32);
        if x < self.bx0 || x > self.bx1 || y < self.by0 || y > self.by1 {
            return None;
        }
        let from_first_row = y - (self.by0 + self.ps.pad_y);
        let i = (from_first_row.max(0) / self.ps.row_h) as usize;
        self.rows.get(i.min(self.rows.len() - 1)).map(|r| r.trace)
    }
}

/// Pull a colour most of the way to `bg` — how a toggled-off legend row reads
/// as off without leaving a hole where the row was.
#[inline]
fn fade(c: Rgb, bg: Rgb) -> Rgb {
    const T: f32 = 0.62;
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * T).round() as u8;
    [mix(c[0], bg[0]), mix(c[1], bg[1]), mix(c[2], bg[2])]
}

/// Drain a colour to its own luminance — series identity off, shape kept.
#[inline]
fn desaturate(c: Rgb) -> Rgb {
    let y = (0.2126 * c[0] as f32 + 0.7152 * c[1] as f32 + 0.0722 * c[2] as f32).round() as u8;
    [y, y, y]
}

/// A rounded rectangle over the inclusive pixel box, filled with `fill` and
/// outlined by a `stroke`-wide border drawn *inside* the shape in `border`.
/// The outer edge is antialiased against whatever is already on the buffer, so
/// this belongs on top of finished geometry — the overlay panels and their
/// colour chips, where a hard 90° corner is the one thing that reads as
/// unfinished. Pass `border == fill` for a plain chip.
#[allow(clippy::too_many_arguments)]
fn rounded_panel(
    fb: &mut Framebuffer,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    r: f32,
    stroke: f32,
    z: f32,
    fill: Rgb,
    border: Rgb,
) {
    let (x0, x1) = (x0.min(x1), x0.max(x1));
    let (y0, y1) = (y0.min(y1), y0.max(y1));
    // Pixel centres live at +0.5, so the shape spans [x0, x1 + 1].
    let (cx, cy) = ((x0 + x1 + 1) as f32 * 0.5, (y0 + y1 + 1) as f32 * 0.5);
    let (hx, hy) = ((x1 + 1 - x0) as f32 * 0.5, (y1 + 1 - y0) as f32 * 0.5);
    let r = r.clamp(0.0, hx.min(hy));
    for y in y0..=y1 {
        for x in x0..=x1 {
            // Signed distance to the rounded rect: negative inside.
            let qx = (x as f32 + 0.5 - cx).abs() - (hx - r);
            let qy = (y as f32 + 0.5 - cy).abs() - (hy - r);
            let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
            let d = qx.max(qy).min(0.0) + outside - r;
            let cov = (0.5 - d).clamp(0.0, 1.0);
            if cov <= 0.0 {
                continue;
            }
            let edge = (d + stroke + 0.5).clamp(0.0, 1.0);
            let c = [
                (fill[0] as f32 + (border[0] as f32 - fill[0] as f32) * edge) as u8,
                (fill[1] as f32 + (border[1] as f32 - fill[1] as f32) * edge) as u8,
                (fill[2] as f32 + (border[2] as f32 - fill[2] as f32) * edge) as u8,
            ];
            fb.blend_px(x, y, z, c, cov);
        }
    }
}

/// Stroke a projected 3D segment with the given half-width, interpolating
/// depth along it so the stroke occludes correctly. The thin case is exactly
/// [`Framebuffer::line`]; wider strokes stamp depth-carrying discs.
fn stroke3d(fb: &mut Framebuffer, a: [f32; 3], b: [f32; 3], r: f32, c: Rgb) {
    if r <= 0.75 {
        fb.line(a, b, c);
        return;
    }
    let steps = (b[0] - a[0]).abs().max((b[1] - a[1]).abs()).ceil().max(1.0) as i32;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let x = a[0] + (b[0] - a[0]) * t;
        let y = a[1] + (b[1] - a[1]) * t;
        let z = a[2] + (b[2] - a[2]) * t;
        fb.disc(x, y, z, r, c);
    }
}

/// Stroke a 2D segment with the given half-width by stamping discs along it.
fn stroke(fb: &mut Framebuffer, a: (f64, f64), b: (f64, f64), r: f32, c: Rgb) {
    if r <= 0.75 {
        fb.line([a.0 as f32, a.1 as f32, 0.0], [b.0 as f32, b.1 as f32, 0.0], c);
        return;
    }
    let steps = ((b.0 - a.0).abs().max((b.1 - a.1).abs()).ceil() as i32).max(1);
    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        let x = a.0 + (b.0 - a.0) * t;
        let y = a.1 + (b.1 - a.1) * t;
        fb.disc(x as f32, y as f32, 0.0, r, c);
    }
}

/// Liang–Barsky clip of segment `a`–`b` to the box `(x0, y0, x1, y1)`;
/// `None` when it lies fully outside. `stroke` costs a disc per pixel of
/// projected length, so an x-windowed plot must clip segments that shoot far
/// off screen before drawing, not after.
fn clip_segment(
    a: (f64, f64),
    b: (f64, f64),
    (x0, y0, x1, y1): (f64, f64, f64, f64),
) -> Option<((f64, f64), (f64, f64))> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [(-dx, a.0 - x0), (dx, x1 - a.0), (-dy, a.1 - y0), (dy, y1 - a.1)] {
        if p == 0.0 {
            if q < 0.0 {
                return None; // parallel and outside this edge
            }
            continue;
        }
        let t = q / p;
        if p < 0.0 {
            t0 = t0.max(t);
        } else {
            t1 = t1.min(t);
        }
        if t0 > t1 {
            return None;
        }
    }
    Some(((a.0 + dx * t0, a.1 + dy * t0), (a.0 + dx * t1, a.1 + dy * t1)))
}

/// Side padding inside a graph node's box, in text-scale units — the air
/// between the label and the border on each side.
const NODE_PAD_X_S: i32 = 4;
/// Padding above and below the label, in text-scale units.
const NODE_PAD_Y_S: i32 = 3;
/// Floor on a node box, in text-scale units, so an unlabelled node is still
/// a box you can see and click rather than a dot.
const NODE_MIN_W_S: i32 = 8;
const NODE_MIN_H_S: i32 = 7;

/// One graph node's pixel box: its centre and half-extents. Sized from the
/// label at the frame's text scale, so it is the *box* that stays constant
/// under zoom while the centres spread apart.
///
/// Shared by drawing and picking — a hit test that measured the box its own
/// way would drift from what is on screen the moment either changed.
#[derive(Clone, Copy, Debug)]
struct NodeBox {
    cx: f64,
    cy: f64,
    hw: f64,
    hh: f64,
}

impl NodeBox {
    /// The inclusive pixel rectangle to fill.
    fn rect(&self) -> (i32, i32, i32, i32) {
        (
            (self.cx - self.hw).round() as i32,
            (self.cy - self.hh).round() as i32,
            (self.cx + self.hw).round() as i32 - 1,
            (self.cy + self.hh).round() as i32 - 1,
        )
    }

    /// Grow by `m` pixels on every side — the halo box behind a hovered or
    /// selected node.
    fn grown(&self, m: f64) -> NodeBox {
        NodeBox { cx: self.cx, cy: self.cy, hw: self.hw + m, hh: self.hh + m }
    }

    /// Is `(px, py)` inside this silhouette? The test is the shape itself,
    /// not its bounding box, so the notch beside a diamond belongs to
    /// whatever is behind it rather than to the diamond.
    fn contains(&self, shape: NodeShape, px: f64, py: f64) -> bool {
        let (dx, dy) = ((px - self.cx).abs(), (py - self.cy).abs());
        let (hw, hh) = (self.hw.max(1e-6), self.hh.max(1e-6));
        match shape {
            NodeShape::Rounded | NodeShape::Box => dx <= hw && dy <= hh,
            NodeShape::Ellipse => (dx / hw).powi(2) + (dy / hh).powi(2) <= 1.0,
            NodeShape::Diamond => dx / hw + dy / hh <= 1.0,
        }
    }

    /// How far `(px, py)` is from this node, squared: zero inside the
    /// silhouette, else the distance to the bounding rectangle. A terminal
    /// mouse reports whole cells, so a pick needs a tolerance *outside* the
    /// box as well as an exact answer inside it.
    fn hit_d2(&self, shape: NodeShape, px: f64, py: f64) -> f64 {
        if self.contains(shape, px, py) {
            return 0.0;
        }
        let dx = ((px - self.cx).abs() - self.hw).max(0.0);
        let dy = ((py - self.cy).abs() - self.hh).max(0.0);
        dx * dx + dy * dy
    }

    /// Where the ray leaving the centre along `(dx, dy)` crosses the
    /// silhouette — where an edge must stop so it meets the box instead of
    /// running under it to the label.
    fn boundary(&self, shape: NodeShape, dx: f64, dy: f64) -> (f64, f64) {
        let (hw, hh) = (self.hw.max(1e-6), self.hh.max(1e-6));
        let (ax, ay) = (dx.abs(), dy.abs());
        if ax < 1e-9 && ay < 1e-9 {
            return (self.cx, self.cy + hh);
        }
        let t = match shape {
            NodeShape::Rounded | NodeShape::Box => {
                let tx = if ax > 1e-9 { hw / ax } else { f64::INFINITY };
                let ty = if ay > 1e-9 { hh / ay } else { f64::INFINITY };
                tx.min(ty)
            }
            NodeShape::Ellipse => 1.0 / ((ax / hw).powi(2) + (ay / hh).powi(2)).sqrt(),
            NodeShape::Diamond => 1.0 / (ax / hw + ay / hh),
        };
        (self.cx + dx * t, self.cy + dy * t)
    }
}

/// Round the corners of a polyline: each interior waypoint becomes a short
/// quadratic Bezier tangent to both of its segments, flattened into straight
/// runs the [`stroke`] helper can draw. There is no curve primitive in the
/// framebuffer, and a routed edge that turned a hard corner at every rank
/// would read as a staircase rather than as one wire.
fn smooth_polyline(pts: &[(f64, f64)]) -> Vec<(f64, f64)> {
    /// Flattening steps per corner: enough that the arc reads as a curve at
    /// terminal resolution, few enough to stay cheap on a dense graph.
    const STEPS: usize = 8;
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let dist = |a: (f64, f64), b: (f64, f64)| (b.0 - a.0).hypot(b.1 - a.1);
    let along = |from: (f64, f64), to: (f64, f64), t: f64| {
        (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t)
    };
    let mut out = vec![pts[0]];
    for i in 1..pts.len() - 1 {
        let (prev, c, next) = (pts[i - 1], pts[i], pts[i + 1]);
        let (d0, d1) = (dist(prev, c), dist(c, next));
        // The cut is bounded by both neighbours, so two corners a few pixels
        // apart never eat past each other into a self-crossing curve.
        let r = (0.4 * d0.min(d1)).min(14.0);
        if r < 1.0 || d0 <= 0.0 || d1 <= 0.0 {
            out.push(c);
            continue;
        }
        let a = along(c, prev, r / d0);
        let b = along(c, next, r / d1);
        out.push(a);
        for k in 1..STEPS {
            let t = k as f64 / STEPS as f64;
            let u = 1.0 - t;
            out.push((
                u * u * a.0 + 2.0 * u * t * c.0 + t * t * b.0,
                u * u * a.1 + 2.0 * u * t * c.1 + t * t * b.1,
            ));
        }
        out.push(b);
    }
    out.push(pts[pts.len() - 1]);
    out
}

/// Pull a colour toward white — how a node's card fill is derived from the
/// chrome background, so the box reads as a panel rather than as a hole.
#[inline]
fn lighten(c: Rgb, by: u8) -> Rgb {
    [c[0].saturating_add(by), c[1].saturating_add(by), c[2].saturating_add(by)]
}

/// One node's pixel box, from its data centre and its label at text scale
/// `s`. The label sets the width; the height is one text cell plus padding,
/// so every box in a graph is the same height and the ranks line up.
fn node_box(m: &Map2d, p: [f32; 2], label: &str, s: i32) -> NodeBox {
    let w = (text_width(label, s) + 2 * NODE_PAD_X_S * s).max(NODE_MIN_W_S * s);
    let h = (CHAR_H * s + 2 * NODE_PAD_Y_S * s).max(NODE_MIN_H_S * s);
    NodeBox { cx: m.sx(p[0] as f64), cy: m.sy(p[1] as f64), hw: w as f64 * 0.5, hh: h as f64 * 0.5 }
}

/// Edge `e`'s waypoints out of the CSR pair, exactly the way [`box_groups`]
/// reads its groups: a run that runs off the end, or backwards, is an empty
/// one — a straight edge — rather than a panic. The renderer must not be the
/// thing that trusts the bindings' validation.
fn edge_route<'a>(pts: &'a [[f32; 2]], starts: &[u32], e: usize) -> &'a [[f32; 2]] {
    let Some(&a) = starts.get(e) else { return &[] };
    let b = starts.get(e + 1).map_or(pts.len(), |v| *v as usize);
    let a = a as usize;
    if a <= b && b <= pts.len() {
        &pts[a..b]
    } else {
        &[]
    }
}

/// A filled diamond inscribed in `b`, as two triangles sharing the waist.
fn diamond_fill(fb: &mut Framebuffer, b: &NodeBox, c: Rgb) {
    let (cx, cy) = (b.cx as f32, b.cy as f32);
    let (hw, hh) = (b.hw as f32, b.hh as f32);
    let (l, r) = ([cx - hw, cy, 0.0], [cx + hw, cy, 0.0]);
    fb.tri(l, [cx, cy - hh, 0.0], r, c);
    fb.tri(l, [cx, cy + hh, 0.0], r, c);
}

/// One node body: a `fill`ed silhouette with a `border`-coloured outline
/// `1.5 · s` px wide, drawn inside the shape so the box never grows past
/// what the hit test measured.
///
/// The rounded and square cases go through [`rounded_panel`], the one
/// rounded-rect primitive in the crate; the other two draw the outline as a
/// larger silhouette with a smaller one painted over it, which is cheaper
/// than a signed-distance pass and indistinguishable at these sizes.
fn draw_node_body(fb: &mut Framebuffer, b: &NodeBox, shape: NodeShape, fill: Rgb, border: Rgb) {
    let bw = 1.5;
    let (x0, y0, x1, y1) = b.rect();
    match shape {
        NodeShape::Rounded | NodeShape::Box => {
            let r = if shape == NodeShape::Box { 0.0 } else { (b.hh as f32 * 0.45).min(6.0) };
            rounded_panel(fb, x0, y0, x1, y1, r, bw, 0.0, fill, border);
        }
        NodeShape::Ellipse => {
            let (rx, ry) = (b.hw as f32, b.hh as f32);
            fb.ellipse(b.cx as f32, b.cy as f32, 0.0, rx, ry, border);
            fb.ellipse(b.cx as f32, b.cy as f32, 0.0, (rx - bw).max(0.5), (ry - bw).max(0.5), fill);
        }
        NodeShape::Diamond => {
            diamond_fill(fb, b, border);
            // A diamond's edge runs diagonally, so a border of `bw` pixels
            // needs the inner shape inset by more than `bw` on each axis.
            let inset = bw as f64 * std::f64::consts::SQRT_2;
            let inner = NodeBox { hw: (b.hw - inset).max(1.0), hh: (b.hh - inset).max(1.0), ..*b };
            diamond_fill(fb, &inner, fill);
        }
    }
}

/// Which y scale a 2D series is measured against. `Y2` and `Y3` are
/// independent right-hand axes: each autoscales from its own traces and gets
/// its own tick-label column (Y2 innermost, Y3 outermost). The grid always
/// belongs to the primary axis. A closed enum keeps invalid axes
/// unrepresentable; render code indexes the right axes via [`Self::right_index`].
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum YAxis {
    #[default]
    Primary,
    Y2,
    Y3,
}

/// How many right-hand axes exist (the size of every per-right-axis array).
pub const RIGHT_AXES: usize = 2;

impl YAxis {
    /// Index into per-right-axis arrays; `None` for the primary axis.
    pub fn right_index(self) -> Option<usize> {
        match self {
            YAxis::Primary => None,
            YAxis::Y2 => Some(0),
            YAxis::Y3 => Some(1),
        }
    }
}

/// A single plotted series.
pub enum Trace {
    Scatter3d {
        pts: Vec<[f32; 3]>,
        color: Rgb,
        size: f32,
        name: Option<String>,
    },
    Graph3d {
        nodes: Vec<[f32; 3]>,
        node_colors: Vec<Rgb>,
        edges: Vec<(u32, u32)>,
        size: f32,
        /// Per-node radius override; falls back to `size` where absent.
        node_sizes: Option<Vec<f32>>,
        /// Per-edge color override; without it an edge takes a dimmed average
        /// of its endpoint node colors.
        edge_colors: Option<Vec<Rgb>>,
        /// Per-node marker silhouette; discs where absent.
        node_shapes: Option<Vec<Shape>>,
        name: Option<String>,
    },
    Line3d {
        pts: Vec<[f32; 3]>,
        color: Rgb,
        width: f32,
        name: Option<String>,
    },
    Surface3d {
        /// Grid axes: `zs[j * xs.len() + i]` is the height at (xs[i], ys[j]),
        /// in the same (x, y, z) space as `Scatter3d` — no axis is special.
        /// A cell with a non-finite corner is left as a hole.
        xs: Vec<f32>,
        ys: Vec<f32>,
        zs: Vec<f32>,
        /// Solid surface color; `colormap` replaces it with a height ramp.
        color: Rgb,
        colormap: Option<Colormap>,
        /// Overlay the grid lines, pulled slightly toward the viewer.
        wireframe: bool,
        name: Option<String>,
    },
    Scatter2d {
        xs: Vec<f32>,
        ys: Vec<f32>,
        color: Rgb,
        size: f32,
        /// Per-point color override; falls back to `color` where absent.
        /// Short lists pad rather than truncate the series, so a partial
        /// mapping never silently drops points.
        colors: Option<Vec<Rgb>>,
        /// Per-point radius override, in the same units as `size`.
        sizes: Option<Vec<f32>>,
        /// Per-point marker silhouette; discs where absent. Shape is a second
        /// channel alongside color, which is what lets a categorical scatter
        /// stay readable in a colorblind-safe palette — or in a terminal
        /// whose palette the user has themed out from under it.
        shapes: Option<Vec<Shape>>,
        /// Per-point uncertainty; see [`ErrBars`]. Set through
        /// [`Plot::set_error_bars`] rather than at construction, so the
        /// bindings' shared two-array add path stays intact.
        err_x: Option<ErrBars>,
        err_y: Option<ErrBars>,
        name: Option<String>,
        axis: YAxis,
    },
    Line2d {
        xs: Vec<f32>,
        ys: Vec<f32>,
        color: Rgb,
        width: f32,
        /// How the stroke gets between samples; see [`Interp`].
        interp: Interp,
        /// Per-point uncertainty; see [`ErrBars`].
        err_x: Option<ErrBars>,
        err_y: Option<ErrBars>,
        name: Option<String>,
        axis: YAxis,
    },
    Box2d {
        /// Every group's values, concatenated. Flat rather than nested so the
        /// shape crosses the C ABI without a second level of indirection —
        /// the same trick `Mesh3d`'s flat triangle indices use.
        values: Vec<f32>,
        /// Where each group starts in `values`; `group_starts[g]..
        /// group_starts[g + 1]` is group `g`, with the last group running to
        /// the end. CSR, in other words.
        group_starts: Vec<u32>,
        color: Rgb,
        orient: Orient,
        name: Option<String>,
        axis: YAxis,
    },
    Band2d {
        /// The sweep axis, shared by both boundaries.
        xs: Vec<f32>,
        /// The two boundaries at each x. Which is higher does not matter —
        /// the fill is between them — so a band whose edges cross is drawn,
        /// not rejected.
        lo: Vec<f32>,
        hi: Vec<f32>,
        color: Rgb,
        name: Option<String>,
        axis: YAxis,
    },
    Heatmap2d {
        /// Grid axes: `zs[j * xs.len() + i]` is the value at (xs[i], ys[j]) —
        /// the same row-major shape [`Trace::Surface3d`] uses, so the two
        /// share one validation rule in the bindings. A non-finite value is
        /// a hole, the way a surface cell with a non-finite corner is.
        xs: Vec<f32>,
        ys: Vec<f32>,
        zs: Vec<f32>,
        colormap: Colormap,
        name: Option<String>,
    },
    Histogram2d {
        /// The raw sample, kept rather than pre-binned: it is what lets
        /// `extend_values` stream new observations and rebin, and what lets
        /// the crosshair report a bin's interval and count.
        values: Vec<f32>,
        bins: BinSpec,
        color: Rgb,
        name: Option<String>,
        axis: YAxis,
    },
    Bar2d {
        /// The category positions. With [`Orient::Horizontal`] these are y
        /// coordinates, not x — the field keeps its name because it is still
        /// the axis the bars are spaced along.
        xs: Vec<f32>,
        heights: Vec<f32>,
        color: Rgb,
        orient: Orient,
        name: Option<String>,
        axis: YAxis,
    },
    Graph2d {
        /// Node centres in data coordinates. The *box* around each centre is
        /// sized in pixels from its label, so zooming spreads the nodes apart
        /// while the labels stay legible — the only readable choice at
        /// terminal resolution.
        nodes: Vec<[f32; 2]>,
        /// One label per node; an empty string draws an unlabelled box.
        /// Short lists pad rather than truncate, as the per-point style
        /// arrays do.
        labels: Vec<String>,
        /// One colour per node — the border of its box, and the channel a
        /// live pipeline repaints through [`Plot::set_graph_colors`].
        node_colors: Vec<Rgb>,
        /// Per-node silhouette; [`NodeShape::Rounded`] where absent.
        node_shapes: Option<Vec<NodeShape>>,
        /// Directed pairs `(from, to)` of node indices. Endpoints out of
        /// range are kept but inert, so an edge never loses its flat index.
        edges: Vec<(u32, u32)>,
        /// Draw an arrowhead at each edge's target end.
        directed: bool,
        /// Per-edge colour override; without it an edge takes a dimmed
        /// average of its endpoint colours, as [`Trace::Graph3d`] does.
        edge_colors: Option<Vec<Rgb>>,
        /// Optional waypoints per edge, in data coordinates and CSR order:
        /// `route_starts[e]..route_starts[e + 1]` indexes `route_pts`.
        /// Waypoints exclude the endpoints, so an empty run is a straight
        /// edge. This is how a layered layout routes an edge that spans
        /// several ranks around the nodes between them.
        route_pts: Vec<[f32; 2]>,
        route_starts: Vec<u32>,
        name: Option<String>,
    },
    Mesh3d {
        /// Indexed triangles: `tris[t]` names three vertices of `verts`, in
        /// the same (x, y, z) space as `Scatter3d`. A triangle with an
        /// out-of-range index or a non-finite vertex is skipped, the way a
        /// surface cell with a non-finite corner is a hole.
        verts: Vec<[f32; 3]>,
        tris: Vec<[u32; 3]>,
        /// Solid mesh color; `colormap` replaces it with a z ramp.
        color: Rgb,
        colormap: Option<Colormap>,
        name: Option<String>,
    },
}

/// The 3D variants as an or-pattern, for the 2D code paths that must name
/// them explicitly rather than fall through a `_`.
///
/// Every 2D path below (axis binding, both bounds scans, both renderers, both
/// crosshair passes) matches exhaustively on purpose: a new 2D trace that
/// forgot one of them would compile and then silently contribute no extent,
/// draw nothing, or ignore its y-axis binding. Spelling the 3D variants out
/// once here is what turns each of those omissions into a compile error.
/// A new *3D* variant belongs in this list — the same seven sites will point
/// here until it is added.
macro_rules! traces_3d {
    () => {
        Trace::Scatter3d { .. }
            | Trace::Graph3d { .. }
            | Trace::Line3d { .. }
            | Trace::Surface3d { .. }
            | Trace::Mesh3d { .. }
    };
}

impl Trace {
    /// Whether this trace lives in the 3D scene. One 3D trace switches the
    /// whole plot to the orbit camera (see [`Plot::is_3d`]).
    pub fn is_3d(&self) -> bool {
        matches!(self, traces_3d!())
    }

    /// Why this trace cannot be appended to: its wire name and the reason its
    /// shape is fixed, or `None` when it extends freely. Graphs, surfaces and
    /// meshes qualify because indices or grid dimensions tie their arrays
    /// together, so appending to one array alone would leave the trace
    /// inconsistent.
    ///
    /// Core supplies the facts and the bindings phrase them, so the message a
    /// user sees stays identical across Python, Go, C and JavaScript. The
    /// match is exhaustive on purpose: a new structural trace that forgot this
    /// would silently accept `extend` and corrupt itself.
    pub fn structural_reason(&self) -> Option<(&'static str, &'static str)> {
        match self {
            Trace::Graph3d { .. } => Some(("graph3d", "edges reference node indices")),
            Trace::Graph2d { .. } => Some(("graph2d", "edges reference node indices")),
            Trace::Surface3d { .. } => Some(("surface3d", "a fixed grid")),
            Trace::Mesh3d { .. } => Some(("mesh3d", "triangles reference vertex indices")),
            Trace::Heatmap2d { .. } => Some(("heatmap", "a fixed grid")),
            Trace::Box2d { .. } => Some((
                "box",
                "its boxes are derived from grouped samples; rebuild the plot to change them",
            )),
            Trace::Histogram2d { .. } => {
                Some(("histogram", "its bars are derived from a sample; append with extend_values"))
            }
            Trace::Scatter3d { .. }
            | Trace::Line3d { .. }
            | Trace::Scatter2d { .. }
            | Trace::Line2d { .. }
            | Trace::Bar2d { .. }
            | Trace::Band2d { .. } => None,
        }
    }

    /// Whether [`Self::structural_reason`] applies — the trace must be rebuilt
    /// rather than appended to.
    pub fn is_structural(&self) -> bool {
        self.structural_reason().is_some()
    }

    fn name(&self) -> Option<&str> {
        match self {
            Trace::Scatter3d { name, .. }
            | Trace::Graph3d { name, .. }
            | Trace::Line3d { name, .. }
            | Trace::Surface3d { name, .. }
            | Trace::Scatter2d { name, .. }
            | Trace::Line2d { name, .. }
            | Trace::Bar2d { name, .. }
            | Trace::Histogram2d { name, .. }
            | Trace::Heatmap2d { name, .. }
            | Trace::Band2d { name, .. }
            | Trace::Box2d { name, .. }
            | Trace::Graph2d { name, .. }
            | Trace::Mesh3d { name, .. } => name.as_deref(),
        }
    }

    fn color(&self) -> Rgb {
        match self {
            Trace::Scatter3d { color, .. }
            | Trace::Line3d { color, .. }
            | Trace::Scatter2d { color, .. }
            | Trace::Line2d { color, .. }
            | Trace::Bar2d { color, .. }
            | Trace::Histogram2d { color, .. }
            | Trace::Band2d { color, .. }
            | Trace::Box2d { color, .. } => *color,
            // A colormapped surface has no single color; its legend swatch is
            // a sample from the upper half of the ramp.
            Trace::Surface3d { color, colormap, .. } | Trace::Mesh3d { color, colormap, .. } => {
                colormap.map_or(*color, |m| m.sample(0.75))
            }
            // A colormapped grid has no single color; its legend swatch is a
            // sample from the upper half of the ramp, as surfaces do.
            Trace::Heatmap2d { colormap, .. } => colormap.sample(0.75),
            Trace::Graph3d { node_colors, .. } | Trace::Graph2d { node_colors, .. } => {
                node_colors.first().copied().unwrap_or([120, 180, 230])
            }
        }
    }

    fn axis(&self) -> YAxis {
        match self {
            Trace::Scatter2d { axis, .. }
            | Trace::Line2d { axis, .. }
            | Trace::Bar2d { axis, .. }
            | Trace::Histogram2d { axis, .. }
            | Trace::Band2d { axis, .. }
            | Trace::Box2d { axis, .. } => *axis,
            // A grid spans both axes itself; binding it to a second y scale
            // would ask which of two scales its rows are measured against.
            // A graph's coordinates are a layout, not a measurement, so the
            // same reasoning keeps it on the primary axis.
            Trace::Heatmap2d { .. } | Trace::Graph2d { .. } => YAxis::Primary,
            traces_3d!() => YAxis::Primary,
        }
    }
}

/// A short human-readable value for the crosshair readout: precision scaled
/// to magnitude, trailing zeros trimmed.
fn format_value(v: f64) -> String {
    let a = v.abs();
    let s = if a >= 100.0 {
        format!("{v:.0}")
    } else if a >= 10.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.2}")
    };
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

/// Half the drawn width of a bar, in data units: 40% of the smallest gap
/// between distinct x positions, so adjacent bars keep a visible gap.
fn bar_halfwidth(xs: &[f32]) -> f32 {
    let mut sorted: Vec<f32> = xs.iter().copied().filter(|v| v.is_finite()).collect();
    sorted.sort_by(f32::total_cmp);
    let mut gap = f32::INFINITY;
    for w in sorted.windows(2) {
        let d = w[1] - w[0];
        if d > 0.0 {
            gap = gap.min(d);
        }
    }
    if gap.is_finite() {
        gap * 0.4
    } else {
        0.4
    }
}

/// A pickable piece of a plot: a node or an edge, identified by its flat index
/// (across all traces, in insertion order — edges keep their index even when
/// their endpoints are out of range, so indices always match the caller's
/// edge list).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Element {
    Node(usize),
    Edge(usize),
}

/// A [`Plot::pick_surface`] hit: the grid vertex's data coordinates and its
/// projected screen position (`[x_px, y_px, depth]`, the
/// [`Plot::project_nodes`] convention).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SurfaceHit {
    pub data: [f32; 3],
    pub screen: [f32; 3],
}

/// A trace's stable identity: its index in [`Plot::traces`]. Traces are never
/// removed, so a handle can't dangle for the lifetime of its plot.
pub type TraceId = usize;

/// Why a per-trace operation ([`Plot::extend_xy`], [`Plot::extend_pts`],
/// [`Plot::set_visible`]) was refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TraceError {
    /// No trace with that id.
    UnknownTrace,
    /// The trace exists but is the wrong dimensionality for the call.
    WrongKind,
    /// The trace's shape is structural (graph edges, surface grid) and cannot
    /// be appended to; rebuild the plot instead.
    Structural,
    /// A per-node or per-edge array does not match the trace's node/edge
    /// count (`set_graph_positions`, `set_graph_colors`).
    LengthMismatch,
}

impl std::fmt::Display for TraceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TraceError::UnknownTrace => write!(f, "unknown trace handle"),
            TraceError::WrongKind => write!(f, "wrong trace kind for this operation"),
            TraceError::Structural => {
                write!(f, "structural trace (graph/surface) cannot be extended")
            }
            TraceError::LengthMismatch => {
                write!(f, "per-node/per-edge array length must match the trace's node/edge count")
            }
        }
    }
}

/// Half the spacing of a grid axis: the smallest positive gap between
/// consecutive coordinates, halved, so neighbouring cells meet without
/// overlapping. Mirrors [`bar_halfwidth`]'s rule, and its fallback for a
/// single coordinate.
fn grid_half_step(vs: &[f32]) -> f64 {
    let mut gap = f64::INFINITY;
    for w in vs.windows(2) {
        let d = (w[1] - w[0]).abs() as f64;
        if d > 0.0 {
            gap = gap.min(d);
        }
    }
    if gap.is_finite() {
        gap * 0.5
    } else {
        0.5
    }
}

/// How a histogram chooses its bins.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BinSpec {
    /// Exactly this many bins across the data's range.
    Count(usize),
    /// Bins of this width, starting at the smallest value.
    Width(f64),
    /// Freedman–Diaconis (`2·IQR/n^⅓`), which adapts to spread rather than
    /// to count, falling back to Sturges when the IQR is zero — as it is for
    /// heavily tied data, where FD would ask for infinitely many bins.
    Auto,
}

/// The most bins any rule may ask for. A terminal plot has a few hundred
/// pixels of width; past this the bars are thinner than a pixel and the
/// histogram stops being readable before it stops being correct.
const MAX_BINS: usize = 200;

/// A histogram's solved bins: uniform `width` from `lo`, one count each.
#[derive(Clone, Debug, PartialEq)]
struct Bins {
    lo: f64,
    width: f64,
    counts: Vec<u32>,
}

impl Bins {
    /// The half-open interval `[lo, hi)` of bin `i`.
    fn edges(&self, i: usize) -> (f64, f64) {
        (self.lo + i as f64 * self.width, self.lo + (i + 1) as f64 * self.width)
    }

    fn center(&self, i: usize) -> f64 {
        self.lo + (i as f64 + 0.5) * self.width
    }

    fn hi(&self) -> f64 {
        self.lo + self.counts.len() as f64 * self.width
    }
}

/// Bin a sample into counts. Non-finite values are dropped, the way a
/// non-finite coordinate drops a point elsewhere; the largest value lands in
/// the last bin rather than falling off the end of the half-open interval.
fn bin_values(values: &[f32], spec: BinSpec) -> Bins {
    let mut v: Vec<f64> = values.iter().map(|&x| x as f64).filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return Bins { lo: 0.0, width: 1.0, counts: Vec::new() };
    }
    v.sort_by(f64::total_cmp);
    let (min, max) = (v[0], v[v.len() - 1]);
    let span = max - min;
    let n = v.len();
    let k = match spec {
        BinSpec::Count(k) => k.clamp(1, MAX_BINS),
        BinSpec::Width(w) if w > 0.0 && span > 0.0 => {
            ((span / w).ceil() as usize).clamp(1, MAX_BINS)
        }
        BinSpec::Width(_) => 1,
        BinSpec::Auto => {
            let q = |p: f64| v[(((n - 1) as f64) * p).round() as usize];
            let iqr = q(0.75) - q(0.25);
            let fd = 2.0 * iqr / (n as f64).cbrt();
            if fd > 0.0 && span > 0.0 {
                ((span / fd).ceil() as usize).clamp(1, MAX_BINS)
            } else {
                ((n as f64).log2().ceil() as usize + 1).clamp(1, MAX_BINS)
            }
        }
    };
    // An explicit width is honoured exactly; the other rules divide the span.
    let width = match spec {
        BinSpec::Width(w) if w > 0.0 => w,
        _ if span > 0.0 => span / k as f64,
        _ => 1.0,
    };
    let mut counts = vec![0u32; k];
    for x in v {
        let i = ((x - min) / width).floor();
        let i = if i < 0.0 { 0 } else { (i as usize).min(k - 1) };
        counts[i] += 1;
    }
    Bins { lo: min, width, counts }
}

/// Cached raw extent of one trace, in the same terms the full scans use:
/// [`CachedBounds::B2`] mirrors `bounds_2d`'s per-point rule (a point counts
/// only when both coordinates are finite; bars contribute `x ± hw` and
/// `h.min(0) / h.max(0)`), [`CachedBounds::B3`] mirrors `extent_points`
/// (scatter/graph vertices unfiltered, line/surface vertices finite-only).
/// Empty traces keep the infinite sentinels, which are the identity of the
/// min/max union.
enum CachedBounds {
    /// `pad` is the `(x, y)` half-extent already folded into the box for a
    /// trace whose marks are wider than their sample point — a bar's shared
    /// half-width, later a grid cell's half-size on each axis. It is kept
    /// rather than recomputed so the renderer reads back exactly the number
    /// the range was built from and the two can never disagree; `None` is a
    /// trace whose extent is its points.
    B2 {
        xlo: f64,
        xhi: f64,
        ylo: f64,
        yhi: f64,
        pad: Option<(f64, f64)>,
    },
    B3 {
        lo: [f32; 3],
        hi: [f32; 3],
    },
}

/// Per-trace bookkeeping kept parallel to [`Plot::traces`]: visibility plus
/// the incremental counters and bounds that let `extend` cost O(delta) and
/// per-frame bounds cost O(traces). Maintained eagerly by the mutating
/// methods; consumers fall back to the full scans whenever `meta` has fallen
/// out of sync with a directly-mutated `traces` field.
struct TraceMeta {
    /// The host's own show/hide ([`Plot::set_visible`]): a trace switched off
    /// this way is gone from the plot entirely, legend row included.
    visible: bool,
    /// Toggled off from the legend ([`Plot::toggle_muted`]): the geometry
    /// goes, but the row stays — greyed out, and it is the way back on.
    muted: bool,
    /// Pickable nodes this trace contributes to the flat node index space.
    node_len: usize,
    /// Vertices this trace contributes to `vertex_count` (extent rule).
    vert_len: usize,
    bounds: CachedBounds,
    /// A histogram's solved bins, cached with the same discipline as
    /// `CachedBounds`: the renderer reads back exactly the bins the bounds
    /// were computed from, so bars and axis can never disagree. `None` for
    /// every other trace, and whenever the cache is stale.
    bins: Option<Bins>,
}

/// One full scan of a trace, replicating exactly what `bounds_2d` /
/// `extent_points` would see for it.
fn compute_meta(t: &Trace) -> TraceMeta {
    match t {
        Trace::Scatter2d { xs, ys, err_x, err_y, .. }
        | Trace::Line2d { xs, ys, err_x, err_y, .. } => {
            let mut b = b2_empty(None);
            b2_seen_xy(&mut b, xs, ys, 0);
            b2_seen_errors(&mut b, xs, ys, err_x.as_ref(), err_y.as_ref());
            TraceMeta {
                visible: true,
                muted: false,
                node_len: 0,
                vert_len: 0,
                bounds: b,
                bins: None,
            }
        }
        Trace::Box2d { values, group_starts, orient, .. } => {
            // A box is centred on its group index and half a slot wide, so the
            // category axis pads like a bar; the value axis spans whiskers and
            // outliers alike — an outlier off the edge of the frame would be
            // the one point you most needed to see.
            let hw = BOX_HALF_WIDTH;
            let pad = if orient.is_horizontal() { (0.0, hw) } else { (hw, 0.0) };
            let mut b = b2_empty(Some(pad));
            if let CachedBounds::B2 { xlo, xhi, ylo, yhi, .. } = &mut b {
                for (g, group) in box_groups(values, group_starts).enumerate() {
                    let Some(st) = box_stats(group) else { continue };
                    let cat = g as f64;
                    let mut val_lo = st.lo.min(st.q1);
                    let mut val_hi = st.hi.max(st.q3);
                    for o in &st.outliers {
                        val_lo = val_lo.min(*o);
                        val_hi = val_hi.max(*o);
                    }
                    let ((a0, a1), (b0, b1)) = if orient.is_horizontal() {
                        ((val_lo, val_hi), (cat - hw, cat + hw))
                    } else {
                        ((cat - hw, cat + hw), (val_lo, val_hi))
                    };
                    *xlo = xlo.min(a0);
                    *xhi = xhi.max(a1);
                    *ylo = ylo.min(b0);
                    *yhi = yhi.max(b1);
                }
            }
            TraceMeta {
                visible: true,
                muted: false,
                node_len: 0,
                vert_len: 0,
                bounds: b,
                bins: None,
            }
        }
        Trace::Band2d { xs, lo, hi, .. } => {
            let mut b = b2_empty(None);
            // Both boundaries are folded in, so the axis fits the whole
            // ribbon rather than one of its edges.
            b2_seen_xy(&mut b, xs, lo, 0);
            b2_seen_xy(&mut b, xs, hi, 0);
            TraceMeta {
                visible: true,
                muted: false,
                node_len: 0,
                vert_len: 0,
                bounds: b,
                bins: None,
            }
        }
        Trace::Heatmap2d { xs, ys, zs, .. } => {
            // Cells are centred on their grid coordinates, so the drawn extent
            // reaches half a cell beyond the outermost centres on both axes —
            // the two-axis generalisation of a bar's half-width.
            let (hx, hy) = (grid_half_step(xs), grid_half_step(ys));
            let mut b = b2_empty(Some((hx, hy)));
            if let CachedBounds::B2 { xlo, xhi, ylo, yhi, .. } = &mut b {
                for (j, &y) in ys.iter().enumerate() {
                    for (i, &x) in xs.iter().enumerate() {
                        let v = zs.get(j * xs.len() + i).copied().unwrap_or(f32::NAN);
                        if x.is_finite() && y.is_finite() && v.is_finite() {
                            *xlo = xlo.min(x as f64 - hx);
                            *xhi = xhi.max(x as f64 + hx);
                            *ylo = ylo.min(y as f64 - hy);
                            *yhi = yhi.max(y as f64 + hy);
                        }
                    }
                }
            }
            TraceMeta {
                visible: true,
                muted: false,
                node_len: 0,
                vert_len: 0,
                bounds: b,
                bins: None,
            }
        }
        Trace::Histogram2d { values, bins, .. } => {
            let b = bin_values(values, *bins);
            // The bars tile the range edge to edge, so the extent is the outer
            // edges themselves — no half-width to pad by — and y runs from the
            // zero baseline to the tallest bin.
            let top = b.counts.iter().copied().max().unwrap_or(0) as f64;
            let bounds = if b.counts.is_empty() {
                b2_empty(None)
            } else {
                CachedBounds::B2 { xlo: b.lo, xhi: b.hi(), ylo: 0.0, yhi: top, pad: None }
            };
            TraceMeta {
                visible: true,
                muted: false,
                node_len: 0,
                vert_len: 0,
                bounds,
                bins: Some(b),
            }
        }
        Trace::Bar2d { xs, heights, orient, .. } => {
            let hw = bar_halfwidth(xs) as f64;
            // Bars widen along their category axis only; along the value axis
            // they span the baseline to the height.
            let pad = if orient.is_horizontal() { (0.0, hw) } else { (hw, 0.0) };
            let mut b = b2_empty(Some(pad));
            b2_seen_bars(&mut b, xs, heights, 0, hw, *orient);
            TraceMeta {
                visible: true,
                muted: false,
                node_len: 0,
                vert_len: 0,
                bounds: b,
                bins: None,
            }
        }
        Trace::Graph2d { nodes, route_pts, .. } => {
            let mut b = b2_empty(None);
            if let (CachedBounds::B2 { xlo, xhi, ylo, yhi, .. }, Some((a, c, d, e))) =
                (&mut b, graph_extent(nodes, route_pts))
            {
                *xlo = a;
                *xhi = c;
                *ylo = d;
                *yhi = e;
            }
            TraceMeta {
                visible: true,
                muted: false,
                node_len: nodes.len(),
                vert_len: nodes.len(),
                bounds: b,
                bins: None,
            }
        }
        Trace::Scatter3d { pts, .. } => {
            let mut b = b3_empty();
            b3_seen_all(&mut b, pts);
            TraceMeta {
                visible: true,
                muted: false,
                node_len: pts.len(),
                vert_len: pts.len(),
                bounds: b,
                bins: None,
            }
        }
        Trace::Graph3d { nodes, .. } => {
            let mut b = b3_empty();
            b3_seen_all(&mut b, nodes);
            TraceMeta {
                visible: true,
                muted: false,
                node_len: nodes.len(),
                vert_len: nodes.len(),
                bounds: b,
                bins: None,
            }
        }
        Trace::Line3d { pts, .. } => {
            let mut b = b3_empty();
            let n = b3_seen_finite(&mut b, pts);
            TraceMeta {
                visible: true,
                muted: false,
                node_len: 0,
                vert_len: n,
                bounds: b,
                bins: None,
            }
        }
        Trace::Mesh3d { verts, .. } => {
            let mut b = b3_empty();
            let n = b3_seen_finite(&mut b, verts);
            TraceMeta {
                visible: true,
                muted: false,
                node_len: 0,
                vert_len: n,
                bounds: b,
                bins: None,
            }
        }
        Trace::Surface3d { xs, ys, zs, .. } => {
            let mut b = b3_empty();
            let nx = xs.len();
            let mut n = 0usize;
            for (j, &y) in ys.iter().enumerate() {
                for (i, &x) in xs.iter().enumerate() {
                    if let Some(&z) = zs.get(j * nx + i) {
                        let p = [x, y, z];
                        if p.iter().all(|c| c.is_finite()) {
                            b3_seen_all(&mut b, &[p]);
                            n += 1;
                        }
                    }
                }
            }
            TraceMeta {
                visible: true,
                muted: false,
                node_len: 0,
                vert_len: n,
                bounds: b,
                bins: None,
            }
        }
    }
}

/// A graph's data extent `(xlo, xhi, ylo, yhi)`: the box its node centres
/// and edge waypoints occupy. `None` when nothing finite was found.
///
/// Only the *centres*. A node's box is measured in pixels, so the room it
/// needs cannot be expressed here at all — that is
/// [`Plot::graph_box_inset`]'s job, in the frame where pixels exist. The one
/// widening this does is for a degenerate span (a single node, or a rank of
/// one), which would otherwise give the axis map nothing to scale by.
fn graph_extent(nodes: &[[f32; 2]], route_pts: &[[f32; 2]]) -> Option<(f64, f64, f64, f64)> {
    let (mut xlo, mut xhi) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut ylo, mut yhi) = (f64::INFINITY, f64::NEG_INFINITY);
    for p in nodes.iter().chain(route_pts) {
        let (x, y) = (p[0] as f64, p[1] as f64);
        if x.is_finite() && y.is_finite() {
            xlo = xlo.min(x);
            xhi = xhi.max(x);
            ylo = ylo.min(y);
            yhi = yhi.max(y);
        }
    }
    if !xlo.is_finite() {
        return None;
    }
    let pad = |lo: f64, hi: f64| if hi > lo { 0.0 } else { 0.5 };
    let (px, py) = (pad(xlo, xhi), pad(ylo, yhi));
    Some((xlo - px, xhi + px, ylo - py, yhi + py))
}

fn b2_empty(pad: Option<(f64, f64)>) -> CachedBounds {
    CachedBounds::B2 {
        xlo: f64::INFINITY,
        xhi: f64::NEG_INFINITY,
        ylo: f64::INFINITY,
        yhi: f64::NEG_INFINITY,
        pad,
    }
}

fn b3_empty() -> CachedBounds {
    CachedBounds::B3 { lo: [f32::INFINITY; 3], hi: [f32::NEG_INFINITY; 3] }
}

/// Fold paired points from index `from` into a `B2`, applying `bounds_2d`'s
/// both-finite rule.
fn b2_seen_xy(b: &mut CachedBounds, xs: &[f32], ys: &[f32], from: usize) {
    let CachedBounds::B2 { xlo, xhi, ylo, yhi, .. } = b else { return };
    for i in from..xs.len().min(ys.len()) {
        let (x, y) = (xs[i] as f64, ys[i] as f64);
        if x.is_finite() && y.is_finite() {
            *xlo = xlo.min(x);
            *xhi = xhi.max(x);
            *ylo = ylo.min(y);
            *yhi = yhi.max(y);
        }
    }
}

/// Draw one point's error bars: a spine through the point and a cap at each
/// end. Caps are what make a bar read as an interval rather than as a stray
/// line through the mark, so they are not decoration.
#[allow(clippy::too_many_arguments)]
fn draw_error_bars(
    fb: &mut Framebuffer,
    m: &Map2d,
    x: f64,
    y: f64,
    err_x: Option<(f64, f64)>,
    err_y: Option<(f64, f64)>,
    s: i32,
    c: Rgb,
) {
    let cap = (2 * s).max(2);
    let (px, py) = (m.sx(x).round() as i32, m.sy(y).round() as i32);
    if let Some((down, up)) = err_y {
        let (a, b) = (m.sy(y + up).round() as i32, m.sy(y - down).round() as i32);
        fb.rect_fill(px, a, px, b, 0.0, c);
        fb.rect_fill(px - cap, a, px + cap, a, 0.0, c);
        fb.rect_fill(px - cap, b, px + cap, b, 0.0, c);
    }
    if let Some((down, up)) = err_x {
        let (a, b) = (m.sx(x - down).round() as i32, m.sx(x + up).round() as i32);
        fb.rect_fill(a, py, b, py, 0.0, c);
        fb.rect_fill(a, py - cap, a, py + cap, 0.0, c);
        fb.rect_fill(b, py - cap, b, py + cap, 0.0, c);
    }
}

/// Widen a `B2` by each point's error bars. Bars reach past the point they
/// qualify, so an axis sized to the points alone would clip their caps.
fn b2_seen_errors(
    b: &mut CachedBounds,
    xs: &[f32],
    ys: &[f32],
    err_x: Option<&ErrBars>,
    err_y: Option<&ErrBars>,
) {
    if err_x.is_none() && err_y.is_none() {
        return;
    }
    let CachedBounds::B2 { xlo, xhi, ylo, yhi, .. } = b else { return };
    for i in 0..xs.len().min(ys.len()) {
        let (x, y) = (xs[i] as f64, ys[i] as f64);
        if !x.is_finite() || !y.is_finite() {
            continue;
        }
        if let Some((down, up)) = err_x.and_then(|e| e.at(i)) {
            *xlo = xlo.min(x - down);
            *xhi = xhi.max(x + up);
        }
        if let Some((down, up)) = err_y.and_then(|e| e.at(i)) {
            *ylo = ylo.min(y - down);
            *yhi = yhi.max(y + up);
        }
    }
}

/// Fold bar extents from index `from` into a `B2`: `x ± hw` on x, the span
/// from the zero baseline to `h` on y — the same contributions `bounds_2d`
/// makes for bars.
fn b2_seen_bars(
    b: &mut CachedBounds,
    xs: &[f32],
    heights: &[f32],
    from: usize,
    hw: f64,
    orient: Orient,
) {
    let CachedBounds::B2 { xlo, xhi, ylo, yhi, .. } = b else { return };
    for i in from..xs.len().min(heights.len()) {
        let (x, h) = (xs[i] as f64, heights[i] as f64);
        if !x.is_finite() || !h.is_finite() {
            continue;
        }
        // The category axis carries the half-width; the value axis spans the
        // zero baseline to the height. Which axis is which is the whole of
        // the orientation.
        let (cat_lo, cat_hi) = (x - hw, x + hw);
        let (val_lo, val_hi) = (h.min(0.0), h.max(0.0));
        let ((a_lo, a_hi), (b_lo, b_hi)) = if orient.is_horizontal() {
            ((val_lo, val_hi), (cat_lo, cat_hi))
        } else {
            ((cat_lo, cat_hi), (val_lo, val_hi))
        };
        *xlo = xlo.min(a_lo);
        *xhi = xhi.max(a_hi);
        *ylo = ylo.min(b_lo);
        *yhi = yhi.max(b_hi);
    }
}

/// Fold points into a `B3` unfiltered — `f32::min`/`max` ignore NaN and
/// propagate infinities exactly like the full `bounds` scan.
fn b3_seen_all(b: &mut CachedBounds, pts: &[[f32; 3]]) {
    let CachedBounds::B3 { lo, hi } = b else { return };
    for p in pts {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
}

/// Fold only fully-finite points into a `B3`; returns how many qualified
/// (the extent rule for line vertices).
fn b3_seen_finite(b: &mut CachedBounds, pts: &[[f32; 3]]) -> usize {
    let mut n = 0usize;
    for p in pts {
        if p.iter().all(|c| c.is_finite()) {
            b3_seen_all(b, &[*p]);
            n += 1;
        }
    }
    n
}

/// The full plot: traces, camera, and hover/selection highlight state.
pub struct Plot {
    pub traces: Vec<Trace>,
    pub camera: Camera,
    pub show_box: bool,
    /// Whether the 2D frame draws its chrome — grid, axis rules and tick
    /// labels. `None`, the default, decides automatically: a frame whose
    /// visible 2D traces are *all* [`Trace::Graph2d`] draws none of it,
    /// because a pipeline's coordinates are a layout, not measurements, and
    /// a numeric ladder beside one says nothing true. `Some(true)` always
    /// draws the chrome (useful to see where a layout actually put its
    /// nodes), `Some(false)` never does. Set it through
    /// [`Self::set_show_axes`]; the legend, colorbar, range slider and
    /// crosshair are unaffected either way. Ignored by 3D plots.
    pub show_axes: Option<bool>,
    /// Explicit 3D data frame `(lo, hi)`; without it the frame is the bounding
    /// box of the node points. A host that rebuilds a plot with a changing
    /// subset of the same data pins this so the view does not re-centre.
    pub bounds_override: Option<([f32; 3], [f32; 3])>,
    /// Element to draw with the selection treatment (click).
    pub selected: Option<Element>,
    /// Element to light up white (hover affordance: "you can click this").
    pub hovered: Option<Element>,
    /// Hovered x in framebuffer pixels, for the 2D crosshair. When set,
    /// `render_2d` snaps it to the nearest sample x, draws a vertical guide
    /// with a marker per series sampled there, and a value readout box.
    /// Ignored by 3D plots.
    pub hover2d_px: Option<f32>,
    /// Hovered surface point in data coordinates (a [`Self::pick_surface`]
    /// hit's `data`). When set, `render_3d` draws the hover guides: a ring at
    /// the point, its shadow on the box floor, axis-parallel guide lines from
    /// the walls to the shadow, and a drop line connecting point to shadow.
    /// Drawn on top of the scene (hover feedback stays visible behind
    /// geometry). Ignored by 2D plots.
    pub surface_hover: Option<[f32; 3]>,
    /// Pinned surface point (click): same guides as [`Self::surface_hover`]
    /// with the selection treatment on the ring. Survives camera changes —
    /// hosts reproject it per frame (see [`Self::project_point`]) to anchor
    /// a tooltip. Ignored by 2D plots.
    pub surface_selected: Option<[f32; 3]>,
    /// Gesture routing for [`Self::apply_drag`]: which camera control each
    /// drag axis drives. Defaults to drag = rotate, shift-drag = pan.
    pub input_map: InputMap,
    /// Explicit 2D x view `(lo, hi)` in data coordinates. When set, the main
    /// plot maps exactly this range (unpadded) to the plot area, every y axis
    /// autoscales from the points inside it, and the camera's 2D zoom/pan is
    /// superseded — the window *is* the view. `None` restores full-extent
    /// autoscale. Ignored by 3D plots.
    pub x_window: Option<(f64, f64)>,
    /// Draw the range-slider strip: a full-extent overview under the plot
    /// with the `x_window` selection in full color and grab handles at its
    /// edges. Silently dropped on frames too short to fit it, and ignored by
    /// 3D plots.
    pub range_slider: bool,
    /// When set, x values are seconds since this UTC epoch base: x ticks
    /// become calendar dates ([`date_ticks`]) and the crosshair readout shows
    /// a timestamp. The offset never enters the coordinate math — it exists
    /// because f32 xs can't hold raw epoch seconds (a 2026 timestamp
    /// quantizes to ~2 minutes), while offsets from a nearby base stay
    /// second-accurate for years.
    pub x_epoch: Option<f64>,
    /// Axis/grid/legend colours; see [`Chrome`].
    /// Names for a categorical x axis: category `i` sits at position `i`, and
    /// ticks become one label per category rather than a numeric ladder.
    /// Supplying names does not move the range — traces still place
    /// themselves, so a series plotted at 0, 1, 2 lines up with the first
    /// three names and a category nothing was plotted at simply falls outside
    /// the view. Takes precedence over [`Self::x_epoch`]: an axis of names is
    /// not a calendar.
    pub x_categories: Option<Vec<String>>,
    /// Names for a categorical primary y axis; the y counterpart of
    /// [`Self::x_categories`], for the sideways charts (horizontal bars,
    /// box-by-group, timelines). The right-hand axes stay numeric — they exist
    /// to carry a second *scale*, which a list of names has no notion of.
    pub y_categories: Option<Vec<String>>,
    /// The colormap legend beside the plot; `None` draws none. Set by whoever
    /// owns the mapping — a heatmap knows its own value range, and a
    /// colormapped scatter knows the range it binned its colors over — so the
    /// core never has to guess which trace the ramp belongs to.
    /// Chart title, centered over the plot area. `None` gives the line back
    /// to the data.
    pub title: Option<String>,
    /// What the numbers on the x axis mean, drawn under its tick labels.
    pub x_title: Option<String>,
    /// The primary y axis's title, rotated a quarter turn in the left margin
    /// (see [`draw_text_rot90`]). The right-hand axes take their identity
    /// from the colour their labels are tinted in instead: a second rotated
    /// column would cost more frame than a terminal has to give.
    pub y_title: Option<String>,
    /// Explicit x extent, replacing what autoscale found. Unlike
    /// [`Self::x_window`] this only decides the *extent*: the camera's 2D
    /// zoom/pan still compose on top, so pinning a range leaves interactive
    /// zoom working. Used exactly as given — an explicit range is a decision,
    /// so it gets none of autoscale's 5% padding. A set `x_window` is the
    /// narrower statement and wins.
    pub x_range: Option<(f64, f64)>,
    /// Explicit primary-y extent; the y counterpart of [`Self::x_range`].
    /// The right-hand axes keep autoscaling — they exist to fit a second
    /// series against its own spread.
    pub y_range: Option<(f64, f64)>,
    /// Scale the x axis by log₁₀. Ignored on a categorical or time axis:
    /// names and calendars own the coordinate they sit on, and there is no
    /// meaningful decade between Tuesday and Wednesday.
    pub x_log: bool,
    /// Scale the primary y axis by log₁₀; ignored on a categorical y axis.
    /// The right-hand axes stay linear.
    pub y_log: bool,
    /// How several bar traces on one axis share their positions; see
    /// [`BarMode`]. The default keeps plotui's original overlay behaviour.
    pub barmode: BarMode,
    pub colorbar: Option<Colorbar>,
    pub chrome: Chrome,
    /// Per-trace visibility + incremental bounds cache, parallel to `traces`.
    /// Private on purpose: it is maintained by the mutating methods, and every
    /// consumer falls back to the full scans when a direct push to the public
    /// `traces` field has left it behind. Equal-length in-place mutation of
    /// `traces` is the one thing this cannot detect and is unsupported.
    meta: Vec<TraceMeta>,
    /// Active color sequence for traces added without an explicit color.
    /// Never empty; swapped by `set_colorway`, read by `next_color`.
    colorway: Vec<Rgb>,
}

impl Default for Plot {
    fn default() -> Self {
        Self {
            traces: Vec::new(),
            camera: Camera::default(),
            show_box: true,
            show_axes: None,
            bounds_override: None,
            selected: None,
            hovered: None,
            hover2d_px: None,
            surface_hover: None,
            surface_selected: None,
            input_map: InputMap::default(),
            x_window: None,
            range_slider: false,
            x_epoch: None,
            x_categories: None,
            title: None,
            x_title: None,
            y_title: None,
            x_range: None,
            y_range: None,
            x_log: false,
            y_log: false,
            barmode: BarMode::default(),
            colorbar: None,
            y_categories: None,
            chrome: Chrome::default(),
            meta: Vec::new(),
            colorway: COLORWAY_PLOTUI.to_vec(),
        }
    }
}

impl Plot {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push a trace with its freshly computed meta; the trace's index is its
    /// stable handle.
    fn push_trace(&mut self, t: Trace) -> TraceId {
        self.resync_meta();
        self.meta.push(compute_meta(&t));
        self.traces.push(t);
        self.traces.len() - 1
    }

    /// Bring `meta` back in step with a `traces` field that grew (or shrank)
    /// behind our back through the public field. Newly discovered traces are
    /// scanned in full and default to visible.
    fn resync_meta(&mut self) {
        self.meta.truncate(self.traces.len());
        while self.meta.len() < self.traces.len() {
            self.meta.push(compute_meta(&self.traces[self.meta.len()]));
        }
    }

    fn meta_synced(&self) -> bool {
        self.meta.len() == self.traces.len()
    }

    /// Desync-safe visibility — is this trace drawn? A trace the cache does
    /// not know about yet is treated as visible. Muting hides the geometry
    /// exactly like hiding does; the two differ only in the legend.
    fn is_visible(&self, i: usize) -> bool {
        self.meta.get(i).is_none_or(|m| m.visible && !m.muted)
    }

    /// Does this trace get a legend row? Host-hidden traces do not exist as
    /// far as the chrome is concerned; muted ones keep their row so there is
    /// something to click to bring them back.
    fn in_legend(&self, i: usize) -> bool {
        self.meta.get(i).is_none_or(|m| m.visible)
    }

    pub fn add_scatter3d(
        &mut self,
        pts: Vec<[f32; 3]>,
        color: Rgb,
        size: f32,
        name: Option<String>,
    ) -> TraceId {
        self.push_trace(Trace::Scatter3d { pts, color, size, name })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_graph3d(
        &mut self,
        nodes: Vec<[f32; 3]>,
        node_colors: Vec<Rgb>,
        edges: Vec<(u32, u32)>,
        size: f32,
        node_sizes: Option<Vec<f32>>,
        edge_colors: Option<Vec<Rgb>>,
        node_shapes: Option<Vec<Shape>>,
        name: Option<String>,
    ) -> TraceId {
        self.push_trace(Trace::Graph3d {
            nodes,
            node_colors,
            edges,
            size,
            node_sizes,
            edge_colors,
            node_shapes,
            name,
        })
    }

    /// Add a directed graph in the 2D plane: labelled boxes at `nodes`, wired
    /// by `edges` (pairs of node indices). `labels`, `node_colors` and
    /// `node_shapes` are per node; `edge_colors` and the CSR `routes` are per
    /// edge. Short per-node lists fall back to the defaults rather than
    /// dropping nodes, so a partial mapping never loses geometry.
    ///
    /// `routes` gives each edge its waypoints — what
    /// [`LayeredLayout`](crate::LayeredLayout) emits for an edge that spans
    /// more than one rank — with an empty list for a straight edge. Pass
    /// `None` for straight edges throughout.
    #[allow(clippy::too_many_arguments)]
    pub fn add_graph2d(
        &mut self,
        nodes: Vec<[f32; 2]>,
        labels: Vec<String>,
        node_colors: Vec<Rgb>,
        edges: Vec<(u32, u32)>,
        directed: bool,
        node_shapes: Option<Vec<NodeShape>>,
        edge_colors: Option<Vec<Rgb>>,
        routes: Option<(Vec<[f32; 2]>, Vec<u32>)>,
        name: Option<String>,
    ) -> TraceId {
        let (route_pts, route_starts) = routes.unwrap_or_default();
        self.push_trace(Trace::Graph2d {
            nodes,
            labels,
            node_colors,
            node_shapes,
            edges,
            directed,
            edge_colors,
            route_pts,
            route_starts,
            name,
        })
    }

    /// Set [`Self::show_axes`]. `false`/`true` pin the chrome off or on;
    /// `None` restores the automatic rule.
    pub fn set_show_axes(&mut self, show: impl Into<Option<bool>>) {
        self.show_axes = show.into();
    }

    /// Half the widest and tallest node box among the visible 2D graphs, in
    /// pixels at text scale `s` — how much room the plot rect has to give up
    /// on each side so an outermost box is not clipped.
    ///
    /// This is the one thing a data-space pad cannot express: a graph's
    /// centres are in data units and its boxes are in pixels, so the right
    /// margin depends on the label text and not on the extent at all. Solved
    /// here, in the frame, where both are known.
    fn graph_box_inset(&self, s: i32) -> (i32, i32) {
        let (mut hw, mut hh) = (0, 0);
        for (i, t) in self.traces.iter().enumerate() {
            let Trace::Graph2d { nodes, labels, .. } = t else { continue };
            if nodes.is_empty() || !self.is_visible(i) {
                continue;
            }
            for j in 0..nodes.len() {
                let label = labels.get(j).map_or("", String::as_str);
                let w = (text_width(label, s) + 2 * NODE_PAD_X_S * s).max(NODE_MIN_W_S * s);
                hw = hw.max((w + 1) / 2);
            }
            let h = (CHAR_H * s + 2 * NODE_PAD_Y_S * s).max(NODE_MIN_H_S * s);
            hh = hh.max((h + 1) / 2);
        }
        // The halo a hovered box grows by, plus the border, so a highlight
        // does not reach past the frame either.
        let halo = if hw > 0 { 3 * s } else { 0 };
        (hw + halo, hh + halo)
    }

    /// How much of the frame's right edge the 2D legend covers, in pixels,
    /// or 0 when nothing is named.
    ///
    /// On a chart the legend merely overlaps some data, which is the house
    /// style everywhere. On a graph it can cover a *node* — and a node the
    /// reader cannot see is a task missing from the pipeline, not a few
    /// obscured samples — so a graph frame reserves this on the right and
    /// nothing lands under it.
    fn legend_width(&self, s: i32) -> i32 {
        // Anchored at the origin: only the width is wanted, and that does
        // not depend on where the box ends up.
        match self.legend_box(0, 0, s, false) {
            Some(lb) => (lb.bx1 - lb.bx0) + lb.ps.inset_x,
            None => 0,
        }
    }

    /// Does this 2D frame skip its grid, axis rules and tick labels? See
    /// [`Self::show_axes`] for the rule; a frame with no visible 2D trace at
    /// all keeps its chrome, so an empty plot still looks like a chart.
    fn chrome_hidden(&self) -> bool {
        if let Some(show) = self.show_axes {
            return !show;
        }
        let mut any = false;
        for (i, t) in self.traces.iter().enumerate() {
            if t.is_3d() || !self.is_visible(i) {
                continue;
            }
            any = true;
            if !matches!(t, Trace::Graph2d { .. }) {
                return false;
            }
        }
        any
    }

    /// Add a 3D polyline through `pts` in order. Vertices are not pickable;
    /// a non-finite vertex breaks the line into separate runs, as in 2D.
    pub fn add_line3d(
        &mut self,
        pts: Vec<[f32; 3]>,
        color: Rgb,
        width: f32,
        name: Option<String>,
    ) -> TraceId {
        self.push_trace(Trace::Line3d { pts, color, width, name })
    }

    /// Add a grid surface: `zs[j * xs.len() + i]` is the height at
    /// (xs[i], ys[j]). Cells with a non-finite corner are holes. Grid
    /// vertices are not pickable.
    #[allow(clippy::too_many_arguments)]
    pub fn add_surface3d(
        &mut self,
        xs: Vec<f32>,
        ys: Vec<f32>,
        zs: Vec<f32>,
        color: Rgb,
        colormap: Option<Colormap>,
        wireframe: bool,
        name: Option<String>,
    ) -> TraceId {
        self.push_trace(Trace::Surface3d { xs, ys, zs, color, colormap, wireframe, name })
    }

    /// Add a triangle mesh: `tris` indexes into `verts`. Triangles with an
    /// out-of-range index or a non-finite vertex are skipped. `colormap`
    /// samples over the mesh's own z range, as for a surface. Vertices are
    /// not pickable.
    ///
    /// [`marching_cubes`] turns a sampled scalar field into exactly this
    /// pair of arrays.
    pub fn add_mesh3d(
        &mut self,
        verts: Vec<[f32; 3]>,
        tris: Vec<[u32; 3]>,
        color: Rgb,
        colormap: Option<Colormap>,
        name: Option<String>,
    ) -> TraceId {
        self.push_trace(Trace::Mesh3d { verts, tris, color, colormap, name })
    }

    /// Append points to an existing 2D trace: `(xs, ys)` for scatter and line
    /// traces, `(xs, heights)` for bars. Concatenation semantics — the result
    /// renders byte-identically to a plot built with the concatenated arrays
    /// in one call, including the min-length pairing of ragged inputs.
    /// Scatter/line updates cost O(delta); bars recompute their bounds and
    /// shared bar width in full, because one appended x can narrow the gap
    /// that sizes every bar (the documented reflow).
    pub fn extend_xy(&mut self, id: TraceId, xs: &[f32], ys: &[f32]) -> Result<(), TraceError> {
        self.resync_meta();
        let t = self.traces.get_mut(id).ok_or(TraceError::UnknownTrace)?;
        match t {
            Trace::Scatter2d { xs: txs, ys: tys, .. } | Trace::Line2d { xs: txs, ys: tys, .. } => {
                let from = txs.len().min(tys.len());
                txs.extend_from_slice(xs);
                tys.extend_from_slice(ys);
                b2_seen_xy(&mut self.meta[id].bounds, txs, tys, from);
                Ok(())
            }
            Trace::Bar2d { xs: txs, heights, .. } => {
                txs.extend_from_slice(xs);
                heights.extend_from_slice(ys);
                // One appended x can narrow the min gap that sizes every bar,
                // so bounds and the cached halfwidth recompute in full — but
                // only the visibility flag survives the rebuild.
                let visible = self.meta[id].visible;
                self.meta[id] = compute_meta(&self.traces[id]);
                self.meta[id].visible = visible;
                Ok(())
            }
            // Structural kinds are named once, on the trace itself, so this
            // path cannot drift from what the bindings report.
            t if t.is_structural() => Err(TraceError::Structural),
            _ => Err(TraceError::WrongKind),
        }
    }

    /// Append points to an existing 3D scatter or line trace. O(delta).
    /// Appending to a scatter that is not the last node-bearing trace shifts
    /// the flat node index of every node after it by the number of appended
    /// points; `selected`/`hovered` are remapped here so highlights keep
    /// pointing at the same node — hosts holding their own flat indices must
    /// do the same.
    pub fn extend_pts(&mut self, id: TraceId, pts: &[[f32; 3]]) -> Result<(), TraceError> {
        self.resync_meta();
        let t = self.traces.get_mut(id).ok_or(TraceError::UnknownTrace)?;
        match t {
            Trace::Scatter3d { pts: tpts, .. } => {
                tpts.extend_from_slice(pts);
                // Flat node indices at/after the end of this trace's block
                // (computed pre-extend) move up by delta.
                let boundary: usize = self.meta[..=id].iter().map(|m| m.node_len).sum();
                let delta = pts.len();
                for el in [&mut self.selected, &mut self.hovered] {
                    if let Some(Element::Node(n)) = el {
                        if *n >= boundary {
                            *n += delta;
                        }
                    }
                }
                let m = &mut self.meta[id];
                b3_seen_all(&mut m.bounds, pts);
                m.node_len += delta;
                m.vert_len += delta;
                Ok(())
            }
            Trace::Line3d { pts: tpts, .. } => {
                tpts.extend_from_slice(pts);
                let m = &mut self.meta[id];
                m.vert_len += b3_seen_finite(&mut m.bounds, pts);
                Ok(())
            }
            Trace::Graph3d { .. } | Trace::Surface3d { .. } | Trace::Mesh3d { .. } => {
                Err(TraceError::Structural)
            }
            _ => Err(TraceError::WrongKind),
        }
    }

    /// Move every node of a graph trace at once — the per-frame call of a
    /// force-directed layout. Structure is untouched, so flat node/edge
    /// indices (and with them `selected`/`hovered` and any host-held
    /// indices) stay valid. `positions.len()` must equal the node count.
    /// O(n): bounds recompute in full, because moving nodes can shrink the
    /// box and the incremental union only widens.
    ///
    /// A 2D graph takes the same `[f32; 3]` list with its z dropped, so one
    /// layout-step call site drives either dimension and the FFI, Python, Go
    /// and JavaScript entry points need no second signature.
    pub fn set_graph_positions(
        &mut self,
        id: TraceId,
        positions: Vec<[f32; 3]>,
    ) -> Result<(), TraceError> {
        self.resync_meta();
        let t = self.traces.get_mut(id).ok_or(TraceError::UnknownTrace)?;
        match t {
            Trace::Graph3d { nodes, .. } => {
                if positions.len() != nodes.len() {
                    return Err(TraceError::LengthMismatch);
                }
                *nodes = positions;
                self.rebuild_meta(id);
                Ok(())
            }
            Trace::Graph2d { nodes, .. } => {
                if positions.len() != nodes.len() {
                    return Err(TraceError::LengthMismatch);
                }
                *nodes = positions.into_iter().map(|p| [p[0], p[1]]).collect();
                self.rebuild_meta(id);
                Ok(())
            }
            _ => Err(TraceError::WrongKind),
        }
    }

    /// Replace a 2D graph's edge waypoints — the second half of a relayout,
    /// after [`Self::set_graph_positions`] has moved the nodes. `route_starts`
    /// carries one entry per edge (CSR, the shape
    /// [`LayeredLayout::routes`](crate::LayeredLayout::routes) returns);
    /// empty lists restore straight edges.
    pub fn set_graph_routes(
        &mut self,
        id: TraceId,
        pts: Vec<[f32; 2]>,
        starts: Vec<u32>,
    ) -> Result<(), TraceError> {
        self.resync_meta();
        let t = self.traces.get_mut(id).ok_or(TraceError::UnknownTrace)?;
        match t {
            Trace::Graph2d { edges, route_pts, route_starts, .. } => {
                if !starts.is_empty() && starts.len() != edges.len() {
                    return Err(TraceError::LengthMismatch);
                }
                *route_pts = pts;
                *route_starts = starts;
                // Waypoints are geometry, so the frame can shrink around them
                // the way it does when nodes move.
                self.rebuild_meta(id);
                Ok(())
            }
            _ => Err(TraceError::WrongKind),
        }
    }

    /// Rescan trace `id` after its geometry changed in place, keeping the
    /// host's view flags. Both `visible` and `muted` are restored: they are
    /// answers about the *host's* intent, and a relayout is not a reason to
    /// forget either of them.
    fn rebuild_meta(&mut self, id: TraceId) {
        let (visible, muted) = (self.meta[id].visible, self.meta[id].muted);
        self.meta[id] = compute_meta(&self.traces[id]);
        self.meta[id].visible = visible;
        self.meta[id].muted = muted;
    }

    /// Recolor a graph trace in place — the host-side highlight primitive
    /// (dim everything, brighten a hovered dependency path, restore).
    /// `node_colors.len()` must equal the node count; `edge_colors`, when
    /// given, must have one color per edge, and `None` restores the default
    /// dimmed endpoint-average edge color. Geometry is untouched, so no
    /// bounds work happens.
    pub fn set_graph_colors(
        &mut self,
        id: TraceId,
        colors: Vec<Rgb>,
        new_edge_colors: Option<Vec<Rgb>>,
    ) -> Result<(), TraceError> {
        self.resync_meta();
        let t = self.traces.get_mut(id).ok_or(TraceError::UnknownTrace)?;
        match t {
            Trace::Graph3d { nodes, edges, node_colors, edge_colors, .. } => {
                if colors.len() != nodes.len() {
                    return Err(TraceError::LengthMismatch);
                }
                if let Some(ec) = &new_edge_colors {
                    if ec.len() != edges.len() {
                        return Err(TraceError::LengthMismatch);
                    }
                }
                *node_colors = colors;
                *edge_colors = new_edge_colors;
                Ok(())
            }
            Trace::Graph2d { nodes, edges, node_colors, edge_colors, .. } => {
                if colors.len() != nodes.len() {
                    return Err(TraceError::LengthMismatch);
                }
                if let Some(ec) = &new_edge_colors {
                    if ec.len() != edges.len() {
                        return Err(TraceError::LengthMismatch);
                    }
                }
                *node_colors = colors;
                *edge_colors = new_edge_colors;
                Ok(())
            }
            _ => Err(TraceError::WrongKind),
        }
    }

    /// Append nodes and edges to a graph trace — how new nodes arrive in a
    /// live graph without a rebuild. `new_colors` colors the appended nodes
    /// (renderer default where missing); `new_edges` may reference old or
    /// new node indices. Per-node `node_sizes`/`node_shapes` overrides are
    /// not extended — appended nodes take the trace defaults. O(delta), and
    /// the same flat-index caveat as [`Self::extend_pts`]: appending to a
    /// graph that is not the last node-bearing trace shifts the flat indices
    /// of every node after it (edge indices likewise); `selected`/`hovered`
    /// are remapped here, hosts holding indices must do the same.
    ///
    /// `new_labels` names the appended nodes of a [`Trace::Graph2d`] (missing
    /// entries give unlabelled boxes); a 3D graph has no labels and ignores
    /// it. Positions are `[f32; 3]` for both, z dropped in 2D, so one call
    /// site grows either dimension — the same rule
    /// [`Self::set_graph_positions`] follows. Appending never adds
    /// waypoints: new edges start straight, and a relayout is what routes
    /// them (see [`Self::set_graph_routes`]).
    pub fn extend_graph(
        &mut self,
        id: TraceId,
        new_nodes: &[[f32; 3]],
        new_colors: &[Rgb],
        new_edges: &[(u32, u32)],
        new_labels: Option<&[String]>,
    ) -> Result<(), TraceError> {
        self.resync_meta();
        // Flat boundaries computed pre-extend: nodes at/after the end of this
        // trace's node block move up by the node delta, edges likewise.
        let node_boundary: usize = self.meta[..=id.min(self.meta.len().saturating_sub(1))]
            .iter()
            .map(|m| m.node_len)
            .sum();
        let edge_boundary: usize = self
            .traces
            .iter()
            .take(id + 1)
            .map(|t| match t {
                Trace::Graph3d { edges, .. } | Trace::Graph2d { edges, .. } => edges.len(),
                _ => 0,
            })
            .sum();
        // Both arms shift the same indices by the same deltas, so the remap
        // is written once here rather than duplicated per dimension.
        let remap = |selected: &mut Option<Element>, hovered: &mut Option<Element>| {
            for el in [selected, hovered] {
                match el {
                    Some(Element::Node(n)) if *n >= node_boundary => *n += new_nodes.len(),
                    Some(Element::Edge(e)) if *e >= edge_boundary => *e += new_edges.len(),
                    _ => {}
                }
            }
        };
        let t = self.traces.get_mut(id).ok_or(TraceError::UnknownTrace)?;
        match t {
            Trace::Graph3d { nodes, node_colors, edges, .. } => {
                nodes.extend_from_slice(new_nodes);
                node_colors.extend_from_slice(new_colors);
                edges.extend_from_slice(new_edges);
                remap(&mut self.selected, &mut self.hovered);
                let m = &mut self.meta[id];
                b3_seen_all(&mut m.bounds, new_nodes);
                m.node_len += new_nodes.len();
                m.vert_len += new_nodes.len();
                Ok(())
            }
            Trace::Graph2d { nodes, labels, node_colors, edges, .. } => {
                nodes.extend(new_nodes.iter().map(|p| [p[0], p[1]]));
                labels.extend(
                    (0..new_nodes.len())
                        .map(|i| new_labels.and_then(|v| v.get(i)).cloned().unwrap_or_default()),
                );
                node_colors.extend_from_slice(new_colors);
                edges.extend_from_slice(new_edges);
                remap(&mut self.selected, &mut self.hovered);
                // A graph's frame pad is a share of its own extent, so the
                // union of the old box with the new points would be wrong at
                // the edges; the rescan is the only honest answer, and it is
                // O(n) rather than O(delta) for exactly that reason.
                self.rebuild_meta(id);
                Ok(())
            }
            _ => Err(TraceError::WrongKind),
        }
    }

    /// Show or hide a trace. Returns whether anything changed. A hidden trace
    /// keeps everything structural — its handle, its palette slot, its flat
    /// node/edge index block, its place in `node_count`/`vertex_count` — and
    /// is skipped only where geometry meets the frame: drawing, bounds,
    /// legend, right-axis columns, the crosshair, and picking.
    pub fn set_visible(&mut self, id: TraceId, visible: bool) -> Result<bool, TraceError> {
        self.resync_meta();
        let m = self.meta.get_mut(id).ok_or(TraceError::UnknownTrace)?;
        let changed = m.visible != visible;
        m.visible = visible;
        Ok(changed)
    }

    /// Hide or show a trace *from the legend*, keeping its row on screen —
    /// greyed out while muted, so a click can bring it back. Returns whether
    /// the trace is now shown.
    ///
    /// This is deliberately not [`Self::set_visible`]: that one takes a trace
    /// out of the plot completely, legend row and all, which is what a host
    /// staging a reveal or filtering a stream wants. Muting is what a viewer
    /// clicking the legend wants.
    pub fn set_muted(&mut self, id: TraceId, muted: bool) -> Result<bool, TraceError> {
        self.resync_meta();
        let m = self.meta.get_mut(id).ok_or(TraceError::UnknownTrace)?;
        m.muted = muted;
        Ok(!m.muted)
    }

    /// Flip [`Self::set_muted`], returning whether the trace is now shown.
    pub fn toggle_muted(&mut self, id: TraceId) -> Result<bool, TraceError> {
        self.resync_meta();
        let m = self.meta.get_mut(id).ok_or(TraceError::UnknownTrace)?;
        m.muted = !m.muted;
        Ok(!m.muted)
    }

    /// The trace whose legend row covers `(px, py)` in a render of this size,
    /// if any — the hook for a clickable legend. Hidden traces keep their row,
    /// so this is how a host offers show/hide:
    ///
    /// ```no_run
    /// # use plotui_core::Plot;
    /// # let (mut plot, w, h, x, y) = (Plot::new(), 800, 600, 0.0, 0.0);
    /// if let Some(id) = plot.legend_hit(w, h, x, y) {
    ///     plot.toggle_muted(id).ok();
    /// }
    /// ```
    pub fn legend_hit(&self, px_w: usize, px_h: usize, px: f32, py: f32) -> Option<TraceId> {
        let (x1, y0, s, three_d) = self.legend_anchor(px_w, px_h);
        self.legend_box(x1, y0, s, three_d)?.row_at(px, py)
    }

    /// Just the legend, drawn into an otherwise transparent framebuffer of
    /// this size. For hosts that drop resolution during interaction (see
    /// [`Self::render_at`]): render the geometry small, scale it up, then
    /// composite this on top, and the legend stays pixel-identical instead of
    /// changing font and weight the moment a drag starts.
    pub fn render_legend_overlay(&self, px_w: usize, px_h: usize) -> Framebuffer {
        let mut fb = Framebuffer::new(px_w, px_h);
        let (x1, y0, s, three_d) = self.legend_anchor(px_w, px_h);
        self.draw_legend(&mut fb, 0, y0, x1, s, 0.0, three_d);
        fb
    }

    /// Project every node (flat-index order, same list as [`Self::pick`])
    /// through the exact projection `render` uses. Returns screen-space
    /// `[x_px, y_px, depth]` per node — the hook for frontends that overlay
    /// text labels or steer the camera toward a node.
    ///
    /// A 2D plot goes through the axis map instead of the orbit projector and
    /// reports depth 0: there is no camera to be in front of, and a node's
    /// box centre is the anchor a tooltip wants either way.
    pub fn project_nodes(&self, px_w: usize, px_h: usize) -> Vec<[f32; 3]> {
        if !self.is_3d() {
            let l = self.layout_2d(px_w, px_h);
            let mut v = Vec::with_capacity(self.node_count());
            for t in &self.traces {
                if let Trace::Graph2d { nodes, .. } = t {
                    v.extend(nodes.iter().map(|p| {
                        [l.map.sx(p[0] as f64) as f32, l.map.sy(p[1] as f64) as f32, 0.0]
                    }));
                }
            }
            return v;
        }
        let (pr, _, _) = self.projector(px_w, px_h, 1.0);
        self.node_points().iter().map(|p| pr.project(*p)).collect()
    }

    /// The next default trace color: colorway slots assigned in fixed order
    /// by the number of traces already added.
    pub fn next_color(&self) -> Rgb {
        self.colorway[self.traces.len() % self.colorway.len()]
    }

    /// Swap the color sequence used for traces added without an explicit
    /// color from here on; colors already resolved onto existing traces keep
    /// their values. An empty list is ignored (the sequence must never be
    /// empty); bindings reject it with a shared error message first.
    pub fn set_colorway(&mut self, colors: Vec<Rgb>) {
        if !colors.is_empty() {
            self.colorway = colors;
        }
    }

    /// Explicit color, or the next palette slot in fixed order — the shared
    /// rule every binding applies to an omitted trace color.
    pub fn resolve_color(&self, color: Option<Rgb>) -> Rgb {
        color.unwrap_or_else(|| self.next_color())
    }

    pub fn add_scatter2d(
        &mut self,
        xs: Vec<f32>,
        ys: Vec<f32>,
        color: Rgb,
        size: f32,
        name: Option<String>,
        axis: YAxis,
    ) -> TraceId {
        self.push_trace(Trace::Scatter2d {
            xs,
            ys,
            color,
            size,
            colors: None,
            sizes: None,
            shapes: None,
            err_x: None,
            err_y: None,
            name,
            axis,
        })
    }

    /// [`add_scatter2d`](Self::add_scatter2d) with per-point styling. Each
    /// array is optional and independent: give colors alone for a categorical
    /// or colormapped cloud, sizes alone for a bubble chart, shapes alone for
    /// a palette-free encoding, or any combination. An array shorter than the
    /// series falls back to the uniform value for the remaining points.
    #[allow(clippy::too_many_arguments)]
    pub fn add_scatter2d_styled(
        &mut self,
        xs: Vec<f32>,
        ys: Vec<f32>,
        color: Rgb,
        size: f32,
        colors: Option<Vec<Rgb>>,
        sizes: Option<Vec<f32>>,
        shapes: Option<Vec<Shape>>,
        name: Option<String>,
        axis: YAxis,
    ) -> TraceId {
        self.push_trace(Trace::Scatter2d {
            xs,
            ys,
            color,
            size,
            colors,
            sizes,
            shapes,
            err_x: None,
            err_y: None,
            name,
            axis,
        })
    }

    /// Replace a scatter's per-point styling in place; `None` clears an array
    /// back to the trace's uniform value. The bindings set styling through
    /// this rather than through wider constructors, so the shared two-array
    /// `add_2d` path stays intact across the C ABI and Go.
    pub fn set_point_styles(
        &mut self,
        id: TraceId,
        colors: Option<Vec<Rgb>>,
        sizes: Option<Vec<f32>>,
        shapes: Option<Vec<Shape>>,
    ) -> Result<(), TraceError> {
        self.resync_meta();
        match self.traces.get_mut(id).ok_or(TraceError::UnknownTrace)? {
            Trace::Scatter2d { colors: c, sizes: sz, shapes: sh, .. } => {
                (*c, *sz, *sh) = (colors, sizes, shapes);
                Ok(())
            }
            _ => Err(TraceError::WrongKind),
        }
    }

    pub fn add_line2d(
        &mut self,
        xs: Vec<f32>,
        ys: Vec<f32>,
        color: Rgb,
        width: f32,
        name: Option<String>,
        axis: YAxis,
    ) -> TraceId {
        self.push_trace(Trace::Line2d {
            xs,
            ys,
            color,
            width,
            interp: Interp::Linear,
            err_x: None,
            err_y: None,
            name,
            axis,
        })
    }

    /// [`add_line2d`](Self::add_line2d) drawn as a step function. Use it for
    /// any series that holds its value between samples — counters, states,
    /// prices — where a straight segment would draw a transition that never
    /// happened.
    #[allow(clippy::too_many_arguments)]
    pub fn add_step2d(
        &mut self,
        xs: Vec<f32>,
        ys: Vec<f32>,
        color: Rgb,
        width: f32,
        interp: Interp,
        name: Option<String>,
        axis: YAxis,
    ) -> TraceId {
        self.push_trace(Trace::Line2d {
            xs,
            ys,
            color,
            width,
            interp,
            err_x: None,
            err_y: None,
            name,
            axis,
        })
    }

    /// Add a box plot: `values` is every group's sample concatenated, and
    /// `group_starts[g]` is where group `g` begins in it (CSR). Group `g` sits
    /// at position `g`, so [`Plot::x_categories`] (or `y_categories` when
    /// horizontal) names the boxes.
    ///
    /// Boxes span the quartiles with a median line; whiskers reach the
    /// furthest values within 1.5·IQR, and anything beyond is drawn as its own
    /// point rather than being swallowed by a longer whisker.
    pub fn add_box2d(
        &mut self,
        values: Vec<f32>,
        group_starts: Vec<u32>,
        color: Rgb,
        orient: Orient,
        name: Option<String>,
        axis: YAxis,
    ) -> TraceId {
        self.push_trace(Trace::Box2d { values, group_starts, color, orient, name, axis })
    }

    /// Add a filled band between two boundaries at each x — a confidence
    /// interval, a min/max envelope, a tolerance range.
    ///
    /// Add it *before* the line it belongs to: 2D draw order is the only
    /// layering there is, so a band added afterwards would paint over its own
    /// centre line.
    pub fn add_band2d(
        &mut self,
        xs: Vec<f32>,
        lo: Vec<f32>,
        hi: Vec<f32>,
        color: Rgb,
        name: Option<String>,
        axis: YAxis,
    ) -> TraceId {
        self.push_trace(Trace::Band2d { xs, lo, hi, color, name, axis })
    }

    /// Attach per-point uncertainty to a 2D scatter or line; `None` clears an
    /// axis's bars.
    ///
    /// Error bars belong to a series rather than being a series of their own:
    /// they take its color, they stay out of the legend, and they cannot drift
    /// out of step with the points they qualify.
    pub fn set_error_bars(
        &mut self,
        id: TraceId,
        err_x: Option<ErrBars>,
        err_y: Option<ErrBars>,
    ) -> Result<(), TraceError> {
        self.resync_meta();
        match self.traces.get_mut(id).ok_or(TraceError::UnknownTrace)? {
            Trace::Scatter2d { err_x: ex, err_y: ey, .. }
            | Trace::Line2d { err_x: ex, err_y: ey, .. } => {
                (*ex, *ey) = (err_x, err_y);
                // The bars reach past the points, so the range must grow with
                // them: rebuild this trace's cached box.
                let (visible, muted) = (self.meta[id].visible, self.meta[id].muted);
                self.meta[id] = compute_meta(&self.traces[id]);
                self.meta[id].visible = visible;
                self.meta[id].muted = muted;
                Ok(())
            }
            _ => Err(TraceError::WrongKind),
        }
    }

    /// Add a heatmap: a grid of cells coloured by value, where
    /// `zs[j * xs.len() + i]` is the value at `(xs[i], ys[j])` — the same
    /// row-major shape [`add_surface3d`](Self::add_surface3d) takes, so both
    /// share one validation rule in the bindings. Cells are centred on their
    /// coordinates and tile outward by half a step, so a regular grid meets
    /// edge to edge. A non-finite value leaves a hole rather than a zero.
    ///
    /// The ramp spans the grid's own finite range. Set [`Plot::colorbar`] to
    /// say what it means — a heatmap without one shows structure but no
    /// values.
    pub fn add_heatmap2d(
        &mut self,
        xs: Vec<f32>,
        ys: Vec<f32>,
        zs: Vec<f32>,
        colormap: Colormap,
        name: Option<String>,
    ) -> TraceId {
        self.push_trace(Trace::Heatmap2d { xs, ys, zs, colormap, name })
    }

    /// The finite value range of a heatmap trace, for sizing a colorbar
    /// against exactly what the ramp was normalized over. `None` when the
    /// trace is not a heatmap or has no finite values.
    pub fn heatmap_range(&self, id: TraceId) -> Option<(f64, f64)> {
        let Some(Trace::Heatmap2d { zs, .. }) = self.traces.get(id) else { return None };
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for v in zs.iter().filter(|v| v.is_finite()) {
            lo = lo.min(*v as f64);
            hi = hi.max(*v as f64);
        }
        lo.is_finite().then_some((lo, hi))
    }

    /// Add a histogram of `values`: the sample is binned and drawn as
    /// touching bars. The raw values are kept, so [`extend_values`] can add
    /// observations later and the crosshair can report a bin's interval and
    /// count.
    ///
    /// Bins are solved once from the whole sample and do not change with
    /// zoom. That is deliberate: bin edges that shifted while panning would
    /// change the shape of the distribution under the reader's hands, which
    /// is a different chart, not a closer look at the same one.
    pub fn add_histogram2d(
        &mut self,
        values: Vec<f32>,
        bins: BinSpec,
        color: Rgb,
        name: Option<String>,
        axis: YAxis,
    ) -> TraceId {
        self.push_trace(Trace::Histogram2d { values, bins, color, name, axis })
    }

    /// Append observations to a histogram and rebin. Unlike the coordinate
    /// traces this cannot be an O(delta) update: one new value can move the
    /// range, and every bin edge with it.
    pub fn extend_values(&mut self, id: TraceId, values: &[f32]) -> Result<(), TraceError> {
        self.resync_meta();
        match self.traces.get_mut(id).ok_or(TraceError::UnknownTrace)? {
            Trace::Histogram2d { values: v, .. } => {
                v.extend_from_slice(values);
                let (visible, muted) = (self.meta[id].visible, self.meta[id].muted);
                self.meta[id] = compute_meta(&self.traces[id]);
                self.meta[id].visible = visible;
                self.meta[id].muted = muted;
                Ok(())
            }
            _ => Err(TraceError::WrongKind),
        }
    }

    pub fn add_bar2d(
        &mut self,
        xs: Vec<f32>,
        heights: Vec<f32>,
        color: Rgb,
        name: Option<String>,
        axis: YAxis,
    ) -> TraceId {
        self.push_trace(Trace::Bar2d { xs, heights, color, orient: Orient::Vertical, name, axis })
    }

    /// [`add_bar2d`](Self::add_bar2d) with an explicit orientation. A
    /// horizontal bar swaps the roles of the two axes: `xs` are y positions,
    /// `heights` run along x, and the baseline is a vertical line at zero.
    /// Pair it with [`Plot::y_categories`] to label the rows.
    #[allow(clippy::too_many_arguments)]
    pub fn add_bar2d_oriented(
        &mut self,
        xs: Vec<f32>,
        heights: Vec<f32>,
        color: Rgb,
        orient: Orient,
        name: Option<String>,
        axis: YAxis,
    ) -> TraceId {
        self.push_trace(Trace::Bar2d { xs, heights, color, orient, name, axis })
    }

    /// The tick-label tint for right axis `k`: the color of the first trace on
    /// that axis. A right axis only exists because a trace put it there, so the
    /// neutral-ink fallback is a totality guard, not a reachable state.
    fn right_axis_color(&self, k: usize) -> Rgb {
        self.traces
            .iter()
            .enumerate()
            .find(|(i, t)| self.is_visible(*i) && t.axis().right_index() == Some(k))
            .map(|(_, t)| t.color())
            .unwrap_or(self.chrome.ink)
    }

    /// True when any trace is 3D; such plots render with the orbit camera.
    pub fn is_3d(&self) -> bool {
        self.traces.iter().any(Trace::is_3d)
    }

    /// Total pickable nodes across every trace (the flat-index space).
    /// Structural: hidden traces keep their slots and are counted.
    pub fn node_count(&self) -> usize {
        if self.meta_synced() {
            return self.meta.iter().map(|m| m.node_len).sum();
        }
        self.node_points().len()
    }

    /// All node points across every trace, in insertion order. The index into
    /// this list is the "flat node index" used by [`Self::pick`] and `selected`.
    /// Line vertices (and surface grids) are deliberately absent: they shape
    /// the plot's extent via [`Self::extent_points`] but are not hover/pick
    /// targets, so adding such traces never shifts existing node indices.
    fn node_points(&self) -> Vec<[f32; 3]> {
        let mut v = Vec::new();
        for t in &self.traces {
            match t {
                Trace::Scatter3d { pts, .. } => v.extend_from_slice(pts),
                Trace::Graph3d { nodes, .. } => v.extend_from_slice(nodes),
                // A 2D graph's nodes are pickable and share the flat index
                // space, so they are listed here too, flattened onto z = 0.
                // The 3D consumers of this list (bounds, the fog range) only
                // ever see it in plots that have no 2D graph in them.
                Trace::Graph2d { nodes, .. } => v.extend(nodes.iter().map(|p| [p[0], p[1], 0.0])),
                // Other 2D traces have no pickable nodes.
                _ => {}
            }
        }
        v
    }

    /// Every 3D point that occupies space: the pickable nodes plus the
    /// vertices of non-pickable geometry. This is what bounds and the fog
    /// depth range are computed from.
    fn extent_points(&self) -> Vec<[f32; 3]> {
        let mut v = self.node_points();
        for t in &self.traces {
            match t {
                // Non-finite vertices are gap markers, not extent.
                Trace::Line3d { pts, .. } => {
                    v.extend(pts.iter().filter(|p| p.iter().all(|c| c.is_finite())));
                }
                Trace::Surface3d { xs, ys, zs, .. } => {
                    let nx = xs.len();
                    for (j, &y) in ys.iter().enumerate() {
                        for (i, &x) in xs.iter().enumerate() {
                            if let Some(&z) = zs.get(j * nx + i) {
                                let p = [x, y, z];
                                if p.iter().all(|c| c.is_finite()) {
                                    v.push(p);
                                }
                            }
                        }
                    }
                }
                Trace::Mesh3d { verts, .. } => {
                    v.extend(verts.iter().filter(|p| p.iter().all(|c| c.is_finite())));
                }
                _ => {}
            }
        }
        v
    }

    /// Every 3D vertex that gets drawn — the load metric frontends use to
    /// decide on reduced-resolution interaction frames. Unlike
    /// [`Self::node_count`] it includes non-pickable geometry (line vertices,
    /// surface grids). Structural: hidden traces are counted. O(traces) when
    /// the meta cache is in sync, so frontends may call it per interaction.
    pub fn vertex_count(&self) -> usize {
        if self.meta_synced() {
            return self.meta.iter().map(|m| m.vert_len).sum();
        }
        self.extent_points().len()
    }

    /// `extent_points`, but without the traces hidden by `set_visible` — what
    /// visible bounds and the fog depth range are computed from.
    fn visible_extent_points(&self) -> Vec<[f32; 3]> {
        let mut v = Vec::new();
        for (i, t) in self.traces.iter().enumerate() {
            if !self.is_visible(i) {
                continue;
            }
            match t {
                Trace::Scatter3d { pts, .. } => v.extend_from_slice(pts),
                Trace::Graph3d { nodes, .. } => v.extend_from_slice(nodes),
                // Non-finite vertices are gap markers, not extent.
                Trace::Line3d { pts, .. } => {
                    v.extend(pts.iter().filter(|p| p.iter().all(|c| c.is_finite())));
                }
                Trace::Surface3d { xs, ys, zs, .. } => {
                    let nx = xs.len();
                    for (j, &y) in ys.iter().enumerate() {
                        for (i, &x) in xs.iter().enumerate() {
                            if let Some(&z) = zs.get(j * nx + i) {
                                let p = [x, y, z];
                                if p.iter().all(|c| c.is_finite()) {
                                    v.push(p);
                                }
                            }
                        }
                    }
                }
                Trace::Mesh3d { verts, .. } => {
                    v.extend(verts.iter().filter(|p| p.iter().all(|c| c.is_finite())));
                }
                _ => {}
            }
        }
        v
    }

    /// Axis-aligned bounding box of all visible data (min, max).
    fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        if let Some(fixed) = self.bounds_override {
            return fixed;
        }
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        if self.meta_synced() {
            for m in &self.meta {
                if let (true, CachedBounds::B3 { lo: tlo, hi: thi }) = (m.visible, &m.bounds) {
                    for k in 0..3 {
                        lo[k] = lo[k].min(tlo[k]);
                        hi[k] = hi[k].max(thi[k]);
                    }
                }
            }
        } else {
            for p in self.visible_extent_points() {
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
        }
        if !lo[0].is_finite() {
            lo = [-1.0; 3];
            hi = [1.0; 3];
        }
        (lo, hi)
    }

    /// Build the projector for a `px_w`×`px_h` framebuffer. `pan_scale` scales
    /// the (pixel-space) camera pan so a frame rendered at reduced resolution
    /// keeps the same *relative* layout: the zoom term rides on `px.min()`
    /// automatically, but pan is absolute pixels, so it must scale with them.
    fn projector(
        &self,
        px_w: usize,
        px_h: usize,
        pan_scale: f64,
    ) -> (Projector, [f32; 3], [f32; 3]) {
        let (lo, hi) = self.bounds();
        let center = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
        let mut extent = 0.0f32;
        for k in 0..3 {
            extent = extent.max((hi[k] - lo[k]) * 0.5);
        }
        if extent <= 0.0 {
            extent = 1.0;
        }
        let cam = self.camera;
        let scale = 0.42 * px_w.min(px_h) as f64 * cam.zoom;
        let cx = px_w as f64 * 0.5 + cam.pan_x * pan_scale;
        let cy = px_h as f64 * 0.5 + cam.pan_y * pan_scale;
        (Projector { center, inv_extent: 1.0 / extent, scale, cx, cy, cam }, lo, hi)
    }

    /// Return the flat node index nearest to screen pixel `(px, py)` within
    /// `radius` pixels, or `None`. Uses the exact projection `render` uses.
    ///
    /// In a 2D plot a node is its *box*, not a point: a hit inside the box
    /// wins outright, and `radius` is the slack outside it a terminal mouse
    /// needs, since it reports whole cells.
    pub fn pick(&self, px_w: usize, px_h: usize, px: f32, py: f32, radius: f32) -> Option<usize> {
        if !self.is_3d() {
            return self.pick_2d(px_w, px_h, px, py, radius);
        }
        let (pr, _, _) = self.projector(px_w, px_h, 1.0);
        let mut best: Option<usize> = None;
        let mut best_d2 = radius * radius;
        let mut flat = 0usize;
        for (ti, t) in self.traces.iter().enumerate() {
            let pts: &[[f32; 3]] = match t {
                Trace::Scatter3d { pts, .. } => pts,
                Trace::Graph3d { nodes, .. } => nodes,
                // 2D traces are not part of 3D node picking.
                _ => continue,
            };
            // Invisible geometry is not a pick target, but its index block
            // stays reserved so visible nodes keep their flat indices.
            if !self.is_visible(ti) {
                flat += pts.len();
                continue;
            }
            for p in pts {
                let s = pr.project(*p);
                let dx = s[0] - px;
                let dy = s[1] - py;
                let d2 = dx * dx + dy * dy;
                if d2 <= best_d2 {
                    best = Some(flat);
                    best_d2 = d2;
                }
                flat += 1;
            }
        }
        best
    }

    /// [`Self::pick`] for a 2D plot: the nearest [`Trace::Graph2d`] node box,
    /// measured by [`NodeBox::hit_d2`], sharing the flat index space with the
    /// 3D path. Hidden traces keep their block so visible nodes never shift.
    fn pick_2d(&self, px_w: usize, px_h: usize, px: f32, py: f32, radius: f32) -> Option<usize> {
        let l = self.layout_2d(px_w, px_h);
        let (px, py) = (px as f64, py as f64);
        let mut best: Option<usize> = None;
        let mut best_d2 = (radius as f64) * (radius as f64);
        let mut flat = 0usize;
        for (ti, t) in self.traces.iter().enumerate() {
            let Trace::Graph2d { nodes, node_shapes, .. } = t else { continue };
            if !self.is_visible(ti) {
                flat += nodes.len();
                continue;
            }
            let boxes = self.graph2d_boxes(ti, &l.map, l.s);
            for (i, p) in nodes.iter().enumerate() {
                if p[0].is_finite() && p[1].is_finite() {
                    let shape =
                        node_shapes.as_ref().and_then(|v| v.get(i)).copied().unwrap_or_default();
                    let d2 = boxes[i].hit_d2(shape, px, py);
                    if d2 <= best_d2 {
                        best = Some(flat);
                        best_d2 = d2;
                    }
                }
                flat += 1;
            }
        }
        best
    }

    /// [`Self::pick_edge`] for a 2D plot: the nearest point on any edge's
    /// drawn polyline, waypoints and all, so a routed edge is clickable
    /// where it is *drawn* rather than along the straight line it is not.
    fn pick_edge_2d(
        &self,
        px_w: usize,
        px_h: usize,
        px: f32,
        py: f32,
        radius: f32,
    ) -> Option<usize> {
        let l = self.layout_2d(px_w, px_h);
        let mut best: Option<usize> = None;
        let mut best_d2 = radius * radius;
        let mut flat = 0usize;
        for (ti, t) in self.traces.iter().enumerate() {
            let Trace::Graph2d { edges, .. } = t else { continue };
            if !self.is_visible(ti) {
                flat += edges.len();
                continue;
            }
            let boxes = self.graph2d_boxes(ti, &l.map, l.s);
            for e in 0..edges.len() {
                // Untrimmed: the arrowhead's footprint is part of the edge as
                // far as the eye is concerned, so the hit path runs the whole
                // way to the target box.
                if let Some(poly) = self.graph2d_edge_path(ti, &boxes, &l.map, e, 0.0) {
                    for w in poly.windows(2) {
                        let a = [w[0].0 as f32, w[0].1 as f32, 0.0];
                        let b = [w[1].0 as f32, w[1].1 as f32, 0.0];
                        let d2 = point_segment_d2(px, py, a, b);
                        if d2 <= best_d2 {
                            best = Some(flat);
                            best_d2 = d2;
                        }
                    }
                }
                flat += 1;
            }
        }
        best
    }

    /// Return the flat edge index nearest to screen pixel `(px, py)` within
    /// `radius` pixels of the projected segment, or `None`.
    pub fn pick_edge(
        &self,
        px_w: usize,
        px_h: usize,
        px: f32,
        py: f32,
        radius: f32,
    ) -> Option<usize> {
        if !self.is_3d() {
            return self.pick_edge_2d(px_w, px_h, px, py, radius);
        }
        let (pr, _, _) = self.projector(px_w, px_h, 1.0);
        let mut best: Option<usize> = None;
        let mut best_d2 = radius * radius;
        let mut flat = 0usize;
        for (ti, t) in self.traces.iter().enumerate() {
            if let Trace::Graph3d { nodes, edges, .. } = t {
                if !self.is_visible(ti) {
                    flat += edges.len();
                    continue;
                }
                for &(a, b) in edges {
                    let (a, b) = (a as usize, b as usize);
                    if a < nodes.len() && b < nodes.len() {
                        let pa = pr.project(nodes[a]);
                        let pb = pr.project(nodes[b]);
                        let d2 = point_segment_d2(px, py, pa, pb);
                        if d2 <= best_d2 {
                            best = Some(flat);
                            best_d2 = d2;
                        }
                    }
                    flat += 1;
                }
            }
        }
        best
    }

    /// The surface-grid vertex nearest to screen pixel `(px, py)` within
    /// `radius` pixels, or `None`. Surface vertices are not node-pick targets
    /// (their indices would shift the flat node order — see
    /// [`Self::pick`]), so surfaces get their own hover query returning the
    /// vertex's data coordinates instead of an index. On a near-tie between
    /// overlapping sheets the frontmost vertex wins.
    pub fn pick_surface(
        &self,
        px_w: usize,
        px_h: usize,
        px: f32,
        py: f32,
        radius: f32,
    ) -> Option<SurfaceHit> {
        let (pr, _, _) = self.projector(px_w, px_h, 1.0);
        let r2 = radius * radius;
        let mut best: Option<SurfaceHit> = None;
        let (mut best_d2, mut best_depth) = (f32::INFINITY, f32::INFINITY);
        for (ti, t) in self.traces.iter().enumerate() {
            let Trace::Surface3d { xs, ys, zs, .. } = t else { continue };
            if !self.is_visible(ti) {
                continue;
            }
            for (j, &y) in ys.iter().enumerate() {
                for (i, &x) in xs.iter().enumerate() {
                    let Some(&z) = zs.get(j * xs.len() + i) else { continue };
                    if !z.is_finite() {
                        continue;
                    }
                    let p = [x, y, z];
                    let s = pr.project(p);
                    let dx = s[0] - px;
                    let dy = s[1] - py;
                    let d2 = dx * dx + dy * dy;
                    if d2 > r2 {
                        continue;
                    }
                    // Nearer in 2D wins; within a pixel of a tie, nearer to
                    // the camera wins, so a fold picks its visible sheet.
                    if d2 + 1.0 < best_d2 || (d2 <= best_d2 + 1.0 && s[2] < best_depth) {
                        best = Some(SurfaceHit { data: p, screen: s });
                        best_d2 = d2;
                        best_depth = s[2];
                    }
                }
            }
        }
        best
    }

    /// Set (or clear) the hovered surface point — pass a
    /// [`Self::pick_surface`] hit's `data`. Returns whether the value
    /// changed, as a repaint hint.
    pub fn set_surface_hover(&mut self, p: Option<[f32; 3]>) -> bool {
        let changed = self.surface_hover != p;
        self.surface_hover = p;
        changed
    }

    /// Pin (or clear) a surface point — the click counterpart of
    /// [`Self::set_surface_hover`]. Returns whether the value changed.
    pub fn set_surface_selected(&mut self, p: Option<[f32; 3]>) -> bool {
        let changed = self.surface_selected != p;
        self.surface_selected = p;
        changed
    }

    /// Project a data-space point with the exact projection `render` uses:
    /// `[x_px, y_px, depth]`. Hosts anchor overlays (a pinned tooltip) with
    /// this after camera changes.
    pub fn project_point(&self, px_w: usize, px_h: usize, p: [f32; 3]) -> [f32; 3] {
        let (pr, _, _) = self.projector(px_w, px_h, 1.0);
        pr.project(p)
    }

    /// Route a drag gesture through [`Self::input_map`]. `(dx, dy)` are
    /// pointer deltas in whatever unit `scales` is calibrated for (pixels,
    /// cells). Sign conventions are the house defaults — dragging grabs the
    /// scene (trackball: drag right turns the object right, the camera
    /// orbiting the other way), panning follows the pointer, dragging up or
    /// left zooms in.
    pub fn apply_drag(&mut self, dx: f64, dy: f64, shift: bool, scales: DragScales) {
        let m = self.input_map;
        let ((cx, ix), (cy, iy)) = if shift {
            ((m.shift_drag_x, m.invert_shift_drag_x), (m.shift_drag_y, m.invert_shift_drag_y))
        } else {
            ((m.drag_x, m.invert_drag_x), (m.drag_y, m.invert_drag_y))
        };
        for (control, inv, d) in [(cx, ix, dx), (cy, iy, dy)] {
            let d = if inv { -d } else { d };
            match control {
                CameraControl::Yaw => self.camera.rotate(-d * scales.rotate, 0.0),
                CameraControl::Pitch => self.camera.rotate(0.0, -d * scales.rotate),
                CameraControl::PanX => self.camera.pan(d * scales.pan_x, 0.0),
                CameraControl::PanY => self.camera.pan(0.0, d * scales.pan_y),
                CameraControl::Zoom => self.camera.zoom_by((-d * scales.zoom).exp()),
                CameraControl::Off => {}
            }
        }
    }

    /// One auto-rotate step: `step` radians of yaw, turned in the direction
    /// a rightward drag pushes the object. Pass a negative `step` to drift
    /// the other way.
    ///
    /// The direction lives here rather than at each call site because it is
    /// easy to get backwards and hard to see: on a point cloud or a
    /// wireframe you cannot tell which way the scene is turning, so a spin
    /// running against [`Self::apply_drag`] goes unnoticed until something
    /// opaque is on screen. Then it reads as the *drag* being inverted —
    /// you push the object right, let go, and it walks back the way it
    /// came. `input_map.invert_drag_x` flips both together, because a spin
    /// is defined as the drag it agrees with.
    pub fn spin(&mut self, step: f64) {
        // Exactly `apply_drag(step, 0.0, false, ..rotate: 1.0)`, which the
        // test in tests/input_map.rs pins.
        self.apply_drag(
            step,
            0.0,
            false,
            DragScales { rotate: 1.0, pan_x: 0.0, pan_y: 0.0, zoom: 0.0 },
        );
    }

    /// Pick whatever is under the cursor, nodes taking priority over edges
    /// (nodes are drawn on top, so this matches what the user sees).
    pub fn pick_element(
        &self,
        px_w: usize,
        px_h: usize,
        px: f32,
        py: f32,
        node_radius: f32,
        edge_radius: f32,
    ) -> Option<Element> {
        if let Some(i) = self.pick(px_w, px_h, px, py, node_radius) {
            return Some(Element::Node(i));
        }
        self.pick_edge(px_w, px_h, px, py, edge_radius).map(Element::Edge)
    }

    /// Render one frame into an RGBA framebuffer of the given pixel size.
    /// Plots containing any 3D trace use the orbit-camera path; pure-2D plots
    /// get axes, ticks, tick labels, and a legend for named traces.
    pub fn render(&self, px_w: usize, px_h: usize) -> Framebuffer {
        self.render_at(px_w, px_h, 1.0)
    }

    /// Render at a reduced framebuffer size for the same view. `pan_scale`
    /// should equal the linear resolution ratio (rendered px / full-res px)
    /// so a downscaled-then-upscaled frame lines up with the full-res one —
    /// the basis for cheap half-res frames during interaction. Only affects
    /// the 3D orbit path; 2D plots always render at native resolution.
    pub fn render_at(&self, px_w: usize, px_h: usize, pan_scale: f64) -> Framebuffer {
        if !self.traces.is_empty() && !self.is_3d() {
            self.render_2d(px_w, px_h)
        } else {
            self.render_3d(px_w, px_h, pan_scale)
        }
    }

    fn render_3d(&self, px_w: usize, px_h: usize, pan_scale: f64) -> Framebuffer {
        let mut fb = Framebuffer::new(px_w, px_h);
        let (pr, lo, hi) = self.projector(px_w, px_h, pan_scale);

        // Depth range for fog — visible geometry only, so a hidden trace's
        // depth cannot tint the fog on what remains on screen.
        let (mut zmin, mut zmax) = (f32::INFINITY, f32::NEG_INFINITY);
        for p in self.visible_extent_points() {
            let z = pr.project(p)[2];
            zmin = zmin.min(z);
            zmax = zmax.max(z);
        }
        let zspan = (zmax - zmin).max(1e-3);
        let fog = |c: Rgb, z: f32| -> Rgb {
            let t = ((z - zmin) / zspan).clamp(0.0, 1.0) * 0.55;
            let bg = [26.0, 30.0, 44.0];
            [
                (c[0] as f32 * (1.0 - t) + bg[0] * t) as u8,
                (c[1] as f32 * (1.0 - t) + bg[1] * t) as u8,
                (c[2] as f32 * (1.0 - t) + bg[2] * t) as u8,
            ]
        };

        // Bounding-box wireframe for 3D orientation.
        if self.show_box {
            let corners: Vec<[f32; 3]> = (0..8)
                .map(|i| {
                    pr.project([
                        if i & 1 == 0 { lo[0] } else { hi[0] },
                        if i & 2 == 0 { lo[1] } else { hi[1] },
                        if i & 4 == 0 { lo[2] } else { hi[2] },
                    ])
                })
                .collect();
            let edges = [
                (0, 1),
                (2, 3),
                (4, 5),
                (6, 7),
                (0, 2),
                (1, 3),
                (4, 6),
                (5, 7),
                (0, 4),
                (1, 5),
                (2, 6),
                (3, 7),
            ];
            for (a, b) in edges {
                fb.line(corners[a], corners[b], [70, 78, 96]);
            }
        }

        let ts = (px_w as f32 / 500.0).clamp(1.0, 3.0);
        let mut flat = 0usize;
        let mut eflat = 0usize;
        for (ti, t) in self.traces.iter().enumerate() {
            // Hidden traces are not drawn but still advance the flat node and
            // edge counters, so later traces keep their index blocks.
            if !self.is_visible(ti) {
                match t {
                    Trace::Scatter3d { pts, .. } => flat += pts.len(),
                    Trace::Graph3d { nodes, edges, .. } => {
                        flat += nodes.len();
                        eflat += edges.len();
                    }
                    _ => {}
                }
                continue;
            }
            match t {
                Trace::Scatter3d { pts, color, size, .. } => {
                    for p in pts {
                        let s = pr.project(*p);
                        self.draw_node(
                            &mut fb,
                            s,
                            size * ts,
                            Shape::Disc,
                            fog(*color, s[2]),
                            *color,
                            flat,
                            ts,
                        );
                        flat += 1;
                    }
                }
                Trace::Graph3d {
                    nodes,
                    node_colors,
                    edges,
                    size,
                    node_sizes,
                    edge_colors,
                    node_shapes,
                    ..
                } => {
                    // Edges first, so nodes sit on top.
                    for (k, &(a, b)) in edges.iter().enumerate() {
                        let el = Element::Edge(eflat);
                        eflat += 1;
                        let (a, b) = (a as usize, b as usize);
                        if a < nodes.len() && b < nodes.len() {
                            let pa = pr.project(nodes[a]);
                            let pb = pr.project(nodes[b]);
                            if self.selected == Some(el) {
                                edge_glow(&mut fb, pa, pb, 1.6 * ts);
                                continue;
                            }
                            if self.hovered == Some(el) {
                                edge_glow(&mut fb, pa, pb, 1.0 * ts);
                                continue;
                            }
                            let ec = match edge_colors.as_ref().and_then(|v| v.get(k)) {
                                Some(c) => *c,
                                None => {
                                    let ca = node_colors.get(a).copied().unwrap_or([150, 150, 150]);
                                    let cb = node_colors.get(b).copied().unwrap_or([150, 150, 150]);
                                    [
                                        ((ca[0] as u16 + cb[0] as u16) / 2) as u8 / 2 + 20,
                                        ((ca[1] as u16 + cb[1] as u16) / 2) as u8 / 2 + 20,
                                        ((ca[2] as u16 + cb[2] as u16) / 2) as u8 / 2 + 20,
                                    ]
                                }
                            };
                            fb.line(pa, pb, ec);
                        }
                    }
                    for (i, p) in nodes.iter().enumerate() {
                        let s = pr.project(*p);
                        let c = node_colors.get(i).copied().unwrap_or([120, 180, 230]);
                        let r =
                            node_sizes.as_ref().and_then(|v| v.get(i)).copied().unwrap_or(*size);
                        let shape = node_shapes
                            .as_ref()
                            .and_then(|v| v.get(i))
                            .copied()
                            .unwrap_or_default();
                        self.draw_node(&mut fb, s, r * ts, shape, fog(c, s[2]), c, flat, ts);
                        flat += 1;
                    }
                }
                Trace::Surface3d { xs, ys, zs, color, colormap, wireframe, .. } => {
                    let (nx, ny) = (xs.len(), ys.len());
                    if nx < 2 || ny < 2 || zs.len() < nx * ny {
                        continue;
                    }
                    // The colormap spans this surface's own height range.
                    let (mut zlo, mut zhi) = (f32::INFINITY, f32::NEG_INFINITY);
                    for &z in zs.iter().take(nx * ny) {
                        if z.is_finite() {
                            zlo = zlo.min(z);
                            zhi = zhi.max(z);
                        }
                    }
                    let zrange = (zhi - zlo).max(1e-6);
                    // Project each grid vertex once, keeping the view-space
                    // point so facet normals are independent of zoom/pixels.
                    let vp: Vec<[f32; 3]> = (0..ny)
                        .flat_map(|j| (0..nx).map(move |i| [xs[i], ys[j], zs[j * nx + i]]))
                        .map(|p| pr.view_norm(p))
                        .collect();
                    let sp: Vec<[f32; 3]> = vp.iter().map(|&v| pr.to_screen(v)).collect();
                    // Headlight slightly up-left of the viewer, in view space;
                    // facets are two-sided so the dot product's sign is moot.
                    let light = [-0.35f32, 0.5, -0.79];
                    // Gouraud shading: color each *vertex* (its own height on
                    // the ramp, its own normal from view-space differences of
                    // its neighbors, its own fog depth) and interpolate across
                    // the face, so cells blend smoothly instead of reading as
                    // flat facets. Non-finite neighbors fall back to one-sided
                    // differences; the vertex color of a non-finite vertex is
                    // never used because its cells are skipped.
                    let idx = |i: usize, j: usize| j * nx + i;
                    let finite = |k: usize| zs[k].is_finite();
                    let vcolor: Vec<Rgb> = (0..ny)
                        .flat_map(|j| (0..nx).map(move |i| (i, j)))
                        .map(|(i, j)| {
                            let k = idx(i, j);
                            if !finite(k) {
                                return [0, 0, 0];
                            }
                            let i0 = if i > 0 && finite(idx(i - 1, j)) { i - 1 } else { i };
                            let i1 = if i + 1 < nx && finite(idx(i + 1, j)) { i + 1 } else { i };
                            let j0 = if j > 0 && finite(idx(i, j - 1)) { j - 1 } else { j };
                            let j1 = if j + 1 < ny && finite(idx(i, j + 1)) { j + 1 } else { j };
                            let du = vsub(vp[idx(i1, j)], vp[idx(i0, j)]);
                            let dv = vsub(vp[idx(i, j1)], vp[idx(i, j0)]);
                            let n = vcross(du, dv);
                            let nn = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                            // An isolated vertex has no slope to shade by.
                            let lambert = if nn > 1e-9 {
                                ((n[0] * light[0] + n[1] * light[1] + n[2] * light[2]) / nn).abs()
                            } else {
                                1.0
                            };
                            let base =
                                colormap.map_or(*color, |m| m.sample((zs[k] - zlo) / zrange));
                            fog(shade(base, 0.55 + 0.45 * lambert), sp[k][2])
                        })
                        .collect();
                    for j in 0..ny - 1 {
                        for i in 0..nx - 1 {
                            let q = [idx(i, j), idx(i + 1, j), idx(i + 1, j + 1), idx(i, j + 1)];
                            if q.iter().any(|&k| !finite(k)) {
                                continue;
                            }
                            let (c0, c1, c2, c3) =
                                (vcolor[q[0]], vcolor[q[1]], vcolor[q[2]], vcolor[q[3]]);
                            fb.tri_shaded(sp[q[0]], sp[q[1]], sp[q[2]], c0, c1, c2);
                            fb.tri_shaded(sp[q[0]], sp[q[2]], sp[q[3]], c0, c2, c3);
                        }
                    }
                    if *wireframe {
                        // Grid lines biased toward the viewer so they win depth
                        // ties against their own facets.
                        let bias = -0.01 * zspan;
                        let mut wire = |k0: usize, k1: usize| {
                            if !zs[k0].is_finite() || !zs[k1].is_finite() {
                                return;
                            }
                            let zm = (zs[k0] + zs[k1]) * 0.5;
                            let base = colormap.map_or(*color, |m| m.sample((zm - zlo) / zrange));
                            let c = fog(shade(base, 0.4), (sp[k0][2] + sp[k1][2]) * 0.5);
                            let a = [sp[k0][0], sp[k0][1], sp[k0][2] + bias];
                            let b = [sp[k1][0], sp[k1][1], sp[k1][2] + bias];
                            fb.line(a, b, c);
                        };
                        for j in 0..ny {
                            for i in 0..nx - 1 {
                                wire(j * nx + i, j * nx + i + 1);
                            }
                        }
                        for i in 0..nx {
                            for j in 0..ny - 1 {
                                wire(j * nx + i, (j + 1) * nx + i);
                            }
                        }
                    }
                }
                Trace::Line3d { pts, color, width, .. } => {
                    // Fog per segment by midpoint depth: segments are short
                    // relative to the depth range, so per-pixel fog would not
                    // read differently. Non-finite vertices break the line,
                    // matching the 2D gap convention.
                    let r = (width * 0.5 * ts).max(0.5);
                    for w in pts.windows(2) {
                        if !w.iter().all(|p| p.iter().all(|v| v.is_finite())) {
                            continue;
                        }
                        let a = pr.project(w[0]);
                        let b = pr.project(w[1]);
                        let c = fog(*color, (a[2] + b[2]) * 0.5);
                        stroke3d(&mut fb, a, b, r, c);
                    }
                }
                Trace::Mesh3d { verts, tris, color, colormap, .. } => {
                    // The colormap spans this mesh's own z range.
                    let (mut zlo, mut zhi) = (f32::INFINITY, f32::NEG_INFINITY);
                    for v in verts.iter().filter(|v| v[2].is_finite()) {
                        zlo = zlo.min(v[2]);
                        zhi = zhi.max(v[2]);
                    }
                    let zrange = (zhi - zlo).max(1e-6);
                    // Project each vertex once, keeping the view-space point
                    // so normals are independent of zoom/pixels.
                    let vp: Vec<[f32; 3]> = verts.iter().map(|&v| pr.view_norm(v)).collect();
                    let sp: Vec<[f32; 3]> = vp.iter().map(|&v| pr.to_screen(v)).collect();
                    // Drawable vertices, resolved once: both passes below run
                    // over every triangle, and re-checking three vertices ×
                    // three coordinates each time is the difference between
                    // this and the rasterizer dominating.
                    let drawable: Vec<bool> =
                        verts.iter().map(|v| v.iter().all(|c| c.is_finite())).collect();
                    let ok =
                        |t: &&[u32; 3]| t.iter().all(|&i| drawable.get(i as usize) == Some(&true));
                    // Gouraud shading as for a surface, but the normals come
                    // from the triangulation: each vertex sums the (area-
                    // weighted) normals of its incident facets, so a mesh
                    // whose vertices are shared between cells shades
                    // smoothly across them.
                    let mut normal = vec![[0.0f32; 3]; verts.len()];
                    for t in tris.iter().filter(ok) {
                        let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
                        let n = vcross(vsub(vp[b], vp[a]), vsub(vp[c], vp[a]));
                        for &k in &[a, b, c] {
                            for d in 0..3 {
                                normal[k][d] += n[d];
                            }
                        }
                    }
                    // The same two-sided headlight the surface uses.
                    let light = [-0.35f32, 0.5, -0.79];
                    let vcolor: Vec<Rgb> = (0..verts.len())
                        .map(|k| {
                            let n = normal[k];
                            let nn = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                            // A vertex no drawn triangle touches has no
                            // normal; its color is never used.
                            let lambert = if nn > 1e-9 {
                                ((n[0] * light[0] + n[1] * light[1] + n[2] * light[2]) / nn).abs()
                            } else {
                                1.0
                            };
                            let base =
                                colormap.map_or(*color, |m| m.sample((verts[k][2] - zlo) / zrange));
                            fog(shade(base, 0.55 + 0.45 * lambert), sp[k][2])
                        })
                        .collect();
                    // Either winding draws: the z-buffer resolves occlusion,
                    // so there is no backface to cull.
                    for t in tris.iter().filter(ok) {
                        let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
                        fb.tri_shaded(sp[a], sp[b], sp[c], vcolor[a], vcolor[b], vcolor[c]);
                    }
                }
                // 2D traces are not projected into a 3D scene.
                _ => {}
            }
        }
        // Surface plots hang a floor plane below the data box: a permanent
        // frame outlining the x-y plane the hover/selection shadow projects
        // onto — offset downward so the projection is visible beside the
        // surface, never hidden under its base. A flat surface has no z-span
        // to hang the plane from; fall back to a fraction of the wider
        // ground extent.
        let surface_floor = self
            .traces
            .iter()
            .enumerate()
            .any(|(ti, t)| matches!(t, Trace::Surface3d { .. }) && self.is_visible(ti))
            .then(|| {
                let zspan_d = hi[2] - lo[2];
                let gap = if zspan_d > 1e-9 {
                    0.5 * zspan_d
                } else {
                    0.15 * (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1e-9)
                };
                lo[2] - gap
            });
        if let Some(floor) = surface_floor {
            if self.show_box {
                // Scene chrome like the box wireframe: z-buffered, so the
                // surface occludes it naturally.
                let c00 = pr.project([lo[0], lo[1], floor]);
                let c10 = pr.project([hi[0], lo[1], floor]);
                let c11 = pr.project([hi[0], hi[1], floor]);
                let c01 = pr.project([lo[0], hi[1], floor]);
                for (a, b) in [(c00, c10), (c10, c11), (c11, c01), (c01, c00)] {
                    fb.line(a, b, [70, 78, 96]);
                }
            }
            // Hover and pinned-point guides: a ring at the point, its x-y
            // shadow on the floor plane, guide lines from the plane's edges
            // to the shadow parallel to the x and y axes, and the drop line
            // connecting point to shadow. Drawn on top of the scene (the
            // legend's trick) — transient feedback should never disappear
            // behind a hill. The pinned point gets the selection treatment:
            // a filled center inside the ring.
            const GUIDE_Z: f32 = -0.9e9; // near everything, behind the legend
            let guides = |fb: &mut Framebuffer, p: [f32; 3], selected: bool| {
                let flat = |q: [f32; 3]| {
                    let s = pr.project(q);
                    [s[0], s[1], GUIDE_Z]
                };
                let guide: Rgb = [96, 106, 130];
                let link: Rgb = [150, 158, 178];
                let shadow = [p[0], p[1], floor];
                fb.line(flat([lo[0], p[1], floor]), flat(shadow), guide);
                fb.line(flat([p[0], lo[1], floor]), flat(shadow), guide);
                fb.line(flat(shadow), flat(p), link);
                let sh = flat(shadow);
                fb.disc(sh[0], sh[1], GUIDE_Z, 2.0 * ts, link);
                let sp = flat(p);
                fb.mark(sp[0], sp[1], GUIDE_Z, 4.0 * ts, Shape::Ring, [245, 248, 255]);
                if selected {
                    fb.disc(sp[0], sp[1], GUIDE_Z, 1.6 * ts, [245, 248, 255]);
                }
            };
            let finite = |p: &[f32; 3]| p.iter().all(|v| v.is_finite());
            if let Some(p) = self.surface_selected.filter(finite) {
                guides(&mut fb, p, true);
            }
            if let Some(p) = self.surface_hover.filter(finite) {
                if Some(p) != self.surface_selected {
                    guides(&mut fb, p, false);
                }
            }
        }
        // Named 3D traces get the same legend as 2D, pulled to the front so
        // rotating geometry never cuts through it.
        let s = ((px_h as f32) / 240.0).round().clamp(1.0, 4.0) as i32;
        self.draw_legend(&mut fb, 0, 0, px_w as i32 - 1, s, -1.0e9, true);
        fb
    }

    /// The `(x, y)` half-extent a trace's cached bounds were built with, when
    /// the meta cache is live. Reading it back is what keeps drawn geometry
    /// and the computed range in step: both use this one number. `None` for a
    /// trace whose extent is its points, or whenever the cache is stale.
    fn cached_pad(&self, ti: usize) -> Option<(f64, f64)> {
        match self.meta.get(ti).map(|tm| &tm.bounds) {
            Some(&CachedBounds::B2 { pad, .. }) if self.meta_synced() => pad,
            _ => None,
        }
    }

    /// A histogram's bins: the cached solve when the meta cache is live (the
    /// one the range was built from), else recomputed. Same read-back
    /// discipline as [`Self::cached_pad`].
    fn hist_bins(&self, ti: usize, values: &[f32], spec: BinSpec) -> std::borrow::Cow<'_, Bins> {
        match self.meta.get(ti) {
            Some(TraceMeta { bins: Some(b), .. }) if self.meta_synced() => {
                std::borrow::Cow::Borrowed(b)
            }
            _ => std::borrow::Cow::Owned(bin_values(values, spec)),
        }
    }

    /// Where a bar trace sits among the visible bar traces it shares an axis
    /// and orientation with: `(index, count)`, in insertion order.
    ///
    /// Grouping and stacking are properties of the *set*, not of any one
    /// trace, so unlike the cached half-width this cannot come from `meta` —
    /// it has to be recomputed from the trace list, which is also what makes
    /// it correct when a trace is hidden.
    fn bar_slot(&self, ti: usize) -> (usize, usize) {
        let Some(Trace::Bar2d { orient, axis, .. }) = self.traces.get(ti) else {
            return (0, 1);
        };
        let (mut index, mut count) = (0, 0);
        for (tj, t) in self.traces.iter().enumerate() {
            if !self.is_visible(tj) {
                continue;
            }
            if let Trace::Bar2d { orient: o, axis: a, .. } = t {
                if o == orient && a == axis {
                    if tj < ti {
                        index += 1;
                    }
                    count += 1;
                }
            }
        }
        (index, count.max(1))
    }

    /// The value a stacked bar starts from: the running total of same-signed
    /// heights at this position across the visible bar traces below it.
    ///
    /// Only same-signed heights accumulate. Letting a negative cancel a
    /// positive would draw a single short bar whose length is a *net* value
    /// the reader has no way to decompose; growing both ways from the
    /// baseline keeps both contributions visible.
    ///
    /// Positions match by exact `f32` equality, the same rule the crosshair
    /// uses — series on different grids stack independently rather than
    /// silently snapping together. Cost is O(traces × bars) per bar, which is
    /// nothing at the sizes a terminal plot holds.
    fn stack_base(&self, ti: usize, pos: f32, h: f64) -> f64 {
        let Some(Trace::Bar2d { orient, axis, .. }) = self.traces.get(ti) else {
            return 0.0;
        };
        let mut base = 0.0;
        for (tj, t) in self.traces.iter().enumerate().take(ti) {
            if !self.is_visible(tj) {
                continue;
            }
            if let Trace::Bar2d { xs, heights, orient: o, axis: a, .. } = t {
                if o != orient || a != axis {
                    continue;
                }
                for i in 0..xs.len().min(heights.len()) {
                    let v = heights[i] as f64;
                    if xs[i] == pos && v.is_finite() && (v >= 0.0) == (h >= 0.0) {
                        base += v;
                    }
                }
            }
        }
        base
    }

    /// One bar's drawn extent, with the plot's [`BarMode`] applied, in
    /// (category, value) terms: `(cat_lo, cat_hi, val_lo, val_hi)`.
    ///
    /// Bounds and the renderer both read this, so a grouped bar can never be
    /// drawn into a slot the axis was not sized for.
    fn bar_geometry(&self, ti: usize, pos: f32, h: f64, hw: f64) -> (f64, f64, f64, f64) {
        let p = pos as f64;
        let (cat_lo, cat_hi) = match self.barmode {
            BarMode::Group => {
                let (index, count) = self.bar_slot(ti);
                let slot = 2.0 * hw / count as f64;
                let left = p - hw + slot * index as f64;
                (left, left + slot)
            }
            BarMode::Overlay | BarMode::Stack => (p - hw, p + hw),
        };
        let base = match self.barmode {
            BarMode::Stack => self.stack_base(ti, pos, h),
            BarMode::Overlay | BarMode::Group => 0.0,
        };
        let (a, b) = (base, base + h);
        (cat_lo, cat_hi, a.min(b), a.max(b))
    }

    /// A bar trace's drawn halfwidth: the cached value when the meta cache is
    /// live (the one bounds already used, so drawn bars and ranges can never
    /// disagree), else recomputed from the data.
    fn bar_hw(&self, ti: usize, xs: &[f32], orient: Orient) -> f64 {
        match self.cached_pad(ti) {
            // The width lives on whichever axis the bars are spaced along.
            Some((x, y)) => {
                if orient.is_horizontal() {
                    y
                } else {
                    x
                }
            }
            None => bar_halfwidth(xs) as f64,
        }
    }

    /// Data bounds over 2D traces, padded 5% per side. Bars widen the x range
    /// by their drawn width and pull their y range to the zero baseline. The x
    /// range unions every trace; each y range covers only its own axis's
    /// traces — primary first, then one `(lo, hi)` per right axis. The plot's
    /// `x_window` overrides here, the way the 3D `bounds_override`
    /// short-circuits `bounds`.
    #[allow(clippy::type_complexity)]
    fn bounds_2d(&self) -> (f64, f64, f64, f64, [(f64, f64); RIGHT_AXES]) {
        self.bounds_2d_in(self.x_window)
    }

    /// `bounds_2d` over an optional explicit x range. With `xr` set, x is
    /// returned exactly as given (the window is explicit, so no padding) and
    /// each y axis autoscales from the points whose x falls inside it — bars
    /// count when their drawn `[x-hw, x+hw]` span touches the window. The
    /// per-trace bounds cache covers whole traces only, so a windowed y is
    /// always a full scan of the visible points.
    #[allow(clippy::type_complexity)]
    fn bounds_2d_in(
        &self,
        xr: Option<(f64, f64)>,
    ) -> (f64, f64, f64, f64, [(f64, f64); RIGHT_AXES]) {
        let (mut xlo, mut xhi) = (f64::INFINITY, f64::NEG_INFINITY);
        // Index 0 is the primary axis, then the right axes in YAxis order.
        let mut ys = [(f64::INFINITY, f64::NEG_INFINITY); 1 + RIGHT_AXES];
        let (logx, logy) = (self.log_x(), self.log_y());
        if let Some((wlo, whi)) = xr {
            let mut seen = |lo: f64, hi: f64, y: f64, slot: usize| {
                if logy && slot == 0 && y <= 0.0 {
                    return;
                }
                if lo.is_finite() && y.is_finite() && hi >= wlo && lo <= whi {
                    ys[slot].0 = ys[slot].0.min(y);
                    ys[slot].1 = ys[slot].1.max(y);
                }
            };
            for (ti, t) in self.traces.iter().enumerate() {
                if !self.is_visible(ti) {
                    continue;
                }
                let slot = t.axis().right_index().map_or(0, |k| k + 1);
                match t {
                    Trace::Band2d { xs, lo, hi, .. } => {
                        for i in 0..xs.len().min(lo.len()).min(hi.len()) {
                            let x = xs[i] as f64;
                            for v in [lo[i] as f64, hi[i] as f64] {
                                seen(x, x, v, slot);
                            }
                        }
                    }
                    Trace::Box2d { values, group_starts, orient, .. } => {
                        for (g, group) in box_groups(values, group_starts).enumerate() {
                            let Some(st) = box_stats(group) else { continue };
                            let (c0, c1) = (g as f64 - BOX_HALF_WIDTH, g as f64 + BOX_HALF_WIDTH);
                            for v in st.spans() {
                                if orient.is_horizontal() {
                                    seen(v, v, c0, slot);
                                    seen(v, v, c1, slot);
                                } else {
                                    seen(c0, c1, v, slot);
                                }
                            }
                        }
                    }
                    Trace::Scatter2d { xs, ys, .. } | Trace::Line2d { xs, ys, .. } => {
                        for i in 0..xs.len().min(ys.len()) {
                            let x = xs[i] as f64;
                            seen(x, x, ys[i] as f64, slot);
                        }
                    }
                    Trace::Bar2d { xs, heights, orient, .. } => {
                        let hw = self.bar_hw(ti, xs, *orient);
                        for i in 0..xs.len().min(heights.len()) {
                            let h = heights[i] as f64;
                            if !xs[i].is_finite() || !h.is_finite() {
                                continue;
                            }
                            // Grouping narrows the slot and stacking lifts the
                            // baseline; both change the extent, so both come
                            // from the same solve the renderer uses.
                            let (c0, c1, v0, v1) = self.bar_geometry(ti, xs[i], h, hw);
                            if orient.is_horizontal() {
                                seen(v0, v1, c0, slot);
                                seen(v0, v1, c1, slot);
                            } else {
                                seen(c0, c1, v0, slot);
                                seen(c0, c1, v1, slot);
                            }
                        }
                    }
                    Trace::Histogram2d { values, bins, .. } => {
                        let b = self.hist_bins(ti, values, *bins);
                        for (i, &n) in b.counts.iter().enumerate() {
                            let (lo, hi) = b.edges(i);
                            seen(lo, hi, 0.0, slot);
                            seen(lo, hi, n as f64, slot);
                        }
                    }
                    Trace::Heatmap2d { xs, ys, zs, .. } => {
                        let (hx, hy) = self
                            .cached_pad(ti)
                            .unwrap_or_else(|| (grid_half_step(xs), grid_half_step(ys)));
                        for (j, &y) in ys.iter().enumerate() {
                            for (i, &x) in xs.iter().enumerate() {
                                let v = zs.get(j * xs.len() + i).copied().unwrap_or(f32::NAN);
                                if !v.is_finite() {
                                    continue;
                                }
                                let (x, y) = (x as f64, y as f64);
                                seen(x - hx, x + hx, y - hy, slot);
                                seen(x - hx, x + hx, y + hy, slot);
                            }
                        }
                    }
                    Trace::Graph2d { nodes, route_pts, .. } => {
                        if let Some((a, b, c, d)) = graph_extent(nodes, route_pts) {
                            seen(a, b, c, slot);
                            seen(a, b, d, slot);
                        }
                    }
                    traces_3d!() => {}
                }
            }
            let (ylo, yhi) = self.y_range.unwrap_or_else(|| pad_range(ys[0].0, ys[0].1, logy));
            return (
                wlo,
                whi,
                ylo,
                yhi,
                [pad_range(ys[1].0, ys[1].1, false), pad_range(ys[2].0, ys[2].1, false)],
            );
        }
        // A per-trace cache cannot describe a cross-trace layout: a stacked
        // bar's extent depends on the traces below it, and a grouped bar's on
        // how many share its slot. Under those modes the cache is skipped and
        // the full scan — which solves through `bar_geometry` — is the only
        // answer. Overlay, the default, keeps the fast path.
        let cross_trace_bars = self.barmode != BarMode::Overlay
            && self.traces.iter().any(|t| matches!(t, Trace::Bar2d { .. }));
        // A log axis drops the samples it has no coordinate for, and the
        // per-trace boxes were cached without knowing that, so it takes the
        // full scan too.
        if self.meta_synced() && !cross_trace_bars && !logx && !logy {
            // Union of the per-trace cached boxes — min/max is order-blind,
            // so this is bit-identical to the full scan below.
            for (m, t) in self.meta.iter().zip(&self.traces) {
                let slot = t.axis().right_index().map_or(0, |k| k + 1);
                if let (true, &CachedBounds::B2 { xlo: a, xhi: b, ylo: c, yhi: d, .. }) =
                    (m.visible, &m.bounds)
                {
                    xlo = xlo.min(a);
                    xhi = xhi.max(b);
                    ys[slot].0 = ys[slot].0.min(c);
                    ys[slot].1 = ys[slot].1.max(d);
                }
            }
        } else {
            let mut seen = |x: f64, y: f64, slot: usize| {
                // A log axis has no coordinate for zero or a negative value,
                // so such a sample does not get to vote on the range either.
                if (logx && x <= 0.0) || (logy && slot == 0 && y <= 0.0) {
                    return;
                }
                if x.is_finite() && y.is_finite() {
                    xlo = xlo.min(x);
                    xhi = xhi.max(x);
                    ys[slot].0 = ys[slot].0.min(y);
                    ys[slot].1 = ys[slot].1.max(y);
                }
            };
            for (ti, t) in self.traces.iter().enumerate() {
                if !self.is_visible(ti) {
                    continue;
                }
                let slot = t.axis().right_index().map_or(0, |k| k + 1);
                match t {
                    Trace::Band2d { xs, lo, hi, .. } => {
                        for i in 0..xs.len().min(lo.len()).min(hi.len()) {
                            let x = xs[i] as f64;
                            seen(x, lo[i] as f64, slot);
                            seen(x, hi[i] as f64, slot);
                        }
                    }
                    Trace::Box2d { values, group_starts, orient, .. } => {
                        for (g, group) in box_groups(values, group_starts).enumerate() {
                            let Some(st) = box_stats(group) else { continue };
                            let (c0, c1) = (g as f64 - BOX_HALF_WIDTH, g as f64 + BOX_HALF_WIDTH);
                            for v in st.spans() {
                                if orient.is_horizontal() {
                                    seen(v, c0, slot);
                                    seen(v, c1, slot);
                                } else {
                                    seen(c0, v, slot);
                                    seen(c1, v, slot);
                                }
                            }
                        }
                    }
                    Trace::Scatter2d { xs, ys, .. } | Trace::Line2d { xs, ys, .. } => {
                        for i in 0..xs.len().min(ys.len()) {
                            seen(xs[i] as f64, ys[i] as f64, slot);
                        }
                    }
                    Trace::Histogram2d { values, bins, .. } => {
                        let b = bin_values(values, *bins);
                        for (i, &n) in b.counts.iter().enumerate() {
                            let (lo, hi) = b.edges(i);
                            seen(lo, 0.0, slot);
                            seen(hi, n as f64, slot);
                        }
                    }
                    Trace::Heatmap2d { xs, ys, zs, .. } => {
                        let (hx, hy) = (grid_half_step(xs), grid_half_step(ys));
                        for (j, &y) in ys.iter().enumerate() {
                            for (i, &x) in xs.iter().enumerate() {
                                let v = zs.get(j * xs.len() + i).copied().unwrap_or(f32::NAN);
                                if !v.is_finite() {
                                    continue;
                                }
                                let (x, y) = (x as f64, y as f64);
                                seen(x - hx, y - hy, slot);
                                seen(x + hx, y + hy, slot);
                            }
                        }
                    }
                    Trace::Graph2d { nodes, route_pts, .. } => {
                        if let Some((a, b, c, d)) = graph_extent(nodes, route_pts) {
                            seen(a, c, slot);
                            seen(b, d, slot);
                        }
                    }
                    Trace::Bar2d { xs, heights, orient, .. } => {
                        let hw = bar_halfwidth(xs) as f64;
                        for i in 0..xs.len().min(heights.len()) {
                            let h = heights[i] as f64;
                            if !xs[i].is_finite() || !h.is_finite() {
                                continue;
                            }
                            let (c0, c1, v0, v1) = self.bar_geometry(ti, xs[i], h, hw);
                            if orient.is_horizontal() {
                                seen(v0, c0, slot);
                                seen(v1, c1, slot);
                                continue;
                            }
                            seen(c0, v0, slot);
                            seen(c1, v1, slot);
                        }
                    }
                    traces_3d!() => {}
                }
            }
        }
        let (xlo, xhi) = self.x_range.unwrap_or_else(|| pad_range(xlo, xhi, logx));
        let (ylo, yhi) = self.y_range.unwrap_or_else(|| pad_range(ys[0].0, ys[0].1, logy));
        (
            xlo,
            xhi,
            ylo,
            yhi,
            [pad_range(ys[1].0, ys[1].1, false), pad_range(ys[2].0, ys[2].1, false)],
        )
    }

    /// One x position as the axis itself would label it: a category name, a
    /// timestamp, or a plain number. The crosshair readout reads through this
    /// so its header agrees with the tick labels under it — a categorical axis
    /// that ticked "Mon, Tue, Wed" but read out "x 2" would be describing two
    /// different charts.
    fn format_x(&self, v: f64) -> String {
        if let Some(names) = &self.x_categories {
            let i = v.round();
            if i >= 0.0 && (i as usize) < names.len() && (v - i).abs() < 1e-6 {
                return names[i as usize].clone();
            }
        }
        match self.x_epoch {
            Some(base) => format_datetime(base + v),
            None => format_value(v),
        }
    }

    /// Is the x axis actually logarithmic? Names and calendars own the
    /// coordinate they sit on, so log defers to both rather than solving a
    /// scale nothing could label — there is no decade between Tue and Wed.
    fn log_x(&self) -> bool {
        self.x_log && self.x_categories.is_none() && self.x_epoch.is_none()
    }

    /// Is the primary y axis logarithmic? The right-hand axes stay linear.
    fn log_y(&self) -> bool {
        self.y_log && self.y_categories.is_none()
    }

    /// The primary y axis's ticks and their labels over the visible range.
    fn y_axis_ticks(&self, lo: f64, hi: f64, target: usize) -> (Vec<f64>, Vec<String>) {
        match &self.y_categories {
            Some(names) => category_ticks(names, lo, hi, target),
            None if self.log_y() => log_ticks(lo, hi, target),
            None => numeric_ticks(lo, hi, target),
        }
    }

    /// The x axis's ticks and their labels over the visible range: names for a
    /// categorical axis, calendar boundaries for a time axis, else the numeric
    /// ladder. Date ticks are generated in absolute epoch seconds and returned
    /// in the offset space the map works in.
    fn x_axis_ticks(&self, lo: f64, hi: f64, target: usize) -> (Vec<f64>, Vec<String>) {
        if let Some(names) = &self.x_categories {
            return category_ticks(names, lo, hi, target);
        }
        match self.x_epoch {
            Some(base) => {
                let (abs, labels) = date_ticks(base + lo, base + hi, target);
                (abs.into_iter().map(|t| t - base).collect(), labels)
            }
            None if self.x_log => log_ticks(lo, hi, target),
            None => numeric_ticks(lo, hi, target),
        }
    }

    /// Solve the 2D frame: margins, plot rect, per-axis maps, ticks and their
    /// labels. Pure with respect to the frame size, so the renderer and any
    /// geometry consumer agree by construction.
    fn layout_2d(&self, px_w: usize, px_h: usize) -> Layout2d {
        // Match Framebuffer::new's 1-pixel floor so degenerate frames solve
        // the same geometry the renderer draws into.
        let (w, h) = (px_w.max(1) as i32, px_h.max(1) as i32);
        let s = ((h as f32) / 240.0).round().clamp(1.0, 4.0) as i32;
        let (cw, ch) = (CHAR_W * s, CHAR_H * s);
        let tick_len = 2 * s;
        let pad = 3 * s;

        let (dxlo, dxhi, dylo, dyhi, dright) = self.bounds_2d();
        let (logx, logy) = (self.log_x(), self.log_y());
        let (dxlo, dxhi) = log_safe(dxlo, dxhi, logx);
        let (dylo, dyhi) = log_safe(dylo, dyhi, logy);
        // The window is the view: an explicit x range and the pixel-space
        // camera transform would fight over what `inv_x` means (a handle drag
        // through a zoomed map lands somewhere else), so a set window
        // supersedes the camera's 2D zoom/pan entirely.
        let flat = Camera::default();
        let cam = if self.x_window.is_some() { &flat } else { &self.camera };
        // A right axis exists only where a trace declared it; per-axis flags
        // drive the column layout below (y2 innermost, y3 outermost,
        // compacting so a y3-only plot uses the inner slot).
        let mut has_right = [false; RIGHT_AXES];
        for (ti, t) in self.traces.iter().enumerate() {
            if let (true, Some(k)) = (self.is_visible(ti), t.axis().right_index()) {
                has_right[k] = true;
            }
        }

        // Two passes: the side margins depend on y tick label widths, which
        // depend on the visible ranges, which depend on the margins.
        // The colorbar's ticks span a fixed data range, so unlike the axes
        // they do not depend on the margins — precomputing them here keeps the
        // fixed point below settling in two passes.
        let cbar_w = 3 * s; // gradient strip width
        let (cbar_ticks, cbar_labels) = match &self.colorbar {
            Some(cb) => numeric_ticks(cb.lo, cb.hi, ((h / (4 * ch)) as usize).clamp(2, 8)),
            None => (Vec::new(), Vec::new()),
        };
        let cbar_reserve = self.colorbar.as_ref().map_or(0, |_| {
            let lw = cbar_labels.iter().map(|t| text_width(t, s)).max().unwrap_or(cw);
            // Every gap `draw_colorbar` actually walks, in its order: the gap
            // to the strip, the strip, its tick mark, the gap to the labels,
            // and the widest label. Missing the tick here clipped the last
            // digit off the top label.
            pad + cbar_w + tick_len + pad + lw
        });
        // A caption has nowhere to go but above the strip: there is no rotated
        // text, so the frame gives up a line of top margin for it.
        let caption = self.colorbar.as_ref().and_then(|cb| cb.label.clone());
        // Without chrome there is no tick-label column to reserve on any
        // side, so every margin collapses to the same small air gap and the
        // graph gets the whole frame.
        let hidden = self.chrome_hidden();
        // A graph's boxes reach past their centres by a pixel amount no
        // data-space pad can know; the map below gives them that room.
        let (box_hw, box_hh) = self.graph_box_inset(s);
        let legend_w = if box_hw > 0 { self.legend_width(s) } else { 0 };
        // Titles are the first thing a cramped frame gives up. Each one is
        // taken only where the margin it asks for stays inside the third of
        // the frame the clamps below allow the chrome — otherwise it would be
        // handed the tick labels' own pixels. A title outlives `hidden`,
        // though: ticks are chrome the frame puts there on its own, while a
        // title is something the caller asked for by name.
        let line = ch + pad;
        let base_bottom = if hidden { 2 * pad } else { ch + tick_len + 2 * pad };
        let title_on = self.title.is_some() && 3 * (2 * pad + line) <= h;
        let xtitle_on = self.x_title.is_some() && 3 * (base_bottom + line) <= h;
        let ytitle_on = self.y_title.is_some() && 3 * (4 * cw + tick_len + 2 * pad + line) <= w;
        let top =
            2 * pad + if title_on { line } else { 0 } + caption.as_ref().map_or(0, |_| ch + pad);
        let bottom = base_bottom + if xtitle_on { line } else { 0 };
        // The strip reserve is a pure function of `s` and `h`, decided before
        // the margin fixed-point below — a reserve that changed inside the
        // loop could keep it from settling in two passes.
        let strip_on = self.range_slider && h >= STRIP_MIN_H * s;
        let strip_h = STRIP_H_S * s;
        let strip_reserve = if strip_on { strip_h + pad } else { 0 };
        let ytitle_reserve = if ytitle_on { line } else { 0 };
        let mut left =
            if hidden { 2 * pad + ytitle_reserve } else { (8 * cw + ytitle_reserve).min(w / 3) };
        let mut right = 2 * pad;
        let (mut x0, mut y0, mut x1, mut y1) = (0, 0, 0, 0);
        let mut map = Map2d::default();
        let (mut xticks, mut xlabels) = (Vec::new(), Vec::<String>::new());
        let (mut yticks, mut ylabels) = (Vec::new(), Vec::<String>::new());
        let mut maps_r = [Map2d::default(); RIGHT_AXES];
        let mut rticks: [Vec<f64>; RIGHT_AXES] = [Vec::new(), Vec::new()];
        let mut rlabels: [Vec<String>; RIGHT_AXES] = [Vec::new(), Vec::new()];
        let mut col_x = [0; RIGHT_AXES]; // label column offset from x1
        let mut cbar_x = 0; // gradient strip offset from x1
        for _ in 0..2 {
            x0 = left;
            y0 = top;
            x1 = (w - 1 - right).max(x0 + 4);
            y1 = (h - 1 - (bottom.min(h / 3) + strip_reserve)).max(y0 + 4);
            let rect = (x0 as f64, y0 as f64, x1 as f64, y1 as f64);
            // Widen the data range by whatever the widest node box needs, so
            // the *centres* land inside the plot rect with room for their
            // boxes. Solved against the rect rather than folded into bounds
            // because the answer is in pixels and only the frame has those.
            // The two ends take separate insets: the legend eats into the
            // high-x end only.
            // Widened in the axis's own scale space: a pixel is a slice of a
            // number on a linear axis and a fraction of a decade on a log
            // one, and the inset is a pixel count either way.
            let room = |lo: f64, hi: f64, span_px: f64, at_lo: i32, at_hi: i32, log: bool| {
                let usable = span_px - (at_lo + at_hi) as f64;
                if (at_lo | at_hi) == 0 || usable <= 1.0 || hi <= lo {
                    return (lo, hi);
                }
                let (slo, shi) = (Map2d::to_scale(lo, log), Map2d::to_scale(hi, log));
                let per_px = (shi - slo) / usable;
                (
                    Map2d::from_scale(slo - per_px * at_lo as f64, log),
                    Map2d::from_scale(shi + per_px * at_hi as f64, log),
                )
            };
            let (mxlo, mxhi) = room(dxlo, dxhi, (x1 - x0) as f64, box_hw, box_hw + legend_w, logx);
            let (mylo, myhi) = room(dylo, dyhi, (y1 - y0) as f64, box_hh, box_hh, logy);
            map = Map2d::new((mxlo, mxhi, mylo, myhi), rect, cam, (logx, logy));
            // Ticks cover what is actually visible after zoom/pan.
            let (vxlo, vxhi) = (map.inv_x(x0 as f64), map.inv_x(x1 as f64));
            let (vylo, vyhi) = (map.inv_y(y1 as f64), map.inv_y(y0 as f64));
            let tx = (((x1 - x0) / (10 * cw)) as usize).clamp(2, 12);
            let ty = (((y1 - y0) / (3 * ch)) as usize).clamp(2, 10);
            (xticks, xlabels) = self.x_axis_ticks(vxlo, vxhi, tx);
            (yticks, ylabels) = self.y_axis_ticks(vylo, vyhi, ty);
            if hidden {
                // Solved and then dropped rather than skipped: the tick
                // *positions* are what size the margins, and clearing them
                // here is what makes "no chrome" one decision instead of a
                // condition repeated at every draw site.
                (xticks, xlabels) = (Vec::new(), Vec::new());
                (yticks, ylabels) = (Vec::new(), Vec::new());
            }
            let label_w = ylabels.iter().map(|t| text_width(t, s)).max().unwrap_or(cw);
            left = if hidden {
                2 * pad + ytitle_reserve
            } else {
                (label_w + tick_len + 2 * pad + ytitle_reserve).min(w / 3)
            };
            // The right margin stacks outward from x1: tick-label columns
            // first (innermost axis nearest the frame), then the colorbar.
            let mut used = 2 * pad;
            if has_right.iter().any(|b| *b) {
                let mut off = tick_len + pad; // x1 → first label column
                for k in 0..RIGHT_AXES {
                    if !has_right[k] {
                        continue;
                    }
                    let (rlo, rhi) = dright[k];
                    maps_r[k] = Map2d::new((mxlo, mxhi, rlo, rhi), rect, cam, (logx, false));
                    let (vlo, vhi) = (maps_r[k].inv_y(y1 as f64), maps_r[k].inv_y(y0 as f64));
                    let (t, labels) = numeric_ticks(vlo, vhi, ty);
                    (rticks[k], rlabels[k]) =
                        if hidden { (Vec::new(), Vec::new()) } else { (t, labels) };
                    let wk = rlabels[k].iter().map(|t| text_width(t, s)).max().unwrap_or(cw);
                    col_x[k] = off;
                    off += wk + 2 * pad;
                }
                used = if hidden { 2 * pad } else { off - pad };
            }
            cbar_x = used + pad;
            right = (used + cbar_reserve).min(w / 3);
        }
        // The strip sits at the very bottom, below the x tick labels, and
        // shows the full extent whatever the window — its maps never depend
        // on `x_window`, so drags read a stable pixel↔data scale from it.
        let strip = if strip_on {
            let (flo, fhi, fylo, fyhi, fright) = self.bounds_2d_in(None);
            let (flo, fhi) = log_safe(flo, fhi, logx);
            let (fylo, fyhi) = log_safe(fylo, fyhi, logy);
            let sy1 = h - 1 - pad;
            let sy0 = sy1 - strip_h;
            let rect = (x0 as f64, sy0 as f64, x1 as f64, sy1 as f64);
            let smap = Map2d::new((flo, fhi, fylo, fyhi), rect, &flat, (logx, logy));
            let mut smaps_r = [Map2d::default(); RIGHT_AXES];
            for (k, sm) in smaps_r.iter_mut().enumerate() {
                if has_right[k] {
                    *sm = Map2d::new(
                        (flo, fhi, fright[k].0, fright[k].1),
                        rect,
                        &flat,
                        (logx, false),
                    );
                }
            }
            let (wx0, wx1) = match self.x_window {
                Some((lo, hi)) => (smap.sx(lo), smap.sx(hi)),
                None => (x0 as f64, x1 as f64),
            };
            Some(StripLayout {
                x0,
                y0: sy0,
                x1,
                y1: sy1,
                map: smap,
                maps_r: smaps_r,
                full: (flo, fhi),
                wx0,
                wx1,
            })
        } else {
            None
        };
        Layout2d {
            s,
            x0,
            y0,
            x1,
            y1,
            map,
            maps_r,
            xticks,
            xlabels,
            yticks,
            ylabels,
            rticks,
            rlabels,
            col_x,
            has_right,
            strip,
            title: self.title.clone().filter(|_| title_on),
            x_title: self.x_title.clone().filter(|_| xtitle_on),
            y_title: self.y_title.clone().filter(|_| ytitle_on),
            cbar: self.colorbar.as_ref().map(|_| CbarLayout {
                x0: x1 + cbar_x,
                y0,
                x1: x1 + cbar_x + cbar_w,
                y1,
                ticks: cbar_ticks,
                labels: cbar_labels,
            }),
        }
    }

    fn render_2d(&self, px_w: usize, px_h: usize) -> Framebuffer {
        let mut fb = Framebuffer::new(px_w, px_h);
        let w = fb.w as i32;
        let l = self.layout_2d(px_w, px_h);
        let (s, x0, y0, x1, y1) = (l.s, l.x0, l.y0, l.x1, l.y1);
        let (map, maps_r) = (l.map, l.maps_r);
        let ch = CHAR_H * s;
        let tick_len = 2 * s;
        let pad = 3 * s;

        // Grid first, then data (clipped), then frame/labels, then legend:
        // ties in the z-buffer resolve to the later draw, so order is layering.
        // Horizontal lines only: the reader compares values, so y levels get
        // guides; x positions are carried by the tick labels alone.
        for v in &l.yticks {
            let py = map.sy(*v).round() as i32;
            if py > y0 && py < y1 {
                fb.rect_fill(x0, py, x1, py, 0.0, self.chrome.grid);
            }
        }

        fb.set_clip(x0 + 1, y0 + 1, x1 - 1, y1 - 1);
        // With an x window, the maps can throw off-window geometry thousands
        // of pixels out; primitives iterate their pixel span before the clip
        // rejects each write, so windowed draws pre-clip in pixel space. The
        // box is padded by the mark radius: a mark centered just outside can
        // still touch the plot area.
        let win = self.x_window.is_some();
        let pix_box = |m: f64| (x0 as f64 - m, y0 as f64 - m, x1 as f64 + m, y1 as f64 + m);
        for (ti, t) in self.traces.iter().enumerate() {
            if !self.is_visible(ti) {
                continue;
            }
            // Every coordinate of a series goes through its own axis's map.
            let m = match t.axis().right_index() {
                Some(k) => &maps_r[k],
                None => &map,
            };
            match t {
                Trace::Heatmap2d { xs, ys, zs, colormap, .. } => {
                    // The ramp spans this grid's own finite range, matching how
                    // a colormapped surface normalizes against its own heights.
                    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
                    for v in zs.iter().filter(|v| v.is_finite()) {
                        lo = lo.min(*v as f64);
                        hi = hi.max(*v as f64);
                    }
                    let span = hi - lo;
                    let (hx, hy) = self
                        .cached_pad(ti)
                        .unwrap_or_else(|| (grid_half_step(xs), grid_half_step(ys)));
                    let (cx0, cy0, cx1, cy1) = pix_box(1.0);
                    for (j, &yv) in ys.iter().enumerate() {
                        for (i, &xv) in xs.iter().enumerate() {
                            let Some(&v) = zs.get(j * xs.len() + i) else { continue };
                            // A non-finite cell is a hole, not a zero.
                            if !v.is_finite() || !xv.is_finite() || !yv.is_finite() {
                                continue;
                            }
                            let t = if span > 0.0 { ((v as f64 - lo) / span) as f32 } else { 0.5 };
                            let (x, y) = (xv as f64, yv as f64);
                            let (mut fx0, mut fx1) = (m.sx(x - hx), m.sx(x + hx));
                            let (mut fy0, mut fy1) = (m.sy(y + hy), m.sy(y - hy));
                            if win {
                                if fx1 < cx0 || fx0 > cx1 {
                                    continue;
                                }
                                (fx0, fx1) = (fx0.max(cx0), fx1.min(cx1));
                                (fy0, fy1) = (fy0.clamp(cy0, cy1), fy1.clamp(cy0, cy1));
                            }
                            fb.rect_fill(
                                fx0.round() as i32,
                                fy0.round() as i32,
                                fx1.round() as i32,
                                fy1.round() as i32,
                                0.0,
                                colormap.sample(t),
                            );
                        }
                    }
                }
                Trace::Box2d { values, group_starts, color, orient, .. } => {
                    let horiz = orient.is_horizontal();
                    let hw = BOX_HALF_WIDTH;
                    let thin = s.max(1);
                    // Solved once in (category, value) terms, then swapped
                    // into pixels — the same trick the bar arm uses.
                    let to_px = |cat: f64, val: f64| {
                        if horiz {
                            (m.sx(val), m.sy(cat))
                        } else {
                            (m.sx(cat), m.sy(val))
                        }
                    };
                    for (g, group) in box_groups(values, group_starts).enumerate() {
                        let Some(st) = box_stats(group) else { continue };
                        let cat = g as f64;
                        let (c0, c1) = (cat - hw, cat + hw);
                        // The IQR box, dimmed so the median reads over it.
                        let (ax, ay) = to_px(c0, st.q1);
                        let (bx, by) = to_px(c1, st.q3);
                        fb.rect_fill(
                            ax.round() as i32,
                            ay.round() as i32,
                            bx.round() as i32,
                            by.round() as i32,
                            0.0,
                            shade(*color, 0.55),
                        );
                        // Whiskers out to the fence ends, capped.
                        for (from, to) in [(st.q1, st.lo), (st.q3, st.hi)] {
                            let (fx, fy) = to_px(cat, from);
                            let (tx, ty) = to_px(cat, to);
                            fb.rect_fill(
                                fx.round() as i32,
                                fy.round() as i32,
                                tx.round() as i32,
                                ty.round() as i32,
                                0.0,
                                *color,
                            );
                            let (kx0, ky0) = to_px(cat - hw * 0.5, to);
                            let (kx1, ky1) = to_px(cat + hw * 0.5, to);
                            fb.rect_fill(
                                kx0.round() as i32,
                                ky0.round() as i32,
                                kx1.round() as i32,
                                ky1.round() as i32,
                                0.0,
                                *color,
                            );
                        }
                        // The median last and at full colour: it is the one
                        // number a reader takes away, so it must not be a
                        // subtle edge of the fill.
                        let (mx0, my0) = to_px(c0, st.median);
                        let (mx1, my1) = to_px(c1, st.median);
                        let (x0, y0) = (mx0.round() as i32, my0.round() as i32);
                        let (mut x1, mut y1) = (mx1.round() as i32, my1.round() as i32);
                        if horiz {
                            x1 = x0 + thin - 1;
                        } else {
                            y1 = y0 + thin - 1;
                        }
                        fb.rect_fill(x0, y0, x1, y1, 0.0, *color);
                        // Outliers as their own marks — the reason a box plot
                        // beats a min/max range.
                        for o in &st.outliers {
                            let (ox, oy) = to_px(cat, *o);
                            fb.disc(ox as f32, oy as f32, 0.0, 1.6 * s as f32, *color);
                        }
                    }
                }
                Trace::Band2d { xs, lo, hi, color, .. } => {
                    let n = xs.len().min(lo.len()).min(hi.len());
                    let cols: Vec<(f64, f64, f64)> = (0..n)
                        .map(|i| {
                            // A non-finite edge breaks the ribbon, the way it
                            // breaks a line; `fill_between` skips those spans.
                            (m.sx(xs[i] as f64), m.sy(lo[i] as f64), m.sy(hi[i] as f64))
                        })
                        .collect();
                    fb.fill_between(&cols, 0.0, *color);
                }
                Trace::Scatter2d {
                    xs,
                    ys,
                    color,
                    size,
                    colors,
                    sizes,
                    shapes,
                    err_x,
                    err_y,
                    ..
                } => {
                    // Bars first, so a mark is never bisected by its own spine.
                    for i in 0..xs.len().min(ys.len()) {
                        let (x, y) = (xs[i] as f64, ys[i] as f64);
                        if x.is_finite() && y.is_finite() {
                            let (ex, ey) = (
                                err_x.as_ref().and_then(|e| e.at(i)),
                                err_y.as_ref().and_then(|e| e.at(i)),
                            );
                            if ex.is_some() || ey.is_some() {
                                let c = colors
                                    .as_ref()
                                    .and_then(|v| v.get(i))
                                    .copied()
                                    .unwrap_or(*color);
                                draw_error_bars(&mut fb, m, x, y, ex, ey, s, c);
                            }
                        }
                    }
                    // The pixel box is sized by the largest mark any point can
                    // take, so a windowed plot cannot clip away a big point
                    // whose centre sits just outside the frame.
                    let max_size =
                        sizes.as_ref().map_or(*size, |v| v.iter().copied().fold(*size, f32::max));
                    let (bx0, by0, bx1, by1) = pix_box((max_size * s as f32) as f64 + 1.0);
                    for i in 0..xs.len().min(ys.len()) {
                        let (px, py) = (m.sx(xs[i] as f64), m.sy(ys[i] as f64));
                        if !px.is_finite()
                            || !py.is_finite()
                            || (win && (px < bx0 || px > bx1 || py < by0 || py > by1))
                        {
                            continue;
                        }
                        let c = colors.as_ref().and_then(|v| v.get(i)).copied().unwrap_or(*color);
                        let r = sizes.as_ref().and_then(|v| v.get(i)).copied().unwrap_or(*size)
                            * s as f32;
                        match shapes.as_ref().and_then(|v| v.get(i)).copied() {
                            Some(sh) => fb.mark(px as f32, py as f32, 0.0, r, sh, c),
                            None => fb.disc(px as f32, py as f32, 0.0, r, c),
                        }
                    }
                }
                Trace::Line2d { xs, ys, color, width, interp, err_x, err_y, .. } => {
                    for i in 0..xs.len().min(ys.len()) {
                        let (x, y) = (xs[i] as f64, ys[i] as f64);
                        if x.is_finite() && y.is_finite() {
                            let (ex, ey) = (
                                err_x.as_ref().and_then(|e| e.at(i)),
                                err_y.as_ref().and_then(|e| e.at(i)),
                            );
                            if ex.is_some() || ey.is_some() {
                                draw_error_bars(&mut fb, m, x, y, ex, ey, s, *color);
                            }
                        }
                    }
                    let n = xs.len().min(ys.len());
                    let pts: Vec<Option<(f64, f64)>> = (0..n)
                        .map(|i| {
                            let (px, py) = (m.sx(xs[i] as f64), m.sy(ys[i] as f64));
                            (px.is_finite() && py.is_finite()).then_some((px, py))
                        })
                        .collect();
                    let r = (width * s as f32 * 0.5).max(0.5);
                    let clip_box = pix_box(r as f64 + 1.0);
                    for pair in pts.windows(2) {
                        if let [Some(a), Some(b)] = pair {
                            // A step expands one segment into the two or three
                            // that trace its right-angle path; `Linear` yields
                            // the single original segment.
                            let mut from = *a;
                            for leg in interp.corners(*a, *b).into_iter().flatten().chain([*b]) {
                                if win {
                                    if let Some((ca, cb)) = clip_segment(from, leg, clip_box) {
                                        stroke(&mut fb, ca, cb, r, *color);
                                    }
                                } else {
                                    stroke(&mut fb, from, leg, r, *color);
                                }
                                from = leg;
                            }
                        }
                    }
                }
                Trace::Histogram2d { values, bins, color, .. } => {
                    let b = self.hist_bins(ti, values, *bins);
                    let base = m.sy(0.0);
                    let (cx0, cy0, cx1, cy1) = pix_box(1.0);
                    for (i, &n) in b.counts.iter().enumerate() {
                        if n == 0 {
                            continue;
                        }
                        let (lo, hi) = b.edges(i);
                        // Bars tile edge to edge — a histogram is a picture of
                        // a continuous range, and gaps would imply the data
                        // stops between bins.
                        let (mut fx0, mut fx1) = (m.sx(lo), m.sx(hi));
                        let (mut fy0, mut fy1) = (m.sy(n as f64), base);
                        if win {
                            if fx1 < cx0 || fx0 > cx1 {
                                continue;
                            }
                            (fx0, fx1) = (fx0.max(cx0), fx1.min(cx1));
                            (fy0, fy1) = (fy0.clamp(cy0, cy1), fy1.clamp(cy0, cy1));
                        }
                        fb.rect_fill(
                            fx0.round() as i32,
                            fy0.round() as i32,
                            fx1.round() as i32,
                            fy1.round() as i32,
                            0.0,
                            *color,
                        );
                    }
                }
                Trace::Bar2d { xs, heights, color, orient, .. } => {
                    let hw = self.bar_hw(ti, xs, *orient);
                    let horiz = orient.is_horizontal();
                    // Everything below is solved once in (category, value)
                    // terms — barmode and all — and swapped into pixels at the
                    // end, so the two orientations share one piece of logic.
                    let (cx0, cy0, cx1, cy1) = pix_box(1.0);
                    for i in 0..xs.len().min(heights.len()) {
                        let (pos, hgt) = (xs[i] as f64, heights[i] as f64);
                        if !pos.is_finite() || !hgt.is_finite() {
                            continue;
                        }
                        let (c0, c1, v0, v1) = self.bar_geometry(ti, xs[i], hgt, hw);
                        let (mut a0, mut a1, mut b0, mut b1) = if horiz {
                            // Category on y (inverted, so c1 is the top),
                            // value along x.
                            (m.sx(v0), m.sx(v1), m.sy(c1), m.sy(c0))
                        } else {
                            (m.sx(c0), m.sx(c1), m.sy(v1), m.sy(v0))
                        };
                        if win {
                            let (lo, hi) = if horiz { (a0.min(a1), a0.max(a1)) } else { (a0, a1) };
                            if hi < cx0 || lo > cx1 {
                                continue;
                            }
                            (a0, a1) = (a0.clamp(cx0, cx1), a1.clamp(cx0, cx1));
                            (b0, b1) = (b0.clamp(cy0, cy1), b1.clamp(cy0, cy1));
                        }
                        fb.rect_fill(
                            a0.round() as i32,
                            b0.round() as i32,
                            a1.round() as i32,
                            b1.round() as i32,
                            0.0,
                            *color,
                        );
                    }
                }
                Trace::Graph2d { .. } => self.draw_graph2d(&mut fb, ti, m, s),
                traces_3d!() => {}
            }
        }
        fb.clear_clip();

        // Axes and tick labels. An open L frame — the y axis and the x axis,
        // no box, no tick marks — so the chart reads like a page figure: the
        // labels alone carry the positions. Right axes share one rule at x1
        // (a second rule at the outer column would close the figure into a
        // box and anchor nothing); each column's tint says who owns it.
        // The tick lists are already empty when the chrome is hidden (see
        // `layout_2d`), so the label loops below sit out on their own; the
        // rules are the one thing that has to be asked.
        if !self.chrome_hidden() {
            fb.rect_fill(x0, y1, x1, y1, 0.0, self.chrome.frame);
            fb.rect_fill(x0, y0, x0, y1, 0.0, self.chrome.frame);
            if l.has_right.iter().any(|b| *b) {
                fb.rect_fill(x1, y0, x1, y1, 0.0, self.chrome.frame);
            }
        }
        for (v, label) in l.xticks.iter().zip(&l.xlabels) {
            let px = map.sx(*v).round() as i32;
            if px < x0 || px > x1 {
                continue;
            }
            let lw = text_width(label, s);
            let lx = (px - lw / 2).clamp(0, (w - lw).max(0));
            draw_text(&mut fb, lx, y1 + tick_len + pad, label, s, 0.0, self.chrome.ink);
        }
        for (v, label) in l.yticks.iter().zip(&l.ylabels) {
            let py = map.sy(*v).round() as i32;
            if py < y0 || py > y1 {
                continue;
            }
            let lw = text_width(label, s);
            draw_text(
                &mut fb,
                (x0 - tick_len - pad - lw).max(0),
                py - ch / 2,
                label,
                s,
                0.0,
                self.chrome.ink,
            );
        }
        // Titles last of the chrome: the chart's above the plot area, the x
        // axis's under its own tick labels, and the y axis's rotated in the
        // left margin, each centered on the side it names. They are drawn in
        // `ink_bright` — a title says what the numbers *are*, so it outranks
        // the numbers, the same way the colorbar's caption outranks its
        // ticks.
        if let Some(t) = &l.title {
            let tw = text_width(t, s);
            let tx = ((x0 + x1) / 2 - tw / 2).clamp(0, (w - tw).max(0));
            draw_text(&mut fb, tx, pad, t, s, 0.0, self.chrome.ink_bright);
        }
        if let Some(t) = &l.x_title {
            let tw = text_width(t, s);
            let tx = ((x0 + x1) / 2 - tw / 2).clamp(0, (w - tw).max(0));
            draw_text(&mut fb, tx, y1 + tick_len + 2 * pad + ch, t, s, 0.0, self.chrome.ink_bright);
        }
        if let Some(t) = &l.y_title {
            // Rotated text grows upward from its anchor, so the anchor is the
            // *bottom* of the run: half its length below the middle of the
            // axis it names.
            let tw = text_width(t, s);
            let ty = ((y0 + y1) / 2 + tw / 2).clamp(tw, fb.h as i32 - 1);
            draw_text_rot90(&mut fb, pad, ty, t, s, 0.0, self.chrome.ink_bright);
        }
        // Right-axis tick labels, one column per axis, tinted to the first
        // trace on that axis — two unlabeled number columns are otherwise
        // unattributable.
        for (k, mr) in maps_r.iter().enumerate() {
            if !l.has_right[k] {
                continue;
            }
            let ink = self.right_axis_color(k);
            for (v, label) in l.rticks[k].iter().zip(&l.rlabels[k]) {
                let py = mr.sy(*v).round() as i32;
                if py < y0 || py > y1 {
                    continue;
                }
                draw_text(&mut fb, x1 + l.col_x[k], py - ch / 2, label, s, 0.0, ink);
            }
        }

        if let Some(cb) = &l.cbar {
            self.draw_colorbar(&mut fb, cb, s);
        }
        self.draw_legend(&mut fb, x0, y0, x1, s, 0.0, false);
        if let Some(st) = &l.strip {
            self.draw_range_slider(&mut fb, st, s);
        }
        if let Some(hover_px) = self.hover2d_px {
            self.draw_crosshair(&mut fb, hover_px, (x0, y0, x1, y1), s, &map, &maps_r);
        }
        fb
    }

    /// The range-slider strip: a bordered full-extent overview of every
    /// visible 2D trace, dimmed outside the `x_window` selection (no alpha in
    /// the framebuffer, so "dim" is a solid dark repaint: one dim pass over
    /// the whole strip, then a full-color pass clipped to the window), with
    /// bright grab handles on the window edges.
    fn draw_range_slider(&self, fb: &mut Framebuffer, st: &StripLayout, s: i32) {
        let (sx0, sy0, sx1, sy1) = (st.x0, st.y0, st.x1, st.y1);
        fb.rect_fill(sx0, sy0, sx1, sy0, 0.0, self.chrome.frame);
        fb.rect_fill(sx0, sy1, sx1, sy1, 0.0, self.chrome.frame);
        fb.rect_fill(sx0, sy0, sx0, sy1, 0.0, self.chrome.frame);
        fb.rect_fill(sx1, sy0, sx1, sy1, 0.0, self.chrome.frame);

        let wx0 = st.wx0.round() as i32;
        let wx1 = st.wx1.round() as i32;
        for pass in 0..2 {
            if pass == 0 {
                fb.set_clip(sx0 + 1, sy0 + 1, sx1 - 1, sy1 - 1);
            } else {
                let (cx0, cx1) = (wx0.max(sx0 + 1), wx1.min(sx1 - 1));
                if cx1 < cx0 {
                    break; // window narrower than a pixel: dim pass stands
                }
                fb.set_clip(cx0, sy0 + 1, cx1, sy1 - 1);
            }
            for (ti, t) in self.traces.iter().enumerate() {
                if !self.is_visible(ti) {
                    continue;
                }
                let m = match t.axis().right_index() {
                    Some(k) => &st.maps_r[k],
                    None => &st.map,
                };
                let tint = |c: &Rgb| if pass == 0 { shade(*c, 0.4) } else { *c };
                match t {
                    // A grid has no useful one-line overview: squeezed into
                    // the strip its cells collapse into a smear that says
                    // nothing about where the window is. It sits the strip out.
                    Trace::Heatmap2d { .. } => {}
                    // Box plots are a categorical summary; squeezed into the
                    // strip they say nothing about where the x window is.
                    Trace::Box2d { .. } => {}
                    // A graph's x coordinates are ranks or columns, not a
                    // sweep; there is nothing along x for a window to select.
                    Trace::Graph2d { .. } => {}
                    Trace::Band2d { xs, lo, hi, color, .. } => {
                        let n = xs.len().min(lo.len()).min(hi.len());
                        let cols: Vec<(f64, f64, f64)> = (0..n)
                            .map(|i| (m.sx(xs[i] as f64), m.sy(lo[i] as f64), m.sy(hi[i] as f64)))
                            .collect();
                        fb.fill_between(&cols, 0.0, tint(color));
                    }
                    Trace::Scatter2d { xs, ys, color, colors, .. } => {
                        for i in 0..xs.len().min(ys.len()) {
                            let (px, py) = (m.sx(xs[i] as f64), m.sy(ys[i] as f64));
                            if px.is_finite() && py.is_finite() {
                                let c = colors.as_ref().and_then(|v| v.get(i)).unwrap_or(color);
                                // Sizes and shapes are dropped here on purpose:
                                // the strip is a one-line overview, and marks
                                // at strip scale would collide into noise.
                                fb.disc(px as f32, py as f32, 0.0, s as f32, tint(c));
                            }
                        }
                    }
                    Trace::Line2d { xs, ys, color, .. } => {
                        let n = xs.len().min(ys.len());
                        let mut prev: Option<(f64, f64)> = None;
                        for i in 0..n {
                            let (px, py) = (m.sx(xs[i] as f64), m.sy(ys[i] as f64));
                            let cur = (px.is_finite() && py.is_finite()).then_some((px, py));
                            if let (Some(a), Some(b)) = (prev, cur) {
                                stroke(fb, a, b, 0.5, tint(color));
                            }
                            prev = cur;
                        }
                    }
                    Trace::Histogram2d { values, bins, color, .. } => {
                        let b = self.hist_bins(ti, values, *bins);
                        let base = m.sy(0.0);
                        for (i, &n) in b.counts.iter().enumerate() {
                            if n == 0 {
                                continue;
                            }
                            let (lo, hi) = b.edges(i);
                            fb.rect_fill(
                                m.sx(lo).round() as i32,
                                m.sy(n as f64).round() as i32,
                                m.sx(hi).round() as i32,
                                base.round() as i32,
                                0.0,
                                tint(color),
                            );
                        }
                    }
                    Trace::Bar2d { xs, heights, color, orient, .. } => {
                        let hw = self.bar_hw(ti, xs, *orient);
                        let horiz = orient.is_horizontal();
                        for i in 0..xs.len().min(heights.len()) {
                            let (pos, hgt) = (xs[i] as f64, heights[i] as f64);
                            if !pos.is_finite() || !hgt.is_finite() {
                                continue;
                            }
                            let (c0, c1, v0, v1) = self.bar_geometry(ti, xs[i], hgt, hw);
                            let (a0, a1, b0, b1) = if horiz {
                                (m.sx(v0), m.sx(v1), m.sy(c1), m.sy(c0))
                            } else {
                                (m.sx(c0), m.sx(c1), m.sy(v1), m.sy(v0))
                            };
                            fb.rect_fill(
                                a0.round() as i32,
                                b0.round() as i32,
                                a1.round() as i32,
                                b1.round() as i32,
                                0.0,
                                tint(color),
                            );
                        }
                    }
                    traces_3d!() => {}
                }
            }
            fb.clear_clip();
        }

        // Handles last, so they read over both passes.
        for wx in [wx0, wx1] {
            let hx0 = (wx - s).max(sx0);
            let hx1 = (wx + s - 1).min(sx1);
            fb.rect_fill(hx0, sy0, hx1, sy1, 0.0, self.chrome.ink_bright);
        }
    }

    /// What the range-slider strip has under `(px, py)` framebuffer pixels at
    /// a `w`×`h` frame, within `tol_px` (handles win over the window body, the
    /// nearer handle wins over the farther). `None` off the strip, when the
    /// strip is inactive, and always for 3D plots. Terminal mice report per
    /// cell, so pass at least one cell width of tolerance.
    pub fn range_slider_hit(
        &self,
        px_w: usize,
        px_h: usize,
        px: f32,
        py: f32,
        tol_px: f32,
    ) -> Option<RangeHit> {
        if self.is_3d() {
            return None;
        }
        let l = self.layout_2d(px_w, px_h);
        let st = l.strip?;
        let (px, py, tol) = (px as f64, py as f64, tol_px.max(0.0) as f64);
        if py < st.y0 as f64 - tol
            || py > st.y1 as f64 + tol
            || px < st.x0 as f64 - tol
            || px > st.x1 as f64 + tol
        {
            return None;
        }
        let (dl, dr) = ((px - st.wx0).abs(), (px - st.wx1).abs());
        if dl <= tol || dr <= tol {
            return Some(if dl <= dr { RangeHit::LeftHandle } else { RangeHit::RightHandle });
        }
        if px > st.wx0 && px < st.wx1 {
            return Some(RangeHit::Window);
        }
        Some(RangeHit::Track)
    }

    /// Drag the grabbed strip `part` by `dx_px` framebuffer pixels: handles
    /// resize the window (never below [`MIN_WINDOW_FRAC`] of the full
    /// extent), the window body slides it span-preserving. A `Track` grab
    /// slides like the body. With no window set, the drag starts from the
    /// full extent. Returns whether the window changed (repaint needed);
    /// `false` when the strip is inactive or the plot is 3D.
    pub fn drag_x_window(&mut self, px_w: usize, px_h: usize, part: RangeHit, dx_px: f32) -> bool {
        if self.is_3d() {
            return false;
        }
        let l = self.layout_2d(px_w, px_h);
        let Some(st) = l.strip else { return false };
        let (dom_lo, dom_hi) = st.full;
        let dx = dx_px as f64 / st.map.ax;
        if !dx.is_finite() {
            return false;
        }
        let min_w = (dom_hi - dom_lo) * MIN_WINDOW_FRAC;
        let (lo, hi) = self.x_window.unwrap_or((dom_lo, dom_hi));
        let new = match part {
            RangeHit::LeftHandle => {
                let cap = (hi - min_w).max(dom_lo);
                ((lo + dx).clamp(dom_lo, cap), hi)
            }
            RangeHit::RightHandle => {
                let floor = (lo + min_w).min(dom_hi);
                (lo, (hi + dx).clamp(floor, dom_hi))
            }
            RangeHit::Window | RangeHit::Track => {
                let (lo_d, hi_d) = (dom_lo - lo, dom_hi - hi);
                let d = if lo_d > hi_d { 0.0 } else { dx.clamp(lo_d, hi_d) };
                (lo + d, hi + d)
            }
        };
        let changed = self.x_window != Some(new);
        self.x_window = Some(new);
        changed
    }

    /// Center the window on the strip position under `px` framebuffer pixels
    /// (a click on the track), keeping its span — or a tenth of the full
    /// extent when no window is set. Returns whether the window changed.
    pub fn jump_x_window(&mut self, px_w: usize, px_h: usize, px: f32) -> bool {
        if self.is_3d() {
            return false;
        }
        let l = self.layout_2d(px_w, px_h);
        let Some(st) = l.strip else { return false };
        let (dom_lo, dom_hi) = st.full;
        let span = match self.x_window {
            Some((lo, hi)) => hi - lo,
            None => (dom_hi - dom_lo) * 0.1,
        };
        let c = st.map.inv_x(px as f64);
        if !c.is_finite() {
            return false;
        }
        let lo = (c - span * 0.5).clamp(dom_lo, (dom_hi - span).max(dom_lo));
        let new = Some((lo, lo + span));
        let changed = self.x_window != new;
        self.x_window = new;
        changed
    }

    /// Slide a set window by a plot-area drag of `dx_px` framebuffer pixels,
    /// at the main map's scale and grab-the-data sign (drag right, view moves
    /// left). No-op without a window — an unwindowed 2D drag stays a camera
    /// pan. Returns whether the window changed.
    pub fn pan_x_window(&mut self, px_w: usize, px_h: usize, dx_px: f32) -> bool {
        if self.is_3d() {
            return false;
        }
        let Some((lo, hi)) = self.x_window else { return false };
        let l = self.layout_2d(px_w, px_h);
        let (dom_lo, dom_hi, ..) = self.bounds_2d_in(None);
        let dx = -(dx_px as f64) / l.map.ax;
        if !dx.is_finite() {
            return false;
        }
        let (lo_d, hi_d) = (dom_lo - lo, dom_hi - hi);
        let d = if lo_d > hi_d { 0.0 } else { dx.clamp(lo_d, hi_d) };
        let new = Some((lo + d, hi + d));
        let changed = self.x_window != new;
        self.x_window = new;
        changed
    }

    /// Zoom the window about the data x under `px` framebuffer pixels
    /// (`factor > 1` narrows it — zooms in), clamped to the full extent and
    /// [`MIN_WINDOW_FRAC`]. Starts from the full extent when no window is
    /// set, so a scroll on an unwindowed plot begins windowing it. Returns
    /// whether the window changed.
    pub fn zoom_x_window(&mut self, px_w: usize, px_h: usize, px: f32, factor: f64) -> bool {
        if self.is_3d() || !factor.is_finite() || factor <= 0.0 {
            return false;
        }
        let l = self.layout_2d(px_w, px_h);
        let (dom_lo, dom_hi, ..) = self.bounds_2d_in(None);
        let (lo, hi) = self.x_window.unwrap_or((dom_lo, dom_hi));
        let a = l.map.inv_x(px as f64).clamp(lo, hi);
        if !a.is_finite() {
            return false;
        }
        let min_w = (dom_hi - dom_lo) * MIN_WINDOW_FRAC;
        let (mut nlo, mut nhi) = (a + (lo - a) / factor, a + (hi - a) / factor);
        if nhi - nlo < min_w {
            // Re-widen to the floor around the anchor, preserving its ratio.
            let t = if hi > lo { (a - lo) / (hi - lo) } else { 0.5 };
            nlo = a - min_w * t;
            nhi = nlo + min_w;
        }
        nlo = nlo.max(dom_lo);
        nhi = nhi.min(dom_hi).max((nlo + min_w).min(dom_hi));
        let new = Some((nlo, nhi));
        let changed = self.x_window != new;
        self.x_window = new;
        changed
    }

    /// Slide a set window by `frac` of its own span (positive = later x),
    /// clamped to the full extent — the keyboard's window step, needing no
    /// pixel geometry. No-op without a window. Returns whether it changed.
    pub fn shift_x_window(&mut self, frac: f64) -> bool {
        if self.is_3d() || !frac.is_finite() {
            return false;
        }
        let Some((lo, hi)) = self.x_window else { return false };
        let (dom_lo, dom_hi, ..) = self.bounds_2d_in(None);
        let (lo_d, hi_d) = (dom_lo - lo, dom_hi - hi);
        let d = if lo_d > hi_d { 0.0 } else { ((hi - lo) * frac).clamp(lo_d, hi_d) };
        let new = Some((lo + d, hi + d));
        let changed = self.x_window != new;
        self.x_window = new;
        changed
    }

    /// The colormap legend: a vertical gradient strip with a tick label per
    /// value, and the caption above it when there is one.
    ///
    /// The ramp is painted a pixel row at a time from the same
    /// [`Colormap::sample`] the traces use, so the strip is the mapping rather
    /// than a picture of it — a ramp that drifted from the data it explains
    /// would be worse than no ramp at all.
    fn draw_colorbar(&self, fb: &mut Framebuffer, cb: &CbarLayout, s: i32) {
        let Some(bar) = &self.colorbar else { return };
        let (ch, pad, tick_len) = (CHAR_H * s, 3 * s, 2 * s);
        let span = (cb.y1 - cb.y0).max(1) as f32;
        for y in cb.y0..=cb.y1 {
            // Top is `hi`, matching a y axis.
            let t = (cb.y1 - y) as f32 / span;
            fb.rect_fill(cb.x0, y, cb.x1, y, 0.0, bar.map.sample(t));
        }
        // A hairline frame, so the ramp reads as an object and its light end
        // does not bleed into the page.
        fb.rect_fill(cb.x0, cb.y0, cb.x1, cb.y0, 0.0, self.chrome.frame);
        fb.rect_fill(cb.x0, cb.y1, cb.x1, cb.y1, 0.0, self.chrome.frame);
        fb.rect_fill(cb.x0, cb.y0, cb.x0, cb.y1, 0.0, self.chrome.frame);
        fb.rect_fill(cb.x1, cb.y0, cb.x1, cb.y1, 0.0, self.chrome.frame);

        let range = bar.hi - bar.lo;
        for (v, label) in cb.ticks.iter().zip(&cb.labels) {
            if range == 0.0 {
                break;
            }
            let f = ((v - bar.lo) / range) as f32;
            let py = cb.y1 - (f * span).round() as i32;
            if py < cb.y0 || py > cb.y1 {
                continue;
            }
            fb.rect_fill(cb.x1, py, cb.x1 + tick_len, py, 0.0, self.chrome.frame);
            draw_text(fb, cb.x1 + tick_len + pad, py - ch / 2, label, s, 0.0, self.chrome.ink);
        }
        if let Some(caption) = &bar.label {
            // A long caption slides left to fit, but keeps the frame's own
            // padding off the edge rather than butting against it.
            let lw = text_width(caption, s);
            let lx = cb.x0.min((fb.w as i32 - lw - pad).max(0));
            draw_text(fb, lx, cb.y0 - ch - pad, caption, s, 0.0, self.chrome.ink_bright);
        }
    }

    /// The 2D hover crosshair: a vertical guide at the sample x nearest the
    /// hovered pixel, a marker on every series sampled at that x, and a
    /// readout box naming each value. Drawn after everything else so no
    /// chrome covers it. Series match by exact sample x, so series on a
    /// shared grid all get a row while series on their own grids only show
    /// where they truly have a sample. The readout box is placed by
    /// [`readout_slot`], which keeps it off the legend and out of the half of
    /// the frame the sampled markers are in.
    fn draw_crosshair(
        &self,
        fb: &mut Framebuffer,
        hover_px: f32,
        rect: (i32, i32, i32, i32),
        s: i32,
        map: &Map2d,
        maps_r: &[Map2d; RIGHT_AXES],
    ) {
        let (x0, y0, x1, y1) = rect;
        let cursor_x = map.inv_x(hover_px as f64);
        // With an x window the nearest sample overall may sit outside it;
        // snapping there would silently drop the crosshair (the guide bails
        // off-rect below), so windowed snapping only considers visible xs.
        let vis = self.x_window;
        let mut snap: Option<f32> = None;
        let mut best = f64::INFINITY;
        for (ti, t) in self.traces.iter().enumerate() {
            if !self.is_visible(ti) {
                continue;
            }
            // A histogram has no x array — its bars are derived — so it offers
            // its bin centres, materialised for this iteration only.
            let centres;
            let xs: &[f32] = match t {
                Trace::Scatter2d { xs, .. }
                | Trace::Line2d { xs, .. }
                | Trace::Band2d { xs, .. } => xs,
                // A box summarises a group; there is no x sample under the
                // guide, and its five numbers are not a value at a position.
                Trace::Box2d { .. } => continue,
                // A horizontal bar's positions are y coordinates; a vertical
                // guide has nothing to snap to on them.
                Trace::Bar2d { orient, .. } if orient.is_horizontal() => continue,
                Trace::Bar2d { xs, .. } => xs,
                Trace::Histogram2d { values, bins, .. } => {
                    let b = self.hist_bins(ti, values, *bins);
                    centres = (0..b.counts.len()).map(|i| b.center(i) as f32).collect::<Vec<_>>();
                    &centres
                }
                // A grid snaps to its columns: the crosshair is a vertical
                // guide, so the x coordinates are the ones it can land on.
                Trace::Heatmap2d { xs, .. } => xs,
                // Hovering a graph means hovering a *node*, which is a
                // different gesture from the x crosshair — and a column of
                // node centres is not a series to read a value off.
                Trace::Graph2d { .. } => continue,
                traces_3d!() => continue,
            };
            for &x in xs {
                if let Some((wlo, whi)) = vis {
                    if (x as f64) < wlo || (x as f64) > whi {
                        continue;
                    }
                }
                let d = (x as f64 - cursor_x).abs();
                if x.is_finite() && d < best {
                    best = d;
                    snap = Some(x);
                }
            }
        }
        let Some(snap) = snap else { return };
        let px = map.sx(snap as f64).round() as i32;
        if px < x0 || px > x1 {
            return;
        }
        fb.rect_fill(px, y0, px, y1, 0.0, self.chrome.ink);

        let mut rows: Vec<(String, Rgb)> = Vec::new();
        // The marker y's actually on the frame, which is what the readout box
        // steers away from. A band contributes a row but no marker, so this
        // is not one entry per row.
        let mut markers: Vec<i32> = Vec::new();
        for (ti, t) in self.traces.iter().enumerate() {
            // Skipping hidden traces here keeps the `series N` fallback names
            // stable: numbering follows the trace index, not the row count.
            if !self.is_visible(ti) {
                continue;
            }
            let m = match t.axis().right_index() {
                Some(k) => &maps_r[k],
                None => map,
            };
            // A histogram reads out the bin itself — the interval and its
            // count — because "27" alone does not say 27 of what.
            let (value, readout) = match t {
                Trace::Histogram2d { values, bins, .. } => {
                    let b = self.hist_bins(ti, values, *bins);
                    let Some(i) = (0..b.counts.len()).find(|&i| b.center(i) as f32 == snap) else {
                        continue;
                    };
                    let (lo, hi) = b.edges(i);
                    let n = b.counts[i];
                    (n as f32, format!("[{}, {})  {n}", format_value(lo), format_value(hi)))
                }
                _ => {
                    let (xs, vals) = match t {
                        Trace::Scatter2d { xs, ys, .. } | Trace::Line2d { xs, ys, .. } => (xs, ys),
                        // A band's readout is the interval itself, since its
                        // whole point is the width between the two edges.
                        Trace::Band2d { xs, lo, hi, .. } => {
                            let Some(i) = xs.iter().position(|&x| x == snap) else { continue };
                            let (Some(&a), Some(&b)) = (lo.get(i), hi.get(i)) else { continue };
                            if !a.is_finite() || !b.is_finite() {
                                continue;
                            }
                            let name = t
                                .name()
                                .map_or_else(|| format!("series {}", ti + 1), str::to_owned);
                            let (a, b) = (a.min(b), a.max(b));
                            rows.push((
                                format!(
                                    "{name}  {}–{}",
                                    format_value(a as f64),
                                    format_value(b as f64)
                                ),
                                t.color(),
                            ));
                            continue;
                        }
                        Trace::Box2d { .. } => continue,
                        Trace::Bar2d { orient, .. } if orient.is_horizontal() => continue,
                        Trace::Bar2d { xs, heights, .. } => (xs, heights),
                        Trace::Histogram2d { .. } => unreachable!("handled above"),
                        // A grid has a value per *cell*, so a vertical guide
                        // crosses a whole column of them; there is no single
                        // number to put in the readout. Hovering a cell is
                        // what a heatmap wants, and that is a different
                        // gesture from the x crosshair.
                        Trace::Heatmap2d { .. } => continue,
                        Trace::Graph2d { .. } => continue,
                        traces_3d!() => continue,
                    };
                    let Some(i) = xs.iter().position(|&x| x == snap) else { continue };
                    let Some(&v) = vals.get(i) else { continue };
                    let name = t.name().map_or_else(|| format!("series {}", ti + 1), str::to_owned);
                    (v, format!("{name}  {}", format_value(v as f64)))
                }
            };
            if !value.is_finite() {
                continue;
            }
            let py = m.sy(value as f64).round() as i32;
            if py >= y0 && py <= y1 {
                fb.disc(px as f32, py as f32, 0.0, 2.6 * s as f32, [255, 255, 255]);
                fb.disc(px as f32, py as f32, 0.0, 1.7 * s as f32, t.color());
                markers.push(py);
            }
            rows.push((readout, t.color()));
        }
        if rows.is_empty() {
            return;
        }

        // The same rounded panel as the legend, so the two read as one family.
        let cw = CHAR_W * s;
        let ps = PanelStyle::new(s);
        let header = format!("x  {}", self.format_x(snap as f64));
        let text_w = rows
            .iter()
            .map(|(l, _)| ps.measure(l))
            .chain([ps.measure(&header)])
            .max()
            .unwrap_or(cw);
        let (box_w, box_h) = ps.box_size(rows.len() as i32 + 1, text_w);
        // Beside the guide, away from the data, and off the legend. The
        // legend rect comes from the same call `draw_legend` made a moment
        // ago, so the box dodges what is actually on screen.
        let legend = self.legend_box(x1, y0, s, false).map(|l| (l.bx0, l.by0, l.bx1, l.by1));
        let (bx0, by0) = readout_slot(px, box_w, box_h, rect, ps.inset, &markers, legend);
        let (bx1, by1) = (bx0 + box_w, by0 + box_h);

        ps.frame(fb, (bx0, by0, bx1, by1), 0.0, &self.chrome);
        let row_y = |row_i: i32| by0 + ps.pad_y + row_i * ps.row_h;
        // The x value heads the box in dimmer ink; the series rows follow.
        ps.label(fb, bx0 + ps.text_dx(), row_y(0), &header, 0.0, self.chrome.ink, self.chrome.bg);
        for (i, (label, color)) in rows.iter().enumerate() {
            let ey = row_y(i as i32 + 1);
            ps.chip(fb, bx0, ey, s, 0.0, *color);
            let ink = self.chrome.ink_bright;
            ps.label(fb, bx0 + ps.text_dx(), ey, label, 0.0, ink, self.chrome.bg);
        }
    }

    /// Where the legend goes for a render of this size, and the font scale it
    /// uses: the 2D path anchors it to the plot frame, the 3D path to the
    /// image itself. Shared by drawing and hit-testing, so a click always
    /// lands on the row the eye is pointing at.
    fn legend_anchor(&self, px_w: usize, px_h: usize) -> (i32, i32, i32, bool) {
        if !self.traces.is_empty() && !self.is_3d() {
            let l = self.layout_2d(px_w, px_h);
            (l.x1, l.y0, l.s, false)
        } else {
            let s = ((px_h.max(1) as f32) / 240.0).round().clamp(1.0, 4.0) as i32;
            (px_w.max(1) as i32 - 1, 0, s, true)
        }
    }

    /// The legend's rows and pixel box, or `None` when nothing is named.
    /// `three_d` says which render path is asking; only traces that path
    /// actually draws are listed, so a named 2D trace mixed into a 3D plot
    /// never appears as a legend entry for geometry that is not on screen.
    /// Hidden traces *are* listed, greyed out — the row is what you click to
    /// bring one back.
    fn legend_box(&self, x1: i32, y0: i32, s: i32, three_d: bool) -> Option<LegendBox<'_>> {
        let rows: Vec<LegendRow> = self
            .traces
            .iter()
            .enumerate()
            .filter(|(i, t)| self.in_legend(*i) && t.is_3d() == three_d)
            .filter_map(|(i, t)| {
                t.name().map(|name| LegendRow {
                    trace: i,
                    name,
                    color: t.color(),
                    visible: self.is_visible(i),
                })
            })
            .collect();
        if rows.is_empty() {
            return None;
        }
        let ps = PanelStyle::new(s);
        let text_w = rows.iter().map(|r| ps.measure(r.name)).max().unwrap_or(CHAR_W * s);
        let (box_w, box_h) = ps.box_size(rows.len() as i32, text_w);
        let bx1 = x1 - ps.inset_x;
        let bx0 = bx1 - box_w;
        let by0 = y0 + ps.inset;
        Some(LegendBox { ps, bx0, by0, bx1, by1: by0 + box_h, rows })
    }

    /// Legend for named traces, top-right inside the plot area. The swatch
    /// carries series identity; the label text stays in neutral ink, and a
    /// hidden series keeps its row with the colour drained out of it. `z` is
    /// the depth to draw at: 0.0 in the 2D path, pulled far forward in 3D so
    /// no geometry can poke through the legend box.
    #[allow(clippy::too_many_arguments)]
    fn draw_legend(
        &self,
        fb: &mut Framebuffer,
        _x0: i32,
        y0: i32,
        x1: i32,
        s: i32,
        z: f32,
        three_d: bool,
    ) {
        let Some(lb) = self.legend_box(x1, y0, s, three_d) else { return };
        let (ps, bx0, by0) = (lb.ps, lb.bx0, lb.by0);

        ps.frame(fb, (bx0, by0, lb.bx1, lb.by1), z, &self.chrome);
        for (i, row) in lb.rows.iter().enumerate() {
            let ey = by0 + ps.pad_y + i as i32 * ps.row_h;
            // A toggled-off series greys out rather than vanishing — the way
            // the website legend does it, and the only way back on.
            let (chip, ink) = if row.visible {
                (row.color, self.chrome.ink_bright)
            } else {
                (fade(desaturate(row.color), self.chrome.bg), fade(self.chrome.ink, self.chrome.bg))
            };
            ps.chip(fb, bx0, ey, s, z, chip);
            ps.label(fb, bx0 + ps.text_dx(), ey, row.name, z, ink, self.chrome.bg);
        }
    }

    /// Where trace `ti`'s block starts in the flat node and edge index
    /// spaces — the `(node, edge)` offsets [`Element`] indices are counted
    /// from. Insertion order, and hidden traces keep their block, exactly as
    /// [`Self::node_count`] describes.
    fn flat_base(&self, ti: usize) -> (usize, usize) {
        let (mut n, mut e) = (0usize, 0usize);
        for t in self.traces.iter().take(ti) {
            match t {
                Trace::Scatter3d { pts, .. } => n += pts.len(),
                Trace::Graph3d { nodes, edges, .. } => {
                    n += nodes.len();
                    e += edges.len();
                }
                Trace::Graph2d { nodes, edges, .. } => {
                    n += nodes.len();
                    e += edges.len();
                }
                _ => {}
            }
        }
        (n, e)
    }

    /// Every node's pixel box for trace `ti` under map `m`, indexed by node.
    /// Empty for anything that is not a [`Trace::Graph2d`]. Drawing and
    /// picking both start here, so what is on screen and what a click
    /// resolves to are the same rectangles.
    fn graph2d_boxes(&self, ti: usize, m: &Map2d, s: i32) -> Vec<NodeBox> {
        let Some(Trace::Graph2d { nodes, labels, .. }) = self.traces.get(ti) else {
            return Vec::new();
        };
        nodes
            .iter()
            .enumerate()
            .map(|(i, p)| node_box(m, *p, labels.get(i).map_or("", String::as_str), s))
            .collect()
    }

    /// One edge's pixel polyline: source box boundary, waypoints, target box
    /// boundary. `None` when either endpoint is out of range or non-finite,
    /// or when the edge is a self-loop (v1 has no loop geometry). When
    /// `trim_arrow` is set the target end stops short by that many pixels,
    /// leaving room for the arrowhead to be the thing that touches the box.
    fn graph2d_edge_path(
        &self,
        ti: usize,
        boxes: &[NodeBox],
        m: &Map2d,
        e: usize,
        trim_arrow: f64,
    ) -> Option<Vec<(f64, f64)>> {
        let Some(Trace::Graph2d { nodes, node_shapes, edges, route_pts, route_starts, .. }) =
            self.traces.get(ti)
        else {
            return None;
        };
        let &(a, b) = edges.get(e)?;
        let (a, b) = (a as usize, b as usize);
        if a == b {
            return None;
        }
        let (ba, bb) = (boxes.get(a)?, boxes.get(b)?);
        for p in [nodes.get(a)?, nodes.get(b)?] {
            if !p[0].is_finite() || !p[1].is_finite() {
                return None;
            }
        }
        let shape_of =
            |i: usize| node_shapes.as_ref().and_then(|v| v.get(i)).copied().unwrap_or_default();
        let mut pts: Vec<(f64, f64)> = Vec::with_capacity(2);
        pts.push((ba.cx, ba.cy));
        for w in edge_route(route_pts, route_starts, e) {
            let (x, y) = (m.sx(w[0] as f64), m.sy(w[1] as f64));
            if x.is_finite() && y.is_finite() {
                pts.push((x, y));
            }
        }
        pts.push((bb.cx, bb.cy));
        // Both ends are solved against the *unclipped* polyline, so the
        // first clip cannot move the point the second one aims at.
        let next = pts[1];
        let prev = pts[pts.len() - 2];
        let start = ba.boundary(shape_of(a), next.0 - ba.cx, next.1 - ba.cy);
        let mut end = bb.boundary(shape_of(b), prev.0 - bb.cx, prev.1 - bb.cy);
        if trim_arrow > 0.0 {
            let (dx, dy) = (end.0 - prev.0, end.1 - prev.1);
            let len = dx.hypot(dy);
            if len > trim_arrow {
                end = (end.0 - dx / len * trim_arrow, end.1 - dy / len * trim_arrow);
            }
        }
        let last = pts.len() - 1;
        pts[0] = start;
        pts[last] = end;
        Some(smooth_polyline(&pts))
    }

    /// Draw a [`Trace::Graph2d`]: edges with their arrowheads first, then the
    /// node boxes over them, then each label. Node centres come from the data
    /// map; box sizes are in pixels, so zooming spreads the graph out while
    /// the text stays the size it has to be to be read.
    fn draw_graph2d(&self, fb: &mut Framebuffer, ti: usize, m: &Map2d, s: i32) {
        let Some(Trace::Graph2d {
            nodes,
            labels,
            node_colors,
            node_shapes,
            edges,
            directed,
            edge_colors,
            ..
        }) = self.traces.get(ti)
        else {
            return;
        };
        let (node0, edge0) = self.flat_base(ti);
        let ts = s as f32;
        let card = lighten(self.chrome.bg, 8);
        let boxes = self.graph2d_boxes(ti, m, s);
        let color_of = |i: usize| node_colors.get(i).copied().unwrap_or([120, 180, 230]);
        let shape_of =
            |i: usize| node_shapes.as_ref().and_then(|v| v.get(i)).copied().unwrap_or_default();
        // The arrowhead is the thing that says which way an edge points, so
        // it is sized against the text scale rather than the edge width.
        let (head_len, head_half) = (4.0 * ts as f64, 2.5 * ts as f64);
        let trim = if *directed { head_len * 0.9 } else { 0.0 };

        for (k, &(a, b)) in edges.iter().enumerate() {
            let el = Element::Edge(edge0 + k);
            let Some(poly) = self.graph2d_edge_path(ti, &boxes, m, k, trim) else { continue };
            let hot = (self.selected == Some(el))
                .then_some(1.6 * ts)
                .or_else(|| (self.hovered == Some(el)).then_some(1.0 * ts));
            if let Some(r) = hot {
                for w in poly.windows(2) {
                    let a = [w[0].0 as f32, w[0].1 as f32, 0.0];
                    let b = [w[1].0 as f32, w[1].1 as f32, 0.0];
                    edge_glow(fb, a, b, r);
                }
                continue;
            }
            let (a, b) = (a as usize, b as usize);
            let ec = match edge_colors.as_ref().and_then(|v| v.get(k)) {
                Some(c) => *c,
                None => {
                    let (ca, cb) = (color_of(a), color_of(b));
                    [
                        ((ca[0] as u16 + cb[0] as u16) / 2) as u8 / 2 + 20,
                        ((ca[1] as u16 + cb[1] as u16) / 2) as u8 / 2 + 20,
                        ((ca[2] as u16 + cb[2] as u16) / 2) as u8 / 2 + 20,
                    ]
                }
            };
            for w in poly.windows(2) {
                stroke(fb, w[0], w[1], 0.5 * ts, ec);
            }
            if *directed {
                let (tail, tip) = (poly[poly.len() - 2], *poly.last().expect("two ends"));
                // The head sits past the trimmed end, so its tip is what
                // meets the target box.
                let (dx, dy) = (tip.0 - tail.0, tip.1 - tail.1);
                let len = dx.hypot(dy);
                if len > 1e-9 {
                    let (ux, uy) = (dx / len, dy / len);
                    let apex = (tip.0 + ux * trim, tip.1 + uy * trim);
                    let base = (apex.0 - ux * head_len, apex.1 - uy * head_len);
                    fb.tri(
                        [apex.0 as f32, apex.1 as f32, 0.0],
                        [(base.0 - uy * head_half) as f32, (base.1 + ux * head_half) as f32, 0.0],
                        [(base.0 + uy * head_half) as f32, (base.1 - ux * head_half) as f32, 0.0],
                        ec,
                    );
                }
            }
        }

        for (i, p) in nodes.iter().enumerate() {
            if !p[0].is_finite() || !p[1].is_finite() {
                continue;
            }
            let b = &boxes[i];
            let shape = shape_of(i);
            let el = Element::Node(node0 + i);
            // A halo drawn first and covered by the body leaves a ring, so
            // the highlight reads as an outline the way the 3D one does
            // without a second silhouette primitive.
            let halo = (self.selected == Some(el))
                .then_some(2.2 * ts as f64)
                .or_else(|| (self.hovered == Some(el)).then_some(1.2 * ts as f64));
            if let Some(margin) = halo {
                let white = [255, 255, 255];
                draw_node_body(fb, &b.grown(margin), shape, white, white);
            }
            draw_node_body(fb, b, shape, card, color_of(i));
            let label = labels.get(i).map_or("", String::as_str);
            if !label.is_empty() {
                let tw = text_width(label, s);
                draw_text(
                    fb,
                    (b.cx - tw as f64 * 0.5).round() as i32,
                    (b.cy - (CHAR_H * s) as f64 * 0.5).round() as i32,
                    label,
                    s,
                    0.0,
                    self.chrome.ink_bright,
                );
            }
        }
    }

    /// Draw one node in its shape. The selected node gets a white halo around
    /// its base color; the hovered node lights up solid white. Both are pulled
    /// to the front so the highlight is never hidden by other geometry, and
    /// both use the shape's filled silhouette so the halo stays one blob.
    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn draw_node(
        &self,
        fb: &mut Framebuffer,
        s: [f32; 3],
        radius: f32,
        shape: Shape,
        fogged: Rgb,
        base: Rgb,
        flat_index: usize,
        ts: f32,
    ) {
        let el = Element::Node(flat_index);
        let front = -1.0e9;
        if self.selected == Some(el) {
            fb.mark(s[0], s[1], front, radius + 2.2 * ts, shape.filled(), [255, 255, 255]);
            fb.mark(s[0], s[1], front, radius + 0.6 * ts, shape.filled(), base);
        } else if self.hovered == Some(el) {
            fb.mark(s[0], s[1], front, radius + 1.2 * ts, shape.filled(), [255, 255, 255]);
        } else {
            fb.mark(s[0], s[1], s[2], radius, shape, fogged);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A drawn pixel's color, or `None` where nothing was written.
    fn px(fb: &Framebuffer, x: usize, y: usize) -> Option<Rgb> {
        let i = y * fb.w + x;
        fb.drawn[i].then(|| fb.color[i])
    }

    const BAND: Rgb = [40, 90, 160];

    fn cat(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn per_point_styling_colors_sizes_and_shapes_independently() {
        let (xs, ys) = (vec![0.0, 1.0, 2.0], vec![1.0, 1.0, 1.0]);
        let red: Rgb = [220, 40, 40];
        let blue: Rgb = [40, 40, 220];

        let mut plot = Plot::new();
        plot.add_scatter2d_styled(
            xs.clone(),
            ys.clone(),
            red,
            3.0,
            Some(vec![blue, blue, blue]),
            None,
            None,
            None,
            YAxis::Primary,
        );
        let fb = plot.render(240, 160);
        let drawn = |c: Rgb| (0..fb.h).any(|y| (0..fb.w).any(|x| px(&fb, x, y) == Some(c)));
        assert!(drawn(blue), "per-point colors are used");
        assert!(!drawn(red), "the uniform color is overridden");
    }

    /// A short array styles a prefix and the rest fall back — a partial
    /// mapping must never drop points off the chart.
    #[test]
    fn short_style_arrays_fall_back_instead_of_truncating() {
        let mut plot = Plot::new();
        let red: Rgb = [220, 40, 40];
        let blue: Rgb = [40, 40, 220];
        plot.add_scatter2d_styled(
            vec![0.0, 1.0, 2.0],
            vec![1.0, 1.0, 1.0],
            red,
            3.0,
            Some(vec![blue]),
            None,
            None,
            None,
            YAxis::Primary,
        );
        let fb = plot.render(240, 160);
        let mut saw = (false, false);
        for y in 0..fb.h {
            for x in 0..fb.w {
                match px(&fb, x, y) {
                    Some(c) if c == blue => saw.0 = true,
                    Some(c) if c == red => saw.1 = true,
                    _ => {}
                }
            }
        }
        assert_eq!(saw, (true, true), "styled prefix and unstyled remainder both draw");
    }

    #[test]
    fn per_point_sizes_widen_the_windowed_clip_box() {
        // A big point whose centre sits just outside the frame must still
        // reach into it; the clip box is sized by the largest mark.
        let mut plot = Plot::new();
        plot.add_scatter2d_styled(
            vec![0.0, 10.0],
            vec![1.0, 1.0],
            [220, 40, 40],
            1.0,
            None,
            Some(vec![1.0, 14.0]),
            None,
            None,
            YAxis::Primary,
        );
        plot.x_window = Some((-1.0, 9.5));
        let fb = plot.render(240, 160);
        assert!(
            (0..fb.h).any(|y| (0..fb.w).any(|x| px(&fb, x, y).is_some())),
            "an oversized point near the window edge must still render"
        );
    }

    #[test]
    fn set_point_styles_rejects_non_scatter_traces() {
        let mut plot = Plot::new();
        let bars = plot.add_bar2d(vec![0.0], vec![1.0], [1, 2, 3], None, YAxis::Primary);
        assert_eq!(
            plot.set_point_styles(bars, Some(vec![[1, 2, 3]]), None, None),
            Err(TraceError::WrongKind)
        );
        assert_eq!(plot.set_point_styles(99, None, None, None), Err(TraceError::UnknownTrace));
    }

    /// The whole point of a step: it must pass through the corner, and the
    /// corner is exactly where a straight segment would not go.
    #[test]
    fn step_modes_place_the_riser_where_they_promise() {
        let a = (0.0, 10.0);
        let b = (10.0, 0.0);
        assert_eq!(Interp::Linear.corners(a, b), [None, None]);
        // Pre: rise at the old x, then run across at the new y.
        assert_eq!(Interp::Pre.corners(a, b), [Some((0.0, 0.0)), None]);
        // Post: run across at the old y, then rise at the new x.
        assert_eq!(Interp::Post.corners(a, b), [Some((10.0, 10.0)), None]);
        // Mid: two corners, riser halfway between.
        assert_eq!(Interp::Mid.corners(a, b), [Some((5.0, 10.0)), Some((5.0, 0.0))]);
        assert_eq!(Interp::parse("post"), Some(Interp::Post));
        assert_eq!(Interp::parse("stairs"), None);
    }

    /// A step has a flat leg; a diagonal does not. Counting total pixels
    /// cannot tell them apart — discs stamped along a 45° path cover more
    /// area per unit length than along an axis-aligned one, so the totals
    /// flip with geometry — but the widest single row is decisive.
    #[test]
    fn a_step_runs_flat_where_a_line_runs_diagonally() {
        let widest_row = |interp: Interp| {
            let mut plot = Plot::new();
            plot.add_step2d(
                vec![0.0, 1.0, 2.0],
                vec![0.0, 1.0, 0.0],
                [220, 40, 40],
                2.0,
                interp,
                None,
                YAxis::Primary,
            );
            let fb = plot.render(240, 240);
            (0..fb.h)
                .map(|y| (0..fb.w).filter(|&x| px(&fb, x, y) == Some([220, 40, 40])).count())
                .max()
                .unwrap_or(0)
        };
        let diagonal = widest_row(Interp::Linear);
        for mode in [Interp::Pre, Interp::Post, Interp::Mid] {
            assert!(
                widest_row(mode) > diagonal * 4,
                "{mode:?} must hold its value across a flat run; \
                 widest row {} vs diagonal {diagonal}",
                widest_row(mode)
            );
        }
    }

    #[test]
    fn binning_counts_every_finite_value_exactly_once() {
        let v: Vec<f32> = (0..100).map(|i| i as f32).collect();
        let b = bin_values(&v, BinSpec::Count(10));
        assert_eq!(b.counts.len(), 10);
        assert_eq!(b.counts.iter().sum::<u32>(), 100, "no value may be lost");
        // The largest value lands in the last bin rather than off the end of
        // the half-open interval.
        assert!(*b.counts.last().unwrap() > 0);

        // Non-finite values are dropped, like a non-finite coordinate.
        let b = bin_values(&[1.0, f32::NAN, 2.0, f32::INFINITY], BinSpec::Count(2));
        assert_eq!(b.counts.iter().sum::<u32>(), 2);

        // Degenerate samples stay representable.
        assert!(bin_values(&[], BinSpec::Auto).counts.is_empty());
        let same = bin_values(&[5.0, 5.0, 5.0], BinSpec::Auto);
        assert_eq!(same.counts.iter().sum::<u32>(), 3);
        assert!(same.width > 0.0, "a zero-span sample still needs a drawable width");
    }

    #[test]
    fn bin_rules_respect_their_knobs_and_stay_bounded() {
        let v: Vec<f32> = (0..1000).map(|i| (i % 37) as f32).collect();
        assert_eq!(bin_values(&v, BinSpec::Count(7)).counts.len(), 7);
        // An explicit width is honoured exactly.
        let w = bin_values(&v, BinSpec::Width(4.0));
        assert!((w.width - 4.0).abs() < 1e-9);
        // No rule may ask for more bins than a terminal can draw.
        assert!(bin_values(&v, BinSpec::Count(usize::MAX)).counts.len() <= MAX_BINS);
        assert!(bin_values(&v, BinSpec::Width(1e-12)).counts.len() <= MAX_BINS);
        // Heavily tied data has a zero IQR; Auto must fall back, not diverge.
        let tied = vec![1.0f32; 500];
        assert!(!bin_values(&tied, BinSpec::Auto).counts.is_empty());
    }

    /// Zoom must not rebin: the bars a reader is looking at have to keep
    /// meaning the same thing when they pan.
    #[test]
    fn a_histogram_keeps_its_bins_under_zoom_and_rebins_on_extend() {
        let mut plot = Plot::new();
        let id = plot.add_histogram2d(
            (0..100).map(|i| i as f32).collect(),
            BinSpec::Count(10),
            [200, 120, 60],
            None,
            YAxis::Primary,
        );
        let before = plot.hist_bins(id, &[], BinSpec::Auto).into_owned();
        plot.x_window = Some((10.0, 20.0));
        let windowed = plot.hist_bins(id, &[], BinSpec::Auto).into_owned();
        assert_eq!(before, windowed, "a window must not move bin edges");

        // Streaming new observations does rebin.
        plot.extend_values(id, &[500.0]).unwrap();
        let after = plot.hist_bins(id, &[], BinSpec::Auto).into_owned();
        assert_ne!(before.hi(), after.hi(), "a new extreme must widen the range");
        assert_eq!(after.counts.iter().sum::<u32>(), 101);
    }

    #[test]
    fn a_histogram_draws_bars_and_reports_its_kind() {
        let mut plot = Plot::new();
        let id = plot.add_histogram2d(
            (0..200).map(|i| (i % 20) as f32).collect(),
            BinSpec::Count(8),
            [200, 120, 60],
            Some("sample".into()),
            YAxis::Primary,
        );
        let fb = plot.render(320, 200);
        let bars = (0..fb.h)
            .map(|y| (0..fb.w).filter(|&x| px(&fb, x, y) == Some([200, 120, 60])).count())
            .sum::<usize>();
        assert!(bars > 500, "the histogram must fill real area, got {bars}");

        // It is structural for the coordinate `extend` path, and says why.
        let (kind, why) = plot.traces[id].structural_reason().unwrap();
        assert_eq!(kind, "histogram");
        assert!(why.contains("extend_values"));
        assert_eq!(plot.extend_xy(id, &[1.0], &[1.0]), Err(TraceError::Structural));
        assert_eq!(plot.extend_values(99, &[1.0]), Err(TraceError::UnknownTrace));
    }

    fn heat() -> Plot {
        let mut plot = Plot::new();
        // A 3x2 grid whose values climb left to right.
        plot.add_heatmap2d(
            vec![0.0, 1.0, 2.0],
            vec![0.0, 1.0],
            vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0],
            Colormap::Viridis,
            Some("grid".into()),
        );
        plot
    }

    /// Cells tile edge to edge: a regular grid must leave no seam between
    /// neighbours, which is what the two-axis pad exists for.
    #[test]
    fn heatmap_cells_tile_the_grid_on_both_axes() {
        let plot = heat();
        assert_eq!(plot.cached_pad(0), Some((0.5, 0.5)));
        let CachedBounds::B2 { xlo, xhi, ylo, yhi, .. } = plot.meta[0].bounds else {
            panic!("a heatmap caches a B2 box");
        };
        assert_eq!((xlo, xhi), (-0.5, 2.5), "x reaches half a cell past the centres");
        assert_eq!((ylo, yhi), (-0.5, 1.5), "and so does y — the bar case padded none");
    }

    #[test]
    fn a_heatmap_paints_its_ramp_and_leaves_holes_for_nan() {
        let fb = heat().render(300, 200);
        let ramp = |t: f32| Colormap::Viridis.sample(t);
        let has = |c: Rgb| (0..fb.h).any(|y| (0..fb.w).any(|x| px(&fb, x, y) == Some(c)));
        assert!(has(ramp(0.0)), "the low end of the ramp is painted");
        assert!(has(ramp(1.0)), "and the high end");

        // A non-finite cell is a hole, not a zero: blanking one must remove
        // area rather than paint it with the ramp's bottom color.
        let filled = |p: &Plot| {
            let fb = p.render(300, 200);
            (0..fb.h).map(|y| (0..fb.w).filter(|&x| px(&fb, x, y).is_some()).count()).sum::<usize>()
        };
        let solid = heat();
        let mut holed = heat();
        if let Trace::Heatmap2d { zs, .. } = &mut holed.traces[0] {
            zs[4] = f32::NAN;
        }
        holed.meta.clear(); // force the desynced full-scan path too
        assert!(filled(&holed) < filled(&solid), "a NaN cell must leave a hole");
    }

    #[test]
    fn a_heatmap_reports_its_range_and_is_structural() {
        let plot = heat();
        assert_eq!(plot.heatmap_range(0), Some((0.0, 5.0)));
        assert_eq!(plot.heatmap_range(99), None);
        let (kind, why) = plot.traces[0].structural_reason().unwrap();
        assert_eq!((kind, why), ("heatmap", "a fixed grid"));
        // It is always on the primary axis: a grid spans both axes itself.
        assert_eq!(plot.traces[0].axis(), YAxis::Primary);
    }

    /// A horizontal bar is the vertical one with its axes swapped — every
    /// consequence of that has to follow, not just the drawing.
    #[test]
    fn horizontal_bars_swap_which_axis_carries_what() {
        let build = |orient: Orient| {
            let mut plot = Plot::new();
            plot.add_bar2d_oriented(
                vec![0.0, 1.0, 2.0],
                vec![3.0, 5.0, 4.0],
                [200, 120, 60],
                orient,
                None,
                YAxis::Primary,
            );
            plot
        };
        let v = build(Orient::Vertical);
        let h = build(Orient::Horizontal);
        let hw = bar_halfwidth(&[0.0, 1.0, 2.0]) as f64;

        // The width moves to the axis the bars are spaced along...
        assert_eq!(v.cached_pad(0), Some((hw, 0.0)));
        assert_eq!(h.cached_pad(0), Some((0.0, hw)));
        // ...and bar_hw still finds it.
        assert_eq!(h.bar_hw(0, &[0.0, 1.0, 2.0], Orient::Horizontal), hw);

        // The cached box is the vertical one transposed.
        let CachedBounds::B2 { xlo: vx0, xhi: vx1, ylo: vy0, yhi: vy1, .. } = v.meta[0].bounds
        else {
            panic!("B2")
        };
        let CachedBounds::B2 { xlo: hx0, xhi: hx1, ylo: hy0, yhi: hy1, .. } = h.meta[0].bounds
        else {
            panic!("B2")
        };
        assert_eq!((hx0, hx1), (vy0, vy1), "x now carries the values");
        assert_eq!((hy0, hy1), (vx0, vx1), "and y carries the categories");
        // The value axis still reaches the zero baseline.
        assert_eq!(hx0, 0.0);
    }

    #[test]
    fn horizontal_bars_draw_wide_rows_not_tall_columns() {
        let widest = |orient: Orient, tallest: bool| {
            let mut plot = Plot::new();
            plot.add_bar2d_oriented(
                vec![0.0, 1.0, 2.0],
                vec![3.0, 5.0, 4.0],
                [200, 120, 60],
                orient,
                None,
                YAxis::Primary,
            );
            let fb = plot.render(300, 300);
            let hit = |x: usize, y: usize| px(&fb, x, y) == Some([200, 120, 60]);
            if tallest {
                (0..fb.w).map(|x| (0..fb.h).filter(|&y| hit(x, y)).count()).max().unwrap_or(0)
            } else {
                (0..fb.h).map(|y| (0..fb.w).filter(|&x| hit(x, y)).count()).max().unwrap_or(0)
            }
        };
        // Vertical bars are tall columns; horizontal ones are wide rows.
        assert!(widest(Orient::Vertical, true) > widest(Orient::Vertical, false));
        assert!(widest(Orient::Horizontal, false) > widest(Orient::Horizontal, true));
    }

    /// The crosshair is a vertical guide, so a horizontal bar has no x sample
    /// for it to land on — it must sit the gesture out rather than snap to a
    /// y coordinate as if it were one.
    #[test]
    fn horizontal_bars_sit_out_the_x_crosshair() {
        let mut plot = Plot::new();
        plot.add_bar2d_oriented(
            vec![0.0, 1.0, 2.0],
            vec![3.0, 5.0, 4.0],
            [200, 120, 60],
            Orient::Horizontal,
            None,
            YAxis::Primary,
        );
        let plain = plot.render(300, 200);
        plot.hover2d_px = Some(150.0);
        let hovered = plot.render(300, 200);
        assert_eq!(plain.rgba(), hovered.rgba(), "no guide, no readout");
    }

    fn two_bar_series(mode: BarMode) -> Plot {
        let mut plot = Plot::new();
        plot.barmode = mode;
        plot.add_bar2d(vec![0.0, 1.0], vec![3.0, 4.0], [200, 0, 0], None, YAxis::Primary);
        plot.add_bar2d(vec![0.0, 1.0], vec![2.0, 5.0], [0, 0, 200], None, YAxis::Primary);
        plot
    }

    fn y_extent(plot: &Plot) -> (f64, f64) {
        let (_, _, ylo, yhi, _) = plot.bounds_2d();
        (ylo, yhi)
    }

    /// Overlay is the historical behaviour and must stay bit-for-bit: two
    /// series at the same positions overplot, and the axis is sized for the
    /// taller one alone.
    #[test]
    fn overlay_leaves_bars_full_width_and_unstacked() {
        let plot = two_bar_series(BarMode::Overlay);
        let hw = bar_halfwidth(&[0.0, 1.0]) as f64;
        assert_eq!(plot.bar_geometry(0, 0.0, 3.0, hw), (-hw, hw, 0.0, 3.0));
        assert_eq!(plot.bar_geometry(1, 0.0, 2.0, hw), (-hw, hw, 0.0, 2.0));
        // The taller single bar is 5, not the 9 a stack would reach.
        assert!(y_extent(&plot).1 < 9.0);
    }

    #[test]
    fn grouping_splits_the_slot_without_overlapping() {
        let plot = two_bar_series(BarMode::Group);
        let hw = bar_halfwidth(&[0.0, 1.0]) as f64;
        let (a0, a1, ..) = plot.bar_geometry(0, 0.0, 3.0, hw);
        let (b0, b1, ..) = plot.bar_geometry(1, 0.0, 2.0, hw);
        assert!(a1 <= b0, "grouped bars must not overlap: {a1} then {b0}");
        assert!((a0 - -hw).abs() < 1e-9 && (b1 - hw).abs() < 1e-9, "they fill the slot");
        assert!(((a1 - a0) - (b1 - b0)).abs() < 1e-9, "and split it evenly");
        // Grouping does not change the value axis.
        assert!(y_extent(&plot).1 < 9.0);
    }

    #[test]
    fn stacking_lifts_the_baseline_and_grows_the_axis() {
        let plot = two_bar_series(BarMode::Stack);
        let hw = bar_halfwidth(&[0.0, 1.0]) as f64;
        // The lower trace starts at zero...
        assert_eq!(plot.bar_geometry(0, 0.0, 3.0, hw).2, 0.0);
        // ...and the upper one starts where it ended.
        let (_, _, v0, v1) = plot.bar_geometry(1, 0.0, 2.0, hw);
        assert_eq!((v0, v1), (3.0, 5.0));
        // The axis must reach the tallest total (4 + 5 = 9), not the tallest bar.
        assert!(y_extent(&plot).1 >= 9.0, "the axis must fit the stack");
    }

    /// A hidden trace leaves the stack and the group, rather than holding an
    /// empty slot or a phantom baseline.
    #[test]
    fn hiding_a_trace_removes_it_from_the_stack_and_the_group() {
        let mut plot = two_bar_series(BarMode::Stack);
        plot.set_visible(0, false).unwrap();
        let hw = bar_halfwidth(&[0.0, 1.0]) as f64;
        assert_eq!(plot.bar_geometry(1, 0.0, 2.0, hw).2, 0.0, "the survivor falls to zero");

        let mut plot = two_bar_series(BarMode::Group);
        plot.set_visible(0, false).unwrap();
        assert_eq!(plot.bar_slot(1), (0, 1), "the survivor takes the whole slot");
    }

    /// Mixed signs grow both ways from the baseline instead of cancelling: a
    /// net bar would hide both contributions behind one number.
    #[test]
    fn stacking_keeps_positive_and_negative_apart() {
        let mut plot = Plot::new();
        plot.barmode = BarMode::Stack;
        plot.add_bar2d(vec![0.0], vec![4.0], [200, 0, 0], None, YAxis::Primary);
        plot.add_bar2d(vec![0.0], vec![-3.0], [0, 0, 200], None, YAxis::Primary);
        plot.add_bar2d(vec![0.0], vec![2.0], [0, 200, 0], None, YAxis::Primary);
        let hw = bar_halfwidth(&[0.0]) as f64;
        // The negative starts at zero, not at +4.
        assert_eq!(plot.bar_geometry(1, 0.0, -3.0, hw), (-hw, hw, -3.0, 0.0));
        // The third stacks on the positives only.
        assert_eq!(plot.bar_geometry(2, 0.0, 2.0, hw).2, 4.0);
        let (lo, hi) = y_extent(&plot);
        assert!(lo <= -3.0 && hi >= 6.0, "both directions must fit: {lo}..{hi}");
    }

    #[test]
    fn a_band_fills_between_its_edges_and_sizes_the_axis_to_both() {
        let mut plot = Plot::new();
        plot.add_band2d(
            vec![0.0, 1.0, 2.0],
            vec![1.0, 0.0, 1.0],
            vec![4.0, 5.0, 4.0],
            [40, 90, 160],
            Some("ci".into()),
            YAxis::Primary,
        );
        let (_, _, ylo, yhi) = {
            let (a, b, c, d, _) = plot.bounds_2d();
            (a, b, c, d)
        };
        assert!(ylo <= 0.0 && yhi >= 5.0, "the axis must fit both edges: {ylo}..{yhi}");

        let fb = plot.render(240, 180);
        let filled = (0..fb.h)
            .map(|y| (0..fb.w).filter(|&x| px(&fb, x, y) == Some([40, 90, 160])).count())
            .sum::<usize>();
        assert!(filled > 2000, "the ribbon must be a solid area, got {filled}");

        // Crossed edges are a band, not an error: the fill is between them.
        let mut crossed = Plot::new();
        crossed.add_band2d(
            vec![0.0, 1.0],
            vec![0.0, 5.0],
            vec![5.0, 0.0],
            [40, 90, 160],
            None,
            YAxis::Primary,
        );
        assert!(crossed.render(240, 180).rgba().chunks(4).any(|p| p[3] > 0));
    }

    /// Error bars reach past the points they qualify, so the axis has to make
    /// room for them or their caps are clipped off.
    #[test]
    fn error_bars_widen_the_axis_and_draw_capped_spines() {
        let mut plot = Plot::new();
        let id = plot.add_scatter2d(
            vec![1.0, 2.0],
            vec![1.0, 1.0],
            [220, 40, 40],
            2.0,
            None,
            YAxis::Primary,
        );
        let (.., before_lo, before_hi, _) = plot.bounds_2d();
        plot.set_error_bars(id, None, Some(ErrBars { plus: vec![3.0, 3.0], minus: None })).unwrap();
        let (.., after_lo, after_hi, _) = plot.bounds_2d();
        assert!(after_hi > before_hi && after_lo < before_lo, "the axis must grow both ways");
        assert!(after_hi >= 4.0, "and reach the cap at y+3");

        // Asymmetric bars are honoured on each side independently.
        plot.set_error_bars(
            id,
            None,
            Some(ErrBars { plus: vec![3.0, 3.0], minus: Some(vec![0.0, 0.0]) }),
        )
        .unwrap();
        let (.., lo, hi, _) = plot.bounds_2d();
        assert!(hi >= 4.0 && lo > after_lo, "minus=0 must not extend downward");

        // They take the series' color and are drawn, not merely accounted for.
        let fb = plot.render(240, 180);
        let red = (0..fb.h)
            .map(|y| (0..fb.w).filter(|&x| px(&fb, x, y) == Some([220, 40, 40])).count())
            .sum::<usize>();
        let mut bare = Plot::new();
        bare.add_scatter2d(
            vec![1.0, 2.0],
            vec![1.0, 1.0],
            [220, 40, 40],
            2.0,
            None,
            YAxis::Primary,
        );
        let bare_fb = bare.render(240, 180);
        let bare_red = (0..bare_fb.h)
            .map(|y| (0..bare_fb.w).filter(|&x| px(&bare_fb, x, y) == Some([220, 40, 40])).count())
            .sum::<usize>();
        assert!(red > bare_red, "bars must add drawn area");

        assert_eq!(plot.set_error_bars(99, None, None), Err(TraceError::UnknownTrace));
        let bars = plot.add_bar2d(vec![0.0], vec![1.0], [1, 2, 3], None, YAxis::Primary);
        assert_eq!(plot.set_error_bars(bars, None, None), Err(TraceError::WrongKind));
    }

    /// A short `plus` list leaves later points without bars rather than
    /// truncating the series.
    #[test]
    fn short_error_arrays_leave_later_points_bare() {
        let e = ErrBars { plus: vec![1.0], minus: None };
        assert_eq!(e.at(0), Some((1.0, 1.0)));
        assert_eq!(e.at(1), None);
        let nan = ErrBars { plus: vec![f32::NAN], minus: None };
        assert_eq!(nan.at(0), None, "a non-finite bar is no bar");
    }

    /// Tukey's rule, checked against a hand-computed case: the whiskers stop
    /// at real data inside the fence, and everything past it is an outlier in
    /// its own right rather than a longer whisker.
    #[test]
    fn box_stats_follow_tukey_and_separate_outliers() {
        // 1..=9 plus a far outlier, type-7 quantiles over the 10 sorted
        // values: q1 = 3.25, median = 5.5, q3 = 7.75, so IQR = 4.5 and the
        // upper fence is 7.75 + 1.5·4.5 = 14.5 — clear of 9, well short of 100.
        let v: Vec<f32> = vec![1., 2., 3., 4., 5., 6., 7., 8., 9., 100.];
        let st = box_stats(&v).unwrap();
        assert!((st.q1 - 3.25).abs() < 1e-9, "q1 = {}", st.q1);
        assert!((st.median - 5.5).abs() < 1e-9, "median = {}", st.median);
        assert!((st.q3 - 7.75).abs() < 1e-9, "q3 = {}", st.q3);
        assert_eq!(st.lo, 1.0, "the low whisker stops at real data");
        assert_eq!(st.hi, 9.0, "the high whisker stops before the outlier");
        assert_eq!(st.outliers, vec![100.0], "and 100 stands alone");

        // Degenerate samples stay representable.
        assert!(box_stats(&[]).is_none());
        let one = box_stats(&[7.0]).unwrap();
        assert_eq!((one.q1, one.median, one.q3, one.lo, one.hi), (7.0, 7.0, 7.0, 7.0, 7.0));
        assert!(one.outliers.is_empty());
        // Non-finite values are dropped, not counted.
        assert_eq!(box_stats(&[f32::NAN, 5.0]).unwrap().median, 5.0);
    }

    #[test]
    fn box_groups_tolerate_malformed_offsets() {
        let v = [1.0f32, 2.0, 3.0];
        let got: Vec<usize> = box_groups(&v, &[0, 2]).map(<[f32]>::len).collect();
        assert_eq!(got, vec![2, 1], "the last group runs to the end");
        // Backwards or past the end yields an empty group, never a panic.
        let got: Vec<usize> = box_groups(&v, &[0, 99]).map(<[f32]>::len).collect();
        assert_eq!(got, vec![0, 0]);
        let got: Vec<usize> = box_groups(&v, &[2, 1]).map(<[f32]>::len).collect();
        assert_eq!(got, vec![0, 2]);
    }

    #[test]
    fn a_box_plot_draws_its_parts_and_frames_its_outliers() {
        let mut plot = Plot::new();
        let a: Vec<f32> = (1..=9).map(|i| i as f32).chain([100.0]).collect();
        let b: Vec<f32> = (1..=9).map(|i| i as f32 * 2.0).collect();
        let mut values = a.clone();
        values.extend_from_slice(&b);
        let id = plot.add_box2d(
            values,
            vec![0, a.len() as u32],
            [220, 40, 40],
            Orient::Vertical,
            Some("groups".into()),
            YAxis::Primary,
        );
        // The axis must reach the outlier — the one point you most need to see.
        let (_, _, _, yhi, _) = plot.bounds_2d();
        assert!(yhi >= 100.0, "the frame must contain the outlier, got {yhi}");
        // Two boxes, one unit apart, padded by half a box each side.
        let (xlo, xhi, ..) = plot.bounds_2d();
        assert!(xlo < 0.0 && xhi > 1.0);

        let fb = plot.render(320, 240);
        let full = (0..fb.h)
            .map(|y| (0..fb.w).filter(|&x| px(&fb, x, y) == Some([220, 40, 40])).count())
            .sum::<usize>();
        let dim = shade([220, 40, 40], 0.55);
        let boxed = (0..fb.h)
            .map(|y| (0..fb.w).filter(|&x| px(&fb, x, y) == Some(dim)).count())
            .sum::<usize>();
        assert!(boxed > 100, "the IQR box must be a real area, got {boxed}");
        assert!(full > 50, "median, whiskers and outliers draw at full colour");

        let (kind, _) = plot.traces[id].structural_reason().unwrap();
        assert_eq!(kind, "box");
    }

    fn demo_2d_plot() -> Plot {
        let mut plot = Plot::new();
        plot.add_scatter2d(
            vec![0.0, 1.0, 2.0],
            vec![0.0, 1.0, 2.0],
            [200, 120, 60],
            2.5,
            None,
            YAxis::Primary,
        );
        plot
    }

    #[test]
    fn a_colorbar_reserves_its_own_margin_and_paints_the_ramp() {
        let mut plot = demo_2d_plot();
        let without = plot.layout_2d(400, 240);
        plot.colorbar = Some(Colorbar { map: Colormap::Viridis, lo: 0.0, hi: 100.0, label: None });
        let with = plot.layout_2d(400, 240);

        // The plot gives up width rather than drawing over itself.
        assert!(with.x1 < without.x1, "the colorbar must take its own margin");
        let cb = with.cbar.as_ref().expect("a colorbar was set");
        assert!(cb.x0 > with.x1, "the strip sits outside the plot rect");
        assert_eq!((cb.y0, cb.y1), (with.y0, with.y1), "the ramp spans the plot height");
        assert!(!cb.ticks.is_empty() && cb.ticks.len() == cb.labels.len());

        // The painted ramp is the colormap itself, top = hi.
        let fb = plot.render(400, 240);
        let mid = (cb.x0 + cb.x1) / 2;
        let top = px(&fb, mid as usize, (cb.y0 + 2) as usize).unwrap();
        let bot = px(&fb, mid as usize, (cb.y1 - 2) as usize).unwrap();
        assert_ne!(top, bot, "the ramp must vary along its length");
        let near = |a: Rgb, b: Rgb| (0..3).all(|i| (a[i] as i32 - b[i] as i32).abs() <= 24);
        assert!(near(top, Colormap::Viridis.sample(1.0)), "top of the ramp is `hi`");
        assert!(near(bot, Colormap::Viridis.sample(0.0)), "bottom of the ramp is `lo`");
    }

    /// The reserved margin has to cover everything `draw_colorbar` walks —
    /// the strip, its tick, and the widest label — or the outermost digits
    /// fall off the frame.
    #[test]
    fn a_colorbar_reserves_room_for_its_widest_label() {
        let mut plot = demo_2d_plot();
        plot.colorbar = Some(Colorbar { map: Colormap::Viridis, lo: 0.0, hi: 1000.0, label: None });
        let (w, h) = (420, 260);
        let l = plot.layout_2d(w, h);
        let cb = l.cbar.as_ref().unwrap();
        let widest = cb.labels.iter().map(|t| text_width(t, l.s)).max().unwrap();
        let right_edge = cb.x1 + 2 * l.s + 3 * l.s + widest;
        assert!(right_edge <= w as i32, "label runs to {right_edge}, past the {w}px frame");
    }

    #[test]
    fn a_colorbar_caption_buys_its_space_from_the_top_margin() {
        let mut plot = demo_2d_plot();
        plot.colorbar = Some(Colorbar { map: Colormap::Plasma, lo: 0.0, hi: 1.0, label: None });
        let plain = plot.layout_2d(400, 240);
        plot.colorbar =
            Some(Colorbar { map: Colormap::Plasma, lo: 0.0, hi: 1.0, label: Some("kW".into()) });
        let captioned = plot.layout_2d(400, 240);
        assert!(captioned.y0 > plain.y0, "a caption pushes the frame down to fit");
    }

    /// A colorbar and the right-hand axes stack outward instead of colliding.
    #[test]
    fn a_colorbar_stacks_outside_the_right_axis_columns() {
        let mut plot = demo_2d_plot();
        plot.add_line2d(
            vec![0.0, 1.0, 2.0],
            vec![100.0, 200.0, 300.0],
            [60, 200, 210],
            2.0,
            None,
            YAxis::Y2,
        );
        plot.colorbar = Some(Colorbar { map: Colormap::Viridis, lo: 0.0, hi: 10.0, label: None });
        let l = plot.layout_2d(500, 260);
        let cb = l.cbar.as_ref().unwrap();
        assert!(l.has_right[0]);
        // The strip clears the innermost tick-label column.
        assert!(
            cb.x0 > l.x1 + l.col_x[0],
            "the ramp must sit outside the y2 labels, not over them"
        );
        assert!(cb.x1 < 500, "and stay on the framebuffer");
    }

    #[test]
    fn category_ticks_land_on_integers_and_clip_to_the_view() {
        let names = cat(&["Mon", "Tue", "Wed", "Thu"]);
        let (pos, labels) = category_ticks(&names, -0.5, 3.5, 10);
        assert_eq!(pos, vec![0.0, 1.0, 2.0, 3.0]);
        assert_eq!(labels, names);

        // Only what is on screen, and never off the end of the name list.
        let (pos, labels) = category_ticks(&names, 1.2, 99.0, 10);
        assert_eq!(pos, vec![2.0, 3.0]);
        assert_eq!(labels, cat(&["Wed", "Thu"]));

        // A view that contains no whole category emits nothing.
        assert_eq!(category_ticks(&names, 1.2, 1.8, 10).0, Vec::<f64>::new());
        assert_eq!(category_ticks(&[], 0.0, 5.0, 10).0, Vec::<f64>::new());
        assert_eq!(category_ticks(&names, f64::NAN, 3.0, 10).0, Vec::<f64>::new());
    }

    /// Thinning drops whole strides, so surviving labels stay on their own
    /// categories rather than drifting onto neighbours.
    #[test]
    fn category_ticks_thin_by_a_whole_stride() {
        let names: Vec<String> = (0..10).map(|i| format!("c{i}")).collect();
        let (pos, labels) = category_ticks(&names, 0.0, 9.0, 3);
        assert_eq!(pos, vec![0.0, 4.0, 8.0]);
        assert_eq!(labels, cat(&["c0", "c4", "c8"]));
        for (p, l) in pos.iter().zip(&labels) {
            assert_eq!(*l, names[*p as usize], "a label drifted off its category");
        }
    }

    #[test]
    fn a_categorical_axis_labels_ticks_and_the_crosshair_alike() {
        let mut plot = Plot::new();
        plot.add_bar2d(
            vec![0.0, 1.0, 2.0],
            vec![3.0, 5.0, 4.0],
            [200, 120, 60],
            None,
            YAxis::Primary,
        );
        plot.x_categories = Some(cat(&["alpha", "beta", "gamma"]));
        let l = plot.layout_2d(400, 240);
        assert_eq!(l.xlabels, cat(&["alpha", "beta", "gamma"]));
        assert_eq!(l.xticks, vec![0.0, 1.0, 2.0]);
        // The readout must agree with the ticks beneath it.
        assert_eq!(plot.format_x(1.0), "beta");
        // Categories win over a time axis, and a position that is not a
        // category falls back rather than mislabelling.
        plot.x_epoch = Some(1.7e9);
        assert_eq!(plot.format_x(1.0), "beta");
        assert_eq!(plot.format_x(1.5), format_datetime(1.7e9 + 1.5));
    }

    /// The y axis carries its own labels now, so a categorical y is possible
    /// at all — and the numeric default must be unchanged.
    #[test]
    fn the_y_axis_carries_labels_numeric_or_categorical() {
        let mut plot = Plot::new();
        plot.add_scatter2d(
            vec![0.0, 1.0, 2.0],
            vec![0.0, 1.0, 2.0],
            [200, 120, 60],
            2.5,
            None,
            YAxis::Primary,
        );
        let l = plot.layout_2d(400, 240);
        assert_eq!(l.ylabels.len(), l.yticks.len(), "every y tick carries a label");
        assert!(!l.ylabels.is_empty());
        let numeric = l.ylabels.clone();

        plot.y_categories = Some(cat(&["low", "mid", "high"]));
        let l = plot.layout_2d(400, 240);
        assert_eq!(l.ylabels, cat(&["low", "mid", "high"]));
        assert_eq!(l.yticks, vec![0.0, 1.0, 2.0]);
        assert_ne!(l.ylabels, numeric);

        // Right axes stay numeric: they carry a second scale, not names.
        assert_eq!(l.rlabels[0].len(), l.rticks[0].len());
    }

    #[test]
    fn fill_between_fills_the_interior_and_nothing_else() {
        let mut fb = Framebuffer::new(40, 40);
        // A flat band spanning y = 10..=20 across the full width.
        fb.fill_between(&[(0.0, 10.0, 20.0), (39.0, 10.0, 20.0)], 0.0, BAND);
        assert_eq!(px(&fb, 20, 15), Some(BAND), "the interior fills");
        assert_eq!(px(&fb, 20, 10), Some(BAND), "the low edge is inclusive");
        assert_eq!(px(&fb, 20, 20), Some(BAND), "the high edge is inclusive");
        assert_eq!(px(&fb, 20, 9), None, "nothing above the band");
        assert_eq!(px(&fb, 20, 21), None, "nothing below the band");
    }

    /// The property the column sweep exists for: where a confidence interval
    /// pinches to zero the ribbon must stay continuous, not disappear.
    #[test]
    fn fill_between_stays_continuous_where_it_pinches_to_zero() {
        let mut fb = Framebuffer::new(40, 40);
        fb.fill_between(&[(0.0, 14.0, 26.0), (20.0, 20.0, 20.0), (39.0, 14.0, 26.0)], 0.0, BAND);
        // Every column across the sweep must have drawn something, including
        // the waist where lo == hi.
        for x in 0..40 {
            assert!(
                (0..40).any(|y| px(&fb, x, y).is_some()),
                "column {x} is empty; the band broke where it narrowed"
            );
        }
    }

    #[test]
    fn fill_between_breaks_at_a_non_finite_column() {
        let mut fb = Framebuffer::new(40, 20);
        fb.fill_between(
            &[(0.0, 5.0, 15.0), (10.0, 5.0, 15.0), (20.0, f64::NAN, 15.0), (39.0, 5.0, 15.0)],
            0.0,
            BAND,
        );
        assert_eq!(px(&fb, 5, 10), Some(BAND), "the run before the gap fills");
        // Both spans touching the NaN column are skipped, so the middle is bare.
        assert_eq!(px(&fb, 15, 10), None, "the span into the gap is skipped");
        assert_eq!(px(&fb, 30, 10), None, "the span out of the gap is skipped");
    }

    #[test]
    fn fill_between_honors_the_clip_rect_and_offscreen_columns() {
        let mut fb = Framebuffer::new(40, 40);
        fb.set_clip(10, 10, 20, 20);
        fb.fill_between(&[(0.0, 0.0, 39.0), (39.0, 0.0, 39.0)], 0.0, BAND);
        assert_eq!(px(&fb, 15, 15), Some(BAND), "inside the clip draws");
        assert_eq!(px(&fb, 5, 15), None, "outside the clip does not");
        fb.clear_clip();

        // A column mapped far off screen must draw nothing and, more to the
        // point, must not walk the whole way there.
        let mut fb = Framebuffer::new(40, 40);
        fb.fill_between(&[(-9000.0, 10.0, 20.0), (-8000.0, 10.0, 20.0)], 0.0, BAND);
        assert!((0..40).all(|x| (0..40).all(|y| px(&fb, x, y).is_none())));
    }

    /// The cached `pad` is the contract between the bounds scan and the
    /// renderer: both read this one number, so a bar can never be drawn wider
    /// than the range that was sized for it. Bars pad x only — padding y would
    /// lift the zero baseline off the axis.
    #[test]
    fn bar_bounds_pad_x_by_the_halfwidth_and_never_y() {
        let mut plot = Plot::new();
        let id = plot.add_bar2d(
            vec![0.0, 1.0, 2.0],
            vec![3.0, 5.0, 4.0],
            [200, 120, 60],
            None,
            YAxis::Primary,
        );
        let hw = bar_halfwidth(&[0.0, 1.0, 2.0]) as f64;
        assert_eq!(plot.cached_pad(id), Some((hw, 0.0)));

        let CachedBounds::B2 { xlo, xhi, ylo, yhi, .. } = plot.meta[id].bounds else {
            panic!("a 2D trace must cache a B2 box");
        };
        // x carries the drawn half-width on both sides...
        assert!((xlo - (0.0 - hw)).abs() < 1e-9, "xlo {xlo} should be -hw");
        assert!((xhi - (2.0 + hw)).abs() < 1e-9, "xhi {xhi} should be 2+hw");
        // ...while y is exactly baseline-to-tallest, unpadded.
        assert_eq!((ylo, yhi), (0.0, 5.0));
    }

    /// A scatter's extent is its points, so it stores no pad and `bar_hw`
    /// falls back to recomputing rather than reading a stale zero.
    #[test]
    fn point_traces_cache_no_pad() {
        let mut plot = Plot::new();
        let id = plot.add_scatter2d(
            vec![0.0, 1.0],
            vec![0.0, 1.0],
            [200, 120, 60],
            2.5,
            None,
            YAxis::Primary,
        );
        assert_eq!(plot.cached_pad(id), None);
        let xs = [0.0f32, 1.0];
        assert_eq!(plot.bar_hw(id, &xs, Orient::Vertical), bar_halfwidth(&xs) as f64);
    }

    #[test]
    fn renders_nonempty() {
        let mut plot = Plot::new();
        plot.add_scatter3d(
            vec![[0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [-1.0, 0.5, -1.0]],
            [230, 60, 120],
            3.0,
            None,
        );
        let fb = plot.render(200, 120);
        let rgba = fb.rgba();
        assert_eq!(rgba.len(), 200 * 120 * 4);
        assert!(rgba.chunks(4).any(|px| px[3] > 0));
    }

    #[test]
    fn pick_finds_node_at_its_projected_position() {
        let mut plot = Plot::new();
        let nodes = vec![[0.0, 0.0, 0.0], [5.0, 5.0, 5.0], [-5.0, -5.0, -5.0]];
        plot.add_graph3d(
            nodes.clone(),
            vec![[200, 100, 100]; 3],
            vec![(0, 1), (1, 2)],
            3.0,
            None,
            None,
            None,
            None,
        );
        // Project node 1 and click exactly there — pick must return index 1.
        let (pr, _, _) = plot.projector(300, 200, 1.0);
        let s = pr.project(nodes[1]);
        let hit = plot.pick(300, 200, s[0], s[1], 4.0);
        assert_eq!(hit, Some(1));
    }

    #[test]
    fn project_nodes_matches_pick_geometry() {
        let mut plot = Plot::new();
        let nodes = vec![[0.0, 0.0, 0.0], [5.0, 5.0, 5.0], [-5.0, -5.0, -5.0]];
        plot.add_graph3d(
            nodes,
            vec![[200, 100, 100]; 3],
            vec![(0, 1)],
            3.0,
            None,
            None,
            None,
            None,
        );
        plot.camera.rotate(0.3, -0.2);
        plot.camera.zoom_by(1.7);
        plot.camera.pan(11.0, -6.0);
        for (i, s) in plot.project_nodes(300, 200).iter().enumerate() {
            assert_eq!(plot.pick(300, 200, s[0], s[1], 2.0), Some(i));
        }
    }

    #[test]
    fn camera_state_roundtrip_clamps() {
        let mut cam = Camera::default();
        cam.set_state(2.0, 9.9, 500.0, 3.0, -4.0);
        assert_eq!(cam.state(), (2.0, 1.55, 50.0, 3.0, -4.0));
    }

    /// A node with an explicit larger radius lights more pixels than its
    /// uniform-size twin.
    #[test]
    fn node_sizes_change_drawn_area() {
        let lit = |node_sizes: Option<Vec<f32>>| -> usize {
            let mut plot = Plot::new();
            plot.show_box = false;
            // On the x axis both nodes stay on-screen under the default tilt.
            plot.add_graph3d(
                vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
                vec![[255, 255, 255]; 2],
                vec![],
                2.0,
                node_sizes,
                None,
                None,
                None,
            );
            plot.render(200, 200).rgba().chunks(4).filter(|px| px[3] > 0).count()
        };
        // Node 0 projects to the canvas center, so its radius change is
        // guaranteed to be visible regardless of the default camera tilt.
        assert!(lit(Some(vec![8.0, 2.0])) > lit(None));
    }

    /// Lit-pixel count of one centred node drawn with `shape`.
    fn lit_with(shape: Shape, r: f32) -> usize {
        let mut plot = Plot::new();
        plot.show_box = false;
        plot.add_graph3d(
            vec![[0.0, 0.0, 0.0]],
            vec![[255, 255, 255]],
            vec![],
            r,
            None,
            None,
            Some(vec![shape]),
            None,
        );
        plot.render(200, 200).rgba().chunks(4).filter(|px| px[3] > 0).count()
    }

    /// Every shape is its own silhouette: the solid ones order by area
    /// (diamond and triangle inside the square), the open/small ones are
    /// lighter than their filled twins, and the disc matches `disc`.
    #[test]
    fn shapes_render_as_distinct_marks() {
        let r = 6.0;
        let disc = lit_with(Shape::Disc, r);
        let square = lit_with(Shape::Square, r);
        let diamond = lit_with(Shape::Diamond, r);
        let triangle = lit_with(Shape::Triangle, r);
        assert!(square > disc);
        assert!(diamond < square && triangle < square);
        assert!(lit_with(Shape::Ring, r) < disc);
        assert!(lit_with(Shape::DiamondOpen, r) < diamond);
        assert!(lit_with(Shape::Dot, r) < disc);
        assert_eq!(lit_with(Shape::Disc, r), {
            let mut plot = Plot::new();
            plot.show_box = false;
            plot.add_graph3d(
                vec![[0.0, 0.0, 0.0]],
                vec![[255, 255, 255]],
                vec![],
                r,
                None,
                None,
                None,
                None,
            );
            plot.render(200, 200).rgba().chunks(4).filter(|px| px[3] > 0).count()
        });
    }

    /// Hover and selection halos are drawn as the filled silhouette, so an
    /// open shape lights up as one solid blob rather than a thin outline.
    #[test]
    fn open_shape_halos_are_solid() {
        for shape in [Shape::Ring, Shape::DiamondOpen] {
            let plain = lit_with(shape, 6.0);
            let mut plot = Plot::new();
            plot.show_box = false;
            plot.add_graph3d(
                vec![[0.0, 0.0, 0.0]],
                vec![[255, 255, 255]],
                vec![],
                6.0,
                None,
                None,
                Some(vec![shape]),
                None,
            );
            plot.hovered = Some(Element::Node(0));
            let lit = plot.render(200, 200).rgba().chunks(4).filter(|px| px[3] > 0).count();
            assert!(lit > plain, "{shape:?}: hovered {lit} <= plain {plain}");
        }
    }

    #[test]
    fn shape_names_round_trip() {
        for name in Shape::NAMES {
            let shape = Shape::parse(name).expect(name);
            assert_eq!(Shape::NAMES[shape as usize], name);
        }
        assert_eq!(Shape::parse("blob"), None);
    }

    /// With a pinned frame, a node projects to the same pixel whether or not
    /// the other nodes that would have widened the bounding box are present.
    #[test]
    fn bounds_override_pins_the_projection() {
        let frame = ([-4.0, -4.0, 0.0], [4.0, 4.0, 0.0]);
        let project = |pts: Vec<[f32; 3]>, pin: bool| {
            let mut plot = Plot::new();
            plot.show_box = false;
            plot.add_graph3d(pts, vec![[255, 255, 255]; 2], vec![], 2.0, None, None, None, None);
            plot.bounds_override = pin.then_some(frame);
            plot.project_nodes(200, 200)[0]
        };
        let full = vec![[1.0, 1.0, 0.0], [-4.0, -4.0, 0.0]];
        let part = vec![[1.0, 1.0, 0.0], [1.0, 1.0, 0.0]];
        assert_ne!(project(full.clone(), false)[..2], project(part.clone(), false)[..2]);
        assert_eq!(project(full, true)[..2], project(part, true)[..2]);
    }

    #[test]
    fn line3d_draws_but_is_not_pickable() {
        let mut plot = Plot::new();
        plot.show_box = false;
        let pts = vec![[0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [2.0, 0.0, 1.0]];
        plot.add_line3d(pts.clone(), [255, 0, 0], 2.0, None);
        let lit = plot.render(200, 200).rgba().chunks(4).filter(|px| px[3] > 0).count();
        assert!(lit > 0, "line3d drew nothing");
        assert_eq!(plot.node_count(), 0);
        assert_eq!(plot.vertex_count(), 3);
        let (pr, _, _) = plot.projector(200, 200, 1.0);
        let s = pr.project(pts[0]);
        assert_eq!(plot.pick(200, 200, s[0], s[1], 10.0), None);
    }

    /// A far-away line vertex widens the bounds a scatter point projects in.
    #[test]
    fn line3d_extends_bounds() {
        let project_node0 = |with_line: bool| {
            let mut plot = Plot::new();
            plot.add_scatter3d(vec![[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0]], [255, 255, 255], 2.0, None);
            if with_line {
                plot.add_line3d(vec![[8.0, 0.0, 0.0], [9.0, 0.0, 0.0]], [255, 0, 0], 1.0, None);
            }
            plot.project_nodes(200, 200)[0]
        };
        assert_ne!(project_node0(false)[..2], project_node0(true)[..2]);
    }

    #[test]
    fn wider_line3d_lights_more_pixels() {
        let lit = |width: f32| -> usize {
            let mut plot = Plot::new();
            plot.show_box = false;
            plot.add_line3d(vec![[-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]], [255, 255, 255], width, None);
            plot.render(200, 200).rgba().chunks(4).filter(|px| px[3] > 0).count()
        };
        assert!(lit(6.0) > lit(1.0));
    }

    /// Non-finite vertices break the polyline instead of drawing through it.
    #[test]
    fn line3d_gap_reduces_drawn_pixels() {
        let lit = |mid: [f32; 3]| -> usize {
            let mut plot = Plot::new();
            plot.show_box = false;
            plot.add_line3d(
                vec![[-1.0, 0.0, 0.0], mid, [1.0, 0.0, 0.0]],
                [255, 255, 255],
                1.0,
                None,
            );
            plot.bounds_override = Some(([-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]));
            plot.render(200, 200).rgba().chunks(4).filter(|px| px[3] > 0).count()
        };
        assert!(lit([f32::NAN; 3]) < lit([0.0, 0.5, 0.0]));
    }

    /// The nearer of two overlapping triangles wins, regardless of draw order.
    #[test]
    fn tri_zbuffer_keeps_the_front_triangle() {
        for flip in [false, true] {
            let mut fb = Framebuffer::new(40, 40);
            let far = ([0.0, 0.0, 1.0], [39.0, 0.0, 1.0], [0.0, 39.0, 1.0]);
            let near = ([0.0, 0.0, -1.0], [39.0, 0.0, -1.0], [0.0, 39.0, -1.0]);
            let draws = [(far, [200, 0, 0]), (near, [0, 200, 0])];
            let order: Vec<_> =
                if flip { draws.iter().rev().collect() } else { draws.iter().collect() };
            for (t, c) in order {
                fb.tri(t.0, t.1, t.2, *c);
            }
            let rgba = fb.rgba();
            let i = (10 * 40 + 10) * 4;
            assert_eq!(&rgba[i..i + 3], &[0, 200, 0], "flip={flip}");
        }
    }

    #[test]
    fn colormap_samples_hit_the_anchor_stops() {
        assert_eq!(Colormap::Viridis.sample(0.0), [68, 1, 84]);
        assert_eq!(Colormap::Viridis.sample(1.0), [253, 231, 37]);
        assert_eq!(Colormap::Plasma.sample(0.0), [13, 8, 135]);
        assert_eq!(Colormap::Plasma.sample(5.0), [240, 249, 33]); // clamped
        for name in Colormap::NAMES {
            assert!(Colormap::parse(name).is_some());
        }
        assert_eq!(Colormap::parse("magma"), None);
    }

    #[test]
    fn surface_draws_and_is_not_pickable() {
        let mut plot = Plot::new();
        plot.show_box = false;
        let (xs, ys) = (vec![0.0, 1.0, 2.0], vec![0.0, 1.0]);
        let zs = vec![0.0, 0.5, 0.0, 0.5, 1.0, 0.5];
        plot.add_surface3d(xs, ys, zs, [200, 60, 60], None, false, None);
        let lit = plot.render(200, 200).rgba().chunks(4).filter(|px| px[3] > 0).count();
        assert!(lit > 100, "surface drew {lit} pixels");
        assert_eq!(plot.node_count(), 0);
        assert_eq!(plot.vertex_count(), 6);
    }

    /// A hole (non-finite corner) removes that cell's pixels only.
    #[test]
    fn surface_holes_reduce_drawn_area() {
        let lit = |corner: f32| -> usize {
            let mut plot = Plot::new();
            plot.show_box = false;
            plot.add_surface3d(
                vec![0.0, 1.0, 2.0],
                vec![0.0, 1.0, 2.0],
                vec![corner, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                [200, 60, 60],
                None,
                false,
                None,
            );
            plot.bounds_override = Some(([0.0, 0.0, -1.0], [2.0, 2.0, 1.0]));
            plot.render(200, 200).rgba().chunks(4).filter(|px| px[3] > 0).count()
        };
        let full = lit(0.0);
        let holed = lit(f32::NAN);
        assert!(holed < full, "hole did not reduce area: {holed} vs {full}");
        assert!(holed > 0, "hole removed the whole surface");
    }

    /// Gouraud shading: a colormapped ramp renders as a near-continuous
    /// gradient, not a handful of flat facet colors.
    #[test]
    fn surface_shading_is_smooth_not_faceted() {
        let mut plot = Plot::new();
        plot.show_box = false;
        let axis: Vec<f32> = (0..4).map(|i| i as f32).collect();
        let zs: Vec<f32> = (0..16).map(|k| (k % 4 + k / 4) as f32).collect();
        plot.add_surface3d(axis.clone(), axis, zs, [0, 0, 0], Some(Colormap::Viridis), false, None);
        let mut colors = std::collections::HashSet::new();
        for px in plot.render(200, 200).rgba().chunks(4) {
            if px[3] > 0 {
                colors.insert([px[0], px[1], px[2]]);
            }
        }
        // 9 cells flat-shaded would give at most ~9 distinct colors (plus
        // fog steps); interpolation gives a gradient with far more.
        assert!(colors.len() > 40, "only {} distinct colors — looks faceted", colors.len());
    }

    /// The colors tri_shaded interpolates lean toward the nearest vertex.
    #[test]
    fn tri_shaded_interpolates_vertex_colors() {
        let mut fb = Framebuffer::new(60, 40);
        fb.tri_shaded(
            [2.0, 2.0, 0.0],
            [58.0, 2.0, 0.0],
            [2.0, 38.0, 0.0],
            [255, 0, 0],
            [0, 255, 0],
            [0, 0, 255],
        );
        let rgba = fb.rgba();
        let at = |x: usize, y: usize| -> [u8; 3] {
            let i = (y * 60 + x) * 4;
            [rgba[i], rgba[i + 1], rgba[i + 2]]
        };
        let near_a = at(5, 4);
        let near_b = at(52, 3);
        assert!(near_a[0] > near_a[1] && near_a[0] > near_a[2], "corner a not reddish: {near_a:?}");
        assert!(
            near_b[1] > near_b[0] && near_b[1] > near_b[2],
            "corner b not greenish: {near_b:?}"
        );
    }

    /// An icosahedron-ish mesh: the six vertices of an octahedron and its
    /// eight faces, wound consistently.
    fn octahedron() -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
        let verts = vec![
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
        ];
        let tris = vec![
            [0, 2, 4],
            [2, 1, 4],
            [1, 3, 4],
            [3, 0, 4],
            [2, 0, 5],
            [1, 2, 5],
            [3, 1, 5],
            [0, 3, 5],
        ];
        (verts, tris)
    }

    #[test]
    fn mesh_draws_and_is_not_pickable() {
        let mut plot = Plot::new();
        plot.show_box = false;
        let (verts, tris) = octahedron();
        plot.add_mesh3d(verts, tris, [200, 60, 60], None, None);
        let lit = plot.render(200, 200).rgba().chunks(4).filter(|px| px[3] > 0).count();
        assert!(lit > 100, "mesh drew {lit} pixels");
        assert_eq!(plot.node_count(), 0);
        assert_eq!(plot.vertex_count(), 6);
    }

    /// A mesh is 3D geometry: one alone routes the plot to the orbit camera.
    #[test]
    fn a_mesh_alone_makes_the_plot_3d() {
        let mut plot = Plot::new();
        let (verts, tris) = octahedron();
        plot.add_mesh3d(verts, tris, [200, 60, 60], None, None);
        assert!(plot.is_3d());
    }

    /// The z-buffer, not the triangle order, decides: a near triangle
    /// covers a far one whichever is drawn first.
    #[test]
    fn mesh_front_triangle_wins_the_depth_test() {
        // 0: the near quad alone, 1: the far quad alone, 2/3: both, in
        // either draw order.
        let probe = |case: usize| -> Rgb {
            let mut plot = Plot::new();
            plot.show_box = false;
            // Two parallel quads facing the default camera, the green one
            // nearer along -y (the camera's look direction at yaw 0).
            let verts = vec![
                [-1.0, 1.0, -1.0],
                [1.0, 1.0, -1.0],
                [1.0, 1.0, 1.0],
                [-1.0, 1.0, 1.0],
                [-1.0, -1.0, -1.0],
                [1.0, -1.0, -1.0],
                [1.0, -1.0, 1.0],
                [-1.0, -1.0, 1.0],
            ];
            let far: Vec<[u32; 3]> = vec![[0, 1, 2], [0, 2, 3]];
            let near: Vec<[u32; 3]> = vec![[4, 5, 6], [4, 6, 7]];
            let tris = match case {
                0 => near.clone(),
                1 => far.clone(),
                2 => [near.clone(), far.clone()].concat(),
                _ => [far.clone(), near.clone()].concat(),
            };
            // Solid colors: the shade/fog terms are the same for both quads,
            // so the winner is identifiable by hue alone.
            plot.add_mesh3d(verts, tris.clone(), [200, 40, 40], None, None);
            plot.bounds_override = Some(([-1.0; 3], [1.0; 3]));
            let rgba = plot.render(200, 200).rgba();
            let i = (100 * 200 + 100) * 4;
            [rgba[i], rgba[i + 1], rgba[i + 2]]
        };
        // Whichever order they arrive in, the pixel is the near quad's —
        // the depth test chose, not the draw order. The far quad alone
        // paints a different (foggier) color, so the check has teeth.
        assert_ne!(probe(0), probe(1), "the two quads are indistinguishable");
        assert_eq!(probe(2), probe(0), "near drawn first");
        assert_eq!(probe(3), probe(0), "far drawn first");
    }

    /// Smooth shading: shared vertices carry averaged normals, so a curved
    /// mesh renders as a gradient rather than a handful of facet colors.
    #[test]
    fn mesh_shading_is_smooth_not_faceted() {
        let n = 24usize;
        let cell = 2.0 / (n - 1) as f32;
        let values: Vec<f32> = (0..n)
            .flat_map(|k| (0..n).flat_map(move |j| (0..n).map(move |i| (i, j, k))))
            .map(|(i, j, k)| {
                let c = |v: usize| -1.0 + v as f32 * cell;
                (c(i) * c(i) + c(j) * c(j) + c(k) * c(k)).sqrt() - 0.7
            })
            .collect();
        let (verts, tris) = marching_cubes(&values, n, n, n, [-1.0; 3], cell, 0.0);
        let mut plot = Plot::new();
        plot.show_box = false;
        plot.add_mesh3d(verts, tris, [0, 0, 0], Some(Colormap::Viridis), None);
        let mut colors = std::collections::HashSet::new();
        for px in plot.render(200, 200).rgba().chunks(4) {
            if px[3] > 0 {
                colors.insert([px[0], px[1], px[2]]);
            }
        }
        assert!(colors.len() > 200, "only {} distinct colors — looks faceted", colors.len());
    }

    /// Bad triangles are skipped, not drawn and not fatal: an out-of-range
    /// index or a non-finite vertex removes just that triangle.
    #[test]
    fn mesh_skips_broken_triangles() {
        let lit = |tris: Vec<[u32; 3]>, nan: bool| -> usize {
            let mut plot = Plot::new();
            plot.show_box = false;
            let mut verts =
                vec![[-1.0, 0.0, -1.0], [1.0, 0.0, -1.0], [1.0, 0.0, 1.0], [-1.0, 0.0, 1.0]];
            if nan {
                verts[2] = [f32::NAN; 3];
            }
            plot.add_mesh3d(verts, tris, [200, 60, 60], None, None);
            plot.bounds_override = Some(([-1.0; 3], [1.0; 3]));
            plot.render(200, 200).rgba().chunks(4).filter(|px| px[3] > 0).count()
        };
        let whole = lit(vec![[0, 1, 2], [0, 2, 3]], false);
        assert!(whole > 100, "quad drew {whole} pixels");
        assert!(lit(vec![[0, 1, 2], [0, 2, 9]], false) < whole, "bad index still drew");
        assert!(lit(vec![[0, 1, 2], [0, 2, 3]], true) < whole, "NaN vertex still drew");
        assert_eq!(lit(vec![[0, 1, 99]], false), 0, "a wholly broken mesh drew pixels");
    }

    /// The wireframe overlay adds pixels of its own on top of the fill.
    #[test]
    fn surface_wireframe_changes_pixels() {
        let render = |wireframe: bool| -> Vec<u8> {
            let mut plot = Plot::new();
            plot.show_box = false;
            plot.add_surface3d(
                vec![0.0, 1.0, 2.0],
                vec![0.0, 1.0, 2.0],
                vec![0.0; 9],
                [200, 60, 60],
                None,
                wireframe,
                None,
            );
            plot.render(200, 200).rgba()
        };
        assert_ne!(render(true), render(false));
    }

    /// A named 3D trace draws a legend; an unnamed one does not. A named 2D
    /// trace mixed into a 3D plot is not drawn, so it gets no entry either.
    #[test]
    fn legend_in_3d_lists_only_drawn_traces() {
        let lit = |name: Option<&str>, with_2d: bool| -> usize {
            let mut plot = Plot::new();
            plot.show_box = false;
            plot.add_line3d(
                vec![[-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
                [255, 0, 0],
                1.0,
                name.map(str::to_owned),
            );
            if with_2d {
                plot.add_line2d(
                    vec![0.0, 1.0],
                    vec![0.0, 1.0],
                    [0, 255, 0],
                    1.0,
                    Some("y".into()),
                    YAxis::Primary,
                );
            }
            plot.render(200, 200).rgba().chunks(4).filter(|px| px[3] > 0).count()
        };
        let unnamed = lit(None, false);
        assert!(lit(Some("trajectory"), false) > unnamed);
        assert_eq!(lit(None, true), unnamed);
    }

    /// A named mesh reaches the 3D legend like any other named trace, with
    /// a swatch sampled from the upper half of its ramp.
    #[test]
    fn named_mesh_draws_a_legend() {
        let lit = |name: Option<String>| -> usize {
            let mut plot = Plot::new();
            plot.show_box = false;
            let (verts, tris) = octahedron();
            plot.add_mesh3d(verts, tris, [0, 0, 0], Some(Colormap::Plasma), name);
            plot.render(200, 200).rgba().chunks(4).filter(|px| px[3] > 0).count()
        };
        assert!(lit(Some("bulb".into())) > lit(None));
    }

    /// `name` on scatter3d and graph3d reaches the legend the same way as
    /// the other named traces.
    #[test]
    fn named_scatter3d_and_graph3d_draw_a_legend() {
        let lit_scatter = |name: Option<String>| -> usize {
            let mut plot = Plot::new();
            plot.show_box = false;
            plot.add_scatter3d(vec![[0.0, 0.0, 0.0]], [255, 0, 0], 2.0, name);
            plot.render(200, 200).rgba().chunks(4).filter(|px| px[3] > 0).count()
        };
        assert!(lit_scatter(Some("clusters".into())) > lit_scatter(None));

        let lit_graph = |name: Option<String>| -> usize {
            let mut plot = Plot::new();
            plot.show_box = false;
            plot.add_graph3d(
                vec![[0.0, 0.0, 0.0]],
                vec![[255, 255, 255]],
                vec![],
                2.0,
                None,
                None,
                None,
                name,
            );
            plot.render(200, 200).rgba().chunks(4).filter(|px| px[3] > 0).count()
        };
        assert!(lit_graph(Some("graph".into())) > lit_graph(None));
    }

    fn crosshair_plot() -> Plot {
        let mut plot = Plot::new();
        plot.add_line2d(
            vec![0.0, 1.0, 2.0, 3.0],
            vec![5.0, 8.0, 3.0, 6.0],
            [230, 60, 120],
            2.0,
            Some("obs".into()),
            YAxis::Primary,
        );
        plot.add_line2d(
            vec![0.0, 1.0, 2.0, 3.0],
            vec![4.0, 6.0, 7.0, 5.0],
            [69, 200, 209],
            2.0,
            None,
            YAxis::Primary,
        );
        plot
    }

    /// Hover draws a crosshair (guide + markers + readout) on top of a 2D
    /// plot; clearing it restores the plain render byte-for-byte.
    #[test]
    fn hover2d_draws_and_clears() {
        let mut plot = crosshair_plot();
        let plain = plot.render(300, 200).rgba();
        plot.hover2d_px = Some(150.0);
        let hovered = plot.render(300, 200).rgba();
        assert_ne!(plain, hovered);
        let lit = |b: &[u8]| b.chunks(4).filter(|p| p[3] > 0).count();
        assert!(lit(&hovered) > lit(&plain), "crosshair added no pixels");
        plot.hover2d_px = None;
        assert_eq!(plot.render(300, 200).rgba(), plain);
    }

    /// The guide snaps to the nearest sample x: two hover positions closer
    /// to each other than to any other sample render identically.
    #[test]
    fn hover2d_snaps_to_samples() {
        let mut plot = crosshair_plot();
        plot.hover2d_px = Some(150.0);
        let a = plot.render(300, 200).rgba();
        plot.hover2d_px = Some(151.5);
        let b = plot.render(300, 200).rgba();
        assert_eq!(a, b);
    }

    /// A 3D plot ignores the 2D hover state entirely.
    #[test]
    fn hover2d_is_ignored_in_3d() {
        let mut plot = Plot::new();
        plot.add_scatter3d(vec![[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]], [230, 60, 120], 3.0, None);
        let plain = plot.render(300, 200).rgba();
        plot.hover2d_px = Some(150.0);
        assert_eq!(plot.render(300, 200).rgba(), plain);
    }

    /// The plot rect, panel inset and panel size the `readout_slot` tests
    /// share, so each one only states the thing it is about.
    const SLOT_RECT: (i32, i32, i32, i32) = (20, 10, 280, 190);
    const SLOT_GAP: i32 = 6;
    const SLOT_W: i32 = 80;
    const SLOT_H: i32 = 50;

    fn slot(px: i32, markers: &[i32], legend: Option<(i32, i32, i32, i32)>) -> (i32, i32) {
        readout_slot(px, SLOT_W, SLOT_H, SLOT_RECT, SLOT_GAP, markers, legend)
    }

    /// The readout takes the half of the frame the markers are not in, so it
    /// never sits on the values it is naming.
    #[test]
    fn readout_sits_opposite_the_data() {
        let (_, y0, _, y1) = SLOT_RECT;
        let mid = (y0 + y1) / 2;
        let (_, high_data) = slot(150, &[30, 40], None);
        assert!(high_data > mid, "markers up top, panel should go low");
        let (_, low_data) = slot(150, &[160, 170], None);
        assert!(low_data + SLOT_H < mid, "markers down low, panel should go high");
    }

    /// With no marker on the frame there is nothing to dodge, so the panel
    /// keeps the top corner it has always used.
    #[test]
    fn readout_without_markers_keeps_the_top_corner() {
        assert_eq!(slot(150, &[], None), (150 + SLOT_GAP, SLOT_RECT.1 + SLOT_GAP));
    }

    /// A legend in the corner the panel wants pushes it to the other side of
    /// the guide rather than being painted over.
    #[test]
    fn readout_dodges_the_legend() {
        // Markers low, so the panel wants the top — the legend's own row.
        let legend = (200, 16, 270, 56);
        let (with_x, with_y) = slot(180, &[160, 170], Some(legend));
        let b = (with_x, with_y, with_x + SLOT_W, with_y + SLOT_H);
        let overlaps = b.0 <= legend.2 && b.2 >= legend.0 && b.1 <= legend.3 && b.3 >= legend.1;
        assert!(!overlaps, "readout {b:?} still covers the legend {legend:?}");
        // And it is the legend that moved it: the same hover without one
        // stays on the preferred right-hand side.
        assert_eq!(slot(180, &[160, 170], None).0, 180 + SLOT_GAP);
    }

    /// Wherever the guide lands, the panel stays inside the plot rect.
    #[test]
    fn readout_stays_in_frame() {
        let (x0, y0, x1, y1) = SLOT_RECT;
        for px in x0..=x1 {
            for markers in [&[30, 40][..], &[160, 170][..], &[][..]] {
                let (bx, by) = slot(px, markers, Some((200, 16, 270, 56)));
                assert!(
                    bx >= x0 && bx + SLOT_W <= x1 && by >= y0 && by + SLOT_H <= y1,
                    "panel at ({bx}, {by}) left the frame for px {px}"
                );
            }
        }
    }

    /// A panel too big for the frame cannot be placed, only pushed back
    /// inside it — it must still land somewhere drawable.
    #[test]
    fn readout_survives_an_oversized_box() {
        let (x0, y0, ..) = SLOT_RECT;
        let (bx, by) = readout_slot(150, 400, 400, SLOT_RECT, SLOT_GAP, &[30], None);
        assert!(bx >= x0 && by >= y0, "oversized panel placed off-frame at ({bx}, {by})");
    }

    /// End to end: hovering near the right edge — where the readout used to
    /// be pinned to the top row and paint straight over the legend — leaves
    /// the legend's swatch untouched.
    #[test]
    fn hover2d_leaves_the_legend_swatch_alone() {
        let mut plot = crosshair_plot();
        let (lx1, ly0, s, three_d) = plot.legend_anchor(300, 200);
        let lb = plot.legend_box(lx1, ly0, s, three_d).expect("the named trace has a row");
        let (ps, bx0, by0) = (lb.ps, lb.bx0, lb.by0);
        let color = lb.rows[0].color;
        // The centre of the first row's chip, mirroring `PanelStyle::chip`.
        let cx = (bx0 + ps.pad_x + ps.swatch / 2) as usize;
        let cy = (by0 + ps.pad_y + (CHAR_H * s - ps.swatch + s) / 2 + ps.swatch / 2) as usize;
        assert_eq!(px(&plot.render(300, 200), cx, cy), Some(color), "wrong probe point");

        plot.hover2d_px = Some(290.0);
        assert_eq!(px(&plot.render(300, 200), cx, cy), Some(color), "the readout ate the legend");
    }

    /// Explicit edge colors reach the framebuffer verbatim (no dimming).
    #[test]
    fn edge_colors_are_used_verbatim() {
        let mut plot = Plot::new();
        plot.show_box = false;
        plot.add_graph3d(
            vec![[-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
            vec![[0, 0, 0]; 2],
            vec![(0, 1)],
            0.5,
            None,
            Some(vec![[9, 250, 9]]),
            None,
            None,
        );
        let fb = plot.render(200, 100);
        let hit =
            fb.rgba().chunks(4).any(|px| px[3] > 0 && px[0] == 9 && px[1] == 250 && px[2] == 9);
        assert!(hit, "explicit edge color not found in framebuffer");
    }

    // ---- axis semantics: titles, explicit ranges, log scales -------------

    /// A title is drawn out of the frame's own margins, never over the plot.
    #[test]
    fn titles_buy_their_space_from_the_margins() {
        let plot = demo_2d_plot();
        let plain = plot.layout_2d(600, 400);

        let mut titled = demo_2d_plot();
        titled.title = Some("p99 latency".into());
        titled.x_title = Some("requests".into());
        titled.y_title = Some("ms".into());
        let l = titled.layout_2d(600, 400);

        assert!(l.y0 > plain.y0, "the chart title pushes the plot area down");
        assert!(l.y1 < plain.y1, "the x title lifts the plot area off the bottom");
        assert!(l.x0 > plain.x0, "the y title widens the left margin");
        assert_eq!(
            (l.title.as_deref(), l.x_title.as_deref(), l.y_title.as_deref()),
            (Some("p99 latency"), Some("requests"), Some("ms")),
        );
    }

    /// Every title has to land inside the margin it was given, or it draws
    /// over the tick labels it was supposed to explain.
    #[test]
    fn titles_stay_inside_their_own_margins() {
        let mut plot = demo_2d_plot();
        plot.title = Some("title".into());
        plot.x_title = Some("x axis".into());
        plot.y_title = Some("y axis".into());
        let (w, h) = (600, 400);
        let l = plot.layout_2d(w, h);
        let fb = plot.render(w, h);

        let ink = plot.chrome.ink_bright;
        let mut top = (h as i32, 0);
        let mut left = (w as i32, 0);
        let mut bottom = 0;
        for y in 0..fb.h {
            for x in 0..fb.w {
                if px(&fb, x, y) != Some(ink) {
                    continue;
                }
                let (x, y) = (x as i32, y as i32);
                if y < l.y0 {
                    top = (top.0.min(y), top.1.max(y));
                }
                if x < l.x0 {
                    left = (left.0.min(x), left.1.max(x));
                }
                if y > l.y1 {
                    bottom = bottom.max(y);
                }
            }
        }
        assert!(top.1 < l.y0, "the chart title stays above the plot rect");
        assert!(left.1 < l.x0, "the y title stays left of the plot rect");
        assert!(bottom > l.y1 && bottom < h as i32, "the x title fits under the axis");
    }

    /// The y title is rotated, so its mark is taller than it is wide — the
    /// same string upright is the other way about.
    #[test]
    fn the_y_title_is_drawn_rotated() {
        let bounds = |fb: &Framebuffer, ink: Rgb| {
            let (mut x0, mut x1, mut y0, mut y1) = (usize::MAX, 0usize, usize::MAX, 0usize);
            for y in 0..fb.h {
                for x in 0..fb.w {
                    if px(fb, x, y) == Some(ink) {
                        (x0, x1) = (x0.min(x), x1.max(x));
                        (y0, y1) = (y0.min(y), y1.max(y));
                    }
                }
            }
            (x1 + 1 - x0, y1 + 1 - y0)
        };
        let ink = [255, 255, 255];
        let text = "latency";

        let mut up = Framebuffer::new(200, 200);
        draw_text(&mut up, 20, 90, text, 2, 0.0, ink);
        let (uw, uh) = bounds(&up, ink);

        let mut rot = Framebuffer::new(200, 200);
        draw_text_rot90(&mut rot, 20, 180, text, 2, 0.0, ink);
        let (rw, rh) = bounds(&rot, ink);

        assert!(uw > uh, "upright text runs across the frame ({uw}x{uh})");
        assert!(rh > rw, "rotated text runs up it ({rw}x{rh})");
        // A quarter turn swaps the two, give or take the rasterizer's edges.
        assert!((rh as i32 - uw as i32).abs() <= 2, "length is preserved: {rh} vs {uw}");
        assert!((rw as i32 - uh as i32).abs() <= 2, "height is preserved: {rw} vs {uh}");
    }

    /// An explicit range is used exactly as given — no autoscale padding —
    /// and, unlike an x window, leaves the camera composing on top of it.
    #[test]
    fn an_explicit_range_replaces_autoscale_without_padding() {
        let mut plot = demo_2d_plot();
        plot.x_range = Some((0.0, 10.0));
        plot.y_range = Some((-5.0, 5.0));
        let l = plot.layout_2d(600, 400);
        let near = |a: f64, b: f64| (a - b).abs() < 1e-6;
        assert!(near(l.map.inv_x(l.x0 as f64), 0.0) && near(l.map.inv_x(l.x1 as f64), 10.0));
        assert!(near(l.map.inv_y(l.y1 as f64), -5.0) && near(l.map.inv_y(l.y0 as f64), 5.0));

        // The camera still moves the view; an `x_window` would have replaced it.
        plot.camera.zoom = 2.0;
        let zoomed = plot.layout_2d(600, 400);
        assert!(zoomed.map.inv_x(zoomed.x0 as f64) > 0.0, "zoom composes over an explicit range",);
        assert!(plot.x_window.is_none(), "a range is not a window");
    }

    /// A log axis is linear in decades: 10 lands halfway between 1 and 100.
    #[test]
    fn a_log_axis_is_linear_in_decades() {
        let mut plot = Plot::new();
        plot.add_line2d(
            vec![1.0, 2.0, 3.0],
            vec![1.0, 100.0, 10000.0],
            [200, 120, 60],
            2.0,
            None,
            YAxis::Primary,
        );
        plot.y_log = true;
        plot.y_range = Some((1.0, 100.0));
        let l = plot.layout_2d(600, 400);
        let (a, b, c) = (l.map.sy(1.0), l.map.sy(10.0), l.map.sy(100.0));
        assert!((b - (a + c) / 2.0).abs() < 0.5, "a decade is a decade: {a} {b} {c}");
        assert!(l.ylabels.iter().any(|t| t == "10"), "decades are labelled: {:?}", l.ylabels);
    }

    /// Zero and negative samples have no log coordinate: they neither set the
    /// range nor drag the drawing back to a saturated pixel.
    #[test]
    fn a_log_axis_ignores_values_it_cannot_place() {
        let mut plot = Plot::new();
        plot.add_line2d(
            vec![1.0, 2.0, 3.0, 4.0],
            vec![0.0, 10.0, -3.0, 1000.0],
            [200, 120, 60],
            2.0,
            None,
            YAxis::Primary,
        );
        plot.y_log = true;
        let l = plot.layout_2d(600, 400);
        let (vlo, vhi) = (l.map.inv_y(l.y1 as f64), l.map.inv_y(l.y0 as f64));
        assert!(vlo > 0.0, "the range stays positive, got {vlo}");
        assert!(vlo <= 10.0 && vhi >= 1000.0, "the positive samples still fit: {vlo}..{vhi}");
        // Off-scale values land below the axis, not at a saturated pixel.
        let py = l.map.sy(0.0);
        assert!(py.is_finite() && py > l.y1 as f64, "zero sits under the axis, at {py}");
        assert!(py < (l.y1 as f64) + 4.0 * (l.y1 - l.y0) as f64, "…but not absurdly far: {py}");
    }

    /// Names and calendars own the coordinate they sit on, so log defers.
    #[test]
    fn log_defers_to_categorical_and_time_axes() {
        let mut plot = demo_2d_plot();
        plot.x_log = true;
        plot.y_log = true;
        plot.x_categories = Some(cat(&["a", "b", "c"]));
        plot.y_categories = Some(cat(&["low", "high"]));
        assert!(!plot.log_x() && !plot.log_y());
        plot.x_categories = None;
        plot.x_epoch = Some(1.0e9);
        assert!(!plot.log_x(), "a calendar is not a ladder of decades");
    }

    /// A chart title and a colorbar caption both want the top margin. They
    /// stack rather than share: the title above, the caption on the line
    /// that belongs to the ramp it names.
    #[test]
    fn a_title_stacks_above_a_colorbar_caption() {
        let mut plot = demo_2d_plot();
        plot.colorbar =
            Some(Colorbar { map: Colormap::Viridis, lo: 0.0, hi: 1.0, label: Some("kW".into()) });
        let captioned = plot.layout_2d(600, 400);
        plot.title = Some("power".into());
        let both = plot.layout_2d(600, 400);
        assert!(both.y0 > captioned.y0, "the title takes a line of its own");

        // Nothing in the top margin is drawn twice over: walk the rows above
        // the plot rect and check the two runs of ink are separated.
        let fb = plot.render(600, 400);
        let ink_rows: Vec<i32> = (0..both.y0)
            .filter(|y| {
                (0..fb.w).any(|x| {
                    px(&fb, x, *y as usize)
                        .is_some_and(|c| c == plot.chrome.ink_bright || c == plot.chrome.ink)
                })
            })
            .collect();
        assert!(!ink_rows.is_empty(), "the top margin carries the title and caption");
        let gaps = ink_rows.windows(2).filter(|w| w[1] - w[0] > 1).count();
        assert_eq!(gaps, 1, "two separated runs of text, not one overlapping block");
    }

    /// A graph frame hides its axes, and the tick labels with them — but a
    /// title is not automatic chrome, it is something the caller named. It
    /// survives, and still buys its own margin out of the frame.
    #[test]
    fn a_graph_frame_keeps_a_title_it_was_given() {
        let mut plot = Plot::new();
        plot.add_graph2d(
            vec![[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]],
            vec!["extract".into(), "transform".into(), "load".into()],
            vec![[230, 60, 120]; 3],
            vec![(0, 1), (1, 2)],
            true,
            None,
            None,
            None,
            None,
        );
        assert!(plot.chrome_hidden(), "a graph-only plot drops its axes");
        let bare = plot.layout_2d(600, 400);
        assert!(bare.xlabels.is_empty(), "…and its tick labels with them");

        plot.title = Some("nightly forecast".into());
        plot.y_title = Some("rank".into());
        let titled = plot.layout_2d(600, 400);
        assert_eq!(titled.title.as_deref(), Some("nightly forecast"));
        assert!(titled.y0 > bare.y0, "the title takes a line off the top");
        assert!(titled.x0 > bare.x0, "the rotated y title widens the left margin");
    }
}
