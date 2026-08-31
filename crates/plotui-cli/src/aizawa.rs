//! `plotui example aizawa` — the Aizawa attractor drawing itself.
//!
//! A fourth-order Runge-Kutta integration of the Aizawa system, appended
//! live through `extend_pts` so the trajectory grows one stroke at a time
//! while the camera orbits: the flat outer shell first, then the column
//! that threads back up through its middle.
//!
//! The curve is split across six `Line3d` traces by speed, so the slow
//! central spike and the fast outer sweeps are colored differently. A
//! trajectory that stays in one band is one continuous run; a band change
//! starts a new run, which is what the NaN gap markers separate.
//!
//! The trajectory is integrated in f64 and cast to f32 only when the points
//! reach the plot. That is not precision for its own sake: the system is
//! chaotic, so an f32 orbit and an f64 one separate visibly over 12 000
//! steps, and site/examples.js — which mirrors these constants verbatim —
//! has only f64. Nothing here is transcendental, so both sides round
//! identically and the website scene is stroke-for-stroke this one.

use plotui_core::{Plot, Rgb, TraceId};
use plotui_ratatui::PlotState;

use crate::examples::{self, Output};
use crate::interactive::{self, Hooks};
use crate::{record, ExampleArgs};

/// The Aizawa system's standard parameters.
const A: f64 = 0.95;
const B: f64 = 0.7;
const C: f64 = 0.6;
const D: f64 = 3.5;
const E: f64 = 0.25;
const F: f64 = 0.1;

/// Integration step and length. 12 000 RK4 steps of 0.01 is 120 time units
/// — enough for the shell to close without overdrawing into a solid blob.
const DT: f64 = 0.01;
const STEPS: usize = 12_000;
/// One step per millisecond: ~12 s of drawing, the lidar sweep's pace.
const STEP_MS: f64 = 1.0;
/// Just off the origin, so the opening strokes spiral outward from the
/// middle instead of starting mid-shell.
const START: [f64; 3] = [0.1, 0.0, 0.0];

const NBANDS: usize = 6;
/// Speed bands (upper bound → color), as fixed thresholds rather than the
/// running min/max: a band must mean the same thing in the first second as
/// in the last, or the curve would recolor itself as it grows. Colors are
/// Plasma sampled at 0.12 + 0.85 b / 5, skipping the darkest end so the
/// slowest band still reads against the background.
#[rustfmt::skip]
const BANDS: [(f64, Rgb); NBANDS] = [
    (0.85, [67, 6, 151]),
    (1.40, [138, 14, 160]),
    (2.20, [192, 60, 128]),
    (3.15, [227, 112, 91]),
    (4.30, [246, 169, 58]),
    (f64::INFINITY, [241, 237, 37]),
];
const WIDTH: f32 = 1.4;

/// The camera box, a little wider than the attractor's own extent
/// (x, y ∈ ±1.51, z ∈ −0.36..1.89) so the frame never breathes as the
/// curve grows into it.
const VIEW_LO: [f32; 3] = [-1.6, -1.6, -0.6];
const VIEW_HI: [f32; 3] = [1.6, 1.6, 1.9];

/// The Aizawa vector field at `p`.
fn deriv(p: [f64; 3]) -> [f64; 3] {
    let [x, y, z] = p;
    [
        (z - B) * x - D * y,
        D * x + (z - B) * y,
        C + A * z - z * z * z / 3.0 - (x * x + y * y) * (1.0 + E * z) + F * z * x * x * x,
    ]
}

/// One classical Runge-Kutta step. The field is smooth but the spike is
/// stiff enough that Euler visibly spirals off it; RK4 at this step size
/// stays on the attractor for the whole run.
fn rk4(p: [f64; 3], dt: f64) -> [f64; 3] {
    let step = |a: [f64; 3], k: [f64; 3], s: f64| std::array::from_fn(|i| a[i] + k[i] * s);
    let k1 = deriv(p);
    let k2 = deriv(step(p, k1, dt * 0.5));
    let k3 = deriv(step(p, k2, dt * 0.5));
    let k4 = deriv(step(p, k3, dt));
    std::array::from_fn(|i| p[i] + dt / 6.0 * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]))
}

fn speed(p: [f64; 3]) -> f64 {
    let d = deriv(p);
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

/// Which color band a point's speed falls in.
fn band(p: [f64; 3]) -> usize {
    let s = speed(p);
    BANDS.iter().position(|&(top, _)| s < top).unwrap_or(NBANDS - 1)
}

/// The plot stores f32; the integrator does not.
fn pt(p: [f64; 3]) -> [f32; 3] {
    p.map(|c| c as f32)
}

struct Scene {
    /// Current state, and how many segments have been drawn.
    p: [f64; 3],
    step: usize,
    handles: [TraceId; NBANDS],
    /// Per band, the step whose endpoint the band's polyline currently ends
    /// at — `Some(step)` means the next segment can extend that run instead
    /// of starting a new one.
    last: [Option<usize>; NBANDS],
    acc: f64,
}

impl Scene {
    fn build() -> (Plot, Scene) {
        let mut plot = Plot::new();
        // Pin the frame to the attractor so the camera never "breathes" as
        // the curve grows.
        plot.bounds_override = Some((VIEW_LO, VIEW_HI));
        let handles = std::array::from_fn(|b| plot.add_line3d(vec![], BANDS[b].1, WIDTH, None));
        (plot, Scene { p: START, step: 0, handles, last: [None; NBANDS], acc: 0.0 })
    }

    fn done(&self) -> bool {
        self.step >= STEPS
    }

    /// Integrate one step and file the new segment under its speed band.
    fn advance_into(&mut self, bands: &mut [Vec<[f32; 3]>; NBANDS]) {
        let prev = self.p;
        let next = rk4(prev, DT);
        let b = band(prev);
        if self.last[b] == Some(self.step) {
            // This band already ends at `prev`: continue the run.
            bands[b].push(pt(next));
        } else {
            // A NaN breaks the polyline, so the jump from wherever this band
            // left off is not drawn as a segment.
            bands[b].extend([[f32::NAN; 3], pt(prev), pt(next)]);
        }
        self.step += 1;
        self.last[b] = Some(self.step);
        self.p = next;
    }

    /// Draw every step the clock crossed in `dt_ms`; true if the curve grew.
    fn feed(&mut self, plot: &mut Plot, dt_ms: f64) -> bool {
        self.acc += dt_ms;
        let mut bands: [Vec<[f32; 3]>; NBANDS] = Default::default();
        while self.acc >= STEP_MS && !self.done() {
            self.acc -= STEP_MS;
            self.advance_into(&mut bands);
        }
        let mut grew = false;
        for (i, pts) in bands.iter().enumerate() {
            if !pts.is_empty() {
                plot.extend_pts(self.handles[i], pts).expect("line3d handle");
                grew = true;
            }
        }
        grew
    }
}

pub fn run(args: &ExampleArgs, out: Output) -> std::io::Result<()> {
    let (mut plot, mut scene) = Scene::build();

    if out.is_still() {
        // One frame: the finished attractor.
        while !scene.done() {
            scene.feed(&mut plot, 50.0);
        }
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

    /// Integrate the whole run, returning the per-band vertex lists.
    fn full_run() -> [Vec<[f32; 3]>; NBANDS] {
        let (_, mut scene) = Scene::build();
        let mut bands: [Vec<[f32; 3]>; NBANDS] = Default::default();
        while !scene.done() {
            scene.advance_into(&mut bands);
        }
        bands
    }

    /// Pure deterministic math — the site mirrors these constants, so two
    /// runs must agree stroke for stroke. Compared bitwise, because the gap
    /// markers are NaN and NaN is not equal to itself.
    #[test]
    fn the_trajectory_is_deterministic() {
        let bits = |bands: [Vec<[f32; 3]>; NBANDS]| {
            bands.map(|pts| pts.iter().map(|p| p.map(f32::to_bits)).collect::<Vec<_>>())
        };
        assert_eq!(bits(full_run()), bits(full_run()));
    }

    /// The one golden value in this file, and the only thing that keeps
    /// site/examples.js honest: an FNV-1a hash of every f32 the trajectory
    /// hands the plot. Safe to pin because the integration is pure IEEE-754
    /// arithmetic — no trig, no pow, nothing from libm — so it is identical
    /// on every platform *and* in the browser, which is the whole reason the
    /// integrator runs in f64.
    ///
    /// If this fails you changed the curve. That is fine, but the site
    /// mirrors these constants: update site/examples.js to match, re-run
    /// the two side by side, then update the hash here.
    #[test]
    fn the_trajectory_matches_the_website_scene() {
        let bands = full_run();
        let mut h: u64 = 0xcbf29ce484222325;
        for p in bands.iter().flatten() {
            for c in p {
                for byte in c.to_bits().to_le_bytes() {
                    h ^= byte as u64;
                    h = h.wrapping_mul(0x100000001b3);
                }
            }
        }
        assert_eq!(
            bands.iter().map(Vec::len).collect::<Vec<_>>(),
            vec![149, 3654, 2954, 1562, 1593, 2424]
        );
        assert_eq!(format!("{h:016x}"), "1c0b5e0f8b195d99");
    }

    /// The integrator stays on the attractor: RK4 at this step size neither
    /// diverges nor collapses, and every drawn point sits inside the pinned
    /// frame.
    #[test]
    fn the_attractor_stays_inside_the_pinned_bounds() {
        let bands = full_run();
        let finite: Vec<[f32; 3]> =
            bands.iter().flatten().copied().filter(|p| p.iter().all(|c| c.is_finite())).collect();
        assert!(finite.len() >= STEPS, "only {} points drawn", finite.len());
        for p in &finite {
            for k in 0..3 {
                assert!(p[k] >= VIEW_LO[k] && p[k] <= VIEW_HI[k], "{p:?} outside the frame");
            }
        }
        // Not a collapsed fixed point either: the curve fills its box.
        let span = |k: usize| {
            let lo = finite.iter().map(|p| p[k]).fold(f32::INFINITY, f32::min);
            let hi = finite.iter().map(|p| p[k]).fold(f32::NEG_INFINITY, f32::max);
            hi - lo
        };
        for k in 0..3 {
            assert!(span(k) > 1.5, "axis {k} spans only {}", span(k));
        }
    }

    /// The fixed thresholds actually straddle the trajectory's speed range,
    /// so the bands are a real spread rather than everything piling into one
    /// end of the ramp.
    #[test]
    fn every_speed_band_is_used() {
        let (_, mut scene) = Scene::build();
        let mut counts = [0usize; NBANDS];
        let mut bands: [Vec<[f32; 3]>; NBANDS] = Default::default();
        while !scene.done() {
            counts[band(scene.p)] += 1;
            scene.advance_into(&mut bands);
        }
        for (b, &n) in counts.iter().enumerate() {
            assert!(n > STEPS / 100, "band {b} holds only {n} of {STEPS} steps");
        }
    }

    /// Gap markers are well formed: every run has at least two points, so a
    /// NaN is always followed by two finite vertices and never by another.
    #[test]
    fn nan_markers_separate_well_formed_runs() {
        for (b, pts) in full_run().iter().enumerate() {
            let nan = |p: &[f32; 3]| p.iter().any(|c| !c.is_finite());
            assert!(!nan(&pts[pts.len() - 1]), "band {b} ends on a gap marker");
            for (i, p) in pts.iter().enumerate() {
                if !nan(p) {
                    continue;
                }
                assert!(i + 2 < pts.len(), "band {b}: run at {i} is too short to draw");
                assert!(!nan(&pts[i + 1]) && !nan(&pts[i + 2]), "band {b}: doubled gap at {i}");
            }
        }
    }

    /// The feed draws at STEP_MS per segment and stops at STEPS.
    #[test]
    fn the_feed_draws_at_a_steady_rate() {
        let (mut plot, mut scene) = Scene::build();
        assert!(scene.feed(&mut plot, STEP_MS * 10.0), "ten ticks drew nothing");
        assert_eq!(scene.step, 10);
        assert!(!scene.feed(&mut plot, STEP_MS * 0.5), "a partial tick drew a segment");
        while !scene.done() {
            scene.feed(&mut plot, 1_000.0);
        }
        assert_eq!(scene.step, STEPS);
        assert!(!scene.feed(&mut plot, 10_000.0), "a finished curve kept drawing");
        // Line vertices shape the extent but are not pick targets. Each
        // segment contributes one vertex, plus one more wherever a run
        // starts, so the count sits just above STEPS.
        assert_eq!(plot.node_count(), 0);
        let verts = plot.vertex_count();
        assert!((STEPS..STEPS + STEPS / 10).contains(&verts), "{verts} vertices for {STEPS} steps");
    }
}
