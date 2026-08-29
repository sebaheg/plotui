//! The plotui CLI: pipe columns of numbers in, get a real-pixel chart out.
//!
//!     seq 1 100 | awk '{print $1, sin($1/10)}' | plotui line
//!
//! On a TTY the chart opens interactively (drag to pan, scroll to zoom, hover
//! for the crosshair, `q` to quit); when stdout is piped, or with `--static`,
//! one frame of Kitty escapes is printed instead.

mod build;
mod input;
mod interactive;
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
    };

    let table = match input::load(args.file.as_deref(), args.delimiter.as_deref(), args.header) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("plotui: {e}");
            return ExitCode::from(2);
        }
    };

    let mode = plotui_term::detect_render_mode();
    if mode == RenderMode::Unsupported {
        for line in plotui_term::policy::UNSUPPORTED_MESSAGE {
            eprintln!("{line}");
        }
        return ExitCode::from(3);
    }

    let plot = build::build_plot(kind, &table);
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
