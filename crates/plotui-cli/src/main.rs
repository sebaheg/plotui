//! The plotui CLI: pipe columns of numbers in, get a real-pixel chart out.
//!
//!     seq 1 100 | LC_ALL=C awk '{print $1, sin($1/10)}' | plotui line
//!
//! On a TTY the chart opens interactively (drag to pan, scroll to zoom, hover
//! for the crosshair, `q` to quit); when stdout is piped, or with `--static`,
//! one frame of Kitty escapes is printed instead.

mod build;
mod deps;
mod examples;
mod input;
mod interactive;
mod lidar;
mod record;
mod render;

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{ArgAction, Parser, Subcommand};
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
    Bar(Args),
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
    /// Export to a file instead of the terminal (.png; needs ffmpeg on PATH)
    #[arg(long)]
    out: Option<PathBuf>,
    /// Export frame size as WxH pixels (only with --out)
    #[arg(long, default_value = "1280x720", value_parser = parse_size)]
    size: (u16, u16),
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
    Bar,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let (kind, args) = match cli.chart {
        Chart::Line(a) => (ChartKind::Line, a),
        Chart::Scatter(a) => (ChartKind::Scatter, a),
        Chart::Bar(a) => (ChartKind::Bar, a),
        Chart::Example(a) => return examples::run(&a),
    };

    let table = match input::load(args.file.as_deref(), args.delimiter.as_deref(), args.header) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("plotui: {e}");
            return ExitCode::from(2);
        }
    };

    let mut plot = build::build_plot(kind, &table);
    plot.range_slider = args.range_slider;

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
