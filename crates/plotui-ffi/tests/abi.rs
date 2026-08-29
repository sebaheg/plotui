//! Tests through the C ABI itself (the extern "C" functions are ordinary
//! Rust symbols here), mirroring tests/test_plotui.py's structural checks.

use std::ffi::{CStr, CString};
use std::ptr;

use plotui_ffi::*;

fn last_error() -> String {
    unsafe { CStr::from_ptr(plotui_last_error()) }.to_str().unwrap().to_string()
}

fn take_string(s: *mut std::ffi::c_char) -> String {
    assert!(!s.is_null());
    let out = unsafe { CStr::from_ptr(s) }.to_str().unwrap().to_string();
    unsafe { plotui_string_free(s) };
    out
}

/// A 2D plot with one auto-colored line.
fn plot_2d() -> *mut PlotuiPlot {
    let p = plotui_new();
    let xs = [0.0f32, 1.0, 2.0];
    let ys = [0.0f32, 2.0, 1.0];
    let mut h = usize::MAX;
    let status = unsafe {
        plotui_add_line2d(
            p,
            xs.as_ptr(),
            3,
            ys.as_ptr(),
            3,
            ptr::null(),
            2.0,
            ptr::null(),
            ptr::null(),
            &mut h,
        )
    };
    assert_eq!(status, PLOTUI_OK);
    assert_eq!(h, 0);
    p
}

#[test]
fn lifecycle_and_null_safety() {
    let p = plotui_new();
    unsafe {
        assert!(!plotui_is_3d(p));
        assert_eq!(plotui_node_count(p), 0);
        plotui_free(p);
        // NULL handles are inert, not UB.
        plotui_free(ptr::null_mut());
        plotui_string_free(ptr::null_mut());
        assert_eq!(plotui_render_rgba(ptr::null(), 4, 4, ptr::null_mut()), PLOTUI_ERR_NULL);
        assert_eq!(last_error(), "null plot handle");
    }
}

#[test]
fn each_plot_gets_its_own_image_id() {
    let (a, b) = (plotui_new(), plotui_new());
    unsafe {
        let (ida, idb) = (plotui_image_id(a), plotui_image_id(b));
        assert_ne!(ida, idb);
        let cleanup = take_string(plotui_kitty_cleanup(a));
        assert_eq!(cleanup, format!("\x1b_Ga=d,d=i,i={ida}\x1b\\"));
        plotui_free(a);
        plotui_free(b);
    }
}

#[test]
fn render_rgba_writes_structured_pixels() {
    let p = plot_2d();
    let (w, h) = (160usize, 120usize);
    let mut buf = vec![0u8; w * h * 4];
    unsafe {
        assert_eq!(plotui_render_rgba(p, w, h, buf.as_mut_ptr()), PLOTUI_OK);
        plotui_free(p);
    }
    let drawn = buf.chunks_exact(4).filter(|px| px[3] != 0).count();
    assert!(drawn > 0, "something must be drawn");
    assert!(drawn < w * h, "undrawn pixels keep alpha 0 (the plot floats on the terminal)");
    // The first palette slot must appear (the auto-assigned line color).
    let palette0 = plotui_core::PALETTE[0];
    assert!(buf
        .chunks_exact(4)
        .any(|px| px[0] == palette0[0] && px[1] == palette0[1] && px[2] == palette0[2]));
}

#[test]
fn render_kitty_matches_the_python_contract() {
    let p = plot_2d();
    // Ids are allocated process-wide, so read this plot's rather than
    // assuming the 4242 default (tests run in parallel).
    let id = unsafe { plotui_image_id(p) };
    let mut escape = ptr::null_mut();
    unsafe {
        assert_eq!(
            plotui_render_kitty(p, 20, 10, 8, 16, false, 1.0, false, &mut escape),
            PLOTUI_OK
        );
    }
    let s = take_string(escape);
    assert!(s.starts_with("\x1b[s\x1b_G"), "save cursor, then APC");
    assert!(s.ends_with("\x1b[u"), "restore cursor");
    assert!(s.contains(&format!("i={id}")), "the plot's own image id");
    assert!(s.contains("s=160,v=160") && s.contains("c=20,r=10"));

    // compat framing + replace skips the delete.
    let mut escape = ptr::null_mut();
    unsafe {
        assert_eq!(plotui_render_kitty(p, 20, 10, 8, 16, true, 1.0, true, &mut escape), PLOTUI_OK);
        plotui_free(p);
    }
    let s = take_string(escape);
    assert!(!s.contains("a=d"), "replace=true skips delete-before-transmit");
}

#[test]
fn placeholder_meta_reconstructs_the_cells_exactly() {
    let p = plot_2d();
    let (cols, rows) = (12u16, 6u16);
    let mut transmit = ptr::null_mut();
    let mut id_rgb = [0u8; 3];
    let mut extra = 0u8;
    let image_id = unsafe { plotui_image_id(p) };
    unsafe {
        assert_eq!(
            plotui_render_kitty_placeholder_meta(
                p,
                cols,
                rows,
                8,
                16,
                1.0,
                &mut transmit,
                id_rgb.as_mut_ptr(),
                &mut extra,
            ),
            PLOTUI_OK
        );
    }
    let transmit = take_string(transmit);

    // Reference: the full cells from plotui-protocol on an identical frame.
    let reference = {
        let mut plot = plotui_core::Plot::new();
        let c = plot.resolve_color(None);
        plot.add_line2d(
            vec![0.0, 1.0, 2.0],
            vec![0.0, 2.0, 1.0],
            c,
            2.0,
            None,
            plotui_core::YAxis::Primary,
        );
        let fb = plot.render(cols as usize * 8, rows as usize * 16);
        plotui_protocol::kitty_placeholder_cells_with_id(&fb, cols, rows, image_id)
    };
    assert_eq!(transmit, reference.transmit);
    assert_eq!((id_rgb[0], id_rgb[1], id_rgb[2]), reference.id_rgb);

    // Synthesize cells from the meta exactly as the Go side will…
    let mut n = 0usize;
    let table = unsafe { plotui_diacritics(&mut n) };
    assert_eq!(n, 297);
    let dia = unsafe { std::slice::from_raw_parts(table, n) };
    for (y, row) in reference.cells.iter().enumerate() {
        for (x, cell) in row.iter().enumerate() {
            let mut synth = String::from('\u{10EEEE}');
            synth.push(char::from_u32(dia[y % n]).unwrap());
            synth.push(char::from_u32(dia[x % n]).unwrap());
            if extra != 0 {
                synth.push(char::from_u32(dia[extra as usize % n]).unwrap());
            }
            assert_eq!(&synth, cell, "cell ({x},{y}) must reconstruct byte-identically");
        }
    }
    unsafe { plotui_free(p) };
}

#[test]
fn error_paths_carry_the_shared_messages() {
    let p = plot_2d();
    unsafe {
        // Unknown handle.
        let xs = [0.0f32];
        assert_eq!(
            plotui_extend(p, 99, xs.as_ptr(), 1, xs.as_ptr(), 1, ptr::null(), 0),
            PLOTUI_ERR_UNKNOWN_HANDLE
        );
        assert_eq!(last_error(), "unknown trace handle 99");

        // zs on a 2D trace.
        assert_eq!(
            plotui_extend(p, 0, xs.as_ptr(), 1, xs.as_ptr(), 1, xs.as_ptr(), 1),
            PLOTUI_ERR_INVALID_ARG
        );
        assert_eq!(last_error(), "2D trace: extend takes (xs, ys) — zs is for 3D traces");

        // Bad axis string.
        let axis = CString::new("y4").unwrap();
        assert_eq!(
            plotui_add_line2d(
                p,
                xs.as_ptr(),
                1,
                xs.as_ptr(),
                1,
                ptr::null(),
                1.0,
                ptr::null(),
                axis.as_ptr(),
                ptr::null_mut(),
            ),
            PLOTUI_ERR_INVALID_ARG
        );
        assert_eq!(last_error(), "axis must be 'y', 'y2' or 'y3', got \"y4\"");

        // Ragged surface grid.
        assert_eq!(
            plotui_add_surface3d(
                p,
                xs.as_ptr(),
                1,
                xs.as_ptr(),
                1,
                xs.as_ptr(),
                0, // 0 heights for a 1×1 grid
                ptr::null(),
                ptr::null(),
                false,
                ptr::null(),
                ptr::null_mut(),
            ),
            PLOTUI_ERR_INVALID_ARG
        );
        assert!(last_error().starts_with("zs must be a 1×1 grid"));

        // Bad shape name on a graph.
        let shape = CString::new("blob").unwrap();
        let shapes = [shape.as_ptr()];
        assert_eq!(
            plotui_add_graph3d(
                p,
                xs.as_ptr(),
                1,
                xs.as_ptr(),
                1,
                xs.as_ptr(),
                1,
                ptr::null(),
                0,
                ptr::null(),
                0,
                ptr::null(),
                3.5,
                ptr::null(),
                0,
                ptr::null(),
                0,
                shapes.as_ptr(),
                1,
                ptr::null_mut(),
            ),
            PLOTUI_ERR_INVALID_ARG
        );
        assert!(last_error().starts_with("unknown node shape \"blob\""));

        // Structural extend.
        let mut hg = 0usize;
        assert_eq!(
            plotui_add_graph3d(
                p,
                xs.as_ptr(),
                1,
                xs.as_ptr(),
                1,
                xs.as_ptr(),
                1,
                ptr::null(),
                0,
                ptr::null(),
                0,
                ptr::null(),
                3.5,
                ptr::null(),
                0,
                ptr::null(),
                0,
                ptr::null(),
                0,
                &mut hg,
            ),
            PLOTUI_OK
        );
        let zs = [0.0f32];
        assert_eq!(
            plotui_extend(p, hg, xs.as_ptr(), 1, xs.as_ptr(), 1, zs.as_ptr(), 1),
            PLOTUI_ERR_STRUCTURAL
        );
        assert!(last_error().starts_with("graph3d traces are structural"));
        plotui_free(p);
    }
}

#[test]
fn camera_and_interaction_roundtrip() {
    let p = plot_2d();
    unsafe {
        let mut state = [0.0f64; 5];
        plotui_camera_state(p, state.as_mut_ptr());
        plotui_rotate(p, 0.25, 0.0);
        plotui_zoom_by(p, 1.5);
        plotui_pan(p, 10.0, -5.0);
        let mut moved = [0.0f64; 5];
        plotui_camera_state(p, moved.as_mut_ptr());
        assert!((moved[0] - state[0] - 0.25).abs() < 1e-9);
        assert!((moved[2] - 1.5).abs() < 1e-9);
        assert_eq!((moved[3], moved[4]), (10.0, -5.0));
        plotui_set_camera_state(p, state[0], state[1], state[2], state[3], state[4]);
        let mut restored = [0.0f64; 5];
        plotui_camera_state(p, restored.as_mut_ptr());
        assert_eq!(restored, state);

        // Visibility, hover change-detection, selection.
        let mut changed = false;
        assert_eq!(plotui_set_visible(p, 0, false, &mut changed), PLOTUI_OK);
        assert!(changed);
        assert_eq!(plotui_set_visible(p, 0, false, &mut changed), PLOTUI_OK);
        assert!(!changed);
        assert_eq!(plotui_set_visible(p, 9, true, &mut changed), PLOTUI_ERR_UNKNOWN_HANDLE);

        assert!(plotui_set_hover2d(p, true, 42.0));
        assert!(!plotui_set_hover2d(p, true, 42.0));
        let mut hchanged = false;
        assert_eq!(plotui_set_hovered(p, 1, 3, &mut hchanged), PLOTUI_OK);
        assert!(hchanged);
        assert_eq!(plotui_set_hovered(p, 7, 3, &mut hchanged), PLOTUI_ERR_INVALID_ARG);
        assert_eq!(plotui_set_selected(p, 0, 0), PLOTUI_OK);
        plotui_free(p);
    }
}

#[test]
fn interactive_scale_follows_the_shared_policy() {
    let p = plotui_new();
    let n = 500usize;
    let coords: Vec<f32> = (0..n).map(|i| i as f32).collect();
    unsafe {
        assert_eq!(
            plotui_add_scatter3d(
                p,
                coords.as_ptr(),
                n,
                coords.as_ptr(),
                n,
                coords.as_ptr(),
                n,
                ptr::null(),
                2.0,
                ptr::null_mut(),
            ),
            PLOTUI_OK
        );
        assert!(plotui_is_3d(p));
        assert_eq!(plotui_vertex_count(p), n);
        assert_eq!(plotui_interactive_scale(p, true, 0.5), 0.5);
        assert_eq!(plotui_interactive_scale(p, false, 0.5), 1.0);

        // project_nodes writes node_count * 3 floats.
        let mut out = vec![0.0f32; n * 3];
        assert_eq!(plotui_project_nodes(p, 160, 160, out.as_mut_ptr()), PLOTUI_OK);
        assert!(out.iter().any(|&v| v != 0.0));
        plotui_free(p);
    }
}
