//! Interactive mode: the plotui-ratatui widget in a minimal event loop.
//! Drag to pan, scroll to zoom, hover for the crosshair; `q`/Esc/Ctrl-C quit.

use std::io::stdout;
use std::time::{Duration, Instant};

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
};
use crossterm::execute;
use plotui_core::Plot;
use plotui_ratatui::{PlotEvent, PlotOptions, PlotState, PlotWidget};
use plotui_term::RenderMode;

pub type FeedFn = Box<dyn FnMut(&mut PlotState, f64)>;
pub type KeyFn = Box<dyn FnMut(&mut PlotState, KeyCode) -> bool>;
pub type EventFn = Box<dyn FnMut(&mut PlotState, PlotEvent)>;

/// Example-scene extensions to the plain data-chart loop: picking, an
/// auto-spinning camera, a per-frame data feed, and extra key bindings.
#[derive(Default)]
pub struct Hooks {
    /// Hover highlights nodes/edges, clicks select them.
    pub pickable: bool,
    /// Spin the camera (~30 Hz); pauses while dragging or while an element
    /// is selected, and space toggles it.
    pub auto_rotate: bool,
    /// Called once per loop pass, before drawing, with the elapsed time in
    /// milliseconds (capped, so a suspended terminal doesn't fast-forward the
    /// feed) — streaming scenes append points here. Taking dt from the caller
    /// keeps scenes clock-free, so the headless recorder can drive them at a
    /// fixed virtual rate.
    pub feed: Option<FeedFn>,
    /// Extra key bindings; return true to consume the key.
    pub on_key: Option<KeyFn>,
    /// Called with every interaction event the widget reports (a pick, a
    /// hover change, a range-slider change).
    pub on_plot_event: Option<EventFn>,
}

/// Cap on per-pass elapsed time so a suspended terminal (or a slow encode)
/// doesn't fast-forward feeds when it resumes.
const MAX_DT_MS: f64 = 250.0;

/// One animation step: run the feed with `dt_ms` of elapsed time, then
/// auto-rotate if `spin`. Shared by the event loop and the headless recorder.
pub fn advance(state: &mut PlotState, hooks: &mut Hooks, dt_ms: f64, spin: bool) {
    if let Some(feed) = hooks.feed.as_mut() {
        feed(state, dt_ms);
    }
    if spin {
        state.tick();
    }
}

pub fn run(
    plot: Plot,
    mode: RenderMode,
    width: Option<u16>,
    height: Option<u16>,
) -> std::io::Result<()> {
    run_with(plot, mode, width, height, Hooks::default())
}

pub fn run_with(
    plot: Plot,
    mode: RenderMode,
    width: Option<u16>,
    height: Option<u16>,
    mut hooks: Hooks,
) -> std::io::Result<()> {
    let mut state = PlotState::new(
        plot,
        PlotOptions {
            render_mode: Some(mode),
            pickable: hooks.pickable,
            auto_rotate: hooks.auto_rotate,
            ..Default::default()
        },
    );
    let mut spin = hooks.auto_rotate;

    let mut terminal = ratatui::init();
    execute!(stdout(), EnableMouseCapture)?;
    let mut last = Instant::now();
    let result = (|| -> std::io::Result<()> {
        loop {
            let now = Instant::now();
            let dt = (now.duration_since(last).as_secs_f64() * 1000.0).min(MAX_DT_MS);
            last = now;
            // Spinning yields to the user: never while dragging, and a selected
            // element holds the view still until it's deselected.
            let spinning = spin && !state.dragging() && state.plot().selected.is_none();
            advance(&mut state, &mut hooks, dt, spinning);
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
                        return Ok(());
                    }
                    Event::Key(k) if k.kind == KeyEventKind::Press => {
                        if hooks.auto_rotate && k.code == KeyCode::Char(' ') {
                            spin = !spin;
                        } else if !hooks.on_key.as_mut().is_some_and(|f| f(&mut state, k.code)) {
                            if let Some(pe) = state.handle_event(&Event::Key(k)) {
                                if let Some(h) = hooks.on_plot_event.as_mut() {
                                    h(&mut state, pe);
                                }
                            }
                        }
                    }
                    ev => {
                        if let Some(pe) = state.handle_event(&ev) {
                            if let Some(h) = hooks.on_plot_event.as_mut() {
                                h(&mut state, pe);
                            }
                        }
                    }
                }
            }
        }
    })();
    // Restore unconditionally: bailing out with mouse capture still on sprays
    // escape reports over the shell on every pointer move.
    let _ = execute!(stdout(), DisableMouseCapture);
    // Delete the image before restoring the terminal, or the last frame
    // outlives the app on terminals that keep placements around.
    let _ = state.cleanup(&mut stdout());
    ratatui::restore();
    result
}
