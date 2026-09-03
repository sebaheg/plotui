//! `--follow`: keep reading rows while the plot is up, and append them.
//!
//! The engine has been able to do this from the start — `Hooks::feed` runs
//! once per frame precisely so a scene can append points, and `extend_xy` is
//! O(new points) — but the CLI's front door could not: `input::load` reads
//! stdin to EOF before drawing anything, so `tail -f x.log | plotui line`
//! waits forever. This is the reader that keeps the door open.
//!
//! Lines are read on their own thread and handed over a channel, so a feed
//! that says nothing for a minute never stalls the frame loop, and a burst
//! never blocks the reader. That the keyboard comes from `/dev/tty` (see the
//! `crossterm` features in Cargo.toml) is what leaves stdin free to be data
//! in the first place.

use std::io::BufRead;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use plotui_core::{Plot, TraceId};

#[cfg(test)]
use crate::input::Table;
use crate::input::{self, Row, Schema};
use crate::ChartKind;

/// Most rows appended in one frame. A producer faster than the display gets
/// drained over several frames rather than holding one open: falling behind
/// by a frame is invisible, a frame that never ends is not.
const MAX_ROWS_PER_FRAME: usize = 20_000;

/// A live feed of rows, appended to the plot one frame at a time.
pub struct Follower {
    rx: Receiver<String>,
    schema: Schema,
    /// One trace handle per series, in column order.
    handles: Vec<TraceId>,
    /// Histograms read their column as a sample rather than as coordinates,
    /// so they take the value alone and re-bin themselves.
    samples_only: bool,
    lineno: usize,
    /// The first line the feed could not parse, and how many were skipped in
    /// total. Held rather than printed: a live chart owns the terminal it is
    /// drawing on, so the report waits for the session to end.
    first_error: Option<String>,
    skipped: usize,
    /// The writer closed the pipe. The plot stays up and interactive — a
    /// finished run is still worth reading.
    ended: bool,
}

/// Read stdin on a thread, one line at a time, forwarding to `tx` until the
/// pipe closes or the plot goes away.
fn spawn_reader() -> Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            // A read error or a hung-up receiver both mean: stop.
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    rx
}

/// Open a follow session: block until the stream has said enough to settle
/// its shape — the header row when one is asked for, then the first data row
/// — then build the plot from what has arrived so far and keep the rest of
/// the stream for [`Follower::drain`].
///
/// Blocking here is the honest behaviour: a feed that has not spoken yet has
/// not said how many columns it has, and guessing would mean redrawing the
/// whole chart the moment it did.
pub fn start(
    kind: ChartKind,
    delimiter: Option<&str>,
    header: bool,
) -> Result<(Plot, Follower), String> {
    let rx = spawn_reader();
    let want = if header { 2 } else { 1 };
    let (mut text, mut seen, mut lineno) = (String::new(), 0usize, 0usize);
    while seen < want {
        let Ok(line) = rx.recv() else {
            return Err(if lineno == 0 {
                "no data on input".into()
            } else {
                "no data on input (only a header row)".into()
            });
        };
        lineno += 1;
        if !line.trim().is_empty() {
            seen += 1;
        }
        text.push_str(&line);
        text.push('\n');
    }
    let table = input::parse(&text, delimiter, header)?;
    let plot = crate::build::build_plot(kind, &table);
    let follower = Follower {
        rx,
        // Trace handles are pushed in column order, so series `i` is trace
        // `i` — the same order `build_plot` walks.
        handles: (0..table.series.len()).collect(),
        schema: table.schema,
        samples_only: matches!(kind, ChartKind::Hist { .. }),
        lineno,
        first_error: None,
        skipped: 0,
        ended: false,
    };
    Ok((plot, follower))
}

impl Follower {
    /// For tests: a follower over an arbitrary line channel.
    #[cfg(test)]
    fn from_parts(rx: Receiver<String>, table: &Table, samples_only: bool) -> Self {
        Follower {
            rx,
            handles: (0..table.series.len()).collect(),
            schema: table.schema.clone(),
            samples_only,
            lineno: 0,
            first_error: None,
            skipped: 0,
            ended: false,
        }
    }

    /// Append everything that has arrived since the last frame. Returns
    /// whether the plot changed, which is what tells the host to repaint.
    pub fn drain(&mut self, plot: &mut Plot) -> bool {
        let mut rows: Vec<Row> = Vec::new();
        while rows.len() < MAX_ROWS_PER_FRAME {
            match self.rx.try_recv() {
                Ok(line) => {
                    self.lineno += 1;
                    if line.trim().is_empty() {
                        continue;
                    }
                    match self.schema.parse_row(&line, self.lineno, false) {
                        Ok(row) => rows.push(row),
                        // A live feed is not a file: one malformed line is a
                        // hiccup in the producer, not a reason to tear the
                        // chart down. Skip it, count it, report at the end.
                        Err(e) => {
                            self.skipped += 1;
                            self.first_error.get_or_insert(e);
                        }
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.ended = true;
                    break;
                }
            }
        }
        if rows.is_empty() {
            return false;
        }
        // A dated x column only learns its epoch base from the first row it
        // ever parses, which may be this one.
        if plot.x_epoch.is_none() {
            plot.x_epoch = self.schema.x_epoch();
        }
        let xs: Vec<f32> = rows.iter().map(|r| r.x).collect();
        for (i, &h) in self.handles.iter().enumerate() {
            let ys: Vec<f32> = rows.iter().filter_map(|r| r.ys.get(i).copied()).collect();
            let appended = if self.samples_only {
                plot.extend_values(h, &ys)
            } else {
                plot.extend_xy(h, &xs[..ys.len()], &ys)
            };
            // The handles came from `build_plot` a moment ago, so a rejection
            // here is a bug in this file, not bad input.
            debug_assert!(appended.is_ok(), "follow: {appended:?}");
        }
        true
    }

    /// Has the writer closed the pipe?
    pub fn ended(&self) -> bool {
        self.ended
    }

    /// What to tell the user once the terminal is theirs again: the first
    /// line that would not parse, and how many the feed dropped in total.
    pub fn report(&self) -> Option<String> {
        let first = self.first_error.as_ref()?;
        Some(match self.skipped {
            1 => format!("skipped 1 unparsable line — {first}"),
            n => format!("skipped {n} unparsable lines — first was {first}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plotui_core::Trace;

    fn table(text: &str) -> Table {
        input::parse(text, None, false).unwrap()
    }

    /// The length of the first trace's x column — what a follower grows.
    fn len_of(plot: &Plot) -> usize {
        match &plot.traces[0] {
            Trace::Line2d { xs, .. } | Trace::Scatter2d { xs, .. } => xs.len(),
            Trace::Histogram2d { values, .. } => values.len(),
            _ => panic!("the built plot's first trace is not one a feed grows"),
        }
    }

    fn feed(lines: &[&str]) -> Receiver<String> {
        let (tx, rx) = mpsc::channel();
        for l in lines {
            tx.send((*l).to_string()).unwrap();
        }
        rx
    }

    #[test]
    fn rows_arriving_later_extend_the_series() {
        let t = table("1 10\n2 20\n");
        let mut plot = crate::build::build_plot(ChartKind::Line, &t);
        let mut f = Follower::from_parts(feed(&["3 30", "4 40"]), &t, false);
        assert_eq!(len_of(&plot), 2);
        assert!(f.drain(&mut plot), "new rows are a change");
        assert_eq!(len_of(&plot), 4);
        // Drained dry, and the sender is gone: nothing more to do, and the
        // plot stays up rather than the session ending with the pipe.
        assert!(!f.drain(&mut plot));
        assert!(f.ended());
    }

    #[test]
    fn every_series_grows_together() {
        let t = table("1 10 100\n2 20 200\n");
        let mut plot = crate::build::build_plot(ChartKind::Line, &t);
        let mut f = Follower::from_parts(feed(&["3 30 300"]), &t, false);
        f.drain(&mut plot);
        for (i, tr) in plot.traces.iter().enumerate() {
            let Trace::Line2d { xs, ys, .. } = tr else { panic!() };
            assert_eq!(xs.len(), 3, "series {i} x");
            assert_eq!(ys.len(), 3, "series {i} y");
        }
    }

    #[test]
    fn a_single_column_feed_keeps_counting_x() {
        let t = table("5\n7\n");
        let mut plot = crate::build::build_plot(ChartKind::Line, &t);
        let mut f = Follower::from_parts(feed(&["9", "11"]), &t, false);
        f.drain(&mut plot);
        let Trace::Line2d { xs, ys, .. } = &plot.traces[0] else { panic!() };
        // The index continues from the batch rather than restarting at 0.
        assert_eq!(*xs, vec![0.0, 1.0, 2.0, 3.0]);
        assert_eq!(*ys, vec![5.0, 7.0, 9.0, 11.0]);
    }

    #[test]
    fn a_bad_line_is_skipped_not_fatal() {
        let t = table("1 10\n");
        let mut plot = crate::build::build_plot(ChartKind::Line, &t);
        let mut f = Follower::from_parts(feed(&["2 20", "oops", "3 30", "4 5 6"]), &t, false);
        assert!(f.drain(&mut plot));
        // The two good rows land; the garbage and the ragged row do not.
        assert_eq!(len_of(&plot), 3);
        let report = f.report().expect("a skipped line is reported");
        assert!(report.starts_with("skipped 2 unparsable lines"), "{report}");
        // The one it quotes is the *first*, and it names the line it was on.
        assert!(report.contains("line 2:"), "{report}");
    }

    #[test]
    fn blank_lines_are_ignored() {
        let t = table("1 10\n");
        let mut plot = crate::build::build_plot(ChartKind::Line, &t);
        let mut f = Follower::from_parts(feed(&["", "   ", "2 20"]), &t, false);
        f.drain(&mut plot);
        assert_eq!(len_of(&plot), 2);
        assert!(f.report().is_none(), "blank lines are not errors");
    }

    #[test]
    fn a_histogram_feed_appends_samples() {
        let t = table("3\n5\n");
        let mut plot =
            crate::build::build_plot(ChartKind::Hist { bins: None, bin_width: None }, &t);
        let mut f = Follower::from_parts(feed(&["7", "9"]), &t, true);
        assert!(f.drain(&mut plot));
        assert_eq!(len_of(&plot), 4, "samples accumulate and the bins resolve again");
    }
}
