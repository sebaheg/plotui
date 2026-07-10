//! PyO3 bindings — expose the plotui engine to Python as `plotui._plotui`.
//!
//! The design: Python (Textual) owns the event loop and input; this layer is a
//! thin stateful handle. Input events call the camera methods; a refresh calls
//! `render_*`, which releases the GIL during rasterization so it never blocks
//! the host's async loop.

use pyo3::prelude::*;

/// A plot handle: data + camera. Held by the Python frontend for a plot's life.
#[pyclass]
struct Plot {
    inner: plotui_core::Plot,
}

#[pymethods]
impl Plot {
    #[new]
    fn new() -> Self {
        Plot { inner: plotui_core::Plot::new() }
    }

    /// Add a 3D scatter series. `xs/ys/zs` accept any float sequence
    /// (lists or numpy arrays). `color` is an (r, g, b) tuple.
    #[pyo3(signature = (xs, ys, zs, color=(230, 60, 120), size=3.0))]
    fn add_scatter3d(
        &mut self,
        xs: Vec<f32>,
        ys: Vec<f32>,
        zs: Vec<f32>,
        color: (u8, u8, u8),
        size: f32,
    ) -> PyResult<()> {
        let n = xs.len().min(ys.len()).min(zs.len());
        let pts: Vec<[f32; 3]> = (0..n).map(|i| [xs[i], ys[i], zs[i]]).collect();
        self.inner
            .add_scatter3d(pts, [color.0, color.1, color.2], size);
        Ok(())
    }

    // --- interaction: the frontend forwards input to these ---
    fn rotate(&mut self, d_yaw: f64, d_pitch: f64) {
        self.inner.camera.rotate(d_yaw, d_pitch);
    }
    fn zoom_by(&mut self, factor: f64) {
        self.inner.camera.zoom_by(factor);
    }
    fn pan(&mut self, dx: f64, dy: f64) {
        self.inner.camera.pan(dx, dy);
    }
    fn reset(&mut self) {
        self.inner.camera.reset();
    }

    /// Render as a Kitty graphics escape sequence for a `cols`×`rows` region of
    /// `cell_w`×`cell_h`-pixel cells. Emit it with the cursor at the region's
    /// top-left. GIL is released during rasterization.
    fn render_kitty(&self, py: Python<'_>, cols: u16, rows: u16, cell_w: u16, cell_h: u16) -> String {
        py.allow_threads(|| {
            let pw = cols as usize * cell_w.max(1) as usize;
            let ph = rows as usize * cell_h.max(1) as usize;
            let fb = self.inner.render(pw, ph);
            plotui_protocol::kitty(&fb, cols, rows)
        })
    }

    /// Render as `rows` lines of half-block text (universal fallback).
    fn render_halfblock(&self, py: Python<'_>, cols: u16, rows: u16) -> String {
        py.allow_threads(|| {
            let fb = self.inner.render(cols as usize, rows as usize * 2);
            plotui_protocol::halfblock(&fb)
        })
    }

    /// Escape sequence that removes plotui's image from the terminal.
    #[staticmethod]
    fn kitty_cleanup() -> String {
        plotui_protocol::kitty_cleanup()
    }
}

#[pymodule]
fn _plotui(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Plot>()?;
    m.add("__doc__", "Native rendering core for plotui.")?;
    Ok(())
}
