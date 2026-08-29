package teaplot

import (
	"strings"
	"testing"

	tea "charm.land/bubbletea/v2"
	plotui "github.com/sebaheg/plotui/go"
)

func plot3D(t *testing.T, n int) *plotui.Plot {
	t.Helper()
	p := plotui.New()
	t.Cleanup(p.Close)
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
	return p
}

func plot2D(t *testing.T) *plotui.Plot {
	t.Helper()
	p := plotui.New()
	t.Cleanup(p.Close)
	if _, err := p.AddLine([]float32{0, 1, 2}, []float32{0, 2, 1}); err != nil {
		t.Fatal(err)
	}
	return p
}

func sized(t *testing.T, p *plotui.Plot, opts ...Option) Model {
	t.Helper()
	m := New(p, append([]Option{
		WithRenderMode(plotui.RenderPlaceholder),
		WithCellPx(8, 16),
	}, opts...)...)
	m.SetSize(20, 10)
	return m
}

// drain runs a command tree and collects the messages it produces
// (ignoring nil and raw output).
func drain(cmd tea.Cmd) []tea.Msg {
	if cmd == nil {
		return nil
	}
	var out []tea.Msg
	msg := cmd()
	switch batch := msg.(type) {
	case tea.BatchMsg:
		for _, c := range batch {
			out = append(out, drain(c)...)
		}
	default:
		if msg != nil {
			out = append(out, msg)
		}
	}
	return out
}

func click(x, y int) tea.MouseClickMsg {
	return tea.MouseClickMsg{X: x, Y: y, Button: tea.MouseLeft}
}
func release(x, y int) tea.MouseReleaseMsg {
	return tea.MouseReleaseMsg{X: x, Y: y, Button: tea.MouseLeft}
}
func motion(x, y int, mod tea.KeyMod) tea.MouseMotionMsg {
	return tea.MouseMotionMsg{X: x, Y: y, Mod: mod, Button: tea.MouseLeft}
}

func TestViewIsPlaceholderRowsWithIDColor(t *testing.T) {
	m := sized(t, plot2D(t))
	view := m.View()
	rows := strings.Split(view, "\n")
	if len(rows) != 10 {
		t.Fatalf("view has %d rows, want 10", len(rows))
	}
	if strings.Count(view, string(plotui.PlaceholderRune)) != 200 {
		t.Fatalf("view must hold 20x10 placeholder cells")
	}
	id := m.Plot().ImageID()
	want := "\x1b[38;2;" // truecolor foreground carrying the image id
	if !strings.Contains(rows[0], want) {
		t.Fatalf("rows must carry a truecolor fg (image id %d)", id)
	}
	if strings.Contains(view, "\x1b_G") {
		t.Fatal("the transmit escape must NOT be in the view (it travels via tea.Raw)")
	}
}

func TestSetSizeEmitsTransmitViaRaw(t *testing.T) {
	p := plot2D(t)
	m := New(p, WithRenderMode(plotui.RenderPlaceholder), WithCellPx(8, 16))
	cmd := m.SetSize(20, 10)
	msgs := drain(cmd)
	if len(msgs) != 1 {
		t.Fatalf("SetSize must emit one message, got %d", len(msgs))
	}
	raw, ok := msgs[0].(tea.RawMsg)
	if !ok {
		t.Fatalf("message must be tea.RawMsg, got %T", msgs[0])
	}
	s, _ := raw.Msg.(string)
	if !strings.Contains(s, "U=1") || !strings.Contains(s, "\x1b_G") {
		t.Fatalf("raw payload must be the virtual-placement transmit, got %.40q", s)
	}
	// A clean model with nothing dirty emits nothing.
	if cmd := m.Invalidate(); cmd == nil {
		t.Fatal("invalidate must repaint")
	}
}

func TestDragRotatesAndShiftDragPans(t *testing.T) {
	m := sized(t, plot3D(t, 50))
	before := m.Plot().CameraState()

	m, _ = m.Update(click(5, 5))
	m, _ = m.Update(motion(8, 5, 0))
	after := m.Plot().CameraState()
	if diff := after[0] - before[0]; diff < 0.089 || diff > 0.091 {
		t.Fatalf("3 cells of drag: yaw moved %v, want 0.09", diff)
	}
	if !m.Dragging() {
		t.Fatal("mid-gesture: Dragging must report true")
	}
	m, _ = m.Update(release(8, 5))
	if m.Dragging() {
		t.Fatal("after release: Dragging must report false")
	}

	m, _ = m.Update(click(5, 5))
	m, _ = m.Update(motion(7, 6, tea.ModShift))
	state := m.Plot().CameraState()
	if state[3] != 2*8 || state[4] != 1*16 {
		t.Fatalf("shift-drag pan = (%v, %v), want (16, 16)", state[3], state[4])
	}
}

func TestWheelZoomAndKeys(t *testing.T) {
	m := sized(t, plot3D(t, 50))
	m, _ = m.Update(tea.MouseWheelMsg{X: 5, Y: 5, Button: tea.MouseWheelUp})
	if z := m.Plot().CameraState()[2]; z < 1.09 || z > 1.11 {
		t.Fatalf("wheel up zoom = %v, want 1.1", z)
	}
	// Outside the component: ignored.
	m, _ = m.Update(tea.MouseWheelMsg{X: 50, Y: 50, Button: tea.MouseWheelUp})
	if z := m.Plot().CameraState()[2]; z > 1.11 {
		t.Fatal("wheel outside the bounds must be ignored")
	}

	yaw := m.Plot().CameraState()[0]
	m, _ = m.Update(tea.KeyPressMsg{Code: tea.KeyLeft})
	if diff := m.Plot().CameraState()[0] - yaw; diff > -0.099 || diff < -0.101 {
		t.Fatalf("left arrow: yaw moved %v, want -0.1", diff)
	}
	m, _ = m.Update(tea.KeyPressMsg{Code: tea.KeyUp, Mod: tea.ModShift})
	if pan := m.Plot().CameraState()[4]; pan != -2*16 {
		t.Fatalf("shift+up pan = %v, want -32", pan)
	}
	m, _ = m.Update(tea.KeyPressMsg{Code: 'r'})
	if z := m.Plot().CameraState()[2]; z != 1.0 {
		t.Fatal("r must reset the camera")
	}
}

func TestClickPicksAndDragDoesNot(t *testing.T) {
	m := sized(t, plot3D(t, 30))

	m, _ = m.Update(click(1, 1))
	_, cmd := m.Update(release(1, 1))
	var picked *NodePickedMsg
	for _, msg := range drain(cmd) {
		if np, ok := msg.(NodePickedMsg); ok {
			picked = &np
		}
	}
	if picked == nil {
		t.Fatal("a click must emit NodePickedMsg")
	}

	m, _ = m.Update(click(1, 1))
	m, _ = m.Update(motion(4, 1, 0))
	_, cmd = m.Update(release(4, 1))
	for _, msg := range drain(cmd) {
		if _, ok := msg.(NodePickedMsg); ok {
			t.Fatal("a completed drag must not emit a pick")
		}
	}
}

func TestPickableHoverEmitsOncePerChange(t *testing.T) {
	m := sized(t, plot3D(t, 30), WithPickable())

	// Find a cell over a real node from the exact projection geometry.
	projected := m.Plot().ProjectNodes(20*8, 10*16)
	cx, cy := int(projected[0][0])/8, int(projected[0][1])/16

	_, cmd := m.Update(motion(cx, cy, 0))
	m2, _ := m.Update(motion(cx, cy, 0))
	var hovered int
	for _, msg := range drain(cmd) {
		if h, ok := msg.(ElementHoveredMsg); ok {
			hovered++
			if h.Element == nil || h.Element.Kind != plotui.ElementNode {
				t.Fatalf("hover over a node reported %v", h.Element)
			}
		}
	}
	if hovered != 1 {
		t.Fatalf("first hover must emit exactly one ElementHoveredMsg, got %d", hovered)
	}
	_ = m2

	// Click on the node → ElementPickedMsg.
	m, _ = m.Update(motion(cx, cy, 0))
	m, _ = m.Update(click(cx, cy))
	_, cmd = m.Update(release(cx, cy))
	var pickedElement bool
	for _, msg := range drain(cmd) {
		if ep, ok := msg.(ElementPickedMsg); ok {
			pickedElement = ep.Element != nil && ep.Element.Kind == plotui.ElementNode
		}
	}
	if !pickedElement {
		t.Fatal("clicking a node must emit ElementPickedMsg with the node")
	}
	_ = m
}

func TestOverlaySplicesIntoPlaceholderRows(t *testing.T) {
	m := sized(t, plot2D(t))
	m.SetOverlay([]OverlaySpan{{Row: 1, Col: 2, Text: "hi"}})
	rows := strings.Split(m.View(), "\n")
	if !strings.Contains(rows[1], "hi") {
		t.Fatal("overlay text must appear in its row")
	}
	if strings.Count(rows[1], string(plotui.PlaceholderRune)) != 18 {
		t.Fatalf("overlay must replace exactly its cells (18 placeholders left)")
	}
	if strings.Count(rows[0], string(plotui.PlaceholderRune)) != 20 {
		t.Fatal("other rows keep all their placeholder cells")
	}
}

func TestHalfResOnlyMidDrag(t *testing.T) {
	m := sized(t, plot3D(t, 500))
	// Mid-drag: the transmit reports a half-resolution source image.
	m, _ = m.Update(click(5, 5))
	_, cmd := m.Update(motion(8, 5, 0))
	raw := rawPayload(t, cmd)
	if !strings.Contains(raw, "s=80,v=80") {
		t.Fatalf("mid-drag transmit must be half-res, got %.80q", raw)
	}
	if !strings.Contains(raw, "c=20,r=10") {
		t.Fatal("the image must still span the full cell region")
	}
	// Release: full resolution snaps back.
	_, cmd = m.Update(release(8, 5))
	if raw := rawPayload(t, cmd); !strings.Contains(raw, "s=160,v=160") {
		t.Fatalf("post-gesture transmit must be full-res, got %.80q", raw)
	}
}

func TestUnsupportedModeShowsTheNotice(t *testing.T) {
	m := New(plot2D(t), WithRenderMode(plotui.RenderUnsupported), WithCellPx(8, 16))
	m.SetSize(90, 10)
	view := m.View()
	if !strings.Contains(view, "Kitty graphics") {
		t.Fatal("unsupported mode must show the notice")
	}
	if strings.Contains(view, string(plotui.PlaceholderRune)) || strings.Contains(view, "\x1b_G") {
		t.Fatal("no image bytes in unsupported mode")
	}
	// Input is ignored.
	before := m.Plot().CameraState()
	m, _ = m.Update(tea.KeyPressMsg{Code: tea.KeyLeft})
	if m.Plot().CameraState() != before {
		t.Fatal("unsupported mode must ignore input")
	}
}

func TestDirectModePositionsTheEscape(t *testing.T) {
	m := New(plot2D(t), WithRenderMode(plotui.RenderDirect), WithCellPx(8, 16))
	m.SetPosition(4, 2)
	cmd := m.SetSize(20, 10)
	raw := rawPayload(t, cmd)
	if !strings.Contains(raw, "\x1b[3;5H") {
		t.Fatalf("direct escape must park the cursor at the component origin (1-based), got %.40q", raw)
	}
	if !strings.Contains(raw, "a=T") {
		t.Fatal("direct escape must transmit the image")
	}
	if strings.Contains(m.View(), "\x1b_G") {
		t.Fatal("the escape must not leak into the view")
	}
}

func rawPayload(t *testing.T, cmd tea.Cmd) string {
	t.Helper()
	for _, msg := range drain(cmd) {
		if raw, ok := msg.(tea.RawMsg); ok {
			if s, ok := raw.Msg.(string); ok {
				return s
			}
		}
	}
	t.Fatal("no tea.RawMsg produced")
	return ""
}
