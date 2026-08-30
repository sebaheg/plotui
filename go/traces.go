package plotui

/*
#include <plotui.h>
#include <stdlib.h>
*/
import "C"

import (
	"fmt"
	"unsafe"
)

// AddScatter3D adds a 3D scatter series (default size 3.0; omitted colors
// take colorway slots in fixed order). xs/ys/zs pair up to the shortest
// length.
func (p *Plot) AddScatter3D(xs, ys, zs []float32, opts ...TraceOption) (TraceHandle, error) {
	o, err := applyOpts(traceOpts{size: 3.0}, opts)
	if err != nil {
		return 0, err
	}
	xp, xn := fptr(xs)
	yp, yn := fptr(ys)
	zp, zn := fptr(zs)
	name := cstrOrNil(o.name)
	defer freeCStr(name)
	var h C.size_t
	status := C.plotui_add_scatter3d(p.h, xp, xn, yp, yn, zp, zn, rgbPtr(o.color), C.float(o.size), name, &h)
	return TraceHandle(h), statusErr(status)
}

// AddGraph3D adds a 3D graph: nodes at xs/ys/zs, edges as (i, j) index
// pairs. Default size 3.5; an omitted uniform color takes the next
// colorway slot.
func (p *Plot) AddGraph3D(xs, ys, zs []float32, edges [][2]uint32, opts ...TraceOption) (TraceHandle, error) {
	o, err := applyOpts(traceOpts{size: 3.5}, opts)
	if err != nil {
		return 0, err
	}
	xp, xn := fptr(xs)
	yp, yn := fptr(ys)
	zp, zn := fptr(zs)

	var ep *C.uint32_t
	if len(edges) > 0 {
		ep = (*C.uint32_t)(unsafe.Pointer(&edges[0][0]))
	}
	var ncp *C.uint8_t
	if len(o.nodeColors) > 0 {
		ncp = (*C.uint8_t)(unsafe.Pointer(&o.nodeColors[0]))
	}
	nsp, nsn := fptr(o.nodeSizes)
	var ecp *C.uint8_t
	if len(o.edgeColors) > 0 {
		ecp = (*C.uint8_t)(unsafe.Pointer(&o.edgeColors[0]))
	}
	var shapes []*C.char
	var shp **C.char
	if len(o.nodeShapes) > 0 {
		shapes = make([]*C.char, len(o.nodeShapes))
		for i, s := range o.nodeShapes {
			shapes[i] = C.CString(s)
			defer C.free(unsafe.Pointer(shapes[i]))
		}
		shp = &shapes[0]
	}

	name := cstrOrNil(o.name)
	defer freeCStr(name)
	var h C.size_t
	status := C.plotui_add_graph3d(p.h,
		xp, xn, yp, yn, zp, zn,
		ep, C.size_t(len(edges)),
		ncp, C.size_t(len(o.nodeColors)),
		rgbPtr(o.color), C.float(o.size),
		nsp, nsn,
		ecp, C.size_t(len(o.edgeColors)),
		shp, C.size_t(len(shapes)),
		name,
		&h)
	return TraceHandle(h), statusErr(status)
}

// AddLine3D adds a 3D polyline (default width 2.0; omitted colors take
// colorway slots in fixed order).
func (p *Plot) AddLine3D(xs, ys, zs []float32, opts ...TraceOption) (TraceHandle, error) {
	o, err := applyOpts(traceOpts{width: 2.0}, opts)
	if err != nil {
		return 0, err
	}
	xp, xn := fptr(xs)
	yp, yn := fptr(ys)
	zp, zn := fptr(zs)
	name := cstrOrNil(o.name)
	defer freeCStr(name)
	var h C.size_t
	status := C.plotui_add_line3d(p.h, xp, xn, yp, yn, zp, zn, rgbPtr(o.color), C.float(o.width), name, &h)
	return TraceHandle(h), statusErr(status)
}

// AddSurface3D adds a grid surface: zs[j][i] is the height at (xs[i],
// ys[j]). Colored by height with "viridis" unless WithColormap /
// WithoutColormap says otherwise.
func (p *Plot) AddSurface3D(xs, ys []float32, zs [][]float32, opts ...TraceOption) (TraceHandle, error) {
	viridis := "viridis"
	o, err := applyOpts(traceOpts{colormap: &viridis}, opts)
	if err != nil {
		return 0, err
	}
	nx, ny := len(xs), len(ys)
	if len(zs) != ny {
		return 0, &Error{Code: ErrInvalidArg, Message: fmt.Sprintf(
			"zs must be a %d×%d grid (len(ys) rows of len(xs) heights); got %d rows", ny, nx, len(zs))}
	}
	flat := make([]float32, 0, nx*ny)
	for j, row := range zs {
		if len(row) != nx {
			return 0, &Error{Code: ErrInvalidArg, Message: fmt.Sprintf(
				"zs must be a %d×%d grid (len(ys) rows of len(xs) heights); row %d has %d", ny, nx, j, len(row))}
		}
		flat = append(flat, row...)
	}
	xp, xn := fptr(xs)
	yp, yn := fptr(ys)
	zp, zn := fptr(flat)
	cm := cstrOrNil(o.colormap)
	defer freeCStr(cm)
	name := cstrOrNil(o.name)
	defer freeCStr(name)
	var h C.size_t
	status := C.plotui_add_surface3d(p.h, xp, xn, yp, yn, zp, zn,
		rgbPtr(o.color), cm, C.bool(o.wireframe), name, &h)
	return TraceHandle(h), statusErr(status)
}

func (p *Plot) add2D(xs, ys []float32, o traceOpts,
	call func(xp *C.float, xn C.size_t, yp *C.float, yn C.size_t, rgb *C.uint8_t, name, axis *C.char, h *C.size_t) C.int32_t,
) (TraceHandle, error) {
	xp, xn := fptr(xs)
	yp, yn := fptr(ys)
	name := cstrOrNil(o.name)
	defer freeCStr(name)
	axis := C.CString(string(o.axis))
	defer freeCStr(axis)
	var h C.size_t
	status := call(xp, xn, yp, yn, rgbPtr(o.color), name, axis, &h)
	return TraceHandle(h), statusErr(status)
}

// AddScatter adds a 2D scatter series (default size 2.5; omitted colors
// take colorway slots in fixed order).
func (p *Plot) AddScatter(xs, ys []float32, opts ...TraceOption) (TraceHandle, error) {
	o, err := applyOpts(traceOpts{size: 2.5, axis: AxisY}, opts)
	if err != nil {
		return 0, err
	}
	return p.add2D(xs, ys, o, func(xp *C.float, xn C.size_t, yp *C.float, yn C.size_t, rgb *C.uint8_t, name, axis *C.char, h *C.size_t) C.int32_t {
		return C.plotui_add_scatter2d(p.h, xp, xn, yp, yn, rgb, C.float(o.size), name, axis, h)
	})
}

// AddLine adds a 2D line series (default width 2.0).
func (p *Plot) AddLine(xs, ys []float32, opts ...TraceOption) (TraceHandle, error) {
	o, err := applyOpts(traceOpts{width: 2.0, axis: AxisY}, opts)
	if err != nil {
		return 0, err
	}
	return p.add2D(xs, ys, o, func(xp *C.float, xn C.size_t, yp *C.float, yn C.size_t, rgb *C.uint8_t, name, axis *C.char, h *C.size_t) C.int32_t {
		return C.plotui_add_line2d(p.h, xp, xn, yp, yn, rgb, C.float(o.width), name, axis, h)
	})
}

// AddBar adds a 2D bar series: bars at xs rising from zero to heights.
func (p *Plot) AddBar(xs, heights []float32, opts ...TraceOption) (TraceHandle, error) {
	o, err := applyOpts(traceOpts{axis: AxisY}, opts)
	if err != nil {
		return 0, err
	}
	return p.add2D(xs, heights, o, func(xp *C.float, xn C.size_t, yp *C.float, yn C.size_t, rgb *C.uint8_t, name, axis *C.char, h *C.size_t) C.int32_t {
		return C.plotui_add_bar2d(p.h, xp, xn, yp, yn, rgb, name, axis, h)
	})
}

// Extend appends points to a 2D trace by handle; the result renders exactly
// as if the concatenated data had been added in one call.
func (p *Plot) Extend(h TraceHandle, xs, ys []float32) error {
	xp, xn := fptr(xs)
	yp, yn := fptr(ys)
	return statusErr(C.plotui_extend(p.h, C.size_t(h), xp, xn, yp, yn, nil, 0))
}

// Extend3D appends points to a 3D scatter/line trace by handle.
func (p *Plot) Extend3D(h TraceHandle, xs, ys, zs []float32) error {
	xp, xn := fptr(xs)
	yp, yn := fptr(ys)
	zp, zn := fptr(zs)
	if zp == nil {
		// A NULL zs means "2D extend" at the ABI, which would misreport the
		// mistake — pin an empty-but-present zs instead.
		var zero float32
		zp = (*C.float)(unsafe.Pointer(&zero))
		zn = 0
	}
	return statusErr(C.plotui_extend(p.h, C.size_t(h), xp, xn, yp, yn, zp, zn))
}

// SetVisible shows or hides a trace by handle; the returned bool says
// whether the state actually changed (a repaint is needed).
func (p *Plot) SetVisible(h TraceHandle, visible bool) (bool, error) {
	var changed C.bool
	status := C.plotui_set_visible(p.h, C.size_t(h), C.bool(visible), &changed)
	return bool(changed), statusErr(status)
}

// SetSelected selects an element (nil clears); it gets the ring/glow
// treatment.
func (p *Plot) SetSelected(el *Element) {
	kind, index := elementParts(el)
	C.plotui_set_selected(p.h, kind, index)
}

// SetHovered hovers an element (nil clears); it lights up white. The bool
// says whether the hover state changed.
func (p *Plot) SetHovered(el *Element) bool {
	kind, index := elementParts(el)
	var changed C.bool
	C.plotui_set_hovered(p.h, kind, index, &changed)
	return bool(changed)
}

// SetHover2D sets the 2D crosshair position in framebuffer pixels (nil
// clears). The bool says whether the state changed.
func (p *Plot) SetHover2D(px *float32) bool {
	if px == nil {
		return bool(C.plotui_set_hover2d(p.h, false, 0))
	}
	return bool(C.plotui_set_hover2d(p.h, true, C.float(*px)))
}

// PickElementPx picks whatever is under pixel (px, py) in a pxW×pxH
// framebuffer: the nearest node within nodeRadius, else the nearest graph
// edge within 0.75×nodeRadius. Returns nil for empty space.
func (p *Plot) PickElementPx(pxW, pxH int, px, py, nodeRadius float32) *Element {
	var kind C.int32_t
	var index C.size_t
	C.plotui_pick_element_px(p.h, C.size_t(pxW), C.size_t(pxH),
		C.float(px), C.float(py), C.float(nodeRadius), C.float(-1.0), &kind, &index)
	if kind == 0 {
		return nil
	}
	return &Element{Kind: ElementKind(kind), Index: int(index)}
}

// PickPx returns the nearest node within radius of (px, py), nodes only.
func (p *Plot) PickPx(pxW, pxH int, px, py, radius float32) (int, bool) {
	var index C.size_t
	found := C.plotui_pick_px(p.h, C.size_t(pxW), C.size_t(pxH),
		C.float(px), C.float(py), C.float(radius), &index)
	return int(index), bool(found)
}

func elementParts(el *Element) (C.int32_t, C.size_t) {
	if el == nil {
		return 0, 0
	}
	return C.int32_t(el.Kind), C.size_t(el.Index)
}
