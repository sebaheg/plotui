//! Table + chart kind → a `plotui_core::Plot`.

use plotui_core::{BinSpec, Interp, Orient, Plot, YAxis};

use crate::input::Table;
use crate::ChartKind;

pub fn build_plot(kind: ChartKind, t: &Table) -> Plot {
    let mut plot = Plot::new();
    plot.x_epoch = t.x_epoch; // ISO-date x column → calendar axis
                              // A box plot summarises whole columns, so unlike every other chart it is
                              // built from the table at once rather than a series at a time.
    if matches!(kind, ChartKind::Box) {
        plot.x_epoch = None;
        let color = plot.resolve_color(None);
        let groups: Vec<Vec<f32>> = t.series.clone();
        let mut values = Vec::new();
        let mut starts = Vec::with_capacity(groups.len());
        for g in &groups {
            starts.push(values.len() as u32);
            values.extend_from_slice(g);
        }
        plot.add_box2d(values, starts, color, Orient::Vertical, None, YAxis::Primary);
        let names: Vec<String> = (0..t.series.len())
            .map(|i| t.names.get(i).cloned().flatten().unwrap_or_else(|| format!("col {}", i + 1)))
            .collect();
        plot.x_categories = Some(names);
        return plot;
    }
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
            ChartKind::Bar { horizontal, mode } => {
                plot.barmode = mode;
                let orient = if horizontal { Orient::Horizontal } else { Orient::Vertical };
                plot.add_bar2d_oriented(
                    t.x.clone(),
                    ys.clone(),
                    color,
                    orient,
                    name,
                    YAxis::Primary,
                );
            }
            ChartKind::Hist { bins, bin_width } => {
                // A histogram reads the column as a sample, not as heights, so
                // it ignores the table's x entirely.
                let spec = match (bins, bin_width) {
                    (Some(k), _) => BinSpec::Count(k),
                    (None, Some(w)) => BinSpec::Width(w),
                    (None, None) => BinSpec::Auto,
                };
                plot.add_histogram2d(ys.clone(), spec, color, name, YAxis::Primary);
            }
            ChartKind::Box => unreachable!("handled above"),
            ChartKind::Step => {
                plot.add_step2d(
                    t.x.clone(),
                    ys.clone(),
                    color,
                    2.0,
                    Interp::Post,
                    name,
                    YAxis::Primary,
                );
            }
        }
    }
    plot
}
