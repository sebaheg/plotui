//! `plotui dag [FILE]` — draw a DOT file. Reads stdin when no file is given,
//! so a pipeline definition can be piped straight in:
//!
//!     dot -Tdot my_dag.dot | plotui dag
//!     plotui dag pipeline.dot --rankdir lr
//!
//! Interactively, hovering a task lights everything it waits on and dims the
//! rest; the readout names the task. That is the whole reason to draw a DAG
//! in a terminal rather than read its edge list.

use std::io::{IsTerminal, Read};
use std::path::Path;
use std::process::ExitCode;

use plotui_bind::plot_from_dot;
use plotui_core::{reachable, Direction, Plot, RankDir, Rgb, TraceId};
use plotui_ratatui::{ElementKind, OverlaySpan, PlotEvent, PlotState};
use plotui_term::RenderMode;
use ratatui::style::{Color, Style};

use crate::interactive::{self, Hooks};
use crate::{record, render, DagArgs};

/// Read the document: a path, or stdin for `-` and for no argument at all —
/// the same rule [`crate::input::load`] applies to data files.
fn read_input(file: Option<&Path>) -> std::io::Result<String> {
    match file {
        Some(p) if p.as_os_str() != "-" => std::fs::read_to_string(p),
        _ => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            Ok(s)
        }
    }
}

pub fn run(args: &DagArgs) -> ExitCode {
    let rankdir = match args.rankdir.as_deref() {
        None => None,
        Some(name) => match RankDir::parse(name) {
            Some(d) => Some(d),
            None => {
                eprintln!(
                    "plotui: unknown rankdir {name:?}; expected one of {}",
                    RankDir::NAMES.join(", ")
                );
                return ExitCode::from(2);
            }
        },
    };
    let text = match read_input(args.file.as_deref()) {
        Ok(t) => t,
        Err(e) => {
            let what = args.file.as_deref().map_or("stdin".into(), |p| p.display().to_string());
            eprintln!("plotui: {what}: {e}");
            return ExitCode::from(2);
        }
    };
    // A parse error is the user's file, not a plotui failure, so it exits 2
    // like a malformed data file does and prints the message verbatim —
    // `line:col` included, so an editor can jump to it.
    let (plot, handle, doc) = match plot_from_dot(&text, rankdir) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("plotui: {e}");
            return ExitCode::from(2);
        }
    };

    if let Some(out) = &args.out {
        return match record::record_static(&plot, out, args.size.0.into(), args.size.1.into()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("plotui: {e}");
                ExitCode::FAILURE
            }
        };
    }

    let mode = plotui_term::detect_render_mode();
    if mode == RenderMode::Unsupported {
        for line in plotui_term::policy::UNSUPPORTED_MESSAGE {
            eprintln!("{line}");
        }
        return ExitCode::from(3);
    }

    let result = if args.static_mode || !std::io::stdout().is_terminal() {
        render::render_static(&plot, mode, args.width, args.height)
    } else {
        let names: Vec<String> = doc.nodes.iter().map(|n| n.label.clone()).collect();
        let edges: Vec<(u32, u32)> = doc.edges.iter().map(|e| (e.from, e.to)).collect();
        let base: Vec<Rgb> = match &plot.traces[handle] {
            plotui_core::Trace::Graph2d { node_colors, .. } => node_colors.clone(),
            _ => Vec::new(),
        };
        let hooks = Hooks {
            pickable: true,
            on_plot_event: Some(Box::new(move |state: &mut PlotState, ev: PlotEvent| match ev {
                PlotEvent::ElementHovered(Some((ElementKind::Node, i))) if i < names.len() => {
                    highlight(state.plot_mut(), handle, &base, &edges, Some(i));
                    let waits = reachable(names.len(), &edges, i, Direction::Upstream)
                        .iter()
                        .filter(|&&x| x)
                        .count()
                        - 1;
                    state.set_overlay(vec![OverlaySpan {
                        row: 0,
                        col: 2,
                        text: format!(" {} · waits on {waits} ", names[i]),
                        style: Style::default().fg(Color::Rgb(205, 210, 220)),
                    }]);
                }
                PlotEvent::ElementHovered(_) => {
                    highlight(state.plot_mut(), handle, &base, &edges, None);
                    state.set_overlay(Vec::new());
                }
                _ => {}
            })),
            ..Default::default()
        };
        interactive::run_with(plot, mode, args.width, args.height, hooks)
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("plotui: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Light the hovered node's upstream set and dim everything else; `None`
/// restores the document's own colours.
fn highlight(
    plot: &mut Plot,
    handle: TraceId,
    base: &[Rgb],
    edges: &[(u32, u32)],
    hovered: Option<usize>,
) {
    let (nodes, edge_colors) = match hovered {
        None => (base.to_vec(), None),
        Some(i) => {
            let on = reachable(base.len(), edges, i, Direction::Upstream);
            let nodes =
                base.iter().enumerate().map(|(j, c)| if on[j] { *c } else { dim(*c) }).collect();
            let lit =
                edges
                    .iter()
                    .map(|&(a, b)| {
                        if on[a as usize] && on[b as usize] {
                            [170, 175, 185]
                        } else {
                            [40, 45, 60]
                        }
                    })
                    .collect();
            (nodes, Some(lit))
        }
    };
    plot.set_graph_colors(handle, nodes, edge_colors).expect("graph handle");
}

fn dim(c: Rgb) -> Rgb {
    [
        (c[0] as f32 * 0.32 + 12.0) as u8,
        (c[1] as f32 * 0.32 + 14.0) as u8,
        (c[2] as f32 * 0.32 + 20.0) as u8,
    ]
}
