//! wasm-bindgen bindings — expose the plotui engine to the browser.
//!
//! The design mirrors the PyO3 bindings: JavaScript owns the event loop and
//! input; this layer is a thin stateful handle. Pointer events call the
//! camera methods; a repaint calls `render`, then blits the RGBA bytes into
//! a canvas via a throwaway view over wasm memory (`frame_ptr`/`frame_len`),
//! so a wasm memory growth can never leave JS holding a stale buffer.

use plotui_core::Element;
use wasm_bindgen::prelude::*;

/// The shared binding semantics (plotui-bind) report errors as `BindError`;
/// JS surfaces every one as a thrown `Error` with the message verbatim.
fn to_js(e: plotui_bind::BindError) -> JsError {
    JsError::new(&e.msg)
}

/// A color from JS: `"#rrggbb"` (or bare `"rrggbb"`) hex or a name like
/// `"red"` (the shared `plotui_bind` rule), or `None` for the binding's
/// default — the next colorway slot for most traces.
fn parse_color(color: Option<&str>) -> Result<Option<[u8; 3]>, JsError> {
    color.map(|s| plotui_bind::parse_color(s).map_err(to_js)).transpose()
}

/// Explicit color, or the next palette slot (see `Plot::resolve_color`).
fn resolve_color(plot: &plotui_core::Plot, color: Option<&str>) -> Result<[u8; 3], JsError> {
    Ok(plot.resolve_color(parse_color(color)?))
}

fn parse_axis(axis: Option<String>) -> Result<plotui_core::YAxis, JsError> {
    plotui_bind::parse_axis(axis.as_deref().unwrap_or("y")).map_err(to_js)
}

/// A `pick_element` hit: a node or an edge, by flat index (nodes across all
/// 3D traces in insertion order; edges across graph traces).
#[wasm_bindgen]
#[derive(Clone, Copy)]
pub struct PickHit {
    pub is_edge: bool,
    pub index: usize,
}

/// A plot handle: data + camera + last rendered frame. Held by the JS
/// frontend for a plot's life.
#[wasm_bindgen]
pub struct Plot {
    inner: plotui_core::Plot,
    frame: Vec<u8>,
}

impl Plot {
    /// Wrap an already-built core plot — what the composers
    /// (`plot_from_dot`) hand back, rather than filling one in.
    fn from_core(inner: plotui_core::Plot) -> Plot {
        Plot { inner, frame: Vec::new() }
    }
}

#[wasm_bindgen]
impl Plot {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Plot {
        Plot::from_core(plotui_core::Plot::new())
    }

    /// Swap the color sequence for traces added without an explicit color:
    /// a built-in name ("plotui", "muted", "vivid"), or a list of color
    /// shorthand strings. Traces already added keep their colors.
    pub fn set_colorway(
        &mut self,
        name: Option<String>,
        colors: Option<Vec<String>>,
    ) -> Result<(), JsError> {
        let list = match (name, colors) {
            (Some(n), _) => plotui_bind::colorway(&n).map_err(to_js)?.to_vec(),
            (None, Some(strs)) => strs
                .iter()
                .map(|s| plotui_bind::parse_color(s).map_err(to_js))
                .collect::<Result<Vec<_>, _>>()?,
            (None, None) => return Err(JsError::new("set_colorway needs a name or a color list")),
        };
        plotui_bind::check_colorway(&list).map_err(to_js)?;
        self.inner.set_colorway(list);
        Ok(())
    }

    // ---- traces -------------------------------------------------------

    /// Add a 3D scatter series; `name` puts it in the legend. Returns the
    /// trace handle for `extend_xyz`/`set_visible`.
    pub fn add_scatter3d(
        &mut self,
        xs: &[f32],
        ys: &[f32],
        zs: &[f32],
        color: Option<String>,
        size: Option<f32>,
        name: Option<String>,
    ) -> Result<usize, JsError> {
        let c = resolve_color(&self.inner, color.as_deref())?;
        let pts = plotui_bind::zip3(xs, ys, zs);
        Ok(self.inner.add_scatter3d(pts, c, size.unwrap_or(3.0), name))
    }

    /// Add a 3D graph: nodes at `xs/ys/zs`, `edges` as flat index pairs
    /// `[a0, b0, a1, b1, …]`, a uniform node `color`, and marker `size`.
    /// `name` puts the graph in the legend.
    #[allow(clippy::too_many_arguments)]
    pub fn add_graph3d(
        &mut self,
        xs: &[f32],
        ys: &[f32],
        zs: &[f32],
        edges: &[u32],
        color: Option<String>,
        size: Option<f32>,
        name: Option<String>,
    ) -> Result<usize, JsError> {
        if !edges.len().is_multiple_of(2) {
            return Err(JsError::new("edges must be flat [a, b] index pairs; got an odd length"));
        }
        let c = resolve_color(&self.inner, color.as_deref())?;
        let nodes = plotui_bind::zip3(xs, ys, zs);
        let colors = plotui_bind::graph_node_colors(nodes.len(), None, c);
        let pairs = edges.as_chunks::<2>().0.iter().map(|&[a, b]| (a, b)).collect();
        Ok(self.inner.add_graph3d(
            nodes,
            colors,
            pairs,
            size.unwrap_or(3.5),
            None,
            None,
            None,
            name,
        ))
    }

    /// Add a directed graph in the 2D plane: labelled boxes at `(xs, ys)`,
    /// wired by `edges` as flat `[a0, b0, a1, b1, …]` index pairs. This is
    /// the pipeline / DAG chart — pair it with `LayeredLayout` for the
    /// positions and routes, or place the nodes yourself.
    ///
    /// `labels` names the boxes; `node_colors` takes one colour shorthand
    /// per node, which is the channel a live pipeline repaints through
    /// `set_graph_colors`; `node_shapes` takes "rounded", "box", "ellipse"
    /// or "diamond" per node. `route_pts` is interleaved x/y waypoints and
    /// `route_starts` one index per edge into them (the CSR pair
    /// `LayeredLayout.routes()` returns).
    ///
    /// Node *centres* are in data coordinates but their boxes are sized in
    /// pixels from the label, so zooming spreads the graph apart while the
    /// text stays legible. A plot whose visible 2D traces are all graphs
    /// draws no axes; see `set_show_axes`.
    #[allow(clippy::too_many_arguments)]
    pub fn add_graph2d(
        &mut self,
        xs: &[f32],
        ys: &[f32],
        edges: &[u32],
        labels: Option<Vec<String>>,
        directed: Option<bool>,
        node_colors: Option<Vec<String>>,
        color: Option<String>,
        node_shapes: Option<Vec<String>>,
        edge_colors: Option<Vec<String>>,
        route_pts: Option<Vec<f32>>,
        route_starts: Option<Vec<u32>>,
        name: Option<String>,
    ) -> Result<usize, JsError> {
        if !edges.len().is_multiple_of(2) {
            return Err(JsError::new("edges must be flat [a, b] index pairs; got an odd length"));
        }
        let uniform = resolve_color(&self.inner, color.as_deref())?;
        let n = xs.len().min(ys.len());
        let nodes: Vec<[f32; 2]> = (0..n).map(|i| [xs[i], ys[i]]).collect();
        let labels = labels.unwrap_or_default();
        let labels: Vec<String> =
            (0..n).map(|i| labels.get(i).cloned().unwrap_or_default()).collect();
        let parse = |v: Vec<String>| -> Result<Vec<[u8; 3]>, JsError> {
            v.iter().map(|s| plotui_bind::parse_color(s).map_err(to_js)).collect()
        };
        let nc = node_colors.map(parse).transpose()?;
        let colors = plotui_bind::graph_node_colors(n, nc, uniform);
        let ec = edge_colors.map(parse).transpose()?;
        let shapes = match node_shapes {
            Some(names) => Some(plotui_bind::parse_node_shapes(&names).map_err(to_js)?),
            None => None,
        };
        let pairs: Vec<(u32, u32)> =
            edges.as_chunks::<2>().0.iter().map(|&[a, b]| (a, b)).collect();
        let routes = match (route_pts, route_starts) {
            (Some(pts), Some(starts)) => {
                if !pts.len().is_multiple_of(2) {
                    return Err(JsError::new(
                        "route_pts must be flat [x, y] pairs; got an odd length",
                    ));
                }
                let pts: Vec<[f32; 2]> = pts.as_chunks::<2>().0.to_vec();
                plotui_bind::check_routes(pairs.len(), pts.len(), &starts).map_err(to_js)?;
                Some((pts, starts))
            }
            _ => None,
        };
        Ok(self.inner.add_graph2d(
            nodes,
            labels,
            colors,
            pairs,
            directed.unwrap_or(true),
            shapes,
            ec,
            routes,
            name,
        ))
    }

    /// Add a 3D polyline. `name` puts it in the legend.
    pub fn add_line3d(
        &mut self,
        xs: &[f32],
        ys: &[f32],
        zs: &[f32],
        color: Option<String>,
        width: Option<f32>,
        name: Option<String>,
    ) -> Result<usize, JsError> {
        let c = resolve_color(&self.inner, color.as_deref())?;
        let pts = plotui_bind::zip3(xs, ys, zs);
        Ok(self.inner.add_line3d(pts, c, width.unwrap_or(2.0), name))
    }

    /// Add a 3D surface over the grid `(xs[i], ys[j])` with flat heights
    /// `zs[j * xs.len() + i]`. Colormapped ("viridis" by default, or
    /// "plasma"), or solid when a `color` is given without a `colormap`.
    #[allow(clippy::too_many_arguments)]
    pub fn add_surface3d(
        &mut self,
        xs: &[f32],
        ys: &[f32],
        zs: &[f32],
        color: Option<String>,
        colormap: Option<String>,
        wireframe: Option<bool>,
        name: Option<String>,
    ) -> Result<usize, JsError> {
        plotui_bind::check_surface_grid_len(xs.len(), ys.len(), zs.len()).map_err(to_js)?;
        let cmap = match (&color, &colormap) {
            (Some(_), None) => None,
            _ => plotui_bind::parse_colormap(Some(colormap.as_deref().unwrap_or("viridis")))
                .map_err(to_js)?,
        };
        let c = resolve_color(&self.inner, color.as_deref())?;
        Ok(self.inner.add_surface3d(
            xs.to_vec(),
            ys.to_vec(),
            zs.to_vec(),
            c,
            cmap,
            wireframe.unwrap_or(false),
            name,
        ))
    }

    /// Add a triangle mesh: `xs`/`ys`/`zs` are the vertices, `tris` the flat
    /// `[a0, b0, c0, a1, …]` index triples that join them. Colormapped
    /// ("viridis" by default, or "plasma") over the mesh's own z range, or
    /// solid when a `color` is given without a `colormap`. Pair with
    /// `marching_cubes` for an iso-surface.
    #[allow(clippy::too_many_arguments)]
    pub fn add_mesh3d(
        &mut self,
        xs: &[f32],
        ys: &[f32],
        zs: &[f32],
        tris: &[u32],
        color: Option<String>,
        colormap: Option<String>,
        name: Option<String>,
    ) -> Result<usize, JsError> {
        let verts = plotui_bind::zip3(xs, ys, zs);
        plotui_bind::check_mesh_indices(verts.len(), tris).map_err(to_js)?;
        let faces: Vec<[u32; 3]> = tris.as_chunks::<3>().0.to_vec();
        let cmap = match (&color, &colormap) {
            (Some(_), None) => None,
            _ => plotui_bind::parse_colormap(Some(colormap.as_deref().unwrap_or("viridis")))
                .map_err(to_js)?,
        };
        let c = resolve_color(&self.inner, color.as_deref())?;
        Ok(self.inner.add_mesh3d(verts, faces, c, cmap, name))
    }

    /// Style a 2D scatter point by point: `colors` as shorthand strings,
    /// `sizes` as radii, `shapes` as silhouette names. An empty or omitted
    /// list leaves that channel uniform.
    pub fn set_point_styles(
        &mut self,
        handle: usize,
        colors: Option<Vec<String>>,
        sizes: Option<Vec<f32>>,
        shapes: Option<Vec<String>>,
    ) -> Result<(), JsError> {
        let colors = colors.unwrap_or_default();
        let colors: Vec<plotui_core::Rgb> = colors
            .iter()
            .map(|c| plotui_bind::parse_color(c))
            .collect::<Result<_, _>>()
            .map_err(to_js)?;
        let shapes = shapes.unwrap_or_default();
        let shapes: Vec<&str> = shapes.iter().map(String::as_str).collect();
        plotui_bind::set_point_styles(
            &mut self.inner,
            handle,
            colors,
            sizes.unwrap_or_default(),
            shapes,
        )
        .map_err(to_js)
    }

    /// Add a 2D scatter series on `axis` "y" (default), "y2" or "y3".
    pub fn add_scatter2d(
        &mut self,
        xs: &[f32],
        ys: &[f32],
        color: Option<String>,
        size: Option<f32>,
        name: Option<String>,
        axis: Option<String>,
    ) -> Result<usize, JsError> {
        let c = resolve_color(&self.inner, color.as_deref())?;
        let a = parse_axis(axis)?;
        Ok(self.inner.add_scatter2d(xs.to_vec(), ys.to_vec(), c, size.unwrap_or(2.5), name, a))
    }

    /// Add a 2D line series.
    pub fn add_line2d(
        &mut self,
        xs: &[f32],
        ys: &[f32],
        color: Option<String>,
        width: Option<f32>,
        name: Option<String>,
        axis: Option<String>,
    ) -> Result<usize, JsError> {
        let c = resolve_color(&self.inner, color.as_deref())?;
        let a = parse_axis(axis)?;
        Ok(self.inner.add_line2d(xs.to_vec(), ys.to_vec(), c, width.unwrap_or(2.0), name, a))
    }

    /// Add a box plot over a flat sample: `groupStarts[g]` is where group
    /// `g` begins in `values` (CSR).
    pub fn add_box2d(
        &mut self,
        values: &[f32],
        group_starts: &[u32],
        color: Option<String>,
        orientation: Option<String>,
        name: Option<String>,
        axis: Option<String>,
    ) -> Result<usize, JsError> {
        plotui_bind::check_group_starts(values.len(), group_starts).map_err(to_js)?;
        let orient = plotui_bind::parse_orient(orientation.as_deref().unwrap_or("vertical"))
            .map_err(to_js)?;
        let c = resolve_color(&self.inner, color.as_deref())?;
        let a = parse_axis(axis)?;
        Ok(self.inner.add_box2d(values.to_vec(), group_starts.to_vec(), c, orient, name, a))
    }

    /// Add a filled band between two boundaries at each x. Add it before the
    /// line it belongs to — draw order is the only layering in 2D.
    pub fn add_band2d(
        &mut self,
        xs: &[f32],
        lo: &[f32],
        hi: &[f32],
        color: Option<String>,
        name: Option<String>,
        axis: Option<String>,
    ) -> Result<usize, JsError> {
        let c = resolve_color(&self.inner, color.as_deref())?;
        let a = parse_axis(axis)?;
        Ok(self.inner.add_band2d(xs.to_vec(), lo.to_vec(), hi.to_vec(), c, name, a))
    }

    /// Attach per-point error bars to a 2D scatter or line; empty arrays
    /// clear that axis, and an empty `minus` mirrors `plus`.
    pub fn set_error_bars(
        &mut self,
        handle: usize,
        y_plus: Option<Vec<f32>>,
        y_minus: Option<Vec<f32>>,
        x_plus: Option<Vec<f32>>,
        x_minus: Option<Vec<f32>>,
    ) -> Result<(), JsError> {
        let ey = plotui_bind::error_bars(y_plus.unwrap_or_default(), y_minus.unwrap_or_default());
        let ex = plotui_bind::error_bars(x_plus.unwrap_or_default(), x_minus.unwrap_or_default());
        plotui_bind::set_error_bars(&mut self.inner, handle, ex, ey).map_err(to_js)
    }

    /// Add a heatmap over a flat row-major grid: `zs[j * xs.len() + i]` is
    /// the value at (xs[i], ys[j]).
    pub fn add_heatmap2d(
        &mut self,
        xs: &[f32],
        ys: &[f32],
        zs: &[f32],
        colormap: Option<String>,
        colorbar: Option<bool>,
        label: Option<String>,
        name: Option<String>,
    ) -> Result<usize, JsError> {
        plotui_bind::check_surface_grid_len(xs.len(), ys.len(), zs.len()).map_err(to_js)?;
        let cm = plotui_bind::parse_colormap(Some(colormap.as_deref().unwrap_or("viridis")))
            .map_err(to_js)?
            .expect("a named colormap always resolves");
        let h = self.inner.add_heatmap2d(xs.to_vec(), ys.to_vec(), zs.to_vec(), cm, name);
        if colorbar.unwrap_or(true) {
            if let Some((lo, hi)) = self.inner.heatmap_range(h) {
                self.inner.colorbar = Some(plotui_core::Colorbar { map: cm, lo, hi, label });
            }
        }
        Ok(h)
    }

    /// Add a histogram of `values`; `bins` or `binWidth` (not both), or
    /// neither for the automatic rule.
    pub fn add_histogram2d(
        &mut self,
        values: &[f32],
        bins: Option<usize>,
        bin_width: Option<f64>,
        color: Option<String>,
        name: Option<String>,
        axis: Option<String>,
    ) -> Result<usize, JsError> {
        let spec = plotui_bind::parse_bins(bins, bin_width).map_err(to_js)?;
        let c = resolve_color(&self.inner, color.as_deref())?;
        let a = parse_axis(axis)?;
        Ok(self.inner.add_histogram2d(values.to_vec(), spec, c, name, a))
    }

    /// Append observations to a histogram and rebin.
    pub fn extend_values(&mut self, handle: usize, values: &[f32]) -> Result<(), JsError> {
        plotui_bind::extend_values(&mut self.inner, handle, values).map_err(to_js)
    }

    /// Add a 2D step series; `where_` is "post" (default), "pre" or "mid".
    pub fn add_step2d(
        &mut self,
        xs: &[f32],
        ys: &[f32],
        color: Option<String>,
        width: Option<f32>,
        where_: Option<String>,
        name: Option<String>,
        axis: Option<String>,
    ) -> Result<usize, JsError> {
        let c = resolve_color(&self.inner, color.as_deref())?;
        let a = parse_axis(axis)?;
        let interp =
            plotui_bind::parse_interp(where_.as_deref().unwrap_or("post")).map_err(to_js)?;
        Ok(self.inner.add_step2d(
            xs.to_vec(),
            ys.to_vec(),
            c,
            width.unwrap_or(2.0),
            interp,
            name,
            a,
        ))
    }

    /// Add a 2D bar series.
    pub fn add_bar2d(
        &mut self,
        xs: &[f32],
        heights: &[f32],
        color: Option<String>,
        name: Option<String>,
        axis: Option<String>,
    ) -> Result<usize, JsError> {
        self.add_bar2d_oriented(xs, heights, color, None, name, axis)
    }

    /// Add a 2D bar series with an explicit orientation ("vertical" or
    /// "horizontal"). A horizontal bar reads `xs` as y positions.
    pub fn add_bar2d_oriented(
        &mut self,
        xs: &[f32],
        heights: &[f32],
        color: Option<String>,
        orientation: Option<String>,
        name: Option<String>,
        axis: Option<String>,
    ) -> Result<usize, JsError> {
        let c = resolve_color(&self.inner, color.as_deref())?;
        let a = parse_axis(axis)?;
        let o = plotui_bind::parse_orient(orientation.as_deref().unwrap_or("vertical"))
            .map_err(to_js)?;
        Ok(self.inner.add_bar2d_oriented(xs.to_vec(), heights.to_vec(), c, o, name, a))
    }

    /// Set how bar series share positions: "overlay", "group" or "stack".
    pub fn set_barmode(&mut self, mode: &str) -> Result<bool, JsError> {
        plotui_bind::set_barmode(&mut self.inner, mode).map_err(to_js)
    }

    /// Name an axis's categories ("x" or "y"); an empty list restores
    /// numeric ticks.
    pub fn set_categories(&mut self, axis: &str, names: Vec<String>) -> Result<bool, JsError> {
        plotui_bind::set_categories(&mut self.inner, axis, names).map_err(to_js)
    }

    /// Append points to a 2D trace by handle.
    pub fn extend_xy(&mut self, handle: usize, xs: &[f32], ys: &[f32]) -> Result<(), JsError> {
        plotui_bind::extend(&mut self.inner, handle, xs, ys, None).map_err(to_js)
    }

    /// Append points to a 3D scatter/line trace by handle.
    pub fn extend_xyz(
        &mut self,
        handle: usize,
        xs: &[f32],
        ys: &[f32],
        zs: &[f32],
    ) -> Result<(), JsError> {
        plotui_bind::extend(&mut self.inner, handle, xs, ys, Some(zs)).map_err(to_js)
    }

    /// Show or hide a trace; returns whether visibility changed.
    pub fn set_visible(&mut self, handle: usize, visible: bool) -> Result<bool, JsError> {
        self.inner.set_visible(handle, visible).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Toggle a trace from the legend: the geometry goes but the row stays,
    /// greyed out, so a second click brings it back. Returns whether the
    /// trace is now shown. Pair with `legend_hit`; use `set_visible` instead
    /// to take a trace out of the plot entirely, legend row included.
    pub fn toggle_muted(&mut self, handle: usize) -> Result<bool, JsError> {
        self.inner.toggle_muted(handle).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Move every node of a graph trace at once — the per-frame call of a
    /// force-directed layout (pair with `ForceLayout`). The point count must
    /// match the trace's node count; structure, indices, hover, and
    /// selection stay valid.
    pub fn set_graph_positions(
        &mut self,
        handle: usize,
        xs: &[f32],
        ys: &[f32],
        zs: &[f32],
    ) -> Result<(), JsError> {
        let pts = plotui_bind::zip3(xs, ys, zs);
        self.inner.set_graph_positions(handle, pts).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Replace a 2D graph's edge waypoints — the second half of a relayout,
    /// after `set_graph_positions` has moved the nodes. `route_pts` is
    /// interleaved x/y and `route_starts` one index per edge into them;
    /// passing both empty restores straight edges.
    pub fn set_graph_routes(
        &mut self,
        handle: usize,
        route_pts: &[f32],
        route_starts: Vec<u32>,
    ) -> Result<(), JsError> {
        if !route_pts.len().is_multiple_of(2) {
            return Err(JsError::new("route_pts must be flat [x, y] pairs; got an odd length"));
        }
        let pts: Vec<[f32; 2]> = route_pts.as_chunks::<2>().0.to_vec();
        self.inner
            .set_graph_routes(handle, pts, route_starts)
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Recolor a graph trace in place: one color shorthand per node, and
    /// optionally one per edge (`None` restores the default dimmed endpoint
    /// blend) — the host-side highlight primitive.
    pub fn set_graph_colors(
        &mut self,
        handle: usize,
        node_colors: Vec<String>,
        edge_colors: Option<Vec<String>>,
    ) -> Result<(), JsError> {
        let parse = |v: Vec<String>| -> Result<Vec<[u8; 3]>, JsError> {
            v.iter().map(|s| plotui_bind::parse_color(s).map_err(to_js)).collect()
        };
        let nc = parse(node_colors)?;
        let ec = edge_colors.map(parse).transpose()?;
        self.inner.set_graph_colors(handle, nc, ec).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Append nodes and edges to a graph trace (pair with
    /// `ForceLayout.add_node`): `edges` as flat `[a0, b0, a1, b1, …]` index
    /// pairs referencing old or new nodes; `node_colors` one shorthand per
    /// appended node (renderer default where missing).
    pub fn extend_graph(
        &mut self,
        handle: usize,
        xs: &[f32],
        ys: &[f32],
        zs: &[f32],
        node_colors: Option<Vec<String>>,
        edges: &[u32],
    ) -> Result<(), JsError> {
        if !edges.len().is_multiple_of(2) {
            return Err(JsError::new("edges must be flat [a, b] index pairs; got an odd length"));
        }
        let pts = plotui_bind::zip3(xs, ys, zs);
        let colors = match node_colors {
            Some(v) => v
                .iter()
                .map(|s| plotui_bind::parse_color(s).map_err(to_js))
                .collect::<Result<Vec<_>, _>>()?,
            None => Vec::new(),
        };
        let pairs: Vec<(u32, u32)> =
            edges.as_chunks::<2>().0.iter().map(|&[a, b]| (a, b)).collect();
        self.inner
            .extend_graph(handle, &pts, &colors, &pairs, None)
            .map_err(|e| JsError::new(&e.to_string()))
    }

    pub fn is_3d(&self) -> bool {
        self.inner.is_3d()
    }

    pub fn node_count(&self) -> usize {
        self.inner.node_count()
    }

    pub fn vertex_count(&self) -> usize {
        self.inner.vertex_count()
    }

    // ---- camera --------------------------------------------------------

    pub fn rotate(&mut self, d_yaw: f64, d_pitch: f64) {
        self.inner.camera.rotate(d_yaw, d_pitch);
    }

    pub fn zoom_by(&mut self, f: f64) {
        self.inner.camera.zoom_by(f);
    }

    /// Pan by a screen-pixel delta (full-resolution framebuffer pixels).
    pub fn pan(&mut self, dx: f64, dy: f64) {
        self.inner.camera.pan(dx, dy);
    }

    pub fn reset(&mut self) {
        self.inner.camera.reset();
    }

    /// Show or hide the 3D bounding-box wireframe.
    /// Draw the 2D chrome — grid, axis rules and tick labels — or not.
    /// `undefined` restores the automatic rule (a frame whose visible 2D
    /// traces are all graphs draws none of it, because a pipeline's
    /// coordinates are a layout rather than measurements); `true` and
    /// `false` pin it. The legend, colorbar, range slider and crosshair are
    /// unaffected either way, and 3D plots ignore it.
    pub fn set_show_axes(&mut self, show: Option<bool>) {
        self.inner.set_show_axes(show);
    }

    pub fn set_show_box(&mut self, show: bool) {
        self.inner.show_box = show;
    }

    /// Pin the 3D data frame to `(lo, hi)` corners so the view stops
    /// re-centering while data moves (a running force layout, streamed
    /// points). `clear_bounds` restores autoscale.
    pub fn set_bounds(&mut self, lo_x: f32, lo_y: f32, lo_z: f32, hi_x: f32, hi_y: f32, hi_z: f32) {
        self.inner.bounds_override = Some(([lo_x, lo_y, lo_z], [hi_x, hi_y, hi_z]));
    }

    /// Restore the autoscaled 3D frame.
    pub fn clear_bounds(&mut self) {
        self.inner.bounds_override = None;
    }

    /// Remap what drag gestures do. Each argument names the camera control
    /// that gesture axis drives — "yaw", "pitch", "pan_x", "pan_y", "zoom"
    /// or "off", optionally prefixed with "-" to invert the axis — or
    /// `None` to keep its current binding. The default map is drag = rotate
    /// as a trackball (yaw/pitch, the drag grabs the object), shift-drag =
    /// pan; "-yaw"/"-pitch" restore camera-grab rotation.
    pub fn set_input_map(
        &mut self,
        drag_x: Option<String>,
        drag_y: Option<String>,
        shift_drag_x: Option<String>,
        shift_drag_y: Option<String>,
    ) -> Result<(), JsError> {
        let mut m = self.inner.input_map;
        for (slot, inv, name) in [
            (&mut m.drag_x, &mut m.invert_drag_x, drag_x),
            (&mut m.drag_y, &mut m.invert_drag_y, drag_y),
            (&mut m.shift_drag_x, &mut m.invert_shift_drag_x, shift_drag_x),
            (&mut m.shift_drag_y, &mut m.invert_shift_drag_y, shift_drag_y),
        ] {
            if let Some(name) = name {
                (*slot, *inv) = plotui_bind::parse_camera_control(&name).map_err(to_js)?;
            }
        }
        self.inner.input_map = m;
        Ok(())
    }

    /// Route a drag through the input map: `(dx, dy)` pointer deltas,
    /// `shift` for the modifier, then the sensitivities — radians per unit,
    /// framebuffer pixels per unit, log-zoom per unit.
    pub fn apply_drag(
        &mut self,
        dx: f64,
        dy: f64,
        shift: bool,
        rotate_scale: f64,
        pan_scale: f64,
        zoom_scale: f64,
    ) {
        let scales = plotui_core::DragScales {
            rotate: rotate_scale,
            pan_x: pan_scale,
            pan_y: pan_scale,
            zoom: zoom_scale,
        };
        self.inner.apply_drag(dx, dy, shift, scales);
    }

    /// One auto-rotate step: `step` radians of yaw, turned the way a
    /// rightward drag pushes the object, so a scene that spins on its own
    /// and a user who grabs it agree. Negative `step` drifts the other way.
    /// Prefer this to `rotate` for an idle spin — `rotate` takes a raw
    /// camera delta, whose sign is the opposite one.
    pub fn spin(&mut self, step: f64) {
        self.inner.spin(step);
    }

    /// `[yaw, pitch, zoom, pan_x, pan_y]` — pass back to `set_camera_state`.
    pub fn camera_state(&self) -> Vec<f64> {
        let (yaw, pitch, zoom, pan_x, pan_y) = self.inner.camera.state();
        vec![yaw, pitch, zoom, pan_x, pan_y]
    }

    pub fn set_camera_state(&mut self, state: &[f64]) -> Result<(), JsError> {
        let [yaw, pitch, zoom, pan_x, pan_y] = *state else {
            return Err(JsError::new("camera state must be [yaw, pitch, zoom, pan_x, pan_y]"));
        };
        self.inner.camera.set_state(yaw, pitch, zoom, pan_x, pan_y);
        Ok(())
    }

    // ---- render ---------------------------------------------------------

    /// Rasterize at `w`×`h` device pixels into the internal frame buffer;
    /// read it with a fresh `Uint8ClampedArray(memory.buffer, frame_ptr(),
    /// frame_len())` per blit.
    pub fn render(&mut self, w: usize, h: usize) {
        self.frame = self.inner.render(w, h).rgba();
    }

    /// Reduced-resolution render for interaction: `pan_scale` = rendered
    /// width / full-resolution width, so a panned view stays centered.
    pub fn render_at(&mut self, w: usize, h: usize, pan_scale: f64) {
        self.frame = self.inner.render_at(w, h, pan_scale).rgba();
    }

    /// Just the legend, at full resolution, on a transparent frame. Composite
    /// it over an upscaled `render_at` frame (with `drawImage`, which honours
    /// alpha — `putImageData` would not) so a half-res drag does not change
    /// the legend under the pointer.
    pub fn render_legend_overlay(&mut self, w: usize, h: usize) {
        self.frame = self.inner.render_legend_overlay(w, h).rgba();
    }

    pub fn frame_ptr(&self) -> *const u8 {
        self.frame.as_ptr()
    }

    pub fn frame_len(&self) -> usize {
        self.frame.len()
    }

    // ---- pick / hover ---------------------------------------------------

    /// The trace whose legend row covers `(px, py)`, if any. Hidden traces
    /// keep their row, so this is the hook for a click-to-toggle legend.
    pub fn legend_hit(&self, w: usize, h: usize, px: f32, py: f32) -> Option<usize> {
        self.inner.legend_hit(w, h, px, py)
    }

    /// The 3D node under `(px, py)` framebuffer pixels, within `radius`.
    /// Picks always use full-resolution geometry regardless of `render_at`.
    pub fn pick(&self, w: usize, h: usize, px: f32, py: f32, radius: f32) -> Option<usize> {
        self.inner.pick(w, h, px, py, radius)
    }

    /// The node or edge under `(px, py)`, nodes first; edge radius defaults
    /// to 0.75 × `node_radius`.
    pub fn pick_element(
        &self,
        w: usize,
        h: usize,
        px: f32,
        py: f32,
        node_radius: f32,
        edge_radius: Option<f32>,
    ) -> Option<PickHit> {
        plotui_bind::pick_element_px(&self.inner, w, h, px, py, node_radius, edge_radius).map(
            |el| match el {
                Element::Node(i) => PickHit { is_edge: false, index: i },
                Element::Edge(i) => PickHit { is_edge: true, index: i },
            },
        )
    }

    /// Highlight a node as hovered (`None` clears); returns whether a
    /// repaint is needed.
    pub fn set_hovered_node(&mut self, index: Option<usize>) -> bool {
        plotui_bind::set_hovered(&mut self.inner, index.map(Element::Node))
    }

    pub fn set_hovered_edge(&mut self, index: Option<usize>) -> bool {
        plotui_bind::set_hovered(&mut self.inner, index.map(Element::Edge))
    }

    /// Mark a node selected (drawn with a glow; `None` clears).
    pub fn set_selected_node(&mut self, index: Option<usize>) -> bool {
        let el = index.map(Element::Node);
        let changed = self.inner.selected != el;
        self.inner.selected = el;
        changed
    }

    /// 2D crosshair: the hovered x in framebuffer pixels (`None` clears);
    /// core snaps to the nearest sample and draws the readout.
    pub fn set_hover2d(&mut self, x_px: Option<f32>) -> bool {
        plotui_bind::set_hover2d(&mut self.inner, x_px)
    }

    /// Set the explicit 2D x window in data coordinates; returns whether a
    /// repaint is needed. Requires finite `lo < hi`.
    pub fn set_x_window(&mut self, lo: f64, hi: f64) -> Result<bool, JsError> {
        plotui_bind::set_x_window(&mut self.inner, Some((lo, hi))).map_err(to_js)
    }

    /// Clear the x window (back to full-extent autoscale).
    pub fn clear_x_window(&mut self) -> bool {
        plotui_bind::set_x_window(&mut self.inner, None).unwrap_or(false)
    }

    /// The current x window as `[lo, hi]`, or `None`.
    pub fn x_window(&self) -> Option<Vec<f64>> {
        self.inner.x_window.map(|(lo, hi)| vec![lo, hi])
    }

    /// Toggle the range-slider strip; returns whether a repaint is needed.
    pub fn set_range_slider(&mut self, on: bool) -> bool {
        plotui_bind::set_range_slider(&mut self.inner, on)
    }

    /// Time axis: x values are seconds since this UTC epoch base (`None`
    /// clears); x ticks become calendar dates.
    pub fn set_x_epoch(&mut self, epoch: Option<f64>) -> Result<bool, JsError> {
        plotui_bind::set_x_epoch(&mut self.inner, epoch).map_err(to_js)
    }

    /// What the range-slider strip has under `(px, py)` framebuffer pixels,
    /// within `tol_px`: `"left"`, `"right"`, `"window"`, `"track"`, or
    /// `undefined` off the strip.
    pub fn range_slider_hit(
        &self,
        w: usize,
        h: usize,
        px: f32,
        py: f32,
        tol_px: f32,
    ) -> Option<String> {
        self.inner
            .range_slider_hit(w, h, px, py, tol_px)
            .map(|hit| plotui_bind::range_hit_to_parts(hit).to_string())
    }

    /// Drag the grabbed strip `part` (a `range_slider_hit` string) by
    /// `dx_px` framebuffer pixels; returns whether a repaint is needed.
    pub fn drag_x_window(
        &mut self,
        w: usize,
        h: usize,
        part: &str,
        dx_px: f32,
    ) -> Result<bool, JsError> {
        let hit = plotui_bind::range_hit_from_parts(part).map_err(to_js)?;
        Ok(self.inner.drag_x_window(w, h, hit, dx_px))
    }

    /// Center the window on the strip position under `px` (a track click).
    pub fn jump_x_window(&mut self, w: usize, h: usize, px: f32) -> bool {
        self.inner.jump_x_window(w, h, px)
    }

    /// Slide a set window by a plot-area drag of `dx_px` framebuffer pixels.
    pub fn pan_x_window(&mut self, w: usize, h: usize, dx_px: f32) -> bool {
        self.inner.pan_x_window(w, h, dx_px)
    }

    /// Zoom the window about the data x under `px` (`factor > 1` zooms in).
    pub fn zoom_x_window(&mut self, w: usize, h: usize, px: f32, factor: f64) -> bool {
        self.inner.zoom_x_window(w, h, px, factor)
    }

    /// Slide a set window by `frac` of its own span (positive = later x) —
    /// the keyboard step.
    pub fn shift_x_window(&mut self, frac: f64) -> bool {
        self.inner.shift_x_window(frac)
    }

    /// The surface-grid vertex under `(px, py)` framebuffer pixels, within
    /// `radius`: `[x, y, z, x_px, y_px]` (data coordinates, then projected
    /// screen position), or `None`. Surfaces are not part of node picking,
    /// so hover tooltips over a surface use this instead of `pick`.
    pub fn pick_surface(
        &self,
        w: usize,
        h: usize,
        px: f32,
        py: f32,
        radius: f32,
    ) -> Option<Vec<f32>> {
        self.inner
            .pick_surface(w, h, px, py, radius)
            .map(|hit| vec![hit.data[0], hit.data[1], hit.data[2], hit.screen[0], hit.screen[1]])
    }

    /// Hover a surface point — pass a `pick_surface` hit's `[x, y, z]` (extra
    /// elements are ignored), or `None` to clear. The engine then draws the
    /// hover guides: a ring at the point, its floor shadow, axis-parallel
    /// guide lines, and the drop line. Returns whether a repaint is needed.
    pub fn set_surface_hover(&mut self, xyz: Option<Vec<f32>>) -> Result<bool, JsError> {
        let p = match xyz {
            None => None,
            Some(v) if v.len() >= 3 => Some([v[0], v[1], v[2]]),
            Some(_) => return Err(JsError::new("surface hover must be [x, y, z]")),
        };
        Ok(self.inner.set_surface_hover(p))
    }

    /// Pin a surface point (click counterpart of `set_surface_hover`): the
    /// guides stay drawn with the selection treatment until cleared with
    /// `None`. Returns whether a repaint is needed.
    pub fn set_surface_selected(&mut self, xyz: Option<Vec<f32>>) -> Result<bool, JsError> {
        let p = match xyz {
            None => None,
            Some(v) if v.len() >= 3 => Some([v[0], v[1], v[2]]),
            Some(_) => return Err(JsError::new("surface selection must be [x, y, z]")),
        };
        Ok(self.inner.set_surface_selected(p))
    }

    /// Project a data-space point with the exact projection `render` uses:
    /// `[x_px, y_px, depth]` — anchor a pinned tooltip with this after
    /// camera changes.
    pub fn project_point(&self, w: usize, h: usize, x: f32, y: f32, z: f32) -> Vec<f32> {
        self.inner.project_point(w, h, [x, y, z]).to_vec()
    }

    /// Projected node positions as flat `[x_px, y_px, depth]` triples, in
    /// the same flat order `pick` uses.
    pub fn project_nodes(&self, w: usize, h: usize) -> Vec<f32> {
        self.inner.project_nodes(w, h).into_iter().flatten().collect()
    }
}

impl Default for Plot {
    fn default() -> Self {
        Self::new()
    }
}

/// A 3D force-directed layout: connected nodes attract, all nodes repel, a
/// cooling temperature settles the motion. Pure math on the host's timer —
/// call `step()` per animation frame and hand `positions()` to
/// `Plot.set_graph_positions`. Deterministic for a given seed.
#[wasm_bindgen]
pub struct ForceLayout {
    inner: plotui_core::ForceLayout,
}

#[wasm_bindgen]
impl ForceLayout {
    /// A layout over `n_nodes` with seeded initial positions in the unit
    /// ball. `edges` are flat `[a0, b0, a1, b1, …]` index pairs.
    #[wasm_bindgen(constructor)]
    pub fn new(n_nodes: usize, edges: &[u32], seed: u32) -> Result<ForceLayout, JsError> {
        if !edges.len().is_multiple_of(2) {
            return Err(JsError::new("edges must be flat [a, b] index pairs; got an odd length"));
        }
        let pairs: Vec<(u32, u32)> =
            edges.as_chunks::<2>().0.iter().map(|&[a, b]| (a, b)).collect();
        Ok(ForceLayout { inner: plotui_core::ForceLayout::new(n_nodes, &pairs, seed) })
    }

    /// One simulation tick. Returns the mean displacement — stop repainting
    /// once it drops below ~1e-3.
    pub fn step(&mut self) -> f32 {
        self.inner.step()
    }

    /// Current node positions as a flat `[x0, y0, z0, x1, …]` array, in
    /// index order.
    pub fn positions(&self) -> Vec<f32> {
        self.inner.positions().iter().flatten().copied().collect()
    }

    /// Warm insertion of one node connected to `neighbors` (existing
    /// indices); returns the new node's index. Pair with
    /// `Plot.extend_graph`.
    pub fn add_node(&mut self, neighbors: &[u32]) -> usize {
        self.inner.add_node(neighbors)
    }
}

/// A hierarchical ("Sugiyama") layout for a directed graph: rank the nodes
/// by depth, order each rank to reduce edge crossings, then place them so
/// edges run as straight as they can. Solved in the constructor — there is
/// nothing to step, because a pipeline has one right shape.
///
/// Feed `positions()`, `route_pts()` and `route_starts()` straight to
/// `Plot.add_graph2d`. Deterministic: same input, same output, no
/// randomness anywhere.
#[wasm_bindgen]
pub struct LayeredLayout {
    inner: plotui_core::LayeredLayout,
}

#[wasm_bindgen]
impl LayeredLayout {
    /// Lay out `n_nodes` connected by `edges` (flat `[a0, b0, a1, b1, …]`
    /// index pairs), flowing in `rankdir` — "TB" (sources on top, the
    /// default) or "LR" (sources on the left). Self-loops and out-of-range
    /// endpoints are inert, and cycles do not hang: a back edge is reversed
    /// for the layout only.
    #[wasm_bindgen(constructor)]
    pub fn new(
        n_nodes: usize,
        edges: &[u32],
        rankdir: Option<String>,
    ) -> Result<LayeredLayout, JsError> {
        if !edges.len().is_multiple_of(2) {
            return Err(JsError::new("edges must be flat [a, b] index pairs; got an odd length"));
        }
        let dir = match rankdir.as_deref() {
            None => plotui_core::RankDir::TB,
            Some(s) => plotui_bind::parse_rankdir(s).map_err(to_js)?,
        };
        let pairs: Vec<(u32, u32)> =
            edges.as_chunks::<2>().0.iter().map(|&[a, b]| (a, b)).collect();
        Ok(LayeredLayout { inner: plotui_core::LayeredLayout::new(n_nodes, &pairs, dir) })
    }

    /// Node centres as a flat `[x0, y0, x1, …]` array, in index order.
    pub fn positions(&self) -> Vec<f32> {
        self.inner.positions().iter().flatten().copied().collect()
    }

    /// Each node's rank: 0 for a source, one more than its deepest
    /// predecessor otherwise.
    pub fn ranks(&self) -> Vec<u32> {
        self.inner.ranks().to_vec()
    }

    /// Edge waypoints as a flat `[x0, y0, x1, …]` array — the first half of
    /// the CSR pair `Plot.add_graph2d` takes.
    pub fn route_pts(&self) -> Vec<f32> {
        self.inner.routes().0.iter().flatten().copied().collect()
    }

    /// One index per edge into `route_pts()`, in the caller's edge order —
    /// the second half of the CSR pair. Edge `e` owns
    /// `route_starts()[e]..route_starts()[e + 1]`, and an empty run is a
    /// straight edge.
    pub fn route_starts(&self) -> Vec<u32> {
        self.inner.routes().1.to_vec()
    }
}

/// Parse a DOT document, lay it out, and return a ready-to-render plot
/// whose graph trace is handle 0. `rankdir` overrides the document's own
/// ("TB" or "LR"); `undefined` honours whatever it says.
///
/// The accepted grammar is a subset: node and edge statements, chains
/// (`a -> b -> c`), braced fan-outs (`a -> {b c}`), `subgraph`s (contents
/// hoisted, grouping ignored), attribute defaults, `rankdir`, and `label` /
/// `color` / `fillcolor` / `shape` / `style=rounded` on nodes with `color`
/// on edges. Unknown attributes are ignored; HTML labels, node ports and a
/// mismatched edge operator throw with the line and column.
#[wasm_bindgen]
pub fn plot_from_dot(text: &str, rankdir: Option<String>) -> Result<Plot, JsError> {
    let dir = match rankdir.as_deref() {
        None => None,
        Some(s) => Some(plotui_bind::parse_rankdir(s).map_err(to_js)?),
    };
    let (plot, _, _) = plotui_bind::plot_from_dot(text, dir).map_err(to_js)?;
    Ok(Plot::from_core(plot))
}

/// Which nodes are reachable from node `i` — everything it waits on with
/// `upstream` true (the default), everything it leads to otherwise —
/// including `i` itself. Returns one byte per node, 1 where reachable.
///
/// This is the primitive behind "hover a task and light everything upstream
/// of it": pair it with `Plot.set_graph_colors`.
#[wasm_bindgen]
pub fn reachable(
    n_nodes: usize,
    edges: &[u32],
    i: usize,
    upstream: Option<bool>,
) -> Result<Vec<u8>, JsError> {
    if !edges.len().is_multiple_of(2) {
        return Err(JsError::new("edges must be flat [a, b] index pairs; got an odd length"));
    }
    let pairs: Vec<(u32, u32)> = edges.as_chunks::<2>().0.iter().map(|&[a, b]| (a, b)).collect();
    Ok(plotui_bind::reachable(n_nodes, &pairs, i, upstream.unwrap_or(true))
        .into_iter()
        .map(u8::from)
        .collect())
}

/// Generated geometry: vertex coordinates split per axis (the shape
/// `Plot.add_mesh3d` takes) plus the flat `[a0, b0, c0, a1, …]` triangle
/// indices that join them. Returned by `marching_cubes`, `tube` and
/// `ribbon`.
#[wasm_bindgen]
pub struct Mesh {
    xs: Vec<f32>,
    ys: Vec<f32>,
    zs: Vec<f32>,
    tris: Vec<u32>,
}

impl Mesh {
    fn new(verts: Vec<[f32; 3]>, tris: Vec<[u32; 3]>) -> Mesh {
        Mesh {
            xs: verts.iter().map(|v| v[0]).collect(),
            ys: verts.iter().map(|v| v[1]).collect(),
            zs: verts.iter().map(|v| v[2]).collect(),
            tris: tris.into_iter().flatten().collect(),
        }
    }
}

/// A flat `[x0, y0, z0, x1, …]` list as points. Paths and direction fields
/// cross this boundary flat because that is the shape a sweep consumes;
/// meshes come back split per axis because that is what `add_mesh3d` takes.
fn points(what: &str, flat: &[f32]) -> Result<Vec<[f32; 3]>, JsError> {
    let (pts, rest) = flat.as_chunks::<3>();
    if !rest.is_empty() {
        return Err(JsError::new(&format!("{what} must be a flat [x, y, z, ...] list")));
    }
    Ok(pts.to_vec())
}

#[wasm_bindgen]
impl Mesh {
    pub fn xs(&self) -> Vec<f32> {
        self.xs.clone()
    }

    pub fn ys(&self) -> Vec<f32> {
        self.ys.clone()
    }

    pub fn zs(&self) -> Vec<f32> {
        self.zs.clone()
    }

    pub fn tris(&self) -> Vec<u32> {
        self.tris.clone()
    }
}

/// Polygonise a sampled scalar field: the `field == iso` surface of
/// `values[(k * ny + j) * nx + i]`, sampled at `origin + [i, j, k] * cell`.
///
/// The marching-cubes tables live only in Rust, so a browser scene and
/// `plotui example mandelbulb` agree triangle for triangle. Vertices are
/// shared between neighbouring cells, which is what lets `add_mesh3d`
/// shade the result smoothly.
#[wasm_bindgen]
pub fn marching_cubes(
    values: &[f32],
    nx: usize,
    ny: usize,
    nz: usize,
    origin: &[f32],
    cell: f32,
    iso: f32,
) -> Result<Mesh, JsError> {
    let [ox, oy, oz] = *origin else {
        return Err(JsError::new("origin must be [x, y, z]"));
    };
    let (verts, tris) = plotui_core::marching_cubes(values, nx, ny, nz, [ox, oy, oz], cell, iso);
    Ok(Mesh::new(verts, tris))
}

/// Resample a coarse polyline through a uniform Catmull-Rom spline:
/// `per_segment` samples for each input segment plus the final point. The
/// curve passes through every input point. Returns the same flat
/// `[x, y, z, …]` shape it takes, ready to hand to `tube` or `ribbon`.
///
/// The spline lives only in Rust, so a browser scene and `plotui example
/// protein` sweep the same curve.
#[wasm_bindgen]
pub fn catmull_rom(path: &[f32], per_segment: usize) -> Result<Vec<f32>, JsError> {
    let pts = points("path", path)?;
    Ok(plotui_core::catmull_rom(&pts, per_segment).into_iter().flatten().collect())
}

/// Sweep a circular cross-section of `radii` along `path`, `sides` facets
/// around, capped at both ends.
///
/// `radii[i]` is the radius at point `i`; a shorter array repeats its last
/// entry, so a one-element array is a constant radius and a per-point one
/// tapers. The frame carried along the path is rotation-minimizing, so the
/// section never spins about its own axis where the path twists.
#[wasm_bindgen]
pub fn tube(path: &[f32], radii: &[f32], sides: usize) -> Result<Mesh, JsError> {
    let pts = points("path", path)?;
    let (verts, tris) = plotui_core::tube(&pts, radii, sides);
    Ok(Mesh::new(verts, tris))
}

/// Sweep a flat rectangular cross-section along `path`: `widths` across the
/// face, `thickness` through it.
///
/// `up` is the flat `[x, y, z, …]` face normal per point — the direction the
/// flat of the ribbon points. It is re-squared against the tangent, so it
/// need only be approximate; an empty array falls back to the same
/// rotation-minimizing frame `tube` uses. `widths` is indexed like `tube`'s
/// `radii`, so tapering the last entries to zero gives an arrowhead.
#[wasm_bindgen]
pub fn ribbon(path: &[f32], up: &[f32], widths: &[f32], thickness: f32) -> Result<Mesh, JsError> {
    let pts = points("path", path)?;
    let ups = points("up", up)?;
    let (verts, tris) = plotui_core::ribbon(&pts, &ups, widths, thickness);
    Ok(Mesh::new(verts, tris))
}
