//! Widget tests: rendered buffers and synthesized crossterm events —
//! structural assertions in the style of the core/protocol suites.

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use plotui_core::YAxis;
use plotui_ratatui::{ElementKind, PlotEvent, PlotOptions, PlotState, PlotWidget, RenderMode};
use plotui_term::policy::pixel_geometry;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::widgets::StatefulWidget;

const AREA: Rect = Rect { x: 0, y: 0, width: 20, height: 10 };
const CELL_PX: (u16, u16) = (8, 16);

fn opts(mode: RenderMode) -> PlotOptions {
    PlotOptions {
        cell_px: Some(CELL_PX),
        render_mode: Some(mode),
        image_id: Some(777),
        ..Default::default()
    }
}

fn plot_2d() -> plotui_core::Plot {
    let mut p = plotui_core::Plot::new();
    let color = p.resolve_color(None);
    p.add_line2d(vec![0.0, 1.0, 2.0], vec![0.0, 2.0, 1.0], color, 2.0, None, YAxis::Primary);
    p
}

fn plot_3d(n: usize) -> plotui_core::Plot {
    let mut p = plotui_core::Plot::new();
    let pts: Vec<[f32; 3]> = (0..n)
        .map(|i| {
            let t = i as f32 / n as f32;
            [(t * 20.0).cos(), t * 2.0 - 1.0, (t * 20.0).sin()]
        })
        .collect();
    p.add_scatter3d(pts, [230, 60, 120], 2.0, None);
    p
}

fn render(state: &mut PlotState) -> Buffer {
    let mut buf = Buffer::empty(AREA);
    PlotWidget.render(AREA, &mut buf, state);
    buf
}

fn mouse(kind: MouseEventKind, column: u16, row: u16, modifiers: KeyModifiers) -> Event {
    Event::Mouse(MouseEvent { kind, column, row, modifiers })
}

#[test]
fn placeholder_mode_fills_the_area_with_addressed_cells() {
    let mut state = PlotState::new(plot_2d(), opts(RenderMode::Placeholder));
    let buf = render(&mut state);

    // The top-left cell carries the transmit escape ahead of its placeholder.
    let first = buf[(0, 0)].symbol();
    assert!(first.contains("\x1b_G"), "transmit escape embedded in cell (0,0)");
    assert!(first.contains("U=1"), "virtual placement");
    assert!(first.contains("i=777,"), "custom image id");
    // Every cell shows the placeholder char with the id-encoding foreground.
    for y in 0..AREA.height {
        for x in 0..AREA.width {
            let cell = &buf[(x, y)];
            assert!(cell.symbol().contains('\u{10EEEE}'), "cell ({x},{y}) is a placeholder");
            assert_eq!(cell.fg, Color::Rgb(0x00, 0x03, 0x09), "777 = 0x0309 in the fg");
        }
    }
    // Distinct column diacritics along a row (self-addressed cells).
    let marks: Vec<char> =
        (0..AREA.width).map(|x| buf[(x, 1)].symbol().chars().nth(2).unwrap()).collect();
    let mut dedup = marks.clone();
    dedup.dedup();
    assert_eq!(dedup.len(), AREA.width as usize);
}

#[test]
fn overlay_spans_splice_into_placeholder_rows() {
    let mut state = PlotState::new(plot_2d(), opts(RenderMode::Placeholder));
    let style = ratatui::style::Style::default().fg(Color::Yellow);
    state.set_overlay(vec![
        plotui_ratatui::OverlaySpan { row: 1, col: 2, text: "hi".into(), style },
        // Overlapping span: first one wins, this is dropped.
        plotui_ratatui::OverlaySpan { row: 1, col: 3, text: "X".into(), style },
    ]);
    let buf = render(&mut state);
    assert_eq!(buf[(2, 1)].symbol(), "h");
    assert_eq!(buf[(3, 1)].symbol(), "i", "the overlapping span lost");
    assert_eq!(buf[(2, 1)].fg, Color::Yellow);
    // Cells after the gap are still placeholders with intact addressing.
    assert!(buf[(4, 1)].symbol().contains('\u{10EEEE}'));
    // A different row is untouched.
    assert!(buf[(2, 2)].symbol().contains('\u{10EEEE}'));
}

#[test]
fn direct_mode_embeds_the_escape_and_leaves_cells_blank() {
    let mut state = PlotState::new(plot_2d(), opts(RenderMode::Direct));
    let buf = render(&mut state);
    let first = buf[(0, 0)].symbol();
    assert!(first.contains("a=T"), "transmit+display escape at the origin");
    assert!(first.contains("c=20,r=10"), "spans the widget's cell region");
    assert!(first.contains("i=777"), "custom image id");
    assert!(first.contains("a=d"), "delete-first by default (iTerm2 stacking)");
    assert!(first.ends_with(' '), "the visible glyph under the escape is a space");
    assert_eq!(buf[(1, 0)].symbol(), " ", "other cells stay blank");
}

#[test]
fn unsupported_mode_centers_the_notice() {
    let mut state = PlotState::new(plot_2d(), opts(RenderMode::Unsupported));
    // Wide enough for the notice lines (narrower widgets truncate them).
    let area = Rect { x: 0, y: 0, width: 80, height: 10 };
    let mut buf = Buffer::empty(area);
    PlotWidget.render(area, &mut buf, &mut state);
    let rows: Vec<String> = (0..area.height)
        .map(|y| (0..area.width).map(|x| buf[(x, y)].symbol().to_string()).collect())
        .collect();
    let all = rows.join("\n");
    assert!(all.contains("Kitty graphics"), "notice names the protocol");
    assert!(rows[0].trim().is_empty() && rows[9].trim().is_empty(), "vertically centered");
    // No image escapes and no placeholders in unsupported mode.
    assert!(!all.contains("\x1b_G") && !all.contains('\u{10EEEE}'));
    // Events are ignored entirely.
    state.handle_event(&mouse(MouseEventKind::ScrollUp, 5, 5, KeyModifiers::NONE));
    assert_eq!(state.plot().camera.zoom, 1.0);
}

#[test]
fn drag_rotates_and_shift_drag_pans() {
    let mut state = PlotState::new(plot_3d(50), opts(RenderMode::Placeholder));
    render(&mut state); // establishes the hit-test area
    let (yaw0, pitch0, ..) = state.plot().camera.state();

    state.handle_event(&mouse(MouseEventKind::Down(MouseButton::Left), 5, 5, KeyModifiers::NONE));
    state.handle_event(&mouse(MouseEventKind::Drag(MouseButton::Left), 8, 5, KeyModifiers::NONE));
    let (yaw, pitch, ..) = state.plot().camera.state();
    // Trackball direction: dragging right turns the object right (yaw −).
    assert!((yaw0 - yaw - 3.0 * 0.03).abs() < 1e-9, "3 cells of drag = 3 * 0.03 rad of yaw");
    assert_eq!(pitch, pitch0);
    assert!(state.dragging());
    state.handle_event(&mouse(MouseEventKind::Up(MouseButton::Left), 8, 5, KeyModifiers::NONE));
    assert!(!state.dragging());

    state.handle_event(&mouse(MouseEventKind::Down(MouseButton::Left), 5, 5, KeyModifiers::NONE));
    state.handle_event(&mouse(MouseEventKind::Drag(MouseButton::Left), 7, 6, KeyModifiers::SHIFT));
    let (.., pan_x, pan_y) = state.plot().camera.state();
    assert_eq!(pan_x, 2.0 * CELL_PX.0 as f64, "pan moves in cell-pixel steps");
    assert_eq!(pan_y, 1.0 * CELL_PX.1 as f64);
}

#[test]
fn scroll_and_keys_zoom_rotate_pan_reset() {
    let mut state = PlotState::new(plot_3d(50), opts(RenderMode::Placeholder));
    render(&mut state);

    state.handle_event(&mouse(MouseEventKind::ScrollUp, 5, 5, KeyModifiers::NONE));
    assert!((state.plot().camera.zoom - 1.1).abs() < 1e-9);
    state.handle_event(&mouse(MouseEventKind::ScrollDown, 5, 5, KeyModifiers::NONE));
    // Scrolls outside the widget's area are ignored.
    state.handle_event(&mouse(MouseEventKind::ScrollUp, 50, 50, KeyModifiers::NONE));
    assert!((state.plot().camera.zoom - 1.1 * 0.9).abs() < 1e-9);

    let yaw0 = state.plot().camera.yaw;
    let key = |code, mods| Event::Key(KeyEvent::new(code, mods));
    // Trackball signs: Left turns the object left (yaw +), matching drag.
    state.handle_event(&key(KeyCode::Left, KeyModifiers::NONE));
    assert!((state.plot().camera.yaw - yaw0 - 0.1).abs() < 1e-9);
    state.handle_event(&key(KeyCode::Up, KeyModifiers::SHIFT));
    assert_eq!(state.plot().camera.pan_y, -2.0 * CELL_PX.1 as f64);
    state.handle_event(&key(KeyCode::Char('+'), KeyModifiers::NONE));
    state.handle_event(&key(KeyCode::Char('r'), KeyModifiers::NONE));
    let default = plotui_core::Camera::default().state();
    assert_eq!(state.plot().camera.state(), default, "r restores the default view");
}

#[test]
fn click_without_motion_picks() {
    // pickable=false: clicks resolve against nodes only.
    let mut state = PlotState::new(plot_3d(50), opts(RenderMode::Placeholder));
    render(&mut state);
    state.handle_event(&mouse(MouseEventKind::Down(MouseButton::Left), 1, 1, KeyModifiers::NONE));
    let ev =
        state.handle_event(&mouse(MouseEventKind::Up(MouseButton::Left), 1, 1, KeyModifiers::NONE));
    assert!(matches!(ev, Some(PlotEvent::NodePicked(_))), "click emits a pick event");

    // A drag is not a click.
    state.handle_event(&mouse(MouseEventKind::Down(MouseButton::Left), 1, 1, KeyModifiers::NONE));
    state.handle_event(&mouse(MouseEventKind::Drag(MouseButton::Left), 4, 1, KeyModifiers::NONE));
    let ev =
        state.handle_event(&mouse(MouseEventKind::Up(MouseButton::Left), 4, 1, KeyModifiers::NONE));
    assert!(ev.is_none(), "a completed drag emits no pick");
}

#[test]
fn pickable_hover_and_click_report_elements_once_per_change() {
    let mut state = PlotState::new(
        plot_3d(50),
        PlotOptions { pickable: true, ..opts(RenderMode::Placeholder) },
    );
    render(&mut state);

    // Find a cell over a real node from the exact projection geometry.
    let (pw, ph) = (AREA.width as usize * 8, AREA.height as usize * 16);
    let projected = state.plot().project_nodes(pw, ph);
    let (nx, ny) = (projected[0][0], projected[0][1]);
    let (cx, cy) = ((nx / 8.0) as u16, (ny / 16.0) as u16);

    let ev = state.handle_event(&mouse(MouseEventKind::Moved, cx, cy, KeyModifiers::NONE));
    let Some(PlotEvent::ElementHovered(Some((ElementKind::Node, _)))) = ev else {
        panic!("hovering a node must report it, got {ev:?}");
    };
    // Same position again: no change, no event.
    let ev = state.handle_event(&mouse(MouseEventKind::Moved, cx, cy, KeyModifiers::NONE));
    assert!(ev.is_none(), "hover reports only changes");
    // Leaving the widget clears the hover.
    let ev = state.handle_event(&mouse(MouseEventKind::Moved, 50, 50, KeyModifiers::NONE));
    assert!(matches!(ev, Some(PlotEvent::ElementHovered(None))));

    // A click on the node selects and reports it.
    state.handle_event(&mouse(MouseEventKind::Down(MouseButton::Left), cx, cy, KeyModifiers::NONE));
    let ev = state.handle_event(&mouse(
        MouseEventKind::Up(MouseButton::Left),
        cx,
        cy,
        KeyModifiers::NONE,
    ));
    assert!(matches!(ev, Some(PlotEvent::ElementPicked(Some((ElementKind::Node, _))))));
    assert!(state.plot().selected.is_some());
}

#[test]
fn large_3d_plots_render_half_res_only_mid_drag() {
    let mut state = PlotState::new(plot_3d(500), opts(RenderMode::Placeholder));
    let buf = render(&mut state);
    assert!(buf[(0, 0)].symbol().contains("s=160,v=160"), "still plots render full-res");

    state.handle_event(&mouse(MouseEventKind::Down(MouseButton::Left), 5, 5, KeyModifiers::NONE));
    state.handle_event(&mouse(MouseEventKind::Drag(MouseButton::Left), 8, 5, KeyModifiers::NONE));
    let buf = render(&mut state);
    let first = buf[(0, 0)].symbol();
    assert!(first.contains("s=80,v=80"), "mid-drag frames render at half resolution");
    assert!(first.contains("c=20,r=10"), "but still span the full cell region");

    state.handle_event(&mouse(MouseEventKind::Up(MouseButton::Left), 8, 5, KeyModifiers::NONE));
    let buf = render(&mut state);
    assert!(buf[(0, 0)].symbol().contains("s=160,v=160"), "full resolution snaps back");

    // A small plot never reduces.
    let mut small = PlotState::new(plot_3d(50), opts(RenderMode::Placeholder));
    render(&mut small);
    small.handle_event(&mouse(MouseEventKind::Down(MouseButton::Left), 5, 5, KeyModifiers::NONE));
    small.handle_event(&mouse(MouseEventKind::Drag(MouseButton::Left), 8, 5, KeyModifiers::NONE));
    let buf = render(&mut small);
    assert!(buf[(0, 0)].symbol().contains("s=160,v=160"));
}

#[test]
fn crosshair_hover_invalidates_2d_plots() {
    let mut state = PlotState::new(plot_2d(), opts(RenderMode::Placeholder));
    render(&mut state);
    assert!(!state.needs_redraw(), "render consumed the initial redraw flag");
    state.handle_event(&mouse(MouseEventKind::Moved, 5, 5, KeyModifiers::NONE));
    assert!(state.needs_redraw(), "crosshair moves repaint");
    state.handle_event(&mouse(MouseEventKind::Moved, 5, 5, KeyModifiers::NONE));
    assert!(!state.needs_redraw(), "same position: no repaint");
    // Leaving clears the crosshair (a repaint) exactly once.
    state.handle_event(&mouse(MouseEventKind::Moved, 50, 50, KeyModifiers::NONE));
    assert!(state.needs_redraw());
    state.handle_event(&mouse(MouseEventKind::Moved, 50, 50, KeyModifiers::NONE));
    assert!(!state.needs_redraw());
}

#[test]
fn streaming_extend_and_visibility_toggle_repaint() {
    let mut plot = plotui_core::Plot::new();
    let color = plot.resolve_color(None);
    let h = plot.add_line2d(vec![], vec![], color, 2.0, Some("s".into()), YAxis::Primary);
    let mut state = PlotState::new(plot, opts(RenderMode::Placeholder));
    render(&mut state);
    state.needs_redraw();

    state.extend(h, &[0.0, 1.0], &[1.0, 2.0], None).unwrap();
    assert!(state.needs_redraw());
    assert!(state.set_visible(h, false));
    assert!(!state.set_visible(h, false), "no-op toggles report unchanged");
    assert!(state.extend(999, &[0.0], &[0.0], None).is_err(), "unknown handles error");
}

#[test]
fn transmit_reemits_when_the_frame_changes() {
    let mut state = PlotState::new(plot_3d(50), opts(RenderMode::Placeholder));
    let buf1 = render(&mut state);
    let first1 = buf1[(0, 0)].symbol().to_string();
    // Unchanged frame: identical symbol (the diff would skip it).
    let buf2 = render(&mut state);
    assert_eq!(buf2[(0, 0)].symbol(), first1);
    // A camera change produces a different payload → the diff re-emits.
    state.plot_mut().camera.rotate(0.5, 0.0);
    let buf3 = render(&mut state);
    assert_ne!(buf3[(0, 0)].symbol(), first1);
}

// --- the range slider ---

/// plot_2d with the strip on and a mid-data window; AREA (20x10 cells at
/// 8x16px) is a 160x160 frame — exactly the strip's minimum height.
fn range_state() -> PlotState {
    let mut plot = plot_2d();
    plot.range_slider = true;
    plot.x_window = Some((0.5, 1.5));
    PlotState::new(plot, opts(RenderMode::Placeholder))
}

#[test]
fn strip_drag_slides_the_window_and_emits_on_release() {
    let mut state = range_state();
    render(&mut state);
    assert!(!state.needs_redraw());
    // Press mid-strip (cell row 9 → pixel row 152, inside the strip band),
    // inside the window body, then drag two cells right and release.
    let down = mouse(MouseEventKind::Down(MouseButton::Left), 10, 9, KeyModifiers::NONE);
    assert_eq!(state.handle_event(&down), None);
    let drag = mouse(MouseEventKind::Drag(MouseButton::Left), 12, 9, KeyModifiers::NONE);
    assert_eq!(state.handle_event(&drag), None);
    assert!(state.needs_redraw(), "a strip drag repaints");
    let up = mouse(MouseEventKind::Up(MouseButton::Left), 12, 9, KeyModifiers::NONE);
    let ev = state.handle_event(&up);
    let Some(PlotEvent::RangeChanged(Some((lo, hi)))) = ev else {
        panic!("expected RangeChanged with a window, got {ev:?}");
    };
    assert!(lo > 0.5 && hi > 1.5, "window must have slid right, got ({lo}, {hi})");
    assert!((hi - lo - 1.0).abs() < 1e-6, "span preserved");
}

#[test]
fn plot_area_drag_pans_a_windowed_plot_instead_of_the_camera() {
    let mut state = range_state();
    render(&mut state);
    let cam_before = state.plot().camera.state();
    let down = mouse(MouseEventKind::Down(MouseButton::Left), 10, 4, KeyModifiers::NONE);
    state.handle_event(&down);
    let drag = mouse(MouseEventKind::Drag(MouseButton::Left), 8, 4, KeyModifiers::NONE);
    state.handle_event(&drag);
    let (lo, _) = state.plot().x_window.unwrap();
    assert!(lo > 0.5, "dragging left must slide the view right (grab the data)");
    assert_eq!(state.plot().camera.state(), cam_before, "the camera stays out of it");
}

#[test]
fn scroll_zooms_the_window_about_the_cursor_and_emits() {
    let mut state = range_state();
    render(&mut state);
    let ev = state.handle_event(&mouse(MouseEventKind::ScrollUp, 10, 4, KeyModifiers::NONE));
    let Some(PlotEvent::RangeChanged(Some((lo, hi)))) = ev else {
        panic!("expected RangeChanged, got {ev:?}");
    };
    assert!(hi - lo < 1.0, "zooming in must shrink the span, got ({lo}, {hi})");
}

#[test]
fn bracket_keys_shift_and_reset_clears() {
    let mut state = range_state();
    render(&mut state);
    let key = |c| Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    let ev = state.handle_event(&key(']'));
    assert!(matches!(ev, Some(PlotEvent::RangeChanged(Some(_)))));
    let (lo, _) = state.plot().x_window.unwrap();
    assert!(lo > 0.5, "']' slides the window right");
    state.handle_event(&key('['));
    let ev = state.handle_event(&key('['));
    assert!(matches!(ev, Some(PlotEvent::RangeChanged(Some(_)))));
    let (lo2, _) = state.plot().x_window.unwrap();
    assert!(lo2 < lo, "'[' slides the window left, got {lo} -> {lo2}");
    let ev = state.handle_event(&key('r'));
    assert_eq!(ev, Some(PlotEvent::RangeChanged(None)), "reset clears the window");
    assert_eq!(state.plot().x_window, None);
}

/// A press on a legend row toggles that series instead of grabbing the
/// camera — the terminal half of the clickable legend. The row is found by
/// asking the engine where it drew the legend, so this stays honest if the
/// panel ever moves.
#[test]
fn legend_click_toggles_a_series_and_never_rotates() {
    let mut plot = plotui_core::Plot::new();
    plot.add_scatter3d(
        vec![[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]],
        [230, 60, 120],
        3.0,
        Some("alpha".into()),
    );
    let mut state = PlotState::new(plot, opts(RenderMode::Placeholder));
    render(&mut state);

    // Find a cell whose pixel centre lands on the legend row, through the
    // same geometry the event handler uses.
    let mut cell = None;
    'outer: for row in 0..AREA.height {
        for col in 0..AREA.width {
            let (pw, ph, px, py, _) =
                pixel_geometry(AREA.width, AREA.height, CELL_PX.0, CELL_PX.1, col, row);
            if state.plot().legend_hit(pw, ph, px, py).is_some() {
                cell = Some((col, row));
                break 'outer;
            }
        }
    }
    let (col, row) = cell.expect("the legend must be reachable from some cell");

    let yaw = state.plot().camera.yaw;
    let ev = state.handle_event(&mouse(
        MouseEventKind::Down(MouseButton::Left),
        AREA.x + col,
        AREA.y + row,
        KeyModifiers::NONE,
    ));
    assert!(matches!(ev, Some(PlotEvent::LegendToggled(_, false))), "the press mutes the series");

    // The press never became a drag, so dragging on from it cannot rotate.
    state.handle_event(&mouse(
        MouseEventKind::Drag(MouseButton::Left),
        AREA.x + col + 3,
        AREA.y + row,
        KeyModifiers::NONE,
    ));
    assert_eq!(state.plot().camera.yaw, yaw, "a legend press must not grab the camera");

    let ev = state.handle_event(&mouse(
        MouseEventKind::Down(MouseButton::Left),
        AREA.x + col,
        AREA.y + row,
        KeyModifiers::NONE,
    ));
    assert!(matches!(ev, Some(PlotEvent::LegendToggled(_, true))), "a second press brings it back");
}
