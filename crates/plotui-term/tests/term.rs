//! Policy and detection tests — pure functions over injected environments.

use plotui_term::policy::{active_scale, pixel_geometry, scaled_dims};
use plotui_term::{
    compose_frame, detect_render_mode_from, next_image_id, tmux_wrap_with, FrameOutput,
    FrameRequest, RenderMode,
};
use std::collections::HashMap;

fn mode_for(vars: &[(&str, &str)]) -> RenderMode {
    let env: HashMap<String, String> =
        vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    detect_render_mode_from(|k| env.get(k).cloned())
}

#[test]
fn detection_tiers_match_the_terminal_matrix() {
    // Placeholder tier.
    assert_eq!(mode_for(&[("KITTY_WINDOW_ID", "1")]), RenderMode::Placeholder);
    assert_eq!(mode_for(&[("TERM", "xterm-kitty")]), RenderMode::Placeholder);
    assert_eq!(mode_for(&[("TERM", "xterm-ghostty")]), RenderMode::Placeholder);
    assert_eq!(mode_for(&[("TERM_PROGRAM", "ghostty")]), RenderMode::Placeholder);
    assert_eq!(mode_for(&[("GHOSTTY_RESOURCES_DIR", "/x")]), RenderMode::Placeholder);
    // Direct tier: iTerm2 from 3.5 up, WezTerm, Konsole.
    assert_eq!(
        mode_for(&[("TERM_PROGRAM", "iTerm.app"), ("TERM_PROGRAM_VERSION", "3.5.1")]),
        RenderMode::Direct
    );
    assert_eq!(
        mode_for(&[("LC_TERMINAL", "iTerm2"), ("LC_TERMINAL_VERSION", "3.6")]),
        RenderMode::Direct
    );
    assert_eq!(mode_for(&[("TERM_PROGRAM", "WezTerm")]), RenderMode::Direct);
    assert_eq!(mode_for(&[("WEZTERM_EXECUTABLE", "/x")]), RenderMode::Direct);
    assert_eq!(mode_for(&[("KONSOLE_VERSION", "230400")]), RenderMode::Direct);
    // Direct tier, partial decoders: Warp, Rio, VS Code.
    assert_eq!(mode_for(&[("TERM_PROGRAM", "WarpTerminal")]), RenderMode::Direct);
    assert_eq!(mode_for(&[("TERM_PROGRAM", "rio")]), RenderMode::Direct);
    assert_eq!(mode_for(&[("TERM", "rio")]), RenderMode::Direct);
    assert_eq!(mode_for(&[("TERM_PROGRAM", "vscode")]), RenderMode::Direct);
    // Old iTerm2 has no Kitty graphics at all.
    assert_eq!(
        mode_for(&[("TERM_PROGRAM", "iTerm.app"), ("TERM_PROGRAM_VERSION", "3.4.19")]),
        RenderMode::Unsupported
    );
    assert_eq!(mode_for(&[("TERM_PROGRAM", "iTerm.app")]), RenderMode::Unsupported);
    // Nothing recognizable.
    assert_eq!(mode_for(&[("TERM", "xterm-256color")]), RenderMode::Unsupported);
    assert_eq!(mode_for(&[]), RenderMode::Unsupported);
}

#[test]
fn plotui_render_overrides_detection() {
    assert_eq!(
        mode_for(&[("PLOTUI_RENDER", "direct"), ("TERM", "xterm-kitty")]),
        RenderMode::Direct
    );
    assert_eq!(mode_for(&[("PLOTUI_RENDER", "placeholder")]), RenderMode::Placeholder);
    // "kitty" is a retired alias for placeholder.
    assert_eq!(mode_for(&[("PLOTUI_RENDER", "kitty")]), RenderMode::Placeholder);
    assert_eq!(mode_for(&[("PLOTUI_RENDER", " DIRECT ")]), RenderMode::Direct, "trim + fold case");
    // Unknown values fall through to detection.
    assert_eq!(
        mode_for(&[("PLOTUI_RENDER", "sixel"), ("TERM", "xterm-kitty")]),
        RenderMode::Placeholder
    );
}

#[test]
fn tmux_wrap_doubles_escapes_inside_passthrough() {
    let escape = "\x1b_Gpayload\x1b\\";
    assert_eq!(tmux_wrap_with(escape, false), escape, "no-op outside tmux");
    assert_eq!(tmux_wrap_with(escape, true), "\x1bPtmux;\x1b\x1b_Gpayload\x1b\x1b\\\x1b\\");
}

#[test]
fn scaled_dims_clamps_and_stays_positive() {
    assert_eq!(scaled_dims(20, 10, 10, 20, 1.0), (200, 200, 1.0));
    assert_eq!(scaled_dims(20, 10, 10, 20, 0.5), (100, 100, 0.5));
    // Below the floor and above 1.0 both clamp.
    assert_eq!(scaled_dims(20, 10, 10, 20, 0.0).2, 0.05);
    assert_eq!(scaled_dims(20, 10, 10, 20, 7.0), (200, 200, 1.0));
    // Degenerate cell sizes floor at one pixel.
    assert_eq!(scaled_dims(1, 1, 0, 0, 0.05), (1, 1, 0.05));
}

#[test]
fn active_scale_reduces_only_large_3d_plots_mid_interaction() {
    assert_eq!(active_scale(0.5, true, 1000, true), 0.5);
    assert_eq!(active_scale(0.5, true, 1000, false), 1.0, "still plots render full-res");
    assert_eq!(active_scale(0.5, true, 100, true), 1.0, "small plots never reduce");
    assert_eq!(active_scale(0.5, false, 1000, true), 1.0, "2D never reduces");
    assert_eq!(active_scale(1.0, true, 1000, true), 1.0, "scale 1.0 disables the feature");
}

#[test]
fn pixel_geometry_hits_cell_centers() {
    let (pw, ph, px, py, radius) = pixel_geometry(20, 10, 10, 20, 3, 2);
    assert_eq!((pw, ph), (200, 200));
    assert_eq!((px, py), (35.0, 50.0));
    assert_eq!(radius, 20.0, "pick radius is one cell height");
}

#[test]
fn compose_frame_produces_mode_shaped_output() {
    let mut plot = plotui_core::Plot::new();
    let color = plot.next_color();
    plot.add_line2d(
        vec![0.0, 1.0, 2.0],
        vec![0.0, 2.0, 1.0],
        color,
        2.0,
        None,
        plotui_core::YAxis::Primary,
    );
    let req = |mode, tmux, replace| FrameRequest {
        cols: 20,
        rows: 10,
        cell_w: 8,
        cell_h: 16,
        scale: 1.0,
        mode,
        image_id: 4321,
        replace,
        tmux,
    };

    match compose_frame(&plot, &req(RenderMode::Placeholder, false, false)) {
        FrameOutput::Placeholder { transmit, id_rgb, cells } => {
            assert!(transmit.contains("U=1") && transmit.contains("i=4321,"));
            assert_eq!(id_rgb, (0x00, 0x10, 0xE1), "4321 = 0x10E1 in the fg color");
            assert_eq!(cells.len(), 10);
            assert!(cells.iter().all(|r| r.len() == 20));
        }
        _ => panic!("placeholder request must produce placeholder output"),
    }

    match compose_frame(&plot, &req(RenderMode::Direct, false, false)) {
        FrameOutput::Direct { escape } => {
            assert!(escape.contains("i=4321") && escape.contains("a=T"));
            assert!(escape.contains("a=d,d=i,i=4321"), "delete-first by default");
            assert!(!escape.starts_with("\x1bPtmux;"));
        }
        _ => panic!("direct request must produce direct output"),
    }
    match compose_frame(&plot, &req(RenderMode::Direct, true, true)) {
        FrameOutput::Direct { escape } => {
            assert!(escape.starts_with("\x1bPtmux;"), "tmux-wrapped on request");
            assert!(!escape.contains("a=d"), "replace skips the delete");
        }
        _ => panic!(),
    }

    assert!(matches!(
        compose_frame(&plot, &req(RenderMode::Unsupported, false, false)),
        FrameOutput::Unsupported
    ));
}

#[test]
fn image_ids_allocate_monotonically_from_the_default() {
    let a = next_image_id();
    let b = next_image_id();
    assert!(a >= plotui_protocol::DEFAULT_IMAGE_ID);
    assert_eq!(b, a + 1);
}
