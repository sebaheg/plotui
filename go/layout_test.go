package plotui

import "testing"

func TestForceLayoutDrivesAGraph(t *testing.T) {
	edges := [][2]uint32{{0, 1}, {1, 2}}
	l := NewForceLayout(3, edges, 7)
	defer l.Close()
	energy := float32(1e9)
	for i := 0; i < 600; i++ {
		energy = l.Step()
	}
	if energy >= 1e-3 {
		t.Fatalf("layout did not settle: energy %v", energy)
	}
	xs, ys, zs := l.Positions()
	if len(xs) != 3 || len(ys) != 3 || len(zs) != 3 {
		t.Fatalf("positions shape: %d/%d/%d", len(xs), len(ys), len(zs))
	}

	p := New()
	defer p.Close()
	h, err := p.AddGraph3D(xs, ys, zs, edges)
	if err != nil {
		t.Fatal(err)
	}
	if err := p.SetGraphPositions(h, xs, ys, zs); err != nil {
		t.Fatal(err)
	}
	if err := p.SetGraphColors(h, []RGB{{9, 250, 9}, {9, 250, 9}, {9, 250, 9}}, []RGB{{250, 9, 9}, {250, 9, 9}}); err != nil {
		t.Fatal(err)
	}
	if err := p.SetGraphColors(h, []RGB{{9, 250, 9}, {9, 250, 9}, {9, 250, 9}}, nil); err != nil {
		t.Fatal(err)
	}

	idx := l.AddNode([]uint32{0})
	if idx != 3 {
		t.Fatalf("AddNode index: %d", idx)
	}
	nxs, nys, nzs := l.Positions()
	if err := p.ExtendGraph(h, nxs[3:], nys[3:], nzs[3:], []RGB{{69, 200, 209}}, [][2]uint32{{0, 3}}); err != nil {
		t.Fatal(err)
	}

	// The shared core error text comes through statusErr verbatim.
	err = p.SetGraphPositions(h, xs, ys, zs) // 3 points for 4 nodes now
	if err == nil || err.Error() != "per-node/per-edge array length must match the trace's node/edge count" {
		t.Fatalf("length mismatch error: %v", err)
	}
}

// pipelineEdges is a three-task chain plus an edge that skips a rank, so
// the layout has both a straight edge and a routed one to hand back.
var pipelineEdges = [][2]uint32{{0, 1}, {1, 2}, {0, 2}}

func TestLayeredLayout(t *testing.T) {
	l, err := NewLayeredLayout(3, pipelineEdges, "TB")
	if err != nil {
		t.Fatal(err)
	}
	defer l.Close()
	xs, ys, ranks := l.Positions()
	if len(xs) != 3 || len(ys) != 3 {
		t.Fatalf("positions shape: %d/%d", len(xs), len(ys))
	}
	if ranks[0] != 0 || ranks[1] != 1 || ranks[2] != 2 {
		t.Fatalf("ranks must follow edge direction: %v", ranks)
	}
	// Data y is up, so a source sits above what it feeds.
	for _, e := range pipelineEdges {
		if ys[e[0]] <= ys[e[1]] {
			t.Fatalf("edge %v points upwards: %v", e, ys)
		}
	}
	routes := l.Routes()
	if len(routes) != len(pipelineEdges) {
		t.Fatalf("one run per edge, got %d", len(routes))
	}
	if len(routes[0]) != 0 || len(routes[1]) != 0 || len(routes[2]) != 1 {
		t.Fatalf("only the rank-skipping edge is routed: %v", routes)
	}

	// Same input, same output — determinism is a hard requirement.
	again, err := NewLayeredLayout(3, pipelineEdges, "TB")
	if err != nil {
		t.Fatal(err)
	}
	defer again.Close()
	xs2, ys2, _ := again.Positions()
	for i := range xs {
		if xs[i] != xs2[i] || ys[i] != ys2[i] {
			t.Fatalf("layout is not deterministic at node %d", i)
		}
	}

	// LR flows left to right: a source sits left of what it feeds.
	lr, err := NewLayeredLayout(3, pipelineEdges, "LR")
	if err != nil {
		t.Fatal(err)
	}
	defer lr.Close()
	lxs, _, _ := lr.Positions()
	for _, e := range pipelineEdges {
		if lxs[e[0]] >= lxs[e[1]] {
			t.Fatalf("edge %v points leftwards in LR: %v", e, lxs)
		}
	}

	// Labels widen the boxes, and the layout keeps siblings apart by that
	// width: two long-named sources in one rank sit further apart than two
	// unlabelled ones.
	fan := [][2]uint32{{0, 2}, {1, 2}}
	bare, err := NewLayeredLayout(3, fan, "TB")
	if err != nil {
		t.Fatal(err)
	}
	defer bare.Close()
	wide, err := NewLayeredLayout(3, fan, "TB", LayoutLabels([]string{"fetch_prices", "fetch_weather", "join"}))
	if err != nil {
		t.Fatal(err)
	}
	defer wide.Close()
	bxs, _, _ := bare.Positions()
	wxs, _, _ := wide.Positions()
	gap := func(xs []float32) float32 {
		d := xs[0] - xs[1]
		if d < 0 {
			d = -d
		}
		return d
	}
	if gap(wxs) <= gap(bxs) {
		t.Fatalf("labelled siblings %v must sit further apart than unlabelled ones %v", wxs, bxs)
	}
	// Spacing is in the same units: more air, wider gap.
	roomy, err := NewLayeredLayout(3, fan, "TB", LayoutLabels([]string{"a", "b", "c"}), LayoutSpacing(10, -1))
	if err != nil {
		t.Fatal(err)
	}
	defer roomy.Close()
	rxs, _, _ := roomy.Positions()
	if gap(rxs) <= gap(bxs) {
		t.Fatalf("a wider nodeSep %v must spread siblings past the default %v", rxs, bxs)
	}
}

func TestAddGraph2D(t *testing.T) {
	l, err := NewLayeredLayout(3, pipelineEdges, "")
	if err != nil {
		t.Fatal(err)
	}
	defer l.Close()
	xs, ys, _ := l.Positions()

	p := New()
	defer p.Close()
	h, err := p.AddGraph2D(xs, ys, pipelineEdges,
		WithLabels([]string{"fetch", "clean", "publish"}),
		WithNodeColors([]RGB{{250, 10, 10}, {10, 250, 10}, {10, 10, 250}}),
		WithNodeShapeNames([]string{"rounded", "box", "ellipse"}),
		WithRoutes(l.Routes()),
		WithName("nightly"))
	if err != nil {
		t.Fatal(err)
	}
	if n := p.NodeCount(); n != 3 {
		t.Fatalf("node count %d", n)
	}
	if p.Is3D() {
		t.Fatal("a 2D graph must not switch the plot to the orbit camera")
	}

	// A node's projected centre is where picking finds it.
	pts := p.ProjectNodes(400, 300)
	if len(pts) != 3 {
		t.Fatalf("projected %d nodes", len(pts))
	}
	for i, pt := range pts {
		el := p.PickElementPx(400, 300, pt[0], pt[1], 0)
		if el == nil || el.Kind != ElementNode || el.Index != i {
			t.Fatalf("node %d picks %v", i, el)
		}
	}

	// A drag moves one node by a pixel delta and reports where it landed;
	// an index that is not a 2D graph node reports nothing.
	if x, y, ok := p.DragNode(400, 300, 1, 30, 0); !ok || x <= xs[1] || y != ys[1] {
		t.Fatalf("DragNode right = (%v, %v, %v), want x > %v at y %v", x, y, ok, xs[1], ys[1])
	}
	if _, _, ok := p.DragNode(400, 300, 99, 1, 1); ok {
		t.Fatal("an out-of-range node must not drag")
	}

	// Relayout: move the nodes, then rewrite the routes.
	if err := p.SetGraphPositions(h, []float32{0, 1, 2}, []float32{2, 1, 0}, []float32{0, 0, 0}); err != nil {
		t.Fatal(err)
	}
	if err := p.SetGraphRoutes(h, l.Routes()); err != nil {
		t.Fatal(err)
	}
	if err := p.SetGraphRoutes(h, nil); err != nil {
		t.Fatal(err)
	}
	if err := p.ExtendGraph(h, []float32{3}, []float32{-1}, []float32{0},
		[]RGB{{80, 80, 80}}, [][2]uint32{{2, 3}}); err != nil {
		t.Fatal(err)
	}
	if n := p.NodeCount(); n != 4 {
		t.Fatalf("node count after extend: %d", n)
	}

	// The chrome tri-state round-trips.
	p.SetShowAxes(true)
	p.SetShowAxes(false)
	p.SetShowAxesAuto()
	if rgba, err := p.RenderRGBA(200, 150); err != nil || len(rgba) == 0 {
		t.Fatalf("the graph must render: %v", err)
	}
}

func TestPlotFromDOT(t *testing.T) {
	p, h, err := PlotFromDOT("digraph nightly { a -> b -> c; a -> c }", "")
	if err != nil {
		t.Fatal(err)
	}
	defer p.Close()
	if h != 0 {
		t.Fatalf("the graph trace is handle %d", h)
	}
	if n := p.NodeCount(); n != 3 {
		t.Fatalf("node count %d", n)
	}
	if rgba, err := p.RenderRGBA(300, 220); err != nil || len(rgba) == 0 {
		t.Fatalf("the parsed graph must render: %v", err)
	}

	// rankdir is overridable: TB spreads along y, LR along x.
	lr, _, err := PlotFromDOT("digraph { a -> b }", "LR")
	if err != nil {
		t.Fatal(err)
	}
	defer lr.Close()
	pts := lr.ProjectNodes(400, 300)
	if pts[0][0] >= pts[1][0] {
		t.Fatalf("LR must run left to right: %v", pts)
	}
}

func TestReachable(t *testing.T) {
	up := Reachable(3, pipelineEdges, 2, true)
	if len(up) != 3 || !up[0] || !up[1] || !up[2] {
		t.Fatalf("everything is upstream of the sink: %v", up)
	}
	down := Reachable(3, pipelineEdges, 2, false)
	if down[0] || down[1] || !down[2] {
		t.Fatalf("the sink leads nowhere: %v", down)
	}
	if got := Reachable(3, pipelineEdges, 99, true); got[0] || got[1] || got[2] {
		t.Fatalf("an out-of-range start reaches nothing: %v", got)
	}
}
