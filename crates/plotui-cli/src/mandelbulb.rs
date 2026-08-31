//! `plotui example mandelbulb` — the 3D cousin of the Mandelbrot set, as a
//! marching-cubes mesh revealed slice by slice.
//!
//! The power-8 Mandelbulb's distance estimator is sampled on a uniform grid,
//! polygonised once at startup, and the triangles are sorted by height into
//! bands. Each band is its own hidden `Mesh3d` trace; the feed hook reveals
//! one every REVEAL_MS, bottom-up like a CT scan, while the camera orbits.
//! Constants are mirrored verbatim in site/examples.js, and the website
//! drives the same marching-cubes tables through wasm, so it shows this
//! bulb — up to the odd triangle where the browser's transcendentals round
//! a sample to the other side of the iso value.

use plotui_core::{marching_cubes, Colormap, Plot, TraceId};
use plotui_ratatui::PlotState;

use crate::examples::{self, Output};
use crate::interactive::{self, Hooks};
use crate::{record, ExampleArgs};

/// Field samples per axis — the one knob worth turning. The mesh is O(RES³)
/// to build and its triangle count grows with it, so the cost is all in the
/// draw: on a 2.8 GHz Xeon, 64³ renders a 1280×720 frame in ~32 ms, 96³ in
/// ~54 ms and 128³ in ~81 ms (`mandelbulb_frame_cost`, below). 64 is the
/// largest that holds 30 fps there; raise it on a faster machine.
const RES: usize = 64;
/// The sampled box; the bulb itself fits comfortably inside |p| ≤ 1.2.
const HALF: f32 = 1.2;
const CELL: f32 = 2.0 * HALF / (RES - 1) as f32;
/// The surface is the zero set of the distance estimator.
const ISO: f32 = 0.0;

/// z ← z⁸ + c, escaping past |z| > BAILOUT within ITERS steps.
const POWER: f32 = 8.0;
const ITERS: usize = 14;
const BAILOUT: f32 = 2.0;

/// Triangles are dealt into this many height bands, revealed one per
/// REVEAL_MS — about nine seconds of reveal.
const BANDS: usize = 60;
const REVEAL_MS: f64 = 150.0;

/// The camera box, a little wider than the sampled one so the frame never
/// breathes as bands appear.
const VIEW_LO: [f32; 3] = [-1.3, -1.3, -1.3];
const VIEW_HI: [f32; 3] = [1.3, 1.3, 1.3];

/// Distance estimate to the power-8 Mandelbulb at `c`: positive outside,
/// negative inside, crossing zero at the surface.
///
/// The iteration runs in spherical coordinates (`r⁸`, `8θ`, `8φ`) alongside
/// a running derivative `dr`, which turns the escape radius into a distance:
/// `0.5 · ln(r) · r / dr`. A point that never escapes is inside, whatever
/// its last radius says, so its estimate is forced negative.
fn distance(c: [f32; 3]) -> f32 {
    let mut z = c;
    let mut dr = 1.0f32;
    let mut r = 0.0f32;
    let mut escaped = false;
    for _ in 0..ITERS {
        r = (z[0] * z[0] + z[1] * z[1] + z[2] * z[2]).sqrt();
        if r > BAILOUT {
            escaped = true;
            break;
        }
        let theta = (z[2] / r).acos() * POWER;
        let phi = z[1].atan2(z[0]) * POWER;
        dr = r.powf(POWER - 1.0) * POWER * dr + 1.0;
        let zr = r.powf(POWER);
        z = [
            zr * theta.sin() * phi.cos() + c[0],
            zr * theta.sin() * phi.sin() + c[1],
            zr * theta.cos() + c[2],
        ];
    }
    // r = 0 (the origin, and anything that lands on it) has no logarithm;
    // it is as far inside as the field goes.
    let d = 0.5 * r.max(1e-9).ln() * r / dr;
    if escaped {
        d.max(1e-6)
    } else {
        d.min(-1e-6)
    }
}

/// The field over the RES³ grid, in the layout `marching_cubes` expects.
fn field() -> Vec<f32> {
    let coord = |i: usize| -HALF + i as f32 * CELL;
    let mut v = Vec::with_capacity(RES * RES * RES);
    for k in 0..RES {
        for j in 0..RES {
            for i in 0..RES {
                v.push(distance([coord(i), coord(j), coord(k)]));
            }
        }
    }
    v
}

/// One reveal band: its own vertices and the triangles indexing them.
type Band = (Vec<[f32; 3]>, Vec<[u32; 3]>);

/// The polygonised bulb, dealt into BANDS height bands by triangle z
/// centroid — the reveal order. Each band is a standalone mesh carrying only
/// the vertices its own triangles use, so the 60 traces together cost about
/// what the whole bulb would as one.
///
/// Every band's vertex list also carries the bulb's lowest and highest
/// vertex (as local indices 0 and 1, referenced by triangles or not): a
/// mesh's colormap spans its own z range, and without them the Plasma ramp
/// would restart inside every slice instead of running once up the bulb.
fn banded_mesh() -> Vec<Band> {
    let (verts, tris) = marching_cubes(&field(), RES, RES, RES, [-HALF; 3], CELL, ISO);
    let zkey = |&i: &usize| verts[i][2];
    let lowest = (0..verts.len()).min_by(|a, b| zkey(a).total_cmp(&zkey(b)));
    let highest = (0..verts.len()).max_by(|a, b| zkey(a).total_cmp(&zkey(b)));
    let (Some(lowest), Some(highest)) = (lowest, highest) else {
        return vec![(Vec::new(), Vec::new()); BANDS];
    };

    let mut banded: Vec<Vec<[u32; 3]>> = vec![Vec::new(); BANDS];
    for t in tris {
        let zc =
            (verts[t[0] as usize][2] + verts[t[1] as usize][2] + verts[t[2] as usize][2]) / 3.0;
        // Bottom-up, over the sampled box rather than the mesh's own extent,
        // so the band a triangle lands in is a property of the field alone.
        let f = (zc + HALF) / (2.0 * HALF);
        banded[((f * BANDS as f32) as usize).min(BANDS - 1)].push(t);
    }

    // Re-index each band against its own vertex list. `local` is reused
    // across bands and cleared through `touched`, so the split stays O(n).
    let mut local = vec![u32::MAX; verts.len()];
    let mut touched: Vec<usize> = Vec::new();
    banded
        .into_iter()
        .map(|band| {
            let mut bverts = vec![verts[lowest], verts[highest]];
            local[lowest] = 0;
            local[highest] = 1;
            touched.extend([lowest, highest]);
            let btris: Vec<[u32; 3]> = band
                .iter()
                .map(|t| {
                    t.map(|g| {
                        let g = g as usize;
                        if local[g] == u32::MAX {
                            local[g] = bverts.len() as u32;
                            bverts.push(verts[g]);
                            touched.push(g);
                        }
                        local[g]
                    })
                })
                .collect();
            for g in touched.drain(..) {
                local[g] = u32::MAX;
            }
            (bverts, btris)
        })
        .collect()
}

struct Scene {
    handles: Vec<TraceId>,
    /// Bands revealed so far; band 0 is visible from the first frame.
    shown: usize,
    acc: f64,
}

impl Scene {
    fn build() -> (Plot, Scene) {
        let mut plot = Plot::new();
        // Pin the frame to the sampled box so the camera never "breathes"
        // as bands arrive.
        plot.bounds_override = Some((VIEW_LO, VIEW_HI));
        // Bands are unnamed — one legend entry per slice would be noise.
        let handles: Vec<TraceId> = banded_mesh()
            .into_iter()
            .map(|(verts, tris)| {
                plot.add_mesh3d(verts, tris, [230, 120, 60], Some(Colormap::Plasma), None)
            })
            .collect();
        for &h in handles.iter().skip(1) {
            plot.set_visible(h, false).expect("mesh handle");
        }
        (plot, Scene { handles, shown: 1, acc: 0.0 })
    }

    fn done(&self) -> bool {
        self.shown >= self.handles.len()
    }

    /// Reveal every band the timer crossed in `dt_ms`; true if any appeared.
    fn feed(&mut self, plot: &mut Plot, dt_ms: f64) -> bool {
        self.acc += dt_ms;
        let mut revealed = false;
        while self.acc >= REVEAL_MS && !self.done() {
            self.acc -= REVEAL_MS;
            plot.set_visible(self.handles[self.shown], true).expect("mesh handle");
            self.shown += 1;
            revealed = true;
        }
        revealed
    }

    /// Reveal everything at once — what a still frame should show.
    fn reveal_all(&mut self, plot: &mut Plot) {
        while !self.done() {
            plot.set_visible(self.handles[self.shown], true).expect("mesh handle");
            self.shown += 1;
        }
    }
}

pub fn run(args: &ExampleArgs, out: Output) -> std::io::Result<()> {
    let (mut plot, mut scene) = Scene::build();

    if out.is_still() {
        // One frame: the whole bulb.
        scene.reveal_all(&mut plot);
        return examples::emit(&plot, args, &out);
    }

    let feed = Box::new(move |state: &mut PlotState, dt_ms: f64| {
        if !scene.done() && scene.feed(state.plot_mut(), dt_ms) {
            state.invalidate();
        }
    });
    let hooks = Hooks { auto_rotate: true, feed: Some(feed), ..Default::default() };
    match out {
        Output::Record(opts) => record::record(plot, hooks, &opts),
        Output::Interactive(mode) => {
            interactive::run_with(plot, mode, args.width, args.height, hooks)
        }
        Output::Static(_) => unreachable!("still outputs handled above"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frame budget behind RES, measured rather than assumed. Ignored by
    /// default — it is a benchmark, not an assertion:
    /// `cargo test --release -p plotui mandelbulb_frame_cost -- --ignored
    /// --nocapture`.
    #[test]
    #[ignore = "benchmark: prints timings, asserts nothing"]
    fn mandelbulb_frame_cost() {
        let (mut plot, mut scene) = Scene::build();
        scene.reveal_all(&mut plot);
        let tris: usize = plot
            .traces
            .iter()
            .map(|t| match t {
                plotui_core::Trace::Mesh3d { tris, .. } => tris.len(),
                _ => 0,
            })
            .sum();
        plot.render(1280, 720); // warm the framebuffer allocation
        let n = 30;
        let start = std::time::Instant::now();
        for _ in 0..n {
            plot.camera.rotate(0.01, 0.0);
            plot.render(1280, 720);
        }
        let ms = start.elapsed().as_secs_f64() * 1000.0 / n as f64;
        println!("RES={RES}: {tris} triangles, {ms:.1} ms/frame at 1280x720");
    }

    /// The field's sign is the inside/outside test the iso-surface depends
    /// on: the origin is deep in the bulb, and a point well outside the
    /// sampled box escapes immediately.
    #[test]
    fn the_distance_estimator_has_the_right_signs() {
        assert!(distance([0.0, 0.0, 0.0]) < 0.0, "the origin is inside the bulb");
        assert!(distance([2.0, 0.0, 0.0]) > 0.0, "a point at |c| = 2 is outside");
        assert!(distance([HALF, HALF, HALF]) > 0.0, "the box corner is outside");
        // Far outside, the estimate grows with the distance.
        assert!(distance([4.0, 0.0, 0.0]) > distance([2.0, 0.0, 0.0]));
    }

    /// Pure deterministic math: the site mirrors these constants, so two
    /// runs must agree triangle for triangle.
    #[test]
    fn the_mesh_is_deterministic() {
        assert_eq!(banded_mesh(), banded_mesh());
    }

    /// The mesh is a recognizable bulb: dense, inside the pinned frame, and
    /// spread across the bands the reveal steps through.
    #[test]
    fn the_bulb_fills_the_pinned_bounds() {
        let bands = banded_mesh();
        assert_eq!(bands.len(), BANDS);
        let tris: usize = bands.iter().map(|(_, t)| t.len()).sum();
        assert!(tris > 10_000, "suspiciously sparse bulb: {tris} triangles");
        for (verts, tris) in &bands {
            for v in verts {
                assert!(v.iter().all(|c| c.is_finite()), "{v:?} is not finite");
                for k in 0..3 {
                    assert!(v[k] >= VIEW_LO[k] && v[k] <= VIEW_HI[k], "{v:?} outside the frame");
                }
            }
            for t in tris {
                assert!(t.iter().all(|&i| (i as usize) < verts.len()), "index out of range");
            }
        }
        // The bulb spans most of the sampled height, so the reveal is a
        // steady climb rather than one band doing all the work.
        let filled = bands.iter().filter(|(_, t)| !t.is_empty()).count();
        assert!(filled > BANDS / 2, "only {filled}/{BANDS} bands carry triangles");
    }

    /// Every band's ramp is pinned to the same span: the bulb's lowest and
    /// highest vertex lead each band's vertex list, so a slice near the top
    /// is colored by its height in the bulb, not by its height in itself.
    #[test]
    fn every_band_spans_the_whole_bulb() {
        let bands = banded_mesh();
        let zrange = |verts: &[[f32; 3]]| {
            let zs: Vec<f32> = verts.iter().map(|v| v[2]).collect();
            let lo = zs.iter().copied().fold(f32::INFINITY, f32::min);
            let hi = zs.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            (lo, hi)
        };
        let whole = zrange(&bands[0].0);
        assert!(whole.1 - whole.0 > 1.0, "the bulb is only {} tall", whole.1 - whole.0);
        for (i, (verts, _)) in bands.iter().enumerate() {
            assert_eq!(zrange(verts), whole, "band {i} carries a different ramp span");
        }
    }

    /// The reveal shows one band at a time and ends with all of them.
    #[test]
    fn the_reveal_walks_the_bands() {
        let (mut plot, mut scene) = Scene::build();
        assert_eq!(scene.shown, 1, "the first band is visible from the start");
        assert!(scene.feed(&mut plot, REVEAL_MS * 3.0), "three ticks revealed nothing");
        assert_eq!(scene.shown, 4);
        assert!(!scene.feed(&mut plot, REVEAL_MS * 0.5), "a partial tick revealed a band");
        scene.reveal_all(&mut plot);
        assert!(scene.done());
        assert_eq!(scene.handles.len(), BANDS);
        assert!(!scene.feed(&mut plot, REVEAL_MS * 10.0), "a finished reveal kept going");
    }
}
