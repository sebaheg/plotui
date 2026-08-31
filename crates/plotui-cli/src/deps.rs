//! `plotui example deps` — plotui's own crate dependency graph, laid out
//! live by the core `ForceLayout` simulation. Workspace crates and their
//! dependencies pull into clusters, late-arriving crates fly in beside the
//! crate that needs them, and hovering a crate lights up everything it
//! transitively depends on.

use std::cell::RefCell;
use std::rc::Rc;

use plotui_core::{ForceLayout, Plot, Rgb, Shape, COLORWAY_PLOTUI};
use plotui_ratatui::{ElementKind, OverlaySpan, PlotEvent, PlotState};
use ratatui::style::{Color, Style};

use crate::examples::{self, Output};
use crate::interactive::{self, Hooks};
use crate::{record, ExampleArgs};

#[derive(Clone, Copy, PartialEq)]
enum Group {
    /// A crate in this workspace.
    Ws,
    /// A direct external dependency of the workspace.
    Ext,
    /// A dependency of a dependency.
    Trans,
}
use Group::{Ext, Trans, Ws};

/// plotui's crate graph, snapshotted from `cargo metadata` (the workspace,
/// its direct dependencies, and their direct dependencies). An edge (a, b)
/// means a depends on b.
const NODES: &[(&str, Group)] = &[
    ("plotui", Ws),
    ("plotui-bind", Ws),
    ("plotui-core", Ws),
    ("plotui-ffi", Ws),
    ("plotui-protocol", Ws),
    ("plotui-py", Ws),
    ("plotui-ratatui", Ws),
    ("plotui-term", Ws),
    ("plotui-wasm", Ws),
    ("base64", Ext),
    ("clap", Ext),
    ("crossterm", Ext),
    ("flate2", Ext),
    ("numpy", Ext),
    ("pyo3", Ext),
    ("ratatui", Ext),
    ("rustix", Ext),
    ("wasm-bindgen", Ext),
    ("bitflags", Trans),
    ("cfg-if", Trans),
    ("clap_builder", Trans),
    ("clap_derive", Trans),
    ("crc32fast", Trans),
    ("crossterm_winapi", Trans),
    ("derive_more", Trans),
    ("document-features", Trans),
    ("errno", Trans),
    ("filedescriptor", Trans),
    ("indoc", Trans),
    ("instability", Trans),
    ("libc", Trans),
    ("linux-raw-sys", Trans),
    ("memoffset", Trans),
    ("miniz_oxide", Trans),
    ("mio", Trans),
    ("ndarray", Trans),
    ("num-complex", Trans),
    ("num-integer", Trans),
    ("num-traits", Trans),
    ("once_cell", Trans),
    ("parking_lot", Trans),
    ("portable-atomic", Trans),
    ("pyo3-ffi", Trans),
    ("pyo3-macros", Trans),
    ("ratatui-core", Trans),
    ("ratatui-crossterm", Trans),
    ("ratatui-macros", Trans),
    ("ratatui-termina", Trans),
    ("ratatui-termwiz", Trans),
    ("ratatui-widgets", Trans),
    ("rustc-hash", Trans),
    ("serde", Trans),
    ("signal-hook", Trans),
    ("signal-hook-mio", Trans),
    ("unindent", Trans),
    ("wasm-bindgen-macro", Trans),
    ("wasm-bindgen-shared", Trans),
    ("winapi", Trans),
    ("windows-sys", Trans),
];

#[rustfmt::skip]
const EDGES: &[(u32, u32)] = &[
    (0, 2), (0, 6), (0, 7), (0, 10), (0, 11), (0, 15), (1, 2), (3, 1), (3, 2),
    (3, 4), (3, 7), (4, 2), (4, 9), (4, 12), (5, 1), (5, 2), (5, 4), (5, 7),
    (5, 13), (5, 14), (6, 2), (6, 4), (6, 7), (6, 11), (7, 2), (7, 4), (7, 16),
    (8, 1), (8, 2), (8, 17), (10, 20), (10, 21), (11, 16), (11, 18), (11, 23),
    (11, 24), (11, 25), (11, 27), (11, 34), (11, 40), (11, 52), (11, 53),
    (11, 57), (12, 22), (12, 33), (13, 14), (13, 30), (13, 35), (13, 36),
    (13, 37), (13, 38), (13, 50), (14, 28), (14, 30), (14, 32), (14, 39),
    (14, 41), (14, 42), (14, 43), (14, 54), (15, 29), (15, 44), (15, 45),
    (15, 46), (15, 47), (15, 48), (15, 49), (15, 51), (16, 18), (16, 26),
    (16, 30), (16, 31), (16, 58), (17, 19), (17, 39), (17, 55), (17, 56),
];

/// How many trailing NODES arrive live instead of at t=0 — the "a new
/// release pulls in a crate" beat. Their edges only reference earlier
/// indices, so insertion order is safe (asserted in a test below).
const FLY_IN: usize = 8;
const SPAWN_EVERY_MS: f64 = 2500.0;
const SEED: u32 = 20260830;
const TICK_MS: f64 = 33.0;
/// Mean-displacement threshold below which the sim stops re-rendering.
const SETTLED: f32 = 1e-3;

fn style(g: Group) -> (Rgb, f32, Shape) {
    match g {
        Ws => (COLORWAY_PLOTUI[0], 5.0, Shape::Disc),
        Ext => (COLORWAY_PLOTUI[1], 4.0, Shape::Ring),
        Trans => (COLORWAY_PLOTUI[2], 2.6, Shape::Dot),
    }
}

fn dim(c: Rgb) -> Rgb {
    [c[0] / 4, c[1] / 4, c[2] / 4]
}

/// The live scene state shared by the feed and hover closures.
struct Scene {
    layout: ForceLayout,
    handle: usize,
    /// Node count currently in the trace (grows as crates fly in).
    n: usize,
    /// Edges currently in the trace, in trace order.
    edges: Vec<(u32, u32)>,
    /// Full-brightness color per current node.
    base: Vec<Rgb>,
    energy: f32,
}

impl Scene {
    /// Initial plot + layout: everything except the FLY_IN tail.
    fn build() -> (Plot, Scene) {
        let n0 = NODES.len() - FLY_IN;
        let edges0: Vec<(u32, u32)> = EDGES
            .iter()
            .copied()
            .filter(|&(a, b)| (a as usize) < n0 && (b as usize) < n0)
            .collect();
        let mut layout = ForceLayout::new(n0, &edges0, SEED);
        // Warm up past the initial explosion, then pin the frame with room
        // to grow so the camera never "breathes" while the sim runs.
        for _ in 0..30 {
            layout.step();
        }
        let mut plot = Plot::new();
        plot.show_box = false;
        plot.bounds_override = Some(([-1.25; 3], [1.25; 3]));
        let base: Vec<Rgb> = (0..n0).map(|i| style(NODES[i].1).0).collect();
        let sizes: Vec<f32> = (0..n0).map(|i| style(NODES[i].1).1).collect();
        let shapes: Vec<Shape> = (0..n0).map(|i| style(NODES[i].1).2).collect();
        let handle = plot.add_graph3d(
            layout.positions().to_vec(),
            base.clone(),
            edges0.clone(),
            2.6, // fallback size: fly-in nodes are all transitive
            Some(sizes),
            None,
            Some(shapes),
            None,
        );
        let scene = Scene { layout, handle, n: n0, edges: edges0, base, energy: f32::INFINITY };
        (plot, scene)
    }

    fn step(&mut self, plot: &mut Plot) {
        self.energy = self.layout.step();
        plot.set_graph_positions(self.handle, self.layout.positions().to_vec())
            .expect("graph handle");
    }

    /// Bring the next queued crate in: spawn it in the layout beside the
    /// crate that needs it, append it to the trace, and note its edges.
    fn spawn_next(&mut self, plot: &mut Plot) -> Option<&'static str> {
        if self.n >= NODES.len() {
            return None;
        }
        let idx = self.n as u32;
        let new_edges: Vec<(u32, u32)> =
            EDGES.iter().copied().filter(|&(a, b)| a == idx || b == idx).collect();
        let neighbors: Vec<u32> =
            new_edges.iter().map(|&(a, b)| if a == idx { b } else { a }).collect();
        self.layout.add_node(&neighbors);
        let (color, _, _) = style(NODES[self.n].1);
        let pos = *self.layout.positions().last().expect("just added");
        plot.extend_graph(self.handle, &[pos], &[color], &new_edges).expect("graph handle");
        self.base.push(color);
        self.edges.extend_from_slice(&new_edges);
        self.n += 1;
        self.energy = f32::INFINITY; // re-heated: keep rendering
        Some(NODES[self.n - 1].0)
    }

    /// The transitive-dependency closure of node `i` over the current edges.
    fn reachable(&self, i: usize) -> Vec<bool> {
        let mut seen = vec![false; self.n];
        let mut stack = vec![i];
        seen[i] = true;
        while let Some(a) = stack.pop() {
            for &(x, y) in &self.edges {
                if x as usize == a && !seen[y as usize] {
                    seen[y as usize] = true;
                    stack.push(y as usize);
                }
            }
        }
        seen
    }

    /// Recolor for a hover: the hovered crate and everything it depends on
    /// at full color, the rest dimmed; edges on the path stay lit.
    fn highlight(&self, plot: &mut Plot, i: usize) {
        let on = self.reachable(i);
        let nodes: Vec<Rgb> =
            (0..self.n).map(|j| if on[j] { self.base[j] } else { dim(self.base[j]) }).collect();
        let edges: Vec<Rgb> = self
            .edges
            .iter()
            .map(
                |&(a, b)| {
                    if on[a as usize] && on[b as usize] {
                        [170, 175, 185]
                    } else {
                        [34, 38, 48]
                    }
                },
            )
            .collect();
        plot.set_graph_colors(self.handle, nodes, Some(edges)).expect("graph handle");
    }

    fn restore(&self, plot: &mut Plot) {
        plot.set_graph_colors(self.handle, self.base.clone(), None).expect("graph handle");
    }
}

pub fn run(args: &ExampleArgs, out: Output) -> std::io::Result<()> {
    let (mut plot, mut scene) = Scene::build();

    if out.is_still() {
        // One frame: the fully-arrived, settled graph.
        while scene.spawn_next(&mut plot).is_some() {}
        for _ in 0..1500 {
            if scene.energy < SETTLED {
                break;
            }
            scene.step(&mut plot);
        }
        return examples::emit(&plot, args, &out);
    }

    let scene = Rc::new(RefCell::new(scene));

    let feed_scene = Rc::clone(&scene);
    let mut acc = 0.0f64;
    let mut spawn_acc = 0.0f64;
    let feed = Box::new(move |state: &mut PlotState, dt_ms: f64| {
        let mut s = feed_scene.borrow_mut();
        acc += dt_ms;
        spawn_acc += dt_ms;
        let mut ticked = false;
        while acc >= TICK_MS {
            acc -= TICK_MS;
            if s.energy >= SETTLED {
                s.step(state.plot_mut());
                ticked = true;
            }
        }
        if spawn_acc >= SPAWN_EVERY_MS {
            spawn_acc = 0.0;
            if let Some(name) = s.spawn_next(state.plot_mut()) {
                state.set_overlay(vec![OverlaySpan {
                    row: 0,
                    col: 2,
                    text: format!(" + {name} "),
                    style: Style::default().fg(Color::Rgb(69, 200, 209)),
                }]);
                ticked = true;
            }
        }
        if ticked {
            state.invalidate();
        }
    });

    let hover_scene = Rc::clone(&scene);
    let on_plot_event = Box::new(move |state: &mut PlotState, ev: PlotEvent| {
        let s = hover_scene.borrow();
        match ev {
            PlotEvent::ElementHovered(Some((ElementKind::Node, i))) if i < s.n => {
                s.highlight(state.plot_mut(), i);
                let deps = s.reachable(i).iter().filter(|&&x| x).count() - 1;
                state.set_overlay(vec![OverlaySpan {
                    row: 0,
                    col: 2,
                    text: format!(" {} · {deps} deps ", NODES[i].0),
                    style: Style::default().fg(Color::Rgb(205, 210, 220)),
                }]);
            }
            PlotEvent::ElementHovered(_) => {
                s.restore(state.plot_mut());
                state.set_overlay(Vec::new());
            }
            _ => {}
        }
    });

    match out {
        Output::Record(opts) => {
            let hooks = Hooks { feed: Some(feed), ..Default::default() };
            record::record(plot, hooks, &opts)
        }
        Output::Interactive(mode) => {
            let hooks = Hooks {
                pickable: true,
                feed: Some(feed),
                on_plot_event: Some(on_plot_event),
                ..Default::default()
            };
            interactive::run_with(plot, mode, args.width, args.height, hooks)
        }
        Output::Static(_) => unreachable!("still outputs handled above"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The FLY_IN tail must only reference earlier indices, or insertion
    /// order would create edges into nodes that don't exist yet.
    #[test]
    fn fly_in_edges_reference_only_earlier_nodes() {
        let n0 = NODES.len() - FLY_IN;
        for &(a, b) in EDGES {
            let (a, b) = (a as usize, b as usize);
            assert!(a < NODES.len() && b < NODES.len(), "edge ({a}, {b}) out of range");
        }
        // For every queued node, every counterparty precedes it.
        for idx in n0..NODES.len() {
            for &(a, b) in EDGES.iter().filter(|&&(a, b)| a as usize == idx || b as usize == idx) {
                let other = if a as usize == idx { b as usize } else { a as usize };
                assert!(other < idx, "node {idx} has an edge to a later node {other}");
            }
        }
    }

    /// Hovering plotui-core (index 2) must light nothing but itself — it is
    /// the root everyone else depends on; hovering plotui (0) lights many.
    #[test]
    fn reachability_follows_dependency_direction() {
        let (mut plot, mut scene) = Scene::build();
        while scene.spawn_next(&mut plot).is_some() {}
        let core = scene.reachable(2);
        assert_eq!(core.iter().filter(|&&x| x).count(), 1, "plotui-core depends on nothing");
        let plotui = scene.reachable(0);
        assert!(plotui.iter().filter(|&&x| x).count() > 10, "plotui pulls in a real closure");
    }
}
