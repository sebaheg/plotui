//! Static mode: compose one frame, print it, exit.

use std::io::Write;

use plotui_core::Plot;
use plotui_term::{
    compose_frame, detect_cell_px, kitty_replace_env, next_image_id, FrameOutput, FrameRequest,
    RenderMode, FALLBACK_CELL_PX,
};

pub fn render_static(
    plot: &Plot,
    mode: RenderMode,
    width: Option<u16>,
    height: Option<u16>,
) -> std::io::Result<()> {
    // detect_cell_px probes stdout, then stderr, then stdin — so real cell
    // metrics survive stdout being piped.
    let (cell_w, cell_h) = detect_cell_px(FALLBACK_CELL_PX);
    let (tcols, trows) = crossterm::terminal::size().unwrap_or((80, 24));
    let cols = width.unwrap_or(tcols).max(1);
    let rows = height.unwrap_or_else(|| trows.saturating_sub(2).max(8)).max(1);

    let out = compose_frame(
        plot,
        &FrameRequest {
            cols,
            rows,
            cell_w,
            cell_h,
            scale: 1.0,
            mode,
            image_id: next_image_id(),
            replace: kitty_replace_env(),
            tmux: std::env::var("TMUX").is_ok_and(|v| !v.is_empty()),
        },
    );

    let stdout = std::io::stdout();
    let mut w = stdout.lock();
    match out {
        FrameOutput::Placeholder { transmit, id_rgb, cells } => {
            // Placeholder cells are self-addressed text: they flow (and
            // scroll) like any other output.
            write!(w, "{transmit}")?;
            for row in cells {
                let line: String = row.concat();
                writeln!(w, "\x1b[38;2;{};{};{}m{line}\x1b[39m", id_rgb.0, id_rgb.1, id_rgb.2)?;
            }
        }
        FrameOutput::Direct { escape } => {
            // The escape draws from the cursor and restores it afterwards, so
            // scroll a region into place first, draw at its top, then park
            // the cursor below the image.
            write!(w, "{}", "\n".repeat(rows as usize))?;
            write!(w, "\x1b[{rows}A{escape}\x1b[{rows}B")?;
        }
        FrameOutput::Unsupported => unreachable!("unsupported terminals are rejected in main"),
    }
    w.flush()
}
