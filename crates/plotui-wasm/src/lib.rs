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

/// A color from JS: `"#rrggbb"` (or bare `"rrggbb"`), or `None` for the
/// binding's default — the next palette slot for most traces.
fn parse_color(color: Option<&str>) -> Result<Option<[u8; 3]>, JsError> {
    let Some(s) = color else { return Ok(None) };
    let hex = s.strip_prefix('#').unwrap_or(s);
    let bad = || JsError::new(&format!("color must be \"#rrggbb\", got {s:?}"));
    if hex.len() != 6 || !hex.is_ascii() {
        return Err(bad());
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| bad());
    Ok(Some([byte(0)?, byte(2)?, byte(4)?]))
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

    // ---- traces -------------------------------------------------------

    /// Add a 3D scatter series; returns the trace handle for
    /// `extend_xyz`/`set_visible`.
    pub fn add_scatter3d(
        &mut self,
        xs: &[f32],
        ys: &[f32],
        zs: &[f32],
        color: Option<String>,
        size: Option<f32>,
    ) -> Result<usize, JsError> {
        let c = resolve_color(&self.inner, color.as_deref())?;
        let pts = plotui_bind::zip3(xs, ys, zs);
        Ok(self.inner.add_scatter3d(pts, c, size.unwrap_or(3.0)))
    }

    /// Add a 3D graph: nodes at `xs/ys/zs`, `edges` as flat index pairs
    /// `[a0, b0, a1, b1, …]`, a uniform node `color`, and marker `size`.
    pub fn add_graph3d(
        &mut self,
        xs: &[f32],
        ys: &[f32],
        zs: &[f32],
        edges: &[u32],
        color: Option<String>,
        size: Option<f32>,
    ) -> Result<usize, JsError> {
        if edges.len() % 2 != 0 {
            return Err(JsError::new("edges must be flat [a, b] index pairs; got an odd length"));
        }
        let c = resolve_color(&self.inner, color.as_deref())?;
        let nodes = plotui_bind::zip3(xs, ys, zs);
        let colors = plotui_bind::graph_node_colors(nodes.len(), None, c);
        let pairs = edges.as_chunks::<2>().0.iter().map(|&[a, b]| (a, b)).collect();
        Ok(self.inner.add_graph3d(nodes, colors, pairs, size.unwrap_or(3.5), None, None, None))
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
