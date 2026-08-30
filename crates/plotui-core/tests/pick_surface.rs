//! `pick_surface`: surfaces are not node-pick targets, but hover queries
//! return the nearest grid vertex with its data coordinates.

use plotui_core::Plot;

fn peaks_plot() -> (Plot, usize) {
    let n = 9usize;
    let xs: Vec<f32> = (0..n).map(|i| -1.0 + 2.0 * i as f32 / (n - 1) as f32).collect();
    let ys = xs.clone();
    let mut zs = Vec::with_capacity(n * n);
    for y in &ys {
        for x in &xs {
            zs.push((-(x * x + y * y)).exp());
        }
    }
    let mut plot = Plot::new();
    let id = plot.add_surface3d(xs, ys, zs, [70, 190, 120], None, false, None);
    (plot, id)
}

#[test]
fn hit_round_trips_through_its_own_screen_position() {
    let (plot, _) = peaks_plot();
    // Somewhere near the middle of the canvas there is always a vertex.
    let hit = plot.pick_surface(200, 200, 100.0, 100.0, 1e4).expect("some vertex");
    let again = plot
        .pick_surface(200, 200, hit.screen[0], hit.screen[1], 4.0)
        .expect("probing a hit's own screen position");
    assert_eq!(again.data, hit.data);
    // The data point is a real grid vertex: z matches the generator.
    let [x, y, z] = hit.data;
    assert!((z - (-(x * x + y * y)).exp()).abs() < 1e-6);
}

#[test]
fn out_of_radius_and_hidden_surfaces_miss() {
    let (mut plot, id) = peaks_plot();
    assert!(plot.pick_surface(200, 200, 100.0, 100.0, 0.0).is_none());
    plot.set_visible(id, false).unwrap();
    assert!(plot.pick_surface(200, 200, 100.0, 100.0, 1e4).is_none());
}

#[test]
fn surfaces_do_not_join_node_picking() {
    let (plot, _) = peaks_plot();
    assert_eq!(plot.node_count(), 0);
    assert!(plot.pick(200, 200, 100.0, 100.0, 1e4).is_none());
}

#[test]
fn surface_hover_draws_guides_and_clears() {
    let (mut plot, _) = peaks_plot();
    let plain = plot.render(200, 200).rgba();
    let hit = plot.pick_surface(200, 200, 100.0, 100.0, 1e4).unwrap();
    assert!(plot.set_surface_hover(Some(hit.data)));
    assert!(!plot.set_surface_hover(Some(hit.data)), "same value is not a change");
    let hovered = plot.render(200, 200).rgba();
    assert_ne!(plain, hovered, "hover guides drew nothing");
    assert!(plot.set_surface_hover(None));
    assert_eq!(plot.render(200, 200).rgba(), plain, "clearing must restore the frame");
}

#[test]
fn surface_selection_pins_guides_and_projects() {
    let (mut plot, _) = peaks_plot();
    let plain = plot.render(200, 200).rgba();
    let hit = plot.pick_surface(200, 200, 100.0, 100.0, 1e4).unwrap();
    assert!(plot.set_surface_selected(Some(hit.data)));
    let pinned = plot.render(200, 200).rgba();
    assert_ne!(plain, pinned, "selection guides drew nothing");
    // The projection accessor matches the pick's own screen position.
    assert_eq!(plot.project_point(200, 200, hit.data), hit.screen);
    // The selection survives a camera change and reprojects consistently.
    plot.camera.rotate(0.4, -0.2);
    let s = plot.project_point(200, 200, hit.data);
    assert_ne!([s[0], s[1]], [hit.screen[0], hit.screen[1]]);
    assert!(plot.set_surface_selected(None));
    plot.camera.reset();
    assert_eq!(plot.render(200, 200).rgba(), plain, "clearing must restore the frame");
}

#[test]
fn floor_frame_is_always_drawn_for_surfaces() {
    let (mut plot, _) = peaks_plot();
    let with_frame = plot.render(200, 200).rgba();
    plot.show_box = false;
    let without_box = plot.render(200, 200).rgba();
    assert_ne!(with_frame, without_box);
}

#[test]
fn surface_hover_is_ignored_in_2d() {
    let mut plot = Plot::new();
    plot.add_line2d(
        vec![0.0, 1.0, 2.0],
        vec![0.0, 1.0, 0.5],
        [230, 60, 120],
        2.0,
        None,
        plotui_core::YAxis::Primary,
    );
    let plain = plot.render(200, 200).rgba();
    plot.set_surface_hover(Some([1.0, 0.5, 0.0]));
    assert_eq!(plot.render(200, 200).rgba(), plain);
}
