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

// AddGraph2D adds a directed graph in the 2D plane: labelled boxes at
// xs/ys, wired by edges as (from, to) index pairs. This is the pipeline /
// DAG chart — pair it with LayeredLayout for the positions and routes, or
// place the nodes yourself.
//
// Node *centres* are in data coordinates but their boxes are sized in
// pixels from the label, so zooming spreads the graph apart while the text
// stays legible. A plot whose visible 2D traces are all graphs draws no
// axes (see SetShowAxes). Options: WithLabels, WithDirected,
// WithNodeColors, WithNodeShapeNames, WithEdgeColors, WithRoutes,
// WithColor, WithName.
func (p *Plot) AddGraph2D(xs, ys []float32, edges [][2]uint32, opts ...TraceOption) (TraceHandle, error) {
	o, err := applyOpts(traceOpts{directed: true}, opts)
	if err != nil {
		return 0, err
	}
	xp, xn := fptr(xs)
	yp, yn := fptr(ys)

	var ep *C.uint32_t
	if len(edges) > 0 {
		ep = (*C.uint32_t)(unsafe.Pointer(&edges[0][0]))
	}
	var ncp *C.uint8_t
	if len(o.nodeColors) > 0 {
		ncp = (*C.uint8_t)(unsafe.Pointer(&o.nodeColors[0]))
	}
	var ecp *C.uint8_t
	if len(o.edgeColors) > 0 {
		ecp = (*C.uint8_t)(unsafe.Pointer(&o.edgeColors[0]))
	}
	labels, lp := cStrings(o.labels)
	defer freeCStrings(labels)
	shapes, shp := cStrings(o.nodeShapes)
	defer freeCStrings(shapes)

	// Nested routes flatten to the CSR pair the ABI takes, exactly as the
	// other bindings do: interleaved x/y plus one start per edge.
	var flat []float32
	starts := make([]uint32, 0, len(o.routes))
	for _, r := range o.routes {
		starts = append(starts, uint32(len(flat)/2))
		for _, pt := range r {
			flat = append(flat, pt[0], pt[1])
		}
	}
	rp, _ := fptr(flat)
	var sp *C.uint32_t
	if len(starts) > 0 {
		sp = (*C.uint32_t)(unsafe.Pointer(&starts[0]))
	}

	name := cstrOrNil(o.name)
	defer freeCStr(name)
	var h C.size_t
	status := C.plotui_add_graph2d(p.h,
		xp, xn, yp, yn,
		lp, C.size_t(len(labels)),
		ep, C.size_t(len(edges)),
		C.bool(o.directed),
		ncp, C.size_t(len(o.nodeColors)),
		rgbPtr(o.color),
		shp, C.size_t(len(shapes)),
		ecp, C.size_t(len(o.edgeColors)),
		rp, C.size_t(len(flat)/2),
		sp, C.size_t(len(starts)),
		name,
		&h)
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
	shapes, shp := cStrings(o.nodeShapes)
	defer freeCStrings(shapes)

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

// AddMesh3D adds an indexed triangle mesh: vertices at xs/ys/zs, tris as
// [a, b, c] vertex-index triples. Colored by height with "viridis" unless
// WithColormap / WithoutColormap says otherwise. A triangle with an
// out-of-range index is rejected; one with a non-finite vertex is skipped
// at render time, the way a surface cell with a NaN corner is a hole.
func (p *Plot) AddMesh3D(xs, ys, zs []float32, tris [][3]uint32, opts ...TraceOption) (TraceHandle, error) {
	viridis := "viridis"
	o, err := applyOpts(traceOpts{colormap: &viridis}, opts)
	if err != nil {
		return 0, err
	}
	xp, xn := fptr(xs)
	yp, yn := fptr(ys)
	zp, zn := fptr(zs)

	var tp *C.uint32_t
	if len(tris) > 0 {
		tp = (*C.uint32_t)(unsafe.Pointer(&tris[0][0]))
	}
	cm := cstrOrNil(o.colormap)
	defer freeCStr(cm)
	name := cstrOrNil(o.name)
	defer freeCStr(name)
	var h C.size_t
	status := C.plotui_add_mesh3d(p.h, xp, xn, yp, yn, zp, zn,
		tp, C.size_t(len(tris)*3), rgbPtr(o.color), cm, name, &h)
	return TraceHandle(h), statusErr(status)
}

// SetPointStyles styles a 2D scatter point by point. Each slice is
// independent: colors for a categorical or colormapped cloud, sizes for a
// bubble chart, shapes ("disc", "ring", "square", "triangle", "diamond",
// "diamond-open", "dot") for an encoding that survives a palette change. A
// nil or empty slice leaves that channel uniform, and a slice shorter than
// the series styles a prefix of it.
func (p *Plot) SetPointStyles(h TraceHandle, colors []RGB, sizes []float32, shapes []string) error {
	var cp *C.uint8_t
	if len(colors) > 0 {
		cp = (*C.uint8_t)(unsafe.Pointer(&colors[0]))
	}
	sp, sn := fptr(sizes)
	var cshapes []*C.char
	var shp **C.char
	if len(shapes) > 0 {
		cshapes = make([]*C.char, len(shapes))
		for i, s := range shapes {
			cshapes[i] = C.CString(s)
			defer C.free(unsafe.Pointer(cshapes[i]))
		}
		shp = &cshapes[0]
	}
	return statusErr(C.plotui_set_point_styles(p.h, C.size_t(h),
		cp, C.size_t(len(colors)), sp, sn, shp, C.size_t(len(shapes))))
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

// AddBox adds a box plot: groups is one sample per box. Group i sits at
// position i, so SetCategories("x", ...) names the boxes (or "y" with
// WithOrientation("horizontal")).
//
// Boxes span the quartiles with a median line; whiskers reach the furthest
// values within 1.5*IQR, and anything beyond is drawn as its own point rather
// than being swallowed by a longer whisker.
func (p *Plot) AddBox(groups [][]float32, opts ...TraceOption) (TraceHandle, error) {
	o, err := applyOpts(traceOpts{axis: AxisY, orient: "vertical"}, opts)
	if err != nil {
		return 0, err
	}
	if len(groups) == 0 {
		return 0, &Error{Code: ErrInvalidArg,
			Message: "a box plot needs at least one group of values"}
	}
	var values []float32
	starts := make([]uint32, 0, len(groups))
	for _, g := range groups {
		starts = append(starts, uint32(len(values)))
		values = append(values, g...)
	}
	vp, vn := fptr(values)
	var sp *C.uint32_t
	if len(starts) > 0 {
		sp = (*C.uint32_t)(unsafe.Pointer(&starts[0]))
	}
	orient := C.CString(o.orient)
	defer C.free(unsafe.Pointer(orient))
	name := cstrOrNil(o.name)
	defer freeCStr(name)
	axis := C.CString(string(o.axis))
	defer C.free(unsafe.Pointer(axis))
	var h C.size_t
	status := C.plotui_add_box2d(p.h, vp, vn, sp, C.size_t(len(starts)),
		rgbPtr(o.color), orient, name, axis, &h)
	return TraceHandle(h), statusErr(status)
}

// AddBand adds a filled band between lo and hi at each x — a confidence
// interval, a min/max envelope, a tolerance range.
//
// Add it before the line it belongs to: draw order is the only layering in
// 2D, so a band added afterwards paints over its own centre line.
func (p *Plot) AddBand(xs, lo, hi []float32, opts ...TraceOption) (TraceHandle, error) {
	o, err := applyOpts(traceOpts{axis: AxisY}, opts)
	if err != nil {
		return 0, err
	}
	xp, xn := fptr(xs)
	lp, ln := fptr(lo)
	hp, hn := fptr(hi)
	name := cstrOrNil(o.name)
	defer freeCStr(name)
	axis := C.CString(string(o.axis))
	defer C.free(unsafe.Pointer(axis))
	var h C.size_t
	status := C.plotui_add_band2d(p.h, xp, xn, lp, ln, hp, hn,
		rgbPtr(o.color), name, axis, &h)
	return TraceHandle(h), statusErr(status)
}

// SetErrorBars attaches per-point uncertainty to a 2D scatter or line. A nil
// or empty slice clears that axis; a nil minus mirrors plus (the symmetric
// case). Error bars belong to the series: they take its color and stay out of
// the legend, so they cannot drift out of step with the points.
func (p *Plot) SetErrorBars(h TraceHandle, yPlus, yMinus, xPlus, xMinus []float32) error {
	ypp, ypn := fptr(yPlus)
	ymp, ymn := fptr(yMinus)
	xpp, xpn := fptr(xPlus)
	xmp, xmn := fptr(xMinus)
	return statusErr(C.plotui_set_error_bars(p.h, C.size_t(h),
		ypp, ypn, ymp, ymn, xpp, xpn, xmp, xmn))
}

// AddHeatmap adds a grid of cells coloured by value: zs[j][i] is the value
// at (xs[i], ys[j]), the same grid shape AddSurface3D takes. Cells centre on
// their coordinates and tile outward by half a step, so a regular grid meets
// edge to edge; a NaN value leaves a hole rather than a zero.
//
// A colorbar is added by default (WithoutColorbar suppresses it) spanning
// this grid's own range — without one the colors show structure but no
// values. WithColormap picks the ramp ("viridis" by default).
func (p *Plot) AddHeatmap(xs, ys []float32, zs [][]float32, opts ...TraceOption) (TraceHandle, error) {
	viridis := "viridis"
	o, err := applyOpts(traceOpts{colormap: &viridis, colorbar: true}, opts)
	if err != nil {
		return 0, err
	}
	nx, ny := len(xs), len(ys)
	if len(zs) != ny {
		return 0, &Error{Code: ErrInvalidArg, Message: fmt.Sprintf(
			"zs must be a %d×%d grid (len(ys) rows of len(xs) values); got %d rows", ny, nx, len(zs))}
	}
	flat := make([]float32, 0, nx*ny)
	for j, row := range zs {
		if len(row) != nx {
			return 0, &Error{Code: ErrInvalidArg, Message: fmt.Sprintf(
				"zs must be a %d×%d grid (len(ys) rows of len(xs) values); row %d has %d", ny, nx, j, len(row))}
		}
		flat = append(flat, row...)
	}
	xp, xn := fptr(xs)
	yp, yn := fptr(ys)
	zp, zn := fptr(flat)
	cm := cstrOrNil(o.colormap)
	defer freeCStr(cm)
	label := cstrOrNil(o.colorbarLabel)
	defer freeCStr(label)
	name := cstrOrNil(o.name)
	defer freeCStr(name)
	var h C.size_t
	status := C.plotui_add_heatmap2d(p.h, xp, xn, yp, yn, zp, zn,
		cm, C.bool(o.colorbar), label, name, &h)
	return TraceHandle(h), statusErr(status)
}

// AddHistogram adds a histogram of values. WithBins sets a bin count and
// WithBinWidth a fixed width — give one or neither (neither takes the
// Freedman-Diaconis rule, which adapts to spread rather than sample size).
// The raw values are kept, so ExtendValues can add observations later.
//
// Bins are solved once from the whole sample and do not change with zoom:
// edges that shifted while panning would change the shape of the
// distribution under the reader's hands.
func (p *Plot) AddHistogram(values []float32, opts ...TraceOption) (TraceHandle, error) {
	o, err := applyOpts(traceOpts{axis: AxisY}, opts)
	if err != nil {
		return 0, err
	}
	vp, vn := fptr(values)
	name := cstrOrNil(o.name)
	defer freeCStr(name)
	axis := C.CString(string(o.axis))
	defer C.free(unsafe.Pointer(axis))
	var h C.size_t
	status := C.plotui_add_histogram2d(p.h, vp, vn,
		C.size_t(o.bins), C.double(o.binWidth), rgbPtr(o.color), name, axis, &h)
	return TraceHandle(h), statusErr(status)
}

// ExtendValues appends observations to a histogram and rebins. Unlike Extend
// on a coordinate series this is not an O(delta) update: one new value can
// move the range and every bin edge with it.
func (p *Plot) ExtendValues(h TraceHandle, values []float32) error {
	vp, vn := fptr(values)
	return statusErr(C.plotui_extend_values(p.h, C.size_t(h), vp, vn))
}

// SetBarMode sets how several bar series on one axis share their positions:
// "overlay" (the default — each draws at full width, so equal positions
// overplot), "group" (side by side, each taking 1/n of the width), or
// "stack" (each starting where the one below ended).
//
// Stacking accumulates same-signed values only, so a mix of positive and
// negative heights grows both ways from the baseline instead of cancelling
// into a net figure the reader cannot decompose.
func (p *Plot) SetBarMode(mode string) (bool, error) {
	m := C.CString(mode)
	defer C.free(unsafe.Pointer(m))
	var changed C.bool
	status := C.plotui_set_barmode(p.h, m, &changed)
	return bool(changed), statusErr(status)
}

// SetCategories names an axis's categories ("x" or "y"): category i sits at
// position i, and the ticks become one label per category instead of a
// numeric ladder. An empty slice restores numbers. The bool reports whether
// anything changed, so a caller can skip a repaint.
//
// Naming categories does not move the range — traces still place themselves —
// so a series plotted at 0, 1, 2 lines up with the first three names. Pair
// SetCategories("y", ...) with WithOrientation("horizontal") for readable
// long labels.
func (p *Plot) SetCategories(axis string, names []string) (bool, error) {
	ax := C.CString(axis)
	defer C.free(unsafe.Pointer(ax))
	var cnames []*C.char
	var np **C.char
	if len(names) > 0 {
		cnames = make([]*C.char, len(names))
		for i, s := range names {
			cnames[i] = C.CString(s)
			defer C.free(unsafe.Pointer(cnames[i]))
		}
		np = &cnames[0]
	}
	var changed C.bool
	status := C.plotui_set_categories(p.h, ax, np, C.size_t(len(names)), &changed)
	return bool(changed), statusErr(status)
}

// AddStep adds a 2D step series: the right-angle path between samples
// rather than the straight one. Use it for anything that holds its value
// between samples — counters, states, prices — where a straight segment
// would draw a transition that never happened. WithStep picks the corner
// ("post" by default, or "pre" / "mid").
func (p *Plot) AddStep(xs, ys []float32, opts ...TraceOption) (TraceHandle, error) {
	o, err := applyOpts(traceOpts{width: 2.0, axis: AxisY, step: "post"}, opts)
	if err != nil {
		return 0, err
	}
	where := C.CString(o.step)
	defer C.free(unsafe.Pointer(where))
	return p.add2D(xs, ys, o, func(xp *C.float, xn C.size_t, yp *C.float, yn C.size_t, rgb *C.uint8_t, name, axis *C.char, h *C.size_t) C.int32_t {
		return C.plotui_add_step2d(p.h, xp, xn, yp, yn, rgb, C.float(o.width), where, name, axis, h)
	})
}

// AddBar adds a 2D bar series: bars at xs rising from zero to heights.
func (p *Plot) AddBar(xs, heights []float32, opts ...TraceOption) (TraceHandle, error) {
	o, err := applyOpts(traceOpts{axis: AxisY, orient: "vertical"}, opts)
	if err != nil {
		return 0, err
	}
	orient := C.CString(o.orient)
	defer C.free(unsafe.Pointer(orient))
	return p.add2D(xs, heights, o, func(xp *C.float, xn C.size_t, yp *C.float, yn C.size_t, rgb *C.uint8_t, name, axis *C.char, h *C.size_t) C.int32_t {
		return C.plotui_add_bar2d_oriented(p.h, xp, xn, yp, yn, rgb, orient, name, axis, h)
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

// cStrings marshals a Go string slice into a NULL-terminated-string array
// for the ABI, returning the owned pointers (free them with freeCStrings)
// and the array pointer to pass. An empty slice yields a nil array.
func cStrings(ss []string) ([]*C.char, **C.char) {
	if len(ss) == 0 {
		return nil, nil
	}
	out := make([]*C.char, len(ss))
	for i, s := range ss {
		out[i] = C.CString(s)
	}
	return out, &out[0]
}

func freeCStrings(ss []*C.char) {
	for _, s := range ss {
		C.free(unsafe.Pointer(s))
	}
}
