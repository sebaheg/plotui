//! The plotui CLI: pipe columns of numbers in, get a real-pixel chart out.
//!
//!     seq 1 100 | LC_ALL=C awk '{print $1, sin($1/10)}' | plotui line
//!
//! On a TTY the chart opens interactively (drag to pan, scroll to zoom, hover
//! for the crosshair, `q` to quit); when stdout is piped, or with `--static`,
//! one frame of Kitty escapes is printed instead.

mod aizawa;
mod build;
mod dag;
mod deps;
mod examples;
mod follow;
mod input;
mod interactive;
mod lidar;
mod mandelbulb;
mod pipeline;
mod protein;
mod record;
mod render;

use std::cell::RefCell;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;

use clap::{ArgAction, Parser, Subcommand};
use crossterm::event::KeyCode;
use plotui_core::BarMode;
use plotui_ratatui::{PlotEvent, PlotState};
use plotui_term::RenderMode;

#[derive(Parser)]
#[command(
    name = "plotui",
    version,
    about = "Plot data from stdin or a file as real terminal pixels (Kitty graphics).",
    disable_help_flag = true
)]
struct Cli {
    #[command(subcommand)]
    chart: Chart,
    /// Print help
    #[arg(long, global = true, action = ArgAction::Help)]
    help: Option<bool>,
}

#[derive(Subcommand)]
enum Chart {
    /// Line chart
    Line(Args),
    /// Scatter plot
    Scatter(Args),
    /// Bar chart
    Bar(BarArgs),
    /// Step chart — the right-angle path between samples, for series that
    /// hold their value (counters, states, prices)
    Step(Args),
    /// Histogram of a column of numbers
    Hist(HistArgs),
    /// Box plot — one box per column, showing quartiles, whiskers and outliers
    Box(Args),
    /// Directed graph from a DOT file — a pipeline, a DAG, a dependency tree
    Dag(DagArgs),
    /// Run a built-in example scene (no input data; lists them when run bare)
    Example(ExampleArgs),
}

// `-h` means height here, so clap's short help flag is disabled and `--help`
// is declared explicitly (global, on the top-level command).
#[derive(clap::Args)]
#[command(disable_help_flag = true)]
struct Args {
    /// Input file; "-" or absent reads stdin
    file: Option<PathBuf>,
    /// Field delimiter: a single character, "tab", or "space" (default: auto-detect)
    #[arg(short = 'd', long)]
    delimiter: Option<String>,
    /// Treat the first row as series names
    #[arg(short = 'H', long)]
    header: bool,
    /// Plot width in terminal cells (default: terminal width)
    #[arg(short = 'w', long)]
    width: Option<u16>,
    /// Plot height in terminal cells (default: terminal height minus 2)
    #[arg(short = 'h', long)]
    height: Option<u16>,
    /// Render one frame and exit (default when stdout is not a terminal)
    #[arg(long = "static")]
    static_mode: bool,
    /// Show a range slider under the chart (drag its handles/window to zoom
    /// the x axis)
    #[arg(long)]
    range_slider: bool,
    /// Keep reading rows as they arrive and append them live. Needs piped
    /// input and an interactive terminal: `tail -f app.log | plotui line -f`
    #[arg(short = 'f', long)]
    follow: bool,
    /// With --follow: keep the view on the last N samples. Press f to go
    /// live again after dragging the range slider back
    #[arg(long, value_name = "N", conflicts_with = "last")]
    window: Option<usize>,
    /// With --follow: keep the view on the last span of x — 30s, 5m, 2h, 1d,
    /// or a bare number in x units
    #[arg(long, value_name = "SPAN", value_parser = parse_span)]
    last: Option<f64>,
    /// Chart title, drawn above the plot
    #[arg(long)]
    title: Option<String>,
    /// x axis title, drawn under its tick labels
    #[arg(long)]
    x_title: Option<String>,
    /// y axis title, drawn rotated in the left margin
    #[arg(long)]
    y_title: Option<String>,
    /// Pin the x extent, as LO:HI (e.g. -5:5). Zoom and pan still work from
    /// there; without it the axis fits the data
    #[arg(long, value_parser = parse_range)]
    x_range: Option<(f64, f64)>,
    /// Pin the y extent, as LO:HI
    #[arg(long, value_parser = parse_range)]
    y_range: Option<(f64, f64)>,
    /// Scale the x axis by log10 (ignored on a categorical axis)
    #[arg(long)]
    log_x: bool,
    /// Scale the y axis by log10
    #[arg(long)]
    log_y: bool,
    /// Export to a file instead of the terminal (.png; needs ffmpeg on PATH)
    #[arg(long)]
    out: Option<PathBuf>,
    /// Export frame size as WxH pixels (only with --out)
    #[arg(long, default_value = "1280x720", value_parser = parse_size)]
    size: (u16, u16),
}

#[derive(clap::Args)]
#[command(disable_help_flag = true)]
struct BarArgs {
    #[command(flatten)]
    common: Args,
    /// Lay the bars along x instead of y (readable for long category labels)
    #[arg(long)]
    horizontal: bool,
    /// Stack the series on top of one another
    #[arg(long, conflicts_with = "group")]
    stack: bool,
    /// Draw the series side by side within each position
    #[arg(long)]
    group: bool,
}

#[derive(clap::Args)]
#[command(disable_help_flag = true)]
struct DagArgs {
    /// DOT file; "-" or absent reads stdin
    file: Option<PathBuf>,
    /// Flow direction: "tb" (top to bottom) or "lr" (left to right);
    /// default: whatever the file's rankdir says, else tb
    #[arg(long)]
    rankdir: Option<String>,
    /// Plot width in terminal cells (default: terminal width)
    #[arg(short = 'w', long)]
    width: Option<u16>,
    /// Plot height in terminal cells (default: terminal height minus 2)
    #[arg(short = 'h', long)]
    height: Option<u16>,
    /// Render one frame and exit (default when stdout is not a terminal)
    #[arg(long = "static")]
    static_mode: bool,
    /// Export to a file instead of the terminal (.png; needs ffmpeg on PATH)
    #[arg(long)]
    out: Option<PathBuf>,
    /// Export frame size as WxH pixels (only with --out)
    #[arg(long, default_value = "1280x720", value_parser = parse_size)]
    size: (u16, u16),
}

#[derive(clap::Args)]
#[command(disable_help_flag = true)]
struct HistArgs {
    #[command(flatten)]
    common: Args,
    /// Number of bins (default: chosen from the data's spread)
    #[arg(long)]
    bins: Option<usize>,
    /// Bin width (default: chosen from the data's spread)
    #[arg(long)]
    bin_width: Option<f64>,
}

#[derive(clap::Args)]
#[command(disable_help_flag = true)]
pub struct ExampleArgs {
    /// Example name; omit to list the available examples
    name: Option<String>,
    /// List the available examples
    #[arg(long)]
    list: bool,
    /// Plot width in terminal cells (default: terminal width)
    #[arg(short = 'w', long)]
    width: Option<u16>,
    /// Plot height in terminal cells (default: terminal height minus 2)
    #[arg(short = 'h', long)]
    height: Option<u16>,
    /// Render one frame and exit (default when stdout is not a terminal)
    #[arg(long = "static")]
    static_mode: bool,
    /// Export to a file instead of the terminal: .mp4/.gif/.webm records the
    /// animation, .png takes one frame (needs ffmpeg on PATH)
    #[arg(long)]
    out: Option<PathBuf>,
    /// Export frame size as WxH pixels (only with --out)
    #[arg(long, default_value = "1280x720", value_parser = parse_size)]
    size: (u16, u16),
    /// Frames to record with --out (at --fps; 300 @ 30 fps ≈ 10 s)
    #[arg(long, default_value_t = 300)]
    frames: u32,
    /// Frame rate for --out recordings
    #[arg(long, default_value_t = 30)]
    fps: u32,
}

/// A span of x: `30s`, `5m`, `2h`, `1d`, or a bare number in x units. The
/// suffixes are seconds-based because that is what a time axis counts in;
/// on a numeric axis a bare number is the honest way to say it.
fn parse_span(s: &str) -> Result<f64, String> {
    let err = || format!("expected a span like 30s, 5m, 2h or a number, got '{s}'");
    let t = s.trim();
    let (num, mult) = match t.chars().last() {
        Some('s') => (&t[..t.len() - 1], 1.0),
        Some('m') => (&t[..t.len() - 1], 60.0),
        Some('h') => (&t[..t.len() - 1], 3600.0),
        Some('d') => (&t[..t.len() - 1], 86_400.0),
        _ => (t, 1.0),
    };
    let v: f64 = num.trim().parse().map_err(|_| err())?;
    if !v.is_finite() || v <= 0.0 {
        return Err(format!("a span must be positive, got '{s}'"));
    }
    Ok(v * mult)
}

/// `LO:HI`, gnuplot's range syntax. A colon rather than a comma so a decimal
/// comma can never be read as a separator — the same reason the input parser
/// leaves comma decimals alone.
fn parse_range(s: &str) -> Result<(f64, f64), String> {
    let err = || format!("expected LO:HI (e.g. 0:100), got '{s}'");
    let (lo, hi) = s.split_once(':').ok_or_else(err)?;
    let lo: f64 = lo.trim().parse().map_err(|_| err())?;
    let hi: f64 = hi.trim().parse().map_err(|_| err())?;
    Ok((lo, hi))
}

fn parse_size(s: &str) -> Result<(u16, u16), String> {
    let err = || format!("expected WxH (e.g. 1280x720), got '{s}'");
    let (w, h) = s.split_once(['x', 'X']).ok_or_else(err)?;
    let w: u16 = w.trim().parse().map_err(|_| err())?;
    let h: u16 = h.trim().parse().map_err(|_| err())?;
    if w < 2 || h < 2 {
        return Err(err());
    }
    Ok((w, h))
}

#[derive(Clone, Copy)]
pub enum ChartKind {
    Line,
    Scatter,
    Bar {
        horizontal: bool,
        mode: BarMode,
    },
    Step,
    Box,
    /// `None` bins by the automatic rule; the two knobs are mutually
    /// exclusive and validated before we get here.
    Hist {
        bins: Option<usize>,
        bin_width: Option<f64>,
    },
}

/// A `--follow` session: check that this is somewhere a live chart can even
/// exist, open the feed, and hand its drain to the frame loop.
///
/// The checks come first and the read second, so a mistake fails instantly
/// instead of after blocking on a pipe that will never satisfy it.
fn run_follow(kind: ChartKind, args: &Args) -> Result<(), String> {
    if args.out.is_some() {
        return Err("--follow draws a live chart; --out writes one frame. Pick one".into());
    }
    if args.static_mode {
        return Err("--follow and --static are opposites: one keeps reading, one draws once".into());
    }
    if matches!(kind, ChartKind::Box) {
        return Err("a box plot summarises a whole column at once, so it has nothing to follow; \
             try line, scatter, step, bar or hist"
            .into());
    }
    let window = match (args.window, args.last) {
        (Some(n), _) if n < 2 => return Err("--window needs at least 2 samples".into()),
        (Some(n), _) => Some(follow::Window::Samples(n)),
        (_, Some(span)) => Some(follow::Window::Span(span)),
        _ => None,
    };
    if window.is_some() && matches!(kind, ChartKind::Hist { .. }) {
        return Err(
            "a histogram's x is its bins, not the order samples arrived, so there is nothing \
             to slide along; drop --window/--last, or plot the series with line or scatter"
                .into(),
        );
    }
    if let Some(f) = args.file.as_deref() {
        if f.as_os_str() != "-" {
            return Err(format!(
                "--follow reads a stream, not a file — pipe one in: tail -f {} | plotui …",
                f.display()
            ));
        }
    }
    if !std::io::stdout().is_terminal() {
        return Err("--follow needs a terminal to draw into; stdout is redirected".into());
    }
    if std::io::stdin().is_terminal() {
        return Err("--follow reads its rows from a pipe, and stdin is a terminal".into());
    }
    // The terminal is asked before the feed is: a chart that can never be
    // drawn should say so now, not after the first row finally arrives.
    let mode = plotui_term::detect_render_mode();
    if mode == RenderMode::Unsupported {
        for line in plotui_term::policy::UNSUPPORTED_MESSAGE {
            eprintln!("{line}");
        }
        return Ok(());
    }

    let (mut plot, mut follower) = follow::start(kind, args.delimiter.as_deref(), args.header)?;
    if let Some(w) = window {
        follower.set_window(w);
        // A sliding view without the strip is a chart that quietly hides most
        // of its data; with it, the window is drawn against the whole run and
        // is draggable back through it.
        plot.range_slider = true;
    } else {
        plot.range_slider = args.range_slider;
    }
    apply_axes(&mut plot, args).map_err(|e| e.to_string())?;

    // The feed owns the follower for the length of the loop, and the report
    // is read after it: nothing may print while the plot has the terminal.
    let follower = Rc::new(RefCell::new(follower));
    let (drained, taken, rearmed) =
        (Rc::clone(&follower), Rc::clone(&follower), Rc::clone(&follower));
    let hooks = interactive::Hooks {
        feed: Some(Box::new(move |state: &mut PlotState, _dt: f64| {
            let mut follower = drained.borrow_mut();
            // Once the writer has hung up there is nothing left to poll for;
            // the chart stays up and interactive on what it already has.
            if !follower.ended() && follower.drain(state.plot_mut()) {
                state.invalidate();
            }
        })),
        // A finished gesture on the window is the reader saying "I'll drive":
        // someone who dragged back to an incident does not want the next row
        // to yank them forward again.
        on_plot_event: Some(Box::new(move |_state: &mut PlotState, event: PlotEvent| {
            if matches!(event, PlotEvent::RangeChanged(_)) {
                taken.borrow_mut().disarm();
            }
        })),
        // …and `f` is how they hand it back, jumping to the head rather than
        // waiting for the next row, so the key does something visible.
        on_key: Some(Box::new(move |state: &mut PlotState, code: KeyCode| {
            if code != KeyCode::Char('f') {
                return false;
            }
            rearmed.borrow_mut().rearm(state.plot_mut());
            state.invalidate();
            true
        })),
        ..Default::default()
    };
    interactive::run_with(plot, mode, args.width, args.height, hooks).map_err(|e| e.to_string())?;
    if let Some(report) = follower.borrow().report() {
        eprintln!("plotui: {report}");
    }
    Ok(())
}

/// The `--title` / `--*-range` / `--log-*` family, applied in the order that
/// makes both orders work: the scales first, so a range that a log axis
/// cannot show is rejected outright rather than silently lifted.
fn apply_axes(plot: &mut plotui_core::Plot, args: &Args) -> Result<(), plotui_bind::BindError> {
    plotui_bind::set_log(plot, "x", args.log_x)?;
    plotui_bind::set_log(plot, "y", args.log_y)?;
    plotui_bind::set_title(plot, "title", args.title.clone())?;
    plotui_bind::set_title(plot, "x", args.x_title.clone())?;
    plotui_bind::set_title(plot, "y", args.y_title.clone())?;
    plotui_bind::set_range(plot, "x", args.x_range)?;
    plotui_bind::set_range(plot, "y", args.y_range)?;
    Ok(())
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let (kind, args) = match cli.chart {
        Chart::Line(a) => (ChartKind::Line, a),
        Chart::Scatter(a) => (ChartKind::Scatter, a),
        Chart::Bar(a) => {
            let mode = match (a.stack, a.group) {
                (true, _) => BarMode::Stack,
                (_, true) => BarMode::Group,
                _ => BarMode::Overlay,
            };
            (ChartKind::Bar { horizontal: a.horizontal, mode }, a.common)
        }
        Chart::Step(a) => (ChartKind::Step, a),
        Chart::Box(a) => (ChartKind::Box, a),
        Chart::Hist(a) => {
            if a.bins.is_some() && a.bin_width.is_some() {
                eprintln!("plotui: give --bins or --bin-width, not both");
                return ExitCode::from(2);
            }
            (ChartKind::Hist { bins: a.bins, bin_width: a.bin_width }, a.common)
        }
        Chart::Example(a) => return examples::run(&a),
        Chart::Dag(a) => return dag::run(&a),
    };

    if args.follow {
        return match run_follow(kind, &args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("plotui: {e}");
                ExitCode::from(2)
            }
        };
    }

    if args.window.is_some() || args.last.is_some() {
        eprintln!("plotui: --window and --last follow a live feed; add --follow");
        return ExitCode::from(2);
    }

    let table = match input::load(args.file.as_deref(), args.delimiter.as_deref(), args.header) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("plotui: {e}");
            return ExitCode::from(2);
        }
    };

    let mut plot = build::build_plot(kind, &table);
    plot.range_slider = args.range_slider;
    // Through the shared binding rules, so `plotui --log-y --y-range 0:1`
    // fails with the same words the Python and Go bindings would use.
    if let Err(e) = apply_axes(&mut plot, &args) {
        eprintln!("plotui: {e}");
        return ExitCode::from(2);
    }

    // File export never needs (or probes) a graphics-capable terminal.
    if let Some(out) = &args.out {
        return match record::record_static(&plot, out, args.size.0.into(), args.size.1.into()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("plotui: {e}");
                ExitCode::FAILURE
            }
        };
    }

    let mode = plotui_term::detect_render_mode();
    if mode == RenderMode::Unsupported {
        for line in plotui_term::policy::UNSUPPORTED_MESSAGE {
            eprintln!("{line}");
        }
        return ExitCode::from(3);
    }

    let result = if args.static_mode || !std::io::stdout().is_terminal() {
        render::render_static(&plot, mode, args.width, args.height)
    } else {
        interactive::run(plot, mode, args.width, args.height)
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("plotui: {e}");
            ExitCode::FAILURE
        }
    }
}
