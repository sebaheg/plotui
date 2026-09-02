//! Built-in example scenes: `plotui example <name>` runs a self-contained
//! demo with no input data — the same scenes the website shows.

use std::process::ExitCode;

use plotui_core::{Plot, Rgb, TraceId, YAxis};
use plotui_ratatui::PlotState;
use plotui_term::RenderMode;

use crate::interactive::{self, Hooks};
use crate::record::{self, RecordOpts};
use crate::{render, ExampleArgs};

const EXAMPLES: &[(&str, &str)] = &[
    ("scatter", "Rotating 3D scatter — three clusters (the website hero)"),
    ("graph", "Interactive 3D graph — hover nodes and edges, click to inspect"),
    ("stream", "Live streaming plot — three series appended at 20 Hz"),
    ("timeseries", "A year of daily data with a range slider — drag it to zoom the x axis"),
    ("deps", "plotui's own dependency graph, laid out live by a force simulation"),
    ("lidar", "Streaming LiDAR sweep — a scanned room arriving beam by beam, height-colored"),
    ("aizawa", "The Aizawa attractor drawing itself — an RK4 trajectory, colored by speed"),
    ("protein", "A protein cartoon folding itself in — ubiquitin, ribbons from its own PDB file"),
    (
        "mandelbulb",
        "The 3D cousin of the Mandelbrot set — a marching-cubes mesh, revealed slice by slice",
    ),
];

/// Where an example's frames go: the terminal (live or one frame of Kitty
/// escapes) or a file via the headless recorder.
pub enum Output {
    Interactive(RenderMode),
    Static(RenderMode),
    Record(RecordOpts),
}

impl Output {
    /// A single-frame destination: `--static`, a pipe, or `--out *.png`.
    /// The scene should arrive fully played out, not empty.
    pub fn is_still(&self) -> bool {
        match self {
            Output::Interactive(_) => false,
            Output::Static(_) => true,
            Output::Record(opts) => opts.is_still(),
        }
    }
}

/// Render a finished (non-animating) plot to whatever `out` selects.
pub fn emit(plot: &Plot, args: &ExampleArgs, out: &Output) -> std::io::Result<()> {
    match out {
        Output::Interactive(_) => unreachable!("emit is for still outputs"),
        Output::Static(mode) => render::render_static(plot, *mode, args.width, args.height),
        Output::Record(opts) => record::record_static(plot, &opts.path, opts.width, opts.height),
    }
}

pub fn run(args: &ExampleArgs) -> ExitCode {
    let name = match args.name.as_deref() {
        Some(n) if !args.list => n,
        _ => return list(),
    };
    if !EXAMPLES.iter().any(|(n, _)| *n == name) {
        eprintln!("plotui: unknown example '{name}' (run `plotui example` to list them)");
        return ExitCode::from(2);
    }

    // File export never needs (or probes) a graphics-capable terminal.
    let output = if let Some(path) = &args.out {
        Output::Record(RecordOpts {
            path: path.clone(),
            width: args.size.0.into(),
            height: args.size.1.into(),
            fps: args.fps,
            frames: args.frames,
        })
    } else {
        let mode = plotui_term::detect_render_mode();
        if mode == RenderMode::Unsupported {
            for line in plotui_term::policy::UNSUPPORTED_MESSAGE {
                eprintln!("{line}");
            }
            return ExitCode::from(3);
        }
        if !args.static_mode && std::io::IsTerminal::is_terminal(&std::io::stdout()) {
            Output::Interactive(mode)
        } else {
            Output::Static(mode)
        }
    };

    let result = match name {
        "scatter" => run_spinning(build_scatter(), args, output),
        "graph" => run_spinning(build_graph(), args, output),
        "stream" => run_stream(args, output),
        "timeseries" => {
            let plot = build_timeseries();
            match output {
                Output::Interactive(mode) => {
                    interactive::run_with(plot, mode, args.width, args.height, Hooks::default())
                }
                // Nothing animates here, so recording means one .png frame
                // (record_static refuses video extensions with a hint).
                out => emit(&plot, args, &out),
            }
        }
        "deps" => crate::deps::run(args, output),
        "lidar" => crate::lidar::run(args, output),
        "aizawa" => crate::aizawa::run(args, output),
        "protein" => crate::protein::run(args, output),
        "mandelbulb" => crate::mandelbulb::run(args, output),
        _ => unreachable!("validated above"),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("plotui: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The auto-rotating pickable scenes (scatter, graph): the plot itself is
/// static data; only the camera moves.
fn run_spinning(plot: Plot, args: &ExampleArgs, out: Output) -> std::io::Result<()> {
    let hooks = Hooks { pickable: true, auto_rotate: true, ..Default::default() };
    match out {
        Output::Interactive(mode) => {
            interactive::run_with(plot, mode, args.width, args.height, hooks)
        }
        Output::Record(opts) if !opts.is_still() => record::record(plot, hooks, &opts),
        out => emit(&plot, args, &out),
    }
}

fn list() -> ExitCode {
    println!("Built-in examples (no input data needed):\n");
    for (name, desc) in EXAMPLES {
        println!("  {name:<12} {desc}");
    }
    println!("\nRun one with `plotui example <name>`.");
    ExitCode::SUCCESS
}

/// mulberry32, the site's PRNG — same seeds, same streams, so the terminal
/// scenes are point-for-point the ones on plotui.xyz.
pub(crate) struct Rng(pub(crate) u32);

impl Rng {
    pub(crate) fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x6D2B79F5);
        let a = self.0;
        let mut t = (a ^ (a >> 15)).wrapping_mul(a | 1);
        t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61)) ^ t;
        ((t ^ (t >> 14)) as f64 / 4294967296.0) as f32
    }

    /// Sum-of-uniforms gaussian-ish noise in roughly [-1, 1].
    pub(crate) fn gauss(&mut self) -> f32 {
        (self.next() + self.next() + self.next() + self.next() - 2.0) / 2.0
    }
}

/// A year of daily observations on a calendar axis, opened on the last 90
/// days with the range slider showing the whole year underneath.
fn build_timeseries() -> Plot {
    const DAY: f64 = 86_400.0;
    let mut rng = Rng(20_260_101);
    let epoch = plotui_core::days_from_civil(2025, 9, 1) as f64 * DAY;
    let mut xs = Vec::with_capacity(365);
    let mut ys = Vec::with_capacity(365);
    let mut level = 42.0f32;
    for d in 0..365 {
        // A yearly swing, a weekly wobble, and a slow random walk.
        let t = d as f32;
        level = (level + rng.gauss() * 1.8).clamp(15.0, 85.0);
        let seasonal = 18.0 * (t / 365.0 * std::f32::consts::TAU).sin();
        let weekly = 4.0 * (t / 7.0 * std::f32::consts::TAU).sin();
        xs.push((d as f64 * DAY) as f32);
        ys.push(level + seasonal + weekly + rng.gauss() * 2.5);
    }
    let mut plot = Plot::new();
    let color = plot.resolve_color(None);
    plot.add_line2d(xs, ys, color, 2.0, Some("demand".into()), YAxis::Primary);
    plot.x_epoch = Some(epoch);
    plot.range_slider = true;
    plot.x_window = Some((275.0 * DAY, 365.0 * DAY)); // the last 90 days
    plot
}

/// The website hero: three gaussian clusters of 85 points (site/js/hero-scatter3d.js).
fn build_scatter() -> Plot {
    const CENTERS: [[f32; 3]; 3] = [[-0.45, -0.2, 0.35], [0.4, 0.3, -0.25], [0.05, -0.45, -0.45]];
    const SPREAD: [f32; 3] = [0.34, 0.3, 0.26];
    let mut rng = Rng(230607);
    let mut plot = Plot::new();
    for t in 0..3 {
        let pts: Vec<[f32; 3]> = (0..85)
            .map(|_| {
                [
                    CENTERS[t][0] + rng.gauss() * SPREAD[t],
                    CENTERS[t][1] + rng.gauss() * SPREAD[t],
                    CENTERS[t][2] + rng.gauss() * SPREAD[t],
                ]
            })
            .collect();
        let color = plot.next_color();
        plot.add_scatter3d(pts, color, 2.5, Some(format!("Cluster {}", ["A", "B", "C"][t])));
    }
    plot
}

/// The labelled 3D random geometric graph from examples/textual_graph.py:
/// nodes in a ball, edges within a radius, coloured pink→green by degree.
fn build_graph() -> Plot {
    const N: usize = 44;
    const PINK: Rgb = [230, 60, 120];
    const GREEN: Rgb = [70, 190, 120];
    let mut rng = Rng(7);
    let mut pts: Vec<[f32; 3]> = Vec::with_capacity(N);
    while pts.len() < N {
        let (x, y, z) = (rng.next() * 2.0 - 1.0, rng.next() * 2.0 - 1.0, rng.next() * 2.0 - 1.0);
        if x * x + y * y + z * z <= 1.0 {
            pts.push([x, y, z]);
        }
    }
    let radius = (9.0 / N as f32).powf(1.0 / 3.0);
    let mut edges: Vec<(u32, u32)> = Vec::new();
    let mut deg = [0usize; N];
    for i in 0..N {
        for j in i + 1..N {
            let d: f32 = (0..3).map(|k| (pts[i][k] - pts[j][k]).powi(2)).sum::<f32>().sqrt();
            if d < radius {
                edges.push((i as u32, j as u32));
                deg[i] += 1;
                deg[j] += 1;
            }
        }
    }
    let dmax = deg.iter().copied().max().unwrap_or(1).max(1) as f32;
    let colors: Vec<Rgb> = deg
        .iter()
        .map(|&d| {
            let t = (d as f32 / dmax).powf(1.4);
            std::array::from_fn(|c| (PINK[c] as f32 + (GREEN[c] as f32 - PINK[c] as f32) * t) as u8)
        })
        .collect();
    let mut plot = Plot::new();
    plot.add_graph3d(pts, colors, edges, 4.0, None, None, None, None);
    plot
}

/// The streaming demo from examples/textual_stream.py: a forecast line, noisy
/// observations, and a load series on y2, appended at 20 Hz through trace
/// handles. Keys 1/2/3 toggle the series.
fn run_stream(args: &ExampleArgs, out: Output) -> std::io::Result<()> {
    const STEP: f32 = 0.25;
    const TICK_MS: f64 = 50.0;

    let mut plot = Plot::new();
    let mut handles: Vec<TraceId> = Vec::new();
    let c = plot.resolve_color(None);
    handles.push(plot.add_line2d(vec![], vec![], c, 2.0, Some("forecast".into()), YAxis::Primary));
    let c = plot.resolve_color(None);
    handles.push(plot.add_scatter2d(
        vec![],
        vec![],
        c,
        1.8,
        Some("observed".into()),
        YAxis::Primary,
    ));
    let c = plot.resolve_color(None);
    handles.push(plot.add_line2d(vec![], vec![], c, 1.0, Some("load".into()), YAxis::Y2));

    let mut rng = Rng(20260830);
    let mut t = 0.0f32;
    let mut push = move |plot: &mut Plot, handles: &[TraceId]| {
        t += STEP;
        let base = (t * 0.4).sin() * 2.0 + (t * 0.09).sin() * 4.0;
        let noise = rng.gauss() * 1.1; // ≈ gauss(0, 0.5), as in the Python demo
        plot.extend_xy(handles[0], &[t], &[base]).expect("forecast handle");
        plot.extend_xy(handles[1], &[t], &[base + noise]).expect("observed handle");
        plot.extend_xy(handles[2], &[t], &[40.0 + 12.0 * (t * 0.13 + 1.0).sin()])
            .expect("load handle");
    };

    if out.is_still() {
        // One frame: arrive with the window the animation would have built.
        for _ in 0..180 {
            push(&mut plot, &handles);
        }
        return emit(&plot, args, &out);
    }

    for _ in 0..10 {
        push(&mut plot, &handles);
    }

    let feed_handles = handles.clone();
    let mut acc = 0.0f64;
    let feed = Box::new(move |state: &mut PlotState, dt_ms: f64| {
        acc += dt_ms;
        let mut moved = false;
        while acc >= TICK_MS {
            push(state.plot_mut(), &feed_handles);
            acc -= TICK_MS;
            moved = true;
        }
        if moved {
            state.invalidate();
        }
    });

    match out {
        Output::Record(opts) => {
            let hooks = Hooks { feed: Some(feed), ..Default::default() };
            record::record(plot, hooks, &opts)
        }
        Output::Interactive(mode) => {
            let mut shown = [true; 3];
            let on_key = Box::new(move |state: &mut PlotState, code: crossterm::event::KeyCode| {
                let crossterm::event::KeyCode::Char(ch @ '1'..='3') = code else {
                    return false;
                };
                let i = ch as usize - '1' as usize;
                shown[i] = !shown[i];
                state.set_visible(handles[i], shown[i]);
                true
            });
            let hooks = Hooks { feed: Some(feed), on_key: Some(on_key), ..Default::default() };
            interactive::run_with(plot, mode, args.width, args.height, hooks)
        }
        Output::Static(_) => unreachable!("still outputs handled above"),
    }
}
