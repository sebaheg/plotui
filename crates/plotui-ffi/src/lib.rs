//! plotui-ffi — the C ABI over the plotui engine.
//!
//! Mirrors the PyO3 binding surface function-for-function, with the shared
//! semantics (defaults, validation, error text) coming from `plotui-bind`
//! and the terminal glue from `plotui-term`, so Python and C callers see
//! identical behavior.
//!
//! Conventions (documented in include/plotui.h, which cbindgen generates
//! from this file — see tests/header.rs):
//! - A plot is an opaque `PlotuiPlot*` from `plotui_new`, freed with
//!   `plotui_free`. Not thread-safe: one thread at a time.
//! - Fallible functions return a `PlotuiStatus` (`PLOTUI_OK` = 0); the
//!   message is `plotui_last_error()` — a thread-local pointer valid until
//!   the next failing call on that thread.
//! - Input pointers are borrowed for the call only. Optional values are
//!   NULL pointers (colors are 3-byte RGB arrays); arrays are (ptr, len)
//!   with independent per-axis lengths, min-truncated like every binding.
//! - Returned strings are freed with `plotui_string_free`; pixel/float
//!   output buffers are caller-allocated.

use std::cell::RefCell;
use std::ffi::{c_char, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;

use plotui_bind::{BindError, BindErrorKind};
use plotui_core::{Element, Plot, RangeHit, Rgb};
use plotui_term::policy::{active_scale, scaled_dims};

pub const PLOTUI_OK: i32 = 0;
pub const PLOTUI_ERR_INVALID_ARG: i32 = 1;
pub const PLOTUI_ERR_UNKNOWN_HANDLE: i32 = 2;
pub const PLOTUI_ERR_STRUCTURAL: i32 = 3;
pub const PLOTUI_ERR_NULL: i32 = 4;

/// Above this 3D vertex count, render at reduced resolution while
/// interacting (see `plotui_interactive_scale`).
pub const PLOTUI_LARGE_VERTEX_COUNT: usize = plotui_term::policy::LARGE_VERTEX_COUNT;

/// An opaque plot handle: data + camera + this plot's Kitty image id.
pub struct PlotuiPlot {
    plot: Plot,
    image_id: u32,
}

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

fn set_error(msg: &str) {
    let sanitized = msg.replace('\0', " ");
    LAST_ERROR.with(|e| *e.borrow_mut() = CString::new(sanitized).unwrap_or_default());
}

fn bind_status(e: BindError) -> i32 {
    set_error(&e.msg);
    match e.kind {
        BindErrorKind::InvalidArg => PLOTUI_ERR_INVALID_ARG,
        BindErrorKind::UnknownHandle => PLOTUI_ERR_UNKNOWN_HANDLE,
        BindErrorKind::Structural => PLOTUI_ERR_STRUCTURAL,
    }
}

/// Run a fallible export body with panic containment (unwinding across
/// `extern "C"` is undefined behavior).
fn guard(f: impl FnOnce() -> i32) -> i32 {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(status) => status,
        Err(payload) => {
            let msg = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "internal panic".into());
            set_error(&format!("internal panic: {msg}"));
            PLOTUI_ERR_INVALID_ARG
        }
    }
}

/// # Safety
/// The standard (ptr, len) contract: null with len 0 is an empty slice.
unsafe fn slice<'a, T>(ptr: *const T, len: usize) -> Result<&'a [T], i32> {
    if ptr.is_null() {
        if len == 0 {
            return Ok(&[]);
        }
        set_error("null pointer with nonzero length");
        return Err(PLOTUI_ERR_NULL);
    }
    Ok(std::slice::from_raw_parts(ptr, len))
}

unsafe fn opt_rgb(ptr: *const u8) -> Option<Rgb> {
    if ptr.is_null() {
        None
    } else {
        let s = std::slice::from_raw_parts(ptr, 3);
        Some([s[0], s[1], s[2]])
    }
}

unsafe fn opt_str<'a>(ptr: *const c_char) -> Result<Option<&'a str>, i32> {
    if ptr.is_null() {
        return Ok(None);
    }
    match CStr::from_ptr(ptr).to_str() {
        Ok(s) => Ok(Some(s)),
        Err(_) => {
            set_error("string argument is not valid UTF-8");
            Err(PLOTUI_ERR_INVALID_ARG)
        }
    }
}

unsafe fn plot_ref<'a>(p: *const PlotuiPlot) -> Result<&'a PlotuiPlot, i32> {
    p.as_ref().ok_or_else(|| {
        set_error("null plot handle");
        PLOTUI_ERR_NULL
    })
}

unsafe fn plot_mut<'a>(p: *mut PlotuiPlot) -> Result<&'a mut PlotuiPlot, i32> {
    p.as_mut().ok_or_else(|| {
        set_error("null plot handle");
        PLOTUI_ERR_NULL
    })
}

fn out_string(s: String, out: *mut *mut c_char) -> i32 {
    if out.is_null() {
        set_error("null output pointer");
        return PLOTUI_ERR_NULL;
    }
    let c = CString::new(s.replace('\0', "")).unwrap_or_default();
    unsafe { *out = c.into_raw() };
    PLOTUI_OK
}

fn axis_of(axis: Option<&str>) -> Result<plotui_core::YAxis, BindError> {
    plotui_bind::parse_axis(axis.unwrap_or("y"))
}

// ---- lifecycle & errors ----

/// A fresh plot with its own Kitty image id (the first plot in a process
/// gets the protocol's default id, 4242).
#[no_mangle]
pub extern "C" fn plotui_new() -> *mut PlotuiPlot {
    new_plot_from(Plot::new())
}

/// Wrap an already-built plot in a fresh handle with its own image id — the
/// composers (`plotui_plot_from_dot`) hand back a whole plot rather than
/// filling one in.
fn new_plot_from(plot: Plot) -> *mut PlotuiPlot {
    Box::into_raw(Box::new(PlotuiPlot { plot, image_id: plotui_term::next_image_id() }))
}

/// Free a plot. NULL is a no-op.
///
/// # Safety
/// `p` must be a pointer from `plotui_new` not yet freed.
#[no_mangle]
pub unsafe extern "C" fn plotui_free(p: *mut PlotuiPlot) {
    if !p.is_null() {
        drop(Box::from_raw(p));
    }
}

/// The message for the last failing call on this thread ("" when none).
/// Borrowed: valid until the next failing call on the same thread.
#[no_mangle]
pub extern "C" fn plotui_last_error() -> *const c_char {
    LAST_ERROR.with(|e| e.borrow().as_ptr())
}

/// Free a string returned by this library. NULL is a no-op.
///
/// # Safety
/// `s` must be a string returned by this library, not yet freed.
#[no_mangle]
pub unsafe extern "C" fn plotui_string_free(s: *mut c_char) {
    if !s.is_null() {
        drop(CString::from_raw(s));
    }
}

/// Parse a color shorthand — "#rrggbb" (or bare "rrggbb") hex, or a name
/// like "red" — into 3 bytes at `out_rgb`. Stateless; the accepted names
/// and the error message are the shared `plotui-bind` rule.
///
/// # Safety
/// `s` must be a NUL-terminated string; `out_rgb` must point at 3 writable
/// bytes.
#[no_mangle]
pub unsafe extern "C" fn plotui_parse_color(s: *const c_char, out_rgb: *mut u8) -> i32 {
    guard(|| {
        let s = match opt_str(s) {
            Ok(Some(s)) => s,
            Ok(None) => {
                set_error("null color string");
                return PLOTUI_ERR_NULL;
            }
            Err(status) => return status,
        };
        let rgb = match plotui_bind::parse_color(s) {
            Ok(rgb) => rgb,
            Err(e) => return bind_status(e),
        };
        if out_rgb.is_null() {
            set_error("null out_rgb");
            return PLOTUI_ERR_NULL;
        }
        std::ptr::copy_nonoverlapping(rgb.as_ptr(), out_rgb, 3);
        PLOTUI_OK
    })
}

/// Swap the color sequence assigned to traces added without an explicit
/// color: `rgbs` is `3 * n` bytes of (r, g, b) triples; `n` must be at
/// least 1. Traces already added keep their colors.
///
/// # Safety
/// Pointer arguments follow the crate conventions.
#[no_mangle]
pub unsafe extern "C" fn plotui_set_colorway(p: *mut PlotuiPlot, rgbs: *const u8, n: usize) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        let colors: Vec<[u8; 3]> = match slice(rgbs, n * 3) {
            Ok(bytes) => bytes.as_chunks::<3>().0.to_vec(),
            Err(s) => return s,
        };
        if let Err(e) = plotui_bind::check_colorway(&colors) {
            return bind_status(e);
        }
        p.plot.set_colorway(colors);
        PLOTUI_OK
    })
}

/// Swap the color sequence to a built-in colorway by name: "plotui" (the
/// default), "muted", or "vivid".
///
/// # Safety
/// Pointer arguments follow the crate conventions.
#[no_mangle]
pub unsafe extern "C" fn plotui_set_colorway_name(p: *mut PlotuiPlot, name: *const c_char) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        let name = match opt_str(name) {
            Ok(Some(s)) => s,
            Ok(None) => {
                set_error("null colorway name");
                return PLOTUI_ERR_NULL;
            }
            Err(status) => return status,
        };
        let colors = match plotui_bind::colorway(name) {
            Ok(c) => c.to_vec(),
            Err(e) => return bind_status(e),
        };
        p.plot.set_colorway(colors);
        PLOTUI_OK
    })
}

// ---- traces ----

/// # Safety
/// Pointer arguments follow the crate conventions (see the module docs).
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn plotui_add_scatter3d(
    p: *mut PlotuiPlot,
    xs: *const f32,
    nx: usize,
    ys: *const f32,
    ny: usize,
    zs: *const f32,
    nz: usize,
    rgb: *const u8,
    size: f32,
    name: *const c_char,
    out_handle: *mut usize,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        let (xs, ys, zs) = match (slice(xs, nx), slice(ys, ny), slice(zs, nz)) {
            (Ok(a), Ok(b), Ok(c)) => (a, b, c),
            (Err(s), ..) | (_, Err(s), _) | (.., Err(s)) => return s,
        };
        let name = match opt_str(name) {
            Ok(n) => n.map(str::to_string),
            Err(s) => return s,
        };
        let pts = plotui_bind::zip3(xs, ys, zs);
        let color = p.plot.resolve_color(opt_rgb(rgb));
        let h = p.plot.add_scatter3d(pts, color, size, name);
        if !out_handle.is_null() {
            *out_handle = h;
        }
        PLOTUI_OK
    })
}

/// Read an array of NUL-terminated strings into owned `String`s. Shared by
/// the label and shape arguments, which are the two places a binding hands
/// the C ABI a list of names.
unsafe fn str_array<'a>(
    ptr: *const *const c_char,
    n: usize,
    what: &str,
) -> Result<Vec<&'a str>, i32> {
    let ptrs = slice(ptr, n)?;
    let mut out = Vec::with_capacity(ptrs.len());
    for &sp in ptrs {
        match opt_str(sp) {
            Ok(Some(s)) => out.push(s),
            Ok(None) => {
                set_error(&format!("null {what}"));
                return Err(PLOTUI_ERR_NULL);
            }
            Err(s) => return Err(s),
        }
    }
    Ok(out)
}

/// Add a 2D directed graph: labelled boxes at `(xs, ys)`, wired by `edges`.
///
/// `labels` is an array of NUL-terminated strings, one per node (an empty
/// string draws an unlabelled box); `node_shapes` likewise, taking the names
/// `rounded`, `box`, `ellipse` and `diamond` plus DOT's synonyms.
/// `route_pts` is `2 * n_route_pts` floats as interleaved x/y waypoints and
/// `route_starts` is one u32 per edge indexing into them (CSR) — what
/// `plotui_layered_layout_routes` writes out. Pass NULL for any of them to
/// take the default.
///
/// # Safety
/// Pointer arguments follow the crate conventions. `edges` is `2 * n_edges`
/// u32s as (i, j) pairs; `node_rgbs`/`edge_rgbs` are `3 * n` byte triples.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn plotui_add_graph2d(
    p: *mut PlotuiPlot,
    xs: *const f32,
    nx: usize,
    ys: *const f32,
    ny: usize,
    labels: *const *const c_char,
    n_labels: usize,
    edges: *const u32,
    n_edges: usize,
    directed: bool,
    node_rgbs: *const u8,
    n_node_rgbs: usize,
    rgb: *const u8,
    node_shapes: *const *const c_char,
    n_shapes: usize,
    edge_rgbs: *const u8,
    n_edge_rgbs: usize,
    route_pts: *const f32,
    n_route_pts: usize,
    route_starts: *const u32,
    n_route_starts: usize,
    name: *const c_char,
    out_handle: *mut usize,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        let (xs, ys) = match (slice(xs, nx), slice(ys, ny)) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(s), _) | (_, Err(s)) => return s,
        };
        let n = xs.len().min(ys.len());
        let nodes: Vec<[f32; 2]> = (0..n).map(|i| [xs[i], ys[i]]).collect();
        let labels = match str_array(labels, n_labels, "node label") {
            Ok(v) => {
                (0..n).map(|i| v.get(i).copied().unwrap_or("").to_string()).collect::<Vec<String>>()
            }
            Err(s) => return s,
        };
        let edge_list: Vec<(u32, u32)> = match slice(edges, n_edges * 2) {
            Ok(e) => e.as_chunks::<2>().0.iter().map(|&[a, b]| (a, b)).collect(),
            Err(s) => return s,
        };
        let uniform = p.plot.resolve_color(opt_rgb(rgb));
        let node_colors = match slice(node_rgbs, n_node_rgbs * 3) {
            Ok([]) => None,
            Ok(bytes) => Some(bytes.as_chunks::<3>().0.to_vec()),
            Err(s) => return s,
        };
        let colors = plotui_bind::graph_node_colors(n, node_colors, uniform);
        let edge_colors = match slice(edge_rgbs, n_edge_rgbs * 3) {
            Ok([]) => None,
            Ok(bytes) => Some(bytes.as_chunks::<3>().0.to_vec()),
            Err(s) => return s,
        };
        let shapes = match str_array(node_shapes, n_shapes, "node shape name") {
            Ok(v) if v.is_empty() => None,
            Ok(v) => match plotui_bind::parse_node_shapes(&v) {
                Ok(v) => Some(v),
                Err(e) => return bind_status(e),
            },
            Err(s) => return s,
        };
        let routes = match (slice(route_pts, n_route_pts * 2), slice(route_starts, n_route_starts))
        {
            (Ok([]), Ok([])) => None,
            (Ok(pts), Ok(starts)) => {
                let pts: Vec<[f32; 2]> = pts.as_chunks::<2>().0.to_vec();
                if let Err(e) = plotui_bind::check_routes(edge_list.len(), pts.len(), starts) {
                    return bind_status(e);
                }
                Some((pts, starts.to_vec()))
            }
            (Err(s), _) | (_, Err(s)) => return s,
        };
        let name = match opt_str(name) {
            Ok(n) => n.map(str::to_string),
            Err(s) => return s,
        };
        let h = p.plot.add_graph2d(
            nodes,
            labels,
            colors,
            edge_list,
            directed,
            shapes,
            edge_colors,
            routes,
            name,
        );
        if !out_handle.is_null() {
            *out_handle = h;
        }
        PLOTUI_OK
    })
}

/// # Safety
/// Pointer arguments follow the crate conventions. `edges` is `2 * n_edges`
/// u32s as (i, j) pairs; `node_rgbs`/`edge_rgbs` are `3 * n` byte triples;
/// `node_shapes` is an array of NUL-terminated shape names.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn plotui_add_graph3d(
    p: *mut PlotuiPlot,
    xs: *const f32,
    nx: usize,
    ys: *const f32,
    ny: usize,
    zs: *const f32,
    nz: usize,
    edges: *const u32,
    n_edges: usize,
    node_rgbs: *const u8,
    n_node_rgbs: usize,
    rgb: *const u8,
    size: f32,
    node_sizes: *const f32,
    n_node_sizes: usize,
    edge_rgbs: *const u8,
    n_edge_rgbs: usize,
    node_shapes: *const *const c_char,
    n_shapes: usize,
    name: *const c_char,
    out_handle: *mut usize,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        let (xs, ys, zs) = match (slice(xs, nx), slice(ys, ny), slice(zs, nz)) {
            (Ok(a), Ok(b), Ok(c)) => (a, b, c),
            (Err(s), ..) | (_, Err(s), _) | (.., Err(s)) => return s,
        };
        let nodes = plotui_bind::zip3(xs, ys, zs);
        let n = nodes.len();
        let edge_list = match slice(edges, n_edges * 2) {
            Ok(e) => e.as_chunks::<2>().0.iter().map(|&[a, b]| (a, b)).collect(),
            Err(s) => return s,
        };
        let uniform = p.plot.resolve_color(opt_rgb(rgb));
        let node_colors = match slice(node_rgbs, n_node_rgbs * 3) {
            Ok([]) => None,
            Ok(bytes) => Some(bytes.as_chunks::<3>().0.to_vec()),
            Err(s) => return s,
        };
        let colors = plotui_bind::graph_node_colors(n, node_colors, uniform);
        let sizes = match slice(node_sizes, n_node_sizes) {
            Ok([]) => None,
            Ok(s) => Some(s.to_vec()),
            Err(s) => return s,
        };
        let edge_colors = match slice(edge_rgbs, n_edge_rgbs * 3) {
            Ok([]) => None,
            Ok(bytes) => Some(bytes.as_chunks::<3>().0.to_vec()),
            Err(s) => return s,
        };
        let shapes = match str_array(node_shapes, n_shapes, "node shape name") {
            Ok(v) if v.is_empty() => None,
            Ok(v) => match plotui_bind::parse_shapes(&v) {
                Ok(v) => Some(v),
                Err(e) => return bind_status(e),
            },
            Err(s) => return s,
        };
        let name = match opt_str(name) {
            Ok(n) => n.map(str::to_string),
            Err(s) => return s,
        };
        let h =
            p.plot.add_graph3d(nodes, colors, edge_list, size, sizes, edge_colors, shapes, name);
        if !out_handle.is_null() {
            *out_handle = h;
        }
        PLOTUI_OK
    })
}

/// # Safety
/// Pointer arguments follow the crate conventions.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn plotui_add_line3d(
    p: *mut PlotuiPlot,
    xs: *const f32,
    nx: usize,
    ys: *const f32,
    ny: usize,
    zs: *const f32,
    nz: usize,
    rgb: *const u8,
    width: f32,
    name: *const c_char,
    out_handle: *mut usize,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        let (xs, ys, zs) = match (slice(xs, nx), slice(ys, ny), slice(zs, nz)) {
            (Ok(a), Ok(b), Ok(c)) => (a, b, c),
            (Err(s), ..) | (_, Err(s), _) | (.., Err(s)) => return s,
        };
        let name = match opt_str(name) {
            Ok(n) => n.map(str::to_string),
            Err(s) => return s,
        };
        let pts = plotui_bind::zip3(xs, ys, zs);
        let color = p.plot.resolve_color(opt_rgb(rgb));
        let h = p.plot.add_line3d(pts, color, width, name);
        if !out_handle.is_null() {
            *out_handle = h;
        }
        PLOTUI_OK
    })
}

/// `zs` is the flat row-major grid: `zs[j * nx + i]` = height at
/// `(xs[i], ys[j])`; `nz` must equal `nx * ny`. `colormap` NULL means a
/// solid color.
///
/// # Safety
/// Pointer arguments follow the crate conventions.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn plotui_add_surface3d(
    p: *mut PlotuiPlot,
    xs: *const f32,
    nx: usize,
    ys: *const f32,
    ny: usize,
    zs: *const f32,
    nz: usize,
    rgb: *const u8,
    colormap: *const c_char,
    wireframe: bool,
    name: *const c_char,
    out_handle: *mut usize,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        let (xs, ys, zs) = match (slice(xs, nx), slice(ys, ny), slice(zs, nz)) {
            (Ok(a), Ok(b), Ok(c)) => (a, b, c),
            (Err(s), ..) | (_, Err(s), _) | (.., Err(s)) => return s,
        };
        if let Err(e) = plotui_bind::check_surface_grid_len(nx, ny, nz) {
            return bind_status(e);
        }
        let cm = match opt_str(colormap) {
            Ok(name) => match plotui_bind::parse_colormap(name) {
                Ok(cm) => cm,
                Err(e) => return bind_status(e),
            },
            Err(s) => return s,
        };
        let name = match opt_str(name) {
            Ok(n) => n.map(str::to_string),
            Err(s) => return s,
        };
        let color = p.plot.resolve_color(opt_rgb(rgb));
        let h =
            p.plot.add_surface3d(xs.to_vec(), ys.to_vec(), zs.to_vec(), color, cm, wireframe, name);
        if !out_handle.is_null() {
            *out_handle = h;
        }
        PLOTUI_OK
    })
}
/// Vertices are `(xs[i], ys[i], zs[i])`, truncated to the shortest of the
/// three; `tris` is a flat run of `[a, b, c]` vertex-index triples, so
/// `ntris` must be a multiple of 3 and every index must name a vertex.
/// `colormap` NULL means a solid color.
///
/// # Safety
/// Pointer arguments follow the crate conventions.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn plotui_add_mesh3d(
    p: *mut PlotuiPlot,
    xs: *const f32,
    nx: usize,
    ys: *const f32,
    ny: usize,
    zs: *const f32,
    nz: usize,
    tris: *const u32,
    ntris: usize,
    rgb: *const u8,
    colormap: *const c_char,
    name: *const c_char,
    out_handle: *mut usize,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        let (xs, ys, zs) = match (slice(xs, nx), slice(ys, ny), slice(zs, nz)) {
            (Ok(a), Ok(b), Ok(c)) => (a, b, c),
            (Err(s), ..) | (_, Err(s), _) | (.., Err(s)) => return s,
        };
        let tris = match slice(tris, ntris) {
            Ok(t) => t,
            Err(s) => return s,
        };
        let verts = plotui_bind::zip3(xs, ys, zs);
        if let Err(e) = plotui_bind::check_mesh_indices(verts.len(), tris) {
            return bind_status(e);
        }
        let faces: Vec<[u32; 3]> = tris.as_chunks::<3>().0.to_vec();
        let cm = match opt_str(colormap) {
            Ok(name) => match plotui_bind::parse_colormap(name) {
                Ok(cm) => cm,
                Err(e) => return bind_status(e),
            },
            Err(s) => return s,
        };
        let name = match opt_str(name) {
            Ok(n) => n.map(str::to_string),
            Err(s) => return s,
        };
        let color = p.plot.resolve_color(opt_rgb(rgb));
        let h = p.plot.add_mesh3d(verts, faces, color, cm, name);
        if !out_handle.is_null() {
            *out_handle = h;
        }
        PLOTUI_OK
    })
}

#[allow(clippy::too_many_arguments)]
unsafe fn add_2d(
    p: *mut PlotuiPlot,
    xs: *const f32,
    nx: usize,
    ys: *const f32,
    ny: usize,
    rgb: *const u8,
    name: *const c_char,
    axis: *const c_char,
    out_handle: *mut usize,
    add: impl FnOnce(&mut Plot, Vec<f32>, Vec<f32>, Rgb, Option<String>, plotui_core::YAxis) -> usize,
) -> i32 {
    let p = match plot_mut(p) {
        Ok(p) => p,
        Err(s) => return s,
    };
    let (xs, ys) = match (slice(xs, nx), slice(ys, ny)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(s), _) | (_, Err(s)) => return s,
    };
    let name = match opt_str(name) {
        Ok(n) => n.map(str::to_string),
        Err(s) => return s,
    };
    let axis = match opt_str(axis) {
        Ok(a) => match axis_of(a) {
            Ok(a) => a,
            Err(e) => return bind_status(e),
        },
        Err(s) => return s,
    };
    let color = p.plot.resolve_color(opt_rgb(rgb));
    let h = add(&mut p.plot, xs.to_vec(), ys.to_vec(), color, name, axis);
    if !out_handle.is_null() {
        *out_handle = h;
    }
    PLOTUI_OK
}

/// # Safety
/// Pointer arguments follow the crate conventions. `axis` is "y", "y2" or
/// "y3" (NULL = "y").
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn plotui_add_scatter2d(
    p: *mut PlotuiPlot,
    xs: *const f32,
    nx: usize,
    ys: *const f32,
    ny: usize,
    rgb: *const u8,
    size: f32,
    name: *const c_char,
    axis: *const c_char,
    out_handle: *mut usize,
) -> i32 {
    guard(|| {
        add_2d(p, xs, nx, ys, ny, rgb, name, axis, out_handle, |plot, xs, ys, c, name, ax| {
            plot.add_scatter2d(xs, ys, c, size, name, ax)
        })
    })
}

/// # Safety
/// Pointer arguments follow the crate conventions.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn plotui_add_line2d(
    p: *mut PlotuiPlot,
    xs: *const f32,
    nx: usize,
    ys: *const f32,
    ny: usize,
    rgb: *const u8,
    width: f32,
    name: *const c_char,
    axis: *const c_char,
    out_handle: *mut usize,
) -> i32 {
    guard(|| {
        add_2d(p, xs, nx, ys, ny, rgb, name, axis, out_handle, |plot, xs, ys, c, name, ax| {
            plot.add_line2d(xs, ys, c, width, name, ax)
        })
    })
}

/// A box plot over a flat sample: `group_starts[g]` is where group `g` begins
/// in `values` (CSR — ascending, starting at 0, none past the end).
///
/// # Safety
/// Pointer arguments follow the crate conventions. `axis` is "y", "y2" or
/// "y3" (NULL = "y").
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn plotui_add_box2d(
    p: *mut PlotuiPlot,
    values: *const f32,
    n: usize,
    group_starts: *const u32,
    n_groups: usize,
    rgb: *const u8,
    orientation: *const c_char,
    name: *const c_char,
    axis: *const c_char,
    out_handle: *mut usize,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        let (values, starts) = match (slice(values, n), slice(group_starts, n_groups)) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(s), _) | (_, Err(s)) => return s,
        };
        if let Err(e) = plotui_bind::check_group_starts(values.len(), starts) {
            return bind_status(e);
        }
        let orient = match opt_str(orientation) {
            Ok(o) => match plotui_bind::parse_orient(o.unwrap_or("vertical")) {
                Ok(o) => o,
                Err(e) => return bind_status(e),
            },
            Err(s) => return s,
        };
        let name = match opt_str(name) {
            Ok(n) => n.map(str::to_string),
            Err(s) => return s,
        };
        let axis = match opt_str(axis) {
            Ok(a) => match axis_of(a) {
                Ok(a) => a,
                Err(e) => return bind_status(e),
            },
            Err(s) => return s,
        };
        let color = p.plot.resolve_color(opt_rgb(rgb));
        let h = p.plot.add_box2d(values.to_vec(), starts.to_vec(), color, orient, name, axis);
        if !out_handle.is_null() {
            *out_handle = h;
        }
        PLOTUI_OK
    })
}

/// A filled band between `lo` and `hi` at each x. Add it before the line it
/// belongs to — draw order is the only layering in 2D.
///
/// # Safety
/// Pointer arguments follow the crate conventions. `axis` is "y", "y2" or
/// "y3" (NULL = "y").
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn plotui_add_band2d(
    p: *mut PlotuiPlot,
    xs: *const f32,
    nx: usize,
    lo: *const f32,
    nlo: usize,
    hi: *const f32,
    nhi: usize,
    rgb: *const u8,
    name: *const c_char,
    axis: *const c_char,
    out_handle: *mut usize,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        let (xs, lo, hi) = match (slice(xs, nx), slice(lo, nlo), slice(hi, nhi)) {
            (Ok(a), Ok(b), Ok(c)) => (a, b, c),
            (Err(s), ..) | (_, Err(s), _) | (.., Err(s)) => return s,
        };
        let name = match opt_str(name) {
            Ok(n) => n.map(str::to_string),
            Err(s) => return s,
        };
        let axis = match opt_str(axis) {
            Ok(a) => match axis_of(a) {
                Ok(a) => a,
                Err(e) => return bind_status(e),
            },
            Err(s) => return s,
        };
        let color = p.plot.resolve_color(opt_rgb(rgb));
        let h = p.plot.add_band2d(xs.to_vec(), lo.to_vec(), hi.to_vec(), color, name, axis);
        if !out_handle.is_null() {
            *out_handle = h;
        }
        PLOTUI_OK
    })
}

/// Attach per-point error bars to a 2D scatter or line. A zero length (or
/// NULL) clears that axis; an empty `minus` mirrors `plus`.
///
/// # Safety
/// Pointer arguments follow the crate conventions.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn plotui_set_error_bars(
    p: *mut PlotuiPlot,
    handle: usize,
    y_plus: *const f32,
    n_yp: usize,
    y_minus: *const f32,
    n_ym: usize,
    x_plus: *const f32,
    n_xp: usize,
    x_minus: *const f32,
    n_xm: usize,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        let arrs =
            (slice(y_plus, n_yp), slice(y_minus, n_ym), slice(x_plus, n_xp), slice(x_minus, n_xm));
        let (yp, ym, xp, xm) = match arrs {
            (Ok(a), Ok(b), Ok(c), Ok(d)) => (a, b, c, d),
            (Err(s), ..) | (_, Err(s), ..) | (.., Err(s), _) | (.., Err(s)) => return s,
        };
        let ey = plotui_bind::error_bars(yp.to_vec(), ym.to_vec());
        let ex = plotui_bind::error_bars(xp.to_vec(), xm.to_vec());
        match plotui_bind::set_error_bars(&mut p.plot, handle, ex, ey) {
            Ok(()) => PLOTUI_OK,
            Err(e) => bind_status(e),
        }
    })
}

/// A heatmap over a flat row-major grid: `zs[j * nx + i]` is the value at
/// `(xs[i], ys[j])`; `nz` must equal `nx * ny`. `colormap` NULL means
/// "viridis". With `colorbar` true the plot's colorbar is set to this grid's
/// own value range.
///
/// # Safety
/// Pointer arguments follow the crate conventions.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn plotui_add_heatmap2d(
    p: *mut PlotuiPlot,
    xs: *const f32,
    nx: usize,
    ys: *const f32,
    ny: usize,
    zs: *const f32,
    nz: usize,
    colormap: *const c_char,
    colorbar: bool,
    label: *const c_char,
    name: *const c_char,
    out_handle: *mut usize,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        let (xs, ys, zs) = match (slice(xs, nx), slice(ys, ny), slice(zs, nz)) {
            (Ok(a), Ok(b), Ok(c)) => (a, b, c),
            (Err(s), ..) | (_, Err(s), _) | (.., Err(s)) => return s,
        };
        if let Err(e) = plotui_bind::check_surface_grid_len(nx, ny, nz) {
            return bind_status(e);
        }
        let cm = match opt_str(colormap) {
            Ok(n) => match plotui_bind::parse_colormap(Some(n.unwrap_or("viridis"))) {
                Ok(Some(cm)) => cm,
                Ok(None) => plotui_core::Colormap::Viridis,
                Err(e) => return bind_status(e),
            },
            Err(s) => return s,
        };
        let (label, name) = match (opt_str(label), opt_str(name)) {
            (Ok(l), Ok(n)) => (l.map(str::to_string), n.map(str::to_string)),
            (Err(s), _) | (_, Err(s)) => return s,
        };
        let h = p.plot.add_heatmap2d(xs.to_vec(), ys.to_vec(), zs.to_vec(), cm, name);
        if colorbar {
            if let Some((lo, hi)) = p.plot.heatmap_range(h) {
                p.plot.colorbar = Some(plotui_core::Colorbar { map: cm, lo, hi, label });
            }
        }
        if !out_handle.is_null() {
            *out_handle = h;
        }
        PLOTUI_OK
    })
}

/// A histogram of `values`. `bins` is a bin count and `bin_width` a fixed
/// width; pass 0 for the one you are not using, and 0 for both to take the
/// automatic rule. Giving both is an error.
///
/// # Safety
/// Pointer arguments follow the crate conventions. `axis` is "y", "y2" or
/// "y3" (NULL = "y").
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn plotui_add_histogram2d(
    p: *mut PlotuiPlot,
    values: *const f32,
    n: usize,
    bins: usize,
    bin_width: f64,
    rgb: *const u8,
    name: *const c_char,
    axis: *const c_char,
    out_handle: *mut usize,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        let values = match slice(values, n) {
            Ok(v) => v,
            Err(s) => return s,
        };
        let spec = match plotui_bind::parse_bins(
            (bins > 0).then_some(bins),
            (bin_width > 0.0).then_some(bin_width),
        ) {
            Ok(s) => s,
            Err(e) => return bind_status(e),
        };
        let name = match opt_str(name) {
            Ok(n) => n.map(str::to_string),
            Err(s) => return s,
        };
        let axis = match opt_str(axis) {
            Ok(a) => match axis_of(a) {
                Ok(a) => a,
                Err(e) => return bind_status(e),
            },
            Err(s) => return s,
        };
        let color = p.plot.resolve_color(opt_rgb(rgb));
        let h = p.plot.add_histogram2d(values.to_vec(), spec, color, name, axis);
        if !out_handle.is_null() {
            *out_handle = h;
        }
        PLOTUI_OK
    })
}

/// Append observations to a histogram and rebin.
///
/// # Safety
/// Pointer arguments follow the crate conventions.
#[no_mangle]
pub unsafe extern "C" fn plotui_extend_values(
    p: *mut PlotuiPlot,
    handle: usize,
    values: *const f32,
    n: usize,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        let values = match slice(values, n) {
            Ok(v) => v,
            Err(s) => return s,
        };
        match plotui_bind::extend_values(&mut p.plot, handle, values) {
            Ok(()) => PLOTUI_OK,
            Err(e) => bind_status(e),
        }
    })
}

/// A 2D bar series with an explicit orientation: "vertical" (NULL =
/// "vertical") or "horizontal". A horizontal bar reads `xs` as y positions.
///
/// # Safety
/// Pointer arguments follow the crate conventions. `axis` is "y", "y2" or
/// "y3" (NULL = "y").
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn plotui_add_bar2d_oriented(
    p: *mut PlotuiPlot,
    xs: *const f32,
    nx: usize,
    heights: *const f32,
    nh: usize,
    rgb: *const u8,
    orientation: *const c_char,
    name: *const c_char,
    axis: *const c_char,
    out_handle: *mut usize,
) -> i32 {
    guard(|| {
        let orient = match opt_str(orientation) {
            Ok(o) => match plotui_bind::parse_orient(o.unwrap_or("vertical")) {
                Ok(o) => o,
                Err(e) => return bind_status(e),
            },
            Err(s) => return s,
        };
        add_2d(p, xs, nx, heights, nh, rgb, name, axis, out_handle, |plot, xs, ys, c, name, ax| {
            plot.add_bar2d_oriented(xs, ys, c, orient, name, ax)
        })
    })
}

/// Set how several bar traces share their positions: "overlay", "group" or
/// "stack". Writes whether anything changed to `out_changed` when non-NULL.
///
/// # Safety
/// Pointer arguments follow the crate conventions.
#[no_mangle]
pub unsafe extern "C" fn plotui_set_barmode(
    p: *mut PlotuiPlot,
    mode: *const c_char,
    out_changed: *mut bool,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        let mode = match opt_str(mode) {
            Ok(Some(m)) => m,
            Ok(None) => {
                set_error("barmode must not be null");
                return PLOTUI_ERR_NULL;
            }
            Err(s) => return s,
        };
        match plotui_bind::set_barmode(&mut p.plot, mode) {
            Ok(changed) => {
                if !out_changed.is_null() {
                    *out_changed = changed;
                }
                PLOTUI_OK
            }
            Err(e) => bind_status(e),
        }
    })
}

/// Name an axis's categories: `axis` is "x" or "y", `names` is `n` C strings
/// (pass 0 to clear back to numeric ticks). Writes whether anything changed
/// to `out_changed` when non-NULL.
///
/// # Safety
/// Pointer arguments follow the crate conventions.
#[no_mangle]
pub unsafe extern "C" fn plotui_set_categories(
    p: *mut PlotuiPlot,
    axis: *const c_char,
    names: *const *const c_char,
    n: usize,
    out_changed: *mut bool,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        let axis = match opt_str(axis) {
            Ok(Some(a)) => a,
            Ok(None) => {
                set_error("axis must not be null");
                return PLOTUI_ERR_NULL;
            }
            Err(s) => return s,
        };
        let ptrs = match slice(names, n) {
            Ok(v) => v,
            Err(s) => return s,
        };
        let mut owned = Vec::with_capacity(ptrs.len());
        for &np in ptrs {
            match opt_str(np) {
                Ok(Some(s)) => owned.push(s.to_string()),
                Ok(None) => {
                    set_error("category name must not be null");
                    return PLOTUI_ERR_NULL;
                }
                Err(s) => return s,
            }
        }
        match plotui_bind::set_categories(&mut p.plot, axis, owned) {
            Ok(changed) => {
                if !out_changed.is_null() {
                    *out_changed = changed;
                }
                PLOTUI_OK
            }
            Err(e) => bind_status(e),
        }
    })
}

/// A 2D step series: the right-angle path between samples. `where_` is
/// "post" (NULL = "post"), "pre" or "mid".
///
/// # Safety
/// Pointer arguments follow the crate conventions. `axis` is "y", "y2" or
/// "y3" (NULL = "y").
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn plotui_add_step2d(
    p: *mut PlotuiPlot,
    xs: *const f32,
    nx: usize,
    ys: *const f32,
    ny: usize,
    rgb: *const u8,
    width: f32,
    where_: *const c_char,
    name: *const c_char,
    axis: *const c_char,
    out_handle: *mut usize,
) -> i32 {
    guard(|| {
        let interp = match opt_str(where_) {
            Ok(w) => match plotui_bind::parse_interp(w.unwrap_or("post")) {
                Ok(i) => i,
                Err(e) => return bind_status(e),
            },
            Err(s) => return s,
        };
        add_2d(p, xs, nx, ys, ny, rgb, name, axis, out_handle, |plot, xs, ys, c, name, ax| {
            plot.add_step2d(xs, ys, c, width, interp, name, ax)
        })
    })
}

/// # Safety
/// Pointer arguments follow the crate conventions.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn plotui_add_bar2d(
    p: *mut PlotuiPlot,
    xs: *const f32,
    nx: usize,
    heights: *const f32,
    nh: usize,
    rgb: *const u8,
    name: *const c_char,
    axis: *const c_char,
    out_handle: *mut usize,
) -> i32 {
    guard(|| {
        add_2d(p, xs, nx, heights, nh, rgb, name, axis, out_handle, |plot, xs, ys, c, name, ax| {
            plot.add_bar2d(xs, ys, c, name, ax)
        })
    })
}

/// Append points to a trace: `(xs, ys)` for 2D traces (`zs` NULL), `(xs,
/// ys, zs)` for 3D scatter/line traces.
///
/// # Safety
/// Pointer arguments follow the crate conventions.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn plotui_extend(
    p: *mut PlotuiPlot,
    handle: usize,
    xs: *const f32,
    nx: usize,
    ys: *const f32,
    ny: usize,
    zs: *const f32,
    nz: usize,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        let (xs, ys) = match (slice(xs, nx), slice(ys, ny)) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(s), _) | (_, Err(s)) => return s,
        };
        let zs = if zs.is_null() {
            None
        } else {
            match slice(zs, nz) {
                Ok(z) => Some(z),
                Err(s) => return s,
            }
        };
        match plotui_bind::extend(&mut p.plot, handle, xs, ys, zs) {
            Ok(()) => PLOTUI_OK,
            Err(e) => bind_status(e),
        }
    })
}

/// Core trace errors surface with the core's canonical Display text,
/// identically across bindings.
fn trace_status(e: plotui_core::TraceError) -> i32 {
    set_error(&e.to_string());
    match e {
        plotui_core::TraceError::UnknownTrace => PLOTUI_ERR_UNKNOWN_HANDLE,
        plotui_core::TraceError::Structural => PLOTUI_ERR_STRUCTURAL,
        plotui_core::TraceError::WrongKind | plotui_core::TraceError::LengthMismatch => {
            PLOTUI_ERR_INVALID_ARG
        }
    }
}

/// Move every node of a graph trace at once — the per-frame call of a
/// force-directed layout (pair with the `plotui_layout_*` functions). The
/// point count (min of nx/ny/nz) must match the trace's node count;
/// structure, indices, hover, and selection stay valid.
///
/// # Safety
/// Pointer arguments follow the crate conventions.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn plotui_set_graph_positions(
    p: *mut PlotuiPlot,
    handle: usize,
    xs: *const f32,
    nx: usize,
    ys: *const f32,
    ny: usize,
    zs: *const f32,
    nz: usize,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        let (xs, ys, zs) = match (slice(xs, nx), slice(ys, ny), slice(zs, nz)) {
            (Ok(a), Ok(b), Ok(c)) => (a, b, c),
            (Err(s), ..) | (_, Err(s), _) | (.., Err(s)) => return s,
        };
        match p.plot.set_graph_positions(handle, plotui_bind::zip3(xs, ys, zs)) {
            Ok(()) => PLOTUI_OK,
            Err(e) => trace_status(e),
        }
    })
}

/// Style a 2D scatter point by point. Each channel is independent: a NULL
/// pointer (or zero length) leaves it uniform. `rgbs` is `n_colors` packed
/// RGB triples, `shapes` is `n_shapes` silhouette names.
///
/// # Safety
/// Pointer arguments follow the crate conventions.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn plotui_set_point_styles(
    p: *mut PlotuiPlot,
    handle: usize,
    rgbs: *const u8,
    n_colors: usize,
    sizes: *const f32,
    n_sizes: usize,
    shapes: *const *const c_char,
    n_shapes: usize,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        let colors = match slice(rgbs, n_colors * 3) {
            Ok(bytes) => bytes.as_chunks::<3>().0.to_vec(),
            Err(s) => return s,
        };
        let sizes = match slice(sizes, n_sizes) {
            Ok(v) => v.to_vec(),
            Err(s) => return s,
        };
        let shape_ptrs = match slice(shapes, n_shapes) {
            Ok(v) => v,
            Err(s) => return s,
        };
        let mut names = Vec::with_capacity(shape_ptrs.len());
        for &sp in shape_ptrs {
            match opt_str(sp) {
                Ok(Some(n)) => names.push(n),
                Ok(None) => {
                    set_error("shape name must not be null");
                    return PLOTUI_ERR_NULL;
                }
                Err(s) => return s,
            }
        }
        match plotui_bind::set_point_styles(&mut p.plot, handle, colors, sizes, names) {
            Ok(()) => PLOTUI_OK,
            Err(e) => bind_status(e),
        }
    })
}

/// Recolor a graph trace in place — the host-side highlight primitive.
/// `node_rgbs` is `3 * n_nodes` bytes (one color per node); `edge_rgbs` is
/// `3 * n_edges` bytes or NULL (with n 0) to restore the default dimmed
/// endpoint blend.
///
/// # Safety
/// Pointer arguments follow the crate conventions.
#[no_mangle]
pub unsafe extern "C" fn plotui_set_graph_colors(
    p: *mut PlotuiPlot,
    handle: usize,
    node_rgbs: *const u8,
    n_nodes: usize,
    edge_rgbs: *const u8,
    n_edges: usize,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        let nc = match slice(node_rgbs, n_nodes * 3) {
            Ok(bytes) => bytes.as_chunks::<3>().0.to_vec(),
            Err(s) => return s,
        };
        let ec = match slice(edge_rgbs, n_edges * 3) {
            Ok([]) if edge_rgbs.is_null() => None,
            Ok(bytes) => Some(bytes.as_chunks::<3>().0.to_vec()),
            Err(s) => return s,
        };
        match p.plot.set_graph_colors(handle, nc, ec) {
            Ok(()) => PLOTUI_OK,
            Err(e) => trace_status(e),
        }
    })
}

/// Append nodes and edges to a graph trace (pair with
/// `plotui_layout_add_node`). `edges` is `2 * n_edges` u32s as (i, j) pairs
/// referencing old or new node indices; `node_rgbs` is `3 * n` byte triples
/// coloring the appended nodes (renderer default where missing). Appending
/// to a graph that is not the last node-bearing trace shifts downstream
/// flat node/edge indices, as with `plotui_extend` on a 3D scatter.
///
/// # Safety
/// Pointer arguments follow the crate conventions.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn plotui_extend_graph(
    p: *mut PlotuiPlot,
    handle: usize,
    xs: *const f32,
    nx: usize,
    ys: *const f32,
    ny: usize,
    zs: *const f32,
    nz: usize,
    node_rgbs: *const u8,
    n_node_rgbs: usize,
    edges: *const u32,
    n_edges: usize,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        let (xs, ys, zs) = match (slice(xs, nx), slice(ys, ny), slice(zs, nz)) {
            (Ok(a), Ok(b), Ok(c)) => (a, b, c),
            (Err(s), ..) | (_, Err(s), _) | (.., Err(s)) => return s,
        };
        let colors = match slice(node_rgbs, n_node_rgbs * 3) {
            Ok(bytes) => bytes.as_chunks::<3>().0.to_vec(),
            Err(s) => return s,
        };
        let edge_list: Vec<(u32, u32)> = match slice(edges, n_edges * 2) {
            Ok(e) => e.as_chunks::<2>().0.iter().map(|&[a, b]| (a, b)).collect(),
            Err(s) => return s,
        };
        match p.plot.extend_graph(handle, &plotui_bind::zip3(xs, ys, zs), &colors, &edge_list, None)
        {
            Ok(()) => PLOTUI_OK,
            Err(e) => trace_status(e),
        }
    })
}

/// Replace a 2D graph's edge waypoints — the second half of a relayout,
/// after `plotui_set_graph_positions` has moved the nodes. `route_pts` is
/// `2 * n_route_pts` floats as interleaved x/y and `route_starts` is one u32
/// per edge indexing into them; passing both empty restores straight edges.
///
/// # Safety
/// Pointer arguments follow the crate conventions.
#[no_mangle]
pub unsafe extern "C" fn plotui_set_graph_routes(
    p: *mut PlotuiPlot,
    handle: usize,
    route_pts: *const f32,
    n_route_pts: usize,
    route_starts: *const u32,
    n_route_starts: usize,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        let (pts, starts) =
            match (slice(route_pts, n_route_pts * 2), slice(route_starts, n_route_starts)) {
                (Ok(a), Ok(b)) => (a.as_chunks::<2>().0.to_vec(), b.to_vec()),
                (Err(s), _) | (_, Err(s)) => return s,
            };
        match p.plot.set_graph_routes(handle, pts, starts) {
            Ok(()) => PLOTUI_OK,
            Err(e) => trace_status(e),
        }
    })
}

/// # Safety
/// Pointer arguments follow the crate conventions.
#[no_mangle]
pub unsafe extern "C" fn plotui_set_visible(
    p: *mut PlotuiPlot,
    handle: usize,
    visible: bool,
    out_changed: *mut bool,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        match p.plot.set_visible(handle, visible) {
            Ok(changed) => {
                if !out_changed.is_null() {
                    *out_changed = changed;
                }
                PLOTUI_OK
            }
            Err(_) => {
                set_error(&format!("unknown trace handle {handle}"));
                PLOTUI_ERR_UNKNOWN_HANDLE
            }
        }
    })
}

const ELEMENT_NONE: i32 = 0;
const ELEMENT_NODE: i32 = 1;
const ELEMENT_EDGE: i32 = 2;

fn element_of(kind: i32, index: usize) -> Result<Option<Element>, BindError> {
    match kind {
        ELEMENT_NONE => Ok(None),
        ELEMENT_NODE => Ok(Some(Element::Node(index))),
        ELEMENT_EDGE => Ok(Some(Element::Edge(index))),
        _ => Err(BindError {
            kind: BindErrorKind::InvalidArg,
            msg: format!("element kind must be 0 (none), 1 (node) or 2 (edge), got {kind}"),
        }),
    }
}

fn element_parts(el: Option<Element>) -> (i32, usize) {
    match el {
        None => (ELEMENT_NONE, 0),
        Some(Element::Node(i)) => (ELEMENT_NODE, i),
        Some(Element::Edge(i)) => (ELEMENT_EDGE, i),
    }
}

/// Select an element (`kind`: 0 none, 1 node, 2 edge).
///
/// # Safety
/// `p` must be a live plot handle.
#[no_mangle]
pub unsafe extern "C" fn plotui_set_selected(p: *mut PlotuiPlot, kind: i32, index: usize) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        match element_of(kind, index) {
            Ok(el) => {
                p.plot.selected = el;
                PLOTUI_OK
            }
            Err(e) => bind_status(e),
        }
    })
}

/// Hover an element (`kind` as in `plotui_set_selected`); `out_changed`
/// tells the frontend whether a repaint is needed.
///
/// # Safety
/// `p` must be a live plot handle.
#[no_mangle]
pub unsafe extern "C" fn plotui_set_hovered(
    p: *mut PlotuiPlot,
    kind: i32,
    index: usize,
    out_changed: *mut bool,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        match element_of(kind, index) {
            Ok(el) => {
                let changed = plotui_bind::set_hovered(&mut p.plot, el);
                if !out_changed.is_null() {
                    *out_changed = changed;
                }
                PLOTUI_OK
            }
            Err(e) => bind_status(e),
        }
    })
}

/// Set the 2D crosshair position in framebuffer pixels (`has_px` false
/// clears it). Returns whether the state changed (repaint needed).
///
/// # Safety
/// `p` must be a live plot handle.
#[no_mangle]
pub unsafe extern "C" fn plotui_set_hover2d(p: *mut PlotuiPlot, has_px: bool, px: f32) -> bool {
    match plot_mut(p) {
        Ok(p) => plotui_bind::set_hover2d(&mut p.plot, has_px.then_some(px)),
        Err(_) => false,
    }
}

/// Set the explicit 2D x window in data coordinates (`has` false clears it);
/// `out_changed` tells the frontend whether a repaint is needed.
///
/// # Safety
/// `p` must be a live plot handle; `out_changed` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn plotui_set_x_window(
    p: *mut PlotuiPlot,
    has: bool,
    lo: f64,
    hi: f64,
    out_changed: *mut bool,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        match plotui_bind::set_x_window(&mut p.plot, has.then_some((lo, hi))) {
            Ok(changed) => {
                if !out_changed.is_null() {
                    *out_changed = changed;
                }
                PLOTUI_OK
            }
            Err(e) => bind_status(e),
        }
    })
}

/// Set the chart title (`text` NULL or empty clears it); writes whether the state
/// changed to `out_changed`.
///
/// # Safety
/// `p` must be a live plot handle; `text` NUL-terminated UTF-8 or NULL,
/// `out_changed` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn plotui_set_title(
    p: *mut PlotuiPlot,
    text: *const c_char,
    out_changed: *mut bool,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        let text = match opt_str(text) {
            Ok(t) => t.map(str::to_string),
            Err(s) => return s,
        };
        match plotui_bind::set_title(&mut p.plot, "title", text) {
            Ok(changed) => {
                if !out_changed.is_null() {
                    *out_changed = changed;
                }
                PLOTUI_OK
            }
            Err(e) => bind_status(e),
        }
    })
}

/// Read the chart title into `out` as a freshly allocated C string — empty when none
/// is set. Free it with `plotui_string_free`.
///
/// # Safety
/// `p` must be a live plot handle; `out` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn plotui_title(p: *const PlotuiPlot, out: *mut *mut c_char) -> i32 {
    guard(|| {
        let p = match plot_ref(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        out_string(p.plot.title.clone().unwrap_or_default(), out)
    })
}

/// Set the x axis title (`text` NULL or empty clears it); writes whether the state
/// changed to `out_changed`.
///
/// # Safety
/// `p` must be a live plot handle; `text` NUL-terminated UTF-8 or NULL,
/// `out_changed` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn plotui_set_x_title(
    p: *mut PlotuiPlot,
    text: *const c_char,
    out_changed: *mut bool,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        let text = match opt_str(text) {
            Ok(t) => t.map(str::to_string),
            Err(s) => return s,
        };
        match plotui_bind::set_title(&mut p.plot, "x", text) {
            Ok(changed) => {
                if !out_changed.is_null() {
                    *out_changed = changed;
                }
                PLOTUI_OK
            }
            Err(e) => bind_status(e),
        }
    })
}

/// Read the x axis title into `out` as a freshly allocated C string — empty when none
/// is set. Free it with `plotui_string_free`.
///
/// # Safety
/// `p` must be a live plot handle; `out` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn plotui_x_title(p: *const PlotuiPlot, out: *mut *mut c_char) -> i32 {
    guard(|| {
        let p = match plot_ref(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        out_string(p.plot.x_title.clone().unwrap_or_default(), out)
    })
}

/// Set the y axis title (`text` NULL or empty clears it); writes whether the state
/// changed to `out_changed`.
///
/// # Safety
/// `p` must be a live plot handle; `text` NUL-terminated UTF-8 or NULL,
/// `out_changed` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn plotui_set_y_title(
    p: *mut PlotuiPlot,
    text: *const c_char,
    out_changed: *mut bool,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        let text = match opt_str(text) {
            Ok(t) => t.map(str::to_string),
            Err(s) => return s,
        };
        match plotui_bind::set_title(&mut p.plot, "y", text) {
            Ok(changed) => {
                if !out_changed.is_null() {
                    *out_changed = changed;
                }
                PLOTUI_OK
            }
            Err(e) => bind_status(e),
        }
    })
}

/// Read the y axis title into `out` as a freshly allocated C string — empty when none
/// is set. Free it with `plotui_string_free`.
///
/// # Safety
/// `p` must be a live plot handle; `out` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn plotui_y_title(p: *const PlotuiPlot, out: *mut *mut c_char) -> i32 {
    guard(|| {
        let p = match plot_ref(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        out_string(p.plot.y_title.clone().unwrap_or_default(), out)
    })
}

/// Pin the x extent to `[lo, hi]` (`has` false autoscales); writes
/// whether the state changed to `out_changed`. Unlike an x window this
/// decides the extent only — the camera still composes on top.
///
/// # Safety
/// `p` must be a live plot handle; `out_changed` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn plotui_set_x_range(
    p: *mut PlotuiPlot,
    has: bool,
    lo: f64,
    hi: f64,
    out_changed: *mut bool,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        match plotui_bind::set_range(&mut p.plot, "x", has.then_some((lo, hi))) {
            Ok(changed) => {
                if !out_changed.is_null() {
                    *out_changed = changed;
                }
                PLOTUI_OK
            }
            Err(e) => bind_status(e),
        }
    })
}

/// Read the explicit x range into `out_lo`/`out_hi`; returns whether one
/// is set (outputs untouched when not).
///
/// # Safety
/// `p` must be a live plot handle; out pointers may be NULL.
#[no_mangle]
pub unsafe extern "C" fn plotui_x_range(
    p: *const PlotuiPlot,
    out_lo: *mut f64,
    out_hi: *mut f64,
) -> bool {
    match plot_ref(p) {
        Ok(p) => match p.plot.x_range {
            Some((lo, hi)) => {
                if !out_lo.is_null() {
                    *out_lo = lo;
                }
                if !out_hi.is_null() {
                    *out_hi = hi;
                }
                true
            }
            None => false,
        },
        Err(_) => false,
    }
}

/// Pin the primary y extent to `[lo, hi]` (`has` false autoscales); writes
/// whether the state changed to `out_changed`. Unlike an x window this
/// decides the extent only — the camera still composes on top.
///
/// # Safety
/// `p` must be a live plot handle; `out_changed` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn plotui_set_y_range(
    p: *mut PlotuiPlot,
    has: bool,
    lo: f64,
    hi: f64,
    out_changed: *mut bool,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        match plotui_bind::set_range(&mut p.plot, "y", has.then_some((lo, hi))) {
            Ok(changed) => {
                if !out_changed.is_null() {
                    *out_changed = changed;
                }
                PLOTUI_OK
            }
            Err(e) => bind_status(e),
        }
    })
}

/// Read the explicit primary y range into `out_lo`/`out_hi`; returns whether one
/// is set (outputs untouched when not).
///
/// # Safety
/// `p` must be a live plot handle; out pointers may be NULL.
#[no_mangle]
pub unsafe extern "C" fn plotui_y_range(
    p: *const PlotuiPlot,
    out_lo: *mut f64,
    out_hi: *mut f64,
) -> bool {
    match plot_ref(p) {
        Ok(p) => match p.plot.y_range {
            Some((lo, hi)) => {
                if !out_lo.is_null() {
                    *out_lo = lo;
                }
                if !out_hi.is_null() {
                    *out_hi = hi;
                }
                true
            }
            None => false,
        },
        Err(_) => false,
    }
}

/// Scale the x axis by log10 (or back). Ignored on a categorical or time axis. Writes whether the state changed
/// to `out_changed`.
///
/// # Safety
/// `p` must be a live plot handle; `out_changed` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn plotui_set_x_log(
    p: *mut PlotuiPlot,
    on: bool,
    out_changed: *mut bool,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        match plotui_bind::set_log(&mut p.plot, "x", on) {
            Ok(changed) => {
                if !out_changed.is_null() {
                    *out_changed = changed;
                }
                PLOTUI_OK
            }
            Err(e) => bind_status(e),
        }
    })
}

/// Whether the x axis is set to log10.
///
/// # Safety
/// `p` must be a live plot handle.
#[no_mangle]
pub unsafe extern "C" fn plotui_x_log(p: *const PlotuiPlot) -> bool {
    plot_ref(p).map(|p| p.plot.x_log).unwrap_or(false)
}

/// Scale the primary y axis by log10 (or back). Ignored on a categorical y axis; the right-hand axes stay linear. Writes whether the state changed
/// to `out_changed`.
///
/// # Safety
/// `p` must be a live plot handle; `out_changed` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn plotui_set_y_log(
    p: *mut PlotuiPlot,
    on: bool,
    out_changed: *mut bool,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        match plotui_bind::set_log(&mut p.plot, "y", on) {
            Ok(changed) => {
                if !out_changed.is_null() {
                    *out_changed = changed;
                }
                PLOTUI_OK
            }
            Err(e) => bind_status(e),
        }
    })
}

/// Whether the primary y axis is set to log10.
///
/// # Safety
/// `p` must be a live plot handle.
#[no_mangle]
pub unsafe extern "C" fn plotui_y_log(p: *const PlotuiPlot) -> bool {
    plot_ref(p).map(|p| p.plot.y_log).unwrap_or(false)
}

/// Read the current x window into `out_lo`/`out_hi`; returns whether one is
/// set (outputs untouched when not).
///
/// # Safety
/// `p` must be a live plot handle; out pointers may be NULL.
#[no_mangle]
pub unsafe extern "C" fn plotui_x_window(
    p: *const PlotuiPlot,
    out_lo: *mut f64,
    out_hi: *mut f64,
) -> bool {
    match plot_ref(p) {
        Ok(p) => match p.plot.x_window {
            Some((lo, hi)) => {
                if !out_lo.is_null() {
                    *out_lo = lo;
                }
                if !out_hi.is_null() {
                    *out_hi = hi;
                }
                true
            }
            None => false,
        },
        Err(_) => false,
    }
}

/// Toggle the range-slider strip. Returns whether the state changed
/// (repaint needed).
///
/// # Safety
/// `p` must be a live plot handle.
#[no_mangle]
pub unsafe extern "C" fn plotui_set_range_slider(p: *mut PlotuiPlot, on: bool) -> bool {
    match plot_mut(p) {
        Ok(p) => plotui_bind::set_range_slider(&mut p.plot, on),
        Err(_) => false,
    }
}

/// Set the time-axis epoch base, seconds UTC (`has` false clears it): x
/// values then mean seconds since this base, x ticks become calendar dates.
/// `out_changed` tells the frontend whether a repaint is needed.
///
/// # Safety
/// `p` must be a live plot handle; `out_changed` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn plotui_set_x_epoch(
    p: *mut PlotuiPlot,
    has: bool,
    epoch: f64,
    out_changed: *mut bool,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        match plotui_bind::set_x_epoch(&mut p.plot, has.then_some(epoch)) {
            Ok(changed) => {
                if !out_changed.is_null() {
                    *out_changed = changed;
                }
                PLOTUI_OK
            }
            Err(e) => bind_status(e),
        }
    })
}

/// Read the time-axis epoch base into `out_epoch`; returns whether one is
/// set.
///
/// # Safety
/// `p` must be a live plot handle; `out_epoch` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn plotui_x_epoch(p: *const PlotuiPlot, out_epoch: *mut f64) -> bool {
    match plot_ref(p) {
        Ok(p) => match p.plot.x_epoch {
            Some(e) => {
                if !out_epoch.is_null() {
                    *out_epoch = e;
                }
                true
            }
            None => false,
        },
        Err(_) => false,
    }
}

const RANGE_NONE: i32 = 0;
const RANGE_LEFT: i32 = 1;
const RANGE_RIGHT: i32 = 2;
const RANGE_WINDOW: i32 = 3;
const RANGE_TRACK: i32 = 4;

fn range_of(part: i32) -> Result<RangeHit, BindError> {
    match part {
        RANGE_LEFT => Ok(RangeHit::LeftHandle),
        RANGE_RIGHT => Ok(RangeHit::RightHandle),
        RANGE_WINDOW => Ok(RangeHit::Window),
        RANGE_TRACK => Ok(RangeHit::Track),
        _ => Err(BindError {
            kind: BindErrorKind::InvalidArg,
            msg: format!(
                "range part must be 1 (left), 2 (right), 3 (window) or 4 (track), got {part}"
            ),
        }),
    }
}

fn range_part(hit: Option<RangeHit>) -> i32 {
    match hit {
        None => RANGE_NONE,
        Some(RangeHit::LeftHandle) => RANGE_LEFT,
        Some(RangeHit::RightHandle) => RANGE_RIGHT,
        Some(RangeHit::Window) => RANGE_WINDOW,
        Some(RangeHit::Track) => RANGE_TRACK,
    }
}

/// What the range-slider strip has under `(px, py)` framebuffer pixels at a
/// `w`×`h` frame, within `tol_px`: `out_part` is 0 none, 1 left handle,
/// 2 right handle, 3 window body, 4 track.
///
/// # Safety
/// `p` must be a live plot handle; `out_part` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn plotui_range_slider_hit(
    p: *const PlotuiPlot,
    px_w: usize,
    px_h: usize,
    px: f32,
    py: f32,
    tol_px: f32,
    out_part: *mut i32,
) -> i32 {
    guard(|| {
        let p = match plot_ref(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        let hit = p.plot.range_slider_hit(px_w, px_h, px, py, tol_px);
        if !out_part.is_null() {
            *out_part = range_part(hit);
        }
        PLOTUI_OK
    })
}

/// Drag the grabbed strip `part` (1 left, 2 right, 3 window, 4 track) by
/// `dx_px` framebuffer pixels; `out_changed` tells the frontend whether a
/// repaint is needed.
///
/// # Safety
/// `p` must be a live plot handle; `out_changed` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn plotui_drag_x_window(
    p: *mut PlotuiPlot,
    px_w: usize,
    px_h: usize,
    part: i32,
    dx_px: f32,
    out_changed: *mut bool,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        match range_of(part) {
            Ok(hit) => {
                let changed = p.plot.drag_x_window(px_w, px_h, hit, dx_px);
                if !out_changed.is_null() {
                    *out_changed = changed;
                }
                PLOTUI_OK
            }
            Err(e) => bind_status(e),
        }
    })
}

/// Center the window on the strip position under `px` framebuffer pixels (a
/// track click). Returns whether the window changed (repaint needed).
///
/// # Safety
/// `p` must be a live plot handle.
#[no_mangle]
pub unsafe extern "C" fn plotui_jump_x_window(
    p: *mut PlotuiPlot,
    px_w: usize,
    px_h: usize,
    px: f32,
) -> bool {
    match plot_mut(p) {
        Ok(p) => p.plot.jump_x_window(px_w, px_h, px),
        Err(_) => false,
    }
}

/// Slide a set window by a plot-area drag of `dx_px` framebuffer pixels.
/// Returns whether the window changed (repaint needed).
///
/// # Safety
/// `p` must be a live plot handle.
#[no_mangle]
pub unsafe extern "C" fn plotui_pan_x_window(
    p: *mut PlotuiPlot,
    px_w: usize,
    px_h: usize,
    dx_px: f32,
) -> bool {
    match plot_mut(p) {
        Ok(p) => p.plot.pan_x_window(px_w, px_h, dx_px),
        Err(_) => false,
    }
}

/// Zoom the window about the data x under `px` framebuffer pixels
/// (`factor > 1` zooms in). Returns whether the window changed (repaint
/// needed).
///
/// # Safety
/// `p` must be a live plot handle.
#[no_mangle]
pub unsafe extern "C" fn plotui_zoom_x_window(
    p: *mut PlotuiPlot,
    px_w: usize,
    px_h: usize,
    px: f32,
    factor: f64,
) -> bool {
    match plot_mut(p) {
        Ok(p) => p.plot.zoom_x_window(px_w, px_h, px, factor),
        Err(_) => false,
    }
}

/// Slide a set window by `frac` of its own span (positive = later x) — the
/// keyboard step, needing no pixel geometry. Returns whether the window
/// changed (repaint needed).
///
/// # Safety
/// `p` must be a live plot handle.
#[no_mangle]
pub unsafe extern "C" fn plotui_shift_x_window(p: *mut PlotuiPlot, frac: f64) -> bool {
    match plot_mut(p) {
        Ok(p) => p.plot.shift_x_window(frac),
        Err(_) => false,
    }
}

/// Pick under `(px, py)`: nearest node within `node_radius`, else nearest
/// graph edge within `edge_radius` (negative = the default,
/// 0.75 × node_radius). `out_kind` is 0/1/2 as in `plotui_set_selected`.
///
/// # Safety
/// `p` must be a live plot handle; out pointers may be NULL.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn plotui_pick_element_px(
    p: *const PlotuiPlot,
    px_w: usize,
    px_h: usize,
    px: f32,
    py: f32,
    node_radius: f32,
    edge_radius: f32,
    out_kind: *mut i32,
    out_index: *mut usize,
) -> i32 {
    guard(|| {
        let p = match plot_ref(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        let er = (edge_radius >= 0.0).then_some(edge_radius);
        let el = plotui_bind::pick_element_px(&p.plot, px_w, px_h, px, py, node_radius, er);
        let (kind, index) = element_parts(el);
        if !out_kind.is_null() {
            *out_kind = kind;
        }
        if !out_index.is_null() {
            *out_index = index;
        }
        PLOTUI_OK
    })
}

/// Nearest node within `radius` of `(px, py)`, nodes only. Returns whether
/// one was found; the index lands in `out_index`.
///
/// # Safety
/// `p` must be a live plot handle; `out_index` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn plotui_pick_px(
    p: *const PlotuiPlot,
    px_w: usize,
    px_h: usize,
    px: f32,
    py: f32,
    radius: f32,
    out_index: *mut usize,
) -> bool {
    match plot_ref(p) {
        Ok(p) => match p.plot.pick(px_w, px_h, px, py, radius) {
            Some(i) => {
                if !out_index.is_null() {
                    *out_index = i;
                }
                true
            }
            None => false,
        },
        Err(_) => false,
    }
}

// ---- camera ----

/// # Safety
/// `p` must be a live plot handle.
#[no_mangle]
pub unsafe extern "C" fn plotui_rotate(p: *mut PlotuiPlot, d_yaw: f64, d_pitch: f64) {
    if let Ok(p) = plot_mut(p) {
        p.plot.camera.rotate(d_yaw, d_pitch);
    }
}

/// # Safety
/// `p` must be a live plot handle.
#[no_mangle]
pub unsafe extern "C" fn plotui_zoom_by(p: *mut PlotuiPlot, factor: f64) {
    if let Ok(p) = plot_mut(p) {
        p.plot.camera.zoom_by(factor);
    }
}

/// Pan in framebuffer pixels.
///
/// # Safety
/// `p` must be a live plot handle.
#[no_mangle]
pub unsafe extern "C" fn plotui_pan(p: *mut PlotuiPlot, dx: f64, dy: f64) {
    if let Ok(p) = plot_mut(p) {
        p.plot.camera.pan(dx, dy);
    }
}

/// # Safety
/// `p` must be a live plot handle.
#[no_mangle]
pub unsafe extern "C" fn plotui_reset(p: *mut PlotuiPlot) {
    if let Ok(p) = plot_mut(p) {
        p.plot.camera.reset();
    }
}

/// Remap what drag gestures do. Each name is a camera control — "yaw",
/// "pitch", "pan_x", "pan_y", "zoom" or "off", optionally prefixed with
/// '-' to invert the axis — or NULL to keep that axis's current binding.
/// The default map is drag = rotate as a trackball (yaw/pitch, the drag
/// grabs the object), shift-drag = pan; "-yaw"/"-pitch" restore
/// camera-grab rotation. Returns 0, or -1 with `plotui_last_error()` set.
///
/// # Safety
/// `p` must be a live plot handle; names must be NULL or valid C strings.
#[no_mangle]
pub unsafe extern "C" fn plotui_set_input_map(
    p: *mut PlotuiPlot,
    drag_x: *const c_char,
    drag_y: *const c_char,
    shift_drag_x: *const c_char,
    shift_drag_y: *const c_char,
) -> i32 {
    let Ok(p) = plot_mut(p) else { return -1 };
    let mut m = p.plot.input_map;
    for (slot, inv, ptr) in [
        (&mut m.drag_x, &mut m.invert_drag_x, drag_x),
        (&mut m.drag_y, &mut m.invert_drag_y, drag_y),
        (&mut m.shift_drag_x, &mut m.invert_shift_drag_x, shift_drag_x),
        (&mut m.shift_drag_y, &mut m.invert_shift_drag_y, shift_drag_y),
    ] {
        match opt_str(ptr) {
            Ok(Some(name)) => match plotui_bind::parse_camera_control(name) {
                Ok((c, i)) => {
                    *slot = c;
                    *inv = i;
                }
                Err(e) => return bind_status(e),
            },
            Ok(None) => {}
            Err(rc) => return rc,
        }
    }
    p.plot.input_map = m;
    0
}

/// Route a drag through the input map (see `plotui_set_input_map`):
/// `(dx, dy)` pointer deltas in whatever unit the scales are calibrated
/// for — `rotate_scale` radians per unit, `pan_*_scale` framebuffer pixels
/// per unit, `zoom_scale` log-zoom per unit. `shift` nonzero selects the
/// shift-drag bindings.
///
/// # Safety
/// `p` must be a live plot handle.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn plotui_apply_drag(
    p: *mut PlotuiPlot,
    dx: f64,
    dy: f64,
    shift: i32,
    rotate_scale: f64,
    pan_x_scale: f64,
    pan_y_scale: f64,
    zoom_scale: f64,
) {
    if let Ok(p) = plot_mut(p) {
        let scales = plotui_core::DragScales {
            rotate: rotate_scale,
            pan_x: pan_x_scale,
            pan_y: pan_y_scale,
            zoom: zoom_scale,
        };
        p.plot.apply_drag(dx, dy, shift != 0, scales);
    }
}

/// Write the camera state (yaw, pitch, zoom, pan_x, pan_y) into `out[5]`.
///
/// # Safety
/// `p` must be a live plot handle; `out` must hold 5 doubles.
#[no_mangle]
pub unsafe extern "C" fn plotui_camera_state(p: *const PlotuiPlot, out: *mut f64) {
    if let (Ok(p), false) = (plot_ref(p), out.is_null()) {
        let (yaw, pitch, zoom, pan_x, pan_y) = p.plot.camera.state();
        let out = std::slice::from_raw_parts_mut(out, 5);
        out.copy_from_slice(&[yaw, pitch, zoom, pan_x, pan_y]);
    }
}

/// # Safety
/// `p` must be a live plot handle.
#[no_mangle]
pub unsafe extern "C" fn plotui_set_camera_state(
    p: *mut PlotuiPlot,
    yaw: f64,
    pitch: f64,
    zoom: f64,
    pan_x: f64,
    pan_y: f64,
) {
    if let Ok(p) = plot_mut(p) {
        p.plot.camera.set_state(yaw, pitch, zoom, pan_x, pan_y);
    }
}

/// Pin the 3D data frame to `(lo, hi)` corners (each NULL or 3 floats);
/// pass both NULL to restore auto-fit. Like the Python binding, anything
/// short of two corners means auto-fit.
///
/// # Safety
/// `p` must be a live plot handle.
#[no_mangle]
pub unsafe extern "C" fn plotui_set_bounds(
    p: *mut PlotuiPlot,
    lo: *const f32,
    hi: *const f32,
) -> i32 {
    guard(|| {
        let p = match plot_mut(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        p.plot.bounds_override = if lo.is_null() || hi.is_null() {
            None
        } else {
            let l = std::slice::from_raw_parts(lo, 3);
            let h = std::slice::from_raw_parts(hi, 3);
            Some(([l[0], l[1], l[2]], [h[0], h[1], h[2]]))
        };
        PLOTUI_OK
    })
}

/// # Safety
/// `p` must be a live plot handle.
#[no_mangle]
pub unsafe extern "C" fn plotui_set_show_box(p: *mut PlotuiPlot, show: bool) {
    if let Ok(p) = plot_mut(p) {
        p.plot.show_box = show;
    }
}

/// Draw the 2D chrome — grid, axis rules and tick labels — or not. `show`
/// is a tri-state: negative restores the automatic rule (a frame whose
/// visible 2D traces are all graphs draws no chrome), 0 pins it off, and
/// any positive value pins it on. The legend, colorbar, range slider and
/// crosshair are unaffected, and 3D plots ignore it.
///
/// # Safety
/// `p` must be a live plot handle.
#[no_mangle]
pub unsafe extern "C" fn plotui_set_show_axes(p: *mut PlotuiPlot, show: i32) {
    if let Ok(p) = plot_mut(p) {
        p.plot.set_show_axes(if show < 0 { None } else { Some(show > 0) });
    }
}

/// Recolour the non-data chrome; each pointer is NULL (keep) or 3 RGB bytes.
///
/// # Safety
/// `p` must be a live plot handle.
#[no_mangle]
pub unsafe extern "C" fn plotui_set_chrome(
    p: *mut PlotuiPlot,
    bg: *const u8,
    frame: *const u8,
    grid: *const u8,
    ink: *const u8,
    ink_bright: *const u8,
) {
    if let Ok(p) = plot_mut(p) {
        let c = &mut p.plot.chrome;
        if let Some(v) = opt_rgb(bg) {
            c.bg = v;
        }
        if let Some(v) = opt_rgb(frame) {
            c.frame = v;
        }
        if let Some(v) = opt_rgb(grid) {
            c.grid = v;
        }
        if let Some(v) = opt_rgb(ink) {
            c.ink = v;
        }
        if let Some(v) = opt_rgb(ink_bright) {
            c.ink_bright = v;
        }
    }
}

// ---- info ----

/// # Safety
/// `p` must be a live plot handle.
#[no_mangle]
pub unsafe extern "C" fn plotui_is_3d(p: *const PlotuiPlot) -> bool {
    plot_ref(p).map(|p| p.plot.is_3d()).unwrap_or(false)
}

/// # Safety
/// `p` must be a live plot handle.
#[no_mangle]
pub unsafe extern "C" fn plotui_node_count(p: *const PlotuiPlot) -> usize {
    plot_ref(p).map(|p| p.plot.node_count()).unwrap_or(0)
}

/// # Safety
/// `p` must be a live plot handle.
#[no_mangle]
pub unsafe extern "C" fn plotui_vertex_count(p: *const PlotuiPlot) -> usize {
    plot_ref(p).map(|p| p.plot.vertex_count()).unwrap_or(0)
}

/// This plot's Kitty image id.
///
/// # Safety
/// `p` must be a live plot handle.
#[no_mangle]
pub unsafe extern "C" fn plotui_image_id(p: *const PlotuiPlot) -> u32 {
    plot_ref(p).map(|p| p.image_id).unwrap_or(0)
}

/// Project every node to screen space for a `px_w`×`px_h` framebuffer:
/// writes `plotui_node_count(p) * 3` floats — (x_px, y_px, depth) per node,
/// flat-index order.
///
/// # Safety
/// `out` must hold `plotui_node_count(p) * 3` floats.
#[no_mangle]
pub unsafe extern "C" fn plotui_project_nodes(
    p: *const PlotuiPlot,
    px_w: usize,
    px_h: usize,
    out: *mut f32,
) -> i32 {
    guard(|| {
        let p = match plot_ref(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        if out.is_null() {
            set_error("null output pointer");
            return PLOTUI_ERR_NULL;
        }
        let projected = p.plot.project_nodes(px_w, px_h);
        let out = std::slice::from_raw_parts_mut(out, projected.len() * 3);
        for (i, pt) in projected.iter().enumerate() {
            out[i * 3..i * 3 + 3].copy_from_slice(pt);
        }
        PLOTUI_OK
    })
}

// ---- rendering ----

/// Render as a Kitty escape for `cols`×`rows` cells of `cell_w`×`cell_h`
/// pixels, using this plot's image id. Same contract as the Python
/// binding's `render_kitty` (`compat_chunks` for the iTerm2 tier, `scale`
/// for reduced-resolution interaction frames, `replace` to skip the
/// delete-before-transmit). Free the string with `plotui_string_free`.
///
/// # Safety
/// `p` must be a live plot handle; `out_escape` must be a valid pointer.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn plotui_render_kitty(
    p: *const PlotuiPlot,
    cols: u16,
    rows: u16,
    cell_w: u16,
    cell_h: u16,
    compat_chunks: bool,
    scale: f64,
    replace: bool,
    out_escape: *mut *mut c_char,
) -> i32 {
    guard(|| {
        let p = match plot_ref(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        let (pw, ph, pan_scale) = scaled_dims(cols, rows, cell_w, cell_h, scale);
        let fb = p.plot.render_at(pw, ph, pan_scale);
        let escape = if compat_chunks {
            plotui_protocol::kitty_compat_with_id(&fb, cols, rows, !replace, p.image_id)
        } else {
            plotui_protocol::kitty_with_id(&fb, cols, rows, p.image_id)
        };
        out_string(escape, out_escape)
    })
}

/// Render one frame and write the raw RGBA8 pixels: `px_w * px_h * 4`
/// bytes, row-major, alpha 0 for undrawn pixels.
///
/// # Safety
/// `out` must hold `px_w * px_h * 4` bytes.
#[no_mangle]
pub unsafe extern "C" fn plotui_render_rgba(
    p: *const PlotuiPlot,
    px_w: usize,
    px_h: usize,
    out: *mut u8,
) -> i32 {
    guard(|| {
        let p = match plot_ref(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        if out.is_null() {
            set_error("null output pointer");
            return PLOTUI_ERR_NULL;
        }
        let rgba = p.plot.render(px_w, px_h).rgba();
        std::slice::from_raw_parts_mut(out, rgba.len()).copy_from_slice(&rgba);
        PLOTUI_OK
    })
}

/// Render a Kitty Unicode-placeholder frame and return its *metadata*: the
/// transmit escape (free with `plotui_string_free`), the placeholder
/// foreground color in `out_id_rgb[3]`, and the high id byte in
/// `out_extra`. The caller synthesizes each cell as U+10EEEE + the row
/// diacritic + the column diacritic (+ the extra-byte diacritic when
/// `out_extra` is nonzero) from `plotui_diacritics` — identical to what
/// `kitty_placeholder_cells` would return, without marshaling cols×rows
/// strings per frame.
///
/// # Safety
/// `p` must be a live plot handle; out pointers must be valid
/// (`out_id_rgb` holds 3 bytes).
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn plotui_render_kitty_placeholder_meta(
    p: *const PlotuiPlot,
    cols: u16,
    rows: u16,
    cell_w: u16,
    cell_h: u16,
    scale: f64,
    out_transmit: *mut *mut c_char,
    out_id_rgb: *mut u8,
    out_extra: *mut u8,
) -> i32 {
    guard(|| {
        let p = match plot_ref(p) {
            Ok(p) => p,
            Err(s) => return s,
        };
        let (pw, ph, pan_scale) = scaled_dims(cols, rows, cell_w, cell_h, scale);
        let fb = p.plot.render_at(pw, ph, pan_scale);
        let enc = plotui_protocol::kitty_placeholder_cells_with_id(&fb, cols, rows, p.image_id);
        if !out_id_rgb.is_null() {
            let out = std::slice::from_raw_parts_mut(out_id_rgb, 3);
            out.copy_from_slice(&[enc.id_rgb.0, enc.id_rgb.1, enc.id_rgb.2]);
        }
        if !out_extra.is_null() {
            *out_extra = p.image_id.to_be_bytes()[0];
        }
        out_string(enc.transmit, out_transmit)
    })
}

/// The escape that deletes this plot's image from the terminal (emit on
/// exit). Free with `plotui_string_free`.
///
/// # Safety
/// `p` must be a live plot handle.
#[no_mangle]
pub unsafe extern "C" fn plotui_kitty_cleanup(p: *const PlotuiPlot) -> *mut c_char {
    match plot_ref(p) {
        Ok(p) => CString::new(plotui_protocol::kitty_cleanup_with_id(p.image_id))
            .map(CString::into_raw)
            .unwrap_or(ptr::null_mut()),
        Err(_) => ptr::null_mut(),
    }
}

/// The spec-defined placeholder diacritic table: `out_len` codepoints,
/// `table[n]` encoding row/column index `n`. Static — never free it.
///
/// # Safety
/// `out_len` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn plotui_diacritics(out_len: *mut usize) -> *const u32 {
    use std::sync::OnceLock;
    static TABLE: OnceLock<Vec<u32>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        plotui_protocol::placeholder_diacritics().iter().map(|&c| c as u32).collect()
    });
    if !out_len.is_null() {
        unsafe { *out_len = table.len() };
    }
    table.as_ptr()
}

// ---- terminal glue (plotui-term) ----

pub const PLOTUI_RENDER_PLACEHOLDER: i32 = 0;
pub const PLOTUI_RENDER_DIRECT: i32 = 1;
pub const PLOTUI_RENDER_UNSUPPORTED: i32 = 2;

/// Best render path for this terminal (honors `PLOTUI_RENDER`): 0
/// placeholder, 1 direct, 2 unsupported.
#[no_mangle]
pub extern "C" fn plotui_detect_render_mode() -> i32 {
    match plotui_term::detect_render_mode() {
        plotui_term::RenderMode::Placeholder => PLOTUI_RENDER_PLACEHOLDER,
        plotui_term::RenderMode::Direct => PLOTUI_RENDER_DIRECT,
        plotui_term::RenderMode::Unsupported => PLOTUI_RENDER_UNSUPPORTED,
    }
}

/// The terminal's device pixels per cell via ioctl. Returns false (and
/// leaves the outputs alone) when no stream reports a pixel size — use the
/// 12×24 fallback then.
///
/// # Safety
/// Out pointers must be valid or NULL.
#[no_mangle]
pub unsafe extern "C" fn plotui_detect_cell_px(out_w: *mut u16, out_h: *mut u16) -> bool {
    // (0, 0) can't come back from a successful probe, so it marks failure.
    let (w, h) = plotui_term::detect_cell_px((0, 0));
    if w == 0 || h == 0 {
        return false;
    }
    if !out_w.is_null() {
        *out_w = w;
    }
    if !out_h.is_null() {
        *out_h = h;
    }
    true
}

/// True when `PLOTUI_KITTY_REPLACE` asks direct mode to skip the
/// delete-before-transmit.
#[no_mangle]
pub extern "C" fn plotui_kitty_replace_env() -> bool {
    plotui_term::kitty_replace_env()
}

/// Wrap an escape for tmux passthrough when `$TMUX` is set (otherwise a
/// plain copy). Free with `plotui_string_free`.
///
/// # Safety
/// `escape` must be a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn plotui_tmux_wrap(escape: *const c_char) -> *mut c_char {
    match opt_str(escape) {
        Ok(Some(s)) => CString::new(plotui_term::tmux_wrap(s))
            .map(CString::into_raw)
            .unwrap_or(ptr::null_mut()),
        _ => ptr::null_mut(),
    }
}

/// Resolution multiplier for the next frame: `configured_scale` only for
/// large 3D plots (≥ `PLOTUI_LARGE_VERTEX_COUNT` vertices) while
/// `interacting`, else 1.0 — the shared half-resolution policy.
///
/// # Safety
/// `p` must be a live plot handle.
#[no_mangle]
pub unsafe extern "C" fn plotui_interactive_scale(
    p: *const PlotuiPlot,
    interacting: bool,
    configured_scale: f64,
) -> f64 {
    match plot_ref(p) {
        Ok(p) => active_scale(configured_scale, p.plot.is_3d(), p.plot.vertex_count(), interacting),
        Err(_) => 1.0,
    }
}

// ---- force layout ----

/// An opaque 3D force-directed layout (`plotui_core::ForceLayout`): pure
/// math on the host's timer, deterministic for a given seed. Not
/// thread-safe: one thread at a time, like `PlotuiPlot`.
pub struct PlotuiLayout {
    layout: plotui_core::ForceLayout,
}

/// A layout over `n_nodes` with seeded initial positions in the unit ball.
/// `edges` is `2 * n_edges` u32s as (i, j) index pairs. Free with
/// `plotui_layout_free`. Returns NULL only on a malformed edge slice.
///
/// # Safety
/// Pointer arguments follow the crate conventions.
#[no_mangle]
pub unsafe extern "C" fn plotui_layout_new(
    n_nodes: usize,
    edges: *const u32,
    n_edges: usize,
    seed: u32,
) -> *mut PlotuiLayout {
    let Ok(e) = slice(edges, n_edges * 2) else {
        return ptr::null_mut();
    };
    let pairs: Vec<(u32, u32)> = e.as_chunks::<2>().0.iter().map(|&[a, b]| (a, b)).collect();
    Box::into_raw(Box::new(PlotuiLayout {
        layout: plotui_core::ForceLayout::new(n_nodes, &pairs, seed),
    }))
}

/// Free a layout. NULL is a no-op.
///
/// # Safety
/// `l` must be a pointer from `plotui_layout_new` not yet freed.
#[no_mangle]
pub unsafe extern "C" fn plotui_layout_free(l: *mut PlotuiLayout) {
    if !l.is_null() {
        drop(Box::from_raw(l));
    }
}

/// One simulation tick; returns the mean displacement ("energy") — hosts
/// stop repainting once it drops below ~1e-3. A NULL layout returns 0.
///
/// # Safety
/// `l` must be a live layout handle or NULL.
#[no_mangle]
pub unsafe extern "C" fn plotui_layout_step(l: *mut PlotuiLayout) -> f32 {
    match l.as_mut() {
        Some(l) => l.layout.step(),
        None => 0.0,
    }
}

/// The layout's node count (grows with `plotui_layout_add_node`) — the
/// `out` size contract for `plotui_layout_positions`.
///
/// # Safety
/// `l` must be a live layout handle or NULL (which counts 0).
#[no_mangle]
pub unsafe extern "C" fn plotui_layout_node_count(l: *const PlotuiLayout) -> usize {
    match l.as_ref() {
        Some(l) => l.layout.positions().len(),
        None => 0,
    }
}

/// Write the current positions as flat `[x0, y0, z0, x1, …]` into `out`,
/// which must hold `plotui_layout_node_count(l) * 3` floats — feed them to
/// `plotui_set_graph_positions`.
///
/// # Safety
/// `l` must be a live layout handle; `out` must point at enough floats.
#[no_mangle]
pub unsafe extern "C" fn plotui_layout_positions(l: *const PlotuiLayout, out: *mut f32) -> i32 {
    guard(|| {
        let Some(l) = l.as_ref() else {
            set_error("null layout handle");
            return PLOTUI_ERR_NULL;
        };
        if out.is_null() {
            set_error("null output pointer");
            return PLOTUI_ERR_NULL;
        }
        let pts = l.layout.positions();
        let dst = std::slice::from_raw_parts_mut(out, pts.len() * 3);
        for (chunk, p) in dst.as_chunks_mut::<3>().0.iter_mut().zip(pts) {
            chunk.copy_from_slice(p);
        }
        PLOTUI_OK
    })
}

/// Warm insertion of one node connected to `neighbors` (existing indices,
/// `n_neighbors` u32s): it spawns beside its first neighbor and re-heats
/// the simulation. Writes the new node's index to `out_index`; pair with
/// `plotui_extend_graph`.
///
/// # Safety
/// Pointer arguments follow the crate conventions.
#[no_mangle]
pub unsafe extern "C" fn plotui_layout_add_node(
    l: *mut PlotuiLayout,
    neighbors: *const u32,
    n_neighbors: usize,
    out_index: *mut usize,
) -> i32 {
    guard(|| {
        let Some(l) = l.as_mut() else {
            set_error("null layout handle");
            return PLOTUI_ERR_NULL;
        };
        let ns = match slice(neighbors, n_neighbors) {
            Ok(ns) => ns,
            Err(s) => return s,
        };
        let idx = l.layout.add_node(ns);
        if !out_index.is_null() {
            *out_index = idx;
        }
        PLOTUI_OK
    })
}

// ---- layered layout, DOT, reachability ----

/// An opaque hierarchical layout (`plotui_core::LayeredLayout`): solved once
/// in `plotui_layered_layout_new`, then read out. Not thread-safe: one
/// thread at a time, like `PlotuiPlot`.
pub struct PlotuiLayeredLayout {
    layout: plotui_core::LayeredLayout,
    n_nodes: usize,
    n_edges: usize,
}

/// Lay out `n_nodes` connected by `edges` (`2 * n_edges` u32s as (i, j)
/// pairs) flowing in `rankdir` (`"TB"` or `"LR"`, case-insensitive; NULL
/// means `"TB"`). Free with `plotui_layered_layout_free`. Returns NULL on a
/// malformed edge slice or an unknown `rankdir`, with the reason in
/// `plotui_last_error`.
///
/// # Safety
/// Pointer arguments follow the crate conventions.
#[no_mangle]
pub unsafe extern "C" fn plotui_layered_layout_new(
    n_nodes: usize,
    edges: *const u32,
    n_edges: usize,
    rankdir: *const c_char,
) -> *mut PlotuiLayeredLayout {
    let Ok(e) = slice(edges, n_edges * 2) else {
        return ptr::null_mut();
    };
    let dir = match opt_str(rankdir) {
        Ok(None) => plotui_core::RankDir::TB,
        Ok(Some(s)) => match plotui_bind::parse_rankdir(s) {
            Ok(d) => d,
            Err(err) => {
                set_error(&err.msg);
                return ptr::null_mut();
            }
        },
        Err(_) => return ptr::null_mut(),
    };
    let pairs: Vec<(u32, u32)> = e.as_chunks::<2>().0.iter().map(|&[a, b]| (a, b)).collect();
    Box::into_raw(Box::new(PlotuiLayeredLayout {
        layout: plotui_core::LayeredLayout::new(n_nodes, &pairs, dir),
        n_nodes,
        n_edges: pairs.len(),
    }))
}

/// Free a layered layout. NULL is a no-op.
///
/// # Safety
/// `l` must be a pointer from `plotui_layered_layout_new` not yet freed.
#[no_mangle]
pub unsafe extern "C" fn plotui_layered_layout_free(l: *mut PlotuiLayeredLayout) {
    if !l.is_null() {
        drop(Box::from_raw(l));
    }
}

/// How many waypoints the layout produced — the `out_pts` size contract for
/// `plotui_layered_layout_routes`, which cannot be known before the layout
/// has run. A NULL layout counts 0.
///
/// # Safety
/// `l` must be a live layered-layout handle or NULL.
#[no_mangle]
pub unsafe extern "C" fn plotui_layered_layout_route_count(l: *const PlotuiLayeredLayout) -> usize {
    match l.as_ref() {
        Some(l) => l.layout.routes().0.len(),
        None => 0,
    }
}

/// Write the node positions as flat `[x0, y0, x1, …]` into `out_xy` (which
/// must hold `2 * n_nodes` floats) and each node's rank into `out_ranks`
/// (`n_nodes` u32s, or NULL to skip). Feed the positions to
/// `plotui_add_graph2d`.
///
/// # Safety
/// `l` must be a live handle; the outputs must point at enough elements.
#[no_mangle]
pub unsafe extern "C" fn plotui_layered_layout_positions(
    l: *const PlotuiLayeredLayout,
    out_xy: *mut f32,
    out_ranks: *mut u32,
) -> i32 {
    guard(|| {
        let Some(l) = l.as_ref() else {
            set_error("null layout handle");
            return PLOTUI_ERR_NULL;
        };
        if out_xy.is_null() {
            set_error("null output pointer");
            return PLOTUI_ERR_NULL;
        }
        let pts = l.layout.positions();
        let dst = std::slice::from_raw_parts_mut(out_xy, pts.len() * 2);
        for (chunk, p) in dst.as_chunks_mut::<2>().0.iter_mut().zip(pts) {
            chunk.copy_from_slice(p);
        }
        if !out_ranks.is_null() {
            let ranks = l.layout.ranks();
            std::slice::from_raw_parts_mut(out_ranks, ranks.len()).copy_from_slice(ranks);
        }
        let _ = l.n_nodes;
        PLOTUI_OK
    })
}

/// Write the edge waypoints as flat `[x0, y0, x1, …]` into `out_pts` (which
/// must hold `2 * plotui_layered_layout_route_count(l)` floats) and the CSR
/// starts into `out_starts` (one u32 per edge). Both feed
/// `plotui_add_graph2d` and `plotui_set_graph_routes` unchanged.
///
/// # Safety
/// `l` must be a live handle; the outputs must point at enough elements.
#[no_mangle]
pub unsafe extern "C" fn plotui_layered_layout_routes(
    l: *const PlotuiLayeredLayout,
    out_pts: *mut f32,
    out_starts: *mut u32,
) -> i32 {
    guard(|| {
        let Some(l) = l.as_ref() else {
            set_error("null layout handle");
            return PLOTUI_ERR_NULL;
        };
        let (pts, starts) = l.layout.routes();
        if !out_pts.is_null() {
            let dst = std::slice::from_raw_parts_mut(out_pts, pts.len() * 2);
            for (chunk, p) in dst.as_chunks_mut::<2>().0.iter_mut().zip(pts) {
                chunk.copy_from_slice(p);
            }
        } else if !pts.is_empty() {
            set_error("null output pointer");
            return PLOTUI_ERR_NULL;
        }
        if !out_starts.is_null() {
            std::slice::from_raw_parts_mut(out_starts, starts.len()).copy_from_slice(starts);
        } else if l.n_edges > 0 {
            set_error("null output pointer");
            return PLOTUI_ERR_NULL;
        }
        PLOTUI_OK
    })
}

/// Parse DOT, lay the graph out, and write a ready-to-render plot to
/// `out_plot` (free it with `plotui_plot_free`) and its graph trace's handle
/// to `out_handle`. `rankdir` is `"TB"`/`"LR"` or NULL to honour whatever
/// the document says. A parse error returns `PLOTUI_ERR_INVALID_ARG` with
/// the `line:col` message in `plotui_last_error`.
///
/// # Safety
/// Pointer arguments follow the crate conventions.
#[no_mangle]
pub unsafe extern "C" fn plotui_plot_from_dot(
    text: *const c_char,
    rankdir: *const c_char,
    out_plot: *mut *mut PlotuiPlot,
    out_handle: *mut usize,
) -> i32 {
    guard(|| {
        let text = match opt_str(text) {
            Ok(Some(t)) => t,
            Ok(None) => {
                set_error("null DOT text");
                return PLOTUI_ERR_NULL;
            }
            Err(s) => return s,
        };
        let dir = match opt_str(rankdir) {
            Ok(None) => None,
            Ok(Some(s)) => match plotui_bind::parse_rankdir(s) {
                Ok(d) => Some(d),
                Err(e) => return bind_status(e),
            },
            Err(s) => return s,
        };
        if out_plot.is_null() {
            set_error("null output pointer");
            return PLOTUI_ERR_NULL;
        }
        match plotui_bind::plot_from_dot(text, dir) {
            Ok((plot, handle, _)) => {
                *out_plot = new_plot_from(plot);
                if !out_handle.is_null() {
                    *out_handle = handle;
                }
                PLOTUI_OK
            }
            Err(e) => bind_status(e),
        }
    })
}

/// Which nodes are reachable from `from` — upstream (everything that leads
/// to it) or downstream (everything it leads to), including `from` itself.
/// `edges` is `2 * n_edges` u32s as (i, j) pairs; `out_flags` receives
/// `n_nodes` bytes, 1 where reachable. This is the primitive behind "hover a
/// task and light everything it waits on".
///
/// # Safety
/// Pointer arguments follow the crate conventions; `out_flags` must point at
/// `n_nodes` bytes.
#[no_mangle]
pub unsafe extern "C" fn plotui_reachable(
    n_nodes: usize,
    edges: *const u32,
    n_edges: usize,
    from: usize,
    upstream: bool,
    out_flags: *mut u8,
) -> i32 {
    guard(|| {
        let e = match slice(edges, n_edges * 2) {
            Ok(e) => e,
            Err(s) => return s,
        };
        if out_flags.is_null() && n_nodes > 0 {
            set_error("null output pointer");
            return PLOTUI_ERR_NULL;
        }
        let pairs: Vec<(u32, u32)> = e.as_chunks::<2>().0.iter().map(|&[a, b]| (a, b)).collect();
        let flags = plotui_bind::reachable(n_nodes, &pairs, from, upstream);
        if n_nodes > 0 {
            let dst = std::slice::from_raw_parts_mut(out_flags, n_nodes);
            for (d, f) in dst.iter_mut().zip(&flags) {
                *d = u8::from(*f);
            }
        }
        PLOTUI_OK
    })
}
