//! `InputMap` / `apply_drag`: hosts remap what drag gestures do; the default
//! reproduces the house feel exactly (drag rotates, shift-drag pans).

use plotui_core::{CameraControl, DragScales, InputMap, Plot};

const S: DragScales = DragScales { rotate: 0.01, pan_x: 1.0, pan_y: 1.0, zoom: 0.002 };

fn cam(p: &Plot) -> (f64, f64, f64, f64, f64) {
    p.camera.state()
}

#[test]
fn default_map_matches_direct_camera_calls() {
    let mut mapped = Plot::new();
    mapped.apply_drag(10.0, -4.0, false, S);
    let mut direct = Plot::new();
    direct.camera.rotate(-10.0 * 0.01, 0.0);
    direct.camera.rotate(0.0, 4.0 * 0.01);
    assert_eq!(cam(&mapped), cam(&direct));

    let mut mapped = Plot::new();
    mapped.apply_drag(6.0, 3.0, true, DragScales { pan_x: 2.0, pan_y: 2.0, ..S });
    let mut direct = Plot::new();
    direct.camera.pan(6.0 * 2.0, 0.0);
    direct.camera.pan(0.0, 3.0 * 2.0);
    assert_eq!(cam(&mapped), cam(&direct));
}

#[test]
fn swapped_axes_rotate_the_other_way_around() {
    let mut p = Plot::new();
    p.input_map =
        InputMap { drag_x: CameraControl::Pitch, drag_y: CameraControl::Yaw, ..Default::default() };
    let before = cam(&p);
    p.apply_drag(10.0, 0.0, false, S);
    let after = cam(&p);
    assert_eq!(after.0, before.0, "horizontal drag must not yaw in the swapped map");
    assert_ne!(after.1, before.1, "horizontal drag must pitch in the swapped map");
}

#[test]
fn pan_first_map_pans_without_rotating() {
    let mut p = Plot::new();
    p.input_map =
        InputMap { drag_x: CameraControl::PanX, drag_y: CameraControl::PanY, ..Default::default() };
    let (yaw0, pitch0, _, panx0, pany0) = cam(&p);
    p.apply_drag(12.0, 7.0, false, S);
    let (yaw1, pitch1, _, panx1, pany1) = cam(&p);
    assert_eq!((yaw1, pitch1), (yaw0, pitch0));
    assert_eq!((panx1 - panx0, pany1 - pany0), (12.0, 7.0));
}

#[test]
fn inverted_axes_restore_camera_grab() {
    // Both rotate axes inverted = the pre-trackball camera-grab feel.
    let mut inverted = Plot::new();
    inverted.input_map =
        InputMap { invert_drag_x: true, invert_drag_y: true, ..Default::default() };
    inverted.apply_drag(10.0, -4.0, false, S);
    let mut direct = Plot::new();
    direct.camera.rotate(10.0 * 0.01, 0.0);
    direct.camera.rotate(0.0, -4.0 * 0.01);
    assert_eq!(cam(&inverted), cam(&direct));

    // Shift-drag inversion is independent of the plain-drag flags.
    let mut p = Plot::new();
    p.input_map = InputMap { invert_shift_drag_x: true, ..Default::default() };
    p.apply_drag(6.0, 3.0, true, DragScales { pan_x: 2.0, pan_y: 2.0, ..S });
    let (.., pan_x, pan_y) = cam(&p);
    assert_eq!((pan_x, pan_y), (-12.0, 6.0));
}

#[test]
fn zoom_and_off_axes() {
    let mut p = Plot::new();
    p.input_map =
        InputMap { drag_x: CameraControl::Off, drag_y: CameraControl::Zoom, ..Default::default() };
    let before = cam(&p);
    p.apply_drag(50.0, -10.0, false, DragScales { zoom: 0.01, ..S });
    let after = cam(&p);
    assert_eq!((after.0, after.1, after.3, after.4), (before.0, before.1, before.3, before.4));
    assert!(after.2 > before.2, "dragging up must zoom in");
}

/// A ring in the z = 0 plane, and the index of the point currently facing
/// the viewer — the one under the pointer when you grab the middle of the
/// scene. Tracking *that* point through a gesture is the only honest way to
/// ask which way the object turned: "whichever point is nearest now" is a
/// different point after any rotation.
fn ring() -> Vec<[f32; 3]> {
    (0..256)
        .map(|i| {
            let a = std::f32::consts::TAU * i as f32 / 256.0;
            [a.cos(), a.sin(), 0.0]
        })
        .collect()
}

fn front_of(plot: &Plot, pts: &[[f32; 3]]) -> usize {
    (0..pts.len())
        .min_by(|&a, &b| {
            plot.project_point(800, 600, pts[a])[2]
                .total_cmp(&plot.project_point(800, 600, pts[b])[2])
        })
        .expect("non-empty ring")
}

fn scene() -> (Plot, Vec<[f32; 3]>) {
    let pts = ring();
    let mut plot = Plot::new();
    plot.add_scatter3d(pts.clone(), [200, 200, 200], 2.0, None);
    (plot, pts)
}

/// The house feel, stated as what the user sees rather than as a sign: the
/// object goes where the pointer takes it, on both axes.
#[test]
fn dragging_carries_the_object_with_the_pointer() {
    for (dx, dy, axis, label) in [
        (60.0, 0.0, 0usize, "drag right must carry the object right"),
        (0.0, 60.0, 1usize, "drag down must carry the object down"),
    ] {
        let (mut plot, pts) = scene();
        let front = front_of(&plot, &pts);
        let before = plot.project_point(800, 600, pts[front]);
        plot.apply_drag(dx, dy, false, S);
        let after = plot.project_point(800, 600, pts[front]);
        // Screen y grows downward, so "down" is also an increase.
        assert!(after[axis] > before[axis], "{label} ({} -> {})", before[axis], after[axis]);
    }
}

/// The bug this pins: an auto-spin that runs against the drag makes the
/// drag itself feel inverted, because releasing a grabbed object sends it
/// back the way it came. `spin` is *defined* as the drag it agrees with, so
/// the two can never drift apart again.
#[test]
fn a_spin_agrees_with_the_drag_it_is_named_after() {
    let (mut spun, pts) = scene();
    let front = front_of(&spun, &pts);
    let before = spun.project_point(800, 600, pts[front]);
    spun.spin(0.35);
    let after = spun.project_point(800, 600, pts[front]);
    assert!(after[0] > before[0], "a positive spin must carry the object right");

    // The same camera as the rightward drag that produces it (to rounding:
    // 0.35 and 35.0 * 0.01 are not the same f64).
    let mut dragged = Plot::new();
    dragged.apply_drag(35.0, 0.0, false, DragScales { rotate: 0.01, ..S });
    let (a, b) = (cam(&spun), cam(&dragged));
    assert!((a.0 - b.0).abs() < 1e-12, "spin yaw {} != drag yaw {}", a.0, b.0);
    assert_eq!((a.1, a.2, a.3, a.4), (b.1, b.2, b.3, b.4));

    // A negative step drifts the other way, and inverting the drag axis
    // inverts the spin with it — one convention, not two.
    let (mut back, _) = scene();
    back.spin(-0.35);
    let after = back.project_point(800, 600, pts[front]);
    assert!(after[0] < before[0], "a negative spin must carry the object left");

    let (mut inverted, _) = scene();
    inverted.input_map = InputMap { invert_drag_x: true, ..Default::default() };
    inverted.spin(0.35);
    let after = inverted.project_point(800, 600, pts[front]);
    assert!(after[0] < before[0], "invert_drag_x must invert the spin too");
}
