package plotui

import (
	"fmt"
	"strings"
	"testing"
)

func plot2D(t *testing.T) *Plot {
	t.Helper()
	p := New()
	t.Cleanup(p.Close)
	if _, err := p.AddLine([]float32{0, 1, 2}, []float32{0, 2, 1}); err != nil {
		t.Fatal(err)
	}
	return p
}

func drawnCount(t *testing.T, p *Plot, w, h int) int {
	t.Helper()
	rgba, err := p.RenderRGBA(w, h)
	if err != nil {
		t.Fatal(err)
	}
	if len(rgba) != w*h*4 {
		t.Fatalf("rgba length %d, want %d", len(rgba), w*h*4)
	}
	n := 0
	for i := 3; i < len(rgba); i += 4 {
		if rgba[i] != 0 {
			n++
		}
	}
	return n
}

func TestRenderRGBAStructure(t *testing.T) {
	p := plot2D(t)
	drawn := drawnCount(t, p, 160, 120)
	if drawn == 0 {
		t.Fatal("nothing drawn")
	}
	if drawn == 160*120 {
		t.Fatal("everything drawn — undrawn pixels must keep alpha 0")
	}
}

func TestKittyEscapeStructure(t *testing.T) {
	p := plot2D(t)
	s, err := p.RenderKitty(20, 10, 8, 16, RenderOpts{})
	if err != nil {
		t.Fatal(err)
	}
	for _, want := range []string{"\x1b[s\x1b_G", "q=2", "s=160,v=160", "c=20,r=10",
		fmt.Sprintf("i=%d", p.ImageID())} {
		if !strings.Contains(s, want) {
			t.Errorf("escape missing %q", want)
		}
	}
	if !strings.HasSuffix(s, "\x1b[u") {
		t.Error("escape must restore the cursor")
	}

	compat, err := p.RenderKitty(20, 10, 8, 16, RenderOpts{CompatChunks: true, Replace: true})
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(compat, "a=d") {
		t.Error("Replace must skip the delete-before-transmit")
	}
}

func TestPlaceholderMeta(t *testing.T) {
	p := plot2D(t)
	ph, err := p.RenderPlaceholder(12, 6, 8, 16, 1.0)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(ph.Transmit, "U=1") {
		t.Error("transmit must be a virtual placement")
	}
	id := p.ImageID()
	if !strings.Contains(ph.Transmit, fmt.Sprintf("i=%d,", id)) {
		t.Errorf("transmit must carry image id %d", id)
	}
	want := RGB{uint8(id >> 16), uint8(id >> 8), uint8(id)}
	if ph.IDColor != want {
		t.Errorf("IDColor %v, want %v (low 24 bits of the id)", ph.IDColor, want)
	}
	if len(ph.Cells) != 6 || len(ph.Cells[0]) != 12 {
		t.Fatalf("cells shape %dx%d, want 6x12", len(ph.Cells), len(ph.Cells[0]))
	}
	for _, row := range ph.Cells {
		for _, cell := range row {
			if []rune(cell)[0] != PlaceholderRune {
				t.Fatalf("cell %q must start with the placeholder rune", cell)
			}
		}
	}
	// Distinct column diacritics along a row (self-addressed cells).
	seen := map[string]bool{}
	for _, cell := range ph.Cells[0] {
		seen[cell] = true
	}
	if len(seen) != 12 {
		t.Errorf("cells in a row must be distinct, got %d unique of 12", len(seen))
	}
}

func TestCameraRoundtripAndReset(t *testing.T) {
	p := plot2D(t)
	before := p.CameraState()
	p.Rotate(0.25, 0.1)
	p.ZoomBy(1.5)
	p.Pan(10, -5)
	after := p.CameraState()
	if after == before {
		t.Fatal("camera must move")
	}
	p.SetCameraState(before)
	if p.CameraState() != before {
		t.Fatal("SetCameraState must restore the exact state")
	}
	p.ZoomBy(2.0)
	p.Reset()
	if p.CameraState()[2] != 1.0 {
		t.Fatal("reset must restore zoom 1.0")
	}
}

func TestZoomChangesPixels(t *testing.T) {
	p := plot2D(t)
	a, _ := p.RenderRGBA(120, 90)
	p.ZoomBy(1.5)
	b, _ := p.RenderRGBA(120, 90)
	if string(a) == string(b) {
		t.Fatal("zoom must change the frame")
	}
}

func TestExtendMatchesOneShot(t *testing.T) {
	oneShot := New()
	defer oneShot.Close()
	if _, err := oneShot.AddLine([]float32{0, 1, 2, 3}, []float32{0, 2, 1, 3}); err != nil {
		t.Fatal(err)
	}
	streamed := New()
	defer streamed.Close()
	h, err := streamed.AddLine([]float32{0, 1}, []float32{0, 2})
	if err != nil {
		t.Fatal(err)
	}
	if err := streamed.Extend(h, []float32{2, 3}, []float32{1, 3}); err != nil {
		t.Fatal(err)
	}
	a, _ := oneShot.RenderRGBA(120, 90)
	b, _ := streamed.RenderRGBA(120, 90)
	if string(a) != string(b) {
		t.Fatal("extend must render exactly like the one-shot plot")
	}
}

func TestErrorStringsMatchTheSharedContract(t *testing.T) {
	p := plot2D(t)
	cases := []struct {
		got  error
		want string
	}{
		{p.Extend(99, []float32{0}, []float32{0}), "unknown trace handle 99"},
		{p.Extend3D(0, []float32{0}, []float32{0}, []float32{0}),
			"2D trace: extend takes (xs, ys) — zs is for 3D traces"},
	}
	if _, err := p.AddLine([]float32{0}, []float32{0}, WithAxis("y4")); err != nil {
		cases = append(cases, struct {
			got  error
			want string
		}{err, `axis must be 'y', 'y2' or 'y3', got "y4"`})
	} else {
		t.Error("bad axis must error")
	}
	for _, c := range cases {
		if c.got == nil || c.got.Error() != c.want {
			t.Errorf("error %v, want %q", c.got, c.want)
		}
	}

	if _, err := p.AddGraph3D([]float32{0}, []float32{0}, []float32{0}, nil,
		WithNodeShapes([]string{"blob"})); err == nil ||
		!strings.HasPrefix(err.Error(), `unknown node shape "blob"`) {
		t.Errorf("bad shape error = %v", err)
	}
}

func TestSetVisibleChangeDetection(t *testing.T) {
	p := plot2D(t)
	changed, err := p.SetVisible(0, false)
	if err != nil || !changed {
		t.Fatalf("first hide: changed=%v err=%v", changed, err)
	}
	changed, err = p.SetVisible(0, false)
	if err != nil || changed {
		t.Fatalf("second hide: changed=%v err=%v", changed, err)
	}
	if _, err = p.SetVisible(9, true); err == nil {
		t.Fatal("unknown handle must error")
	}
}

func TestPickAndHover3D(t *testing.T) {
	p := New()
	defer p.Close()
	n := 30
	xs := make([]float32, n)
	ys := make([]float32, n)
	zs := make([]float32, n)
	for i := range xs {
		xs[i] = float32(i % 5)
		ys[i] = float32(i / 5)
		zs[i] = float32(i % 3)
	}
	if _, err := p.AddScatter3D(xs, ys, zs); err != nil {
		t.Fatal(err)
	}
	if !p.Is3D() || p.NodeCount() != n || p.VertexCount() != n {
		t.Fatalf("counts: is3d=%v nodes=%d vertices=%d", p.Is3D(), p.NodeCount(), p.VertexCount())
	}
	projected := p.ProjectNodes(160, 160)
	if len(projected) != n {
		t.Fatalf("projected %d nodes, want %d", len(projected), n)
	}
	// Picking at a projected node must find one.
	el := p.PickElementPx(160, 160, projected[0][0], projected[0][1], 16)
	if el == nil || el.Kind != ElementNode {
		t.Fatalf("pick at a node's projection = %v", el)
	}
	if changed := p.SetHovered(el); !changed {
		t.Fatal("first hover must report a change")
	}
	if changed := p.SetHovered(el); changed {
		t.Fatal("same hover must not report a change")
	}
	if idx, ok := p.PickPx(160, 160, projected[0][0], projected[0][1], 16); !ok || idx != el.Index {
		t.Fatalf("PickPx = (%d, %v), want (%d, true)", idx, ok, el.Index)
	}
}

func TestInteractiveScalePolicy(t *testing.T) {
	p := New()
	defer p.Close()
	n := 500
	coords := make([]float32, n)
	for i := range coords {
		coords[i] = float32(i)
	}
	if _, err := p.AddScatter3D(coords, coords, coords); err != nil {
		t.Fatal(err)
	}
	if got := p.InteractiveScale(true, 0.5); got != 0.5 {
		t.Errorf("large 3D plot mid-interaction: scale %v, want 0.5", got)
	}
	if got := p.InteractiveScale(false, 0.5); got != 1.0 {
		t.Errorf("still plot: scale %v, want 1.0", got)
	}
}

func TestSurfaceGridValidation(t *testing.T) {
	p := New()
	defer p.Close()
	if _, err := p.AddSurface3D([]float32{0, 1}, []float32{0, 1, 2},
		[][]float32{{1, 2}, {3, 4}, {5, 6}}); err != nil {
		t.Fatalf("valid grid: %v", err)
	}
	if _, err := p.AddSurface3D([]float32{0, 1}, []float32{0, 1, 2},
		[][]float32{{1, 2}}); err == nil || !strings.HasPrefix(err.Error(), "zs must be a 3×2 grid") {
		t.Errorf("ragged grid error = %v", err)
	}
	if _, err := p.AddSurface3D([]float32{0, 1}, []float32{0, 1},
		[][]float32{{1, 2}, {3, 4}}, WithColormap("heat")); err == nil ||
		!strings.HasPrefix(err.Error(), `unknown colormap "heat"`) {
		t.Errorf("bad colormap error = %v", err)
	}
}

func TestDistinctImageIDsAndCleanup(t *testing.T) {
	a, b := New(), New()
	defer a.Close()
	defer b.Close()
	if a.ImageID() == b.ImageID() {
		t.Fatal("plots must get distinct image ids")
	}
	want := fmt.Sprintf("\x1b_Ga=d,d=i,i=%d\x1b\\", a.ImageID())
	if got := a.KittyCleanup(); got != want {
		t.Fatalf("cleanup %q, want %q", got, want)
	}
}

func TestTmuxWrapOutsideTmux(t *testing.T) {
	t.Setenv("TMUX", "")
	if got := TmuxWrap("\x1b_Gx\x1b\\"); got != "\x1b_Gx\x1b\\" {
		t.Fatalf("outside tmux must be a no-op, got %q", got)
	}
}
