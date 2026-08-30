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

#[wasm_bindgen]
impl Plot {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Plot {
        Plot { inner: plotui_core::Plot::new(), frame: Vec::new() }
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

    /// Add a 2D bar series.
    pub fn add_bar2d(
        &mut self,
        xs: &[f32],
        heights: &[f32],
        color: Option<String>,
        name: Option<String>,
        axis: Option<String>,
    ) -> Result<usize, JsError> {
        let c = resolve_color(&self.inner, color.as_deref())?;
        let a = parse_axis(axis)?;
        Ok(self.inner.add_bar2d(xs.to_vec(), heights.to_vec(), c, name, a))
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
            .extend_graph(handle, &pts, &colors, &pairs)
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
    /// or "off" — or `None` to keep its current binding. The default map is
    /// drag = rotate (yaw/pitch), shift-drag = pan.
    pub fn set_input_map(
        &mut self,
        drag_x: Option<String>,
        drag_y: Option<String>,
        shift_drag_x: Option<String>,
        shift_drag_y: Option<String>,
    ) -> Result<(), JsError> {
        let mut m = self.inner.input_map;
        for (slot, name) in [
            (&mut m.drag_x, drag_x),
            (&mut m.drag_y, drag_y),
            (&mut m.shift_drag_x, shift_drag_x),
            (&mut m.shift_drag_y, shift_drag_y),
        ] {
            if let Some(name) = name {
                *slot = plotui_bind::parse_camera_control(&name).map_err(to_js)?;
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

    pub fn frame_ptr(&self) -> *const u8 {
        self.frame.as_ptr()
    }

    pub fn frame_len(&self) -> usize {
        self.frame.len()
    }

    // ---- pick / hover ---------------------------------------------------

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
