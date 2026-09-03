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
use pyo3::types::{IntoPyDict, PyDict};

/// The shared binding semantics (plotui-bind) report errors as `BindError`;
/// Python surfaces every one as a ValueError with the message verbatim.
fn to_py(e: plotui_bind::BindError) -> PyErr {
    PyValueError::new_err(e.msg)
}

/// Core trace errors (`TraceError`) surface as ValueError with the core's
/// canonical Display text, identically across bindings.
fn trace_to_py(e: plotui_core::TraceError) -> PyErr {
    PyValueError::new_err(e.to_string())
}

/// A color from Python: an `(r, g, b)` tuple, or a string shorthand —
/// `"#rrggbb"` hex or a name like `"red"` (the shared `plotui_bind` rule,
/// so the accepted names and error messages match every other binding).
#[derive(FromPyObject)]
enum ColorArg {
    Rgb((u8, u8, u8)),
    Shorthand(String),
}

impl ColorArg {
    fn rgb(self) -> PyResult<[u8; 3]> {
        match self {
            ColorArg::Rgb((r, g, b)) => Ok([r, g, b]),
            ColorArg::Shorthand(s) => plotui_bind::parse_color(&s).map_err(to_py),
        }
    }
}

fn opt_rgb(color: Option<ColorArg>) -> PyResult<Option<[u8; 3]>> {
    color.map(ColorArg::rgb).transpose()
}

fn rgb_list(colors: Vec<ColorArg>) -> PyResult<Vec<[u8; 3]>> {
    colors.into_iter().map(ColorArg::rgb).collect()
}

/// Explicit color, or the next colorway slot (see `Plot::resolve_color`).
fn resolve_color(plot: &plotui_core::Plot, color: Option<ColorArg>) -> PyResult<[u8; 3]> {
    Ok(plot.resolve_color(opt_rgb(color)?))
}

/// A colorway from Python: a built-in name ("plotui", "muted", "vivid") or a
/// list of colors (tuples or shorthand strings).
#[derive(FromPyObject)]
enum ColorwayArg {
    Name(String),
    List(Vec<ColorArg>),
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

const MIXED_X_MSG: &str = "cannot mix datetime and numeric x on one plot";

/// A 2D x column: numbers, or datetimes (a numpy `datetime64` array, a
/// pandas `DatetimeIndex`, or a list of `datetime.datetime`). Datetimes land
/// as absolute epoch seconds and are re-based against the plot's `x_epoch`
/// by `resolve_x`; naive datetimes are read as UTC wall time (matching the
/// engine's UTC calendar axis), aware ones convert exactly.
enum XCoords {
    Numeric(Vec<f32>),
    Times(Vec<f64>),
}

impl<'py> FromPyObject<'py> for XCoords {
    fn extract_bound(ob: &Bound<'py, PyAny>) -> PyResult<Self> {
        let py = ob.py();
        // The numeric fast paths first — a plain series must never pay a
        // datetime probe.
        if numpy_loaded(py) {
            if let Ok(a) = ob.downcast::<PyArray1<f32>>() {
                return Ok(XCoords::Numeric(a.readonly().as_array().iter().copied().collect()));
            }
            if let Ok(a) = ob.downcast::<PyArray1<f64>>() {
                return Ok(XCoords::Numeric(
                    a.readonly().as_array().iter().map(|&v| v as f32).collect(),
                ));
            }
        }
        if let Ok(v) = ob.extract::<Vec<f32>>() {
            return Ok(XCoords::Numeric(v));
        }
        // numpy datetime64 (asarray also lands a pandas DatetimeIndex).
        if numpy_loaded(py) {
            if let Ok(arr) = py.import("numpy").and_then(|np| np.call_method1("asarray", (ob,))) {
                let kind: String = arr
                    .getattr("dtype")
                    .and_then(|d| d.getattr("kind"))
                    .and_then(|k| k.extract())
                    .unwrap_or_default();
                if kind == "M" {
                    let ns = arr
                        .call_method1("astype", ("datetime64[ns]",))?
                        .call_method1("astype", ("int64",))?;
                    let r = ns.downcast::<PyArray1<i64>>().map_err(PyErr::from)?.readonly();
                    return Ok(XCoords::Times(
                        r.as_array().iter().map(|&v| v as f64 / 1e9).collect(),
                    ));
                }
            }
        }
        // A list of datetime.datetime.
        if let Ok(items) = ob.extract::<Vec<Bound<'py, PyAny>>>() {
            let dt = py.import("datetime")?;
            let dt_type = dt.getattr("datetime")?;
            if items.first().is_some_and(|i| i.is_instance(&dt_type).unwrap_or(false)) {
                let utc = dt.getattr("timezone")?.getattr("utc")?;
                let mut ts = Vec::with_capacity(items.len());
                for item in &items {
                    if !item.is_instance(&dt_type)? {
                        return Err(PyValueError::new_err(MIXED_X_MSG));
                    }
                    let aware = if item.getattr("tzinfo")?.is_none() {
                        item.call_method(
                            "replace",
                            (),
                            Some(&[("tzinfo", &utc)].into_py_dict(py)?),
                        )?
                    } else {
                        item.clone()
                    };
                    ts.push(aware.call_method0("timestamp")?.extract::<f64>()?);
                }
                return Ok(XCoords::Times(ts));
            }
        }
        // Fall back to the numeric extractor's own error message.
        Ok(XCoords::Numeric(ob.extract()?))
    }
}

/// Land an x column on the plot: datetimes set (or re-use) `x_epoch` and
/// become f32 offsets; numerics require the plot not to be on a time axis.
/// The first datetime column pins the epoch to its first timestamp's UTC
/// midnight.
fn resolve_x(plot: &mut plotui_core::Plot, xs: XCoords) -> PyResult<Vec<f32>> {
    match xs {
        XCoords::Numeric(v) => {
            if plot.x_epoch.is_some() {
                return Err(PyValueError::new_err(MIXED_X_MSG));
            }
            Ok(v)
        }
        XCoords::Times(ts) => {
            let base = match plot.x_epoch {
                Some(b) => b,
                None => {
                    // Core owns the 2D/3D split, so a new 2D trace joins this
                    // guard automatically instead of silently escaping it.
                    let has_2d = plot.traces.iter().any(|t| !t.is_3d());
                    if has_2d {
                        return Err(PyValueError::new_err(MIXED_X_MSG));
                    }
                    let first = ts.first().copied().unwrap_or(0.0);
                    let b = (first / 86_400.0).floor() * 86_400.0;
                    plot.x_epoch = Some(b);
                    b
                }
            };
            Ok(ts.into_iter().map(|t| (t - base) as f32).collect())
        }
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

    /// Swap the color sequence assigned to traces added without an explicit
    /// color: a built-in name — "plotui" (the default), "muted", "vivid" —
    /// or a list of colors (tuples or shorthand strings). Traces already
    /// added keep the colors they resolved to.
    fn set_colorway(&mut self, colorway: ColorwayArg) -> PyResult<()> {
        let colors = match colorway {
            ColorwayArg::Name(name) => plotui_bind::colorway(&name).map_err(to_py)?.to_vec(),
            ColorwayArg::List(colors) => rgb_list(colors)?,
        };
        plotui_bind::check_colorway(&colors).map_err(to_py)?;
        self.inner.set_colorway(colors);
        Ok(())
    }

    /// Add a 3D scatter series. `xs/ys/zs` accept any float sequence; numpy
    /// float32/float64 arrays are read in one bulk copy. `color` is an
    /// (r, g, b) tuple or a shorthand string ("#e63c78", "red"); omitted,
    /// colorway slots are assigned in fixed order. `name` puts the series in
    /// the legend. Returns the trace handle for `extend`/`set_visible`.
    #[pyo3(signature = (xs, ys, zs, color=None, size=3.0, name=None))]
    fn add_scatter3d(
        &mut self,
        xs: Coords,
        ys: Coords,
        zs: Coords,
        color: Option<ColorArg>,
        size: f32,
        name: Option<String>,
    ) -> PyResult<usize> {
        let c = resolve_color(&self.inner, color)?;
        let pts = zip3(xs, ys, zs);
        Ok(self.inner.add_scatter3d(pts, c, size, name))
    }

    /// Add a 3D graph: nodes at `xs/ys/zs`, `edges` as (i, j) index pairs,
    /// optional per-node `node_colors`, else a uniform `color`. `node_sizes`
    /// overrides `size` per node; `edge_colors` (one per edge) overrides the
    /// default dimmed endpoint-average edge color. `node_shapes` picks a
    /// marker silhouette per node — "disc", "ring", "square", "triangle",
    /// "diamond", "diamond-open", "dot" — so node categories read by
    /// shape as well as colour; an unknown name is a ValueError. `name`
    /// puts the graph in the legend.
    #[pyo3(signature = (
        xs, ys, zs, edges, node_colors=None, color=None, size=3.5,
        node_sizes=None, edge_colors=None, node_shapes=None, name=None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn add_graph3d(
        &mut self,
        xs: Coords,
        ys: Coords,
        zs: Coords,
        edges: Vec<(u32, u32)>,
        node_colors: Option<Vec<ColorArg>>,
        color: Option<ColorArg>,
        size: f32,
        node_sizes: Option<Vec<f32>>,
        edge_colors: Option<Vec<ColorArg>>,
        node_shapes: Option<Vec<String>>,
        name: Option<String>,
    ) -> PyResult<usize> {
        let uniform = resolve_color(&self.inner, color)?;
        let nodes = zip3(xs, ys, zs);
        let n = nodes.len();
        let nc = node_colors.map(rgb_list).transpose()?;
        let colors = plotui_bind::graph_node_colors(n, nc, uniform);
        let ec = edge_colors.map(rgb_list).transpose()?;
        let shapes = match node_shapes {
            Some(names) => Some(plotui_bind::parse_shapes(&names).map_err(to_py)?),
            None => None,
        };
        Ok(self.inner.add_graph3d(nodes, colors, edges, size, node_sizes, ec, shapes, name))
    }

    /// Add a directed graph in the 2D plane: labelled boxes at (xs, ys),
    /// wired by `edges` as (from, to) index pairs. This is the pipeline /
    /// DAG chart — pair it with `LayeredLayout` for the positions, or place
    /// the nodes yourself.
    ///
    /// `labels` names the boxes (defaulting to unlabelled); `node_colors`
    /// takes one colour per node, which is the channel a live pipeline
    /// repaints through `set_graph_colors`; `node_shapes` takes "rounded",
    /// "box", "ellipse" or "diamond" per node, and an unknown name is a
    /// ValueError. `routes` gives each edge its waypoints as a list of
    /// (x, y) lists — what `LayeredLayout.routes()` returns — with an empty
    /// list for a straight edge. `directed=False` drops the arrowheads.
    ///
    /// Node *centres* are in data coordinates but their boxes are sized in
    /// pixels from the label, so zooming spreads the graph apart while the
    /// text stays legible. A plot whose visible 2D traces are all graphs
    /// draws no axes; see the `show_axes` property.
    #[pyo3(signature = (
        xs, ys, edges, labels=None, directed=true, node_colors=None, color=None,
        node_shapes=None, edge_colors=None, routes=None, name=None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn add_graph2d(
        &mut self,
        xs: Coords,
        ys: Coords,
        edges: Vec<(u32, u32)>,
        labels: Option<Vec<String>>,
        directed: bool,
        node_colors: Option<Vec<ColorArg>>,
        color: Option<ColorArg>,
        node_shapes: Option<Vec<String>>,
        edge_colors: Option<Vec<ColorArg>>,
        routes: Option<Vec<Vec<(f32, f32)>>>,
        name: Option<String>,
    ) -> PyResult<usize> {
        let uniform = resolve_color(&self.inner, color)?;
        let n = xs.0.len().min(ys.0.len());
        let nodes: Vec<[f32; 2]> = (0..n).map(|i| [xs.0[i], ys.0[i]]).collect();
        let labels = labels.unwrap_or_default();
        // Short lists pad rather than truncate, the way the per-point style
        // arrays do: a partial mapping must never drop a node.
        let labels: Vec<String> =
            (0..n).map(|i| labels.get(i).cloned().unwrap_or_default()).collect();
        let nc = node_colors.map(rgb_list).transpose()?;
        let colors = plotui_bind::graph_node_colors(n, nc, uniform);
        let ec = edge_colors.map(rgb_list).transpose()?;
        let shapes = match node_shapes {
            Some(names) => Some(plotui_bind::parse_node_shapes(&names).map_err(to_py)?),
            None => None,
        };
        let routes = match routes {
            None => None,
            Some(rs) => {
                let nested: Vec<Vec<[f32; 2]>> =
                    rs.into_iter().map(|r| r.into_iter().map(|(x, y)| [x, y]).collect()).collect();
                let (pts, starts) = plotui_bind::flatten_routes(nested);
                plotui_bind::check_routes(edges.len(), pts.len(), &starts).map_err(to_py)?;
                Some((pts, starts))
            }
        };
        Ok(self.inner.add_graph2d(nodes, labels, colors, edges, directed, shapes, ec, routes, name))
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
        color: Option<ColorArg>,
        width: f32,
        name: Option<String>,
    ) -> PyResult<usize> {
        let pts = zip3(xs, ys, zs);
        let c = resolve_color(&self.inner, color)?;
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
        color: Option<ColorArg>,
        colormap: Option<&str>,
        wireframe: bool,
        name: Option<String>,
    ) -> PyResult<usize> {
        let (xs, ys) = (xs.0, ys.0);
        let flat = plotui_bind::flatten_surface_grid(xs.len(), ys.len(), zs).map_err(to_py)?;
        let cm = plotui_bind::parse_colormap(colormap).map_err(to_py)?;
        let c = resolve_color(&self.inner, color)?;
        Ok(self.inner.add_surface3d(xs, ys, flat, c, cm, wireframe, name))
    }

    /// Add an indexed triangle mesh: vertices at (xs[i], ys[i], zs[i]) —
    /// the same (x, y, z) space as `add_scatter3d` — and `tris` a flat run
    /// of [a, b, c] vertex-index triples. `colormap` ("viridis" or "plasma",
    /// default "viridis") colors by z over the mesh's own range; pass
    /// `colormap=None` with a `color` for a solid mesh. A triangle whose
    /// index names no vertex is an error; one with a non-finite vertex is
    /// skipped at render time, the way a surface cell with a NaN corner is a
    /// hole. `name` puts the mesh in the legend. Vertices are not pickable.
    #[pyo3(signature = (xs, ys, zs, tris, color=None, colormap="viridis", name=None))]
    #[allow(clippy::too_many_arguments)]
    fn add_mesh3d(
        &mut self,
        xs: Coords,
        ys: Coords,
        zs: Coords,
        tris: Vec<[u32; 3]>,
        color: Option<ColorArg>,
        colormap: Option<&str>,
        name: Option<String>,
    ) -> PyResult<usize> {
        let verts = zip3(xs, ys, zs);
        // Validate through the shared rule so the message is byte-identical
        // to every other binding; nested triples can't be ragged, so only the
        // out-of-range arm is reachable from here.
        let flat: Vec<u32> = tris.iter().flatten().copied().collect();
        plotui_bind::check_mesh_indices(verts.len(), &flat).map_err(to_py)?;
        let cm = plotui_bind::parse_colormap(colormap).map_err(to_py)?;
        let c = resolve_color(&self.inner, color)?;
        Ok(self.inner.add_mesh3d(verts, tris, c, cm, name))
    }

    /// Add a 2D scatter series. With `color=None`, palette slots are assigned
    /// in fixed order. `name` puts the series in the legend. `axis="y2"` or
    /// `"y3"` binds the series to an independent right-hand axis (own
    /// autoscale and ticks, labels tinted to the series colour; y2 sits
    /// innermost, y3 outermost; the grid belongs to the left axis).
    #[pyo3(signature = (xs, ys, color=None, size=2.5, name=None, axis="y"))]
    fn add_scatter(
        &mut self,
        xs: XCoords,
        ys: Coords,
        color: Option<ColorArg>,
        size: f32,
        name: Option<String>,
        axis: &str,
    ) -> PyResult<usize> {
        let c = resolve_color(&self.inner, color)?;
        let xs = resolve_x(&mut self.inner, xs)?;
        Ok(self.inner.add_scatter2d(xs, ys.0, c, size, name, parse_axis(axis)?))
    }

    /// Add a 2D line series (2px stroke by default). `axis="y2"`/`"y3"` puts
    /// it on an independent right-hand axis, as in `add_scatter`.
    #[pyo3(signature = (xs, ys, color=None, width=2.0, name=None, axis="y"))]
    fn add_line(
        &mut self,
        xs: XCoords,
        ys: Coords,
        color: Option<ColorArg>,
        width: f32,
        name: Option<String>,
        axis: &str,
    ) -> PyResult<usize> {
        let c = resolve_color(&self.inner, color)?;
        let xs = resolve_x(&mut self.inner, xs)?;
        Ok(self.inner.add_line2d(xs, ys.0, c, width, name, parse_axis(axis)?))
    }

    /// Add a box plot: `groups` is a list of samples, one per box. Group *i*
    /// sits at position *i*, so `set_categories("x", …)` names the boxes (or
    /// `"y"` with `orientation="horizontal"`).
    ///
    /// Boxes span the quartiles with a median line; whiskers reach the
    /// furthest values within 1.5·IQR, and anything beyond is drawn as its own
    /// point rather than being swallowed by a longer whisker.
    #[pyo3(signature = (groups, color=None, orientation="vertical", name=None, axis="y"))]
    #[allow(clippy::too_many_arguments)]
    fn add_box(
        &mut self,
        groups: Vec<Vec<f32>>,
        color: Option<ColorArg>,
        orientation: &str,
        name: Option<String>,
        axis: &str,
    ) -> PyResult<usize> {
        let (values, starts) = plotui_bind::flatten_box_groups(groups).map_err(to_py)?;
        let orient = plotui_bind::parse_orient(orientation).map_err(to_py)?;
        let c = resolve_color(&self.inner, color)?;
        Ok(self.inner.add_box2d(values, starts, c, orient, name, parse_axis(axis)?))
    }

    /// Add a filled band between two boundaries at each x — a confidence
    /// interval, a min/max envelope, a tolerance range.
    ///
    /// Add it *before* the line it belongs to: draw order is the only
    /// layering in 2D, so a band added afterwards paints over its own centre
    /// line.
    #[pyo3(signature = (xs, lo, hi, color=None, name=None, axis="y"))]
    #[allow(clippy::too_many_arguments)]
    fn add_band(
        &mut self,
        xs: XCoords,
        lo: Coords,
        hi: Coords,
        color: Option<ColorArg>,
        name: Option<String>,
        axis: &str,
    ) -> PyResult<usize> {
        let c = resolve_color(&self.inner, color)?;
        let xs = resolve_x(&mut self.inner, xs)?;
        Ok(self.inner.add_band2d(xs, lo.0, hi.0, c, name, parse_axis(axis)?))
    }

    /// Attach per-point error bars to a 2D scatter or line. Give
    /// `y_plus`/`y_minus` (or `x_plus`/`x_minus`); omit the `minus` half for
    /// the symmetric case. Pass nothing for an axis to clear its bars.
    ///
    /// Error bars belong to the series: they take its color and stay out of
    /// the legend, so they cannot drift out of step with the points.
    #[pyo3(signature = (handle, y_plus=None, y_minus=None, x_plus=None, x_minus=None))]
    fn set_error_bars(
        &mut self,
        handle: usize,
        y_plus: Option<Vec<f32>>,
        y_minus: Option<Vec<f32>>,
        x_plus: Option<Vec<f32>>,
        x_minus: Option<Vec<f32>>,
    ) -> PyResult<()> {
        let ey = plotui_bind::error_bars(y_plus.unwrap_or_default(), y_minus.unwrap_or_default());
        let ex = plotui_bind::error_bars(x_plus.unwrap_or_default(), x_minus.unwrap_or_default());
        plotui_bind::set_error_bars(&mut self.inner, handle, ex, ey).map_err(to_py)
    }

    /// Add a heatmap: `zs` is a list of rows (or a 2D numpy array), `zs[j][i]`
    /// giving the value at (xs[i], ys[j]) — the same grid shape
    /// `add_surface3d` takes. Cells centre on their coordinates and tile
    /// outward by half a step, so a regular grid meets edge to edge; a NaN
    /// value leaves a hole rather than a zero.
    ///
    /// `colorbar=True` (the default) puts a labelled ramp beside the plot
    /// spanning this grid's own range — without one the colors show structure
    /// but no values.
    #[pyo3(signature = (xs, ys, zs, colormap="viridis", colorbar=true, label=None, name=None))]
    #[allow(clippy::too_many_arguments)]
    fn add_heatmap(
        &mut self,
        xs: Coords,
        ys: Coords,
        zs: Vec<Vec<f32>>,
        colormap: &str,
        colorbar: bool,
        label: Option<String>,
        name: Option<String>,
    ) -> PyResult<usize> {
        let (xs, ys) = (xs.0, ys.0);
        let flat = plotui_bind::flatten_surface_grid(xs.len(), ys.len(), zs).map_err(to_py)?;
        let cm = plotui_bind::parse_colormap(Some(colormap))
            .map_err(to_py)?
            .expect("a named colormap always resolves");
        let h = self.inner.add_heatmap2d(xs, ys, flat, cm, name);
        if colorbar {
            if let Some((lo, hi)) = self.inner.heatmap_range(h) {
                self.inner.colorbar = Some(plotui_core::Colorbar { map: cm, lo, hi, label });
            }
        }
        Ok(h)
    }

    /// Add a histogram of `values`. Give `bins` for a bin count, `bin_width`
    /// for a fixed width, or neither for the Freedman–Diaconis rule (which
    /// adapts to spread rather than to sample size). The raw values are kept,
    /// so `extend_values` can add observations later and the crosshair reads
    /// out each bar's interval and count.
    ///
    /// Bins are solved once from the whole sample and do not change with
    /// zoom: edges that shifted while panning would change the shape of the
    /// distribution under your hands.
    #[pyo3(signature = (values, bins=None, bin_width=None, color=None, name=None, axis="y"))]
    #[allow(clippy::too_many_arguments)]
    fn add_histogram(
        &mut self,
        values: Coords,
        bins: Option<usize>,
        bin_width: Option<f64>,
        color: Option<ColorArg>,
        name: Option<String>,
        axis: &str,
    ) -> PyResult<usize> {
        let spec = plotui_bind::parse_bins(bins, bin_width).map_err(to_py)?;
        let c = resolve_color(&self.inner, color)?;
        Ok(self.inner.add_histogram2d(values.0, spec, c, name, parse_axis(axis)?))
    }

    /// Append observations to a histogram and rebin. Unlike `extend` on a
    /// coordinate series this is not an O(delta) update: one new value can
    /// move the range and every bin edge with it.
    fn extend_values(&mut self, handle: usize, values: Coords) -> PyResult<()> {
        plotui_bind::extend_values(&mut self.inner, handle, &values.0).map_err(to_py)
    }

    /// Add a 2D step series: the right-angle path between samples rather
    /// than the straight one. Use it for anything that *holds* its value
    /// between samples — counters, states, prices — where a straight segment
    /// would draw a transition that never happened. `where_` is "post" (the
    /// old value holds until the next sample, the default), "pre" (the new
    /// value applies from the previous one), or "mid" (the riser sits halfway
    /// between).
    #[pyo3(signature = (xs, ys, color=None, width=2.0, where_="post", name=None, axis="y"))]
    #[allow(clippy::too_many_arguments)]
    fn add_step(
        &mut self,
        xs: XCoords,
        ys: Coords,
        color: Option<ColorArg>,
        width: f32,
        where_: &str,
        name: Option<String>,
        axis: &str,
    ) -> PyResult<usize> {
        let c = resolve_color(&self.inner, color)?;
        let interp = plotui_bind::parse_interp(where_).map_err(to_py)?;
        let xs = resolve_x(&mut self.inner, xs)?;
        Ok(self.inner.add_step2d(xs, ys.0, c, width, interp, name, parse_axis(axis)?))
    }

    /// Add a 2D bar series: bars at `xs` rising (or falling) from zero to
    /// `heights`. Bar width comes from the smallest gap between x positions.
    /// `axis="y2"`/`"y3"` puts it on an independent right-hand axis, whose
    /// own scale supplies the zero baseline.
    #[pyo3(signature = (xs, heights, color=None, orientation="vertical", name=None, axis="y"))]
    #[allow(clippy::too_many_arguments)]
    fn add_bar(
        &mut self,
        xs: XCoords,
        heights: Coords,
        color: Option<ColorArg>,
        orientation: &str,
        name: Option<String>,
        axis: &str,
    ) -> PyResult<usize> {
        let c = resolve_color(&self.inner, color)?;
        let orient = plotui_bind::parse_orient(orientation).map_err(to_py)?;
        let xs = resolve_x(&mut self.inner, xs)?;
        Ok(self.inner.add_bar2d_oriented(xs, heights.0, c, orient, name, parse_axis(axis)?))
    }

    /// Set how several bar series on one axis share their positions:
    /// "overlay" (the default — each draws at full width, so equal positions
    /// overplot), "group" (side by side, each taking 1/n of the width), or
    /// "stack" (each starting where the one below ended).
    ///
    /// Stacking accumulates same-signed values only, so a mix of positive and
    /// negative heights grows both ways from the baseline instead of
    /// cancelling into a net figure the reader cannot decompose.
    fn set_barmode(&mut self, mode: &str) -> PyResult<bool> {
        plotui_bind::set_barmode(&mut self.inner, mode).map_err(to_py)
    }

    /// Name an axis's categories: category *i* sits at position *i*, and the
    /// ticks become one label per category instead of a numeric ladder. Pass
    /// an empty list to go back to numbers.
    ///
    /// Naming categories does not move the range — traces still place
    /// themselves — so a series plotted at 0, 1, 2 lines up with the first
    /// three names. Pair `set_categories("y", ...)` with
    /// `orientation="horizontal"` for readable long labels.
    #[pyo3(signature = (axis, names))]
    fn set_categories(&mut self, axis: &str, names: Vec<String>) -> PyResult<bool> {
        plotui_bind::set_categories(&mut self.inner, axis, names).map_err(to_py)
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
        xs: XCoords,
        ys: Coords,
        zs: Option<Coords>,
    ) -> PyResult<()> {
        // Numeric appends pass through untouched — they are offsets in the
        // trace's own x space, valid on plain and time axes alike (and 3D
        // extends are always numeric). Datetime appends re-base against the
        // epoch the add established.
        let xs = match xs {
            XCoords::Numeric(v) => v,
            t @ XCoords::Times(_) => resolve_x(&mut self.inner, t)?,
        };
        plotui_bind::extend(&mut self.inner, handle, &xs, &ys.0, zs.as_ref().map(|z| &z.0[..]))
            .map_err(to_py)
    }

    /// Move every node of a graph trace at once — the per-frame call of a
    /// force-directed layout (pair with `ForceLayout`). Structure is
    /// untouched, so node/edge indices, hover, and selection stay valid;
    /// the point count must match the trace's node count.
    fn set_graph_positions(
        &mut self,
        handle: usize,
        xs: Coords,
        ys: Coords,
        zs: Coords,
    ) -> PyResult<()> {
        let pts = zip3(xs, ys, zs);
        self.inner.set_graph_positions(handle, pts).map_err(trace_to_py)
    }

    /// Replace a 2D graph's edge waypoints — the second half of a relayout,
    /// after `set_graph_positions` has moved the nodes. `routes` is one list
    /// of (x, y) points per edge (what `LayeredLayout.routes()` returns);
    /// pass an empty list to restore straight edges.
    fn set_graph_routes(&mut self, handle: usize, routes: Vec<Vec<(f32, f32)>>) -> PyResult<()> {
        let nested: Vec<Vec<[f32; 2]>> =
            routes.into_iter().map(|r| r.into_iter().map(|(x, y)| [x, y]).collect()).collect();
        let (pts, starts) = plotui_bind::flatten_routes(nested);
        self.inner.set_graph_routes(handle, pts, starts).map_err(trace_to_py)
    }

    /// Recolor a graph trace in place — the host-side highlight primitive:
    /// dim everything, brighten a hovered dependency path, restore.
    /// `node_colors` needs one color per node; `edge_colors`, when given,
    /// one per edge (`None` restores the default dimmed endpoint blend).
    /// Colors accept (r, g, b) tuples or shorthand strings.
    #[pyo3(signature = (handle, node_colors, edge_colors=None))]
    fn set_graph_colors(
        &mut self,
        handle: usize,
        node_colors: Vec<ColorArg>,
        edge_colors: Option<Vec<ColorArg>>,
    ) -> PyResult<()> {
        let nc = rgb_list(node_colors)?;
        let ec = edge_colors.map(rgb_list).transpose()?;
        self.inner.set_graph_colors(handle, nc, ec).map_err(trace_to_py)
    }

    /// Style a 2D scatter point by point. Each list is independent and
    /// optional: `colors` for a categorical or colormapped cloud, `sizes` for
    /// a bubble chart, `shapes` ("disc", "ring", "square", "triangle",
    /// "diamond", "diamond-open", "dot") for an encoding that survives a
    /// palette change. Pass `None` to leave a channel uniform, or a list
    /// shorter than the series to style a prefix of it.
    #[pyo3(signature = (handle, colors=None, sizes=None, shapes=None))]
    fn set_point_styles(
        &mut self,
        handle: usize,
        colors: Option<Vec<ColorArg>>,
        sizes: Option<Vec<f32>>,
        shapes: Option<Vec<String>>,
    ) -> PyResult<()> {
        let colors = colors.map(rgb_list).transpose()?.unwrap_or_default();
        let shapes = shapes.unwrap_or_default();
        let shapes: Vec<&str> = shapes.iter().map(String::as_str).collect();
        plotui_bind::set_point_styles(
            &mut self.inner,
            handle,
            colors,
            sizes.unwrap_or_default(),
            shapes,
        )
        .map_err(to_py)
    }

    /// Append nodes and edges to a graph trace — how new nodes arrive in a
    /// live graph without a rebuild (pair with `ForceLayout.add_node`).
    /// `edges` may reference old or new node indices; appended nodes take
    /// the trace's default size and shape. Same flat-index caveat as
    /// `extend` on a 3D scatter: appending to a graph that is not the last
    /// node-bearing trace shifts the flat indices of every node (and edge)
    /// after it — plotui remaps its own selection/hover, hosts holding
    /// indices must do the same.
    #[pyo3(signature = (handle, xs, ys, zs, node_colors=None, edges=vec![], labels=None))]
    #[allow(clippy::too_many_arguments)]
    fn extend_graph(
        &mut self,
        handle: usize,
        xs: Coords,
        ys: Coords,
        zs: Coords,
        node_colors: Option<Vec<ColorArg>>,
        edges: Vec<(u32, u32)>,
        labels: Option<Vec<String>>,
    ) -> PyResult<()> {
        let pts = zip3(xs, ys, zs);
        let colors = node_colors.map(rgb_list).transpose()?.unwrap_or_default();
        // A 2D graph takes the same (xs, ys, zs) with z dropped, so one call
        // grows either dimension; `labels` names the new boxes and a 3D
        // graph, which has no labels, ignores it.
        self.inner
            .extend_graph(handle, &pts, &colors, &edges, labels.as_deref())
            .map_err(trace_to_py)
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

    /// Set the explicit 2D x view `(lo, hi)` in data coordinates, or `None`
    /// for full-extent autoscale. With a window set, the plot maps exactly
    /// that range, every y axis autoscales from the points inside it, and
    /// the camera's 2D zoom/pan is superseded. Returns True when the state
    /// changed (repaint needed). Ignored by 3D plots.
    #[pyo3(signature = (window))]
    fn set_x_window(&mut self, window: Option<(f64, f64)>) -> PyResult<bool> {
        plotui_bind::set_x_window(&mut self.inner, window).map_err(to_py)
    }

    /// The current x window as `(lo, hi)`, or `None`.
    fn x_window(&self) -> Option<(f64, f64)> {
        self.inner.x_window
    }

    /// Toggle the range-slider strip: a full-extent overview under the plot
    /// with the x-window selection in full color and grab handles at its
    /// edges. Silently dropped on frames too short to fit it. Returns True
    /// when the state changed.
    fn set_range_slider(&mut self, on: bool) -> bool {
        plotui_bind::set_range_slider(&mut self.inner, on)
    }

    /// Whether the range-slider strip is enabled.
    fn range_slider(&self) -> bool {
        self.inner.range_slider
    }

    /// Declare x values as seconds since this UTC epoch base (`None` clears):
    /// x ticks become calendar dates and the crosshair readout shows
    /// timestamps. Set automatically when a datetime x column is added.
    #[pyo3(signature = (epoch))]
    fn set_x_epoch(&mut self, epoch: Option<f64>) -> PyResult<bool> {
        plotui_bind::set_x_epoch(&mut self.inner, epoch).map_err(to_py)
    }

    /// The time-axis epoch base in epoch seconds, or `None`.
    fn x_epoch(&self) -> Option<f64> {
        self.inner.x_epoch
    }

    /// Set the chart title, drawn centered above the plot area. `None` (or
    /// `""`) clears it. Returns True when the state changed.
    #[pyo3(signature = (text))]
    fn set_title(&mut self, text: Option<String>) -> PyResult<bool> {
        plotui_bind::set_title(&mut self.inner, "title", text).map_err(to_py)
    }

    /// The chart title, or `None`.
    fn title(&self) -> Option<String> {
        self.inner.title.clone()
    }

    /// Set the x axis's title, drawn under its tick labels. `None` clears.
    #[pyo3(signature = (text))]
    fn set_x_title(&mut self, text: Option<String>) -> PyResult<bool> {
        plotui_bind::set_title(&mut self.inner, "x", text).map_err(to_py)
    }

    /// The x axis title, or `None`.
    fn x_title(&self) -> Option<String> {
        self.inner.x_title.clone()
    }

    /// Set the primary y axis's title, drawn rotated in the left margin.
    /// `None` clears. The right-hand axes take their identity from the color
    /// their labels are tinted in instead.
    #[pyo3(signature = (text))]
    fn set_y_title(&mut self, text: Option<String>) -> PyResult<bool> {
        plotui_bind::set_title(&mut self.inner, "y", text).map_err(to_py)
    }

    /// The y axis title, or `None`.
    fn y_title(&self) -> Option<String> {
        self.inner.y_title.clone()
    }

    /// Pin the x extent to `(lo, hi)`, or `None` to autoscale. Unlike
    /// `set_x_window` this decides the extent only — zoom and pan still
    /// compose on top of it — and it is used exactly as given, without
    /// autoscale's 5% padding. A set x window is the narrower statement and
    /// wins. Returns True when the state changed.
    #[pyo3(signature = (range))]
    fn set_x_range(&mut self, range: Option<(f64, f64)>) -> PyResult<bool> {
        plotui_bind::set_range(&mut self.inner, "x", range).map_err(to_py)
    }

    /// The explicit x range as `(lo, hi)`, or `None`.
    fn x_range(&self) -> Option<(f64, f64)> {
        self.inner.x_range
    }

    /// Pin the primary y extent to `(lo, hi)`, or `None` to autoscale. The
    /// right-hand axes keep autoscaling — they exist to fit a second series
    /// against its own spread.
    #[pyo3(signature = (range))]
    fn set_y_range(&mut self, range: Option<(f64, f64)>) -> PyResult<bool> {
        plotui_bind::set_range(&mut self.inner, "y", range).map_err(to_py)
    }

    /// The explicit y range as `(lo, hi)`, or `None`.
    fn y_range(&self) -> Option<(f64, f64)> {
        self.inner.y_range
    }

    /// Scale the x axis by log10. Ignored on a categorical or time axis:
    /// names and calendars own the coordinate they sit on. Returns True when
    /// the state changed.
    fn set_x_log(&mut self, on: bool) -> PyResult<bool> {
        plotui_bind::set_log(&mut self.inner, "x", on).map_err(to_py)
    }

    /// Whether the x axis is set to log10.
    fn x_log(&self) -> bool {
        self.inner.x_log
    }

    /// Scale the primary y axis by log10; ignored on a categorical y axis.
    /// The right-hand axes stay linear.
    fn set_y_log(&mut self, on: bool) -> PyResult<bool> {
        plotui_bind::set_log(&mut self.inner, "y", on).map_err(to_py)
    }

    /// Whether the primary y axis is set to log10.
    fn y_log(&self) -> bool {
        self.inner.y_log
    }

    /// What the range-slider strip has under `(px, py)` framebuffer pixels
    /// at a `px_w`×`px_h` frame, within `tol_px`: `"left"`, `"right"`,
    /// `"window"`, `"track"`, or `None` off the strip. Terminal mice report
    /// per cell, so pass at least one cell width of tolerance.
    fn range_slider_hit(
        &self,
        px_w: usize,
        px_h: usize,
        px: f32,
        py: f32,
        tol_px: f32,
    ) -> Option<&'static str> {
        self.inner.range_slider_hit(px_w, px_h, px, py, tol_px).map(plotui_bind::range_hit_to_parts)
    }

    /// Drag the grabbed strip `part` (a `range_slider_hit` string) by
    /// `dx_px` framebuffer pixels: handles resize the window, `"window"`
    /// (and `"track"`) slides it. With no window set, the drag starts from
    /// the full extent. Returns True when the window changed.
    fn drag_x_window(
        &mut self,
        px_w: usize,
        px_h: usize,
        part: &str,
        dx_px: f32,
    ) -> PyResult<bool> {
        let hit = plotui_bind::range_hit_from_parts(part).map_err(to_py)?;
        Ok(self.inner.drag_x_window(px_w, px_h, hit, dx_px))
    }

    /// Center the window on the strip position under `px` (a track click),
    /// keeping its span. Returns True when the window changed.
    fn jump_x_window(&mut self, px_w: usize, px_h: usize, px: f32) -> bool {
        self.inner.jump_x_window(px_w, px_h, px)
    }

    /// Slide a set window by a plot-area drag of `dx_px` framebuffer pixels
    /// (grab-the-data sign: drag right, view moves left). Returns True when
    /// the window changed.
    fn pan_x_window(&mut self, px_w: usize, px_h: usize, dx_px: f32) -> bool {
        self.inner.pan_x_window(px_w, px_h, dx_px)
    }

    /// Zoom the window about the data x under `px` framebuffer pixels
    /// (`factor > 1` zooms in), starting from the full extent when no window
    /// is set. Returns True when the window changed.
    fn zoom_x_window(&mut self, px_w: usize, px_h: usize, px: f32, factor: f64) -> bool {
        self.inner.zoom_x_window(px_w, px_h, px, factor)
    }

    /// Slide a set window by `frac` of its own span (positive = later x) —
    /// the keyboard step. Returns True when the window changed.
    fn shift_x_window(&mut self, frac: f64) -> bool {
        self.inner.shift_x_window(frac)
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
    /// One auto-rotate step: `step` radians of yaw, turned the way a
    /// rightward drag pushes the object, so a view that spins on its own
    /// and a user who grabs it agree. Negative `step` drifts the other way.
    /// Prefer this to `rotate` for an idle spin — `rotate` takes a raw
    /// camera delta, whose sign is the opposite one.
    fn spin(&mut self, step: f64) {
        self.inner.spin(step);
    }
    /// Remap what drag gestures do. Each argument names the camera control
    /// that gesture axis drives — "yaw", "pitch", "pan_x", "pan_y", "zoom"
    /// or "off", optionally prefixed with "-" to invert the axis — or None
    /// to keep its current binding. The default map is drag = rotate as a
    /// trackball (yaw/pitch, the drag grabs the object), shift-drag = pan;
    /// "-yaw"/"-pitch" restore camera-grab rotation.
    #[pyo3(signature = (drag_x=None, drag_y=None, shift_drag_x=None, shift_drag_y=None))]
    fn set_input_map(
        &mut self,
        drag_x: Option<&str>,
        drag_y: Option<&str>,
        shift_drag_x: Option<&str>,
        shift_drag_y: Option<&str>,
    ) -> PyResult<()> {
        let mut m = self.inner.input_map;
        for (slot, inv, name) in [
            (&mut m.drag_x, &mut m.invert_drag_x, drag_x),
            (&mut m.drag_y, &mut m.invert_drag_y, drag_y),
            (&mut m.shift_drag_x, &mut m.invert_shift_drag_x, shift_drag_x),
            (&mut m.shift_drag_y, &mut m.invert_shift_drag_y, shift_drag_y),
        ] {
            if let Some(name) = name {
                (*slot, *inv) = plotui_bind::parse_camera_control(name).map_err(to_py)?;
            }
        }
        self.inner.input_map = m;
        Ok(())
    }
    /// The current gesture map as `(drag_x, drag_y, shift_drag_x,
    /// shift_drag_y)` control names, in the same spelling `set_input_map`
    /// accepts (a `-` prefix marks an inverted axis). A frontend reads this
    /// to decompose a drag into the individual camera moves it maps to,
    /// rather than calling `apply_drag` and losing the breakdown.
    fn input_map(&self) -> (&'static str, &'static str, &'static str, &'static str) {
        let m = self.inner.input_map;
        (
            plotui_bind::camera_control_name(m.drag_x, m.invert_drag_x),
            plotui_bind::camera_control_name(m.drag_y, m.invert_drag_y),
            plotui_bind::camera_control_name(m.shift_drag_x, m.invert_shift_drag_x),
            plotui_bind::camera_control_name(m.shift_drag_y, m.invert_shift_drag_y),
        )
    }
    /// Route a drag through the input map (see `set_input_map`): `(dx, dy)`
    /// pointer deltas in whatever unit the scales are calibrated for —
    /// `rotate_scale` radians per unit, `pan_*_scale` framebuffer pixels
    /// per unit, `zoom_scale` log-zoom per unit.
    #[allow(clippy::too_many_arguments)]
    fn apply_drag(
        &mut self,
        dx: f64,
        dy: f64,
        shift: bool,
        rotate_scale: f64,
        pan_x_scale: f64,
        pan_y_scale: f64,
        zoom_scale: f64,
    ) {
        let scales = plotui_core::DragScales {
            rotate: rotate_scale,
            pan_x: pan_x_scale,
            pan_y: pan_y_scale,
            zoom: zoom_scale,
        };
        self.inner.apply_drag(dx, dy, shift, scales);
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

    /// Whether the 2D frame draws its chrome — grid, axis rules and tick
    /// labels. `None` (the default) decides automatically: a frame whose
    /// visible 2D traces are all graphs draws none of it, because a
    /// pipeline's coordinates are a layout rather than measurements. `True`
    /// always draws it (useful to see where a layout put its nodes), `False`
    /// never does. The legend, colorbar, range slider and crosshair are
    /// unaffected either way, and 3D plots ignore it.
    #[getter]
    fn get_show_axes(&self) -> Option<bool> {
        self.inner.show_axes
    }

    #[setter]
    fn set_show_axes(&mut self, show: Option<bool>) {
        self.inner.set_show_axes(show);
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

/// A 3D force-directed layout: connected nodes attract, all nodes repel, a
/// cooling temperature settles the motion. Pure math on the host's timer —
/// call `step()` per tick and hand `positions()` to
/// `Plot.set_graph_positions`. Deterministic for a given seed.
#[pyclass]
struct ForceLayout {
    inner: plotui_core::ForceLayout,
}

#[pymethods]
impl ForceLayout {
    /// A layout over `n_nodes` with seeded initial positions in the unit
    /// ball. `edges` are (i, j) index pairs; out-of-range endpoints are
    /// kept but inert, matching the renderer.
    #[new]
    #[pyo3(signature = (n_nodes, edges, seed=0))]
    fn new(n_nodes: usize, edges: Vec<(u32, u32)>, seed: u32) -> Self {
        ForceLayout { inner: plotui_core::ForceLayout::new(n_nodes, &edges, seed) }
    }

    /// One simulation tick. Returns the mean displacement — watch it to
    /// stop repainting once the layout settles (below ~1e-3 is settled).
    fn step(&mut self) -> f32 {
        self.inner.step()
    }

    /// Current node positions as (xs, ys, zs) lists, in index order — feed
    /// them straight to `Plot.set_graph_positions`.
    fn positions(&self) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
        let pts = self.inner.positions();
        (
            pts.iter().map(|p| p[0]).collect(),
            pts.iter().map(|p| p[1]).collect(),
            pts.iter().map(|p| p[2]).collect(),
        )
    }

    /// Warm insertion of one node connected to `neighbors` (existing
    /// indices): it spawns beside its first neighbor and re-heats the
    /// simulation so the neighborhood reorganizes. Returns the new node's
    /// index; pair with `Plot.extend_graph`.
    fn add_node(&mut self, neighbors: Vec<u32>) -> usize {
        self.inner.add_node(&neighbors)
    }
}

/// A hierarchical ("Sugiyama") layout for a directed graph: rank the nodes
/// by depth, order each rank to reduce edge crossings, then place them so
/// edges run as straight as they can. Solved in the constructor — there is
/// nothing to step, because a pipeline has one right shape.
///
/// Feed `positions()` and `routes()` straight to `Plot.add_graph2d`.
/// Deterministic: same input, same output, no randomness anywhere.
#[pyclass]
struct LayeredLayout {
    inner: plotui_core::LayeredLayout,
}

#[pymethods]
impl LayeredLayout {
    /// Lay out `n_nodes` connected by `edges` as (from, to) index pairs,
    /// flowing in `rankdir` — "TB" (sources on top, the default) or "LR"
    /// (sources on the left). Self-loops and out-of-range endpoints are kept
    /// inert, so an edge list can be passed verbatim from the plot; cycles
    /// do not hang, since a back edge is reversed for the layout only.
    #[new]
    #[pyo3(signature = (n_nodes, edges, rankdir="TB"))]
    fn new(n_nodes: usize, edges: Vec<(u32, u32)>, rankdir: &str) -> PyResult<Self> {
        let dir = plotui_bind::parse_rankdir(rankdir).map_err(to_py)?;
        Ok(LayeredLayout { inner: plotui_core::LayeredLayout::new(n_nodes, &edges, dir) })
    }

    /// Node centres as (xs, ys) lists, in the caller's index order.
    fn positions(&self) -> (Vec<f32>, Vec<f32>) {
        let pts = self.inner.positions();
        (pts.iter().map(|p| p[0]).collect(), pts.iter().map(|p| p[1]).collect())
    }

    /// Each node's rank: 0 for a source, one more than its deepest
    /// predecessor otherwise.
    fn ranks(&self) -> Vec<u32> {
        self.inner.ranks().to_vec()
    }

    /// Edge waypoints: one list of (x, y) points per edge, in the caller's
    /// edge order and direction, empty for a straight edge. Pass this
    /// straight to `Plot.add_graph2d(routes=...)`.
    fn routes(&self) -> Vec<Vec<(f32, f32)>> {
        let (pts, starts) = self.inner.routes();
        (0..starts.len())
            .map(|e| {
                let a = starts[e] as usize;
                let b = starts.get(e + 1).map_or(pts.len(), |v| *v as usize);
                pts[a.min(pts.len())..b.min(pts.len())].iter().map(|p| (p[0], p[1])).collect()
            })
            .collect()
    }
}

/// Parse a DOT document, lay it out, and return a ready-to-render `Plot`
/// whose graph trace is handle 0. `rankdir` overrides the document's own
/// ("TB" or "LR"); `None` honours whatever it says.
///
/// The accepted grammar is a subset: node and edge statements, chains
/// (`a -> b -> c`), braced fan-outs (`a -> {b c}`), `subgraph`s (contents
/// hoisted, grouping ignored), `node`/`edge`/`graph` attribute defaults,
/// `rankdir`, and `label` / `color` / `fillcolor` / `shape` /
/// `style=rounded` on nodes with `color` on edges. Unknown attributes are
/// ignored; HTML labels, node ports and a mismatched edge operator are a
/// ValueError naming the line and column.
#[pyfunction]
#[pyo3(signature = (text, rankdir=None))]
fn from_dot(text: &str, rankdir: Option<&str>) -> PyResult<Plot> {
    let dir = match rankdir {
        Some(name) => Some(plotui_bind::parse_rankdir(name).map_err(to_py)?),
        None => None,
    };
    let (plot, _, _) = plotui_bind::plot_from_dot(text, dir).map_err(to_py)?;
    Ok(Plot { inner: plot })
}

/// Which nodes are reachable from node `i` — everything it waits on with
/// `upstream=True` (the default), everything it leads to with
/// `upstream=False` — including `i` itself. Returns one bool per node.
///
/// This is the primitive behind "hover a task and light everything upstream
/// of it": pair it with `Plot.set_graph_colors`.
#[pyfunction]
#[pyo3(signature = (n_nodes, edges, i, upstream=true))]
fn reachable(n_nodes: usize, edges: Vec<(u32, u32)>, i: usize, upstream: bool) -> Vec<bool> {
    plotui_bind::reachable(n_nodes, &edges, i, upstream)
}

#[pymodule]
fn _plotui(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Plot>()?;
    m.add_class::<ForceLayout>()?;
    m.add_class::<LayeredLayout>()?;
    m.add_function(pyo3::wrap_pyfunction!(from_dot, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(reachable, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(detect_render_mode, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(detect_cell_px, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(cell_px_from_winsize, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(tmux_wrap, m)?)?;
    m.add("__doc__", "Native rendering core for plotui.")?;
    Ok(())
}
