//! Graph layout — where the nodes of a network go.
//!
//! Two of them, sharing one rule with the rest of the crate: pure math, no
//! event loop. The host (a TUI framework, the CLI, the browser) owns time and
//! hands the result to
//! [`Plot::set_graph_positions`](crate::Plot::set_graph_positions).
//!
//! [`ForceLayout`] is the motion half of a network graph: Fruchterman–Reingold
//! in 3D, where connected nodes attract like springs, all nodes repel like
//! charges, a mild gravity keeps disconnected components from drifting to
//! infinity, and a cooling temperature caps how far a node may move per step
//! so the layout settles instead of oscillating. The host calls
//! [`ForceLayout::step`] on its own tick.
//!
//! [`LayeredLayout`] is the still half, for a graph whose edges mean
//! *direction*: a Sugiyama layout that ranks nodes by depth, orders each rank
//! to reduce crossings, and routes long edges around the nodes they skip. It
//! is solved once, not stepped, because a pipeline has one right shape and
//! watching it settle into that shape says nothing about the pipeline.
//!
//! Both are deterministic by construction. `ForceLayout` seeds its initial
//! positions from the same mulberry32 generator the CLI examples and the
//! website demos use, so a shared seed reproduces a scene across frontends;
//! `LayeredLayout` needs no randomness at all and breaks every tie by index.

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

/// Which way a [`LayeredLayout`] flows: sources at the top with edges running
/// down (`TB`, the DOT default), or sources at the left with edges running
/// right (`LR`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RankDir {
    #[default]
    TB,
    LR,
}

impl RankDir {
    /// The wire names, in declaration order.
    pub const NAMES: [&'static str; 2] = ["TB", "LR"];

    /// Parse a DOT `rankdir=` value, case-insensitively. `None` for anything
    /// else, so the bindings phrase the error once.
    pub fn parse(name: &str) -> Option<RankDir> {
        match name.to_ascii_uppercase().as_str() {
            "TB" | "TD" => Some(RankDir::TB),
            "LR" => Some(RankDir::LR),
            _ => None,
        }
    }
}

/// Which way [`reachable`] walks the edges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Follow edges backwards: everything that leads *to* the start node.
    Upstream,
    /// Follow edges forwards: everything the start node leads to.
    Downstream,
}

/// Which nodes are reachable from `from` by following edges in `dir`,
/// including `from` itself. Out-of-range endpoints are skipped rather than
/// panicking, matching how the renderer and [`ForceLayout`] treat them; a
/// `from` past the end reaches nothing.
///
/// This is the primitive behind "hover a task and light everything it waits
/// on": the core does not decide what a highlight *means*, but every host
/// that wants one needs the same closure, and reimplementing a DFS per
/// frontend is how they drift.
pub fn reachable(n: usize, edges: &[(u32, u32)], from: usize, dir: Direction) -> Vec<bool> {
    let mut seen = vec![false; n];
    if from >= n {
        return seen;
    }
    seen[from] = true;
    let mut stack = vec![from];
    while let Some(a) = stack.pop() {
        for &(x, y) in edges {
            let (from_i, to_i) = match dir {
                Direction::Downstream => (x as usize, y as usize),
                Direction::Upstream => (y as usize, x as usize),
            };
            if from_i == a && to_i < n && !seen[to_i] {
                seen[to_i] = true;
                stack.push(to_i);
            }
        }
    }
    seen
}

/// How many barycenter sweeps to run in each direction. Four is where the
/// improvement stops paying for itself on the graphs a terminal can show;
/// the loop also stops early once a sweep fails to beat the best ordering.
const SWEEPS: usize = 4;

/// A hierarchical ("Sugiyama") layout for a directed graph: rank the nodes by
/// depth, order each rank to reduce edge crossings, then place them so edges
/// run as straight as they can. Feeds
/// [`Trace::Graph2d`](crate::Trace::Graph2d) directly — [`Self::positions`]
/// are node centres and [`Self::routes`] is the CSR waypoint pair.
///
/// Solved in `new`; there is nothing to step. Determinism is a hard
/// requirement — same input, same output, no RNG — so every tie breaks by
/// index and no phase depends on hash order.
pub struct LayeredLayout {
    positions: Vec<[f32; 2]>,
    ranks: Vec<u32>,
    route_pts: Vec<[f32; 2]>,
    route_starts: Vec<u32>,
}

/// One edge as the layout sees it: which of the caller's edges it is, the
/// endpoints *after* cycle removal, and whether that removal flipped it.
/// Reversed edges still emit their waypoints in the caller's direction, so a
/// cycle is a drawing artefact and never an index one.
#[derive(Clone, Copy)]
struct LayoutEdge {
    orig: usize,
    from: usize,
    to: usize,
    reversed: bool,
}

impl LayeredLayout {
    /// Lay out `n_nodes` connected by `edges`, flowing in `dir`.
    ///
    /// Self-loops and edges with an out-of-range endpoint are kept inert —
    /// they take no part in the layout but keep their index, so
    /// [`Self::routes`] still has one (empty) run per edge and the caller's
    /// edge list can be passed straight through. Cycles do not hang: a back
    /// edge is reversed for the layout and drawn in its original direction.
    pub fn new(n_nodes: usize, edges: &[(u32, u32)], dir: RankDir) -> Self {
        let live = remove_cycles(n_nodes, edges);
        let ranks = rank_nodes(n_nodes, &live);
        let (layers, segs, chains, rank_of) = insert_dummies(n_nodes, &live, &ranks);
        let layers = reduce_crossings(layers, &segs);
        let x = assign_x(&layers, &segs, n_nodes);

        // `[col, -rank]` puts rank 0 highest on screen (data y is up), which
        // is what "sources on top" means once the frame flips it.
        let place = |col: f32, rank: f32| -> [f32; 2] {
            match dir {
                RankDir::TB => [col, -rank],
                RankDir::LR => [rank, -col],
            }
        };
        let positions = (0..n_nodes).map(|v| place(x[v], ranks[v] as f32)).collect::<Vec<_>>();

        // Waypoints per *caller* edge, in the caller's direction.
        let mut route_pts = Vec::new();
        let mut route_starts = vec![0u32; edges.len()];
        let mut per_edge: Vec<Vec<[f32; 2]>> = vec![Vec::new(); edges.len()];
        for (e, chain) in chains {
            per_edge[e] = chain.iter().map(|&d| place(x[d], rank_of[d] as f32)).collect();
        }
        for (e, chain) in per_edge.iter_mut().enumerate() {
            route_starts[e] = route_pts.len() as u32;
            route_pts.append(chain);
        }
        LayeredLayout { positions, ranks, route_pts, route_starts }
    }

    /// Node centres in the caller's index order — feed these to
    /// [`Plot::add_graph2d`](crate::Plot::add_graph2d) or
    /// [`Plot::set_graph_positions`](crate::Plot::set_graph_positions).
    pub fn positions(&self) -> &[[f32; 2]] {
        &self.positions
    }

    /// Each node's rank: 0 for a source, one more than its deepest
    /// predecessor otherwise. Hosts colour or group by this.
    pub fn ranks(&self) -> &[u32] {
        &self.ranks
    }

    /// Edge waypoints as the CSR pair
    /// [`Trace::Graph2d`](crate::Trace::Graph2d) takes: one run per edge, in
    /// the caller's edge order and direction, empty for a straight edge.
    pub fn routes(&self) -> (&[[f32; 2]], &[u32]) {
        (&self.route_pts, &self.route_starts)
    }
}

/// Phase 1 — cycle removal. Depth-first from every unvisited node in index
/// order; an edge back into the current path is reversed *for the layout
/// only*. Self-loops and out-of-range endpoints drop out here, the same
/// policy [`ForceLayout::new`] applies.
///
/// The DFS is iterative: a dependency graph is exactly the shape that gets
/// deep, and blowing the stack on someone's monorepo is not an acceptable
/// failure mode.
fn remove_cycles(n: usize, edges: &[(u32, u32)]) -> Vec<LayoutEdge> {
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        White,
        Gray,
        Black,
    }
    let live: Vec<LayoutEdge> = edges
        .iter()
        .enumerate()
        .filter_map(|(i, &(a, b))| {
            let (a, b) = (a as usize, b as usize);
            (a < n && b < n && a != b).then_some(LayoutEdge {
                orig: i,
                from: a,
                to: b,
                reversed: false,
            })
        })
        .collect();
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (k, e) in live.iter().enumerate() {
        out[e.from].push(k);
    }
    let mut state = vec![State::White; n];
    let mut back = vec![false; live.len()];
    // Each frame is (node, how many of its out-edges we have walked).
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for s in 0..n {
        if state[s] != State::White {
            continue;
        }
        state[s] = State::Gray;
        stack.push((s, 0));
        while let Some(&mut (u, ref mut i)) = stack.last_mut() {
            if *i >= out[u].len() {
                state[u] = State::Black;
                stack.pop();
                continue;
            }
            let k = out[u][*i];
            *i += 1;
            let v = live[k].to;
            match state[v] {
                State::White => {
                    state[v] = State::Gray;
                    stack.push((v, 0));
                }
                // v is on the current path: this edge closes a cycle.
                State::Gray => back[k] = true,
                State::Black => {}
            }
        }
    }
    live.into_iter()
        .zip(back)
        .map(|(e, is_back)| {
            if is_back {
                LayoutEdge { orig: e.orig, from: e.to, to: e.from, reversed: true }
            } else {
                e
            }
        })
        .collect()
}

/// Phase 2 — ranking. Longest path from the sources, so an edge always runs
/// from a lower rank to a higher one, followed by one tightening pass that
/// pulls a node down toward its successors when doing so shortens more edges
/// than it lengthens. Without the pass a source feeding one deep task sits
/// alone at the top with a wire running past three ranks of nothing.
fn rank_nodes(n: usize, edges: &[LayoutEdge]) -> Vec<u32> {
    let mut indeg = vec![0usize; n];
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut inc: Vec<Vec<usize>> = vec![Vec::new(); n];
    for e in edges {
        out[e.from].push(e.to);
        inc[e.to].push(e.from);
        indeg[e.to] += 1;
    }
    // Kahn's order, sources in index order, so the result never depends on
    // anything but the caller's numbering.
    let mut queue: Vec<usize> = (0..n).filter(|&v| indeg[v] == 0).collect();
    let mut topo = Vec::with_capacity(n);
    let mut head = 0;
    let mut rank = vec![0u32; n];
    while head < queue.len() {
        let u = queue[head];
        head += 1;
        topo.push(u);
        for &v in &out[u] {
            rank[v] = rank[v].max(rank[u] + 1);
            indeg[v] -= 1;
            if indeg[v] == 0 {
                queue.push(v);
            }
        }
    }
    // Cycle removal guarantees a full topological order; a node left out
    // would mean a cycle survived, and its rank stays where it is rather
    // than the layout failing.
    for &u in topo.iter().rev() {
        // Moving `u` later lengthens every incoming edge by the same amount
        // it shortens every outgoing one, so it only pays when there are
        // more of the latter.
        if out[u].len() <= inc[u].len() {
            continue;
        }
        let Some(succ_min) = out[u].iter().map(|&v| rank[v]).min() else { continue };
        let floor = inc[u].iter().map(|&v| rank[v] + 1).max().unwrap_or(0);
        rank[u] = (succ_min.saturating_sub(1)).max(floor);
    }
    // A rank list that no longer starts at 0 (every source pulled down)
    // would leave an empty top layer.
    if let Some(&lo) = rank.iter().min() {
        for r in &mut rank {
            *r -= lo;
        }
    }
    rank
}

/// Phase 3 — dummy nodes. Every edge spanning more than one rank is split
/// into unit-length segments with one dummy per rank it skips, so the
/// ordering and placement phases below only ever see neighbouring ranks.
///
/// Returns the layers (virtual node ids, reals first then dummies), the unit
/// segments between them, each original edge's dummy chain in the *caller's*
/// direction, and every virtual node's rank.
type Layered = (Vec<Vec<usize>>, Vec<(usize, usize)>, Vec<(usize, Vec<usize>)>, Vec<usize>);
fn insert_dummies(n: usize, edges: &[LayoutEdge], ranks: &[u32]) -> Layered {
    let depth = ranks.iter().copied().max().unwrap_or(0) as usize + 1;
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); depth];
    let mut rank_of: Vec<usize> = ranks.iter().map(|&r| r as usize).collect();
    let mut segs = Vec::new();
    let mut chains = Vec::new();
    let mut next = n;
    for e in edges {
        let (r0, r1) = (ranks[e.from] as usize, ranks[e.to] as usize);
        if r1 <= r0 + 1 {
            segs.push((e.from, e.to));
            continue;
        }
        let mut chain = Vec::with_capacity(r1 - r0 - 1);
        let mut prev = e.from;
        for (r, layer) in layers.iter_mut().enumerate().take(r1).skip(r0 + 1) {
            let d = next;
            next += 1;
            layer.push(d);
            rank_of.push(r);
            chain.push(d);
            segs.push((prev, d));
            prev = d;
        }
        segs.push((prev, e.to));
        // The chain runs source→target in *layout* order; a reversed edge
        // is drawn the other way round, so its waypoints are too.
        if e.reversed {
            chain.reverse();
        }
        chains.push((e.orig, chain));
    }
    // Real nodes join their layers after the dummies exist, so the initial
    // DFS order below sees one consistent node set.
    let mut ordered: Vec<Vec<usize>> = vec![Vec::new(); depth];
    for (r, layer) in ordered.iter_mut().enumerate() {
        layer.extend((0..n).filter(|&v| ranks[v] as usize == r));
        layer.extend(layers[r].iter().copied());
    }
    (ordered, segs, chains, rank_of)
}

/// Phase 4 — crossing reduction. Start from depth-first discovery order,
/// then sweep barycenters down and up, keeping the best ordering seen. Ties
/// break by current position and then by index, so the result is a pure
/// function of the input.
fn reduce_crossings(layers: Vec<Vec<usize>>, segs: &[(usize, usize)]) -> Vec<Vec<usize>> {
    let total: usize = layers.iter().map(Vec::len).sum();
    if total == 0 {
        return layers;
    }
    let n_virtual = layers.iter().flatten().copied().max().unwrap_or(0) + 1;
    let mut up: Vec<Vec<usize>> = vec![Vec::new(); n_virtual];
    let mut down: Vec<Vec<usize>> = vec![Vec::new(); n_virtual];
    for &(a, b) in segs {
        down[a].push(b);
        up[b].push(a);
    }

    let mut order = dfs_order(&layers, &down);
    let mut best = order.clone();
    let mut best_cross = count_crossings(&best, segs, n_virtual);
    for _ in 0..SWEEPS {
        for descending in [true, false] {
            barycenter_sweep(&mut order, if descending { &up } else { &down }, descending);
            let c = count_crossings(&order, segs, n_virtual);
            if c < best_cross {
                best_cross = c;
                best = order.clone();
            }
        }
        if best_cross == 0 {
            break;
        }
    }
    best
}

/// Seed the ordering by depth-first discovery from the first rank: nodes
/// that arrive together in the traversal start beside each other, which is a
/// far better opening position than the caller's index order.
fn dfs_order(layers: &[Vec<usize>], down: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let n_virtual = down.len();
    let mut rank_of = vec![usize::MAX; n_virtual];
    for (r, layer) in layers.iter().enumerate() {
        for &v in layer {
            rank_of[v] = r;
        }
    }
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); layers.len()];
    let mut seen = vec![false; n_virtual];
    // Roots in each layer's existing order, so a disconnected node still
    // lands somewhere predictable.
    for layer in layers {
        for &root in layer {
            if seen[root] {
                continue;
            }
            let mut stack = vec![root];
            seen[root] = true;
            while let Some(v) = stack.pop() {
                out[rank_of[v]].push(v);
                // Reversed, so the first child is walked first.
                for &w in down[v].iter().rev() {
                    if !seen[w] {
                        seen[w] = true;
                        stack.push(w);
                    }
                }
            }
        }
    }
    out
}

/// One barycenter pass: each node moves to the average position of its
/// neighbours in the layer just visited, and each layer is re-sorted by that
/// value. A node with no neighbours there keeps its place.
fn barycenter_sweep(layers: &mut [Vec<usize>], nbrs: &[Vec<usize>], descending: bool) {
    let n_virtual = nbrs.len();
    let mut pos = vec![0f32; n_virtual];
    let apply = |layer: &mut Vec<usize>, pos: &[f32]| {
        let keyed: Vec<(f32, usize, usize)> = layer
            .iter()
            .enumerate()
            .map(|(i, &v)| {
                let ns = &nbrs[v];
                let b = if ns.is_empty() {
                    i as f32
                } else {
                    ns.iter().map(|&u| pos[u]).sum::<f32>() / ns.len() as f32
                };
                (b, i, v)
            })
            .collect();
        let mut keyed = keyed;
        // Ties fall back to the current position and then to the index —
        // both total, so `sort_by` never has to compare two equal keys.
        keyed.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        *layer = keyed.into_iter().map(|(_, _, v)| v).collect();
    };
    let idx: Vec<usize> =
        if descending { (0..layers.len()).collect() } else { (0..layers.len()).rev().collect() };
    for (step, &r) in idx.iter().enumerate() {
        if step > 0 {
            apply(&mut layers[r], &pos);
        }
        for (i, &v) in layers[r].iter().enumerate() {
            pos[v] = i as f32;
        }
    }
}

/// Total edge crossings across every pair of adjacent layers — the number
/// the sweeps are trying to reduce, and the tie-breaker that decides which
/// ordering to keep.
fn count_crossings(layers: &[Vec<usize>], segs: &[(usize, usize)], n_virtual: usize) -> usize {
    let mut pos = vec![usize::MAX; n_virtual];
    let mut rank_of = vec![usize::MAX; n_virtual];
    for (r, layer) in layers.iter().enumerate() {
        for (i, &v) in layer.iter().enumerate() {
            pos[v] = i;
            rank_of[v] = r;
        }
    }
    let mut by_gap: Vec<Vec<(usize, usize)>> = vec![Vec::new(); layers.len().max(1)];
    for &(a, b) in segs {
        if rank_of[a] < by_gap.len() {
            by_gap[rank_of[a]].push((pos[a], pos[b]));
        }
    }
    let mut total = 0;
    for gap in &mut by_gap {
        gap.sort_unstable();
        // Two segments cross exactly when their lower endpoints are ordered
        // the other way round from their upper ones: an inversion count.
        for i in 0..gap.len() {
            for j in (i + 1)..gap.len() {
                if gap[i].1 > gap[j].1 {
                    total += 1;
                }
            }
        }
    }
    total
}

/// Phase 5 — coordinate assignment. Start each rank at unit spacing centred
/// on zero, then run priority passes that pull every node toward the median
/// of its neighbours in the rank just placed. A node may push its
/// lower-priority siblings aside but never crosses one of equal or higher
/// priority, which is what keeps ranks non-overlapping without a separate
/// constraint solve.
fn assign_x(layers: &[Vec<usize>], segs: &[(usize, usize)], n_real: usize) -> Vec<f32> {
    let n_virtual = layers.iter().flatten().copied().max().map_or(0, |m| m + 1);
    let mut x = vec![0f32; n_virtual];
    for layer in layers {
        let mid = (layer.len() as f32 - 1.0) * 0.5;
        for (i, &v) in layer.iter().enumerate() {
            x[v] = i as f32 - mid;
        }
    }
    let mut up: Vec<Vec<usize>> = vec![Vec::new(); n_virtual];
    let mut down: Vec<Vec<usize>> = vec![Vec::new(); n_virtual];
    for &(a, b) in segs {
        down[a].push(b);
        up[b].push(a);
    }
    // A dummy is a piece of one long edge, so keeping it in line is worth
    // more than any real node's preference — a bent wire reads as two edges.
    // Ids at or above `n_real` are dummies (see `insert_dummies`). Below
    // that, the busiest node wins, because it is the one with most edges to
    // straighten.
    let prio = |v: usize| -> u32 {
        let deg = (up[v].len() + down[v].len()) as u32;
        if v >= n_real {
            u32::MAX / 2 + deg
        } else {
            deg
        }
    };
    for _ in 0..2 {
        for descending in [true, false] {
            let idx: Vec<usize> = if descending {
                (1..layers.len()).collect()
            } else {
                (0..layers.len().saturating_sub(1)).rev().collect()
            };
            for &r in &idx {
                let nbrs = if descending { &up } else { &down };
                let targets: Vec<Option<f32>> =
                    layers[r].iter().map(|&v| median_of(&nbrs[v], &x)).collect();
                priority_pass(&layers[r], &mut x, &prio, &targets);
            }
        }
    }
    x
}

/// The median x of a node's neighbours — the position that leaves its edges
/// as balanced as they can be. An even count averages the two middles, which
/// is what centres a join between two feeders.
fn median_of(nbrs: &[usize], x: &[f32]) -> Option<f32> {
    if nbrs.is_empty() {
        return None;
    }
    let mut v: Vec<f32> = nbrs.iter().map(|&u| x[u]).collect();
    v.sort_by(f32::total_cmp);
    let m = v.len() / 2;
    Some(if v.len() % 2 == 1 { v[m] } else { (v[m - 1] + v[m]) * 0.5 })
}

/// Move each node of one rank toward its target, highest priority first,
/// shifting only strictly lower-priority neighbours out of the way and
/// stopping at the first one that outranks it. Nodes keep a gap of at least
/// one unit, so the rank stays ordered and non-overlapping.
fn priority_pass(
    layer: &[usize],
    x: &mut [f32],
    prio: &impl Fn(usize) -> u32,
    targets: &[Option<f32>],
) {
    let mut order: Vec<usize> = (0..layer.len()).collect();
    order.sort_by_key(|&i| (std::cmp::Reverse(prio(layer[i])), layer[i]));
    for &i in &order {
        let Some(target) = targets[i] else { continue };
        let v = layer[i];
        let p = prio(v);
        if target > x[v] {
            // How far right can `v` go before it would crowd someone who
            // outranks it? Each lower-priority node in between needs a unit.
            let mut limit = f32::INFINITY;
            let mut k = i + 1;
            let mut gap = 1.0f32;
            while k < layer.len() {
                if prio(layer[k]) >= p {
                    limit = x[layer[k]] - gap;
                    break;
                }
                k += 1;
                gap += 1.0;
            }
            let nx = target.min(limit);
            if nx <= x[v] {
                continue;
            }
            x[v] = nx;
            let mut prev = nx;
            for &u in &layer[i + 1..k.min(layer.len())] {
                x[u] = x[u].max(prev + 1.0);
                prev = x[u];
            }
        } else if target < x[v] {
            let mut limit = f32::NEG_INFINITY;
            let mut k = i;
            let mut gap = 1.0f32;
            while k > 0 {
                if prio(layer[k - 1]) >= p {
                    limit = x[layer[k - 1]] + gap;
                    break;
                }
                k -= 1;
                gap += 1.0;
            }
            let nx = target.max(limit);
            if nx >= x[v] {
                continue;
            }
            x[v] = nx;
            let mut prev = nx;
            for j in (k..i).rev() {
                let u = layer[j];
                x[u] = x[u].min(prev - 1.0);
                prev = x[u];
            }
        }
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

    // --- LayeredLayout ---

    /// The nine-task pipeline the CLI example draws: two feeds, a join, a
    /// fan-out to two consumers, a merge, and one long edge that skips two
    /// ranks (clean_prices -> backtest).
    fn pipeline() -> (usize, Vec<(u32, u32)>) {
        (9, vec![(0, 2), (1, 3), (2, 4), (3, 4), (4, 5), (5, 6), (5, 7), (6, 8), (7, 8), (2, 7)])
    }

    #[test]
    fn ranks_respect_edge_direction() {
        let (n, edges) = pipeline();
        let l = LayeredLayout::new(n, &edges, RankDir::TB);
        let r = l.ranks();
        for &(a, b) in &edges {
            assert!(
                r[a as usize] < r[b as usize],
                "edge {a}->{b} runs backwards: rank {} -> {}",
                r[a as usize],
                r[b as usize]
            );
        }
        assert_eq!(r.iter().copied().min(), Some(0), "the first rank must not be empty");
    }

    #[test]
    fn sources_come_out_on_top_and_lr_transposes_tb() {
        let (n, edges) = pipeline();
        let tb = LayeredLayout::new(n, &edges, RankDir::TB);
        let lr = LayeredLayout::new(n, &edges, RankDir::LR);
        let (rp, rl) = (tb.positions(), lr.positions());
        // Data y is up, so a source must sit *above* everything it feeds.
        for &(a, b) in &edges {
            assert!(rp[a as usize][1] > rp[b as usize][1], "edge {a}->{b} points upwards in TB");
            assert!(rl[a as usize][0] < rl[b as usize][0], "edge {a}->{b} points leftwards in LR");
        }
        // LR is TB turned a quarter turn: rank moves from y to x, column
        // from x to y, and the ranks themselves are identical.
        assert_eq!(tb.ranks(), lr.ranks());
        for i in 0..n {
            assert_eq!(rl[i][0], -rp[i][1], "rank axis");
            assert_eq!(rl[i][1], -rp[i][0], "column axis");
        }
    }

    #[test]
    fn long_edges_get_one_waypoint_per_skipped_rank() {
        let (n, edges) = pipeline();
        let l = LayeredLayout::new(n, &edges, RankDir::TB);
        let (pts, starts) = l.routes();
        assert_eq!(starts.len(), edges.len(), "one CSR run per edge, always");
        let run = |e: usize| {
            let a = starts[e] as usize;
            let b = starts.get(e + 1).map_or(pts.len(), |v| *v as usize);
            &pts[a..b]
        };
        let r = l.ranks();
        for (e, &(a, b)) in edges.iter().enumerate() {
            let span = r[b as usize] - r[a as usize];
            assert_eq!(
                run(e).len(),
                span.saturating_sub(1) as usize,
                "edge {a}->{b} spans {span} ranks"
            );
        }
        // And the long one really is long: clean_prices -> backtest is the
        // last edge and skips at least one rank.
        assert!(!run(edges.len() - 1).is_empty(), "the skipping edge must be routed");
    }

    #[test]
    fn waypoints_run_in_the_callers_edge_direction() {
        // A three-rank chain plus a skip edge: the skip's single waypoint
        // sits on the middle rank either way, but a *reversed* skip must
        // still emit its chain from the caller's source to its target.
        let edges = vec![(0u32, 1), (1, 2), (0, 2)];
        let l = LayeredLayout::new(3, &edges, RankDir::TB);
        let (pts, starts) = l.routes();
        let skip = &pts[starts[2] as usize..];
        assert_eq!(skip.len(), 1);
        let p = l.positions();
        assert!(
            skip[0][1] < p[0][1] && skip[0][1] > p[2][1],
            "the waypoint must sit between the ranks it bridges"
        );
    }

    #[test]
    fn barycenter_sweeps_reduce_crossings_on_a_known_graph() {
        // A 2x2 bipartite graph wired across itself: 0->3 and 1->2. Laid out
        // in index order the two edges cross; one sweep must untangle it.
        let edges = vec![(0u32, 3), (1, 2)];
        let l = LayeredLayout::new(4, &edges, RankDir::TB);
        let p = l.positions();
        // No crossing means the two edges are ordered the same way at both
        // ends: whichever source is left has the left target.
        let src_left_is_0 = p[0][0] < p[1][0];
        let tgt_left_is_3 = p[3][0] < p[2][0];
        assert_eq!(src_left_is_0, tgt_left_is_3, "the crossing survived: {p:?}");
    }

    #[test]
    fn cycles_do_not_hang_and_reverse_a_back_edge() {
        // A 3-cycle: one edge has to be reversed for the layout, and every
        // node still gets a rank and a position.
        let edges = vec![(0u32, 1), (1, 2), (2, 0)];
        let l = LayeredLayout::new(3, &edges, RankDir::TB);
        assert_eq!(l.positions().len(), 3);
        assert_eq!(l.ranks(), &[0, 1, 2], "the back edge 2->0 is the one reversed");
        // A self-loop and an out-of-range endpoint are inert, not fatal.
        let odd = LayeredLayout::new(2, &[(0, 0), (0, 9), (0, 1)], RankDir::TB);
        assert_eq!(odd.ranks(), &[0, 1]);
        assert_eq!(odd.routes().1.len(), 3, "every edge keeps a CSR run");
    }

    #[test]
    fn layout_is_deterministic() {
        let (n, edges) = pipeline();
        for dir in [RankDir::TB, RankDir::LR] {
            let a = LayeredLayout::new(n, &edges, dir);
            let b = LayeredLayout::new(n, &edges, dir);
            assert_eq!(a.positions(), b.positions());
            assert_eq!(a.ranks(), b.ranks());
            assert_eq!(a.routes(), b.routes());
        }
    }

    #[test]
    fn an_empty_or_edgeless_graph_still_lays_out() {
        let empty = LayeredLayout::new(0, &[], RankDir::TB);
        assert!(empty.positions().is_empty());
        assert!(empty.routes().0.is_empty());
        let loose = LayeredLayout::new(3, &[], RankDir::TB);
        assert_eq!(loose.ranks(), &[0, 0, 0], "with no edges everything is a source");
        let xs: Vec<f32> = loose.positions().iter().map(|p| p[0]).collect();
        let mut sorted = xs.clone();
        sorted.sort_by(f32::total_cmp);
        sorted.dedup();
        assert_eq!(sorted.len(), 3, "loose nodes must not stack on one another");
    }

    #[test]
    fn ranking_pulls_a_lone_source_down_to_its_consumers() {
        // `far` feeds only the deepest task; longest-path alone would strand
        // it at rank 0 with a wire past two empty ranks beside it.
        //          0 -> 1 -> 2 -> 3,  far(4) -> 3
        let edges = vec![(0u32, 1), (1, 2), (2, 3), (4, 3)];
        let l = LayeredLayout::new(5, &edges, RankDir::TB);
        assert_eq!(l.ranks()[4], 2, "the lone source sits just above what it feeds");
        assert_eq!(l.ranks()[3], 3);
    }

    #[test]
    fn reachable_follows_direction() {
        let (n, edges) = pipeline();
        // Downstream from a feed reaches everything it eventually triggers.
        let down = reachable(n, &edges, 0, Direction::Downstream);
        assert!(down[0] && down[2] && down[4] && down[8], "the closure must reach the sink");
        assert!(!down[1] && !down[3], "the other feed's branch is not downstream");
        // Upstream from the sink reaches every task it waits on.
        let up = reachable(n, &edges, 8, Direction::Upstream);
        assert_eq!(up.iter().filter(|&&x| x).count(), n, "everything feeds the publish step");
        // A source has nothing upstream but itself, and an out-of-range
        // start reaches nothing at all.
        assert_eq!(reachable(n, &edges, 0, Direction::Upstream).iter().filter(|&&x| x).count(), 1);
        assert!(reachable(n, &edges, 99, Direction::Downstream).iter().all(|&x| !x));
    }

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
