//! plotui-bind — the binding-layer semantics every language binding shares.
//!
//! The PyO3 bindings and the C ABI both sit between a loosely-typed caller
//! and the strongly-typed core. What they share is not marshaling but
//! *semantics*: how ragged inputs pair up, which strings name axes and
//! shapes, what an omitted color means, how `extend` dispatches on trace
//! kind — and the exact error messages for getting any of it wrong. This
//! crate is that single home, so Python and Go callers see identical
//! behavior down to the error text.

use plotui_core::{Colormap, Element, Plot, RangeHit, Rgb, Shape, Trace, YAxis};

/// Default edge pick radius as a fraction of the node pick radius (the
/// `pick_element_px` default every binding applies).
pub const EDGE_RADIUS_FACTOR: f32 = 0.75;

/// How a binding call went wrong — the receiving binding maps `kind` to its
/// error convention (Python: always `ValueError`; C: a status code) and
/// surfaces `msg` verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindError {
    pub kind: BindErrorKind,
    pub msg: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindErrorKind {
    /// A malformed argument (bad enum string, wrong grid shape, …).
    InvalidArg,
    /// A trace handle that names no trace.
    UnknownHandle,
    /// A structurally immutable trace (graph, surface).
    Structural,
}

impl BindError {
    fn invalid(msg: String) -> Self {
        Self { kind: BindErrorKind::InvalidArg, msg }
    }
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.msg)
    }
}

impl std::error::Error for BindError {}

/// Zip three coordinate sequences into points, truncated to the shortest —
/// the pairing rule every 3D `add_*` and `extend` shares.
pub fn zip3(xs: &[f32], ys: &[f32], zs: &[f32]) -> Vec<[f32; 3]> {
    let n = xs.len().min(ys.len()).min(zs.len());
    (0..n).map(|i| [xs[i], ys[i], zs[i]]).collect()
}

/// The y scale a 2D series binds to: `"y"`, `"y2"`, or `"y3"`.
pub fn parse_axis(axis: &str) -> Result<YAxis, BindError> {
    match axis {
        "y" => Ok(YAxis::Primary),
        "y2" => Ok(YAxis::Y2),
        "y3" => Ok(YAxis::Y3),
        _ => Err(BindError::invalid(format!("axis must be 'y', 'y2' or 'y3', got {axis:?}"))),
    }
}

/// Per-node marker shapes by name; an unknown name is an error.
pub fn parse_shapes<S: AsRef<str>>(names: &[S]) -> Result<Vec<Shape>, BindError> {
    names
        .iter()
        .map(|name| {
            let name = name.as_ref();
            Shape::parse(name).ok_or_else(|| {
                BindError::invalid(format!(
                    "unknown node shape {name:?}; expected one of {}",
                    Shape::NAMES.join(", ")
                ))
            })
        })
        .collect()
}

/// Color-name shorthands, CSS values, shared by every binding. Sorted, so
/// the error message enumerates them stably.
pub const COLOR_NAMES: [(&str, Rgb); 18] = [
    ("black", [0, 0, 0]),
    ("blue", [0, 0, 255]),
    ("brown", [165, 42, 42]),
    ("cyan", [0, 255, 255]),
    ("gray", [128, 128, 128]),
    ("green", [0, 128, 0]),
    ("grey", [128, 128, 128]),
    ("lime", [0, 255, 0]),
    ("magenta", [255, 0, 255]),
    ("navy", [0, 0, 128]),
    ("orange", [255, 165, 0]),
    ("pink", [255, 192, 203]),
    ("purple", [128, 0, 128]),
    ("red", [255, 0, 0]),
    ("teal", [0, 128, 128]),
    ("violet", [238, 130, 238]),
    ("white", [255, 255, 255]),
    ("yellow", [255, 255, 0]),
];

/// A color shorthand: `"#rrggbb"` (or bare `"rrggbb"`) hex, or a name from
/// [`COLOR_NAMES`]. The shared rule — and the exact error message — every
/// binding applies to a string color.
pub fn parse_color(s: &str) -> Result<Rgb, BindError> {
    if let Some((_, rgb)) = COLOR_NAMES.iter().find(|(n, _)| *n == s) {
        return Ok(*rgb);
    }
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap();
        return Ok([byte(0), byte(2), byte(4)]);
    }
    Err(BindError::invalid(format!(
        "unknown color {s:?}; expected \"#rrggbb\" hex or one of {}",
        COLOR_NAMES.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")
    )))
}

/// A built-in colorway by name; an unknown name is an error.
pub fn colorway(name: &str) -> Result<&'static [Rgb; 8], BindError> {
    plotui_core::colorway_by_name(name).ok_or_else(|| {
        BindError::invalid(format!(
            "unknown colorway {name:?}; expected one of plotui, muted, vivid",
        ))
    })
}

/// The shared rule for a custom colorway list: it must not be empty.
pub fn check_colorway(colors: &[Rgb]) -> Result<(), BindError> {
    if colors.is_empty() {
        return Err(BindError::invalid("colorway must contain at least one color".to_string()));
    }
    Ok(())
}

/// A colormap by name; an unknown name is an error (`None` means a solid
/// color, so it maps through).
pub fn parse_colormap(name: Option<&str>) -> Result<Option<Colormap>, BindError> {
    match name {
        None => Ok(None),
        Some(name) => Colormap::parse(name).map(Some).ok_or_else(|| {
            BindError::invalid(format!(
                "unknown colormap {name:?}; expected one of {}, or None for a solid color",
                Colormap::NAMES.join(", ")
            ))
        }),
    }
}

/// A camera control by name, for [`plotui_core::InputMap`] remapping — the
/// shared rule every binding applies to gesture-map strings.
pub fn parse_camera_control(name: &str) -> Result<plotui_core::CameraControl, BindError> {
    use plotui_core::CameraControl as C;
    match name {
        "yaw" => Ok(C::Yaw),
        "pitch" => Ok(C::Pitch),
        "pan_x" => Ok(C::PanX),
        "pan_y" => Ok(C::PanY),
        "zoom" => Ok(C::Zoom),
        "off" => Ok(C::Off),
        _ => Err(BindError::invalid(format!(
            "camera control must be 'yaw', 'pitch', 'pan_x', 'pan_y', 'zoom' or 'off', got {name:?}"
        ))),
    }
}

/// Validate a nested surface grid (`zs[j][i]` = height at `(xs[i], ys[j])`)
/// and flatten it row-major for the core.
pub fn flatten_surface_grid(
    nx: usize,
    ny: usize,
    zs: Vec<Vec<f32>>,
) -> Result<Vec<f32>, BindError> {
    if zs.len() != ny || zs.iter().any(|row| row.len() != nx) {
        return Err(BindError::invalid(format!(
            "zs must be a {ny}×{nx} grid (len(ys) rows of len(xs) heights); got {} rows of {:?}",
            zs.len(),
            zs.iter().map(Vec::len).take(4).collect::<Vec<_>>(),
        )));
    }
    Ok(zs.into_iter().flatten().collect())
}

/// Validate an already-flat surface grid's length (the C-ABI form of
/// [`flatten_surface_grid`]).
pub fn check_surface_grid_len(nx: usize, ny: usize, len: usize) -> Result<(), BindError> {
    if len != nx * ny {
        return Err(BindError::invalid(format!(
            "zs must be a {ny}×{nx} grid (len(ys) rows of len(xs) heights); got {len} heights",
        )));
    }
    Ok(())
}

/// The graph node-color rule: one color per node, padding or truncating a
/// partial `node_colors` with the uniform fallback.
pub fn graph_node_colors(n: usize, node_colors: Option<Vec<Rgb>>, uniform: Rgb) -> Vec<Rgb> {
    match node_colors {
        Some(c) => (0..n).map(|i| c.get(i).copied().unwrap_or(uniform)).collect(),
        None => vec![uniform; n],
    }
}

/// An element from its string kind: `("node" | "edge", index)`.
pub fn element_from_parts(kind: &str, index: usize) -> Result<Element, BindError> {
    match kind {
        "node" => Ok(Element::Node(index)),
        "edge" => Ok(Element::Edge(index)),
        _ => {
            Err(BindError::invalid(format!("element kind must be 'node' or 'edge', got {kind:?}")))
        }
    }
}

/// An element's string kind and index.
pub fn element_to_parts(el: Element) -> (&'static str, usize) {
    match el {
        Element::Node(i) => ("node", i),
        Element::Edge(i) => ("edge", i),
    }
}

/// Set the hovered element with change detection; the returned bool tells
/// the frontend whether a repaint is needed.
pub fn set_hovered(plot: &mut Plot, element: Option<Element>) -> bool {
    let changed = plot.hovered != element;
    plot.hovered = element;
    changed
}

/// Set the 2D crosshair position (framebuffer px; `None` clears) with
/// change detection.
pub fn set_hover2d(plot: &mut Plot, px: Option<f32>) -> bool {
    let changed = plot.hover2d_px != px;
    plot.hover2d_px = px;
    changed
}

/// Set (or clear) the explicit 2D x window with validation and change
/// detection; the returned bool tells the frontend whether a repaint is
/// needed.
pub fn set_x_window(plot: &mut Plot, w: Option<(f64, f64)>) -> Result<bool, BindError> {
    if let Some((lo, hi)) = w {
        if !lo.is_finite() || !hi.is_finite() || lo >= hi {
            return Err(BindError::invalid(format!(
                "x_window needs finite lo < hi, got ({lo}, {hi})"
            )));
        }
    }
    let changed = plot.x_window != w;
    plot.x_window = w;
    Ok(changed)
}

/// Toggle the range-slider strip with change detection.
pub fn set_range_slider(plot: &mut Plot, on: bool) -> bool {
    let changed = plot.range_slider != on;
    plot.range_slider = on;
    changed
}

/// Set (or clear) the time-axis epoch base (seconds, UTC) with validation
/// and change detection.
pub fn set_x_epoch(plot: &mut Plot, epoch: Option<f64>) -> Result<bool, BindError> {
    if let Some(e) = epoch {
        if !e.is_finite() {
            return Err(BindError::invalid(format!("x_epoch must be finite, got {e}")));
        }
    }
    let changed = plot.x_epoch != epoch;
    plot.x_epoch = epoch;
    Ok(changed)
}

/// A range-slider hit's string kind, for bindings whose hosts speak strings.
pub fn range_hit_to_parts(hit: RangeHit) -> &'static str {
    match hit {
        RangeHit::LeftHandle => "left",
        RangeHit::RightHandle => "right",
        RangeHit::Window => "window",
        RangeHit::Track => "track",
    }
}

/// A range-slider hit from its string kind.
pub fn range_hit_from_parts(kind: &str) -> Result<RangeHit, BindError> {
    match kind {
        "left" => Ok(RangeHit::LeftHandle),
        "right" => Ok(RangeHit::RightHandle),
        "window" => Ok(RangeHit::Window),
        "track" => Ok(RangeHit::Track),
        _ => Err(BindError::invalid(format!(
            "range part must be 'left', 'right', 'window' or 'track', got {kind:?}"
        ))),
    }
}

/// Pick under `(px, py)`, nodes before edges, with the shared default edge
/// radius when the caller passes `None`.
pub fn pick_element_px(
    plot: &Plot,
    px_w: usize,
    px_h: usize,
    px: f32,
    py: f32,
    node_radius: f32,
    edge_radius: Option<f32>,
) -> Option<Element> {
    let er = edge_radius.unwrap_or(node_radius * EDGE_RADIUS_FACTOR);
    plot.pick_element(px_w, px_h, px, py, node_radius, er)
}

/// Append points to a trace by handle: `(xs, ys)` for 2D traces, `(xs, ys,
/// zs)` for 3D scatter/line traces — dispatched on the trace's actual kind,
/// with the canonical error for every mismatch.
pub fn extend(
    plot: &mut Plot,
    handle: usize,
    xs: &[f32],
    ys: &[f32],
    zs: Option<&[f32]>,
) -> Result<(), BindError> {
    enum Kind {
        D2,
        D3,
    }
    let kind = match plot.traces.get(handle) {
        None => {
            return Err(BindError {
                kind: BindErrorKind::UnknownHandle,
                msg: format!("unknown trace handle {handle}"),
            });
        }
        Some(Trace::Graph3d { .. }) => {
            return Err(BindError {
                kind: BindErrorKind::Structural,
                msg: "graph3d traces are structural (edges reference node indices); \
                     rebuild the plot to change them"
                    .into(),
            });
        }
        Some(Trace::Surface3d { .. }) => {
            return Err(BindError {
                kind: BindErrorKind::Structural,
                msg: "surface3d traces are structural (a fixed grid); \
                     rebuild the plot to change them"
                    .into(),
            });
        }
        Some(Trace::Scatter3d { .. } | Trace::Line3d { .. }) => Kind::D3,
        Some(_) => Kind::D2,
    };
    let result = match (kind, zs) {
        (Kind::D2, Some(_)) => {
            return Err(BindError::invalid(
                "2D trace: extend takes (xs, ys) — zs is for 3D traces".into(),
            ));
        }
        (Kind::D3, None) => {
            return Err(BindError::invalid("3D trace: extend needs xs, ys and zs".into()));
        }
        (Kind::D2, None) => plot.extend_xy(handle, xs, ys),
        (Kind::D3, Some(zs)) => plot.extend_pts(handle, &zip3(xs, ys, zs)),
    };
    result.map_err(|e| BindError { kind: BindErrorKind::Structural, msg: e.to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zip3_truncates_to_the_shortest() {
        assert_eq!(
            zip3(&[1.0, 2.0, 3.0], &[4.0, 5.0], &[6.0, 7.0, 8.0]),
            vec![[1.0, 4.0, 6.0], [2.0, 5.0, 7.0]]
        );
    }

    #[test]
    fn color_shorthands_parse_names_and_hex() {
        assert_eq!(parse_color("red").unwrap(), [255, 0, 0]);
        assert_eq!(parse_color("grey").unwrap(), parse_color("gray").unwrap());
        assert_eq!(parse_color("#e63c78").unwrap(), [230, 60, 120]);
        assert_eq!(parse_color("45C8D1").unwrap(), [69, 200, 209]);
        assert_eq!(colorway("plotui").unwrap()[0], [230, 60, 120]);
    }

    #[test]
    fn parse_errors_carry_the_canonical_messages() {
        assert_eq!(parse_axis("y4").unwrap_err().msg, "axis must be 'y', 'y2' or 'y3', got \"y4\"");
        assert!(parse_shapes(&["blob"]).unwrap_err().msg.starts_with("unknown node shape"));
        assert!(parse_colormap(Some("heat")).unwrap_err().msg.starts_with("unknown colormap"));
        assert!(parse_color("blurple").unwrap_err().msg.starts_with("unknown color"));
        assert!(colorway("neon").unwrap_err().msg.starts_with("unknown colorway"));
        assert_eq!(
            check_colorway(&[]).unwrap_err().msg,
            "colorway must contain at least one color"
        );
        assert_eq!(
            element_from_parts("face", 0).unwrap_err().msg,
            "element kind must be 'node' or 'edge', got \"face\""
        );
    }

    #[test]
    fn surface_grid_validation() {
        assert!(flatten_surface_grid(2, 2, vec![vec![1.0, 2.0], vec![3.0, 4.0]]).is_ok());
        let err = flatten_surface_grid(2, 3, vec![vec![1.0, 2.0]]).unwrap_err();
        assert!(err.msg.starts_with("zs must be a 3×2 grid"));
        assert!(check_surface_grid_len(2, 3, 6).is_ok());
        assert!(check_surface_grid_len(2, 3, 5).is_err());
    }

    #[test]
    fn extend_dispatches_on_trace_kind() {
        let mut p = Plot::new();
        let h2 = p.add_line2d(vec![0.0], vec![0.0], [1, 2, 3], 1.0, None, YAxis::Primary);
        let h3 = p.add_scatter3d(vec![[0.0; 3]], [1, 2, 3], 1.0, None);
        let hg =
            p.add_graph3d(vec![[0.0; 3]], vec![[1, 2, 3]], vec![], 1.0, None, None, None, None);

        assert!(extend(&mut p, h2, &[1.0], &[1.0], None).is_ok());
        assert!(extend(&mut p, h3, &[1.0], &[1.0], Some(&[1.0])).is_ok());
        assert_eq!(
            extend(&mut p, h2, &[1.0], &[1.0], Some(&[1.0])).unwrap_err().msg,
            "2D trace: extend takes (xs, ys) — zs is for 3D traces"
        );
        assert_eq!(
            extend(&mut p, h3, &[1.0], &[1.0], None).unwrap_err().msg,
            "3D trace: extend needs xs, ys and zs"
        );
        let err = extend(&mut p, hg, &[1.0], &[1.0], None).unwrap_err();
        assert_eq!(err.kind, BindErrorKind::Structural);
        assert!(err.msg.starts_with("graph3d traces are structural"));
        let err = extend(&mut p, 99, &[1.0], &[1.0], None).unwrap_err();
        assert_eq!(err.kind, BindErrorKind::UnknownHandle);
        assert_eq!(err.msg, "unknown trace handle 99");
    }

    #[test]
    fn hover_helpers_detect_change() {
        let mut p = Plot::new();
        assert!(set_hovered(&mut p, Some(Element::Node(1))));
        assert!(!set_hovered(&mut p, Some(Element::Node(1))));
        assert!(set_hover2d(&mut p, Some(4.0)));
        assert!(!set_hover2d(&mut p, Some(4.0)));
    }

    #[test]
    fn x_window_helpers_detect_change_and_validate() {
        let mut p = Plot::new();
        assert!(set_x_window(&mut p, Some((0.0, 10.0))).unwrap());
        assert!(!set_x_window(&mut p, Some((0.0, 10.0))).unwrap());
        assert!(set_x_window(&mut p, None).unwrap());
        assert_eq!(
            set_x_window(&mut p, Some((5.0, 5.0))).unwrap_err().to_string(),
            "x_window needs finite lo < hi, got (5, 5)"
        );
        assert!(set_x_window(&mut p, Some((f64::NAN, 1.0))).is_err());
        assert!(set_range_slider(&mut p, true));
        assert!(!set_range_slider(&mut p, true));
        assert!(set_x_epoch(&mut p, Some(1.7e9)).unwrap());
        assert!(!set_x_epoch(&mut p, Some(1.7e9)).unwrap());
        assert_eq!(
            set_x_epoch(&mut p, Some(f64::INFINITY)).unwrap_err().to_string(),
            "x_epoch must be finite, got inf"
        );
    }

    #[test]
    fn range_hit_parts_roundtrip() {
        for hit in [RangeHit::LeftHandle, RangeHit::RightHandle, RangeHit::Window, RangeHit::Track]
        {
            assert_eq!(range_hit_from_parts(range_hit_to_parts(hit)).unwrap(), hit);
        }
        assert_eq!(
            range_hit_from_parts("middle").unwrap_err().to_string(),
            "range part must be 'left', 'right', 'window' or 'track', got \"middle\""
        );
    }
}
