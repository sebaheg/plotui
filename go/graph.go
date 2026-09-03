package plotui

/*
#include <plotui.h>
#include <stdlib.h>
*/
import "C"

import "unsafe"

// SetGraphPositions moves every node of a graph trace at once — the
// per-frame call of a force-directed layout (pair with ForceLayout). The
// point count (min of xs/ys/zs) must match the trace's node count;
// structure, indices, hover, and selection stay valid.
func (p *Plot) SetGraphPositions(h TraceHandle, xs, ys, zs []float32) error {
	xp, xn := fptr(xs)
	yp, yn := fptr(ys)
	zp, zn := fptr(zs)
	return statusErr(C.plotui_set_graph_positions(p.h, C.size_t(h), xp, xn, yp, yn, zp, zn))
}

// SetGraphRoutes replaces a 2D graph's edge waypoints — the second half of
// a relayout, after SetGraphPositions has moved the nodes. routes is one
// list of (x, y) points per edge (what LayeredLayout.Routes returns); pass
// nil to restore straight edges.
func (p *Plot) SetGraphRoutes(h TraceHandle, routes [][][2]float32) error {
	var flat []float32
	starts := make([]uint32, 0, len(routes))
	for _, r := range routes {
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
	return statusErr(C.plotui_set_graph_routes(p.h, C.size_t(h),
		rp, C.size_t(len(flat)/2), sp, C.size_t(len(starts))))
}

// SetGraphColors recolors a graph trace in place — the host-side highlight
// primitive: dim everything, brighten a hovered dependency path, restore.
// nodeColors needs one color per node; edgeColors one per edge, or nil to
// restore the default dimmed endpoint blend.
func (p *Plot) SetGraphColors(h TraceHandle, nodeColors, edgeColors []RGB) error {
	var ncp *C.uint8_t
	if len(nodeColors) > 0 {
		ncp = (*C.uint8_t)(unsafe.Pointer(&nodeColors[0]))
	}
	var ecp *C.uint8_t
	if len(edgeColors) > 0 {
		ecp = (*C.uint8_t)(unsafe.Pointer(&edgeColors[0]))
	}
	return statusErr(C.plotui_set_graph_colors(p.h, C.size_t(h),
		ncp, C.size_t(len(nodeColors)), ecp, C.size_t(len(edgeColors))))
}

// ExtendGraph appends nodes and edges to a graph trace (pair with
// ForceLayout.AddNode). Edges may reference old or new node indices;
// nodeColors colors the appended nodes (renderer default where missing).
// Same flat-index caveat as Extend3D on a scatter: appending to a graph
// that is not the last node-bearing trace shifts downstream flat indices.
func (p *Plot) ExtendGraph(h TraceHandle, xs, ys, zs []float32, nodeColors []RGB, edges [][2]uint32) error {
	xp, xn := fptr(xs)
	yp, yn := fptr(ys)
	zp, zn := fptr(zs)
	var ncp *C.uint8_t
	if len(nodeColors) > 0 {
		ncp = (*C.uint8_t)(unsafe.Pointer(&nodeColors[0]))
	}
	var ep *C.uint32_t
	if len(edges) > 0 {
		ep = (*C.uint32_t)(unsafe.Pointer(&edges[0][0]))
	}
	return statusErr(C.plotui_extend_graph(p.h, C.size_t(h),
		xp, xn, yp, yn, zp, zn,
		ncp, C.size_t(len(nodeColors)),
		ep, C.size_t(len(edges))))
}

// Reachable reports which of n nodes are reachable from node i by following
// edges — upstream (everything that leads to it) or downstream (everything
// it leads to) — including i itself. This is the primitive behind "hover a
// task and light everything it waits on": pair it with SetGraphColors.
func Reachable(n int, edges [][2]uint32, i int, upstream bool) []bool {
	if n <= 0 {
		return nil
	}
	var ep *C.uint32_t
	if len(edges) > 0 {
		ep = (*C.uint32_t)(unsafe.Pointer(&edges[0][0]))
	}
	flags := make([]uint8, n)
	C.plotui_reachable(C.size_t(n), ep, C.size_t(len(edges)), C.size_t(i),
		C.bool(upstream), (*C.uint8_t)(unsafe.Pointer(&flags[0])))
	out := make([]bool, n)
	for j, f := range flags {
		out[j] = f != 0
	}
	return out
}
