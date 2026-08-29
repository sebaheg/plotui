//! The stateless widget: paints the cached frame into ratatui's buffer.

use std::num::NonZeroU16;

use plotui_term::{FrameOutput, RenderMode};
use ratatui::buffer::{Buffer, CellDiffOption};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::StatefulWidget;

use crate::state::{OverlaySpan, PlotState};

/// The plot widget. Stateless — render it with
/// `frame.render_stateful_widget(PlotWidget, area, &mut plot_state)`.
pub struct PlotWidget;

impl StatefulWidget for PlotWidget {
    type State = PlotState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut PlotState) {
        state.area = area;
        state.needs_redraw = false;
        if area.width == 0 || area.height == 0 {
            return;
        }
        if state.mode == RenderMode::Unsupported {
            render_notice(area, buf);
            return;
        }
        state.ensure_frame(area.width, area.height);
        let force = std::mem::take(&mut state.force_retransmit);
        let always = state.always_retransmit;

        let Some(frame) = state.frame.as_ref() else { return };
        match frame {
            FrameOutput::Placeholder { transmit, id_rgb, cells } => {
                let fg = Color::Rgb(id_rgb.0, id_rgb.1, id_rgb.2);
                for (y, row) in cells.iter().enumerate().take(area.height as usize) {
                    for (x, cell) in row.iter().enumerate().take(area.width as usize) {
                        if let Some(c) = buf.cell_mut((area.x + x as u16, area.y + y as u16)) {
                            c.set_symbol(cell);
                            c.set_fg(fg);
                        }
                    }
                }
                splice_overlay(&state.overlay, area, buf);
                embed_transmit(transmit, area, buf, force || always);
            }
            FrameOutput::Direct { escape } => {
                // The cells under the image stay blank; overlays are still
                // spliced, though most terminals draw the image above them —
                // prefer the placeholder path for text-over-plot.
                splice_overlay(&state.overlay, area, buf);
                embed_transmit(escape, area, buf, force || always);
            }
            FrameOutput::Unsupported => render_notice(area, buf),
        }
    }
}

/// Carry an escape in the top-left cell's symbol, ahead of whatever glyph is
/// there. The escape has zero visible width (placeholder transmits are
/// virtual placements; direct escapes save/restore the cursor), so
/// `ForcedWidth(1)` keeps the diff's width accounting honest — and the diff
/// then re-emits the escape exactly when the frame's bytes change.
fn embed_transmit(escape: &str, area: Rect, buf: &mut Buffer, always: bool) {
    if let Some(c) = buf.cell_mut((area.x, area.y)) {
        let symbol = format!("{escape}{}", c.symbol());
        c.set_symbol(&symbol);
        c.set_diff_option(if always {
            CellDiffOption::AlwaysUpdate
        } else {
            CellDiffOption::ForcedWidth(NonZeroU16::MIN)
        });
    }
}

/// Write overlay spans over the frame: sorted by (row, col), first one wins
/// on overlap, clipped to the widget. Placeholder cells are self-addressed,
/// so cells after a spliced gap still map to the right part of the image.
fn splice_overlay(overlay: &[OverlaySpan], area: Rect, buf: &mut Buffer) {
    if overlay.is_empty() {
        return;
    }
    let mut order: Vec<&OverlaySpan> = overlay.iter().collect();
    order.sort_by_key(|s| (s.row, s.col));
    let mut row_end = (u16::MAX, 0u16); // (row, first free col)
    for span in order {
        if span.row >= area.height || span.col >= area.width || span.text.is_empty() {
            continue;
        }
        if span.row == row_end.0 && span.col < row_end.1 {
            continue; // overlaps the previous span — first one wins
        }
        let (end_x, _) = buf.set_stringn(
            area.x + span.col,
            area.y + span.row,
            &span.text,
            (area.width - span.col) as usize,
            span.style,
        );
        row_end = (span.row, end_x - area.x);
    }
}

/// The centered "this terminal can't do pixels" notice.
fn render_notice(area: Rect, buf: &mut Buffer) {
    let lines = plotui_term::policy::UNSUPPORTED_MESSAGE;
    let top = (area.height as usize).saturating_sub(lines.len()) / 2;
    for (i, line) in lines.iter().enumerate() {
        let y = area.y + (top + i) as u16;
        if y >= area.bottom() {
            break;
        }
        let len = line.chars().count().min(area.width as usize);
        let x = area.x + ((area.width as usize - len) / 2) as u16;
        let style = match i {
            0 => Style::default().add_modifier(Modifier::BOLD),
            i if i == lines.len() - 1 => Style::default().add_modifier(Modifier::DIM),
            _ => Style::default(),
        };
        buf.set_stringn(x, y, line, area.width as usize, style);
    }
}
