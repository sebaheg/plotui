package plotui

/*
#include <plotui.h>
#include <stdlib.h>
*/
import "C"

import (
	"runtime"
	"unsafe"
)

// ForceLayout is a 3D force-directed layout: connected nodes attract, all
// nodes repel, a cooling temperature settles the motion. Pure math on the
// host's timer — call Step per tick and hand Positions to
// Plot.SetGraphPositions. Deterministic for a given seed. Like Plot, not
// thread-safe: one goroutine at a time.
type ForceLayout struct {
	h *C.PlotuiLayout
}

// NewForceLayout builds a layout over n nodes with seeded initial positions
// in the unit ball. Edges are (i, j) index pairs.
func NewForceLayout(n int, edges [][2]uint32, seed uint32) *ForceLayout {
	var ep *C.uint32_t
	if len(edges) > 0 {
		ep = (*C.uint32_t)(unsafe.Pointer(&edges[0][0]))
	}
	l := &ForceLayout{h: C.plotui_layout_new(C.size_t(n), ep, C.size_t(len(edges)), C.uint32_t(seed))}
	runtime.SetFinalizer(l, (*ForceLayout).Close)
	return l
}

// Close frees the engine-side layout. Safe to call twice.
func (l *ForceLayout) Close() {
	if l.h != nil {
		C.plotui_layout_free(l.h)
		l.h = nil
		runtime.SetFinalizer(l, nil)
	}
}

// Step runs one simulation tick and returns the mean displacement — stop
// repainting once it drops below ~1e-3.
func (l *ForceLayout) Step() float32 {
	return float32(C.plotui_layout_step(l.h))
}

// NodeCount reports the layout's node count (grows with AddNode).
func (l *ForceLayout) NodeCount() int {
	return int(C.plotui_layout_node_count(l.h))
}

// Positions returns the current node positions as xs, ys, zs slices in
// index order — feed them straight to Plot.SetGraphPositions.
func (l *ForceLayout) Positions() (xs, ys, zs []float32) {
	n := l.NodeCount()
	if n == 0 {
		return nil, nil, nil
	}
	flat := make([]float32, n*3)
	C.plotui_layout_positions(l.h, (*C.float)(unsafe.Pointer(&flat[0])))
	xs, ys, zs = make([]float32, n), make([]float32, n), make([]float32, n)
	for i := 0; i < n; i++ {
		xs[i], ys[i], zs[i] = flat[i*3], flat[i*3+1], flat[i*3+2]
	}
	return xs, ys, zs
}

// AddNode warm-inserts one node connected to neighbors (existing indices):
// it spawns beside its first neighbor and re-heats the simulation. Returns
// the new node's index; pair with Plot.ExtendGraph.
func (l *ForceLayout) AddNode(neighbors []uint32) int {
	var np *C.uint32_t
	if len(neighbors) > 0 {
		np = (*C.uint32_t)(unsafe.Pointer(&neighbors[0]))
	}
	var idx C.size_t
	C.plotui_layout_add_node(l.h, np, C.size_t(len(neighbors)), &idx)
	return int(idx)
}

// LayeredLayout is a hierarchical ("Sugiyama") layout for a directed graph:
// rank the nodes by depth, order each rank to reduce edge crossings, then
// place them so edges run as straight as they can. Solved in the
// constructor — there is nothing to step, because a pipeline has one right
// shape and watching a simulation converge on it says nothing about the
// pipeline.
//
// Feed Positions and Routes straight to Plot.AddGraph2D. Deterministic:
// same input, same output, no randomness anywhere. Like Plot, not
// thread-safe: one goroutine at a time.
type LayeredLayout struct {
	h      *C.PlotuiLayeredLayout
	nNodes int
	nEdges int
}

// NewLayeredLayout lays out n nodes connected by edges as (from, to) index
// pairs, flowing in rankdir — "TB" (sources on top; also the empty-string
// default) or "LR" (sources on the left). Self-loops and out-of-range
// endpoints are kept inert, so an edge list can be passed verbatim from the
// plot; cycles do not hang, since a back edge is reversed for the layout
// only. An unknown rankdir returns the shared parse error.
func NewLayeredLayout(n int, edges [][2]uint32, rankdir string) (*LayeredLayout, error) {
	var ep *C.uint32_t
	if len(edges) > 0 {
		ep = (*C.uint32_t)(unsafe.Pointer(&edges[0][0]))
	}
	var cdir *C.char
	if rankdir != "" {
		cdir = C.CString(rankdir)
		defer C.free(unsafe.Pointer(cdir))
	}
	h := C.plotui_layered_layout_new(C.size_t(n), ep, C.size_t(len(edges)), cdir)
	if h == nil {
		return nil, &Error{Code: -1, Message: C.GoString(C.plotui_last_error())}
	}
	l := &LayeredLayout{h: h, nNodes: n, nEdges: len(edges)}
	runtime.SetFinalizer(l, (*LayeredLayout).Close)
	return l, nil
}

// Close frees the engine-side layout. Safe to call twice.
func (l *LayeredLayout) Close() {
	if l.h != nil {
		C.plotui_layered_layout_free(l.h)
		l.h = nil
		runtime.SetFinalizer(l, nil)
	}
}

// Positions returns the node centres as xs, ys slices in the caller's index
// order, and each node's rank (0 for a source, one more than its deepest
// predecessor otherwise).
func (l *LayeredLayout) Positions() (xs, ys []float32, ranks []uint32) {
	if l.nNodes == 0 {
		return nil, nil, nil
	}
	flat := make([]float32, l.nNodes*2)
	ranks = make([]uint32, l.nNodes)
	C.plotui_layered_layout_positions(l.h,
		(*C.float)(unsafe.Pointer(&flat[0])),
		(*C.uint32_t)(unsafe.Pointer(&ranks[0])))
	xs, ys = make([]float32, l.nNodes), make([]float32, l.nNodes)
	for i := 0; i < l.nNodes; i++ {
		xs[i], ys[i] = flat[i*2], flat[i*2+1]
	}
	return xs, ys, ranks
}

// Routes returns each edge's waypoints — one list of (x, y) points per edge,
// in the caller's edge order and direction, empty for a straight edge. Pass
// it straight to Plot.AddGraph2D via WithRoutes.
func (l *LayeredLayout) Routes() [][][2]float32 {
	if l.nEdges == 0 {
		return nil
	}
	n := int(C.plotui_layered_layout_route_count(l.h))
	flat := make([]float32, n*2)
	starts := make([]uint32, l.nEdges)
	var fp *C.float
	if n > 0 {
		fp = (*C.float)(unsafe.Pointer(&flat[0]))
	}
	C.plotui_layered_layout_routes(l.h, fp, (*C.uint32_t)(unsafe.Pointer(&starts[0])))
	out := make([][][2]float32, l.nEdges)
	for e := 0; e < l.nEdges; e++ {
		a := int(starts[e])
		b := n
		if e+1 < l.nEdges {
			b = int(starts[e+1])
		}
		if a > b || b > n {
			continue
		}
		run := make([][2]float32, 0, b-a)
		for i := a; i < b; i++ {
			run = append(run, [2]float32{flat[i*2], flat[i*2+1]})
		}
		out[e] = run
	}
	return out
}
