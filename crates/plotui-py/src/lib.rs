//! PyO3 bindings — expose the plotui engine to Python as `plotui._plotui`.
//!
//! The design: Python (Textual) owns the event loop and input; this layer is a
//! thin stateful handle. Input events call the camera methods; a refresh calls
//! `render_*`, which releases the GIL during rasterization so it never blocks
//! the host's async loop.

use numpy::{PyArray1, PyArrayMethods};
use plotui_core::{Element, YAxis};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

/// The shared binding semantics (plotui-bind) report errors as `BindError`;
/// Python surfaces every one as a ValueError with the message verbatim.
fn to_py(e: plotui_bind::BindError) -> PyErr {
    PyValueError::new_err(e.msg)
}

/// Explicit color, or the next palette slot (see `Plot::resolve_color`).
fn resolve_color(plot: &plotui_core::Plot, color: Option<(u8, u8, u8)>) -> [u8; 3] {
    plot.resolve_color(color.map(|(r, g, b)| [r, g, b]))
}

/// True when numpy is already imported in this interpreter. Gate for the
/// fast-path downcasts below: rust-numpy's type objects come from numpy's
/// C API, whose lazy load aborts when numpy is absent — but an object can
/// only *be* a numpy array if numpy is in `sys.modules`, so this check makes
/// the fast path exactly as available as it can safely be.
fn numpy_loaded(py: Python<'_>) -> bool {
    (|| -> PyResult<bool> {
        let modules = py.import("sys")?.getattr("modules")?;
        modules.downcast::<PyDict>().map_err(PyErr::from)?.contains("numpy")
    })()
    .unwrap_or(false)
}

/// A 1-D coordinate sequence from Python, landed as the `Vec<f32>` the core
/// stores. numpy float32/float64 arrays are read in one bulk copy with no
/// per-element Python calls; anything else takes generic sequence extraction.
struct Coords(Vec<f32>);

impl<'py> FromPyObject<'py> for Coords {
    fn extract_bound(ob: &Bound<'py, PyAny>) -> PyResult<Self> {
        if numpy_loaded(ob.py()) {
            if let Ok(a) = ob.downcast::<PyArray1<f32>>() {
                let r = a.readonly();
                return Ok(Coords(match r.as_slice() {
                    Ok(s) => s.to_vec(),
                    Err(_) => r.as_array().iter().copied().collect(),
                }));
            }
            if let Ok(a) = ob.downcast::<PyArray1<f64>>() {
                let r = a.readonly();
                return Ok(Coords(match r.as_slice() {
                    Ok(s) => s.iter().map(|&v| v as f32).collect(),
                    Err(_) => r.as_array().iter().map(|&v| v as f32).collect(),
                }));
            }
        }
        Ok(Coords(ob.extract()?))
    }
}

/// The y scale a 2D series binds to: the primary left axis, or one of the two
/// independent right-hand axes.
fn parse_axis(axis: &str) -> PyResult<YAxis> {
    plotui_bind::parse_axis(axis).map_err(to_py)
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
        Some(ElementArg::Typed(kind, i)) => {
            plotui_bind::element_from_parts(&kind, i).map(Some).map_err(to_py)
        }
    }
}

fn from_element(el: Option<Element>) -> Option<(&'static str, usize)> {
    el.map(plotui_bind::element_to_parts)
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

    /// Add a 3D scatter series. `xs/ys/zs` accept any float sequence; numpy
    /// float32/float64 arrays are read in one bulk copy. `color` is an
    /// (r, g, b) tuple. Returns the trace handle for `extend`/`set_visible`.
    #[pyo3(signature = (xs, ys, zs, color=(230, 60, 120), size=3.0))]
    fn add_scatter3d(
        &mut self,
        xs: Coords,
        ys: Coords,
        zs: Coords,
        color: (u8, u8, u8),
        size: f32,
    ) -> PyResult<usize> {
        let pts = zip3(xs, ys, zs);
        Ok(self.inner.add_scatter3d(pts, [color.0, color.1, color.2], size))
    }

    /// Add a 3D graph: nodes at `xs/ys/zs`, `edges` as (i, j) index pairs,
    /// optional per-node `node_colors`, else a uniform `color`. `node_sizes`
    /// overrides `size` per node; `edge_colors` (one per edge) overrides the
    /// default dimmed endpoint-average edge color. `node_shapes` picks a
    /// marker silhouette per node — "disc", "ring", "square", "triangle",
    /// "diamond", "diamond-open", "dot" — so node categories read by
    /// shape as well as colour; an unknown name is a ValueError.
    #[pyo3(signature = (
        xs, ys, zs, edges, node_colors=None, color=(120, 180, 230), size=3.5,
        node_sizes=None, edge_colors=None, node_shapes=None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn add_graph3d(
        &mut self,
        xs: Coords,
        ys: Coords,
        zs: Coords,
        edges: Vec<(u32, u32)>,
        node_colors: Option<Vec<(u8, u8, u8)>>,
        color: (u8, u8, u8),
        size: f32,
        node_sizes: Option<Vec<f32>>,
        edge_colors: Option<Vec<(u8, u8, u8)>>,
        node_shapes: Option<Vec<String>>,
    ) -> PyResult<usize> {
        let nodes = zip3(xs, ys, zs);
        let n = nodes.len();
        let nc = node_colors.map(|v| v.into_iter().map(|(r, g, b)| [r, g, b]).collect());
        let colors = plotui_bind::graph_node_colors(n, nc, [color.0, color.1, color.2]);
        let ec = edge_colors.map(|v| v.into_iter().map(|(r, g, b)| [r, g, b]).collect());
        let shapes = match node_shapes {
            Some(names) => Some(plotui_bind::parse_shapes(&names).map_err(to_py)?),
            None => None,
        };
        Ok(self.inner.add_graph3d(nodes, colors, edges, size, node_sizes, ec, shapes))
    }

    /// Add a 3D polyline through (xs, ys, zs) in order — a trajectory or
    /// curve in the same space as `add_scatter3d`. With `color=None`, palette
    /// slots are assigned in fixed order. A NaN vertex breaks the line into
    /// separate runs. `name` puts the series in the legend. Vertices are not
    /// pickable, so adding lines never shifts existing node indices.
    #[pyo3(signature = (xs, ys, zs, color=None, width=2.0, name=None))]
    fn add_line3d(
        &mut self,
        xs: Coords,
        ys: Coords,
        zs: Coords,
        color: Option<(u8, u8, u8)>,
        width: f32,
        name: Option<String>,
    ) -> PyResult<usize> {
        let pts = zip3(xs, ys, zs);
        let c = resolve_color(&self.inner, color);
        Ok(self.inner.add_line3d(pts, c, width, name))
    }

    /// Add a grid surface: `zs` is a list of rows (or a 2D numpy array),
    /// `zs[j][i]` giving the height at (xs[i], ys[j]) — the same (x, y, z)
    /// space as `add_scatter3d`. `colormap` ("viridis" or "plasma", default
    /// "viridis") colors by height over the surface's own range; pass
    /// `colormap=None` with a `color` for a solid surface. `wireframe=True`
    /// overlays the grid lines. Cells with a NaN corner are holes. `name`
    /// puts the surface in the legend. Grid vertices are not pickable.
    #[pyo3(signature = (xs, ys, zs, color=None, colormap="viridis", wireframe=false, name=None))]
    #[allow(clippy::too_many_arguments)]
    fn add_surface3d(
        &mut self,
        xs: Coords,
        ys: Coords,
        zs: Vec<Vec<f32>>,
        color: Option<(u8, u8, u8)>,
        colormap: Option<&str>,
        wireframe: bool,
        name: Option<String>,
    ) -> PyResult<usize> {
        let (xs, ys) = (xs.0, ys.0);
        let flat = plotui_bind::flatten_surface_grid(xs.len(), ys.len(), zs).map_err(to_py)?;
        let cm = plotui_bind::parse_colormap(colormap).map_err(to_py)?;
        let c = resolve_color(&self.inner, color);
        Ok(self.inner.add_surface3d(xs, ys, flat, c, cm, wireframe, name))
    }

    /// Add a 2D scatter series. With `color=None`, palette slots are assigned
    /// in fixed order. `name` puts the series in the legend. `axis="y2"` or
    /// `"y3"` binds the series to an independent right-hand axis (own
    /// autoscale and ticks, labels tinted to the series colour; y2 sits
    /// innermost, y3 outermost; the grid belongs to the left axis).
    #[pyo3(signature = (xs, ys, color=None, size=2.5, name=None, axis="y"))]
    fn add_scatter(
        &mut self,
        xs: Coords,
        ys: Coords,
        color: Option<(u8, u8, u8)>,
        size: f32,
        name: Option<String>,
        axis: &str,
    ) -> PyResult<usize> {
        let c = resolve_color(&self.inner, color);
        let (xs, ys) = (xs.0, ys.0);
        Ok(self.inner.add_scatter2d(xs, ys, c, size, name, parse_axis(axis)?))
    }

    /// Add a 2D line series (2px stroke by default). `axis="y2"`/`"y3"` puts
    /// it on an independent right-hand axis, as in `add_scatter`.
    #[pyo3(signature = (xs, ys, color=None, width=2.0, name=None, axis="y"))]
    fn add_line(
        &mut self,
        xs: Coords,
        ys: Coords,
        color: Option<(u8, u8, u8)>,
        width: f32,
        name: Option<String>,
        axis: &str,
    ) -> PyResult<usize> {
        let c = resolve_color(&self.inner, color);
        let (xs, ys) = (xs.0, ys.0);
        Ok(self.inner.add_line2d(xs, ys, c, width, name, parse_axis(axis)?))
    }

    /// Add a 2D bar series: bars at `xs` rising (or falling) from zero to
    /// `heights`. Bar width comes from the smallest gap between x positions.
    /// `axis="y2"`/`"y3"` puts it on an independent right-hand axis, whose
    /// own scale supplies the zero baseline.
    #[pyo3(signature = (xs, heights, color=None, name=None, axis="y"))]
    fn add_bar(
        &mut self,
        xs: Coords,
        heights: Coords,
        color: Option<(u8, u8, u8)>,
        name: Option<String>,
        axis: &str,
    ) -> PyResult<usize> {
        let c = resolve_color(&self.inner, color);
        let (xs, heights) = (xs.0, heights.0);
        Ok(self.inner.add_bar2d(xs, heights, c, name, parse_axis(axis)?))
    }

    /// Append points to an existing trace by handle: `extend(h, xs, ys)` for
    /// 2D traces (`ys` are the heights for a bar trace), `extend(h, xs, ys, zs)`
    /// for 3D scatter/line traces. Same input flexibility as the `add_*`
    /// calls, including numpy arrays and min-length pairing of ragged inputs;
    /// the result renders exactly as if the concatenated data had been added
    /// in one call. Graph and surface traces are structural and raise
    /// ValueError — rebuild the plot to change them. Two caveats: appending a
    /// bar whose x narrows the smallest gap re-flows the width of every bar
    /// in that trace, and appending to a 3D scatter that is not the last
    /// node-bearing trace shifts the flat node indices of every node after
    /// it (plotui remaps its own selection/hover; hosts holding node indices
    /// must do the same).
    #[pyo3(signature = (handle, xs, ys, zs=None))]
    fn extend(
        &mut self,
        handle: usize,
        xs: Coords,
        ys: Coords,
        zs: Option<Coords>,
    ) -> PyResult<()> {
        plotui_bind::extend(&mut self.inner, handle, &xs.0, &ys.0, zs.as_ref().map(|z| &z.0[..]))
            .map_err(to_py)
    }

    /// Show or hide a trace by handle. Returns True when the state changed,
    /// so the frontend knows whether a repaint is needed. A hidden trace
    /// keeps its handle, its palette slot, and its node/edge index block —
    /// only its geometry, bounds contribution, legend entry, and right-axis
    /// column disappear; showing it again restores everything.
    fn set_visible(&mut self, handle: usize, visible: bool) -> PyResult<bool> {
        self.inner
            .set_visible(handle, visible)
            .map_err(|_| PyValueError::new_err(format!("unknown trace handle {handle}")))
    }

    /// Select an element: a bare node index, a `("node"|"edge", index)` tuple,
    /// or `None` to clear. The selected element gets the ring/glow treatment.
    #[pyo3(signature = (element=None))]
    fn set_selected(&mut self, element: Option<ElementArg>) -> PyResult<()> {
        self.inner.selected = to_element(element)?;
        Ok(())
    }

    /// Set the 2D crosshair hover position, in framebuffer pixels (`None`
    /// clears it). The renderer snaps to the nearest sample x, draws a
    /// vertical guide with a marker per series sampled there, and a value
    /// readout box. Ignored by 3D plots. Returns True when the state
    /// changed, so the frontend knows whether a repaint is needed.
    #[pyo3(signature = (px=None))]
    fn set_hover2d(&mut self, px: Option<f32>) -> bool {
        plotui_bind::set_hover2d(&mut self.inner, px)
    }

    /// Hover an element (same forms as `set_selected`); it lights up white to
    /// signal it can be clicked. Returns True when the hover state changed,
    /// so the frontend knows whether a repaint is needed.
    #[pyo3(signature = (element=None))]
    fn set_hovered(&mut self, element: Option<ElementArg>) -> PyResult<bool> {
        let el = to_element(element)?;
        Ok(plotui_bind::set_hovered(&mut self.inner, el))
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
        from_element(plotui_bind::pick_element_px(
            &self.inner,
            px_w,
            px_h,
            px,
            py,
            node_radius,
            edge_radius,
        ))
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
    /// Pin the 3D data frame to `(lo, hi)` corners instead of the nodes'
    /// bounding box — so a plot rebuilt with a subset of the same data keeps
    /// every remaining node at the same pixel. `None` restores auto-fit.
    #[pyo3(signature = (lo, hi))]
    fn set_bounds(&mut self, lo: Option<(f32, f32, f32)>, hi: Option<(f32, f32, f32)>) {
        self.inner.bounds_override = match (lo, hi) {
            (Some(lo), Some(hi)) => Some(([lo.0, lo.1, lo.2], [hi.0, hi.1, hi.2])),
            _ => None,
        };
    }

    fn set_show_box(&mut self, show: bool) {
        self.inner.show_box = show;
    }

    /// Recolour the chrome (everything that is not data) to sit on the
    /// host's own background. Each is an `(r, g, b)` tuple; omitted ones keep
    /// their current value. `bg` fills the legend box, `frame` draws the
    /// axes, tick marks and legend border, `grid` the grid lines, `ink` the
    /// tick labels, `ink_bright` the legend text.
    #[pyo3(signature = (bg=None, frame=None, grid=None, ink=None, ink_bright=None))]
    fn set_chrome(
        &mut self,
        bg: Option<(u8, u8, u8)>,
        frame: Option<(u8, u8, u8)>,
        grid: Option<(u8, u8, u8)>,
        ink: Option<(u8, u8, u8)>,
        ink_bright: Option<(u8, u8, u8)>,
    ) {
        let c = &mut self.inner.chrome;
        if let Some(v) = bg {
            c.bg = [v.0, v.1, v.2];
        }
        if let Some(v) = frame {
            c.frame = [v.0, v.1, v.2];
        }
        if let Some(v) = grid {
            c.grid = [v.0, v.1, v.2];
        }
        if let Some(v) = ink {
            c.ink = [v.0, v.1, v.2];
        }
        if let Some(v) = ink_bright {
            c.ink_bright = [v.0, v.1, v.2];
        }
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
    /// `scale` (0 < s ≤ 1) shrinks the rasterized framebuffer while the image
    /// still fills the same `cols`×`rows` cells — the terminal upscales it.
    /// Used for cheap half-resolution frames during interaction.
    /// `replace=True` skips the delete-before-transmit: use it on terminals
    /// whose Kitty decoder replaces a same-id image atomically (e.g. xterm.js
    /// addon-image), where the delete otherwise blanks the image between the
    /// async redraws and flickers during interaction. Leave it `False` for
    /// iTerm2, which stacks placements without the delete.
    #[pyo3(signature = (cols, rows, cell_w, cell_h, compat_chunks=false, scale=1.0, replace=false))]
    #[allow(clippy::too_many_arguments)]
    fn render_kitty(
        &self,
        py: Python<'_>,
        cols: u16,
        rows: u16,
        cell_w: u16,
        cell_h: u16,
        compat_chunks: bool,
        scale: f64,
        replace: bool,
    ) -> String {
        py.allow_threads(|| {
            let (pw, ph, pan_scale) = scaled_dims(cols, rows, cell_w, cell_h, scale);
            let fb = self.inner.render_at(pw, ph, pan_scale);
            if compat_chunks {
                plotui_protocol::kitty_compat(&fb, cols, rows, !replace)
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
    #[pyo3(signature = (cols, rows, cell_w, cell_h, scale=1.0))]
    fn render_kitty_placeholder_cells(
        &self,
        py: Python<'_>,
        cols: u16,
        rows: u16,
        cell_w: u16,
        cell_h: u16,
        scale: f64,
    ) -> (String, (u8, u8, u8), Vec<Vec<String>>) {
        py.allow_threads(|| {
            let (pw, ph, pan_scale) = scaled_dims(cols, rows, cell_w, cell_h, scale);
            let fb = self.inner.render_at(pw, ph, pan_scale);
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

    /// True when any trace is 3D (the orbit-camera path). Lets a frontend
    /// decide whether interaction-time half-res rendering applies.
    fn is_3d(&self) -> bool {
        self.inner.is_3d()
    }

    /// Number of pickable nodes across all traces — a cheap "is this plot big
    /// enough to bother rendering at reduced resolution while moving?" signal.
    fn node_count(&self) -> usize {
        self.inner.node_count()
    }

    /// Every 3D vertex that gets drawn, including non-pickable geometry
    /// (line vertices, surface grids) — the load metric for deciding on
    /// reduced-resolution interaction frames.
    fn vertex_count(&self) -> usize {
        self.inner.vertex_count()
    }

    /// Escape sequence that removes plotui's image from the terminal.
    #[staticmethod]
    fn kitty_cleanup() -> String {
        plotui_protocol::kitty_cleanup()
    }
}

/// Zip three coordinate sequences into points, truncated to the shortest
/// (the shared pairing rule — see `plotui_bind::zip3`).
fn zip3(xs: Coords, ys: Coords, zs: Coords) -> Vec<[f32; 3]> {
    plotui_bind::zip3(&xs.0, &ys.0, &zs.0)
}

use plotui_term::policy::scaled_dims;

/// Best render path for this terminal: `"placeholder"`, `"direct"`, or
/// `"unsupported"`. Pass `env` to detect against an explicit environment
/// (tests); `None` reads the process environment. Honors `PLOTUI_RENDER`.
#[pyfunction]
#[pyo3(signature = (env=None))]
fn detect_render_mode(env: Option<std::collections::HashMap<String, String>>) -> &'static str {
    let mode = match env {
        Some(map) => plotui_term::detect_render_mode_from(|k| map.get(k).cloned()),
        None => plotui_term::detect_render_mode(),
    };
    match mode {
        plotui_term::RenderMode::Placeholder => "placeholder",
        plotui_term::RenderMode::Direct => "direct",
        plotui_term::RenderMode::Unsupported => "unsupported",
    }
}

/// The terminal's device pixels per cell via the TIOCGWINSZ ioctl, or
/// `fallback` when no stream reports a pixel size.
#[pyfunction]
#[pyo3(signature = (fallback=plotui_term::FALLBACK_CELL_PX))]
fn detect_cell_px(fallback: (u16, u16)) -> (u16, u16) {
    plotui_term::detect_cell_px(fallback)
}

/// The per-cell pixel size from a winsize report `(rows, cols, xpix, ypix)`,
/// or `None` when the terminal reports no pixel size — the pure, tty-free
/// core of `detect_cell_px`.
#[pyfunction]
fn cell_px_from_winsize(rows: u16, cols: u16, xpix: u16, ypix: u16) -> Option<(u16, u16)> {
    plotui_term::cell_px_from_winsize(rows, cols, xpix, ypix)
}

/// Wrap a terminal escape for tmux passthrough when `$TMUX` is set (a no-op
/// otherwise).
#[pyfunction]
fn tmux_wrap(escape: &str) -> String {
    plotui_term::tmux_wrap(escape)
}

#[pymodule]
fn _plotui(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Plot>()?;
    m.add_function(pyo3::wrap_pyfunction!(detect_render_mode, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(detect_cell_px, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(cell_px_from_winsize, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(tmux_wrap, m)?)?;
    m.add("__doc__", "Native rendering core for plotui.")?;
    Ok(())
}
