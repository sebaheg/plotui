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
