//! `plotui example lidar` — a spinning LiDAR-like scanner in a synthetic
//! room. A 16-beam column of rays is cast per azimuth step and the hits are
//! appended live through `extend_pts`, so the room materializes beam by beam
//! while the camera orbits. Height-banded colors: floor deep blue, wall tops
//! warm amber. Constants are mirrored verbatim in site/examples.js (same
//! PRNG, same seed), so the website scene is point-for-point this one.

use plotui_core::{Plot, Rgb, TraceId};
use plotui_ratatui::PlotState;

use crate::examples::{self, Output, Rng};
use crate::interactive::{self, Hooks};
use crate::{record, ExampleArgs};

const DEG: f32 = std::f32::consts::PI / 180.0;

/// Scanner head position: room center, tripod height.
const SENSOR: [f32; 3] = [0.0, 0.0, 0.8];
/// Beams per azimuth column, elevations linearly spaced LO..=HI.
const BEAMS: usize = 16;
const ELEV_LO: f32 = -22.0 * DEG;
const ELEV_HI: f32 = 12.0 * DEG;
/// 0.9° azimuth steps; two full revolutions, then the scan is done
/// (traces only grow — there is no point-removal API — so the sweep is
/// finite by construction: ≤ 12,800 points).
const AZ_COLS: usize = 400;
const REVS: usize = 2;
const TOTAL_COLS: usize = AZ_COLS * REVS;
const AZ_STEP: f32 = 360.0 * DEG / AZ_COLS as f32;
/// 60°/s sweep → one column every 15 ms (~12 s of streaming).
const COL_MS: f64 = 15.0;
const MAX_RANGE: f32 = 9.0;
const SEED: u32 = 20260830;

/// The room, as axis-aligned (min, max) boxes: four perimeter walls, then
/// crates and pillars for the beams to wrap around.
#[rustfmt::skip]
const BOXES: &[([f32; 3], [f32; 3])] = &[
    ([ 7.9, -8.1, 0.0], [ 8.1,  8.1, 2.5]),
    ([-8.1, -8.1, 0.0], [-7.9,  8.1, 2.5]),
    ([-8.1,  7.9, 0.0], [ 8.1,  8.1, 2.5]),
    ([-8.1, -8.1, 0.0], [ 8.1, -7.9, 2.5]),
    ([ 2.0,  1.0, 0.0], [ 3.2,  2.2, 1.2]),
    ([-4.5,  3.0, 0.0], [-3.3,  4.2, 2.0]),
    ([-2.0, -5.0, 0.0], [-0.4, -3.6, 0.9]),
    ([ 4.2, -3.5, 0.0], [ 5.0, -2.7, 2.5]),
    ([-6.2, -1.0, 0.0], [-5.4,  0.2, 1.6]),
];

const NBANDS: usize = 6;
/// Height bands (upper z bound → color): deep blue → cyan → warm amber.
const BANDS: [(f32, Rgb); NBANDS] = [
    (0.15, [45, 70, 165]),
    (0.60, [40, 120, 205]),
    (1.10, [45, 175, 215]),
    (1.60, [110, 205, 185]),
    (2.10, [200, 200, 120]),
    (f32::INFINITY, [235, 175, 90]),
];

fn band(z: f32) -> usize {
    BANDS.iter().position(|&(top, _)| z < top).unwrap_or(NBANDS - 1)
}

/// Ray/AABB slab test: distance to entry, if the ray hits ahead of the
/// sensor.
fn slab(o: [f32; 3], d: [f32; 3], lo: [f32; 3], hi: [f32; 3]) -> Option<f32> {
    let (mut t0, mut t1) = (0.0f32, f32::INFINITY);
    for k in 0..3 {
        if d[k].abs() < 1e-8 {
            if o[k] < lo[k] || o[k] > hi[k] {
                return None;
            }
        } else {
            let (mut a, mut b) = ((lo[k] - o[k]) / d[k], (hi[k] - o[k]) / d[k]);
            if a > b {
                std::mem::swap(&mut a, &mut b);
            }
            t0 = t0.max(a);
            t1 = t1.min(b);
            if t0 > t1 {
                return None;
            }
        }
    }
    (t0 > 1e-4).then_some(t0)
}

/// Range to the nearest surface (ground plane or a box) within MAX_RANGE.
fn cast(o: [f32; 3], d: [f32; 3]) -> Option<f32> {
    let mut best = MAX_RANGE;
    let mut hit = false;
    if d[2] < 0.0 {
        let t = -o[2] / d[2];
        if t < best && (o[0] + d[0] * t).abs() <= 8.0 && (o[1] + d[1] * t).abs() <= 8.0 {
            best = t;
            hit = true;
        }
    }
    for &(lo, hi) in BOXES {
        if let Some(t) = slab(o, d, lo, hi) {
            if t < best {
                best = t;
                hit = true;
            }
        }
    }
    hit.then_some(best)
}

struct Scene {
    /// Azimuth columns emitted so far (across all revolutions).
    az_step: usize,
    handles: [TraceId; NBANDS],
    rng: Rng,
    acc: f64,
}

impl Scene {
    fn build() -> (Plot, Scene) {
        let mut plot = Plot::new();
        // Pin the frame to the room so the camera never "breathes" as
        // points arrive.
        plot.bounds_override = Some(([-8.5, -8.5, -0.4], [8.5, 8.5, 3.6]));
        let handles = BANDS.map(|(_, color)| plot.add_scatter3d(vec![], color, 1.8, None));
        (plot, Scene { az_step: 0, handles, rng: Rng(SEED), acc: 0.0 })
    }

    fn done(&self) -> bool {
        self.az_step >= TOTAL_COLS
    }

    /// One azimuth column: BEAMS rays, hits sorted into height bands.
    fn column_into(&mut self, bands: &mut [Vec<[f32; 3]>; NBANDS]) {
        let (sin_t, cos_t) = (self.az_step as f32 * AZ_STEP).sin_cos();
        for b in 0..BEAMS {
            let phi = ELEV_LO + (ELEV_HI - ELEV_LO) * b as f32 / (BEAMS - 1) as f32;
            let (sin_p, cos_p) = phi.sin_cos();
            let d = [cos_p * cos_t, cos_p * sin_t, sin_p];
            let Some(t) = cast(SENSOR, d) else {
                self.rng.gauss(); // keep the noise stream aligned across misses
                continue;
            };
            let r = t + self.rng.gauss() * 0.02;
            let p = [SENSOR[0] + d[0] * r, SENSOR[1] + d[1] * r, SENSOR[2] + d[2] * r];
            bands[band(p[2])].push(p);
        }
        self.az_step += 1;
    }

    /// Append every column the sweep crossed in `dt_ms`; true if points
    /// were added.
    fn feed(&mut self, plot: &mut Plot, dt_ms: f64) -> bool {
        self.acc += dt_ms;
        let mut bands: [Vec<[f32; 3]>; NBANDS] = Default::default();
        while self.acc >= COL_MS && !self.done() {
            self.acc -= COL_MS;
            self.column_into(&mut bands);
        }
        let mut added = false;
        for (i, pts) in bands.iter().enumerate() {
            if !pts.is_empty() {
                plot.extend_pts(self.handles[i], pts).expect("scatter3d handle");
                added = true;
            }
        }
        added
    }
}

pub fn run(args: &ExampleArgs, out: Output) -> std::io::Result<()> {
    let (mut plot, mut scene) = Scene::build();

    if out.is_still() {
        // One frame: the completed scan.
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

    fn full_sweep() -> [Vec<[f32; 3]>; NBANDS] {
        let (_, mut scene) = Scene::build();
        let mut bands: [Vec<[f32; 3]>; NBANDS] = Default::default();
        while !scene.done() {
            scene.column_into(&mut bands);
        }
        bands
    }

    /// Same seed, same stream: the site mirrors these constants, so the
    /// sweep must be reproducible.
    #[test]
    fn sweep_is_deterministic() {
        let (a, b) = (full_sweep(), full_sweep());
        for i in 0..NBANDS {
            assert_eq!(a[i], b[i], "band {i} diverged");
        }
    }

    /// bounds_override pins the frame; a point outside it would clip.
    #[test]
    fn all_points_inside_the_pinned_bounds() {
        let (lo, hi) = Scene::build().0.bounds_override.expect("build pins bounds");
        let bands = full_sweep();
        let n: usize = bands.iter().map(Vec::len).sum();
        assert!(n > 8_000, "suspiciously sparse sweep: {n} points");
        for p in bands.iter().flatten() {
            for k in 0..3 {
                assert!(p[k] >= lo[k] && p[k] <= hi[k], "{p:?} outside bounds");
            }
        }
    }

    /// The ray caster agrees with the room geometry.
    #[test]
    fn cast_hits_floor_and_walls_where_expected() {
        let down = cast(SENSOR, [0.0, 0.0, -1.0]).expect("floor below the sensor");
        assert!((down - 0.8).abs() < 1e-5, "floor range {down}");
        let east = cast(SENSOR, [1.0, 0.0, 0.0]).expect("east wall");
        assert!((east - 7.9).abs() < 1e-4, "wall range {east}");
        assert_eq!(cast(SENSOR, [0.0, 0.0, 1.0]), None, "no ceiling");
    }
}
