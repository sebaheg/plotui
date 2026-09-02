//! Sweeping a cross-section along a polyline: round tubes and flat ribbons,
//! as indexed triangle meshes for [`crate::Trace::Mesh3d`].
//!
//! The caller supplies the path, so the geometry here knows nothing about
//! what the path means — a streamline, an integrated trajectory, a road,
//! a protein backbone. What it does know is that a naively swept section
//! spins about its own axis wherever the curve twists, which reads as a
//! ribbon flapping for no reason. [`tube`] and [`ribbon`] therefore carry
//! a rotation-minimizing frame (parallel transport) along the path;
//! [`ribbon`] also takes an explicit per-point face normal, for callers
//! that have a real one to impose — a peptide plane, a banking angle.
//!
//! Paths are usually too coarse to sweep directly (a protein's alpha
//! carbons sit 3.8 Å apart); [`catmull_rom`] resamples one into something
//! smooth enough that the swept surface reads as a curve.

use crate::{vcross, vsub};

/// Vector helpers. The two the renderer already has are shared; these are
/// the rest of what a sweep needs.
#[inline]
fn vadd(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

#[inline]
fn vmul(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

#[inline]
fn vdot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[inline]
fn vlen(a: [f32; 3]) -> f32 {
    vdot(a, a).sqrt()
}

/// Normalize, or `None` when there is no direction to speak of.
#[inline]
fn vunit(a: [f32; 3]) -> Option<[f32; 3]> {
    let n = vlen(a);
    (n > 1e-9).then(|| vmul(a, 1.0 / n))
}

/// Component of `a` perpendicular to the unit vector `axis`, normalized.
#[inline]
fn reject(a: [f32; 3], axis: [f32; 3]) -> Option<[f32; 3]> {
    vunit(vsub(a, vmul(axis, vdot(a, axis))))
}

/// Rotate `v` about the unit `axis` by `angle`, the Rodrigues form.
fn rotate(v: [f32; 3], axis: [f32; 3], angle: f32) -> [f32; 3] {
    let (s, c) = angle.sin_cos();
    let a = vmul(v, c);
    let b = vmul(vcross(axis, v), s);
    let d = vmul(axis, vdot(axis, v) * (1.0 - c));
    vadd(vadd(a, b), d)
}

/// Index into a per-point parameter slice, repeating the last entry. A
/// one-element slice is therefore a constant, which is the common case.
#[inline]
fn at(vals: &[f32], i: usize) -> f32 {
    vals[i.min(vals.len() - 1)]
}

/// Resample `path` through a uniform Catmull-Rom spline, `per_segment`
/// samples for each input segment plus the final point — so `n` points
/// become `(n - 1) * per_segment + 1`. The curve passes through every
/// input point; the ends are clamped by duplicating the first and last.
///
/// Uniform (rather than centripetal) parameterization is the right trade
/// here because the paths this is built for are near-equidistant to begin
/// with; on wildly uneven spacing it can overshoot at the short segments.
pub fn catmull_rom(path: &[[f32; 3]], per_segment: usize) -> Vec<[f32; 3]> {
    let n = path.len();
    if n < 2 || per_segment == 0 {
        return path.to_vec();
    }
    let mut out = Vec::with_capacity((n - 1) * per_segment + 1);
    for i in 0..n - 1 {
        let p0 = path[i.saturating_sub(1)];
        let (p1, p2) = (path[i], path[i + 1]);
        let p3 = path[(i + 2).min(n - 1)];
        for k in 0..per_segment {
            let t = k as f32 / per_segment as f32;
            let (t2, t3) = (t * t, t * t * t);
            out.push(std::array::from_fn(|d| {
                0.5 * (2.0 * p1[d]
                    + (p2[d] - p0[d]) * t
                    + (2.0 * p0[d] - 5.0 * p1[d] + 4.0 * p2[d] - p3[d]) * t2
                    + (3.0 * p1[d] - p0[d] - 3.0 * p2[d] + p3[d]) * t3)
            }));
        }
    }
    out.push(path[n - 1]);
    out
}

/// Unit tangents along `path`: central differences inside, one-sided at
/// the ends, so the frame turns with the curve instead of stepping at each
/// control point. A repeated point has no direction of its own and inherits
/// its predecessor's.
fn tangents(path: &[[f32; 3]]) -> Vec<[f32; 3]> {
    let n = path.len();
    let mut last = [1.0, 0.0, 0.0];
    (0..n)
        .map(|i| {
            let (a, b) = (path[i.saturating_sub(1)], path[(i + 1).min(n - 1)]);
            last = vunit(vsub(b, a)).unwrap_or(last);
            last
        })
        .collect()
}

/// A rotation-minimizing normal field along `tan`: one reference vector is
/// carried down the path, turned at each step by exactly the rotation that
/// takes the previous tangent onto the current one. Nothing else rotates
/// it, which is the whole point — a section swept on these frames does not
/// spin about its own axis.
fn transport(tan: &[[f32; 3]]) -> Vec<[f32; 3]> {
    // Seed with the world axis the first tangent leans on least, so the
    // rejection below never divides by ~0.
    let t0 = tan[0];
    let axis = (0..3).min_by(|&a, &b| t0[a].abs().total_cmp(&t0[b].abs())).unwrap_or(0);
    let mut seed = [0.0; 3];
    seed[axis] = 1.0;
    let mut n = reject(seed, t0).unwrap_or([0.0, 0.0, 1.0]);

    let mut out = Vec::with_capacity(tan.len());
    out.push(n);
    for i in 1..tan.len() {
        let v = vcross(tan[i - 1], tan[i]);
        let s = vlen(v);
        // Collinear tangents leave the frame untouched; the re-rejection
        // below still keeps it square to the (unchanged) tangent.
        if let Some(axis) = vunit(v) {
            n = rotate(n, axis, s.atan2(vdot(tan[i - 1], tan[i])));
        }
        n = reject(n, tan[i]).unwrap_or(n);
        out.push(n);
    }
    out
}

/// Stitch equal-length cross-section rings into a closed surface: a quad
/// band between each consecutive pair, plus a centroid fan capping either
/// end so a cut tube is not a hollow straw.
///
/// Winding is consistent but not load-bearing for this crate's renderer,
/// which shades two-sided and culls nothing.
fn stitch(rings: &[Vec<[f32; 3]>]) -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
    let sides = rings[0].len();
    let mut verts: Vec<[f32; 3]> = rings.iter().flatten().copied().collect();
    let mut tris = Vec::with_capacity(rings.len() * sides * 2 + sides * 2);
    let v = |ring: usize, j: usize| (ring * sides + j) as u32;

    for i in 0..rings.len() - 1 {
        for j in 0..sides {
            let k = (j + 1) % sides;
            tris.push([v(i, j), v(i + 1, j), v(i + 1, k)]);
            tris.push([v(i, j), v(i + 1, k), v(i, k)]);
        }
    }

    let centroid = |ring: &Vec<[f32; 3]>| {
        let s = ring.iter().fold([0.0; 3], |a, &p| vadd(a, p));
        vmul(s, 1.0 / sides as f32)
    };
    let (first, last) = (0, rings.len() - 1);
    verts.push(centroid(&rings[first]));
    verts.push(centroid(&rings[last]));
    let (c0, c1) = (verts.len() as u32 - 2, verts.len() as u32 - 1);
    for j in 0..sides {
        let k = (j + 1) % sides;
        tris.push([c0, v(first, k), v(first, j)]);
        tris.push([c1, v(last, j), v(last, k)]);
    }
    (verts, tris)
}

/// Sweep a circular cross-section of `radii` along `path`, `sides` facets
/// around, capped at both ends.
///
/// `radii[i]` is the radius at `path[i]`; a shorter slice repeats its last
/// entry, so a one-element slice is a constant radius and a per-point slice
/// tapers. `sides` is clamped to at least 3. A path of fewer than two
/// points, or an empty `radii`, sweeps nothing and returns an empty mesh.
///
/// `path` must be finite: a NaN has no tangent, and the frame it poisons
/// propagates down the rest of the curve. Split a gapped path yourself and
/// sweep each run.
pub fn tube(path: &[[f32; 3]], radii: &[f32], sides: usize) -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
    if path.len() < 2 || radii.is_empty() {
        return (vec![], vec![]);
    }
    let sides = sides.max(3);
    let tan = tangents(path);
    let nrm = transport(&tan);
    let rings: Vec<Vec<[f32; 3]>> = path
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            let (u, w) = (nrm[i], vcross(tan[i], nrm[i]));
            let r = at(radii, i);
            (0..sides)
                .map(|j| {
                    let a = std::f32::consts::TAU * j as f32 / sides as f32;
                    let (s, c) = a.sin_cos();
                    vadd(p, vadd(vmul(u, c * r), vmul(w, s * r)))
                })
                .collect()
        })
        .collect();
    stitch(&rings)
}

/// Sweep a flat rectangular cross-section along `path`: `widths` across the
/// face, `thickness` through it.
///
/// `up[i]` is the face normal at `path[i]` — the direction the flat of the
/// ribbon points. It is re-squared against the tangent, so it need only be
/// approximate; where it is missing, empty, or parallel to the tangent, the
/// transported frame stands in. `widths` is indexed like [`tube`]'s
/// `radii`, so tapering the last few entries to zero gives an arrowhead.
///
/// The same finiteness requirement as [`tube`] applies to `path`.
pub fn ribbon(
    path: &[[f32; 3]],
    up: &[[f32; 3]],
    widths: &[f32],
    thickness: f32,
) -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
    if path.len() < 2 || widths.is_empty() {
        return (vec![], vec![]);
    }
    let tan = tangents(path);
    let nrm = transport(&tan);
    let ht = thickness * 0.5;
    let rings: Vec<Vec<[f32; 3]>> = path
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            // `right` spans the face; `face` is the given normal squared up
            // against the tangent, or the transported one where there is
            // none to give.
            let want = up.get(i).copied().unwrap_or(nrm[i]);
            let right = vunit(vcross(tan[i], want)).unwrap_or_else(|| vcross(tan[i], nrm[i]));
            let face = vcross(right, tan[i]);
            let hw = at(widths, i) * 0.5;
            let (a, b) = (vmul(right, hw), vmul(face, ht));
            vec![vadd(p, vadd(a, b)), vadd(p, vsub(b, a)), vsub(p, vadd(a, b)), vadd(p, vsub(a, b))]
        })
        .collect();
    stitch(&rings)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A quarter turn of a helix: curved, twisting, and finite everywhere —
    /// the shape a sweep has to survive.
    fn helix(n: usize) -> Vec<[f32; 3]> {
        (0..n)
            .map(|i| {
                let a = i as f32 * 0.35;
                [a.cos(), a.sin(), i as f32 * 0.2]
            })
            .collect()
    }

    fn close(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    /// Every triangle indexes a real vertex, and no vertex is NaN — the two
    /// things `Mesh3d` would otherwise silently drop on the floor.
    fn well_formed(verts: &[[f32; 3]], tris: &[[u32; 3]]) {
        assert!(!verts.is_empty() && !tris.is_empty(), "swept nothing");
        for v in verts {
            assert!(v.iter().all(|c| c.is_finite()), "non-finite vertex {v:?}");
        }
        for t in tris {
            for &i in t {
                assert!(
                    (i as usize) < verts.len(),
                    "triangle indexes vertex {i} of {}",
                    verts.len()
                );
            }
        }
    }

    /// The spline interpolates: every control point is still on the curve,
    /// at the sample its segment starts on.
    #[test]
    fn catmull_rom_passes_through_its_control_points() {
        let path = helix(8);
        let per = 6;
        let out = catmull_rom(&path, per);
        assert_eq!(out.len(), (path.len() - 1) * per + 1);
        for (i, p) in path.iter().enumerate() {
            let got = out[i * per];
            for d in 0..3 {
                assert!(close(got[d], p[d], 1e-5), "control point {i}: {got:?} != {p:?}");
            }
        }
    }

    /// Resampling is a no-op where there is nothing to resample.
    #[test]
    fn catmull_rom_leaves_degenerate_paths_alone() {
        let one = [[1.0, 2.0, 3.0]];
        assert_eq!(catmull_rom(&one, 4), one);
        assert_eq!(catmull_rom(&[], 4), Vec::<[f32; 3]>::new());
        let two = [[0.0; 3], [1.0, 0.0, 0.0]];
        assert_eq!(catmull_rom(&two, 0), two);
    }

    /// A swept tube is exactly `radius` from its path, all the way round —
    /// the frames are orthonormal or this drifts.
    #[test]
    fn tube_holds_its_radius_about_the_path() {
        let path = helix(24);
        let (verts, tris) = tube(&path, &[0.3], 12);
        well_formed(&verts, &tris);
        // The trailing two vertices are the end caps' centroids, which sit
        // on the path rather than on the surface.
        assert_eq!(verts.len(), path.len() * 12 + 2);
        for (i, p) in path.iter().enumerate() {
            for j in 0..12 {
                let d = vlen(vsub(verts[i * 12 + j], *p));
                assert!(close(d, 0.3, 1e-4), "ring {i} vertex {j} is {d} from the path");
            }
        }
    }

    /// Per-point radii taper. A trailing zero collapses the last ring onto
    /// the path, which is what an arrowhead's tip is made of.
    #[test]
    fn tube_radii_are_per_point_and_repeat_their_last_entry() {
        let path = helix(4);
        let (verts, _) = tube(&path, &[1.0, 0.5, 0.0], 8);
        let ring_r = |i: usize| vlen(vsub(verts[i * 8], path[i]));
        assert!(close(ring_r(0), 1.0, 1e-5));
        assert!(close(ring_r(1), 0.5, 1e-5));
        assert!(close(ring_r(2), 0.0, 1e-5));
        // Point 3 has no entry of its own and repeats the last.
        assert!(close(ring_r(3), 0.0, 1e-5));
    }

    /// Nothing to sweep sweeps nothing, rather than panicking on an empty
    /// ring or emitting a degenerate cap.
    #[test]
    fn degenerate_sweeps_are_empty() {
        assert_eq!(tube(&[[0.0; 3]], &[1.0], 8), (vec![], vec![]));
        assert_eq!(tube(&helix(4), &[], 8), (vec![], vec![]));
        assert_eq!(ribbon(&[[0.0; 3]], &[], &[1.0], 0.1), (vec![], vec![]));
        assert_eq!(ribbon(&helix(4), &[], &[], 0.1), (vec![], vec![]));
    }

    /// The frame is rotation-minimizing: on a straight path it never turns
    /// at all, so the section keeps one orientation from end to end.
    #[test]
    fn a_straight_sweep_does_not_twist() {
        let path: Vec<[f32; 3]> = (0..10).map(|i| [i as f32, 0.0, 0.0]).collect();
        let (verts, _) = tube(&path, &[0.5], 6);
        for i in 1..path.len() {
            for j in 0..6 {
                let a = vsub(verts[j], path[0]);
                let b = vsub(verts[i * 6 + j], path[i]);
                for d in 0..3 {
                    assert!(close(a[d], b[d], 1e-5), "ring {i} rotated: {a:?} vs {b:?}");
                }
            }
        }
    }

    /// Frames stay square to the curve: every ring lies in the plane the
    /// tangent is normal to, which is what keeps a swept surface from
    /// pinching where the path bends.
    #[test]
    fn rings_stay_perpendicular_to_the_tangent() {
        let path = catmull_rom(&helix(12), 4);
        let tan = tangents(&path);
        let (verts, _) = tube(&path, &[0.4], 8);
        for (i, t) in tan.iter().enumerate() {
            for j in 0..8 {
                let spoke = vsub(verts[i * 8 + j], path[i]);
                assert!(close(vdot(spoke, *t), 0.0, 1e-4), "ring {i} leans on its tangent");
            }
        }
    }

    /// A ribbon points its face where it is told: given `up`, the flat of
    /// the section spans `up` × tangent, not some transported guess.
    #[test]
    fn ribbon_faces_the_normal_it_is_given() {
        // Straight along x, face normal +z: the ribbon should be wide in y
        // and thin in z.
        let path: Vec<[f32; 3]> = (0..6).map(|i| [i as f32, 0.0, 0.0]).collect();
        let up = vec![[0.0, 0.0, 1.0]; path.len()];
        let (verts, tris) = ribbon(&path, &up, &[2.0], 0.2);
        well_formed(&verts, &tris);
        for i in 0..path.len() {
            let ring = &verts[i * 4..i * 4 + 4];
            let span = |d: usize| {
                let lo = ring.iter().map(|p| p[d]).fold(f32::INFINITY, f32::min);
                let hi = ring.iter().map(|p| p[d]).fold(f32::NEG_INFINITY, f32::max);
                hi - lo
            };
            assert!(close(span(1), 2.0, 1e-5), "ring {i} is {} wide in y", span(1));
            assert!(close(span(2), 0.2, 1e-5), "ring {i} is {} thick in z", span(2));
        }
    }

    /// An `up` parallel to the tangent names no plane; the transported
    /// frame stands in rather than the ring collapsing to a line.
    #[test]
    fn a_degenerate_normal_falls_back_to_the_transported_frame() {
        let path: Vec<[f32; 3]> = (0..5).map(|i| [i as f32, 0.0, 0.0]).collect();
        let up = vec![[1.0, 0.0, 0.0]; path.len()];
        let (verts, tris) = ribbon(&path, &up, &[1.0], 0.1);
        well_formed(&verts, &tris);
        for i in 0..path.len() {
            let ring = &verts[i * 4..i * 4 + 4];
            let widest = (0..3)
                .map(|d| {
                    let lo = ring.iter().map(|p| p[d]).fold(f32::INFINITY, f32::min);
                    let hi = ring.iter().map(|p| p[d]).fold(f32::NEG_INFINITY, f32::max);
                    hi - lo
                })
                .fold(0.0f32, f32::max);
            assert!(close(widest, 1.0, 1e-5), "ring {i} collapsed: widest span {widest}");
        }
    }

    /// Sweeping is deterministic — the website and the CLI mirror these
    /// scenes, so the same path has to give the same mesh every time.
    #[test]
    fn sweeps_are_deterministic() {
        let path = catmull_rom(&helix(10), 5);
        assert_eq!(tube(&path, &[0.3], 8), tube(&path, &[0.3], 8));
        assert_eq!(ribbon(&path, &[], &[1.0], 0.2), ribbon(&path, &[], &[1.0], 0.2));
    }
}
