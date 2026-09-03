//! `plotui example pipeline` — a nightly forecasting DAG running in front of
//! you. Nine tasks laid out by rank, colours advancing pending → running →
//! done in topological order, one task failing and stranding everything
//! downstream of it, then the whole run starting over.
//!
//! It is the example because it is the thing the feature is *for*: a
//! pipeline's shape is static, its state is not, and watching the state move
//! through the shape is the whole reason to draw it in a terminal instead of
//! reading a table of task statuses.

use std::cell::RefCell;
use std::rc::Rc;

use plotui_core::{
    reachable, Direction, LayeredLayout, NodeShape, Plot, RankDir, Rgb, TraceId, COLORWAY_PLOTUI,
};
use plotui_ratatui::{ElementKind, OverlaySpan, PlotEvent, PlotState};
use ratatui::style::{Color, Style};

use crate::examples::{self, Output};
use crate::interactive::{self, Hooks};
use crate::{record, ExampleArgs};

/// The tasks, in declaration order — which is also index order.
const TASKS: [&str; 9] = [
    "fetch_prices",
    "fetch_weather",
    "clean_prices",
    "clean_weather",
    "join_frames",
    "build_features",
    "train_model",
    "backtest",
    "publish",
];

/// `(from, to)`: `to` waits on `from`. The last edge skips two ranks, so the
/// layout has a long edge to route around the tasks in between — which is
/// the case a straight line would draw straight through a node.
const EDGES: [(u32, u32); 10] =
    [(0, 2), (1, 3), (2, 4), (3, 4), (4, 5), (5, 6), (5, 7), (6, 8), (7, 8), (2, 7)];

/// Which task fails, once per run. `train_model` is the interesting one: it
/// is not a leaf, so its failure strands `publish` while `backtest` beside
/// it still finishes — the picture a table of statuses cannot give you.
const FAILS: usize = 6;

/// How long a task spends running, and the pause before the run restarts.
const STEP_MS: f64 = 1200.0;
const RESTART_MS: f64 = 2600.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Pending,
    Running,
    Done,
    Failed,
}

impl State {
    fn color(self) -> Rgb {
        match self {
            State::Done => COLORWAY_PLOTUI[4],
            State::Running => COLORWAY_PLOTUI[1],
            State::Failed => COLORWAY_PLOTUI[7],
            State::Pending => PENDING,
        }
    }
}

/// Pending is deliberately not a colorway slot: a task that has not started
/// is not a *category*, it is the absence of one, so it reads as chrome.
const PENDING: Rgb = [58, 64, 84];

/// The four state colours as legend rows. A legend row needs a trace, and
/// these are states rather than series, so each gets an empty graph trace:
/// no geometry, one name, one swatch.
const LEGEND: [(&str, State); 4] = [
    ("done", State::Done),
    ("running", State::Running),
    ("failed", State::Failed),
    ("pending", State::Pending),
];

/// The simulated run: which task each tick advances, and what everything is
/// currently doing.
struct Run {
    states: [State; TASKS.len()],
    /// Tasks in the order they will execute — a topological order, so a task
    /// never starts before what it waits on has finished.
    order: Vec<usize>,
    /// How far through `order` we are; past the end the run is over.
    at: usize,
    elapsed: f64,
}

impl Run {
    fn new() -> Self {
        Run { states: [State::Pending; TASKS.len()], order: topological(), at: 0, elapsed: 0.0 }
    }

    fn colors(&self) -> Vec<Rgb> {
        self.states.iter().map(|s| s.color()).collect()
    }

    /// Advance the simulated clock; returns whether anything changed.
    fn tick(&mut self, dt_ms: f64) -> bool {
        self.elapsed += dt_ms;
        // The run is over only once the queue is empty *and* the last task
        // has finished — otherwise the restart would wipe a task that is
        // still on screen with its running colour.
        let over = self.at >= self.order.len() && !self.states.contains(&State::Running);
        let due = if over { RESTART_MS } else { STEP_MS };
        if self.elapsed < due {
            return false;
        }
        self.elapsed -= due;
        if over {
            *self = Run::new();
            return true;
        }
        // Finish whatever was running, then start the next task that can.
        for (i, s) in self.states.iter_mut().enumerate() {
            if *s == State::Running {
                *s = if i == FAILS { State::Failed } else { State::Done };
            }
        }
        while self.at < self.order.len() {
            let next = self.order[self.at];
            self.at += 1;
            // A task whose upstream failed never runs at all; that is what
            // makes the failure legible on the *downstream* nodes.
            let blocked = EDGES
                .iter()
                .any(|&(a, b)| b as usize == next && self.states[a as usize] != State::Done);
            if !blocked {
                self.states[next] = State::Running;
                return true;
            }
        }
        true
    }
}

/// A topological order of the tasks, ties by index — the order a scheduler
/// with one worker would run them in.
fn topological() -> Vec<usize> {
    let n = TASKS.len();
    let mut indeg = vec![0usize; n];
    for &(_, b) in &EDGES {
        indeg[b as usize] += 1;
    }
    let mut out = Vec::with_capacity(n);
    let mut done = vec![false; n];
    while out.len() < n {
        let Some(next) = (0..n).find(|&v| !done[v] && indeg[v] == 0) else { break };
        done[next] = true;
        out.push(next);
        for &(a, b) in &EDGES {
            if a as usize == next {
                indeg[b as usize] -= 1;
            }
        }
    }
    out
}

/// The scene: the laid-out graph plus the run driving its colours.
fn build() -> (Plot, TraceId, Run) {
    let layout = LayeredLayout::new(TASKS.len(), &EDGES, RankDir::TB);
    let (pts, starts) = layout.routes();
    let run = Run::new();
    let mut plot = Plot::new();
    let handle = plot.add_graph2d(
        layout.positions().to_vec(),
        TASKS.iter().map(|t| t.to_string()).collect(),
        run.colors(),
        EDGES.to_vec(),
        true,
        // The sink is the one task with an outward effect, so it gets a
        // shape of its own rather than only a colour.
        Some(
            (0..TASKS.len())
                .map(|i| if i == TASKS.len() - 1 { NodeShape::Ellipse } else { NodeShape::Rounded })
                .collect(),
        ),
        None,
        Some((pts.to_vec(), starts.to_vec())),
        None,
    );
    // Four empty graph traces: no geometry, one legend row each. The states
    // need a key, and a key is what the legend is.
    for (name, state) in LEGEND {
        plot.add_graph2d(
            Vec::new(),
            Vec::new(),
            vec![state.color()],
            Vec::new(),
            true,
            None,
            None,
            None,
            Some(name.to_string()),
        );
    }
    (plot, handle, run)
}

/// Recolour for a hover: the hovered task and everything it waits on stay
/// lit, the rest dims. Same treatment the 3D `deps` scene uses, and the same
/// shared `reachable` behind it.
fn highlight(plot: &mut Plot, handle: TraceId, run: &Run, hovered: Option<usize>) {
    let base = run.colors();
    let (nodes, edges) = match hovered {
        None => (base.clone(), None),
        Some(i) => {
            let on = reachable(TASKS.len(), &EDGES, i, Direction::Upstream);
            let nodes = base
                .iter()
                .enumerate()
                .map(|(j, c)| if on[j] { *c } else { dim(*c) })
                .collect::<Vec<_>>();
            let edges =
                EDGES
                    .iter()
                    .map(|&(a, b)| {
                        if on[a as usize] && on[b as usize] {
                            [170, 175, 185]
                        } else {
                            [40, 45, 60]
                        }
                    })
                    .collect();
            (nodes, Some(edges))
        }
    };
    plot.set_graph_colors(handle, nodes, edges).expect("graph handle");
}

/// Most of the way to the background — off, without leaving a hole.
fn dim(c: Rgb) -> Rgb {
    [
        (c[0] as f32 * 0.32 + 12.0) as u8,
        (c[1] as f32 * 0.32 + 14.0) as u8,
        (c[2] as f32 * 0.32 + 20.0) as u8,
    ]
}

pub fn run(args: &ExampleArgs, out: Output) -> std::io::Result<()> {
    let (mut plot, handle, mut run) = build();

    if out.is_still() {
        // One frame: arrive mid-run, with the failure already visible, so
        // the still says what the animation is about.
        for _ in 0..(FAILS + 2) {
            run.tick(STEP_MS);
        }
        plot.set_graph_colors(handle, run.colors(), None).expect("graph handle");
        return examples::emit(&plot, args, &out);
    }

    let run = Rc::new(RefCell::new(run));
    let feed_run = Rc::clone(&run);
    let feed = Box::new(move |state: &mut PlotState, dt_ms: f64| {
        let mut r = feed_run.borrow_mut();
        if r.tick(dt_ms) {
            // A hover in flight is restored by the next hover event; the feed
            // only ever paints the run's own colours.
            state.plot_mut().set_graph_colors(handle, r.colors(), None).expect("graph handle");
            state.invalidate();
        }
    });

    let hover_run = Rc::clone(&run);
    let on_plot_event = Box::new(move |state: &mut PlotState, ev: PlotEvent| {
        let r = hover_run.borrow();
        match ev {
            PlotEvent::ElementHovered(Some((ElementKind::Node, i))) if i < TASKS.len() => {
                highlight(state.plot_mut(), handle, &r, Some(i));
                let waits = reachable(TASKS.len(), &EDGES, i, Direction::Upstream)
                    .iter()
                    .filter(|&&x| x)
                    .count()
                    - 1;
                state.set_overlay(vec![OverlaySpan {
                    row: 0,
                    col: 2,
                    text: format!(" {} · waits on {waits} ", TASKS[i]),
                    style: Style::default().fg(Color::Rgb(205, 210, 220)),
                }]);
            }
            PlotEvent::ElementHovered(_) => {
                highlight(state.plot_mut(), handle, &r, None);
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

    #[test]
    fn the_scene_is_an_acyclic_graph_over_declared_tasks() {
        for &(a, b) in &EDGES {
            assert!(
                (a as usize) < TASKS.len() && (b as usize) < TASKS.len(),
                "edge ({a}, {b}) names a task that does not exist"
            );
            assert_ne!(a, b, "a task cannot wait on itself");
        }
        // A full topological order exists exactly when there is no cycle.
        assert_eq!(topological().len(), TASKS.len(), "the pipeline has a cycle");
        // And every task is reachable from the sink, or the picture would
        // have a task in it that nothing is waiting for.
        let up = reachable(TASKS.len(), &EDGES, TASKS.len() - 1, Direction::Upstream);
        assert!(up.iter().all(|&x| x), "a task feeds nothing: {up:?}");
    }

    #[test]
    fn a_run_advances_in_order_and_strands_what_follows_a_failure() {
        let mut run = Run::new();
        // One tick starts a task and finishes the one before it, so the run
        // plays out in one tick per task plus one to finish the last.
        for _ in 0..=TASKS.len() {
            run.tick(STEP_MS);
        }
        assert_eq!(run.states[FAILS], State::Failed, "train_model must fail");
        assert_eq!(run.states[8], State::Pending, "publish waits on it forever");
        assert_eq!(run.states[7], State::Done, "backtest does not, and finishes");
        assert!(!run.states.contains(&State::Running), "the run has played out");
        // And it restarts rather than sitting there.
        run.tick(RESTART_MS);
        assert!(
            run.states.iter().all(|&s| s == State::Pending || s == State::Running),
            "the restart must clear the board: {:?}",
            run.states
        );
    }

    #[test]
    fn the_layout_routes_the_edge_that_skips_a_rank() {
        let (plot, handle, _) = build();
        let plotui_core::Trace::Graph2d { route_starts, route_pts, .. } = &plot.traces[handle]
        else {
            panic!("expected a graph2d trace");
        };
        assert_eq!(route_starts.len(), EDGES.len(), "one CSR run per edge");
        assert!(!route_pts.is_empty(), "clean_prices -> backtest must be routed around a rank");
    }
}
