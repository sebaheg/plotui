//! Streaming demo: live data appended 20 times a second (mirroring
//! examples/textual_stream.py).
//!
//!     cargo run -p plotui-ratatui --example stream
//!
//! `1`/`2`/`3` toggle series visibility by handle, `q` quits. Hover for the
//! 2D crosshair.

use std::io::stdout;
use std::time::{Duration, Instant};

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode};
use crossterm::execute;
use plotui_core::YAxis;
use plotui_ratatui::{PlotOptions, PlotState, PlotWidget};

fn main() -> std::io::Result<()> {
    let mut plot = plotui_core::Plot::new();
    let color = plot.resolve_color(None);
    let h_forecast =
        plot.add_line2d(vec![], vec![], color, 2.0, Some("forecast".into()), YAxis::Primary);
    let color = plot.resolve_color(None);
    let h_observed =
        plot.add_scatter2d(vec![], vec![], color, 1.8, Some("observed".into()), YAxis::Primary);
    let color = plot.resolve_color(None);
    let h_load = plot.add_line2d(vec![], vec![], color, 1.0, Some("load".into()), YAxis::Y2);
    let handles = [h_forecast, h_observed, h_load];
    let mut shown = [true; 3];

    let mut state = PlotState::new(plot, PlotOptions::default());
    let mut terminal = ratatui::init();
    execute!(stdout(), EnableMouseCapture)?;

    // A tiny LCG stands in for gaussian noise — no rand dependency.
    let mut seed: u64 = 0x9E3779B97F4A7C15;
    let mut noise = move || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((seed >> 33) as f32 / (1u64 << 31) as f32 - 0.5) * 1.5
    };

    let mut t = 0.0f32;
    let mut next_feed = Instant::now();
    'outer: loop {
        terminal.draw(|f| f.render_stateful_widget(PlotWidget, f.area(), &mut state))?;
        while event::poll(Duration::from_millis(10))? {
            match event::read()? {
                Event::Key(k) => match k.code {
                    KeyCode::Char('q') => break 'outer,
                    KeyCode::Char(c @ '1'..='3') => {
                        let i = c as usize - '1' as usize;
                        shown[i] = !shown[i];
                        state.set_visible(handles[i], shown[i]);
                    }
                    _ => {
                        state.handle_event(&Event::Key(k));
                    }
                },
                ev => {
                    state.handle_event(&ev);
                }
            }
        }
        if Instant::now() >= next_feed {
            next_feed += Duration::from_millis(50);
            t += 0.25;
            let base = (t * 0.4).sin() * 2.0 + (t * 0.09).sin() * 4.0;
            state.extend(h_forecast, &[t], &[base], None).unwrap();
            state.extend(h_observed, &[t], &[base + noise()], None).unwrap();
            state.extend(h_load, &[t], &[40.0 + 12.0 * (t * 0.13 + 1.0).sin()], None).unwrap();
        }
    }
    execute!(stdout(), DisableMouseCapture)?;
    state.cleanup(&mut stdout())?;
    ratatui::restore();
    Ok(())
}
