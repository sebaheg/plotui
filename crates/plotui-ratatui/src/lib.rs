//! plotui-ratatui — a Ratatui widget hosting an interactive plotui plot.
//!
//! Ratatui apps own the loop and input; this crate follows the same contract
//! as the Textual widget: forward events to [`PlotState::handle_event`], tick
//! auto-rotation with [`PlotState::tick`], and draw with the stateless
//! [`PlotWidget`]. The Rust core rasterizes pixels; the Kitty graphics
//! protocol puts them on screen.
//!
//! ```no_run
//! use plotui_ratatui::{PlotOptions, PlotState, PlotWidget};
//!
//! let mut plot = plotui_core::Plot::new();
//! let color = plot.resolve_color(None);
//! plot.add_scatter3d(vec![[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]], color, 2.0, None);
//! let mut state = PlotState::new(plot, PlotOptions::default());
//!
//! let mut terminal = ratatui::init();
//! loop {
//!     terminal.draw(|f| f.render_stateful_widget(PlotWidget, f.area(), &mut state)).unwrap();
//!     if let Ok(ev) = crossterm::event::read() {
//!         if let crossterm::event::Event::Key(_) = ev {
//!             break;
//!         }
//!         state.handle_event(&ev);
//!     }
//! }
//! // Delete the image before restoring the terminal, or the last frame
//! // outlives the app on terminals that keep placements around.
//! let mut out = std::io::stdout();
//! state.cleanup(&mut out).unwrap();
//! ratatui::restore();
//! ```
//!
//! Everything tunable (drag/zoom/key constants, the half-resolution policy,
//! terminal detection) comes from `plotui-term`, shared with the other
//! frontends.

mod events;
mod state;
mod widget;

pub use plotui_term::RenderMode;
pub use state::{OverlaySpan, PlotOptions, PlotState};
pub use widget::PlotWidget;

/// What part of a plot an interaction hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementKind {
    Node,
    Edge,
}

/// An interaction result from [`PlotState::handle_event`], for the host to
/// act on (open an inspector, show a status line, …).
// PartialEq only: `RangeChanged` carries f64 bounds, which have no `Eq`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlotEvent {
    /// A click resolved against nodes only (`pickable: false`); `None` means
    /// empty space was clicked.
    NodePicked(Option<usize>),
    /// A click resolved against nodes and edges (`pickable: true`).
    ElementPicked(Option<(ElementKind, usize)>),
    /// The hovered element changed (`pickable: true` only).
    ElementHovered(Option<(ElementKind, usize)>),
    /// The 2D x window changed through a finished gesture (a released strip
    /// drag, a track jump, a scroll zoom, or an `[`/`]` key), carrying the
    /// new window (`None` = full extent).
    RangeChanged(Option<(f64, f64)>),
}

fn to_kind(el: plotui_core::Element) -> (ElementKind, usize) {
    match el {
        plotui_core::Element::Node(i) => (ElementKind::Node, i),
        plotui_core::Element::Edge(i) => (ElementKind::Edge, i),
    }
}
