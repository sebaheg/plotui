//! Behavior tests for the rendering engine, written against the public API.
//!
//! Golden values are structural (pixel counts, specific probed pixels, hashes
//! compared within one process) rather than hard-coded image hashes, so they
//! hold on any platform regardless of libm rounding in the camera trig.

use plotui_core::{
    draw_text, nice_ticks, Element, Framebuffer, NodeShape, Plot, TraceError, YAxis, PALETTE,
};

/// FNV-1a over the RGBA buffer — stable fingerprint for same-process compares.
fn hash(fb: &Framebuffer) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for b in fb.rgba() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn drawn_pixels(fb: &Framebuffer) -> Vec<(usize, usize, [u8; 3])> {
    let rgba = fb.rgba();
    let mut v = Vec::new();
    for y in 0..fb.h {
        for x in 0..fb.w {
            let i = (y * fb.w + x) * 4;
            if rgba[i + 3] > 0 {
                v.push((x, y, [rgba[i], rgba[i + 1], rgba[i + 2]]));
            }
        }
    }
    v
}

fn has_color(fb: &Framebuffer, c: [u8; 3]) -> bool {
    drawn_pixels(fb).iter().any(|(_, _, px)| *px == c)
}

fn demo_3d() -> Plot {
    let mut p = Plot::new();
    p.add_scatter3d(
        vec![[0.0, 0.0, 0.0], [1.0, 2.0, 3.0], [-2.0, 1.0, -1.0], [3.0, -1.0, 2.0]],
        [230, 60, 120],
        3.0,
        None,
    );
    p
}

fn demo_2d() -> Plot {
    let mut p = Plot::new();
    let xs: Vec<f32> = (0..=20).map(|i| i as f32).collect();
    let ys: Vec<f32> = xs.iter().map(|x| (x * 0.5).sin() * 3.0 + 5.0).collect();
    p.add_line2d(xs, ys, PALETTE[0], 2.0, Some("signal".into()), YAxis::Primary);
    p
}

// --- framebuffer primitives ---

#[test]
fn zbuffer_keeps_the_closer_write_regardless_of_order() {
    for order in [[(0.0f32, [1, 1, 1]), (5.0, [2, 2, 2])], [(5.0, [2, 2, 2]), (0.0, [1, 1, 1])]] {
        let mut fb = Framebuffer::new(4, 4);
        for (z, c) in order {
            fb.disc(1.5, 1.5, z, 0.6, c);
        }
        assert!(has_color(&fb, [1, 1, 1]), "closer write must win");
        assert!(!has_color(&fb, [2, 2, 2]), "farther write must lose");
    }
}

#[test]
fn drawing_out_of_bounds_is_safe_and_clipped() {
    let mut fb = Framebuffer::new(10, 10);
    fb.disc(-5.0, -5.0, 0.0, 3.0, [9, 9, 9]);
    fb.disc(100.0, 100.0, 0.0, 3.0, [9, 9, 9]);
    fb.line([-50.0, 5.0, 0.0], [50.0, 5.0, 0.0], [7, 7, 7]);
    // The horizontal line crosses the buffer; everything else is outside.
    assert!(drawn_pixels(&fb).iter().all(|(_, y, c)| *y == 5 && *c == [7, 7, 7]));
}

#[test]
fn clip_rect_confines_drawing() {
    let mut fb = Framebuffer::new(20, 20);
    fb.set_clip(5, 5, 10, 10);
    fb.rect_fill(0, 0, 19, 19, 0.0, [3, 3, 3]);
    fb.clear_clip();
    let px = drawn_pixels(&fb);
    assert_eq!(px.len(), 36);
    assert!(px.iter().all(|(x, y, _)| (5..=10).contains(x) && (5..=10).contains(y)));
}

#[test]
fn undrawn_pixels_are_transparent() {
    let mut fb = Framebuffer::new(8, 8);
    // Centered on a pixel center so a sub-pixel radius still covers it.
    fb.disc(2.5, 2.5, 0.0, 0.6, [200, 100, 50]);
    let rgba = fb.rgba();
    assert_eq!(rgba.len(), 8 * 8 * 4);
    let opaque = rgba.chunks(4).filter(|p| p[3] == 255).count();
    let transparent = rgba.chunks(4).filter(|p| p[3] == 0).count();
    assert_eq!(opaque + transparent, 64, "alpha must be fully on or off");
    assert!(opaque >= 1);
}

// --- 3D path ---

#[test]
fn render_is_deterministic() {
    let p = demo_3d();
    assert_eq!(hash(&p.render(320, 200)), hash(&p.render(320, 200)));
    let p2 = demo_2d();
    assert_eq!(hash(&p2.render(320, 200)), hash(&p2.render(320, 200)));
    let p3 = demo_graph2d();
    assert_eq!(hash(&p3.render(320, 200)), hash(&p3.render(320, 200)));
}

#[test]
fn pitch_is_a_turntable_orbit_never_a_sideways_tumble() {
    // A vertical drag must change elevation only: the world up-axis stays on
    // the same screen column through any pitch, at any yaw. (Pitch-first
    // rotation order used to tumble the scene around the data x-axis, which
    // skewed vertical drags sideways at nonzero yaw.)
    let mut p = Plot::new();
    p.add_scatter3d(
        vec![[0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [1.0, 0.0, 1.0]],
        [255, 0, 0],
        2.0,
        None,
    );
    for yaw in [-0.9, 0.0, 0.6, 2.3] {
        p.camera.reset();
        p.camera.rotate(yaw - p.camera.yaw, 0.0);
        let x_before = p.project_nodes(400, 400)[0][0];
        p.camera.rotate(0.0, 0.35);
        let after = p.project_nodes(400, 400);
        assert!(
            (after[0][0] - x_before).abs() < 1e-3,
            "up-axis tip drifted sideways under pitch at yaw {yaw}: {x_before} -> {}",
            after[0][0]
        );
    }
}

#[test]
fn camera_moves_change_the_frame_and_reset_restores_it() {
    let mut p = demo_3d();
    let before = hash(&p.render(320, 200));
    p.camera.rotate(0.3, 0.2);
    assert_ne!(hash(&p.render(320, 200)), before, "rotation must change pixels");
    p.camera.reset();
    assert_eq!(hash(&p.render(320, 200)), before, "reset must restore the frame");
}

#[test]
fn empty_plot_renders_without_panicking() {
    let p = Plot::new();
    let fb = p.render(100, 60);
    assert_eq!(fb.rgba().len(), 100 * 60 * 4);
    // Tiny buffers must not panic either.
    let _ = demo_2d().render(1, 1);
    let _ = demo_2r().render(1, 1);
    let _ = demo_3d().render(1, 1);
}

// --- 2D path ---

#[test]
fn line2d_draws_its_series_color_and_axes_chrome() {
    let fb = demo_2d().render(400, 240);
    assert!(has_color(&fb, PALETTE[0]), "series pixels present");
    assert!(has_color(&fb, [70, 78, 96]), "frame present");
    assert!(has_color(&fb, [45, 50, 66]), "grid present");
    assert!(has_color(&fb, [150, 156, 170]), "tick labels present");
}

#[test]
fn named_trace_gets_a_legend_and_unnamed_does_not() {
    let named = demo_2d().render(400, 240);
    assert!(has_color(&named, [205, 210, 220]), "legend text ink present");
    assert!(has_color(&named, [26, 30, 44]), "legend background present");

    let mut p = Plot::new();
    p.add_line2d(vec![0.0, 1.0], vec![0.0, 1.0], PALETTE[0], 2.0, None, YAxis::Primary);
    let unnamed = p.render(400, 240);
    assert!(!has_color(&unnamed, [205, 210, 220]), "no legend without names");
}

#[test]
fn bars_fill_from_the_zero_baseline() {
    let mut p = Plot::new();
    p.add_bar2d(vec![0.0, 1.0, 2.0], vec![3.0, 1.0, 2.0], PALETTE[2], None, YAxis::Primary);
    let fb = p.render(400, 240);
    let bar_px = drawn_pixels(&fb).into_iter().filter(|(_, _, c)| *c == PALETTE[2]).count();
    // Three bars on a 400x240 canvas are a large filled area, not a sliver.
    assert!(bar_px > 2000, "bars should be solid fills, got {bar_px} px");
}

#[test]
fn scatter2d_marks_all_points() {
    let mut p = Plot::new();
    p.add_scatter2d(
        vec![0.0, 5.0, 10.0],
        vec![0.0, 5.0, 10.0],
        PALETTE[5],
        3.0,
        None,
        YAxis::Primary,
    );
    let fb = p.render(400, 240);
    assert!(has_color(&fb, PALETTE[5]));
}

#[test]
fn zoom_and_pan_change_the_2d_frame() {
    let mut p = demo_2d();
    let before = hash(&p.render(400, 240));
    p.camera.zoom_by(2.0);
    let zoomed = hash(&p.render(400, 240));
    assert_ne!(zoomed, before);
    p.camera.pan(30.0, 10.0);
    assert_ne!(hash(&p.render(400, 240)), zoomed);
}

#[test]
fn data_never_bleeds_into_the_margins() {
    let mut p = demo_2d();
    p.camera.pan(-2000.0, 0.0); // shove the data far left, toward the y labels
    let fb = p.render(400, 240);
    // Left margin (label gutter) must contain no series-colored pixels.
    for (x, _, c) in drawn_pixels(&fb) {
        if c == PALETTE[0] {
            assert!(x > 20, "series pixel leaked into the margin at x={x}");
        }
    }
}

#[test]
fn non_finite_data_is_skipped_not_drawn() {
    let mut p = Plot::new();
    p.add_line2d(
        vec![0.0, 1.0, f32::NAN, 3.0, 4.0],
        vec![0.0, 1.0, 2.0, f32::INFINITY, 4.0],
        PALETTE[0],
        2.0,
        None,
        YAxis::Primary,
    );
    let fb = p.render(300, 200);
    assert!(has_color(&fb, PALETTE[0]), "finite segments still draw");
}

#[test]
fn mixed_2d_and_3d_uses_the_3d_camera_path() {
    let mut p = demo_3d();
    p.add_line2d(vec![0.0, 1.0], vec![0.0, 1.0], PALETTE[0], 2.0, Some("x".into()), YAxis::Primary);
    let fb = p.render(300, 200);
    // 3D path: no axes frame, no legend ink.
    assert!(!has_color(&fb, [205, 210, 220]));
}

// --- right-hand axes (y2/y3) ---

/// Primary ~0..1, y2 0..1000, y3 0..50000 — three scales that would flatten
/// each other to slivers if they shared an axis. Distinct shapes (sine,
/// linear, quadratic), because three full-scale straight lines would be one
/// screen diagonal and the z-buffer would keep only the last.
fn demo_2r() -> Plot {
    let xs: Vec<f32> = (0..=20).map(|i| i as f32).collect();
    let mut p = Plot::new();
    p.add_line2d(
        xs.clone(),
        xs.iter().map(|x| (x * 0.5).sin() * 0.5 + 0.5).collect(),
        PALETTE[0],
        2.0,
        Some("score".into()),
        YAxis::Primary,
    );
    p.add_line2d(
        xs.clone(),
        xs.iter().map(|x| x * 50.0).collect(),
        PALETTE[1],
        2.0,
        Some("tokens".into()),
        YAxis::Y2,
    );
    p.add_line2d(
        xs.clone(),
        xs.iter().map(|x| x * x * 125.0).collect(),
        PALETTE[2],
        2.0,
        Some("cpu".into()),
        YAxis::Y3,
    );
    p
}

/// The x of the plot area's right edge, located structurally: the rightmost
/// column in the right half whose frame-colored pixels form most of a rule.
/// (The legend border also uses frame color, but its box is far shorter.)
fn frame_x1(fb: &Framebuffer) -> usize {
    let mut runs = vec![0usize; fb.w];
    for (x, _, c) in drawn_pixels(fb) {
        if c == [70, 78, 96] {
            runs[x] += 1;
        }
    }
    (fb.w / 2..fb.w).filter(|x| runs[*x] > fb.h / 3).max().expect("no right rule found")
}

#[test]
fn no_secondary_means_no_right_rule_or_gutter() {
    // Without right-axis traces the layout must match the pre-y2 engine:
    // a fixed 6px right margin (s=1) holding nothing but, at most, the
    // clamped last x tick label near the bottom.
    let fb = demo_2d().render(400, 240);
    for (x, y, _) in drawn_pixels(&fb) {
        if y < fb.h / 2 {
            assert!(x < fb.w - 6, "pixel in the right gutter at ({x}, {y})");
        }
    }
}

#[test]
fn y2_draws_right_rule_and_tinted_labels() {
    let xs: Vec<f32> = (0..=10).map(|i| i as f32).collect();
    let mut p = Plot::new();
    p.add_line2d(
        xs.clone(),
        xs.iter().map(|x| x / 10.0).collect(),
        PALETTE[0],
        2.0,
        None,
        YAxis::Primary,
    );
    p.add_line2d(
        xs.clone(),
        xs.iter().map(|x| x * 100.0).collect(),
        PALETTE[1],
        2.0,
        None,
        YAxis::Y2,
    );
    let fb = p.render(400, 240);
    let x1 = frame_x1(&fb);
    // Data is clipped to the plot area, so any series-colored pixel beyond
    // the rule can only be a tick label tinted to the y2 series.
    assert!(
        drawn_pixels(&fb).iter().any(|(x, y, c)| *x > x1 && *y < fb.h / 2 && *c == PALETTE[1]),
        "no tinted y2 labels in the right gutter"
    );
    // One shared rule: no frame-colored pixels beyond x1.
    assert!(
        !drawn_pixels(&fb).iter().any(|(x, _, c)| *x > x1 && *c == [70, 78, 96]),
        "unexpected second rule beyond x1"
    );
}

#[test]
fn two_right_axes_stack_y2_inside_y3() {
    let fb = demo_2r().render(400, 240);
    let x1 = frame_x1(&fb);
    let gutter: Vec<(usize, [u8; 3])> = drawn_pixels(&fb)
        .into_iter()
        .filter(|(x, y, _)| *x > x1 && *y < fb.h / 2)
        .map(|(x, _, c)| (x, c))
        .collect();
    let min_x = |color: [u8; 3]| {
        gutter.iter().filter(|(_, c)| *c == color).map(|(x, _)| *x).min().expect("column missing")
    };
    assert!(min_x(PALETTE[1]) < min_x(PALETTE[2]), "y2 must sit inside y3");
}

#[test]
fn right_axes_scale_independently() {
    // Each series spans most of the plot height on its own axis; shared
    // scaling would flatten the smaller-magnitude series into slivers.
    let fb = demo_2r().render(400, 240);
    let x1 = frame_x1(&fb);
    for color in [PALETTE[0], PALETTE[1], PALETTE[2]] {
        let ys: Vec<usize> = drawn_pixels(&fb)
            .into_iter()
            .filter(|(x, _, c)| *x < x1 && *c == color)
            .map(|(_, y, _)| y)
            .collect();
        let extent = ys.iter().max().unwrap() - ys.iter().min().unwrap();
        assert!(extent > fb.h / 3, "series {color:?} spans only {extent}px");
    }
}

#[test]
fn data_never_bleeds_into_either_gutter() {
    // Toward the right label columns…
    let mut p = demo_2r();
    p.camera.pan(2000.0, 0.0);
    let fb = p.render(400, 240);
    let x1 = frame_x1(&fb);
    // Primary labels stay neutral ink, so its color beyond the rule can only
    // be leaked data.
    assert!(
        !drawn_pixels(&fb).iter().any(|(x, _, c)| *x > x1 && *c == PALETTE[0]),
        "primary series leaked into the right gutter"
    );
    // …and toward the left one, where right-axis series have no labels.
    let mut p = demo_2r();
    p.camera.pan(-2000.0, 0.0);
    let fb = p.render(400, 240);
    for (x, _, c) in drawn_pixels(&fb) {
        if c == PALETTE[1] || c == PALETTE[2] {
            assert!(x > 20, "right-axis series leaked into the left margin at x={x}");
        }
    }
}

#[test]
fn only_y3_compacts_to_innermost_column() {
    let series = |axis| {
        let xs: Vec<f32> = (0..=10).map(|i| i as f32).collect();
        let ys: Vec<f32> = xs.iter().map(|x| x * 5000.0).collect();
        let mut p = Plot::new();
        p.add_line2d(xs.clone(), xs.clone(), PALETTE[0], 2.0, None, YAxis::Primary);
        p.add_line2d(xs, ys, PALETTE[1], 2.0, None, axis);
        p
    };
    // The same single right-axis series must claim the same margin whether it
    // is y2 or y3 — an absent axis reserves no column.
    let x1_y2 = frame_x1(&series(YAxis::Y2).render(400, 240));
    let x1_y3 = frame_x1(&series(YAxis::Y3).render(400, 240));
    assert_eq!(x1_y2, x1_y3, "a lone y3 axis must compact into the inner slot");
    // And two right axes take more room than one.
    let x1_both = frame_x1(&demo_2r().render(400, 240));
    assert!(x1_both < x1_y2, "two label columns must widen the right margin");
}

#[test]
fn bars_on_y2_fill_from_their_own_baseline() {
    let mut p = Plot::new();
    // Primary range far above zero: if the bar baseline used the primary
    // map, the bars would hang off-screen instead of filling from zero.
    p.add_line2d(
        vec![0.0, 1.0, 2.0],
        vec![500.0, 501.0, 502.0],
        PALETTE[0],
        2.0,
        None,
        YAxis::Primary,
    );
    p.add_bar2d(vec![0.0, 1.0, 2.0], vec![3.0, 1.0, 2.0], PALETTE[2], None, YAxis::Y2);
    let fb = p.render(400, 240);
    let bar_px = drawn_pixels(&fb).into_iter().filter(|(_, _, c)| *c == PALETTE[2]).count();
    assert!(bar_px > 2000, "y2 bars should be solid fills, got {bar_px} px");
}

// --- pick / projection consistency (beyond the in-crate unit tests) ---

#[test]
fn pick_misses_when_nothing_is_near() {
    let p = demo_3d();
    assert_eq!(p.pick(300, 200, 1.0, 1.0, 2.0), None);
}

// --- text ---

#[test]
fn text_respects_the_framebuffer_bounds() {
    let mut fb = Framebuffer::new(30, 10);
    draw_text(&mut fb, -3, -2, "clipped", 2, 0.0, [255, 255, 255]);
    assert!(!drawn_pixels(&fb).is_empty()); // partially visible, no panic
}

// --- ticks at the public boundary ---

#[test]
fn ticks_stay_inside_the_requested_range() {
    for (lo, hi) in [(0.0, 1.0), (-17.3, 4.11), (1e-6, 2e-6), (-1e9, 1e9)] {
        let (ticks, step) = nice_ticks(lo, hi, 6);
        assert!(step > 0.0);
        assert!(!ticks.is_empty(), "range {lo}..{hi} produced no ticks");
        assert!(ticks.iter().all(|t| *t >= lo - step * 1e-6 && *t <= hi + step * 1e-6));
    }
}

// --- element picking & hover (3D graphs) ---

fn demo_graph() -> (Plot, Vec<[f32; 3]>, Vec<(u32, u32)>) {
    let nodes = vec![[0.0, 0.0, 0.0], [5.0, 5.0, 5.0], [-5.0, -5.0, -5.0], [5.0, -5.0, 0.0]];
    let edges = vec![(0u32, 1u32), (1, 2), (0, 3)];
    let mut p = Plot::new();
    p.add_graph3d(
        nodes.clone(),
        vec![[200, 100, 100]; 4],
        edges.clone(),
        3.0,
        None,
        None,
        None,
        None,
    );
    (p, nodes, edges)
}

/// Scan a pixel grid and collect every distinct pick result.
fn scan_elements(p: &Plot, w: usize, h: usize) -> Vec<Element> {
    let mut seen = Vec::new();
    for y in (0..h).step_by(2) {
        for x in (0..w).step_by(2) {
            if let Some(el) = p.pick_element(w, h, x as f32, y as f32, 4.0, 3.0) {
                if !seen.contains(&el) {
                    seen.push(el);
                }
            }
        }
    }
    seen
}

#[test]
fn edges_are_pickable_between_their_endpoints() {
    let (p, _, edges) = demo_graph();
    let found = scan_elements(&p, 300, 200);
    let edge_hits: Vec<usize> = found
        .iter()
        .filter_map(|e| if let Element::Edge(i) = e { Some(*i) } else { None })
        .collect();
    assert!(!edge_hits.is_empty(), "some edge must be hoverable on screen");
    assert!(edge_hits.iter().all(|i| *i < edges.len()), "edge indices are in range");
}

#[test]
fn nodes_win_over_edges_at_a_node() {
    let (p, _, _) = demo_graph();
    let found = scan_elements(&p, 300, 200);
    // Every node lies on an edge endpoint here, so node priority is what makes
    // nodes reachable at all.
    assert!(found.iter().any(|e| matches!(e, Element::Node(_))));
}

#[test]
fn pick_edge_misses_far_away() {
    let (p, _, _) = demo_graph();
    assert_eq!(p.pick_edge(300, 200, 1.0, 1.0, 2.0), None);
}

#[test]
fn hover_lights_up_white_and_changes_the_frame() {
    let (mut p, _, _) = demo_graph();
    let plain = hash(&p.render(300, 200));
    assert!(!has_color(&p.render(300, 200), [255, 255, 255]), "no white before hover");

    p.hovered = Some(Element::Node(1));
    assert_ne!(hash(&p.render(300, 200)), plain);
    assert!(has_color(&p.render(300, 200), [255, 255, 255]), "hovered node is white");

    p.hovered = Some(Element::Edge(0));
    assert!(has_color(&p.render(300, 200), [255, 255, 255]), "hovered edge is white");

    p.hovered = None;
    assert_eq!(hash(&p.render(300, 200)), plain, "clearing hover restores the frame");
}

#[test]
fn selected_edge_glows_and_out_of_range_hover_is_harmless() {
    let (mut p, _, _) = demo_graph();
    p.selected = Some(Element::Edge(1));
    assert!(has_color(&p.render(300, 200), [255, 255, 255]));
    p.selected = None;
    p.hovered = Some(Element::Edge(999));
    let _ = p.render(300, 200); // no panic, nothing to highlight
}

#[test]
fn edge_flat_index_counts_invalid_edges_too() {
    // Edge 0 is unrenderable (endpoint 9 doesn't exist); edge 1 is real. The
    // pick result must still be index 1 — indices match the caller's list.
    let mut p = Plot::new();
    p.add_graph3d(
        vec![[0.0, 0.0, 0.0], [5.0, 5.0, 5.0]],
        vec![[200, 100, 100]; 2],
        vec![(0, 9), (0, 1)],
        3.0,
        None,
        None,
        None,
        None,
    );
    let found = scan_elements(&p, 300, 200);
    let edge_hits: Vec<usize> = found
        .iter()
        .filter_map(|e| if let Element::Edge(i) = e { Some(*i) } else { None })
        .collect();
    assert_eq!(edge_hits, vec![1]);
}

// --- reduced-resolution rendering (render_at) ---

#[test]
fn render_at_half_matches_full_size() {
    // A downscaled frame is half the pixels each way — same aspect, drawn.
    let p = demo_3d();
    let full = p.render(320, 200);
    let half = p.render_at(160, 100, 0.5);
    assert_eq!(half.w, 160);
    assert_eq!(half.h, 100);
    assert!(half.rgba().chunks(4).any(|px| px[3] > 0));
    // The full-res frame is unchanged by the new path (render delegates to it).
    assert_eq!(hash(&full), hash(&p.render_at(320, 200, 1.0)));
}

#[test]
fn pan_scale_keeps_a_panned_view_centered_across_resolutions() {
    // With a pan applied, the node's *relative* screen position must match
    // between full-res and half-res-with-pan_scale (that's what stops the
    // plot from jumping when interaction toggles resolution).
    let mut p = Plot::new();
    p.add_scatter3d(vec![[0.0, 0.0, 0.0]], [255, 0, 0], 3.0, None);
    p.camera.pan(40.0, -25.0);

    let centroid = |fb: &Framebuffer| -> (f64, f64) {
        let (mut sx, mut sy, mut n) = (0.0, 0.0, 0.0);
        for (x, y, _) in drawn_pixels(fb) {
            sx += x as f64;
            sy += y as f64;
            n += 1.0;
        }
        (sx / n, sy / n)
    };
    let (fx, fy) = centroid(&p.render_at(320, 200, 1.0));
    let (hx, hy) = centroid(&p.render_at(160, 100, 0.5));
    // Relative position (fraction of the frame) must match within a pixel.
    assert!((fx / 320.0 - hx / 160.0).abs() < 0.01, "x drifted: {fx}/320 vs {hx}/160");
    assert!((fy / 200.0 - hy / 100.0).abs() < 0.01, "y drifted: {fy}/200 vs {hy}/100");
}

#[test]
fn node_count_spans_all_traces() {
    let mut p = Plot::new();
    assert_eq!(p.node_count(), 0);
    p.add_scatter3d(vec![[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]], [1, 2, 3], 1.0, None);
    p.add_graph3d(vec![[0.0; 3]; 3], vec![[9, 9, 9]; 3], vec![(0, 1)], 1.0, None, None, None, None);
    assert_eq!(p.node_count(), 5);
}

// --- streaming append: handles, extend, set_visible ---

#[test]
fn incremental_build_hash_equals_one_shot_2d() {
    let xs: Vec<f32> = (0..=20).map(|i| i as f32).collect();
    let ys: Vec<f32> = xs.iter().map(|x| (x * 0.5).sin() * 3.0 + 5.0).collect();
    let bxs: Vec<f32> = (0..8).map(|i| i as f32).collect();
    let bhs: Vec<f32> = bxs.iter().map(|x| x * 0.3 + 1.0).collect();

    let mut whole = Plot::new();
    whole.add_line2d(xs.clone(), ys.clone(), PALETTE[0], 2.0, Some("l".into()), YAxis::Primary);
    whole.add_scatter2d(xs.clone(), ys.clone(), PALETTE[1], 2.5, None, YAxis::Primary);
    whole.add_bar2d(bxs.clone(), bhs.clone(), PALETTE[2], None, YAxis::Primary);

    let mut inc = Plot::new();
    let l = inc.add_line2d(
        xs[..7].to_vec(),
        ys[..7].to_vec(),
        PALETTE[0],
        2.0,
        Some("l".into()),
        YAxis::Primary,
    );
    let s = inc.add_scatter2d(
        xs[..1].to_vec(),
        ys[..1].to_vec(),
        PALETTE[1],
        2.5,
        None,
        YAxis::Primary,
    );
    let b = inc.add_bar2d(bxs[..3].to_vec(), bhs[..3].to_vec(), PALETTE[2], None, YAxis::Primary);
    inc.extend_xy(l, &xs[7..], &ys[7..]).unwrap();
    inc.extend_xy(s, &xs[1..12], &ys[1..12]).unwrap();
    inc.extend_xy(s, &xs[12..], &ys[12..]).unwrap();
    inc.extend_xy(b, &bxs[3..], &bhs[3..]).unwrap();

    assert_eq!(hash(&whole.render(240, 160)), hash(&inc.render(240, 160)));
}

#[test]
fn incremental_build_hash_equals_one_shot_3d() {
    let pts: Vec<[f32; 3]> = (0..30)
        .map(|i| [(i as f32 * 0.7).sin() * 2.0, i as f32 * 0.1, (i as f32 * 0.5).cos()])
        .collect();

    let mut whole = Plot::new();
    whole.add_scatter3d(pts.clone(), [230, 60, 120], 3.0, None);
    whole.add_line3d(pts.clone(), [69, 200, 209], 2.0, Some("path".into()));

    let mut inc = Plot::new();
    let s = inc.add_scatter3d(pts[..10].to_vec(), [230, 60, 120], 3.0, None);
    let l = inc.add_line3d(pts[..4].to_vec(), [69, 200, 209], 2.0, Some("path".into()));
    inc.extend_pts(s, &pts[10..]).unwrap();
    inc.extend_pts(l, &pts[4..20]).unwrap();
    inc.extend_pts(l, &pts[20..]).unwrap();

    assert_eq!(hash(&whole.render(240, 160)), hash(&inc.render(240, 160)));
}

#[test]
fn bar_extend_reflows_like_one_shot() {
    // The appended bar at x=3.4 narrows the min gap from 1.0 to 0.4, which
    // must re-flow every bar's width exactly as a one-shot build would.
    let mut whole = Plot::new();
    whole.add_bar2d(
        vec![0.0, 1.0, 2.0, 3.0, 3.4],
        vec![1.0, 2.0, 3.0, 2.0, 1.5],
        PALETTE[0],
        None,
        YAxis::Primary,
    );
    let mut inc = Plot::new();
    let b = inc.add_bar2d(
        vec![0.0, 1.0, 2.0, 3.0],
        vec![1.0, 2.0, 3.0, 2.0],
        PALETTE[0],
        None,
        YAxis::Primary,
    );
    inc.extend_xy(b, &[3.4], &[1.5]).unwrap();
    assert_eq!(hash(&whole.render(240, 160)), hash(&inc.render(240, 160)));
}

#[test]
fn ragged_extend_matches_concatenation() {
    let mut whole = Plot::new();
    whole.add_line2d(
        vec![1.0, 2.0, 3.0, 4.0],
        vec![1.0, 2.0, 3.0],
        PALETTE[0],
        2.0,
        None,
        YAxis::Primary,
    );
    let mut inc = Plot::new();
    let l =
        inc.add_line2d(vec![1.0, 2.0, 3.0], vec![1.0, 2.0], PALETTE[0], 2.0, None, YAxis::Primary);
    inc.extend_xy(l, &[4.0], &[3.0]).unwrap();
    assert_eq!(hash(&whole.render(200, 120)), hash(&inc.render(200, 120)));
}

#[test]
fn hidden_trace_renders_like_never_added_2d() {
    let xs: Vec<f32> = (0..=10).map(|i| i as f32).collect();
    let ys: Vec<f32> = xs.iter().map(|x| x * 0.5 + 1.0).collect();
    let y2: Vec<f32> = xs.iter().map(|x| 100.0 - x * 3.0).collect();

    // An *unnamed* trace has no legend row to leave behind, so hiding it is
    // indistinguishable from never adding it: geometry, bounds contribution,
    // and the right-axis column + tint all go.
    let mut bare = Plot::new();
    bare.add_line2d(xs.clone(), ys.clone(), PALETTE[0], 2.0, Some("a".into()), YAxis::Primary);

    let mut toggled = Plot::new();
    toggled.add_line2d(xs.clone(), ys.clone(), PALETTE[0], 2.0, Some("a".into()), YAxis::Primary);
    let h = toggled.add_line2d(xs.clone(), y2.clone(), PALETTE[1], 2.0, None, YAxis::Y2);
    let before = hash(&toggled.render(240, 160));

    assert!(toggled.set_visible(h, false).unwrap());
    assert!(!toggled.set_visible(h, false).unwrap(), "second hide is a no-op");
    assert_eq!(hash(&toggled.render(240, 160)), hash(&bare.render(240, 160)));

    assert!(toggled.set_visible(h, true).unwrap());
    assert_eq!(hash(&toggled.render(240, 160)), before, "re-show restores the original frame");
}

/// Muting and hiding both drop the geometry, and differ only in the legend:
/// `set_visible(false)` takes the series out of the plot completely, while
/// `set_muted(true)` keeps a greyed row — the thing a click brings back.
#[test]
fn muting_keeps_the_legend_row_that_hiding_removes() {
    let xs: Vec<f32> = (0..=10).map(|i| i as f32).collect();
    let ys: Vec<f32> = xs.iter().map(|x| x * 0.5 + 1.0).collect();
    let y2: Vec<f32> = xs.iter().map(|x| 100.0 - x * 3.0).collect();

    let mut bare = Plot::new();
    bare.add_line2d(xs.clone(), ys.clone(), PALETTE[0], 2.0, Some("a".into()), YAxis::Primary);

    let mut p = Plot::new();
    p.add_line2d(xs.clone(), ys.clone(), PALETTE[0], 2.0, Some("a".into()), YAxis::Primary);
    let h = p.add_line2d(xs.clone(), y2.clone(), PALETTE[1], 2.0, Some("b".into()), YAxis::Y2);
    let before = hash(&p.render(240, 160));

    // Hidden: indistinguishable from never adding b, legend row included.
    p.set_visible(h, false).unwrap();
    assert_eq!(hash(&p.render(240, 160)), hash(&bare.render(240, 160)));
    p.set_visible(h, true).unwrap();

    // Muted: b's geometry and right axis go, but its row stays behind.
    assert!(!p.set_muted(h, true).unwrap(), "set_muted reports the new shown state");
    let muted = hash(&p.render(240, 160));
    assert_ne!(muted, before, "muting drops b's geometry");
    assert_ne!(muted, hash(&bare.render(240, 160)), "muting keeps b's legend row");

    assert!(p.toggle_muted(h).unwrap(), "toggling back shows it");
    assert_eq!(hash(&p.render(240, 160)), before, "unmuting restores the original frame");
}

#[test]
fn hidden_trace_renders_like_never_added_3d() {
    // The fog depth range is the subtle dependency: hidden geometry that
    // stretched the depth span must stop tinting what remains.
    let near: Vec<[f32; 3]> = vec![[0.0, 0.0, 0.0], [1.0, 1.0, 0.5], [-1.0, 0.5, -0.5]];
    let far: Vec<[f32; 3]> = vec![[8.0, 8.0, 8.0], [-8.0, -8.0, -8.0]];

    let mut bare = Plot::new();
    bare.add_scatter3d(near.clone(), [230, 60, 120], 3.0, None);

    let mut toggled = Plot::new();
    toggled.add_scatter3d(near.clone(), [230, 60, 120], 3.0, None);
    let h = toggled.add_scatter3d(far.clone(), [69, 200, 209], 3.0, None);
    toggled.set_visible(h, false).unwrap();
    assert_eq!(hash(&toggled.render(200, 140)), hash(&bare.render(200, 140)));
}

#[test]
fn hidden_trace_keeps_flat_slots() {
    let a_pts: Vec<[f32; 3]> = vec![[-4.0, 0.0, 0.0], [-4.0, 2.0, 0.0]];
    let b_nodes: Vec<[f32; 3]> = vec![[4.0, 0.0, 0.0], [4.0, 2.0, 0.0]];
    let mut p = Plot::new();
    let a = p.add_scatter3d(a_pts, [230, 60, 120], 3.0, None);
    p.add_graph3d(b_nodes, vec![[69, 200, 209]; 2], vec![(0, 1)], 3.0, None, None, None, None);
    // Pin the projection so hiding A cannot move B's nodes on screen.
    p.bounds_override = Some(([-5.0, -1.0, -1.0], [5.0, 3.0, 1.0]));

    let nodes = p.project_nodes(200, 140);
    let (bx, by) = (nodes[2][0], nodes[2][1]); // first B node, flat index 2
    let (ax, ay) = (nodes[0][0], nodes[0][1]); // first A node, flat index 0
    assert_eq!(p.pick(200, 140, bx, by, 3.0), Some(2));

    p.set_visible(a, false).unwrap();
    assert_eq!(p.node_count(), 4, "hidden nodes still occupy the flat index space");
    assert_eq!(p.pick(200, 140, bx, by, 3.0), Some(2), "B keeps its flat index while A is hidden");
    assert_eq!(p.pick(200, 140, ax, ay, 3.0), None, "hidden geometry is not a pick target");
}

#[test]
fn extend_remaps_selection() {
    let a: Vec<[f32; 3]> = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
    let b: Vec<[f32; 3]> = vec![[0.0, 2.0, 0.0], [1.0, 2.0, 0.0]];
    let extra = [[0.5, 1.0, 0.5]];

    let mut inc = Plot::new();
    let ha = inc.add_scatter3d(a.clone(), [230, 60, 120], 3.0, None);
    inc.add_scatter3d(b.clone(), [69, 200, 209], 3.0, None);
    inc.selected = Some(Element::Node(2)); // first node of B
    inc.extend_pts(ha, &extra).unwrap();
    assert_eq!(inc.selected, Some(Element::Node(3)), "selection follows the shifted flat index");

    let mut whole = Plot::new();
    whole.add_scatter3d([a.as_slice(), &extra].concat(), [230, 60, 120], 3.0, None);
    whole.add_scatter3d(b, [69, 200, 209], 3.0, None);
    whole.selected = Some(Element::Node(3));
    assert_eq!(hash(&whole.render(200, 140)), hash(&inc.render(200, 140)));
}

#[test]
fn extend_and_visibility_error_paths() {
    let mut p = Plot::new();
    let s3 = p.add_scatter3d(vec![[0.0, 0.0, 0.0]], [230, 60, 120], 3.0, None);
    let g = p.add_graph3d(
        vec![[1.0, 1.0, 1.0]],
        vec![[69, 200, 209]],
        vec![],
        3.0,
        None,
        None,
        None,
        None,
    );
    let sf =
        p.add_surface3d(vec![0.0, 1.0], vec![0.0, 1.0], vec![0.0; 4], [1, 2, 3], None, false, None);
    let l2 = p.add_line2d(vec![0.0], vec![0.0], PALETTE[0], 2.0, None, YAxis::Primary);
    let me = p.add_mesh3d(vec![[0.0; 3]; 3], vec![[0, 1, 2]], [1, 2, 3], None, None);

    assert_eq!(p.extend_xy(99, &[], &[]), Err(TraceError::UnknownTrace));
    assert_eq!(p.set_visible(99, false), Err(TraceError::UnknownTrace));
    assert_eq!(p.extend_xy(s3, &[1.0], &[1.0]), Err(TraceError::WrongKind));
    assert_eq!(p.extend_pts(l2, &[[0.0; 3]]), Err(TraceError::WrongKind));
    assert_eq!(p.extend_pts(g, &[[0.0; 3]]), Err(TraceError::Structural));
    assert_eq!(p.extend_xy(sf, &[1.0], &[1.0]), Err(TraceError::Structural));
    // A mesh is structural like a graph or a surface, not the wrong kind.
    assert_eq!(p.extend_pts(me, &[[0.0; 3]]), Err(TraceError::Structural));
    assert_eq!(p.extend_xy(me, &[1.0], &[1.0]), Err(TraceError::Structural));
}

#[test]
fn extend_survives_degenerate_frames() {
    let mut p = Plot::new();
    let l = p.add_line2d(vec![], vec![], PALETTE[0], 2.0, None, YAxis::Primary);
    p.extend_xy(l, &[], &[]).unwrap();
    p.extend_xy(l, &[0.0, f32::NAN, 1.0], &[1.0, 2.0, f32::NAN]).unwrap();
    p.render(1, 1);
    p.render(120, 80);

    let mut q = Plot::new();
    let s = q.add_scatter3d(vec![], [230, 60, 120], 3.0, None);
    q.extend_pts(s, &[[f32::NAN, 0.0, 0.0], [1.0, 1.0, 1.0]]).unwrap();
    q.render(1, 1);
    q.render(120, 80);
}

#[test]
fn direct_trace_push_falls_back_without_panicking() {
    use plotui_core::Trace;
    let mut p = Plot::new();
    p.add_line2d(vec![0.0, 1.0], vec![0.0, 1.0], PALETTE[0], 2.0, None, YAxis::Primary);
    // Bypass the API: the meta cache is now behind, so consumers must fall
    // back to full scans and still draw the pushed trace.
    p.traces.push(Trace::Scatter2d {
        xs: vec![0.5],
        ys: vec![0.5],
        color: [1, 2, 3],
        size: 4.0,
        colors: None,
        sizes: None,
        shapes: None,
        err_x: None,
        err_y: None,
        name: None,
        axis: YAxis::Primary,
    });
    let fb = p.render(160, 120);
    assert!(has_color(&fb, [1, 2, 3]), "directly pushed trace still draws");
    assert_eq!(p.node_count(), 0);
}

// --- the 2D x window ---

/// A quiet sine over x 0..=10, then a 1000-scale burst over 11..=20: windowed
/// to the quiet stretch, y must rescale to it or the sine is a flat sliver.
fn quiet_then_burst() -> Plot {
    let xs: Vec<f32> = (0..=20).map(|i| i as f32).collect();
    let ys: Vec<f32> =
        xs.iter().map(|x| if *x <= 10.0 { (x * 0.9).sin() } else { 1000.0 }).collect();
    let mut p = Plot::new();
    p.add_line2d(xs, ys, PALETTE[0], 2.0, Some("signal".into()), YAxis::Primary);
    p
}

#[test]
fn x_window_changes_the_2d_frame() {
    let mut p = demo_2d();
    let before = hash(&p.render(400, 240));
    p.x_window = Some((5.0, 12.0));
    assert_ne!(hash(&p.render(400, 240)), before);
    p.x_window = None;
    assert_eq!(hash(&p.render(400, 240)), before, "clearing the window restores the frame");
}

#[test]
fn x_window_autoscales_y_to_visible_points() {
    let mut p = quiet_then_burst();
    p.x_window = Some((0.0, 10.0));
    let fb = p.render(400, 240);
    let ys: Vec<usize> = drawn_pixels(&fb)
        .into_iter()
        .filter(|(_, _, c)| *c == PALETTE[0])
        .map(|(_, y, _)| y)
        .collect();
    let extent = ys.iter().max().unwrap() - ys.iter().min().unwrap();
    assert!(extent > fb.h / 3, "windowed sine spans only {extent}px — y did not rescale");
}

#[test]
fn x_window_supersedes_camera() {
    let mut p = demo_2d();
    p.x_window = Some((3.0, 15.0));
    let windowed = hash(&p.render(400, 240));
    p.camera.zoom_by(2.5);
    p.camera.pan(80.0, -40.0);
    assert_eq!(hash(&p.render(400, 240)), windowed, "camera must not move a windowed view");
}

#[test]
fn x_window_right_axes_scale_independently() {
    let mut p = demo_2r();
    p.x_window = Some((0.0, 10.0));
    let fb = p.render(400, 240);
    let x1 = frame_x1(&fb);
    for color in [PALETTE[0], PALETTE[1], PALETTE[2]] {
        let ys: Vec<usize> = drawn_pixels(&fb)
            .into_iter()
            .filter(|(x, _, c)| *x < x1 && *c == color)
            .map(|(_, y, _)| y)
            .collect();
        let extent = ys.iter().max().unwrap() - ys.iter().min().unwrap();
        assert!(extent > fb.h / 3, "windowed series {color:?} spans only {extent}px");
    }
}

#[test]
fn x_window_never_bleeds_into_the_margins() {
    // An extreme ratio: 10k points, a window covering one thousandth of them.
    // Guards both the gutters and the pre-clip draw cost (unclipped, this
    // render would stamp millions of rejected pixels).
    let xs: Vec<f32> = (0..=10_000).map(|i| i as f32).collect();
    let ys: Vec<f32> = xs.iter().map(|x| (x * 0.01).sin() * 5.0).collect();
    let bh: Vec<f32> = xs.iter().map(|x| (x * 0.003).cos().abs()).collect();
    let mut p = Plot::new();
    p.add_line2d(xs.clone(), ys.clone(), PALETTE[0], 2.0, None, YAxis::Primary);
    p.add_scatter2d(xs.clone(), ys, PALETTE[1], 2.5, None, YAxis::Primary);
    p.add_bar2d(xs, bh, PALETTE[2], None, YAxis::Y2);
    p.x_window = Some((5000.0, 5010.0));
    let fb = p.render(400, 240);
    let x1 = frame_x1(&fb);
    for (x, _, c) in drawn_pixels(&fb) {
        // Y2's tick labels are tinted PALETTE[2] and live beyond the rule by
        // design, so only its left-margin side is asserted.
        if c == PALETTE[0] || c == PALETTE[1] {
            assert!(x > 20 && x <= x1 + 1, "series pixel leaked to x={x} (rule at {x1})");
        } else if c == PALETTE[2] {
            assert!(x > 20, "bar pixel leaked into the left margin at x={x}");
        }
    }
}

#[test]
fn x_window_with_no_visible_points_falls_back() {
    // Unnamed trace: a legend swatch would otherwise count as a data pixel.
    let xs: Vec<f32> = (0..=20).map(|i| i as f32).collect();
    let ys: Vec<f32> = xs.iter().map(|x| (x * 0.5).sin()).collect();
    let mut p = Plot::new();
    p.add_line2d(xs, ys, PALETTE[0], 2.0, None, YAxis::Primary);
    p.x_window = Some((100.0, 101.0)); // beyond every sample
    let fb = p.render(400, 240);
    assert!(has_color(&fb, [70, 78, 96]), "axes still draw over an empty window");
    assert!(!has_color(&fb, PALETTE[0]), "no data pixels for an empty window");
}

#[test]
fn bars_straddling_the_window_keep_their_baseline() {
    let xs: Vec<f32> = (0..10).map(|i| i as f32).collect();
    let hs: Vec<f32> = xs.iter().map(|x| x + 1.0).collect();
    let mut p = Plot::new();
    p.add_bar2d(xs, hs, PALETTE[3], None, YAxis::Primary);
    p.x_window = Some((2.5, 4.5)); // cuts through the bars at 2, 3, 4, 5
    let fb = p.render(400, 240);
    let cols: Vec<usize> = drawn_pixels(&fb)
        .into_iter()
        .filter(|(_, _, c)| *c == PALETTE[3])
        .map(|(x, _, _)| x)
        .collect();
    assert!(!cols.is_empty(), "straddling bars still draw");
    for x in cols {
        assert!(x > 20, "bar pixel leaked into the margin at x={x}");
    }
}

#[test]
fn extend_does_not_move_x_window() {
    let mut p = demo_2d();
    p.x_window = Some((0.0, 10.0));
    let before = hash(&p.render(400, 240));
    p.extend_xy(0, &[30.0, 31.0], &[900.0, 901.0]).unwrap();
    assert_eq!(
        hash(&p.render(400, 240)),
        before,
        "appending off-window points must not change a windowed frame"
    );
}

// --- the range-slider strip ---

/// A trace color at 40% — must mirror `shade(c, 0.4)` in the engine.
fn dim(c: [u8; 3]) -> [u8; 3] {
    c.map(|v| (v as f32 * 0.4).clamp(0.0, 255.0) as u8)
}

/// Rows in the frame's bottom half whose frame-colored pixels span most of
/// the width — the x-axis rule, plus the strip's top and bottom borders when
/// the range slider is active. The structural sibling of `frame_x1`.
fn long_frame_rows(fb: &Framebuffer) -> Vec<usize> {
    let mut runs = vec![0usize; fb.h];
    for (_, y, c) in drawn_pixels(fb) {
        if c == [70, 78, 96] {
            runs[y] += 1;
        }
    }
    (fb.h / 2..fb.h).filter(|y| runs[*y] > fb.w / 3).collect()
}

#[test]
fn range_slider_reserves_a_bottom_strip() {
    let mut p = demo_2d();
    let plain_rows = long_frame_rows(&p.render(400, 240));
    p.range_slider = true;
    let fb = p.render(400, 240);
    let rows = long_frame_rows(&fb);
    assert!(
        rows.len() >= plain_rows.len() + 2,
        "expected the strip's two borders below the axis, got rows {rows:?} vs {plain_rows:?}"
    );
    // The strip sits below the x-axis rule, near the frame's bottom edge.
    assert!(*rows.last().unwrap() > fb.h - 10, "strip must hug the bottom edge");
}

#[test]
fn range_slider_dims_outside_the_window() {
    let mut p = demo_2d();
    p.range_slider = true;
    // No window: the full-color pass repaints the whole strip, no dim shows.
    let fb = p.render(400, 240);
    assert!(!has_color(&fb, dim(PALETTE[0])), "windowless strip must be full-color");
    // Windowed: outside the selection stays dim, inside is full color.
    p.x_window = Some((5.0, 10.0));
    let fb = p.render(400, 240);
    assert!(has_color(&fb, dim(PALETTE[0])), "off-window overview must be dimmed");
    assert!(has_color(&fb, PALETTE[0]), "in-window overview keeps the trace color");
}

#[test]
fn no_range_slider_is_byte_identical() {
    let mut p = demo_2d();
    let before = hash(&p.render(400, 240));
    p.range_slider = true;
    assert_ne!(hash(&p.render(400, 240)), before, "the strip must change pixels");
    p.range_slider = false;
    assert_eq!(hash(&p.render(400, 240)), before, "disabling must restore the frame");
}

#[test]
fn range_slider_ignored_by_3d_plots() {
    let mut p = demo_3d();
    let before = hash(&p.render(320, 200));
    p.range_slider = true;
    p.x_window = Some((0.0, 1.0));
    assert_eq!(hash(&p.render(320, 200)), before);
    assert_eq!(p.range_slider_hit(320, 200, 160.0, 190.0, 8.0), None);
    assert!(!p.drag_x_window(320, 200, plotui_core::RangeHit::Window, 10.0));
}

#[test]
fn range_slider_drops_on_short_frames() {
    let mut p = demo_2d();
    let before = hash(&p.render(400, 120));
    p.range_slider = true;
    assert_eq!(hash(&p.render(400, 120)), before, "short frames must drop the strip");
    assert_eq!(p.range_slider_hit(400, 120, 200.0, 110.0, 8.0), None);
}

#[test]
fn hidden_traces_absent_from_strip() {
    let mut p = Plot::new();
    let xs: Vec<f32> = (0..=20).map(|i| i as f32).collect();
    p.add_line2d(xs.clone(), xs.clone(), PALETTE[0], 2.0, None, YAxis::Primary);
    let h = p.add_line2d(
        xs.iter().map(|x| x + 0.5).collect(),
        xs,
        PALETTE[1],
        2.0,
        None,
        YAxis::Primary,
    );
    p.range_slider = true;
    p.x_window = Some((5.0, 10.0));
    p.set_visible(h, false).unwrap();
    let fb = p.render(400, 240);
    assert!(!has_color(&fb, PALETTE[1]), "hidden trace in the strip");
    assert!(!has_color(&fb, dim(PALETTE[1])), "hidden trace in the dim pass");
}

// --- range-slider hit testing and drags ---

use plotui_core::{RangeHit, MIN_WINDOW_FRAC};

/// demo_2d with the slider on, at 400x240 (s = 1): strip rows ~212..236.
fn slider_plot() -> Plot {
    let mut p = demo_2d();
    p.range_slider = true;
    p
}

#[test]
fn range_hit_zones_prioritize_handles() {
    let mut p = slider_plot();
    p.x_window = Some((5.0, 15.0)); // data 0..=20: window in the middle
    let sy = 224.0; // inside the strip
                    // Way off the strip: nothing.
    assert_eq!(p.range_slider_hit(400, 240, 200.0, 100.0, 4.0), None);
    // The window body sits between the handles.
    let mid = p.range_slider_hit(400, 240, 200.0, sy, 4.0);
    assert_eq!(mid, Some(RangeHit::Window));
    // Left of the window: track; and hugging the frame's left rule: still on it.
    assert_eq!(p.range_slider_hit(400, 240, 60.0, sy, 4.0), Some(RangeHit::Track));
    // Sweep to find the handle columns, and check they beat Window/Track.
    let hits: Vec<Option<RangeHit>> =
        (40..390).map(|x| p.range_slider_hit(400, 240, x as f32, sy, 4.0)).collect();
    assert!(hits.contains(&Some(RangeHit::LeftHandle)));
    assert!(hits.contains(&Some(RangeHit::RightHandle)));
    let order: Vec<RangeHit> = hits.into_iter().flatten().collect();
    let first_left = order.iter().position(|h| *h == RangeHit::LeftHandle).unwrap();
    let first_right = order.iter().position(|h| *h == RangeHit::RightHandle).unwrap();
    assert!(first_left < first_right, "left handle must come before right");
}

#[test]
fn drag_clamps_to_extent_and_min_width() {
    let mut p = slider_plot();
    p.x_window = Some((5.0, 15.0));
    // Slam the left handle far right: it stops at min width from the right edge.
    assert!(p.drag_x_window(400, 240, RangeHit::LeftHandle, 10_000.0));
    let (lo, hi) = p.x_window.unwrap();
    assert_eq!(hi, 15.0, "right edge must not move under a left-handle drag");
    let full_span = {
        let mut q = slider_plot();
        assert!(q.drag_x_window(400, 240, RangeHit::RightHandle, 0.0) || q.x_window.is_some());
        let (flo, fhi) = q.x_window.unwrap();
        fhi - flo
    };
    assert!(hi - lo >= full_span * MIN_WINDOW_FRAC * 0.999, "window collapsed below min width");
    // Slam the whole window far left: span preserved, pinned to the extent.
    let mut p = slider_plot();
    p.x_window = Some((5.0, 15.0));
    assert!(p.drag_x_window(400, 240, RangeHit::Window, -10_000.0));
    let (lo2, hi2) = p.x_window.unwrap();
    assert!((hi2 - lo2 - 10.0).abs() < 1e-6, "span must be preserved");
    assert!(lo2 < 5.0, "window must have slid left");
}

#[test]
fn drag_with_no_window_starts_from_full_extent() {
    let mut p = slider_plot();
    assert_eq!(p.x_window, None);
    // Dragging the right handle inward from "everything" starts windowing.
    assert!(p.drag_x_window(400, 240, RangeHit::RightHandle, -60.0));
    let (lo, hi) = p.x_window.unwrap();
    assert!(lo < 0.0, "left edge stays at the padded full extent");
    assert!(hi < 20.0, "right edge must have moved in");
}

#[test]
fn jump_centers_the_window() {
    let mut p = slider_plot();
    p.x_window = Some((0.0, 4.0));
    // Click the track around the strip's midpoint: window keeps its span and
    // re-centers near the middle of the data.
    assert!(p.jump_x_window(400, 240, 220.0));
    let (lo, hi) = p.x_window.unwrap();
    assert!((hi - lo - 4.0).abs() < 1e-6, "span must be preserved");
    assert!(lo > 4.0 && hi < 20.0, "window must have jumped toward the click, got ({lo}, {hi})");
}

#[test]
fn zoom_x_window_pins_the_cursor_x() {
    let mut p = slider_plot();
    p.x_window = Some((0.0, 20.0));
    // The data x under the cursor before the zoom is still under it after.
    let px = 250.0f32;
    let (lo0, hi0) = p.x_window.unwrap();
    assert!(p.zoom_x_window(400, 240, px, 2.0));
    let (lo1, hi1) = p.x_window.unwrap();
    assert!(hi1 - lo1 < (hi0 - lo0) * 0.75, "zoom in must shrink the span");
    assert!(lo1 > lo0 && hi1 < hi0, "new window must nest inside the old one");
}

#[test]
fn pan_x_window_requires_a_window() {
    let mut p = slider_plot();
    assert!(!p.pan_x_window(400, 240, 50.0), "no window, nothing to pan");
    p.x_window = Some((5.0, 15.0));
    assert!(p.pan_x_window(400, 240, 50.0));
    let (lo, _) = p.x_window.unwrap();
    assert!(lo < 5.0, "dragging right must move the view left (grab the data)");
}

// --- the time axis ---

#[test]
fn epoch_axis_labels_are_dates_not_scientific() {
    // A week of daily samples, x as offsets from a midnight base. Without
    // x_epoch these xs would label numerically; with it, calendar dates.
    let base = plotui_core::days_from_civil(2026, 3, 10) as f64 * 86_400.0;
    let xs: Vec<f32> = (0..=7).map(|d| (d * 86_400) as f32).collect();
    let ys: Vec<f32> = xs.iter().map(|x| (x / 40_000.0).sin()).collect();
    let mut p = Plot::new();
    p.add_line2d(xs, ys, PALETTE[0], 2.0, None, YAxis::Primary);
    let plain = hash(&p.render(400, 240));
    p.x_epoch = Some(base);
    let dated = p.render(400, 240);
    assert_ne!(hash(&dated), plain, "date labels must change the frame");
    assert!(has_color(&dated, [150, 156, 170]), "tick labels still present");
    // The crosshair header formats a date on time axes (drawing must not panic
    // and must change pixels vs. the un-hovered dated frame).
    p.hover2d_px = Some(200.0);
    assert_ne!(hash(&p.render(400, 240)), hash(&dated));
}

// --- graph mutators: set_graph_positions / set_graph_colors / extend_graph ---

fn graph_at(pts: &[[f32; 3]], edges: &[(u32, u32)]) -> (Plot, usize) {
    let mut p = Plot::new();
    let h = p.add_graph3d(
        pts.to_vec(),
        vec![[200, 120, 90]; pts.len()],
        edges.to_vec(),
        3.0,
        None,
        None,
        None,
        None,
    );
    (p, h)
}

#[test]
fn set_graph_positions_renders_like_one_shot() {
    let edges = [(0u32, 1), (1, 2)];
    let target = [[0.5, 0.0, -0.5], [-0.5, 0.5, 0.0], [0.0, -0.5, 0.5]];
    // Start WIDER than the target, so a stale (widen-only) bounds cache
    // would keep the old frame and the hashes would differ.
    let wide = [[5.0, 0.0, -5.0], [-5.0, 5.0, 0.0], [0.0, -5.0, 5.0]];
    let (oneshot, _) = graph_at(&target, &edges);
    let (mut moved, h) = graph_at(&wide, &edges);
    moved.set_graph_positions(h, target.to_vec()).unwrap();
    assert_eq!(
        hash(&moved.render(300, 200)),
        hash(&oneshot.render(300, 200)),
        "moved graph must render exactly like one built in place (bounds must shrink)"
    );
}

#[test]
fn set_graph_colors_recolors_and_restores() {
    let pts = [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]];
    let (mut p, h) = graph_at(&pts, &[(0, 1)]);
    p.show_box = false;
    let before = hash(&p.render(200, 150));
    p.set_graph_colors(h, vec![[9, 250, 9]; 2], Some(vec![[250, 9, 9]])).unwrap();
    let lit = p.render(200, 150);
    assert!(has_color(&lit, [9, 250, 9]), "new node color must reach the pixels");
    assert_ne!(hash(&lit), before);
    p.set_graph_colors(h, vec![[200, 120, 90]; 2], None).unwrap();
    assert_eq!(hash(&p.render(200, 150)), before, "restore must be exact");
}

#[test]
fn extend_graph_renders_like_one_shot_and_keeps_flat_slots() {
    let base = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
    let extra = [[0.0, 1.0, 0.0]];
    let all: Vec<[f32; 3]> = base.iter().chain(&extra).copied().collect();
    let (oneshot, _) = graph_at(&all, &[(0, 1), (1, 2)]);
    let (mut inc, h) = graph_at(&base, &[(0, 1)]);
    inc.extend_graph(h, &extra, &[[200, 120, 90]], &[(1, 2)], None).unwrap();
    assert_eq!(
        hash(&inc.render(300, 200)),
        hash(&oneshot.render(300, 200)),
        "append must render exactly like a one-shot build"
    );
    // Selection on a LATER trace's node survives the append: a graph that is
    // not the last node-bearing trace shifts downstream flat indices, and
    // the plot remaps its own selection.
    let mut p = Plot::new();
    let g = p.add_graph3d(
        base.to_vec(),
        vec![[200, 120, 90]; 2],
        vec![(0, 1)],
        3.0,
        None,
        None,
        None,
        None,
    );
    p.add_scatter3d(vec![[2.0, 2.0, 2.0]], [50, 60, 70], 3.0, None);
    p.bounds_override = Some(([-1.0; 3], [3.0; 3]));
    p.selected = Some(Element::Node(2)); // the scatter's point
    p.hovered = Some(Element::Edge(0));
    p.extend_graph(g, &extra, &[[200, 120, 90]], &[(1, 2)], None).unwrap();
    assert_eq!(p.selected, Some(Element::Node(3)), "downstream node index remapped");
    assert_eq!(p.hovered, Some(Element::Edge(0)), "edge before the append keeps its index");
}

#[test]
fn graph_mutator_error_paths() {
    let (mut p, h) = graph_at(&[[0.0; 3], [1.0; 3]], &[(0, 1)]);
    let s = p.add_scatter3d(vec![[0.0; 3]], [1, 2, 3], 1.0, None);
    assert_eq!(p.set_graph_positions(99, vec![]), Err(TraceError::UnknownTrace));
    assert_eq!(p.set_graph_positions(s, vec![[0.0; 3]]), Err(TraceError::WrongKind));
    assert_eq!(p.set_graph_positions(h, vec![[0.0; 3]]), Err(TraceError::LengthMismatch));
    assert_eq!(p.set_graph_colors(h, vec![[0; 3]], None), Err(TraceError::LengthMismatch));
    assert_eq!(
        p.set_graph_colors(h, vec![[0; 3]; 2], Some(vec![])),
        Err(TraceError::LengthMismatch)
    );
    assert_eq!(p.extend_graph(s, &[], &[], &[], None), Err(TraceError::WrongKind));
    // The failed calls must not have desynced anything: a good call still works.
    assert!(p.set_graph_positions(h, vec![[0.5; 3], [1.5; 3]]).is_ok());
}

/// A click on a legend row resolves to that row's trace, in both render
/// paths, and toggling it round-trips the frame. The rows are found by
/// probing the box the renderer actually drew, so this also pins that
/// hit-testing and drawing agree on where the legend is.
#[test]
fn legend_rows_are_clickable_and_toggle() {
    let (w, h) = (600usize, 400usize);

    // 3D: two named traces, so the legend has two distinguishable rows.
    let mut p = Plot::new();
    let a = p.add_scatter3d(
        vec![[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]],
        PALETTE[0],
        3.0,
        Some("alpha".into()),
    );
    let b = p.add_scatter3d(
        vec![[-1.0, 0.0, 1.0], [0.5, -1.0, 0.0]],
        PALETTE[1],
        3.0,
        Some("beta".into()),
    );

    // Sweep the top-right corner for the two rows the legend drew there.
    let mut seen = std::collections::BTreeMap::new();
    for y in 0..h {
        for x in (w / 2)..w {
            if let Some(id) = p.legend_hit(w, h, x as f32, y as f32) {
                seen.entry(id).or_insert((x, y));
            }
        }
    }
    assert_eq!(seen.keys().copied().collect::<Vec<_>>(), vec![a, b], "both rows are hittable");
    // The rows are stacked in trace order: alpha's row sits above beta's.
    assert!(seen[&a].1 < seen[&b].1, "alpha's row is above beta's");

    let before = hash(&p.render(w, h));
    let (bx, by) = seen[&b];
    let hit = p.legend_hit(w, h, bx as f32, by as f32).expect("row b");
    assert_eq!(hit, b);
    assert!(!p.toggle_muted(hit).unwrap(), "first toggle mutes");
    let muted = hash(&p.render(w, h));
    assert_ne!(muted, before, "muting b changes the frame");
    // Still hittable while muted — that is how it comes back.
    assert_eq!(p.legend_hit(w, h, bx as f32, by as f32), Some(b));
    assert!(p.toggle_muted(hit).unwrap(), "second toggle shows");
    assert_eq!(hash(&p.render(w, h)), before, "toggling back restores the frame");

    // 2D anchors the legend to the plot frame, not the image; the same probe
    // must still find both rows.
    let mut q = Plot::new();
    let xs: Vec<f32> = (0..8).map(|i| i as f32).collect();
    let c =
        q.add_line2d(xs.clone(), xs.clone(), PALETTE[0], 2.0, Some("up".into()), YAxis::Primary);
    let d = q.add_line2d(
        xs.clone(),
        xs.iter().map(|x| 8.0 - x).collect(),
        PALETTE[1],
        2.0,
        Some("down".into()),
        YAxis::Primary,
    );
    let mut seen2 = std::collections::BTreeSet::new();
    for y in 0..h {
        for x in (w / 2)..w {
            if let Some(id) = q.legend_hit(w, h, x as f32, y as f32) {
                seen2.insert(id);
            }
        }
    }
    assert_eq!(seen2.into_iter().collect::<Vec<_>>(), vec![c, d], "2D rows are hittable too");
}

/// The legend overlay is the legend and nothing else: transparent everywhere
/// the panel is not, and byte-identical to the legend in a full render.
#[test]
fn legend_overlay_matches_the_rendered_legend() {
    let (w, h) = (500usize, 320usize);
    let mut p = Plot::new();
    p.show_box = false;
    p.add_scatter3d(vec![[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]], PALETTE[0], 3.0, Some("one".into()));

    let overlay = p.render_legend_overlay(w, h).rgba();
    let lit: Vec<usize> =
        overlay.chunks(4).enumerate().filter(|(_, px)| px[3] > 0).map(|(i, _)| i).collect();
    assert!(!lit.is_empty(), "the overlay must carry the legend");
    // Everything drawn sits in the top-right corner where the legend goes.
    for i in &lit {
        let (x, y) = (i % w, i / w);
        assert!(x > w / 2 && y < h / 2, "overlay pixel ({x},{y}) is outside the legend corner");
    }
    // And those pixels match what a full render puts there.
    let full = p.render(w, h).rgba();
    for i in &lit {
        assert_eq!(&full[i * 4..i * 4 + 3], &overlay[i * 4..i * 4 + 3], "pixel {i} differs");
    }
}

// --- 2D graphs (Graph2d) ---

/// The node card fill: `Chrome::default().bg` lightened by 8, so a box reads
/// as a panel over the frame rather than as a hole in it.
const CARD: [u8; 3] = [34, 38, 52];

/// A two-node pipeline with contrasting colours on the nodes and the wire, at
/// a size whose text scale is 1 — the bitmap font writes its ink unblended,
/// which is what lets a probe compare against an exact colour.
fn demo_graph2d() -> Plot {
    let mut p = Plot::new();
    p.add_graph2d(
        vec![[0.0, 1.0], [0.0, 0.0]],
        vec!["alpha".into(), "beta".into()],
        vec![[250, 10, 10], [10, 250, 10]],
        vec![(0, 1)],
        true,
        None,
        Some(vec![[10, 10, 250]]),
        None,
        None,
    );
    p
}

#[test]
fn graph2d_draws_labels_boxes_and_arrowheads() {
    let fb = demo_graph2d().render(300, 220);
    assert!(has_color(&fb, CARD), "node boxes are filled with the card colour");
    for c in [[250, 10, 10], [10, 250, 10], [10, 10, 250]] {
        assert!(has_color(&fb, c), "{c:?} must reach the pixels");
    }

    // Labels go into the framebuffer (never an overlay), so their ink is on
    // the buffer — and it has to be inside a box, not floating beside one.
    let px = drawn_pixels(&fb);
    let fill: Vec<_> = px.iter().filter(|(_, _, c)| *c == CARD).collect();
    assert!(!fill.is_empty());
    let (x0, x1) =
        (fill.iter().map(|f| f.0).min().unwrap(), fill.iter().map(|f| f.0).max().unwrap());
    let (y0, y1) =
        (fill.iter().map(|f| f.1).min().unwrap(), fill.iter().map(|f| f.1).max().unwrap());
    assert!(
        px.iter().any(|(x, y, c)| {
            *c == [205, 210, 220] && (x0..=x1).contains(x) && (y0..=y1).contains(y)
        }),
        "label ink must land inside the node boxes"
    );

    // The arrowhead is a filled triangle in the edge colour, so somewhere the
    // wire is several pixels wide across a row; the 1px stroke never is.
    let mut per_row = std::collections::BTreeMap::<usize, usize>::new();
    for (_, y, _) in px.iter().filter(|(_, _, c)| *c == [10, 10, 250]) {
        *per_row.entry(*y).or_default() += 1;
    }
    let widest = per_row.values().copied().max().unwrap_or(0);
    assert!(widest >= 4, "no arrowhead: widest edge row is {widest}px");
}

#[test]
fn graph2d_alone_hides_axes_and_show_axes_restores_them() {
    const FRAME: [u8; 3] = [70, 78, 96];
    let mut p = demo_graph2d();
    assert!(!has_color(&p.render(300, 220), FRAME), "a graph-only frame draws no axis rules");
    p.set_show_axes(true);
    assert!(has_color(&p.render(300, 220), FRAME), "show_axes = true restores them");
    p.set_show_axes(None);
    assert!(!has_color(&p.render(300, 220), FRAME), "None restores the automatic rule");

    // One ordinary 2D trace alongside the graph brings the chrome back: its
    // values *are* measurements, and they need a scale to be read against.
    let mut mixed = demo_graph2d();
    mixed.add_line2d(vec![0.0, 1.0], vec![0.0, 1.0], PALETTE[0], 2.0, None, YAxis::Primary);
    assert!(has_color(&mixed.render(300, 220), FRAME), "a mixed frame keeps its axes");

    // And show_axes = false hides the chrome on an ordinary chart too. The
    // series is unnamed on purpose: the legend keeps its own frame-coloured
    // border either way, because hiding the axes is not hiding the legend.
    let mut chart = Plot::new();
    chart.add_line2d(
        vec![0.0, 1.0, 2.0],
        vec![0.0, 1.0, 0.5],
        PALETTE[0],
        2.0,
        None,
        YAxis::Primary,
    );
    assert!(has_color(&chart.render(300, 220), FRAME));
    chart.set_show_axes(false);
    assert!(!has_color(&chart.render(300, 220), FRAME));
}

#[test]
fn graph2d_node_shapes_and_routes_change_the_drawing() {
    let base = hash(&demo_graph2d().render(300, 220));

    let mut shaped = Plot::new();
    shaped.add_graph2d(
        vec![[0.0, 1.0], [0.0, 0.0]],
        vec!["alpha".into(), "beta".into()],
        vec![[250, 10, 10], [10, 250, 10]],
        vec![(0, 1)],
        true,
        Some(vec![NodeShape::Ellipse, NodeShape::Diamond]),
        Some(vec![[10, 10, 250]]),
        None,
        None,
    );
    assert_ne!(hash(&shaped.render(300, 220)), base, "node shapes must change the pixels");

    // A waypoint bends the edge away from the straight run between the boxes.
    let mut routed = demo_graph2d();
    let mut with_route = Plot::new();
    with_route.add_graph2d(
        vec![[0.0, 1.0], [0.0, 0.0]],
        vec!["alpha".into(), "beta".into()],
        vec![[250, 10, 10], [10, 250, 10]],
        vec![(0, 1)],
        true,
        None,
        Some(vec![[10, 10, 250]]),
        Some((vec![[0.6, 0.5]], vec![0])),
        None,
    );
    assert_ne!(
        hash(&with_route.render(300, 220)),
        hash(&routed.render(300, 220)),
        "an edge waypoint must route the edge somewhere else"
    );
    routed.set_show_axes(false);
    assert_eq!(
        hash(&routed.render(300, 220)),
        base,
        "pinning show_axes off matches what the automatic rule already did"
    );
}

#[test]
fn graph2d_pick_element_hits_boxes_and_edges() {
    let (w, h) = (400usize, 300usize);
    let p = demo_graph2d();
    let nodes = p.project_nodes(w, h);
    assert_eq!(nodes.len(), 2, "a 2D graph's nodes are in the flat index space");
    assert_eq!(p.node_count(), 2);

    // A node is its box: the centre hits, and so does a point well off the
    // centre that is still inside the box.
    for (i, n) in nodes.iter().enumerate() {
        assert_eq!(
            p.pick_element(w, h, n[0], n[1], 0.0, 0.0),
            Some(Element::Node(i)),
            "node {i} must pick at its own centre"
        );
        assert_eq!(
            p.pick_element(w, h, n[0] + 12.0, n[1], 0.0, 0.0),
            Some(Element::Node(i)),
            "node {i} must pick off-centre but inside its box"
        );
    }

    // The edge runs between the two boxes; its midpoint picks the edge, not
    // either node, because nodes take priority only where they actually are.
    let mid = [(nodes[0][0] + nodes[1][0]) * 0.5, (nodes[0][1] + nodes[1][1]) * 0.5];
    assert_eq!(
        p.pick_element(w, h, mid[0], mid[1], 0.0, 3.0),
        Some(Element::Edge(0)),
        "the wire between the boxes must pick the edge"
    );

    // Far from everything: nothing.
    assert_eq!(p.pick_element(w, h, 2.0, 2.0, 4.0, 4.0), None);
}

#[test]
fn project_nodes_matches_pick_in_2d() {
    // The projected position of every node must be exactly where picking
    // finds it — the two must never be solved separately.
    let (w, h) = (500usize, 360usize);
    let mut p = demo_graph2d();
    p.camera.zoom_by(1.7);
    p.camera.pan(11.0, -6.0);
    for (i, n) in p.project_nodes(w, h).iter().enumerate() {
        assert_eq!(p.pick(w, h, n[0], n[1], 0.0), Some(i), "node {i} moved under the camera");
        assert_eq!(n[2], 0.0, "a 2D node reports no depth");
    }
}

#[test]
fn graph2d_hover_and_selection_light_nodes_and_edges() {
    let (w, h) = (400usize, 300usize);
    let mut p = demo_graph2d();
    let plain = hash(&p.render(w, h));
    for el in [Element::Node(0), Element::Node(1), Element::Edge(0)] {
        p.hovered = Some(el);
        let lit = p.render(w, h);
        assert_ne!(hash(&lit), plain, "{el:?} hovered must change the frame");
        assert!(has_color(&lit, [255, 255, 255]), "{el:?} hovered must light up white");
        p.hovered = None;
        p.selected = Some(el);
        assert_ne!(hash(&p.render(w, h)), plain, "{el:?} selected must change the frame");
        p.selected = None;
    }
    assert_eq!(hash(&p.render(w, h)), plain, "clearing both restores the frame exactly");
}

#[test]
fn graph2d_is_structural_and_extend_graph_grows_it() {
    let mut p = demo_graph2d();
    let h = 0;
    assert_eq!(p.extend_xy(h, &[0.0], &[0.0]), Err(TraceError::Structural));

    let one_shot = {
        let mut q = Plot::new();
        q.add_graph2d(
            vec![[0.0, 1.0], [0.0, 0.0], [1.0, 0.0]],
            vec!["alpha".into(), "beta".into(), "gamma".into()],
            vec![[250, 10, 10], [10, 250, 10], [80, 80, 80]],
            vec![(0, 1), (1, 2)],
            true,
            None,
            Some(vec![[10, 10, 250], [10, 10, 250]]),
            None,
            None,
        );
        q
    };
    p.extend_graph(h, &[[1.0, 0.0, 0.0]], &[[80, 80, 80]], &[(1, 2)], Some(&["gamma".into()]))
        .unwrap();
    // The appended edge inherits the trace's default colour rule, so the
    // one-shot build names it explicitly to match.
    p.set_graph_colors(
        h,
        vec![[250, 10, 10], [10, 250, 10], [80, 80, 80]],
        Some(vec![[10, 10, 250]; 2]),
    )
    .unwrap();
    assert_eq!(p.node_count(), 3, "appended nodes join the flat index space");
    assert_eq!(
        hash(&p.render(320, 240)),
        hash(&one_shot.render(320, 240)),
        "append must render exactly like a one-shot build"
    );
}

#[test]
fn graph2d_mutators_move_recolor_and_reroute() {
    let target = vec![[0.0, 2.0, 0.0], [1.0, 0.0, 0.0]];
    let one_shot = {
        let mut q = Plot::new();
        q.add_graph2d(
            vec![[0.0, 2.0], [1.0, 0.0]],
            vec!["alpha".into(), "beta".into()],
            vec![[250, 10, 10], [10, 250, 10]],
            vec![(0, 1)],
            true,
            None,
            Some(vec![[10, 10, 250]]),
            None,
            None,
        );
        q
    };
    // Start much wider than the target, so a widen-only bounds cache would
    // keep the old frame and the hashes would differ.
    let mut p = Plot::new();
    let h = p.add_graph2d(
        vec![[-9.0, 9.0], [9.0, -9.0]],
        vec!["alpha".into(), "beta".into()],
        vec![[250, 10, 10], [10, 250, 10]],
        vec![(0, 1)],
        true,
        None,
        Some(vec![[10, 10, 250]]),
        None,
        None,
    );
    p.set_graph_positions(h, target).unwrap();
    assert_eq!(
        hash(&p.render(320, 240)),
        hash(&one_shot.render(320, 240)),
        "a moved 2D graph must render like one built in place (bounds must shrink)"
    );

    // Muting survives a relayout: it is the host's intent, not geometry.
    p.set_muted(h, true).unwrap();
    p.set_graph_positions(h, vec![[0.0, 2.0, 0.0], [1.0, 0.0, 0.0]]).unwrap();
    assert!(!has_color(&p.render(320, 240), CARD), "a muted graph stays hidden across a relayout");
    p.set_muted(h, false).unwrap();

    let straight = hash(&p.render(320, 240));
    p.set_graph_routes(h, vec![[1.0, 2.0]], vec![0]).unwrap();
    assert_ne!(hash(&p.render(320, 240)), straight, "a waypoint must reroute the edge");
    p.set_graph_routes(h, vec![], vec![]).unwrap();
    assert_eq!(hash(&p.render(320, 240)), straight, "clearing routes restores straight edges");

    assert_eq!(p.set_graph_routes(99, vec![], vec![]), Err(TraceError::UnknownTrace));
    assert_eq!(p.set_graph_routes(h, vec![], vec![0, 1, 2]), Err(TraceError::LengthMismatch));
    assert_eq!(p.set_graph_positions(h, vec![[0.0; 3]]), Err(TraceError::LengthMismatch));
    assert_eq!(p.set_graph_colors(h, vec![[0; 3]], None), Err(TraceError::LengthMismatch));
}
