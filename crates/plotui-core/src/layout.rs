//! Force-directed graph layout — the motion half of a network graph.
//!
//! Pure math with no event loop, mirroring the crate's one rule: the host
//! (a TUI framework, the CLI, the browser) owns time, calls [`ForceLayout::step`]
//! on its own tick, and hands the positions to
//! [`Plot::set_graph_positions`](crate::Plot::set_graph_positions).
//! Fruchterman–Reingold in 3D: connected nodes attract like springs, all
//! nodes repel like charges, a mild gravity keeps disconnected components
//! from drifting to infinity, and a cooling temperature caps how far a node
//! may move per step so the layout settles instead of oscillating.
//!
//! Deterministic by construction: seeded initial positions from the same
//! mulberry32 generator the CLI examples and the website demos use, and no
//! dependence on iteration order beyond the caller's node/edge order.

/// mulberry32 — tiny deterministic PRNG, identical to the one in the CLI
/// examples and `site/examples.js`, so a shared seed reproduces a scene
/// across frontends.
struct Rng(u32);

impl Rng {
    /// Uniform in [0, 1).
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x6D2B79F5);
        let mut t = self.0 as u64;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1) & 0xFFFF_FFFF;
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61) & 0xFFFF_FFFF);
        ((t ^ (t >> 14)) & 0xFFFF_FFFF) as f32 / 4294967296.0
    }

    /// Approximately gaussian in ≈[-1, 1] (sum of uniforms).
    fn gauss(&mut self) -> f32 {
        (self.next() + self.next() + self.next() + self.next()) / 2.0 - 1.0
    }

    /// A point uniformly inside the unit ball (rejection sampling).
    fn in_ball(&mut self) -> [f32; 3] {
        loop {
            let p = [self.next() * 2.0 - 1.0, self.next() * 2.0 - 1.0, self.next() * 2.0 - 1.0];
            if p[0] * p[0] + p[1] * p[1] + p[2] * p[2] <= 1.0 {
                return p;
            }
        }
    }
}

/// Ideal edge length as a fraction of the unit-ball radius: `K_C · n^(-1/3)`,
/// the 3D volume-share heuristic.
const K_C: f32 = 0.9;
/// Initial temperature (max per-step displacement), in unit-ball radii.
const T0: f32 = 0.15;
/// Per-step cooling factor and the floor it never cools past. The floor sits
/// under the hosts' practical settle threshold (1e-3), so a cooled layout's
/// mean displacement always drops below it; `add_node` re-heats explicitly.
const COOLING: f32 = 0.985;
const T_MIN: f32 = 2e-4;
/// Re-heat floor applied by `add_node`, as a fraction of `T0`: warm enough
/// for the neighborhood to reorganize, cool enough that the settled bulk
/// barely stirs.
const REHEAT: f32 = 0.35;
/// Minimum pair distance for the repulsion term, so coincident nodes get a
/// bounded, deterministic push instead of an infinite one.
const D_MIN: f32 = 1e-4;

/// A 3D force-directed layout over `n` nodes and an undirected edge list.
/// Node indices are the caller's — the same indices a
/// [`Trace::Graph3d`](crate::Trace::Graph3d) uses, so positions feed
/// straight into [`Plot::set_graph_positions`](crate::Plot::set_graph_positions).
pub struct ForceLayout {
    positions: Vec<[f32; 3]>,
    edges: Vec<(u32, u32)>,
    k: f32,
    temperature: f32,
    rng: Rng,
}

impl ForceLayout {
    /// A layout over `n_nodes` with seeded initial positions in the unit
    /// ball. Edges with out-of-range endpoints are kept but inert (matching
    /// the renderer, which skips them), so the edge list can be passed
    /// verbatim from the plot.
    pub fn new(n_nodes: usize, edges: &[(u32, u32)], seed: u32) -> Self {
        let mut rng = Rng(seed);
        let positions = (0..n_nodes).map(|_| rng.in_ball()).collect();
        let k = K_C * (1.0 / (n_nodes.max(1) as f32)).cbrt();
        ForceLayout { positions, edges: edges.to_vec(), k, temperature: T0, rng }
    }

    /// One simulation tick. Returns the mean displacement this step — the
    /// "energy" a host watches to stop rendering (a practical settled
    /// threshold is `1e-3`, in unit-ball radii).
    pub fn step(&mut self) -> f32 {
        let n = self.positions.len();
        if n == 0 {
            return 0.0;
        }
        let k = self.k;
        let mut disp = vec![[0.0f32; 3]; n];

        // All pairs repel: k²/d, clamped at D_MIN so coincident nodes part.
        for i in 0..n {
            for j in (i + 1)..n {
                let d = sub(self.positions[i], self.positions[j]);
                let dist = len(d).max(D_MIN);
                let f = (k * k) / dist / dist; // (k²/d) / d → per-axis scale
                for a in 0..3 {
                    disp[i][a] += d[a] * f;
                    disp[j][a] -= d[a] * f;
                }
            }
        }

        // Connected nodes attract: d²/k along each edge.
        for &(a, b) in &self.edges {
            let (a, b) = (a as usize, b as usize);
            if a >= n || b >= n || a == b {
                continue;
            }
            let d = sub(self.positions[a], self.positions[b]);
            let dist = len(d).max(D_MIN);
            let f = dist / k; // (d²/k) / d → per-axis scale
            for x in 0..3 {
                disp[a][x] -= d[x] * f;
                disp[b][x] += d[x] * f;
            }
        }

        // Mild gravity toward the origin keeps disconnected components on
        // screen without visibly distorting clusters.
        for (d, p) in disp.iter_mut().zip(&self.positions) {
            for (da, pa) in d.iter_mut().zip(p) {
                *da -= pa * 0.03;
            }
        }

        // Apply, capped by temperature; cool; report mean movement.
        let mut moved = 0.0f32;
        for (p, d) in self.positions.iter_mut().zip(&disp) {
            let mag = len(*d);
            if mag > 0.0 {
                let step = mag.min(self.temperature);
                for (pa, da) in p.iter_mut().zip(d) {
                    *pa += da / mag * step;
                }
                moved += step;
            }
        }
        self.temperature = (self.temperature * COOLING).max(T_MIN);
        moved / n as f32
    }

    /// The current node positions, in the caller's index order — feed these
    /// to [`Plot::set_graph_positions`](crate::Plot::set_graph_positions).
    pub fn positions(&self) -> &[[f32; 3]] {
        &self.positions
    }

    /// Warm insertion of one node connected to `neighbors` (existing
    /// indices): it spawns beside its first neighbor — so it visibly flies
    /// in from there — and the temperature re-heats to a floor so the
    /// neighborhood reorganizes while the settled bulk barely moves.
    /// Returns the new node's index; pair it with
    /// [`Plot::extend_graph`](crate::Plot::extend_graph).
    pub fn add_node(&mut self, neighbors: &[u32]) -> usize {
        let idx = self.positions.len();
        let pos = match neighbors.first().map(|&f| self.positions.get(f as usize)) {
            Some(Some(&p)) => {
                let j = self.k * 0.5;
                [
                    p[0] + self.rng.gauss() * j,
                    p[1] + self.rng.gauss() * j,
                    p[2] + self.rng.gauss() * j,
                ]
            }
            // No (valid) neighbor: drop in from the ball surface.
            _ => {
                let p = self.rng.in_ball();
                let l = len(p).max(D_MIN);
                [p[0] / l, p[1] / l, p[2] / l]
            }
        };
        self.positions.push(pos);
        self.edges.extend(neighbors.iter().map(|&f| (f, idx as u32)));
        self.temperature = self.temperature.max(T0 * REHEAT);
        idx
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn len(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two 4-cliques joined by nothing: a layout must pull each clique
    /// together and push the cliques apart.
    fn two_cliques() -> Vec<(u32, u32)> {
        let mut e = Vec::new();
        for c in [0u32, 4] {
            for i in 0..4 {
                for j in (i + 1)..4 {
                    e.push((c + i, c + j));
                }
            }
        }
        e
    }

    fn centroid(pts: &[[f32; 3]]) -> [f32; 3] {
        let n = pts.len() as f32;
        let mut c = [0.0; 3];
        for p in pts {
            for a in 0..3 {
                c[a] += p[a] / n;
            }
        }
        c
    }

    #[test]
    fn same_seed_is_deterministic() {
        let mut a = ForceLayout::new(8, &two_cliques(), 42);
        let mut b = ForceLayout::new(8, &two_cliques(), 42);
        for _ in 0..50 {
            a.step();
            b.step();
        }
        assert_eq!(a.positions(), b.positions());
    }

    #[test]
    fn energy_settles_under_threshold() {
        let mut l = ForceLayout::new(8, &two_cliques(), 7);
        let mut energy = f32::INFINITY;
        for _ in 0..600 {
            energy = l.step();
        }
        assert!(energy < 1e-3, "layout did not settle: energy {energy}");
    }

    #[test]
    fn disconnected_cliques_separate_into_clusters() {
        let mut l = ForceLayout::new(8, &two_cliques(), 3);
        for _ in 0..400 {
            l.step();
        }
        let (a, b) = l.positions().split_at(4);
        let gap = len(sub(centroid(a), centroid(b)));
        let radius = |pts: &[[f32; 3]]| {
            let c = centroid(pts);
            pts.iter().map(|p| len(sub(*p, c))).fold(0.0f32, f32::max)
        };
        assert!(
            gap > radius(a) + radius(b),
            "cliques overlap: gap {gap}, radii {} + {}",
            radius(a),
            radius(b)
        );
    }

    #[test]
    fn add_node_spawns_beside_its_first_neighbor_and_reheats() {
        let mut l = ForceLayout::new(8, &two_cliques(), 11);
        for _ in 0..500 {
            l.step();
        }
        let anchor = l.positions()[2];
        let idx = l.add_node(&[2, 3]);
        assert_eq!(idx, 8);
        let spawned = l.positions()[8];
        assert!(len(sub(spawned, anchor)) <= l.k, "spawn not beside neighbor");
        assert!(l.temperature >= T0 * REHEAT - f32::EPSILON, "no re-heat");
        // The appended edges must be live: the new node ends up nearer its
        // own clique's centroid than the other clique's.
        for _ in 0..300 {
            l.step();
        }
        let (a, b) = (l.positions()[..4].to_vec(), l.positions()[4..8].to_vec());
        let d_own = len(sub(l.positions()[8], centroid(&a)));
        let d_other = len(sub(l.positions()[8], centroid(&b)));
        assert!(d_own < d_other, "new node settled by the wrong clique");
    }
}
