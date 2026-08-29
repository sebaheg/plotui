// Package teaplot embeds an interactive plotui plot in a Bubble Tea (v2)
// app.
//
// The host owns the loop; the component follows the same contract as the
// Textual and Ratatui frontends: forward messages to Update, compose View()
// into your own view, and let the returned commands carry the image bytes
// (Kitty graphics escapes go out through tea.Raw — never write to stdout
// behind the renderer's back).
//
// Host obligations:
//   - Set MouseMode: tea.MouseModeAllMotion on your tea.View (hover and
//     drag need motion events).
//   - Forward tea.WindowSizeMsg → SetSize (or size it yourself from your
//     layout), and tell the component where it sits with SetPosition —
//     mouse math and direct-mode placement both need it.
//   - Run CleanupCmd() before quitting, so the image doesn't outlive the
//     app.
//
// Rendering picks the best path for the terminal (see plotui's
// DetectRenderMode): Unicode-placeholder Kitty graphics in Kitty/Ghostty
// (flicker-free, overlay-friendly), direct Kitty placement in
// iTerm2/WezTerm/Konsole, and a supported-terminals notice elsewhere.
// PLOTUI_RENDER overrides detection.
package teaplot

import (
	"fmt"
	"strings"
	"time"

	tea "charm.land/bubbletea/v2"
	plotui "github.com/sebaheg/plotui/go"
)

// Interaction constants — identical to the Textual and Ratatui frontends
// (they live canonically in the Rust plotui-term crate).
const (
	rotatePerCell   = 0.03
	keyRotateStep   = 0.1
	keyPanCells     = 2.0
	zoomIn          = 1.1
	zoomOut         = 0.9
	autoRotateStep  = 0.02
	autoRotateEvery = time.Second / 30
)

// Messages the component sends through returned commands.
type (
	// NodePickedMsg reports a click resolved against nodes only (when the
	// component is not pickable). OK is false for empty space.
	NodePickedMsg struct {
		Index int
		OK    bool
	}
	// ElementPickedMsg reports a click resolved against nodes and edges
	// (pickable components). Element is nil for empty space.
	ElementPickedMsg struct{ Element *plotui.Element }
	// ElementHoveredMsg reports a hover change (pickable components).
	ElementHoveredMsg struct{ Element *plotui.Element }

	tickMsg struct{}
)

// OverlaySpan is text drawn over the plot at (Row, Col) in component cells.
// Render styles the text (pass a lipgloss style's Render, or nil for
// plain).
type OverlaySpan struct {
	Row, Col int
	Text     string
	Render   func(string) string
}

// Option configures New.
type Option func(*Model)

// WithAutoRotate spins 3D plots at ~30 Hz.
func WithAutoRotate() Option { return func(m *Model) { m.autoRotate = true } }

// WithPickable turns on interactive picking: hovering lights elements up,
// clicking sends ElementPickedMsg.
func WithPickable() Option { return func(m *Model) { m.pickable = true } }

// WithoutCrosshair disables the 2D hover crosshair (on by default).
func WithoutCrosshair() Option { return func(m *Model) { m.crosshair = false } }

// WithRenderMode forces a render path instead of detecting one.
func WithRenderMode(mode plotui.RenderMode) Option {
	return func(m *Model) { m.mode = mode }
}

// WithCellPx sets the device pixels per terminal cell instead of querying
// the terminal.
func WithCellPx(w, h int) Option {
	return func(m *Model) { m.cellW, m.cellH = w, h }
}

// WithInteractiveScale sets the resolution multiplier for large 3D plots
// while interacting (default 0.5; 1.0 disables the reduction).
func WithInteractiveScale(scale float64) Option {
	return func(m *Model) { m.interactiveScale = scale }
}

// Model is the plot component. Embed it in your model; it follows the
// usual bubbles conventions (Init/Update/View + setters that return
// commands).
type Model struct {
	plot *plotui.Plot
	mode plotui.RenderMode

	width, height int
	posX, posY    int
	cellW, cellH  int

	autoRotate       bool
	pickable         bool
	crosshair        bool
	interactiveScale float64
	replace          bool

	dragging     bool
	moved        bool
	lastX, lastY int
	hovered      *plotui.Element
	hover2d      bool

	dirty   bool
	rows    []string // styled View rows (placeholder cells or blanks)
	overlay []OverlaySpan
}

// New wraps a plot. The plot stays yours: mutate it via the component's
// command-returning helpers (Extend, SetVisible, Invalidate) so repaints
// and image retransmits happen at the right time.
func New(p *plotui.Plot, opts ...Option) Model {
	m := Model{
		plot:             p,
		mode:             plotui.RenderMode(-1),
		crosshair:        true,
		interactiveScale: 0.5,
		replace:          plotui.KittyReplaceEnv(),
	}
	for _, opt := range opts {
		opt(&m)
	}
	if m.mode == plotui.RenderMode(-1) {
		m.mode = plotui.DetectRenderMode()
	}
	if m.cellW == 0 || m.cellH == 0 {
		m.cellW, m.cellH, _ = plotui.DetectCellPx()
	}
	m.dirty = true
	return m
}

// Plot is the wrapped plot (camera state, project-nodes, …).
func (m *Model) Plot() *plotui.Plot { return m.plot }

// RenderMode is the render path in use.
func (m Model) RenderMode() plotui.RenderMode { return m.mode }

// Dragging reports an active drag gesture — a hook for hosts that defer
// expensive work mid-gesture.
func (m Model) Dragging() bool { return m.dragging && m.moved }

// SetSize sets the component's size in terminal cells (forward your
// WindowSizeMsg or layout result here). Returns the repaint command.
func (m *Model) SetSize(width, height int) tea.Cmd {
	if width != m.width || height != m.height {
		m.width, m.height = width, height
		m.dirty = true
	}
	return m.refresh()
}

// SetPosition tells the component where its top-left cell sits on screen —
// required for mouse hit-testing and direct-mode placement.
func (m *Model) SetPosition(x, y int) {
	if x != m.posX || y != m.posY {
		m.posX, m.posY = x, y
		if m.mode == plotui.RenderDirect {
			m.dirty = true
		}
	}
}

// Invalidate marks the view dirty after direct plot mutations and returns
// the repaint command.
func (m *Model) Invalidate() tea.Cmd {
	m.dirty = true
	return m.refresh()
}

// Extend appends points to a 2D trace and repaints (Extend3D for 3D).
func (m *Model) Extend(h plotui.TraceHandle, xs, ys []float32) (tea.Cmd, error) {
	if err := m.plot.Extend(h, xs, ys); err != nil {
		return nil, err
	}
	return m.Invalidate(), nil
}

// Extend3D appends points to a 3D scatter/line trace and repaints.
func (m *Model) Extend3D(h plotui.TraceHandle, xs, ys, zs []float32) (tea.Cmd, error) {
	if err := m.plot.Extend3D(h, xs, ys, zs); err != nil {
		return nil, err
	}
	return m.Invalidate(), nil
}

// SetVisible shows or hides a trace; repaints only on an actual change.
func (m *Model) SetVisible(h plotui.TraceHandle, visible bool) (tea.Cmd, error) {
	changed, err := m.plot.SetVisible(h, visible)
	if err != nil || !changed {
		return nil, err
	}
	return m.Invalidate(), nil
}

// SetOverlay draws text spans over the plot (labels, badges); repaints
// without re-rasterizing the image.
func (m *Model) SetOverlay(spans []OverlaySpan) tea.Cmd {
	m.overlay = spans
	m.dirty = true
	return m.refresh()
}

// CleanupCmd deletes the plot's image from the terminal. Run it before
// quitting.
func (m Model) CleanupCmd() tea.Cmd {
	if m.mode == plotui.RenderUnsupported {
		return nil
	}
	return tea.Raw(plotui.TmuxWrap(m.plot.KittyCleanup()))
}

// Init starts the auto-rotate ticker when enabled.
func (m Model) Init() tea.Cmd {
	if m.autoRotate {
		return tick()
	}
	return nil
}

func tick() tea.Cmd {
	return tea.Tick(autoRotateEvery, func(time.Time) tea.Msg { return tickMsg{} })
}

// Update handles input and timers. Returns the updated model and any
// commands (image retransmits, pick/hover messages, the next tick).
func (m Model) Update(msg tea.Msg) (Model, tea.Cmd) {
	if m.mode == plotui.RenderUnsupported {
		return m, nil
	}
	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		return m, m.SetSize(msg.Width, msg.Height)
	case tickMsg:
		m.plot.Rotate(autoRotateStep, 0)
		m.dirty = true
		return m, tea.Batch(m.refresh(), tick())
	case tea.MouseClickMsg:
		if msg.Button == plotuiMouseLeft && m.contains(msg.X, msg.Y) {
			m.dragging = true
			m.moved = false
			m.lastX, m.lastY = msg.X, msg.Y
		}
		return m, nil
	case tea.MouseMotionMsg:
		return m.motion(tea.Mouse(msg))
	case tea.MouseReleaseMsg:
		return m.release(tea.Mouse(msg))
	case tea.MouseWheelMsg:
		if !m.contains(msg.X, msg.Y) {
			return m, nil
		}
		switch msg.Button {
		case tea.MouseWheelUp:
			m.plot.ZoomBy(zoomIn)
		case tea.MouseWheelDown:
			m.plot.ZoomBy(zoomOut)
		default:
			return m, nil
		}
		m.dirty = true
		return m, m.refresh()
	case tea.KeyPressMsg:
		return m.key(msg)
	}
	return m, nil
}

// View is the component's cell content; compose it into your tea.View.
// (The image itself travels via tea.Raw commands, not the view.)
func (m Model) View() string {
	if m.mode == plotui.RenderUnsupported {
		return m.unsupportedView()
	}
	return strings.Join(m.rows, "\n")
}

// refresh re-rasterizes when dirty, rebuilds the styled rows, and returns
// the command that carries the image escape to the terminal.
func (m *Model) refresh() tea.Cmd {
	if !m.dirty || m.width <= 0 || m.height <= 0 || m.mode == plotui.RenderUnsupported {
		return nil
	}
	m.dirty = false
	scale := m.plot.InteractiveScale(m.Dragging() || m.autoRotate, m.interactiveScale)
	switch m.mode {
	case plotui.RenderPlaceholder:
		ph, err := m.plot.RenderPlaceholder(m.width, m.height, m.cellW, m.cellH, scale)
		if err != nil {
			return nil
		}
		m.rows = m.placeholderRows(ph)
		// A U=1 virtual placement draws nothing at the cursor, so where the
		// transmit lands in the output stream is irrelevant — only the
		// placeholder cells in View make pixels appear, and the fixed image
		// id replaces frames atomically.
		return tea.Raw(ph.Transmit)
	case plotui.RenderDirect:
		escape, err := m.plot.RenderKitty(m.width, m.height, m.cellW, m.cellH, plotui.RenderOpts{
			CompatChunks: true,
			Scale:        scale,
			Replace:      m.replace,
		})
		if err != nil {
			return nil
		}
		m.rows = m.blankRows()
		// Save the cursor, park it at the component's origin, draw (the
		// escape saves/restores around its own placement), restore.
		positioned := fmt.Sprintf("\x1b[s\x1b[%d;%dH%s\x1b[u",
			m.posY+1, m.posX+1, plotui.TmuxWrap(escape))
		return tea.Raw(positioned)
	}
	return nil
}

// placeholderRows styles each row of placeholder cells with the
// id-encoding foreground color and splices overlay spans in. Cells are
// self-addressed (each carries its own position diacritics), so cells
// after a spliced gap still map to the right part of the image.
func (m *Model) placeholderRows(ph *plotui.Placeholder) []string {
	fg := fmt.Sprintf("\x1b[38;2;%d;%d;%dm", ph.IDColor.R, ph.IDColor.G, ph.IDColor.B)
	rows := make([]string, len(ph.Cells))
	for y, cells := range ph.Cells {
		spans := m.rowSpans(y)
		var b strings.Builder
		x := 0
		for _, span := range spans {
			if span.Col > x {
				b.WriteString(fg)
				for _, cell := range cells[x:span.Col] {
					b.WriteString(cell)
				}
				b.WriteString("\x1b[39m")
			}
			text, width := clip(span.Text, m.width-span.Col)
			if span.Render != nil {
				text = span.Render(text)
			}
			b.WriteString(text)
			x = span.Col + width
		}
		if x < len(cells) {
			b.WriteString(fg)
			for _, cell := range cells[x:] {
				b.WriteString(cell)
			}
			b.WriteString("\x1b[39m")
		}
		rows[y] = b.String()
	}
	return rows
}

// blankRows renders the cells under a direct-mode image (spaces + overlay
// text; most terminals draw the image above text — prefer placeholder mode
// for text-over-plot).
func (m *Model) blankRows() []string {
	rows := make([]string, m.height)
	for y := range rows {
		var b strings.Builder
		x := 0
		for _, span := range m.rowSpans(y) {
			if span.Col > x {
				b.WriteString(strings.Repeat(" ", span.Col-x))
			}
			text, width := clip(span.Text, m.width-span.Col)
			if span.Render != nil {
				text = span.Render(text)
			}
			b.WriteString(text)
			x = span.Col + width
		}
		if x < m.width {
			b.WriteString(strings.Repeat(" ", m.width-x))
		}
		rows[y] = b.String()
	}
	return rows
}

// rowSpans returns this row's overlay spans, sorted, overlaps dropped
// (first one wins), clipped to the component.
func (m *Model) rowSpans(row int) []OverlaySpan {
	var spans []OverlaySpan
	for _, s := range m.overlay {
		if s.Row == row && s.Col >= 0 && s.Col < m.width && s.Text != "" {
			spans = append(spans, s)
		}
	}
	if len(spans) > 1 {
		for i := 1; i < len(spans); i++ {
			for j := i; j > 0 && spans[j].Col < spans[j-1].Col; j-- {
				spans[j], spans[j-1] = spans[j-1], spans[j]
			}
		}
		kept := spans[:1]
		for _, s := range spans[1:] {
			last := kept[len(kept)-1]
			_, lastWidth := clip(last.Text, m.width-last.Col)
			if s.Col >= last.Col+lastWidth {
				kept = append(kept, s)
			}
		}
		spans = kept
	}
	return spans
}

// clip truncates text to width cells (rune-counted) and returns it with
// its cell width.
func clip(text string, width int) (string, int) {
	if width <= 0 {
		return "", 0
	}
	runes := []rune(text)
	if len(runes) > width {
		runes = runes[:width]
	}
	return string(runes), len(runes)
}

var unsupportedMessage = [4]string{
	"Plotting requires a terminal that supports the Kitty graphics protocol.",
	"",
	"Supported terminals include Kitty, Ghostty, iTerm2 (3.5+), WezTerm, and Konsole.",
	"If yours does support it, force a path with PLOTUI_RENDER=placeholder|direct.",
}

func (m Model) unsupportedView() string {
	rows := make([]string, m.height)
	top := (m.height - len(unsupportedMessage)) / 2
	if top < 0 {
		top = 0
	}
	for y := range rows {
		line := ""
		if i := y - top; i >= 0 && i < len(unsupportedMessage) {
			text, width := clip(unsupportedMessage[i], m.width)
			pad := (m.width - width) / 2
			switch i {
			case 0:
				text = "\x1b[1m" + text + "\x1b[22m"
			case len(unsupportedMessage) - 1:
				text = "\x1b[2m" + text + "\x1b[22m"
			}
			line = strings.Repeat(" ", pad) + text
		}
		rows[y] = line
	}
	return strings.Join(rows, "\n")
}
