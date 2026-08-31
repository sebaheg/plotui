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
mod hershey;
mod layout;
mod marching;
mod ticks;

pub use font::{draw_text, draw_text_aa, text_width, CHAR_H, CHAR_W};
pub use hershey::{draw_text_hershey, hershey_text_width};
pub use layout::ForceLayout;
pub use marching::marching_cubes;
pub use ticks::{
    civil_from_days, date_ticks, days_from_civil, format_datetime, format_tick, nice_ticks,
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
/// the house feel: dragging rotates (horizontal orbits, vertical tilts),
/// shift-dragging pans. A pan-first UI would set
/// `drag_x: PanX, drag_y: PanY`; axis-swapped rotation is
/// `drag_x: Pitch, drag_y: Yaw`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InputMap {
    pub drag_x: CameraControl,
    pub drag_y: CameraControl,
    pub shift_drag_x: CameraControl,
    pub shift_drag_y: CameraControl,
}

impl Default for InputMap {
    fn default() -> Self {
        Self {
            drag_x: CameraControl::Yaw,
            drag_y: CameraControl::Pitch,
            shift_drag_x: CameraControl::PanX,
            shift_drag_y: CameraControl::PanY,
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
}

impl Map2d {
    fn new(data: (f64, f64, f64, f64), rect: (f64, f64, f64, f64), cam: &Camera) -> Self {
        let (dxlo, dxhi, dylo, dyhi) = data;
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
        }
    }

    fn sx(&self, x: f64) -> f64 {
        self.ax * x + self.bx
    }
    fn sy(&self, y: f64) -> f64 {
        self.ay * y + self.by
    }
    fn inv_x(&self, px: f64) -> f64 {
        (px - self.bx) / self.ax
    }
    fn inv_y(&self, py: f64) -> f64 {
        (py - self.by) / self.ay
    }
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
    ystep: f64,
    rticks: [Vec<f64>; RIGHT_AXES],
    rsteps: [f64; RIGHT_AXES],
    col_x: [i32; RIGHT_AXES], // label column offset from x1
    has_right: [bool; RIGHT_AXES],
    strip: Option<StripLayout>,
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
fn vsub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[inline]
fn vcross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
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
        name: Option<String>,
        axis: YAxis,
    },
    Line2d {
        xs: Vec<f32>,
        ys: Vec<f32>,
        color: Rgb,
        width: f32,
        name: Option<String>,
        axis: YAxis,
    },
    Bar2d {
        xs: Vec<f32>,
        heights: Vec<f32>,
        color: Rgb,
        name: Option<String>,
        axis: YAxis,
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

impl Trace {
    fn is_3d(&self) -> bool {
        matches!(
            self,
            Trace::Scatter3d { .. }
                | Trace::Graph3d { .. }
                | Trace::Line3d { .. }
                | Trace::Surface3d { .. }
                | Trace::Mesh3d { .. }
        )
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
            | Trace::Mesh3d { name, .. } => name.as_deref(),
        }
    }

    fn color(&self) -> Rgb {
        match self {
            Trace::Scatter3d { color, .. }
            | Trace::Line3d { color, .. }
            | Trace::Scatter2d { color, .. }
            | Trace::Line2d { color, .. }
            | Trace::Bar2d { color, .. } => *color,
            // A colormapped surface has no single color; its legend swatch is
            // a sample from the upper half of the ramp.
            Trace::Surface3d { color, colormap, .. } | Trace::Mesh3d { color, colormap, .. } => {
                colormap.map_or(*color, |m| m.sample(0.75))
            }
            Trace::Graph3d { node_colors, .. } => {
                node_colors.first().copied().unwrap_or([120, 180, 230])
            }
        }
    }

    fn axis(&self) -> YAxis {
        match self {
            Trace::Scatter2d { axis, .. }
            | Trace::Line2d { axis, .. }
            | Trace::Bar2d { axis, .. } => *axis,
            _ => YAxis::Primary,
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

/// Cached raw (unpadded) extent of one trace, in the same terms the full
/// scans use: [`CachedBounds::B2`] mirrors `bounds_2d`'s per-point rule (a
/// point counts only when both coordinates are finite; bars contribute
/// `x ± hw` and `h.min(0) / h.max(0)`), [`CachedBounds::B3`] mirrors
/// `extent_points` (scatter/graph vertices unfiltered, line/surface vertices
/// finite-only). Empty traces keep the infinite sentinels, which are the
/// identity of the min/max union.
enum CachedBounds {
    B2 { xlo: f64, xhi: f64, ylo: f64, yhi: f64, hw: Option<f64> },
    B3 { lo: [f32; 3], hi: [f32; 3] },
}

/// Per-trace bookkeeping kept parallel to [`Plot::traces`]: visibility plus
/// the incremental counters and bounds that let `extend` cost O(delta) and
/// per-frame bounds cost O(traces). Maintained eagerly by the mutating
/// methods; consumers fall back to the full scans whenever `meta` has fallen
/// out of sync with a directly-mutated `traces` field.
struct TraceMeta {
    visible: bool,
    /// Pickable nodes this trace contributes to the flat node index space.
    node_len: usize,
    /// Vertices this trace contributes to `vertex_count` (extent rule).
    vert_len: usize,
    bounds: CachedBounds,
}

/// One full scan of a trace, replicating exactly what `bounds_2d` /
/// `extent_points` would see for it.
fn compute_meta(t: &Trace) -> TraceMeta {
    match t {
        Trace::Scatter2d { xs, ys, .. } | Trace::Line2d { xs, ys, .. } => {
            let mut b = b2_empty(None);
            b2_seen_xy(&mut b, xs, ys, 0);
            TraceMeta { visible: true, node_len: 0, vert_len: 0, bounds: b }
        }
        Trace::Bar2d { xs, heights, .. } => {
            let hw = bar_halfwidth(xs) as f64;
            let mut b = b2_empty(Some(hw));
            b2_seen_bars(&mut b, xs, heights, 0, hw);
            TraceMeta { visible: true, node_len: 0, vert_len: 0, bounds: b }
        }
        Trace::Scatter3d { pts, .. } => {
            let mut b = b3_empty();
            b3_seen_all(&mut b, pts);
            TraceMeta { visible: true, node_len: pts.len(), vert_len: pts.len(), bounds: b }
        }
        Trace::Graph3d { nodes, .. } => {
            let mut b = b3_empty();
            b3_seen_all(&mut b, nodes);
            TraceMeta { visible: true, node_len: nodes.len(), vert_len: nodes.len(), bounds: b }
        }
        Trace::Line3d { pts, .. } => {
            let mut b = b3_empty();
            let n = b3_seen_finite(&mut b, pts);
            TraceMeta { visible: true, node_len: 0, vert_len: n, bounds: b }
        }
        Trace::Mesh3d { verts, .. } => {
            let mut b = b3_empty();
            let n = b3_seen_finite(&mut b, verts);
            TraceMeta { visible: true, node_len: 0, vert_len: n, bounds: b }
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
            TraceMeta { visible: true, node_len: 0, vert_len: n, bounds: b }
        }
    }
}

fn b2_empty(hw: Option<f64>) -> CachedBounds {
    CachedBounds::B2 {
        xlo: f64::INFINITY,
        xhi: f64::NEG_INFINITY,
        ylo: f64::INFINITY,
        yhi: f64::NEG_INFINITY,
        hw,
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

/// Fold bar extents from index `from` into a `B2`: `x ± hw` on x, the span
/// from the zero baseline to `h` on y — the same contributions `bounds_2d`
/// makes for bars.
fn b2_seen_bars(b: &mut CachedBounds, xs: &[f32], heights: &[f32], from: usize, hw: f64) {
    let CachedBounds::B2 { xlo, xhi, ylo, yhi, .. } = b else { return };
    for i in from..xs.len().min(heights.len()) {
        let (x, h) = (xs[i] as f64, heights[i] as f64);
        if x.is_finite() && h.is_finite() {
            *xlo = xlo.min(x - hw);
            *xhi = xhi.max(x + hw);
            *ylo = ylo.min(h.min(0.0));
            *yhi = yhi.max(h.max(0.0));
        }
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

    /// Desync-safe visibility: a trace the cache does not know about yet is
    /// treated as visible.
    fn is_visible(&self, i: usize) -> bool {
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
            Trace::Graph3d { .. } | Trace::Surface3d { .. } | Trace::Mesh3d { .. } => {
                Err(TraceError::Structural)
            }
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
                let visible = self.meta[id].visible;
                self.meta[id] = compute_meta(&self.traces[id]);
                self.meta[id].visible = visible;
                Ok(())
            }
            _ => Err(TraceError::WrongKind),
        }
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
    pub fn extend_graph(
        &mut self,
        id: TraceId,
        new_nodes: &[[f32; 3]],
        new_colors: &[Rgb],
        new_edges: &[(u32, u32)],
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
                Trace::Graph3d { edges, .. } => edges.len(),
                _ => 0,
            })
            .sum();
        let t = self.traces.get_mut(id).ok_or(TraceError::UnknownTrace)?;
        match t {
            Trace::Graph3d { nodes, node_colors, edges, .. } => {
                nodes.extend_from_slice(new_nodes);
                node_colors.extend_from_slice(new_colors);
                edges.extend_from_slice(new_edges);
                for el in [&mut self.selected, &mut self.hovered] {
                    match el {
                        Some(Element::Node(n)) if *n >= node_boundary => *n += new_nodes.len(),
                        Some(Element::Edge(e)) if *e >= edge_boundary => *e += new_edges.len(),
                        _ => {}
                    }
                }
                let m = &mut self.meta[id];
                b3_seen_all(&mut m.bounds, new_nodes);
                m.node_len += new_nodes.len();
                m.vert_len += new_nodes.len();
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

    /// Project every node (flat-index order, same list as [`Self::pick`])
    /// through the exact projector `render` uses. Returns screen-space
    /// `[x_px, y_px, depth]` per node — the hook for frontends that overlay
    /// text labels or steer the camera toward a node.
    pub fn project_nodes(&self, px_w: usize, px_h: usize) -> Vec<[f32; 3]> {
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
        self.push_trace(Trace::Scatter2d { xs, ys, color, size, name, axis })
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
        self.push_trace(Trace::Line2d { xs, ys, color, width, name, axis })
    }

    pub fn add_bar2d(
        &mut self,
        xs: Vec<f32>,
        heights: Vec<f32>,
        color: Rgb,
        name: Option<String>,
        axis: YAxis,
    ) -> TraceId {
        self.push_trace(Trace::Bar2d { xs, heights, color, name, axis })
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
                // 2D traces are not part of 3D node picking.
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
    pub fn pick(&self, px_w: usize, px_h: usize, px: f32, py: f32, radius: f32) -> Option<usize> {
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
    /// camera (drag right orbits the view right), panning follows the
    /// pointer, dragging up or left zooms in.
    pub fn apply_drag(&mut self, dx: f64, dy: f64, shift: bool, scales: DragScales) {
        let m = self.input_map;
        let (cx, cy) = if shift { (m.shift_drag_x, m.shift_drag_y) } else { (m.drag_x, m.drag_y) };
        for (control, d) in [(cx, dx), (cy, dy)] {
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
                    let ok = |t: &&[u32; 3]| {
                        t.iter().all(|&i| {
                            verts.get(i as usize).is_some_and(|v| v.iter().all(|c| c.is_finite()))
                        })
                    };
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

    /// A bar trace's drawn halfwidth: the cached value when the meta cache is
    /// live (the one bounds already used, so drawn bars and ranges can never
    /// disagree), else recomputed from the data.
    fn bar_hw(&self, ti: usize, xs: &[f32]) -> f64 {
        match self.meta.get(ti).map(|tm| &tm.bounds) {
            Some(&CachedBounds::B2 { hw: Some(hw), .. }) if self.meta_synced() => hw,
            _ => bar_halfwidth(xs) as f64,
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
        if let Some((wlo, whi)) = xr {
            let mut seen = |lo: f64, hi: f64, y: f64, slot: usize| {
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
                    Trace::Scatter2d { xs, ys, .. } | Trace::Line2d { xs, ys, .. } => {
                        for i in 0..xs.len().min(ys.len()) {
                            let x = xs[i] as f64;
                            seen(x, x, ys[i] as f64, slot);
                        }
                    }
                    Trace::Bar2d { xs, heights, .. } => {
                        let hw = self.bar_hw(ti, xs);
                        for i in 0..xs.len().min(heights.len()) {
                            let (x, h) = (xs[i] as f64, heights[i] as f64);
                            seen(x - hw, x + hw, h.min(0.0), slot);
                            seen(x - hw, x + hw, h.max(0.0), slot);
                        }
                    }
                    _ => {}
                }
            }
            let pad = |lo: f64, hi: f64| -> (f64, f64) {
                if !lo.is_finite() {
                    return (-1.0, 1.0);
                }
                let span = hi - lo;
                let p = if span > 0.0 { span * 0.05 } else { 1.0 };
                (lo - p, hi + p)
            };
            let (ylo, yhi) = pad(ys[0].0, ys[0].1);
            return (wlo, whi, ylo, yhi, [pad(ys[1].0, ys[1].1), pad(ys[2].0, ys[2].1)]);
        }
        if self.meta_synced() {
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
                    Trace::Scatter2d { xs, ys, .. } | Trace::Line2d { xs, ys, .. } => {
                        for i in 0..xs.len().min(ys.len()) {
                            seen(xs[i] as f64, ys[i] as f64, slot);
                        }
                    }
                    Trace::Bar2d { xs, heights, .. } => {
                        let hw = bar_halfwidth(xs) as f64;
                        for i in 0..xs.len().min(heights.len()) {
                            let (x, h) = (xs[i] as f64, heights[i] as f64);
                            seen(x - hw, h.min(0.0), slot);
                            seen(x + hw, h.max(0.0), slot);
                        }
                    }
                    _ => {}
                }
            }
        }
        let pad = |lo: f64, hi: f64| -> (f64, f64) {
            if !lo.is_finite() {
                return (-1.0, 1.0);
            }
            let span = hi - lo;
            let p = if span > 0.0 { span * 0.05 } else { 1.0 };
            (lo - p, hi + p)
        };
        let (xlo, xhi) = pad(xlo, xhi);
        let (ylo, yhi) = pad(ys[0].0, ys[0].1);
        (xlo, xhi, ylo, yhi, [pad(ys[1].0, ys[1].1), pad(ys[2].0, ys[2].1)])
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
        let top = 2 * pad;
        let bottom = ch + tick_len + 2 * pad;
        // The strip reserve is a pure function of `s` and `h`, decided before
        // the margin fixed-point below — a reserve that changed inside the
        // loop could keep it from settling in two passes.
        let strip_on = self.range_slider && h >= STRIP_MIN_H * s;
        let strip_h = STRIP_H_S * s;
        let strip_reserve = if strip_on { strip_h + pad } else { 0 };
        let mut left = (8 * cw).min(w / 3);
        let mut right = 2 * pad;
        let (mut x0, mut y0, mut x1, mut y1) = (0, 0, 0, 0);
        let mut map = Map2d::default();
        let (mut xticks, mut xlabels) = (Vec::new(), Vec::<String>::new());
        let (mut yticks, mut ystep) = (Vec::new(), 1.0);
        let mut maps_r = [Map2d::default(); RIGHT_AXES];
        let mut rticks: [Vec<f64>; RIGHT_AXES] = [Vec::new(), Vec::new()];
        let mut rsteps = [1.0; RIGHT_AXES];
        let mut col_x = [0; RIGHT_AXES]; // label column offset from x1
        for _ in 0..2 {
            x0 = left;
            y0 = top;
            x1 = (w - 1 - right).max(x0 + 4);
            y1 = (h - 1 - (bottom.min(h / 3) + strip_reserve)).max(y0 + 4);
            let rect = (x0 as f64, y0 as f64, x1 as f64, y1 as f64);
            map = Map2d::new((dxlo, dxhi, dylo, dyhi), rect, cam);
            // Ticks cover what is actually visible after zoom/pan.
            let (vxlo, vxhi) = (map.inv_x(x0 as f64), map.inv_x(x1 as f64));
            let (vylo, vyhi) = (map.inv_y(y1 as f64), map.inv_y(y0 as f64));
            let tx = (((x1 - x0) / (10 * cw)) as usize).clamp(2, 12);
            let ty = (((y1 - y0) / (3 * ch)) as usize).clamp(2, 10);
            // A time axis ticks on calendar boundaries in absolute epoch
            // seconds; positions come back to offset space for the map.
            if let Some(base) = self.x_epoch {
                let (abs, labels) = date_ticks(base + vxlo, base + vxhi, tx);
                xticks = abs.into_iter().map(|t| t - base).collect();
                xlabels = labels;
            } else {
                let (t, step) = nice_ticks(vxlo, vxhi, tx);
                xlabels = t.iter().map(|v| format_tick(*v, step)).collect();
                xticks = t;
            }
            (yticks, ystep) = nice_ticks(vylo, vyhi, ty);
            let label_w =
                yticks.iter().map(|v| text_width(&format_tick(*v, ystep), s)).max().unwrap_or(cw);
            left = (label_w + tick_len + 2 * pad).min(w / 3);
            if has_right.iter().any(|b| *b) {
                let mut off = tick_len + pad; // x1 → first label column
                for k in 0..RIGHT_AXES {
                    if !has_right[k] {
                        continue;
                    }
                    let (rlo, rhi) = dright[k];
                    maps_r[k] = Map2d::new((dxlo, dxhi, rlo, rhi), rect, cam);
                    let (vlo, vhi) = (maps_r[k].inv_y(y1 as f64), maps_r[k].inv_y(y0 as f64));
                    (rticks[k], rsteps[k]) = nice_ticks(vlo, vhi, ty);
                    let wk = rticks[k]
                        .iter()
                        .map(|v| text_width(&format_tick(*v, rsteps[k]), s))
                        .max()
                        .unwrap_or(cw);
                    col_x[k] = off;
                    off += wk + 2 * pad;
                }
                right = (off - pad).min(w / 3);
            }
        }
        // The strip sits at the very bottom, below the x tick labels, and
        // shows the full extent whatever the window — its maps never depend
        // on `x_window`, so drags read a stable pixel↔data scale from it.
        let strip = if strip_on {
            let (flo, fhi, fylo, fyhi, fright) = self.bounds_2d_in(None);
            let sy1 = h - 1 - pad;
            let sy0 = sy1 - strip_h;
            let rect = (x0 as f64, sy0 as f64, x1 as f64, sy1 as f64);
            let smap = Map2d::new((flo, fhi, fylo, fyhi), rect, &flat);
            let mut smaps_r = [Map2d::default(); RIGHT_AXES];
            for (k, sm) in smaps_r.iter_mut().enumerate() {
                if has_right[k] {
                    *sm = Map2d::new((flo, fhi, fright[k].0, fright[k].1), rect, &flat);
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
            ystep,
            rticks,
            rsteps,
            col_x,
            has_right,
            strip,
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
                Trace::Scatter2d { xs, ys, color, size, .. } => {
                    let r = size * s as f32;
                    let (bx0, by0, bx1, by1) = pix_box(r as f64 + 1.0);
                    for i in 0..xs.len().min(ys.len()) {
                        let (px, py) = (m.sx(xs[i] as f64), m.sy(ys[i] as f64));
                        if px.is_finite()
                            && py.is_finite()
                            && !(win && (px < bx0 || px > bx1 || py < by0 || py > by1))
                        {
                            fb.disc(px as f32, py as f32, 0.0, r, *color);
                        }
                    }
                }
                Trace::Line2d { xs, ys, color, width, .. } => {
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
                            if win {
                                if let Some((ca, cb)) = clip_segment(*a, *b, clip_box) {
                                    stroke(&mut fb, ca, cb, r, *color);
                                }
                            } else {
                                stroke(&mut fb, *a, *b, r, *color);
                            }
                        }
                    }
                }
                Trace::Bar2d { xs, heights, color, .. } => {
                    let hw = self.bar_hw(ti, xs);
                    let base = m.sy(0.0);
                    let (cx0, cy0, cx1, cy1) = pix_box(1.0);
                    for i in 0..xs.len().min(heights.len()) {
                        let (x, hgt) = (xs[i] as f64, heights[i] as f64);
                        if !x.is_finite() || !hgt.is_finite() {
                            continue;
                        }
                        let (mut fx0, mut fx1) = (m.sx(x - hw), m.sx(x + hw));
                        let (mut fy0, mut fy1) = (m.sy(hgt), base);
                        if win {
                            if fx1 < cx0 || fx0 > cx1 {
                                continue;
                            }
                            (fx0, fx1) = (fx0.max(cx0), fx1.min(cx1));
                            (fy0, fy1) = (fy0.clamp(cy0, cy1), fy1.clamp(cy0, cy1));
                        }
                        let bx0 = fx0.round() as i32;
                        let bx1 = fx1.round() as i32;
                        let by = fy0.round() as i32;
                        fb.rect_fill(bx0, by, bx1, fy1.round() as i32, 0.0, *color);
                    }
                }
                _ => {}
            }
        }
        fb.clear_clip();

        // Axes and tick labels. An open L frame — the y axis and the x axis,
        // no box, no tick marks — so the chart reads like a page figure: the
        // labels alone carry the positions. Right axes share one rule at x1
        // (a second rule at the outer column would close the figure into a
        // box and anchor nothing); each column's tint says who owns it.
        fb.rect_fill(x0, y1, x1, y1, 0.0, self.chrome.frame);
        fb.rect_fill(x0, y0, x0, y1, 0.0, self.chrome.frame);
        if l.has_right.iter().any(|b| *b) {
            fb.rect_fill(x1, y0, x1, y1, 0.0, self.chrome.frame);
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
        for v in &l.yticks {
            let py = map.sy(*v).round() as i32;
            if py < y0 || py > y1 {
                continue;
            }
            let label = format_tick(*v, l.ystep);
            let lw = text_width(&label, s);
            draw_text(
                &mut fb,
                (x0 - tick_len - pad - lw).max(0),
                py - ch / 2,
                &label,
                s,
                0.0,
                self.chrome.ink,
            );
        }
        // Right-axis tick labels, one column per axis, tinted to the first
        // trace on that axis — two unlabeled number columns are otherwise
        // unattributable.
        for (k, mr) in maps_r.iter().enumerate() {
            if !l.has_right[k] {
                continue;
            }
            let ink = self.right_axis_color(k);
            for v in &l.rticks[k] {
                let py = mr.sy(*v).round() as i32;
                if py < y0 || py > y1 {
                    continue;
                }
                draw_text(
                    &mut fb,
                    x1 + l.col_x[k],
                    py - ch / 2,
                    &format_tick(*v, l.rsteps[k]),
                    s,
                    0.0,
                    ink,
                );
            }
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
                    Trace::Scatter2d { xs, ys, color, .. } => {
                        for i in 0..xs.len().min(ys.len()) {
                            let (px, py) = (m.sx(xs[i] as f64), m.sy(ys[i] as f64));
                            if px.is_finite() && py.is_finite() {
                                fb.disc(px as f32, py as f32, 0.0, s as f32, tint(color));
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
                    Trace::Bar2d { xs, heights, color, .. } => {
                        let hw = self.bar_hw(ti, xs);
                        let base = m.sy(0.0).round() as i32;
                        for i in 0..xs.len().min(heights.len()) {
                            let (x, hgt) = (xs[i] as f64, heights[i] as f64);
                            if !x.is_finite() || !hgt.is_finite() {
                                continue;
                            }
                            let bx0 = m.sx(x - hw).round() as i32;
                            let bx1 = m.sx(x + hw).round() as i32;
                            let by = m.sy(hgt).round() as i32;
                            fb.rect_fill(bx0, by, bx1, base, 0.0, tint(color));
                        }
                    }
                    _ => {}
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

    /// The 2D hover crosshair: a vertical guide at the sample x nearest the
    /// hovered pixel, a marker on every series sampled at that x, and a
    /// readout box naming each value. Drawn after everything else so no
    /// chrome covers it. Series match by exact sample x, so series on a
    /// shared grid all get a row while series on their own grids only show
    /// where they truly have a sample.
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
            let xs = match t {
                Trace::Scatter2d { xs, .. }
                | Trace::Line2d { xs, .. }
                | Trace::Bar2d { xs, .. } => xs,
                _ => continue,
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
            let (xs, vals) = match t {
                Trace::Scatter2d { xs, ys, .. } | Trace::Line2d { xs, ys, .. } => (xs, ys),
                Trace::Bar2d { xs, heights, .. } => (xs, heights),
                _ => continue,
            };
            let Some(i) = xs.iter().position(|&x| x == snap) else { continue };
            let Some(&v) = vals.get(i) else { continue };
            if !v.is_finite() {
                continue;
            }
            let py = m.sy(v as f64).round() as i32;
            if py >= y0 && py <= y1 {
                fb.disc(px as f32, py as f32, 0.0, 2.6 * s as f32, [255, 255, 255]);
                fb.disc(px as f32, py as f32, 0.0, 1.7 * s as f32, t.color());
            }
            let name = t.name().map_or_else(|| format!("series {}", ti + 1), str::to_owned);
            rows.push((format!("{name}  {}", format_value(v as f64)), t.color()));
        }
        if rows.is_empty() {
            return;
        }

        let (cw, ch) = (CHAR_W * s, CHAR_H * s);
        let pad = 3 * s;
        let swatch = ch - s;
        let cap_height = ch as f32 - s as f32 * 0.5;
        let measure = |n: &str| -> i32 {
            if s > 1 {
                hershey_text_width(n, cap_height)
            } else {
                text_width(n, s)
            }
        };
        let header = match self.x_epoch {
            Some(base) => format!("x  {}", format_datetime(base + snap as f64)),
            None => format!("x  {}", format_value(snap as f64)),
        };
        let text_w =
            rows.iter().map(|(l, _)| measure(l)).chain([measure(&header)]).max().unwrap_or(cw);
        let entry_h = ch + pad;
        let box_w = pad + swatch + pad + text_w + pad;
        let box_h = (rows.len() as i32 + 1) * entry_h + pad;
        // Beside the guide, flipped to its left when the right side would
        // leave the frame, and clamped inside the plot area vertically.
        let mut bx0 = px + 2 * pad;
        if bx0 + box_w > x1 {
            bx0 = (px - 2 * pad - box_w).max(x0 + 1);
        }
        let bx1 = bx0 + box_w;
        let by0 = (y0 + pad).min((y1 - box_h).max(y0));
        let by1 = by0 + box_h;

        fb.rect_fill(bx0, by0, bx1, by1, 0.0, self.chrome.bg);
        fb.rect_fill(bx0, by0, bx1, by0, 0.0, self.chrome.frame);
        fb.rect_fill(bx0, by1, bx1, by1, 0.0, self.chrome.frame);
        fb.rect_fill(bx0, by0, bx0, by1, 0.0, self.chrome.frame);
        fb.rect_fill(bx1, by0, bx1, by1, 0.0, self.chrome.frame);
        let mut draw_row = |row_i: i32, label: &str, ink: Rgb| {
            let ey = by0 + pad + row_i * entry_h;
            if s > 1 {
                draw_text_hershey(
                    fb,
                    bx0 + pad + swatch + pad,
                    ey,
                    label,
                    cap_height,
                    0.0,
                    ink,
                    self.chrome.bg,
                );
            } else {
                draw_text(fb, bx0 + pad + swatch + pad, ey, label, s, 0.0, ink);
            }
        };
        draw_row(0, &header, self.chrome.ink);
        for (i, (label, _)) in rows.iter().enumerate() {
            draw_row(i as i32 + 1, label, self.chrome.ink_bright);
        }
        for (i, (_, color)) in rows.iter().enumerate() {
            let ey = by0 + pad + (i as i32 + 1) * entry_h;
            fb.rect_fill(bx0 + pad, ey, bx0 + pad + swatch, ey + swatch, 0.0, *color);
        }
    }

    /// Legend for named traces, top-right inside the plot area. The swatch
    /// carries series identity; the label text stays in neutral ink. `z` is
    /// the depth to draw at: 0.0 in the 2D path, pulled far forward in 3D so
    /// no geometry can poke through the legend box. `three_d` says which
    /// render path is asking; only traces that path actually draws are
    /// listed, so a named 2D trace mixed into a 3D plot never appears as a
    /// legend entry for geometry that is not on screen.
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
        let entries: Vec<(&str, Rgb)> = self
            .traces
            .iter()
            .enumerate()
            .filter(|(i, t)| self.is_visible(*i) && t.is_3d() == three_d)
            .filter_map(|(_, t)| t.name().map(|n| (n, t.color())))
            .collect();
        if entries.is_empty() {
            return;
        }
        let (cw, ch) = (CHAR_W * s, CHAR_H * s);
        let pad = 3 * s;
        let swatch = ch - s; // slightly smaller than a text row
                             // At scale 1 the 5×7 bitmap font is the crispest thing there is; any
                             // larger, the Hershey stroke font renders smooth instead of blocky.
        let cap_height = ch as f32 - s as f32 * 0.5;
        let measure = |n: &str| -> i32 {
            if s > 1 {
                hershey_text_width(n, cap_height)
            } else {
                text_width(n, s)
            }
        };
        let text_w = entries.iter().map(|(n, _)| measure(n)).max().unwrap_or(cw);
        let entry_h = ch + pad;
        let box_w = pad + swatch + pad + text_w + pad;
        let box_h = entries.len() as i32 * entry_h + pad;
        let bx1 = x1 - pad;
        let bx0 = bx1 - box_w;
        let by0 = y0 + pad;
        let by1 = by0 + box_h;

        fb.rect_fill(bx0, by0, bx1, by1, z, self.chrome.bg);
        fb.rect_fill(bx0, by0, bx1, by0, z, self.chrome.frame);
        fb.rect_fill(bx0, by1, bx1, by1, z, self.chrome.frame);
        fb.rect_fill(bx0, by0, bx0, by1, z, self.chrome.frame);
        fb.rect_fill(bx1, by0, bx1, by1, z, self.chrome.frame);
        for (i, (name, color)) in entries.iter().enumerate() {
            let ey = by0 + pad + i as i32 * entry_h;
            let (sx0, sx1) = (bx0 + pad, bx0 + pad + swatch);
            fb.rect_fill(sx0, ey, sx1, ey + swatch, z, *color);
            if s > 1 {
                // Soften the swatch: knock the corner pixels back to the
                // legend background so it reads as a rounded chip.
                for (cx, cy) in [(sx0, ey), (sx1, ey), (sx0, ey + swatch), (sx1, ey + swatch)] {
                    fb.rect_fill(cx, cy, cx, cy, z, self.chrome.bg);
                }
            }
            // Rendered against the legend's own background — the one place
            // text sits on a known opaque fill, so it can be antialiased.
            let tx = bx0 + pad + swatch + pad;
            if s > 1 {
                draw_text_hershey(
                    fb,
                    tx,
                    ey,
                    name,
                    cap_height,
                    z,
                    self.chrome.ink_bright,
                    self.chrome.bg,
                );
            } else {
                draw_text(fb, tx, ey, name, s, z, self.chrome.ink_bright);
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
}
