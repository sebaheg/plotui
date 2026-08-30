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
