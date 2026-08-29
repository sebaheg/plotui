//! Interactive mode: the plotui-ratatui widget in a minimal event loop.
//! Drag to pan, scroll to zoom, hover for the crosshair; `q`/Esc/Ctrl-C quit.

use std::io::stdout;
use std::time::Duration;

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers,
};
use crossterm::execute;
use plotui_core::Plot;
use plotui_ratatui::{PlotOptions, PlotState, PlotWidget};
use plotui_term::RenderMode;

pub fn run(
    plot: Plot,
    mode: RenderMode,
    width: Option<u16>,
    height: Option<u16>,
) -> std::io::Result<()> {
    let mut state =
        PlotState::new(plot, PlotOptions { render_mode: Some(mode), ..Default::default() });

    let mut terminal = ratatui::init();
    execute!(stdout(), EnableMouseCapture)?;
    loop {
        terminal.draw(|f| {
            let mut area = f.area();
            if let Some(w) = width {
                area.width = area.width.min(w);
            }
            if let Some(h) = height {
                area.height = area.height.min(h);
            }
            f.render_stateful_widget(PlotWidget, area, &mut state);
        })?;
        if event::poll(Duration::from_millis(33))? {
            match event::read()? {
                Event::Key(k)
                    if k.code == KeyCode::Char('q')
                        || k.code == KeyCode::Esc
                        || (k.code == KeyCode::Char('c')
                            && k.modifiers.contains(KeyModifiers::CONTROL)) =>
                {
                    break;
                }
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
