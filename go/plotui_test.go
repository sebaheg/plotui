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

func TestAddBox(t *testing.T) {
	p := New()
	defer p.Close()
	a := []float32{1, 2, 3, 4, 5, 6, 7, 8, 9, 100}
	b := []float32{2, 4, 6, 8, 10, 12, 14, 16, 18}
	h, err := p.AddBox([][]float32{a, b}, WithName("groups"))
	if err != nil {
		t.Fatalf("AddBox: %v", err)
	}
	if _, err := p.SetCategories("x", []string{"a", "b"}); err != nil {
		t.Fatal(err)
	}
	if drawnCount(t, p, 320, 240) == 0 {
		t.Error("box plot drew nothing")
	}
	if _, err := p.AddBox(nil); err == nil || !strings.Contains(err.Error(), "at least one group") {
		t.Errorf("empty groups error = %v", err)
	}
	if _, err := p.AddBox([][]float32{a}, WithOrientation("sideways")); err == nil ||
		!strings.Contains(err.Error(), "orientation") {
		t.Errorf("bad orientation error = %v", err)
	}
	if err := p.Extend(h, []float32{1}, []float32{1}); err == nil ||
		!strings.Contains(err.Error(), "rebuild the plot") {
		t.Errorf("structural error = %v", err)
	}
}

func TestBandAndErrorBars(t *testing.T) {
	p := New()
	defer p.Close()
	xs := []float32{0, 1, 2}
	if _, err := p.AddBand(xs, []float32{1, 0, 1}, []float32{4, 5, 4}, WithName("ci")); err != nil {
		t.Fatalf("AddBand: %v", err)
	}
	h, err := p.AddScatter([]float32{1, 2}, []float32{1, 1}, WithColor(RGB{255, 0, 0}))
	if err != nil {
		t.Fatal(err)
	}
	// Count the series colour, not every drawn pixel: widening the axis
	// rescales the whole frame, so totals move for unrelated reasons.
	red := func() int {
		rgba, err := p.RenderRGBA(240, 180)
		if err != nil {
			t.Fatal(err)
		}
		n := 0
		for i := 0; i+3 < len(rgba); i += 4 {
			if rgba[i+3] != 0 && rgba[i] == 255 && rgba[i+1] == 0 && rgba[i+2] == 0 {
				n++
			}
		}
		return n
	}
	before := red()
	if err := p.SetErrorBars(h, []float32{3, 3}, nil, nil, nil); err != nil {
		t.Fatalf("SetErrorBars: %v", err)
	}
	if red() <= before {
		t.Error("error bars must add drawn area")
	}
	bars, _ := p.AddBar([]float32{0}, []float32{1})
	if err := p.SetErrorBars(bars, []float32{1}, nil, nil, nil); err == nil ||
		!strings.Contains(err.Error(), "scatter and line") {
		t.Errorf("wrong-kind error = %v", err)
	}
}

func TestBarMode(t *testing.T) {
	p := New()
	defer p.Close()
	if _, err := p.AddBar([]float32{0, 1}, []float32{3, 4}); err != nil {
		t.Fatal(err)
	}
	if _, err := p.AddBar([]float32{0, 1}, []float32{2, 5}); err != nil {
		t.Fatal(err)
	}
	overlay := drawnCount(t, p, 300, 300)
	changed, err := p.SetBarMode("stack")
	if err != nil || !changed {
		t.Fatalf("SetBarMode: changed=%v err=%v", changed, err)
	}
	if drawnCount(t, p, 300, 300) == overlay {
		t.Error("stacking must change the frame")
	}
	if changed, _ = p.SetBarMode("stack"); changed {
		t.Error("setting the same mode must report no change")
	}
	if _, err := p.SetBarMode("pile"); err == nil || !strings.Contains(err.Error(), "barmode") {
		t.Errorf("bad barmode error = %v", err)
	}
}

func TestHorizontalBarsAndCategories(t *testing.T) {
	p := New()
	defer p.Close()
	if _, err := p.AddBar([]float32{0, 1, 2}, []float32{3, 5, 4},
		WithOrientation("horizontal")); err != nil {
		t.Fatalf("horizontal bar: %v", err)
	}
	if _, err := p.AddBar([]float32{0}, []float32{1}, WithOrientation("sideways")); err == nil ||
		!strings.Contains(err.Error(), "orientation") {
		t.Errorf("bad orientation error = %v", err)
	}
	changed, err := p.SetCategories("y", []string{"alpha", "beta", "gamma"})
	if err != nil || !changed {
		t.Fatalf("SetCategories: changed=%v err=%v", changed, err)
	}
	if changed, _ = p.SetCategories("y", []string{"alpha", "beta", "gamma"}); changed {
		t.Error("setting the same names must report no change")
	}
	if changed, _ = p.SetCategories("y", nil); !changed {
		t.Error("clearing names must report a change")
	}
	if _, err := p.SetCategories("z", []string{"a"}); err == nil ||
		!strings.Contains(err.Error(), "axis 'x' or 'y'") {
		t.Errorf("bad axis error = %v", err)
	}
}

func TestAddHeatmap(t *testing.T) {
	p := New()
	defer p.Close()
	xs, ys := []float32{0, 1, 2}, []float32{0, 1}
	zs := [][]float32{{0, 1, 2}, {3, 4, 5}}
	if _, err := p.AddHeatmap(xs, ys, zs); err != nil {
		t.Fatalf("AddHeatmap: %v", err)
	}
	if drawnCount(t, p, 300, 200) == 0 {
		t.Error("heatmap drew nothing")
	}
	if _, err := p.AddHeatmap(xs, ys, [][]float32{{0, 1}}); err == nil ||
		!strings.HasPrefix(err.Error(), "zs must be a 2×3 grid") {
		t.Errorf("ragged grid error = %v", err)
	}
	if _, err := p.AddHeatmap(xs, ys, zs, WithColormap("heat")); err == nil ||
		!strings.HasPrefix(err.Error(), `unknown colormap "heat"`) {
		t.Errorf("bad colormap error = %v", err)
	}
	if _, err := p.AddHeatmap(xs, ys, zs, WithoutColorbar(), WithColorbarLabel("kW")); err != nil {
		t.Errorf("colorbar options: %v", err)
	}
}

func TestAddHistogram(t *testing.T) {
	p := New()
	defer p.Close()
	values := make([]float32, 200)
	for i := range values {
		values[i] = float32(i % 20)
	}
	h, err := p.AddHistogram(values, WithBins(8))
	if err != nil {
		t.Fatalf("AddHistogram: %v", err)
	}
	if drawnCount(t, p, 240, 180) == 0 {
		t.Error("histogram drew nothing")
	}
	if err := p.ExtendValues(h, []float32{500}); err != nil {
		t.Errorf("ExtendValues: %v", err)
	}
	if _, err := p.AddHistogram(values, WithBins(8), WithBinWidth(1)); err == nil ||
		!strings.Contains(err.Error(), "not both") {
		t.Errorf("mutually exclusive knobs error = %v", err)
	}
	if err := p.Extend(h, []float32{1}, []float32{1}); err == nil ||
		!strings.Contains(err.Error(), "extend_values") {
		t.Errorf("structural error = %v", err)
	}
}

func TestAddStep(t *testing.T) {
	p := New()
	defer p.Close()
	for _, where := range []string{"pre", "post", "mid"} {
		if _, err := p.AddStep([]float32{0, 1, 2}, []float32{0, 1, 0}, WithStep(where)); err != nil {
			t.Errorf("AddStep(%q): %v", where, err)
		}
	}
	if _, err := p.AddStep([]float32{0, 1}, []float32{0, 1}, WithStep("stairs")); err == nil ||
		!strings.Contains(err.Error(), "step mode") {
		t.Errorf("bad step mode error = %v", err)
	}
}

func TestSetPointStyles(t *testing.T) {
	p := New()
	defer p.Close()
	h, err := p.AddScatter([]float32{0, 1, 2}, []float32{1, 1, 1})
	if err != nil {
		t.Fatalf("AddScatter: %v", err)
	}
	if err := p.SetPointStyles(h, []RGB{{10, 200, 30}}, nil, nil); err != nil {
		t.Errorf("colors only: %v", err)
	}
	if err := p.SetPointStyles(h, nil, []float32{4, 5, 6}, nil); err != nil {
		t.Errorf("sizes only: %v", err)
	}
	if err := p.SetPointStyles(h, nil, nil, []string{"ring", "square", "dot"}); err != nil {
		t.Errorf("shapes only: %v", err)
	}
	if err := p.SetPointStyles(h, nil, nil, []string{"blob"}); err == nil ||
		!strings.Contains(err.Error(), "shape") {
		t.Errorf("bad shape error = %v", err)
	}
	bars, _ := p.AddBar([]float32{0}, []float32{1})
	if err := p.SetPointStyles(bars, []RGB{{1, 2, 3}}, nil, nil); err == nil ||
		!strings.Contains(err.Error(), "scatter") {
		t.Errorf("wrong-kind error = %v", err)
	}
}

func TestMeshIndexValidation(t *testing.T) {
	p := New()
	defer p.Close()
	xs := []float32{0, 1, 0}
	ys := []float32{0, 0, 1}
	zs := []float32{0, 0, 0}
	if _, err := p.AddMesh3D(xs, ys, zs, [][3]uint32{{0, 1, 2}}); err != nil {
		t.Fatalf("valid mesh: %v", err)
	}
	if _, err := p.AddMesh3D(xs, ys, zs, [][3]uint32{{0, 1, 7}}); err == nil ||
		!strings.HasPrefix(err.Error(), "triangle index 7 names no vertex") {
		t.Errorf("out-of-range index error = %v", err)
	}
	if _, err := p.AddMesh3D(xs, ys, zs, [][3]uint32{{0, 1, 2}},
		WithColormap("heat")); err == nil ||
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

func TestXWindowRoundtripAndValidation(t *testing.T) {
	p := plot2D(t)
	if _, _, ok := p.XWindow(); ok {
		t.Fatal("fresh plot must have no x window")
	}
	changed, err := p.SetXWindow(0.5, 1.5)
	if err != nil || !changed {
		t.Fatalf("set: changed=%v err=%v", changed, err)
	}
	if changed, _ := p.SetXWindow(0.5, 1.5); changed {
		t.Fatal("same window must not report a change")
	}
	lo, hi, ok := p.XWindow()
	if !ok || lo != 0.5 || hi != 1.5 {
		t.Fatalf("read back (%v, %v, %v)", lo, hi, ok)
	}
	if _, err := p.SetXWindow(2, 2); err == nil ||
		!strings.Contains(err.Error(), "x_window needs finite lo < hi") {
		t.Fatalf("degenerate window error: %v", err)
	}
	if !p.ClearXWindow() {
		t.Fatal("clear must report a change")
	}
	if changed, err := p.SetXEpoch(1.7e9); err != nil || !changed {
		t.Fatalf("epoch set: %v %v", changed, err)
	}
	if e, ok := p.XEpoch(); !ok || e != 1.7e9 {
		t.Fatalf("epoch read back (%v, %v)", e, ok)
	}
}

func TestRangeSliderHitAndDrag(t *testing.T) {
	p := plot2D(t)
	if !p.SetRangeSlider(true) {
		t.Fatal("enabling the strip must report a change")
	}
	// 400x240: the strip hugs the bottom rows; mid-strip must hit something.
	if hit := p.RangeSliderHit(400, 240, 200, 224, 4); hit == RangeNone {
		t.Fatal("mid-strip point must hit the slider")
	}
	if hit := p.RangeSliderHit(400, 240, 200, 100, 4); hit != RangeNone {
		t.Fatalf("plot-area point must miss the slider, got %v", hit)
	}
	// Shrink from the full extent, then slide.
	if changed, err := p.DragXWindow(400, 240, RangeRight, -120); err != nil || !changed {
		t.Fatalf("handle drag: %v %v", changed, err)
	}
	if _, _, ok := p.XWindow(); !ok {
		t.Fatal("a drag must materialize a window")
	}
	if _, err := p.DragXWindow(400, 240, RangeHit(9), 1); err == nil {
		t.Fatal("invalid part must error")
	}
	if !p.JumpXWindow(400, 240, 300) {
		t.Fatal("track jump must move the shrunken window")
	}
	if !p.PanXWindow(400, 240, 25) {
		t.Fatal("pan must slide the window")
	}
	if !p.ZoomXWindow(400, 240, 200, 2) {
		t.Fatal("zoom must narrow the window")
	}
}

func TestInputMapRemapAndSharedError(t *testing.T) {
	p := New()
	t.Cleanup(p.Close)
	if _, err := p.AddScatter3D([]float32{0, 1}, []float32{0, 1}, []float32{0, 1}); err != nil {
		t.Fatal(err)
	}
	// Default map: a plain drag rotates and never pans.
	before := p.CameraState()
	p.ApplyDrag(10, 5, false, 0.03, 1, 1, 0.15)
	after := p.CameraState()
	if after[0] == before[0] || after[1] == before[1] {
		t.Fatal("default drag must rotate both axes")
	}
	if after[3] != before[3] || after[4] != before[4] {
		t.Fatal("default drag must not pan")
	}
	// Remapped: a plain drag pans exactly and never rotates.
	if err := p.SetInputMap("pan_x", "pan_y", "", ""); err != nil {
		t.Fatal(err)
	}
	before = p.CameraState()
	p.ApplyDrag(10, 5, false, 0.03, 1, 1, 0.15)
	after = p.CameraState()
	if after[0] != before[0] || after[1] != before[1] {
		t.Fatal("remapped drag must not rotate")
	}
	if after[3]-before[3] != 10 || after[4]-before[4] != 5 {
		t.Fatalf("remapped drag must pan exactly, got (%v, %v)", after[3]-before[3], after[4]-before[4])
	}
	// The shared bind error string, byte-identical across bindings.
	err := p.SetInputMap("bogus", "", "", "")
	want := `camera control must be 'yaw', 'pitch', 'pan_x', 'pan_y', 'zoom' or 'off', ` +
		`optionally prefixed with '-' to invert the axis, got "bogus"`
	if err == nil || err.Error() != want {
		t.Fatalf("got %v, want %s", err, want)
	}
}
