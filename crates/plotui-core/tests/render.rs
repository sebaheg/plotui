//! Behavior tests for the rendering engine, written against the public API.
//!
//! Golden values are structural (pixel counts, specific probed pixels, hashes
//! compared within one process) rather than hard-coded image hashes, so they
//! hold on any platform regardless of libm rounding in the camera trig.

use plotui_core::{draw_text, nice_ticks, Framebuffer, Plot, PALETTE};

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
    );
    p
}

fn demo_2d() -> Plot {
    let mut p = Plot::new();
    let xs: Vec<f32> = (0..=20).map(|i| i as f32).collect();
    let ys: Vec<f32> = xs.iter().map(|x| (x * 0.5).sin() * 3.0 + 5.0).collect();
    p.add_line2d(xs, ys, PALETTE[0], 2.0, Some("signal".into()));
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
    p.add_line2d(vec![0.0, 1.0], vec![0.0, 1.0], PALETTE[0], 2.0, None);
    let unnamed = p.render(400, 240);
    assert!(!has_color(&unnamed, [205, 210, 220]), "no legend without names");
}

#[test]
fn bars_fill_from_the_zero_baseline() {
    let mut p = Plot::new();
    p.add_bar2d(vec![0.0, 1.0, 2.0], vec![3.0, 1.0, 2.0], PALETTE[2], None);
    let fb = p.render(400, 240);
    let bar_px = drawn_pixels(&fb).into_iter().filter(|(_, _, c)| *c == PALETTE[2]).count();
    // Three bars on a 400x240 canvas are a large filled area, not a sliver.
    assert!(bar_px > 2000, "bars should be solid fills, got {bar_px} px");
}

#[test]
fn scatter2d_marks_all_points() {
    let mut p = Plot::new();
    p.add_scatter2d(vec![0.0, 5.0, 10.0], vec![0.0, 5.0, 10.0], PALETTE[5], 3.0, None);
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
    );
    let fb = p.render(300, 200);
    assert!(has_color(&fb, PALETTE[0]), "finite segments still draw");
}

#[test]
fn mixed_2d_and_3d_uses_the_3d_camera_path() {
    let mut p = demo_3d();
    p.add_line2d(vec![0.0, 1.0], vec![0.0, 1.0], PALETTE[0], 2.0, Some("x".into()));
    let fb = p.render(300, 200);
    // 3D path: no axes frame, no legend ink.
    assert!(!has_color(&fb, [205, 210, 220]));
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

use plotui_core::Element;

fn demo_graph() -> (Plot, Vec<[f32; 3]>, Vec<(u32, u32)>) {
    let nodes = vec![[0.0, 0.0, 0.0], [5.0, 5.0, 5.0], [-5.0, -5.0, -5.0], [5.0, -5.0, 0.0]];
    let edges = vec![(0u32, 1u32), (1, 2), (0, 3)];
    let mut p = Plot::new();
    p.add_graph3d(nodes.clone(), vec![[200, 100, 100]; 4], edges.clone(), 3.0, None, None);
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
    p.add_scatter3d(vec![[0.0, 0.0, 0.0]], [255, 0, 0], 3.0);
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
    p.add_scatter3d(vec![[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]], [1, 2, 3], 1.0);
    p.add_graph3d(vec![[0.0; 3]; 3], vec![[9, 9, 9]; 3], vec![(0, 1)], 1.0, None, None);
    assert_eq!(p.node_count(), 5);
}
