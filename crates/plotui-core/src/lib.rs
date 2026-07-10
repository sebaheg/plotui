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

pub type Rgb = [u8; 3];

/// An RGBA framebuffer with a z-buffer for correct point/line occlusion.
pub struct Framebuffer {
    pub w: usize,
    pub h: usize,
    color: Vec<Rgb>,
    depth: Vec<f32>,
    drawn: Vec<bool>,
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
        }
    }

    #[inline]
    fn put(&mut self, x: i32, y: i32, z: f32, c: Rgb) {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return;
        }
        let i = y as usize * self.w + x as usize;
        if z <= self.depth[i] {
            self.depth[i] = z;
            self.color[i] = c;
            self.drawn[i] = true;
        }
    }

    /// Filled disc — the mark used for scatter points.
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

    /// Depth-interpolated line — used for axis boxes and line traces.
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

/// A single plotted series.
pub enum Trace {
    Scatter3d { pts: Vec<[f32; 3]>, color: Rgb, size: f32 },
}

/// The full plot: traces plus the camera. Frontends hold one of these.
pub struct Plot {
    pub traces: Vec<Trace>,
    pub camera: Camera,
    pub show_box: bool,
}

impl Default for Plot {
    fn default() -> Self {
        Self { traces: Vec::new(), camera: Camera::default(), show_box: true }
    }
}

impl Plot {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_scatter3d(&mut self, pts: Vec<[f32; 3]>, color: Rgb, size: f32) {
        self.traces.push(Trace::Scatter3d { pts, color, size });
    }

    /// Axis-aligned bounding box of all data (min, max).
    fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for t in &self.traces {
            let Trace::Scatter3d { pts, .. } = t;
            for p in pts {
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

    /// Render one frame into an RGBA framebuffer of the given pixel size.
    pub fn render(&self, px_w: usize, px_h: usize) -> Framebuffer {
        let mut fb = Framebuffer::new(px_w, px_h);
        let (lo, hi) = self.bounds();
        let center = [
            (lo[0] + hi[0]) * 0.5,
            (lo[1] + hi[1]) * 0.5,
            (lo[2] + hi[2]) * 0.5,
        ];
        let mut extent = 0.0f32;
        for k in 0..3 {
            extent = extent.max((hi[k] - lo[k]) * 0.5);
        }
        if extent <= 0.0 {
            extent = 1.0;
        }

        let cam = &self.camera;
        let scale = 0.42 * px_w.min(px_h) as f64 * cam.zoom;
        let cx = px_w as f64 * 0.5 + cam.pan_x;
        let cy = px_h as f64 * 0.5 + cam.pan_y;

        // Normalize a data point into [-1, 1]^3, then project to screen.
        let project = |p: [f32; 3]| -> [f32; 3] {
            let n = [
                (p[0] - center[0]) / extent,
                (p[1] - center[1]) / extent,
                (p[2] - center[2]) / extent,
            ];
            let (vx, vy, vz) = cam.view(n);
            [
                (cx + vx * scale) as f32,
                (cy - vy * scale) as f32, // flip: +y is up on screen
                vz as f32,
            ]
        };

        // Depth range for fog.
        let (mut zmin, mut zmax) = (f32::INFINITY, f32::NEG_INFINITY);
        for t in &self.traces {
            let Trace::Scatter3d { pts, .. } = t;
            for p in pts {
                let z = project(*p)[2];
                zmin = zmin.min(z);
                zmax = zmax.max(z);
            }
        }
        let zspan = (zmax - zmin).max(1e-3);
        let fog = |c: Rgb, z: f32| -> Rgb {
            // Farther points fade toward the cool background tint.
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
                    project([
                        if i & 1 == 0 { lo[0] } else { hi[0] },
                        if i & 2 == 0 { lo[1] } else { hi[1] },
                        if i & 4 == 0 { lo[2] } else { hi[2] },
                    ])
                })
                .collect();
            let edges = [
                (0, 1), (2, 3), (4, 5), (6, 7),
                (0, 2), (1, 3), (4, 6), (5, 7),
                (0, 4), (1, 5), (2, 6), (3, 7),
            ];
            for (a, b) in edges {
                fb.line(corners[a], corners[b], [70, 78, 96]);
            }
        }

        // Points.
        let ts = (px_w as f32 / 500.0).clamp(1.0, 3.0);
        for t in &self.traces {
            let Trace::Scatter3d { pts, color, size } = t;
            for p in pts {
                let s = project(*p);
                fb.disc(s[0], s[1], s[2], size * ts, fog(*color, s[2]));
            }
        }
        fb
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
        // At least some pixels drawn (alpha > 0).
        assert!(rgba.chunks(4).any(|px| px[3] > 0));
    }
}
