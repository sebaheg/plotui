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
