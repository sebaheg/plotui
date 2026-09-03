// Package plotui — Go bindings for the plotui terminal plotting engine.
//
// The engine is the same Rust core behind the Python/Textual and Ratatui
// frontends, linked statically through the plotui-ffi C ABI. Build the
// library once with `cargo build -p plotui-ffi --release` at the repo root,
// then `go build` here links it in; the resulting binary has no runtime
// dependency on the Rust artifact.
//
// A Plot is not thread-safe: use it from one goroutine at a time (Bubble
// Tea's Update/View already guarantee that).
package plotui

/*
#cgo CFLAGS: -I${SRCDIR}/../crates/plotui-ffi/include
#cgo LDFLAGS: ${SRCDIR}/../target/release/libplotui_ffi.a
#cgo linux LDFLAGS: -lm -ldl -lpthread
#include <plotui.h>
#include <stdlib.h>
*/
import "C"

import (
	"runtime"
	"unsafe"
)

// RGB is a color triple, matching the engine's (r, g, b) byte tuples.
type RGB struct{ R, G, B uint8 }

// TraceHandle names a trace for Extend / SetVisible.
type TraceHandle int

// Plot is a handle to an engine-side plot: data + camera + a Kitty image id
// of its own (so several plots never clobber each other's images).
type Plot struct{ h *C.PlotuiPlot }

// New creates an empty plot. Close it when done; a finalizer covers leaks.
func New() *Plot {
	p := &Plot{h: C.plotui_new()}
	runtime.SetFinalizer(p, (*Plot).Close)
	return p
}

// Close frees the engine-side plot. Safe to call twice.
func (p *Plot) Close() {
	if p.h != nil {
		C.plotui_free(p.h)
		p.h = nil
		runtime.SetFinalizer(p, nil)
	}
}

// ParseColor parses a color shorthand — "#rrggbb" (or bare "rrggbb") hex,
// or a name like "red" — with the shared engine rule, so the accepted
// names and the error message match every other binding.
func ParseColor(s string) (RGB, error) {
	cs := C.CString(s)
	defer C.free(unsafe.Pointer(cs))
	var rgb RGB
	status := C.plotui_parse_color(cs, (*C.uint8_t)(unsafe.Pointer(&rgb)))
	return rgb, statusErr(status)
}

// SetColorway swaps the color sequence assigned to traces added without an
// explicit color; it must contain at least one color. Traces already added
// keep the colors they resolved to.
func (p *Plot) SetColorway(colors []RGB) error {
	var cp *C.uint8_t
	if len(colors) > 0 {
		cp = (*C.uint8_t)(unsafe.Pointer(&colors[0]))
	}
	return statusErr(C.plotui_set_colorway(p.h, cp, C.size_t(len(colors))))
}

// SetColorwayName swaps the color sequence to a built-in colorway:
// "plotui" (the default), "muted", or "vivid".
func (p *Plot) SetColorwayName(name string) error {
	cs := C.CString(name)
	defer C.free(unsafe.Pointer(cs))
	return statusErr(C.plotui_set_colorway_name(p.h, cs))
}

// ---- camera (forward your framework's events to these) ----

func (p *Plot) Rotate(dYaw, dPitch float64) { C.plotui_rotate(p.h, C.double(dYaw), C.double(dPitch)) }
func (p *Plot) ZoomBy(factor float64)       { C.plotui_zoom_by(p.h, C.double(factor)) }

// Pan moves the view in framebuffer pixels.
func (p *Plot) Pan(dx, dy float64) { C.plotui_pan(p.h, C.double(dx), C.double(dy)) }
func (p *Plot) Reset()             { C.plotui_reset(p.h) }

// SetInputMap remaps what drag gestures do. Each name is a camera control —
// "yaw", "pitch", "pan_x", "pan_y", "zoom" or "off", optionally prefixed
// with "-" to invert the axis — or "" to keep that axis's current binding.
// The default map is drag = rotate as a trackball (yaw/pitch, the drag
// grabs the object), shift-drag = pan; "-yaw"/"-pitch" restore camera-grab
// rotation.
func (p *Plot) SetInputMap(dragX, dragY, shiftDragX, shiftDragY string) error {
	opt := func(s string) *C.char {
		if s == "" {
			return nil
		}
		return C.CString(s)
	}
	cdx, cdy, csx, csy := opt(dragX), opt(dragY), opt(shiftDragX), opt(shiftDragY)
	defer func() {
		for _, c := range []*C.char{cdx, cdy, csx, csy} {
			if c != nil {
				C.free(unsafe.Pointer(c))
			}
		}
	}()
	return statusErr(C.plotui_set_input_map(p.h, cdx, cdy, csx, csy))
}

// ApplyDrag routes a drag through the input map (see SetInputMap): (dx, dy)
// pointer deltas in whatever unit the scales are calibrated for —
// rotateScale radians per unit, pan*Scale framebuffer pixels per unit,
// zoomScale log-zoom per unit.
func (p *Plot) ApplyDrag(dx, dy float64, shift bool, rotateScale, panXScale, panYScale, zoomScale float64) {
	s := C.int(0)
	if shift {
		s = 1
	}
	C.plotui_apply_drag(p.h, C.double(dx), C.double(dy), s,
		C.double(rotateScale), C.double(panXScale), C.double(panYScale), C.double(zoomScale))
}

// CameraState returns (yaw, pitch, zoom, panX, panY) — capture it before
// rebuilding a plot so the restored view is seamless.
func (p *Plot) CameraState() [5]float64 {
	var out [5]float64
	C.plotui_camera_state(p.h, (*C.double)(unsafe.Pointer(&out[0])))
	return out
}

// SetCameraState restores a state captured by CameraState (values clamped
// the same way the incremental mutators clamp).
func (p *Plot) SetCameraState(s [5]float64) {
	C.plotui_set_camera_state(p.h, C.double(s[0]), C.double(s[1]), C.double(s[2]), C.double(s[3]), C.double(s[4]))
}

// SetBounds pins the 3D data frame to (lo, hi) corners; pass nil, nil to
// restore auto-fit.
func (p *Plot) SetBounds(lo, hi *[3]float32) {
	var lp, hp *C.float
	if lo != nil {
		lp = (*C.float)(unsafe.Pointer(&lo[0]))
	}
	if hi != nil {
		hp = (*C.float)(unsafe.Pointer(&hi[0]))
	}
	C.plotui_set_bounds(p.h, lp, hp)
}

// SetShowBox shows or hides the 3D orientation cube.
func (p *Plot) SetShowBox(show bool) { C.plotui_set_show_box(p.h, C.bool(show)) }

// SetShowAxes pins the 2D chrome — grid, axis rules and tick labels — on or
// off. Use SetShowAxesAuto to hand the decision back to the engine, which is
// the default: a frame whose visible 2D traces are all graphs draws none of
// it, because a pipeline's coordinates are a layout rather than
// measurements. The legend, colorbar, range slider and crosshair are
// unaffected either way, and 3D plots ignore it.
func (p *Plot) SetShowAxes(show bool) {
	v := C.int32_t(0)
	if show {
		v = 1
	}
	C.plotui_set_show_axes(p.h, v)
}

// SetShowAxesAuto restores the automatic rule described on SetShowAxes.
func (p *Plot) SetShowAxesAuto() { C.plotui_set_show_axes(p.h, -1) }

// PlotFromDOT parses a DOT document, lays the graph out, and returns a
// ready-to-render plot whose graph trace is handle 0. rankdir overrides the
// document's own ("TB" or "LR"); an empty string honours whatever it says.
//
// The accepted grammar is a subset: node and edge statements, chains
// (a -> b -> c), braced fan-outs (a -> {b c}), subgraphs (contents hoisted,
// grouping ignored), node/edge/graph attribute defaults, rankdir, and
// label / color / fillcolor / shape / style=rounded on nodes with color on
// edges. Unknown attributes are ignored; HTML labels, node ports and a
// mismatched edge operator are an error naming the line and column.
func PlotFromDOT(text, rankdir string) (*Plot, TraceHandle, error) {
	ctext := C.CString(text)
	defer C.free(unsafe.Pointer(ctext))
	var cdir *C.char
	if rankdir != "" {
		cdir = C.CString(rankdir)
		defer C.free(unsafe.Pointer(cdir))
	}
	var h C.size_t
	var handle *C.PlotuiPlot
	if err := statusErr(C.plotui_plot_from_dot(ctext, cdir, &handle, &h)); err != nil {
		return nil, 0, err
	}
	p := &Plot{h: handle}
	runtime.SetFinalizer(p, (*Plot).Close)
	return p, TraceHandle(h), nil
}

// Chrome recolors the non-data chrome; nil fields keep their current value.
type Chrome struct{ BG, Frame, Grid, Ink, InkBright *RGB }

func (p *Plot) SetChrome(c Chrome) {
	C.plotui_set_chrome(p.h, rgbPtr(c.BG), rgbPtr(c.Frame), rgbPtr(c.Grid), rgbPtr(c.Ink), rgbPtr(c.InkBright))
}

// ---- info ----

// Is3D reports whether any trace is 3D (the orbit-camera path).
func (p *Plot) Is3D() bool { return bool(C.plotui_is_3d(p.h)) }

// NodeCount is the number of pickable nodes across all traces.
func (p *Plot) NodeCount() int { return int(C.plotui_node_count(p.h)) }

// VertexCount counts every drawn 3D vertex — the load metric for the
// reduced-resolution interaction policy.
func (p *Plot) VertexCount() int { return int(C.plotui_vertex_count(p.h)) }

// ImageID is this plot's Kitty image id.
func (p *Plot) ImageID() uint32 { return uint32(C.plotui_image_id(p.h)) }

// ProjectNodes projects every node (flat-index order, matching PickPx) to
// screen space for a pxW×pxH framebuffer: (xPx, yPx, depth) per node.
func (p *Plot) ProjectNodes(pxW, pxH int) [][3]float32 {
	n := p.NodeCount()
	if n == 0 {
		return nil
	}
	out := make([][3]float32, n)
	C.plotui_project_nodes(p.h, C.size_t(pxW), C.size_t(pxH), (*C.float)(unsafe.Pointer(&out[0][0])))
	return out
}

// InteractiveScale is the shared half-resolution policy: the configured
// scale only for large 3D plots while interacting, else 1.0.
func (p *Plot) InteractiveScale(interacting bool, configured float64) float64 {
	return float64(C.plotui_interactive_scale(p.h, C.bool(interacting), C.double(configured)))
}

// ---- small cgo helpers ----

func fptr(s []float32) (*C.float, C.size_t) {
	if len(s) == 0 {
		return nil, 0
	}
	return (*C.float)(unsafe.Pointer(&s[0])), C.size_t(len(s))
}

func rgbPtr(c *RGB) *C.uint8_t {
	if c == nil {
		return nil
	}
	return (*C.uint8_t)(unsafe.Pointer(c))
}

func cstrOrNil(s *string) *C.char {
	if s == nil {
		return nil
	}
	return C.CString(*s)
}

func freeCStr(s *C.char) {
	if s != nil {
		C.free(unsafe.Pointer(s))
	}
}

// takeString copies and frees a Rust-allocated string.
func takeString(s *C.char) string {
	if s == nil {
		return ""
	}
	out := C.GoString(s)
	C.plotui_string_free(s)
	return out
}
