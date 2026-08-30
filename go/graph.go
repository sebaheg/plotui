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
