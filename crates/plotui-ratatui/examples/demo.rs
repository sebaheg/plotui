//! Ratatui demo: an interactive 3D scatter (a double helix, mirroring
//! examples/textual_demo.py).
//!
//!     cargo run -p plotui-ratatui --example demo
//!
//! Drag to rotate, shift-drag to pan, scroll to zoom, `r` to reset, `q` to
//! quit. Requires a terminal with Kitty graphics (Kitty, Ghostty, iTerm2 ≥
//! 3.5, WezTerm).

use std::io::stdout;
use std::time::Duration;

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode};
use crossterm::execute;
use plotui_ratatui::{PlotOptions, PlotState, PlotWidget};

fn make_plot() -> plotui_core::Plot {
    let mut plot = plotui_core::Plot::new();
    let n = 1600;
    let pts: Vec<[f32; 3]> = (0..n)
        .map(|i| {
            let t = i as f32 / n as f32 * 6.0 * std::f32::consts::PI;
            let strand = if i % 2 == 0 { 1.0 } else { -1.0 };
            [t.cos() * strand, t / (6.0 * std::f32::consts::PI) * 2.0 - 1.0, t.sin() * strand]
        })
        .collect();
    plot.add_scatter3d(pts, [230, 60, 120], 2.0, None);
    plot
}

fn main() -> std::io::Result<()> {
    let mut state =
        PlotState::new(make_plot(), PlotOptions { auto_rotate: true, ..Default::default() });

    let mut terminal = ratatui::init();
    execute!(stdout(), EnableMouseCapture)?;
    loop {
        terminal.draw(|f| f.render_stateful_widget(PlotWidget, f.area(), &mut state))?;
        if event::poll(Duration::from_millis(33))? {
            match event::read()? {
                Event::Key(k) if k.code == KeyCode::Char('q') => break,
                ev => {
                    state.handle_event(&ev);
                }
            }
        }
        state.tick();
    }
    execute!(stdout(), DisableMouseCapture)?;
    // Delete the image before restoring the terminal, or the last frame
    // outlives the app on terminals that keep placements around.
    state.cleanup(&mut stdout())?;
    ratatui::restore();
    Ok(())
}
