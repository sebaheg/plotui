//! PyO3 bindings — expose the plotui engine to Python as `plotui._plotui`.
//!
//! The design: Python (Textual) owns the event loop and input; this layer is a
//! thin stateful handle. Input events call the camera methods; a refresh calls
//! `render_*`, which releases the GIL during rasterization so it never blocks
//! the host's async loop.

use plotui_core::Element;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

/// Explicit color, or the next palette slot in fixed order.
fn resolve_color(plot: &plotui_core::Plot, color: Option<(u8, u8, u8)>) -> [u8; 3] {
    match color {
        Some((r, g, b)) => [r, g, b],
        None => plot.next_color(),
    }
}

/// A plot element from Python: either a bare node index (the original API) or
/// a `("node" | "edge", index)` tuple.
#[derive(FromPyObject)]
enum ElementArg {
    Index(usize),
    Typed(String, usize),
}

fn to_element(arg: Option<ElementArg>) -> PyResult<Option<Element>> {
    match arg {
        None => Ok(None),
        Some(ElementArg::Index(i)) => Ok(Some(Element::Node(i))),
        Some(ElementArg::Typed(kind, i)) => match kind.as_str() {
            "node" => Ok(Some(Element::Node(i))),
            "edge" => Ok(Some(Element::Edge(i))),
            _ => Err(PyValueError::new_err(format!(
                "element kind must be 'node' or 'edge', got {kind:?}"
            ))),
        },
    }
}

fn from_element(el: Option<Element>) -> Option<(&'static str, usize)> {
    match el {
        Some(Element::Node(i)) => Some(("node", i)),
        Some(Element::Edge(i)) => Some(("edge", i)),
        None => None,
    }
}

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
        self.inner.add_scatter3d(pts, [color.0, color.1, color.2], size);
        Ok(())
    }

    /// Add a 3D graph: nodes at `xs/ys/zs`, `edges` as (i, j) index pairs,
    /// optional per-node `node_colors`, else a uniform `color`. `node_sizes`
    /// overrides `size` per node; `edge_colors` (one per edge) overrides the
    /// default dimmed endpoint-average edge color.
    #[pyo3(signature = (
        xs, ys, zs, edges, node_colors=None, color=(120, 180, 230), size=3.5,
        node_sizes=None, edge_colors=None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn add_graph3d(
        &mut self,
        xs: Vec<f32>,
        ys: Vec<f32>,
        zs: Vec<f32>,
        edges: Vec<(u32, u32)>,
        node_colors: Option<Vec<(u8, u8, u8)>>,
        color: (u8, u8, u8),
        size: f32,
        node_sizes: Option<Vec<f32>>,
        edge_colors: Option<Vec<(u8, u8, u8)>>,
    ) -> PyResult<()> {
        let n = xs.len().min(ys.len()).min(zs.len());
        let nodes: Vec<[f32; 3]> = (0..n).map(|i| [xs[i], ys[i], zs[i]]).collect();
        let colors: Vec<[u8; 3]> = match node_colors {
            Some(c) => (0..n)
                .map(|i| {
                    let t = c.get(i).copied().unwrap_or(color);
                    [t.0, t.1, t.2]
                })
                .collect(),
            None => vec![[color.0, color.1, color.2]; n],
        };
        let ec = edge_colors.map(|v| v.into_iter().map(|(r, g, b)| [r, g, b]).collect());
        self.inner.add_graph3d(nodes, colors, edges, size, node_sizes, ec);
        Ok(())
    }

    /// Add a 2D scatter series. With `color=None`, palette slots are assigned
    /// in fixed order. `name` puts the series in the legend.
    #[pyo3(signature = (xs, ys, color=None, size=2.5, name=None))]
    fn add_scatter(
        &mut self,
        xs: Vec<f32>,
        ys: Vec<f32>,
        color: Option<(u8, u8, u8)>,
        size: f32,
        name: Option<String>,
    ) {
        let c = resolve_color(&self.inner, color);
        self.inner.add_scatter2d(xs, ys, c, size, name);
    }

    /// Add a 2D line series (2px stroke by default).
    #[pyo3(signature = (xs, ys, color=None, width=2.0, name=None))]
    fn add_line(
        &mut self,
        xs: Vec<f32>,
        ys: Vec<f32>,
        color: Option<(u8, u8, u8)>,
        width: f32,
        name: Option<String>,
    ) {
        let c = resolve_color(&self.inner, color);
        self.inner.add_line2d(xs, ys, c, width, name);
    }

    /// Add a 2D bar series: bars at `xs` rising (or falling) from zero to
    /// `heights`. Bar width comes from the smallest gap between x positions.
    #[pyo3(signature = (xs, heights, color=None, name=None))]
    fn add_bar(
        &mut self,
        xs: Vec<f32>,
        heights: Vec<f32>,
        color: Option<(u8, u8, u8)>,
        name: Option<String>,
    ) {
        let c = resolve_color(&self.inner, color);
        self.inner.add_bar2d(xs, heights, c, name);
    }

    /// Select an element: a bare node index, a `("node"|"edge", index)` tuple,
    /// or `None` to clear. The selected element gets the ring/glow treatment.
    #[pyo3(signature = (element=None))]
    fn set_selected(&mut self, element: Option<ElementArg>) -> PyResult<()> {
        self.inner.selected = to_element(element)?;
        Ok(())
    }

    /// Hover an element (same forms as `set_selected`); it lights up white to
    /// signal it can be clicked. Returns True when the hover state changed,
    /// so the frontend knows whether a repaint is needed.
    #[pyo3(signature = (element=None))]
    fn set_hovered(&mut self, element: Option<ElementArg>) -> PyResult<bool> {
        let el = to_element(element)?;
        let changed = self.inner.hovered != el;
        self.inner.hovered = el;
        Ok(changed)
    }

    /// Pick whatever is under pixel `(px, py)` in a `px_w`×`px_h` framebuffer:
    /// the nearest node within `node_radius`, else the nearest graph edge
    /// within `edge_radius`. Returns `("node"|"edge", index)` or None.
    #[pyo3(signature = (px_w, px_h, px, py, node_radius, edge_radius=None))]
    #[allow(clippy::too_many_arguments)]
    fn pick_element_px(
        &self,
        px_w: usize,
        px_h: usize,
        px: f32,
        py: f32,
        node_radius: f32,
        edge_radius: Option<f32>,
    ) -> Option<(&'static str, usize)> {
        let er = edge_radius.unwrap_or(node_radius * 0.75);
        from_element(self.inner.pick_element(px_w, px_h, px, py, node_radius, er))
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

    /// The camera state `(yaw, pitch, zoom, pan_x, pan_y)` — capture it before
    /// rebuilding a plot so the restored view is seamless.
    fn camera_state(&self) -> (f64, f64, f64, f64, f64) {
        self.inner.camera.state()
    }

    /// Restore a camera state captured by `camera_state` (values are clamped
    /// the same way the incremental mutators clamp).
    fn set_camera_state(&mut self, yaw: f64, pitch: f64, zoom: f64, pan_x: f64, pan_y: f64) {
        self.inner.camera.set_state(yaw, pitch, zoom, pan_x, pan_y);
    }

    /// Show or hide the 3D bounding-box wireframe (on by default).
    fn set_show_box(&mut self, show: bool) {
        self.inner.show_box = show;
    }

    /// Project every node (flat-index order, matching `pick_px`) to screen
    /// space for a `px_w`×`px_h` framebuffer. Returns `(x_px, y_px, depth)`
    /// per node — the hook for label overlays and camera targeting.
    fn project_nodes(&self, px_w: usize, px_h: usize) -> Vec<(f32, f32, f32)> {
        self.inner.project_nodes(px_w, px_h).into_iter().map(|p| (p[0], p[1], p[2])).collect()
    }

    /// Render as a Kitty graphics escape sequence for a `cols`×`rows` region of
    /// `cell_w`×`cell_h`-pixel cells. Emit it with the cursor at the region's
    /// top-left. GIL is released during rasterization.
    /// With `compat_chunks=True`, the image id is repeated on every data
    /// chunk — off-spec, but required by iTerm2, which drops spec-framed
    /// chunked transmissions.
    #[pyo3(signature = (cols, rows, cell_w, cell_h, compat_chunks=false))]
    fn render_kitty(
        &self,
        py: Python<'_>,
        cols: u16,
        rows: u16,
        cell_w: u16,
        cell_h: u16,
        compat_chunks: bool,
    ) -> String {
        py.allow_threads(|| {
            let pw = cols as usize * cell_w.max(1) as usize;
            let ph = rows as usize * cell_h.max(1) as usize;
            let fb = self.inner.render(pw, ph);
            if compat_chunks {
                plotui_protocol::kitty_compat(&fb, cols, rows)
            } else {
                plotui_protocol::kitty(&fb, cols, rows)
            }
        })
    }

    /// Render one frame and return the raw RGBA8 pixels (`px_w * px_h * 4`
    /// bytes, row-major; undrawn pixels have alpha 0). The escape-free way to
    /// inspect exactly what would be drawn — for tests, snapshots, or export.
    fn render_rgba<'py>(
        &self,
        py: Python<'py>,
        px_w: usize,
        px_h: usize,
    ) -> pyo3::Bound<'py, pyo3::types::PyBytes> {
        let rgba = py.allow_threads(|| self.inner.render(px_w, px_h).rgba());
        pyo3::types::PyBytes::new(py, &rgba)
    }

    /// Render a full-resolution Kitty image placed via Unicode placeholders for
    /// a `cols`×`rows` region of `cell_w`×`cell_h`-pixel cells. Returns
    /// `(transmit_escape, (id_r, id_g, id_b), placeholder_rows)`. GIL released.
    fn render_kitty_placeholder(
        &self,
        py: Python<'_>,
        cols: u16,
        rows: u16,
        cell_w: u16,
        cell_h: u16,
    ) -> (String, (u8, u8, u8), Vec<String>) {
        py.allow_threads(|| {
            let pw = cols as usize * cell_w.max(1) as usize;
            let ph = rows as usize * cell_h.max(1) as usize;
            let fb = self.inner.render(pw, ph);
            let p = plotui_protocol::kitty_placeholder(&fb, cols, rows);
            (p.transmit, p.id_rgb, p.rows)
        })
    }

    /// Like `render_kitty_placeholder`, but every placeholder cell is returned
    /// separately and carries its own position diacritics, so the frontend can
    /// splice text (label overlays) into a row without breaking the cells after
    /// the gap. Returns `(transmit_escape, (id_r, id_g, id_b), cells)` where
    /// `cells[y][x]` is the placeholder string for that cell. GIL released.
    fn render_kitty_placeholder_cells(
        &self,
        py: Python<'_>,
        cols: u16,
        rows: u16,
        cell_w: u16,
        cell_h: u16,
    ) -> (String, (u8, u8, u8), Vec<Vec<String>>) {
        py.allow_threads(|| {
            let pw = cols as usize * cell_w.max(1) as usize;
            let ph = rows as usize * cell_h.max(1) as usize;
            let fb = self.inner.render(pw, ph);
            let p = plotui_protocol::kitty_placeholder_cells(&fb, cols, rows);
            (p.transmit, p.id_rgb, p.cells)
        })
    }

    /// Pick the nearest node to pixel `(px, py)` in a `px_w`×`px_h` framebuffer,
    /// within `radius` pixels. Lets the frontend map clicks in whatever cell
    /// geometry its active render mode uses.
    fn pick_px(&self, px_w: usize, px_h: usize, px: f32, py: f32, radius: f32) -> Option<usize> {
        self.inner.pick(px_w, px_h, px, py, radius)
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
