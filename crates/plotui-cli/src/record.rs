//! Headless export: drive the same feed/auto-rotate hooks the interactive
//! loop uses, but at a fixed virtual frame rate, piping raw RGBA frames to
//! ffmpeg. No terminal is touched, so this works in CI and over ssh.
//!
//! Terminal-only chrome (overlay text spans like the deps "+ crate" badge)
//! is drawn by ratatui over the image, not into the framebuffer, so it does
//! not appear in exports.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use plotui_core::Plot;
use plotui_ratatui::{PlotOptions, PlotState};
use plotui_term::{RenderMode, FALLBACK_CELL_PX};

use crate::interactive::{self, Hooks};

pub struct RecordOpts {
    pub path: PathBuf,
    pub width: usize,
    pub height: usize,
    pub fps: u32,
    pub frames: u32,
}

impl RecordOpts {
    /// A `.png` destination: one frame of the played-out scene, not a video.
    pub fn is_still(&self) -> bool {
        container(&self.path).is_ok_and(|c| c == Container::Png)
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Container {
    Mp4,
    Webm,
    Gif,
    Png,
}

fn container(path: &Path) -> io::Result<Container> {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("mp4") => Ok(Container::Mp4),
        Some("webm") => Ok(Container::Webm),
        Some("gif") => Ok(Container::Gif),
        Some("png") => Ok(Container::Png),
        _ => Err(io::Error::other(format!(
            "can't tell the output format from '{}' (use .mp4, .webm, .gif, or .png)",
            path.display()
        ))),
    }
}

/// yuv420p subsamples chroma 2x2, so encoders reject odd dimensions.
fn even(n: usize) -> usize {
    n & !1
}

fn spawn_ffmpeg(path: &Path, c: Container, w: usize, h: usize, fps: u32) -> io::Result<Child> {
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-y", "-v", "error", "-f", "rawvideo", "-pix_fmt", "rgba"]).args([
        "-s",
        &format!("{w}x{h}"),
        "-r",
        &fps.to_string(),
        "-i",
        "-",
    ]);
    match c {
        Container::Mp4 => {
            cmd.args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-movflags", "+faststart"]);
        }
        Container::Webm => {
            cmd.args(["-c:v", "libvpx-vp9", "-pix_fmt", "yuv420p", "-crf", "32", "-b:v", "0"]);
        }
        Container::Gif => {
            // One invocation: palette pass and encode in a single filter graph.
            cmd.args(["-filter_complex", "[0:v]split[a][b];[a]palettegen[p];[b][p]paletteuse"]);
        }
        Container::Png => {
            cmd.args(["-frames:v", "1"]);
        }
    }
    cmd.arg(path).stdin(Stdio::piped()).stdout(Stdio::null());
    cmd.spawn().map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            io::Error::other(
                "exporting needs ffmpeg on your PATH (brew install ffmpeg / apt install ffmpeg)",
            )
        } else {
            e
        }
    })
}

/// One rasterized frame as opaque RGBA: pixels the renderer left blank take
/// the plot's chrome background (the same tone the 3D fog fades toward), so
/// exports match the terminal look and survive encoders that ignore alpha.
fn frame_rgba(plot: &Plot, w: usize, h: usize) -> Vec<u8> {
    let bg = plot.chrome.bg;
    let mut px = plot.render(w, h).rgba();
    for p in px.chunks_exact_mut(4) {
        if p[3] == 0 {
            p[..3].copy_from_slice(&bg);
        }
        p[3] = 255;
    }
    px
}

fn finish(mut child: Child, path: &Path) -> io::Result<()> {
    drop(child.stdin.take());
    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("ffmpeg failed writing {}", path.display())))
    }
}

/// Record an animated scene: step the shared tick driver `frames` times at a
/// fixed virtual dt of `1000 / fps` ms and encode every frame.
pub fn record(plot: Plot, mut hooks: Hooks, opts: &RecordOpts) -> io::Result<()> {
    let c = container(&opts.path)?;
    if c == Container::Png {
        return Err(io::Error::other(
            "a .png holds one frame; pick .mp4/.webm/.gif for animation (or use --static)",
        ));
    }
    let (w, h) = (even(opts.width), even(opts.height));
    if (w, h) != (opts.width, opts.height) {
        eprintln!("plotui: rounding size to {w}x{h} (video encoders need even dimensions)");
    }

    // Headless widget state: the forced mode and cell size skip every
    // terminal probe, and nothing here ever draws the widget.
    let spin = hooks.auto_rotate;
    let mut state = PlotState::new(
        plot,
        PlotOptions {
            render_mode: Some(RenderMode::Unsupported),
            cell_px: Some(FALLBACK_CELL_PX),
            auto_rotate: spin,
            ..Default::default()
        },
    );

    let mut child = spawn_ffmpeg(&opts.path, c, w, h, opts.fps)?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    let dt = 1000.0 / opts.fps as f64;
    for _ in 0..opts.frames {
        interactive::advance(&mut state, &mut hooks, dt, spin);
        stdin.write_all(&frame_rgba(state.plot(), w, h))?;
    }
    drop(stdin);
    finish(child, &opts.path)?;
    eprintln!("wrote {} ({} frames, {w}x{h} @ {} fps)", opts.path.display(), opts.frames, opts.fps);
    Ok(())
}

/// Export one frame of a plot to an image file (`.png`).
pub fn record_static(plot: &Plot, path: &Path, width: usize, height: usize) -> io::Result<()> {
    let c = container(path)?;
    if c != Container::Png {
        return Err(io::Error::other(format!(
            "'{}' is a video format; this plot is a single frame — use .png \
             (animated examples can record video)",
            path.display()
        )));
    }
    let (w, h) = (even(width), even(height));
    let mut child = spawn_ffmpeg(path, c, w, h, 1)?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    stdin.write_all(&frame_rgba(plot, w, h))?;
    drop(stdin);
    finish(child, path)?;
    eprintln!("wrote {} ({w}x{h})", path.display());
    Ok(())
}
