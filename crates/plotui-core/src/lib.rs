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
mod ticks;

pub use font::{draw_text, text_width, CHAR_H, CHAR_W};
pub use ticks::{format_tick, nice_ticks};

pub type Rgb = [u8; 3];

/// Default per-trace colors, assigned in fixed order to 2D traces added without
/// an explicit color. Stepped for dark surfaces and ordered so adjacent slots
/// stay distinguishable under color-vision deficiency.
pub const PALETTE: [Rgb; 8] = [
    [57, 135, 229],  // blue
    [25, 158, 112],  // aqua
    [201, 133, 0],   // yellow
    [0, 131, 0],     // green
    [144, 133, 233], // violet
    [230, 103, 103], // red
    [213, 81, 129],  // magenta
    [217, 89, 38],   // orange
];

// Chrome colors shared by the 2D and 3D paths: the frame/grid recede, ink is
// neutral (identity lives in the marks, never in the text).
const COLOR_BG: Rgb = [26, 30, 44];
const COLOR_FRAME: Rgb = [70, 78, 96];
const COLOR_GRID: Rgb = [45, 50, 66];
const COLOR_INK: Rgb = [150, 156, 170];
const COLOR_INK_BRIGHT: Rgb = [205, 210, 220];

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
    #[inline]
    fn view(&self, p: [f32; 3]) -> (f64, f64, f64) {
        let (x, y, z) = (p[0] as f64, p[1] as f64, p[2] as f64);
        let (sp, cp) = self.pitch.sin_cos();
        let y1 = y * cp - z * sp;
        let z1 = y * sp + z * cp;
        let (sy, cy) = self.yaw.sin_cos();
        let x2 = x * cy + z1 * sy;
        let z2 = -x * sy + z1 * cy;
        (x2, y1, z2)
    }
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
    #[inline]
    fn project(&self, p: [f32; 3]) -> [f32; 3] {
        let n = [
            (p[0] - self.center[0]) * self.inv_extent,
            (p[1] - self.center[1]) * self.inv_extent,
            (p[2] - self.center[2]) * self.inv_extent,
        ];
        let (vx, vy, vz) = self.cam.view(n);
        [
            (self.cx + vx * self.scale) as f32,
            (self.cy - vy * self.scale) as f32, // flip: +y is up on screen
            vz as f32,
        ]
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

/// A single plotted series.
pub enum Trace {
    Scatter3d {
        pts: Vec<[f32; 3]>,
        color: Rgb,
        size: f32,
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
    },
    Scatter2d {
        xs: Vec<f32>,
        ys: Vec<f32>,
        color: Rgb,
        size: f32,
        name: Option<String>,
    },
    Line2d {
        xs: Vec<f32>,
        ys: Vec<f32>,
        color: Rgb,
        width: f32,
        name: Option<String>,
    },
    Bar2d {
        xs: Vec<f32>,
        heights: Vec<f32>,
        color: Rgb,
        name: Option<String>,
    },
}

impl Trace {
    fn is_3d(&self) -> bool {
        matches!(self, Trace::Scatter3d { .. } | Trace::Graph3d { .. })
    }

    fn name(&self) -> Option<&str> {
        match self {
            Trace::Scatter2d { name, .. }
            | Trace::Line2d { name, .. }
            | Trace::Bar2d { name, .. } => name.as_deref(),
            _ => None,
        }
    }

    fn color(&self) -> Rgb {
        match self {
            Trace::Scatter3d { color, .. }
            | Trace::Scatter2d { color, .. }
            | Trace::Line2d { color, .. }
            | Trace::Bar2d { color, .. } => *color,
            Trace::Graph3d { node_colors, .. } => {
                node_colors.first().copied().unwrap_or([120, 180, 230])
            }
        }
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

/// The full plot: traces, camera, and hover/selection highlight state.
pub struct Plot {
    pub traces: Vec<Trace>,
    pub camera: Camera,
    pub show_box: bool,
    /// Element to draw with the selection treatment (click).
    pub selected: Option<Element>,
    /// Element to light up white (hover affordance: "you can click this").
    pub hovered: Option<Element>,
}

impl Default for Plot {
    fn default() -> Self {
        Self {
            traces: Vec::new(),
            camera: Camera::default(),
            show_box: true,
            selected: None,
            hovered: None,
        }
    }
}

impl Plot {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_scatter3d(&mut self, pts: Vec<[f32; 3]>, color: Rgb, size: f32) {
        self.traces.push(Trace::Scatter3d { pts, color, size });
    }

    pub fn add_graph3d(
        &mut self,
        nodes: Vec<[f32; 3]>,
        node_colors: Vec<Rgb>,
        edges: Vec<(u32, u32)>,
        size: f32,
        node_sizes: Option<Vec<f32>>,
        edge_colors: Option<Vec<Rgb>>,
    ) {
        self.traces.push(Trace::Graph3d {
            nodes,
            node_colors,
            edges,
            size,
            node_sizes,
            edge_colors,
        });
    }

    /// Project every node (flat-index order, same list as [`Self::pick`])
    /// through the exact projector `render` uses. Returns screen-space
    /// `[x_px, y_px, depth]` per node — the hook for frontends that overlay
    /// text labels or steer the camera toward a node.
    pub fn project_nodes(&self, px_w: usize, px_h: usize) -> Vec<[f32; 3]> {
        let (pr, _, _) = self.projector(px_w, px_h);
        self.node_points().iter().map(|p| pr.project(*p)).collect()
    }

    /// The next default trace color: palette slots assigned in fixed order by
    /// the number of traces already added.
    pub fn next_color(&self) -> Rgb {
        PALETTE[self.traces.len() % PALETTE.len()]
    }

    pub fn add_scatter2d(
        &mut self,
        xs: Vec<f32>,
        ys: Vec<f32>,
        color: Rgb,
        size: f32,
        name: Option<String>,
    ) {
        self.traces.push(Trace::Scatter2d { xs, ys, color, size, name });
    }

    pub fn add_line2d(
        &mut self,
        xs: Vec<f32>,
        ys: Vec<f32>,
        color: Rgb,
        width: f32,
        name: Option<String>,
    ) {
        self.traces.push(Trace::Line2d { xs, ys, color, width, name });
    }

    pub fn add_bar2d(&mut self, xs: Vec<f32>, heights: Vec<f32>, color: Rgb, name: Option<String>) {
        self.traces.push(Trace::Bar2d { xs, heights, color, name });
    }

    /// True when any trace is 3D; such plots render with the orbit camera.
    pub fn is_3d(&self) -> bool {
        self.traces.iter().any(Trace::is_3d)
    }

    /// All node points across every trace, in insertion order. The index into
    /// this list is the "flat node index" used by [`Self::pick`] and `selected`.
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

    /// Axis-aligned bounding box of all data (min, max).
    fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for p in self.node_points() {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        if !lo[0].is_finite() {
            lo = [-1.0; 3];
            hi = [1.0; 3];
        }
        (lo, hi)
    }

    fn projector(&self, px_w: usize, px_h: usize) -> (Projector, [f32; 3], [f32; 3]) {
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
        let cx = px_w as f64 * 0.5 + cam.pan_x;
        let cy = px_h as f64 * 0.5 + cam.pan_y;
        (Projector { center, inv_extent: 1.0 / extent, scale, cx, cy, cam }, lo, hi)
    }

    /// Return the flat node index nearest to screen pixel `(px, py)` within
    /// `radius` pixels, or `None`. Uses the exact projection `render` uses.
    pub fn pick(&self, px_w: usize, px_h: usize, px: f32, py: f32, radius: f32) -> Option<usize> {
        let (pr, _, _) = self.projector(px_w, px_h);
        let mut best: Option<usize> = None;
        let mut best_d2 = radius * radius;
        for (i, p) in self.node_points().iter().enumerate() {
            let s = pr.project(*p);
            let dx = s[0] - px;
            let dy = s[1] - py;
            let d2 = dx * dx + dy * dy;
            if d2 <= best_d2 {
                best = Some(i);
                best_d2 = d2;
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
        let (pr, _, _) = self.projector(px_w, px_h);
        let mut best: Option<usize> = None;
        let mut best_d2 = radius * radius;
        let mut flat = 0usize;
        for t in &self.traces {
            if let Trace::Graph3d { nodes, edges, .. } = t {
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
        if !self.traces.is_empty() && !self.is_3d() {
            self.render_2d(px_w, px_h)
        } else {
            self.render_3d(px_w, px_h)
        }
    }

    fn render_3d(&self, px_w: usize, px_h: usize) -> Framebuffer {
        let mut fb = Framebuffer::new(px_w, px_h);
        let (pr, lo, hi) = self.projector(px_w, px_h);

        // Depth range for fog.
        let (mut zmin, mut zmax) = (f32::INFINITY, f32::NEG_INFINITY);
        for p in self.node_points() {
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
        for t in &self.traces {
            match t {
                Trace::Scatter3d { pts, color, size } => {
                    for p in pts {
                        let s = pr.project(*p);
                        self.draw_node(&mut fb, s, size * ts, fog(*color, s[2]), *color, flat, ts);
                        flat += 1;
                    }
                }
                Trace::Graph3d { nodes, node_colors, edges, size, node_sizes, edge_colors } => {
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
                        self.draw_node(&mut fb, s, r * ts, fog(c, s[2]), c, flat, ts);
                        flat += 1;
                    }
                }
                // 2D traces are not projected into a 3D scene.
                _ => {}
            }
        }
        fb
    }

    /// Data bounds over 2D traces, padded 5% per side. Bars widen the x range
    /// by their drawn width and pull the y range to the zero baseline.
    fn bounds_2d(&self) -> (f64, f64, f64, f64) {
        let (mut xlo, mut xhi) = (f64::INFINITY, f64::NEG_INFINITY);
        let (mut ylo, mut yhi) = (f64::INFINITY, f64::NEG_INFINITY);
        let seen = |x: f64, y: f64, xlo: &mut f64, xhi: &mut f64, ylo: &mut f64, yhi: &mut f64| {
            if x.is_finite() && y.is_finite() {
                *xlo = xlo.min(x);
                *xhi = xhi.max(x);
                *ylo = ylo.min(y);
                *yhi = yhi.max(y);
            }
        };
        for t in &self.traces {
            match t {
                Trace::Scatter2d { xs, ys, .. } | Trace::Line2d { xs, ys, .. } => {
                    for i in 0..xs.len().min(ys.len()) {
                        seen(xs[i] as f64, ys[i] as f64, &mut xlo, &mut xhi, &mut ylo, &mut yhi);
                    }
                }
                Trace::Bar2d { xs, heights, .. } => {
                    let hw = bar_halfwidth(xs) as f64;
                    for i in 0..xs.len().min(heights.len()) {
                        let (x, h) = (xs[i] as f64, heights[i] as f64);
                        seen(x - hw, h.min(0.0), &mut xlo, &mut xhi, &mut ylo, &mut yhi);
                        seen(x + hw, h.max(0.0), &mut xlo, &mut xhi, &mut ylo, &mut yhi);
                    }
                }
                _ => {}
            }
        }
        if !xlo.is_finite() || !ylo.is_finite() {
            return (-1.0, 1.0, -1.0, 1.0);
        }
        let pad = |lo: f64, hi: f64| -> (f64, f64) {
            let span = hi - lo;
            let p = if span > 0.0 { span * 0.05 } else { 1.0 };
            (lo - p, hi + p)
        };
        let (xlo, xhi) = pad(xlo, xhi);
        let (ylo, yhi) = pad(ylo, yhi);
        (xlo, xhi, ylo, yhi)
    }

    fn render_2d(&self, px_w: usize, px_h: usize) -> Framebuffer {
        let mut fb = Framebuffer::new(px_w, px_h);
        let (w, h) = (fb.w as i32, fb.h as i32);
        let s = ((h as f32) / 240.0).round().clamp(1.0, 4.0) as i32;
        let (cw, ch) = (CHAR_W * s, CHAR_H * s);
        let tick_len = 2 * s;
        let pad = 3 * s;

        let (dxlo, dxhi, dylo, dyhi) = self.bounds_2d();

        // Two passes: the left margin depends on y tick label width, which
        // depends on the visible range, which depends on the margins.
        let (right, top) = (2 * pad, 2 * pad);
        let bottom = ch + tick_len + 2 * pad;
        let mut left = (8 * cw).min(w / 3);
        let (mut x0, mut y0, mut x1, mut y1) = (0, 0, 0, 0);
        let mut map = Map2d::default();
        let (mut xticks, mut xstep) = (Vec::new(), 1.0);
        let (mut yticks, mut ystep) = (Vec::new(), 1.0);
        for _ in 0..2 {
            x0 = left;
            y0 = top;
            x1 = (w - 1 - right).max(x0 + 4);
            y1 = (h - 1 - bottom.min(h / 3)).max(y0 + 4);
            map = Map2d::new(
                (dxlo, dxhi, dylo, dyhi),
                (x0 as f64, y0 as f64, x1 as f64, y1 as f64),
                &self.camera,
            );
            // Ticks cover what is actually visible after zoom/pan.
            let (vxlo, vxhi) = (map.inv_x(x0 as f64), map.inv_x(x1 as f64));
            let (vylo, vyhi) = (map.inv_y(y1 as f64), map.inv_y(y0 as f64));
            let tx = (((x1 - x0) / (10 * cw)) as usize).clamp(2, 12);
            let ty = (((y1 - y0) / (3 * ch)) as usize).clamp(2, 10);
            (xticks, xstep) = nice_ticks(vxlo, vxhi, tx);
            (yticks, ystep) = nice_ticks(vylo, vyhi, ty);
            let label_w =
                yticks.iter().map(|v| text_width(&format_tick(*v, ystep), s)).max().unwrap_or(cw);
            left = (label_w + tick_len + 2 * pad).min(w / 3);
        }

        // Grid first, then data (clipped), then frame/labels, then legend:
        // ties in the z-buffer resolve to the later draw, so order is layering.
        for v in &xticks {
            let px = map.sx(*v).round() as i32;
            if px > x0 && px < x1 {
                fb.rect_fill(px, y0, px, y1, 0.0, COLOR_GRID);
            }
        }
        for v in &yticks {
            let py = map.sy(*v).round() as i32;
            if py > y0 && py < y1 {
                fb.rect_fill(x0, py, x1, py, 0.0, COLOR_GRID);
            }
        }

        fb.set_clip(x0 + 1, y0 + 1, x1 - 1, y1 - 1);
        for t in &self.traces {
            match t {
                Trace::Scatter2d { xs, ys, color, size, .. } => {
                    for i in 0..xs.len().min(ys.len()) {
                        let (px, py) = (map.sx(xs[i] as f64), map.sy(ys[i] as f64));
                        if px.is_finite() && py.is_finite() {
                            fb.disc(px as f32, py as f32, 0.0, size * s as f32, *color);
                        }
                    }
                }
                Trace::Line2d { xs, ys, color, width, .. } => {
                    let n = xs.len().min(ys.len());
                    let pts: Vec<Option<(f64, f64)>> = (0..n)
                        .map(|i| {
                            let (px, py) = (map.sx(xs[i] as f64), map.sy(ys[i] as f64));
                            (px.is_finite() && py.is_finite()).then_some((px, py))
                        })
                        .collect();
                    let r = (width * s as f32 * 0.5).max(0.5);
                    for pair in pts.windows(2) {
                        if let [Some(a), Some(b)] = pair {
                            stroke(&mut fb, *a, *b, r, *color);
                        }
                    }
                }
                Trace::Bar2d { xs, heights, color, .. } => {
                    let hw = bar_halfwidth(xs) as f64;
                    let base = map.sy(0.0);
                    for i in 0..xs.len().min(heights.len()) {
                        let (x, hgt) = (xs[i] as f64, heights[i] as f64);
                        if !x.is_finite() || !hgt.is_finite() {
                            continue;
                        }
                        let bx0 = map.sx(x - hw).round() as i32;
                        let bx1 = map.sx(x + hw).round() as i32;
                        let by = map.sy(hgt).round() as i32;
                        fb.rect_fill(bx0, by, bx1, base.round() as i32, 0.0, *color);
                    }
                }
                _ => {}
            }
        }
        fb.clear_clip();

        // Frame, tick marks, tick labels.
        fb.rect_fill(x0, y0, x1, y0, 0.0, COLOR_FRAME);
        fb.rect_fill(x0, y1, x1, y1, 0.0, COLOR_FRAME);
        fb.rect_fill(x0, y0, x0, y1, 0.0, COLOR_FRAME);
        fb.rect_fill(x1, y0, x1, y1, 0.0, COLOR_FRAME);
        for v in &xticks {
            let px = map.sx(*v).round() as i32;
            if px < x0 || px > x1 {
                continue;
            }
            fb.rect_fill(px, y1 + 1, px, y1 + tick_len, 0.0, COLOR_FRAME);
            let label = format_tick(*v, xstep);
            let lw = text_width(&label, s);
            let lx = (px - lw / 2).clamp(0, (w - lw).max(0));
            draw_text(&mut fb, lx, y1 + tick_len + pad, &label, s, 0.0, COLOR_INK);
        }
        for v in &yticks {
            let py = map.sy(*v).round() as i32;
            if py < y0 || py > y1 {
                continue;
            }
            fb.rect_fill(x0 - tick_len, py, x0 - 1, py, 0.0, COLOR_FRAME);
            let label = format_tick(*v, ystep);
            let lw = text_width(&label, s);
            draw_text(
                &mut fb,
                (x0 - tick_len - pad - lw).max(0),
                py - ch / 2,
                &label,
                s,
                0.0,
                COLOR_INK,
            );
        }

        self.draw_legend(&mut fb, x0, y0, x1, s);
        fb
    }

    /// Legend for named traces, top-right inside the plot area. The swatch
    /// carries series identity; the label text stays in neutral ink.
    fn draw_legend(&self, fb: &mut Framebuffer, _x0: i32, y0: i32, x1: i32, s: i32) {
        let entries: Vec<(&str, Rgb)> =
            self.traces.iter().filter_map(|t| t.name().map(|n| (n, t.color()))).collect();
        if entries.is_empty() {
            return;
        }
        let (cw, ch) = (CHAR_W * s, CHAR_H * s);
        let pad = 3 * s;
        let swatch = ch - s; // slightly smaller than a text row
        let text_w = entries.iter().map(|(n, _)| text_width(n, s)).max().unwrap_or(cw);
        let entry_h = ch + pad;
        let box_w = pad + swatch + pad + text_w + pad;
        let box_h = entries.len() as i32 * entry_h + pad;
        let bx1 = x1 - pad;
        let bx0 = bx1 - box_w;
        let by0 = y0 + pad;
        let by1 = by0 + box_h;

        fb.rect_fill(bx0, by0, bx1, by1, 0.0, COLOR_BG);
        fb.rect_fill(bx0, by0, bx1, by0, 0.0, COLOR_FRAME);
        fb.rect_fill(bx0, by1, bx1, by1, 0.0, COLOR_FRAME);
        fb.rect_fill(bx0, by0, bx0, by1, 0.0, COLOR_FRAME);
        fb.rect_fill(bx1, by0, bx1, by1, 0.0, COLOR_FRAME);
        for (i, (name, color)) in entries.iter().enumerate() {
            let ey = by0 + pad + i as i32 * entry_h;
            fb.rect_fill(bx0 + pad, ey, bx0 + pad + swatch, ey + swatch, 0.0, *color);
            draw_text(fb, bx0 + pad + swatch + pad, ey, name, s, 0.0, COLOR_INK_BRIGHT);
        }
    }

    /// Draw one node. The selected node gets a white ring around its base
    /// color; the hovered node lights up solid white. Both are pulled to the
    /// front so the highlight is never hidden by other geometry.
    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn draw_node(
        &self,
        fb: &mut Framebuffer,
        s: [f32; 3],
        radius: f32,
        fogged: Rgb,
        base: Rgb,
        flat_index: usize,
        ts: f32,
    ) {
        let el = Element::Node(flat_index);
        let front = -1.0e9;
        if self.selected == Some(el) {
            fb.disc(s[0], s[1], front, radius + 2.2 * ts, [255, 255, 255]);
            fb.disc(s[0], s[1], front, radius + 0.6 * ts, base);
        } else if self.hovered == Some(el) {
            fb.disc(s[0], s[1], front, radius + 1.2 * ts, [255, 255, 255]);
        } else {
            fb.disc(s[0], s[1], s[2], radius, fogged);
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
        );
        // Project node 1 and click exactly there — pick must return index 1.
        let (pr, _, _) = plot.projector(300, 200);
        let s = pr.project(nodes[1]);
        let hit = plot.pick(300, 200, s[0], s[1], 4.0);
        assert_eq!(hit, Some(1));
    }

    #[test]
    fn project_nodes_matches_pick_geometry() {
        let mut plot = Plot::new();
        let nodes = vec![[0.0, 0.0, 0.0], [5.0, 5.0, 5.0], [-5.0, -5.0, -5.0]];
        plot.add_graph3d(nodes, vec![[200, 100, 100]; 3], vec![(0, 1)], 3.0, None, None);
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
            );
            plot.render(200, 200).rgba().chunks(4).filter(|px| px[3] > 0).count()
        };
        // Node 0 projects to the canvas center, so its radius change is
        // guaranteed to be visible regardless of the default camera tilt.
        assert!(lit(Some(vec![8.0, 2.0])) > lit(None));
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
        );
        let fb = plot.render(200, 100);
        let hit =
            fb.rgba().chunks(4).any(|px| px[3] > 0 && px[0] == 9 && px[1] == 250 && px[2] == 9);
        assert!(hit, "explicit edge color not found in framebuffer");
    }
}
