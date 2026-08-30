//! Table + chart kind → a `plotui_core::Plot`.

use plotui_core::{Plot, YAxis};

use crate::input::Table;
use crate::ChartKind;

pub fn build_plot(kind: ChartKind, t: &Table) -> Plot {
    let mut plot = Plot::new();
    plot.x_epoch = t.x_epoch; // ISO-date x column → calendar axis
    for (i, ys) in t.series.iter().enumerate() {
        let color = plot.resolve_color(None);
        let name = t.names.get(i).cloned().flatten();
        match kind {
            ChartKind::Line => {
                plot.add_line2d(t.x.clone(), ys.clone(), color, 2.0, name, YAxis::Primary);
            }
            ChartKind::Scatter => {
                plot.add_scatter2d(t.x.clone(), ys.clone(), color, 1.8, name, YAxis::Primary);
            }
            ChartKind::Bar => {
                plot.add_bar2d(t.x.clone(), ys.clone(), color, name, YAxis::Primary);
            }
        }
    }
    plot
}
