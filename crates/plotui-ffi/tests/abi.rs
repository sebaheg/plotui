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
    let pixels = buf.as_chunks::<4>().0;
    let drawn = pixels.iter().filter(|px| px[3] != 0).count();
    assert!(drawn > 0, "something must be drawn");
    assert!(drawn < w * h, "undrawn pixels keep alpha 0 (the plot floats on the terminal)");
    // The first palette slot must appear (the auto-assigned line color).
    let palette0 = plotui_core::PALETTE[0];
    assert!(pixels
        .iter()
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

        // Mesh tris that are not whole triples.
        let tris = [0u32, 0, 0, 0];
        assert_eq!(
            plotui_add_mesh3d(
                p,
                xs.as_ptr(),
                1,
                xs.as_ptr(),
                1,
                xs.as_ptr(),
                1,
                tris.as_ptr(),
                4,
                ptr::null(),
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
            ),
            PLOTUI_ERR_INVALID_ARG
        );
        assert_eq!(last_error(), "tris must be flat [a, b, c] vertex triples; got 4 indices");

        // Mesh index naming no vertex.
        let tris = [0u32, 1, 2];
        assert_eq!(
            plotui_add_mesh3d(
                p,
                xs.as_ptr(),
                1,
                xs.as_ptr(),
                1,
                xs.as_ptr(),
                1,
                tris.as_ptr(),
                3,
                ptr::null(),
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
            ),
            PLOTUI_ERR_INVALID_ARG
        );
        assert_eq!(last_error(), "triangle index 1 names no vertex; the mesh has 1");

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
                ptr::null(),
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
                ptr::null(),
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
                ptr::null(),
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

#[test]
fn range_slider_roundtrip() {
    let p = plot_2d();
    unsafe {
        // Window set / read / change-detect / validate / clear.
        let mut changed = false;
        assert_eq!(plotui_set_x_window(p, true, 0.5, 1.5, &mut changed), PLOTUI_OK);
        assert!(changed);
        assert_eq!(plotui_set_x_window(p, true, 0.5, 1.5, &mut changed), PLOTUI_OK);
        assert!(!changed);
        let (mut lo, mut hi) = (0.0f64, 0.0f64);
        assert!(plotui_x_window(p, &mut lo, &mut hi));
        assert_eq!((lo, hi), (0.5, 1.5));
        assert_eq!(plotui_set_x_window(p, true, 2.0, 2.0, &mut changed), PLOTUI_ERR_INVALID_ARG);
        assert_eq!(last_error(), "x_window needs finite lo < hi, got (2, 2)");
        assert_eq!(plotui_set_x_window(p, false, 0.0, 0.0, &mut changed), PLOTUI_OK);
        assert!(changed);
        assert!(!plotui_x_window(p, &mut lo, &mut hi));

        // Epoch set / read / validate.
        assert_eq!(plotui_set_x_epoch(p, true, 1.7e9, &mut changed), PLOTUI_OK);
        assert!(changed);
        let mut epoch = 0.0f64;
        assert!(plotui_x_epoch(p, &mut epoch));
        assert_eq!(epoch, 1.7e9);
        assert_eq!(plotui_set_x_epoch(p, true, f64::NAN, &mut changed), PLOTUI_ERR_INVALID_ARG);

        // Strip toggle, hit, drag: exercised at a size where the strip is
        // live (400x240; the strip hugs the bottom rows).
        assert!(plotui_set_range_slider(p, true));
        assert!(!plotui_set_range_slider(p, true));
        let mut part = -1i32;
        assert_eq!(plotui_range_slider_hit(p, 400, 240, 200.0, 224.0, 4.0, &mut part), PLOTUI_OK);
        assert_ne!(part, 0, "mid-strip point must hit something");
        // Shrink from the full extent via the right handle, so the window
        // has room to jump and pan.
        assert_eq!(plotui_drag_x_window(p, 400, 240, 2, -120.0, &mut changed), PLOTUI_OK);
        assert!(changed);
        assert!(plotui_x_window(p, &mut lo, &mut hi), "a drag materializes a window");
        assert!(hi < 2.0, "right handle must have pulled the window in, got hi={hi}");
        assert_eq!(plotui_drag_x_window(p, 400, 240, 9, 1.0, &mut changed), PLOTUI_ERR_INVALID_ARG);
        assert_eq!(
            last_error(),
            "range part must be 1 (left), 2 (right), 3 (window) or 4 (track), got 9"
        );
        assert!(plotui_jump_x_window(p, 400, 240, 300.0));
        assert!(plotui_pan_x_window(p, 400, 240, 25.0));
        assert!(plotui_zoom_x_window(p, 400, 240, 200.0, 2.0));
        plotui_free(p);
    }
}

#[test]
fn graph_mutators_and_layout_roundtrip() {
    unsafe {
        // A settled layout drives a graph through the C ABI end to end.
        let edges = [0u32, 1, 1, 2];
        let l = plotui_layout_new(3, edges.as_ptr(), 2, 7);
        assert!(!l.is_null());
        let mut energy = f32::INFINITY;
        for _ in 0..600 {
            energy = plotui_layout_step(l);
        }
        assert!(energy < 1e-3, "layout must settle, got {energy}");
        assert_eq!(plotui_layout_node_count(l), 3);
        let mut pos = [0.0f32; 9];
        assert_eq!(plotui_layout_positions(l, pos.as_mut_ptr()), PLOTUI_OK);

        // Build a graph and move it onto the layout's positions.
        let p = plotui_new();
        let (xs, ys, zs): (Vec<f32>, Vec<f32>, Vec<f32>) = (
            pos.chunks(3).map(|c| c[0]).collect(),
            pos.chunks(3).map(|c| c[1]).collect(),
            pos.chunks(3).map(|c| c[2]).collect(),
        );
        let mut h = usize::MAX;
        assert_eq!(
            plotui_add_graph3d(
                p,
                xs.as_ptr(),
                3,
                ys.as_ptr(),
                3,
                zs.as_ptr(),
                3,
                edges.as_ptr(),
                2,
                ptr::null(),
                0,
                ptr::null(),
                3.0,
                ptr::null(),
                0,
                ptr::null(),
                0,
                ptr::null(),
                0,
                ptr::null(),
                &mut h,
            ),
            PLOTUI_OK
        );
        assert_eq!(
            plotui_set_graph_positions(p, h, xs.as_ptr(), 3, ys.as_ptr(), 3, zs.as_ptr(), 3),
            PLOTUI_OK
        );

        // Recolor, then restore the default edge blend with NULL edges.
        let node_rgbs = [9u8, 250, 9, 9, 250, 9, 9, 250, 9];
        let edge_rgbs = [250u8, 9, 9, 250, 9, 9];
        assert_eq!(
            plotui_set_graph_colors(p, h, node_rgbs.as_ptr(), 3, edge_rgbs.as_ptr(), 2),
            PLOTUI_OK
        );
        assert_eq!(plotui_set_graph_colors(p, h, node_rgbs.as_ptr(), 3, ptr::null(), 0), PLOTUI_OK);

        // A node flies in: layout first, then the trace.
        let neighbors = [0u32];
        let mut idx = usize::MAX;
        assert_eq!(plotui_layout_add_node(l, neighbors.as_ptr(), 1, &mut idx), PLOTUI_OK);
        assert_eq!(idx, 3);
        let (nx, ny, nz) = (&[0.1f32], &[0.2f32], &[0.3f32]);
        let new_rgb = [69u8, 200, 209];
        let new_edges = [0u32, 3];
        assert_eq!(
            plotui_extend_graph(
                p,
                h,
                nx.as_ptr(),
                1,
                ny.as_ptr(),
                1,
                nz.as_ptr(),
                1,
                new_rgb.as_ptr(),
                1,
                new_edges.as_ptr(),
                1,
            ),
            PLOTUI_OK
        );
        assert_eq!(plotui_node_count(p), 4);

        // Error paths carry the core's canonical messages.
        assert_eq!(
            plotui_set_graph_positions(p, 99, nx.as_ptr(), 1, ny.as_ptr(), 1, nz.as_ptr(), 1),
            PLOTUI_ERR_UNKNOWN_HANDLE
        );
        assert_eq!(last_error(), "unknown trace handle");
        assert_eq!(
            plotui_set_graph_positions(p, h, nx.as_ptr(), 1, ny.as_ptr(), 1, nz.as_ptr(), 1),
            PLOTUI_ERR_INVALID_ARG
        );
        assert_eq!(
            last_error(),
            "per-node/per-edge array length must match the trace's node/edge count"
        );

        plotui_free(p);
        plotui_layout_free(l);
        plotui_layout_free(ptr::null_mut()); // NULL is inert
    }
}

/// A 2D graph over the C ABI: layout, add, pick, relayout, and the DOT
/// composer — every entry point a frontend needs to draw a pipeline.
#[test]
fn graph2d_round_trips_through_the_abi() {
    // 0 -> 1 -> 2 plus a 0 -> 2 edge that skips a rank, so the layout has a
    // route to hand back.
    let edges = [0u32, 1, 1, 2, 0, 2];
    unsafe {
        let tb = CString::new("TB").unwrap();
        let l = plotui_layered_layout_new(3, edges.as_ptr(), 3, tb.as_ptr());
        assert!(!l.is_null());
        let mut xy = [0f32; 6];
        let mut ranks = [0u32; 3];
        assert_eq!(
            plotui_layered_layout_positions(l, xy.as_mut_ptr(), ranks.as_mut_ptr()),
            PLOTUI_OK
        );
        assert_eq!(ranks, [0, 1, 2], "rank follows edge direction");
        let n_pts = plotui_layered_layout_route_count(l);
        assert_eq!(n_pts, 1, "the skipping edge gets one waypoint");
        let mut pts = vec![0f32; n_pts * 2];
        let mut starts = [0u32; 3];
        assert_eq!(
            plotui_layered_layout_routes(l, pts.as_mut_ptr(), starts.as_mut_ptr()),
            PLOTUI_OK
        );

        let p = plotui_new();
        let (xs, ys): (Vec<f32>, Vec<f32>) =
            (xy.chunks(2).map(|c| c[0]).collect(), xy.chunks(2).map(|c| c[1]).collect());
        let labels: Vec<CString> =
            ["fetch", "clean", "publish"].iter().map(|s| CString::new(*s).unwrap()).collect();
        let label_ptrs: Vec<*const std::ffi::c_char> = labels.iter().map(|c| c.as_ptr()).collect();
        let shapes: Vec<CString> =
            ["rounded", "box", "ellipse"].iter().map(|s| CString::new(*s).unwrap()).collect();
        let shape_ptrs: Vec<*const std::ffi::c_char> = shapes.iter().map(|c| c.as_ptr()).collect();
        let node_rgbs = [250u8, 10, 10, 10, 250, 10, 10, 10, 250];
        let name = CString::new("nightly").unwrap();
        let mut h = usize::MAX;
        assert_eq!(
            plotui_add_graph2d(
                p,
                xs.as_ptr(),
                3,
                ys.as_ptr(),
                3,
                label_ptrs.as_ptr(),
                3,
                edges.as_ptr(),
                3,
                true,
                node_rgbs.as_ptr(),
                3,
                ptr::null(),
                shape_ptrs.as_ptr(),
                3,
                ptr::null(),
                0,
                pts.as_ptr(),
                n_pts,
                starts.as_ptr(),
                3,
                name.as_ptr(),
                &mut h,
            ),
            PLOTUI_OK
        );
        assert_eq!(plotui_node_count(p), 3);

        // The graph's own nodes are pickable through the 2D path.
        let mut screen = [0f32; 9];
        assert_eq!(plotui_project_nodes(p, 400, 300, screen.as_mut_ptr()), PLOTUI_OK);
        let mut kind = -1i32;
        let mut index = usize::MAX;
        assert_eq!(
            plotui_pick_element_px(
                p, 400, 300, screen[3], screen[4], 0.0, 0.0, &mut kind, &mut index,
            ),
            PLOTUI_OK
        );
        assert_eq!((kind, index), (1, 1), "the middle node's own centre picks it");

        // A relayout moves the nodes and rewrites the routes.
        let (mx, my, mz) = ([0f32, 1.0, 2.0], [2f32, 1.0, 0.0], [0f32; 3]);
        assert_eq!(
            plotui_set_graph_positions(p, h, mx.as_ptr(), 3, my.as_ptr(), 3, mz.as_ptr(), 3),
            PLOTUI_OK
        );
        assert_eq!(
            plotui_set_graph_routes(p, h, pts.as_ptr(), n_pts, starts.as_ptr(), 3),
            PLOTUI_OK
        );
        assert_eq!(plotui_set_graph_routes(p, h, ptr::null(), 0, ptr::null(), 0), PLOTUI_OK);

        // The chrome tri-state is reachable from C.
        plotui_set_show_axes(p, 1);
        plotui_set_show_axes(p, 0);
        plotui_set_show_axes(p, -1);

        // An unknown shape name is the shared message, not a silent default.
        let bad = CString::new("blob").unwrap();
        let bad_ptrs = [bad.as_ptr()];
        assert_eq!(
            plotui_add_graph2d(
                p,
                xs.as_ptr(),
                1,
                ys.as_ptr(),
                1,
                ptr::null(),
                0,
                ptr::null(),
                0,
                true,
                ptr::null(),
                0,
                ptr::null(),
                bad_ptrs.as_ptr(),
                1,
                ptr::null(),
                0,
                ptr::null(),
                0,
                ptr::null(),
                0,
                ptr::null(),
                ptr::null_mut(),
            ),
            PLOTUI_ERR_INVALID_ARG
        );
        assert_eq!(
            last_error(),
            "unknown node shape \"blob\"; expected one of rounded, box, ellipse, diamond"
        );

        plotui_free(p);
        plotui_layered_layout_free(l);
        plotui_layered_layout_free(ptr::null_mut()); // NULL is inert
    }
}

#[test]
fn plot_from_dot_and_reachable_cross_the_abi() {
    unsafe {
        let text = CString::new("digraph nightly { a -> b -> c; a -> c }").unwrap();
        let mut plot = ptr::null_mut();
        let mut h = usize::MAX;
        assert_eq!(plotui_plot_from_dot(text.as_ptr(), ptr::null(), &mut plot, &mut h), PLOTUI_OK);
        assert!(!plot.is_null());
        assert_eq!(plotui_node_count(plot), 3);
        assert_eq!(h, 0);
        plotui_free(plot);

        // A parse error keeps its line:col and does not hand back a plot.
        let bad = CString::new("digraph { a -- b }").unwrap();
        let mut none = ptr::null_mut();
        assert_eq!(
            plotui_plot_from_dot(bad.as_ptr(), ptr::null(), &mut none, ptr::null_mut()),
            PLOTUI_ERR_INVALID_ARG
        );
        assert_eq!(last_error(), "1:13: '--' joins nodes in a graph; a digraph uses '->'");

        // And an unknown rankdir is caught before any parsing happens.
        let sideways = CString::new("sideways").unwrap();
        assert_eq!(
            plotui_plot_from_dot(text.as_ptr(), sideways.as_ptr(), &mut none, ptr::null_mut()),
            PLOTUI_ERR_INVALID_ARG
        );
        assert_eq!(last_error(), "unknown rankdir \"sideways\"; expected one of TB, LR");

        // Reachability: upstream of the sink is everything, downstream of it
        // is only itself.
        let edges = [0u32, 1, 1, 2, 0, 2];
        let mut flags = [9u8; 3];
        assert_eq!(plotui_reachable(3, edges.as_ptr(), 3, 2, true, flags.as_mut_ptr()), PLOTUI_OK);
        assert_eq!(flags, [1, 1, 1]);
        assert_eq!(plotui_reachable(3, edges.as_ptr(), 3, 2, false, flags.as_mut_ptr()), PLOTUI_OK);
        assert_eq!(flags, [0, 0, 1]);
    }
}
