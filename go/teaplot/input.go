package teaplot

import (
	tea "charm.land/bubbletea/v2"
	plotui "github.com/sebaheg/plotui/go"
)

const plotuiMouseLeft = tea.MouseLeft

func (m Model) contains(x, y int) bool {
	return x >= m.posX && x < m.posX+m.width && y >= m.posY && y < m.posY+m.height
}

// geometry maps a component-relative cell coordinate into the
// full-resolution framebuffer's pixel space: (pxW, pxH, px, py, radius).
func (m Model) geometry(cellX, cellY int) (int, int, float32, float32, float32) {
	cw, ch := float32(m.cellW), float32(m.cellH)
	return m.width * m.cellW, m.height * m.cellH,
		float32(cellX)*cw + cw/2, float32(cellY)*ch + ch/2, ch
}

func (m Model) motion(mouse tea.Mouse) (Model, tea.Cmd) {
	if m.dragging {
		// Deltas from screen coordinates: drags keep arriving even when the
		// pointer leaves the component under all-motion tracking.
		dx, dy := float64(mouse.X-m.lastX), float64(mouse.Y-m.lastY)
		m.lastX, m.lastY = mouse.X, mouse.Y
		if dx != 0 || dy != 0 {
			m.moved = true
		}
		if mouse.Mod&tea.ModShift != 0 {
			// Pan in full-resolution image pixels: one dragged cell is one
			// cell's worth of pixels, so the plot stays under the pointer.
			m.plot.Pan(dx*float64(m.cellW), dy*float64(m.cellH))
		} else {
			// Negated: dragging grabs the camera, not the object — drag
			// right orbits the view right (website-example feel).
			m.plot.Rotate(-dx*rotatePerCell, -dy*rotatePerCell)
		}
		m.dirty = true
		return m, m.refresh()
	}
	if !m.contains(mouse.X, mouse.Y) {
		// No leave event: leaving the component's bounds substitutes —
		// clear the crosshair and the hover highlight.
		var cmds []tea.Cmd
		if m.hover2d && m.plot.SetHover2D(nil) {
			m.hover2d = false
			m.dirty = true
			cmds = append(cmds, m.refresh())
		}
		if m.pickable && m.hovered != nil {
			m.hovered = nil
			if m.plot.SetHovered(nil) {
				m.dirty = true
				cmds = append(cmds, m.refresh())
			}
			cmds = append(cmds, func() tea.Msg { return ElementHoveredMsg{} })
		}
		return m, tea.Batch(cmds...)
	}
	cellX, cellY := mouse.X-m.posX, mouse.Y-m.posY
	if !m.plot.Is3D() {
		if m.crosshair {
			_, _, px, _, _ := m.geometry(cellX, cellY)
			m.hover2d = true
			if m.plot.SetHover2D(&px) {
				m.dirty = true
				return m, m.refresh()
			}
		}
		return m, nil
	}
	if m.pickable {
		pw, ph, px, py, radius := m.geometry(cellX, cellY)
		element := m.plot.PickElementPx(pw, ph, px, py, radius)
		if !sameElement(element, m.hovered) {
			m.hovered = element
			var cmds []tea.Cmd
			if m.plot.SetHovered(element) {
				m.dirty = true
				cmds = append(cmds, m.refresh())
			}
			cmds = append(cmds, func() tea.Msg { return ElementHoveredMsg{Element: element} })
			return m, tea.Batch(cmds...)
		}
	}
	return m, nil
}

func (m Model) release(mouse tea.Mouse) (Model, tea.Cmd) {
	if mouse.Button != tea.MouseLeft || !m.dragging {
		return m, nil
	}
	wasClick := !m.moved
	m.dragging = false
	if !wasClick {
		// Gesture over: replace a half-res interaction frame with a crisp
		// full-res one.
		m.dirty = true
		return m, m.refresh()
	}
	cellX, cellY := mouse.X-m.posX, mouse.Y-m.posY
	pw, ph, px, py, radius := m.geometry(cellX, cellY)
	if m.pickable {
		element := m.plot.PickElementPx(pw, ph, px, py, radius)
		m.plot.SetSelected(element)
		m.dirty = true
		return m, tea.Batch(m.refresh(), func() tea.Msg { return ElementPickedMsg{Element: element} })
	}
	index, ok := m.plot.PickPx(pw, ph, px, py, radius)
	if ok {
		m.plot.SetSelected(&plotui.Element{Kind: plotui.ElementNode, Index: index})
	} else {
		m.plot.SetSelected(nil)
	}
	m.dirty = true
	return m, tea.Batch(m.refresh(), func() tea.Msg { return NodePickedMsg{Index: index, OK: ok} })
}

func (m Model) key(msg tea.KeyPressMsg) (Model, tea.Cmd) {
	shift := msg.Mod&tea.ModShift != 0
	cw, ch := float64(m.cellW), float64(m.cellH)
	switch {
	case msg.Code == '+' || msg.Code == '=':
		m.plot.ZoomBy(zoomIn)
	case msg.Code == '-':
		m.plot.ZoomBy(zoomOut)
	case msg.Code == tea.KeyLeft && shift:
		m.plot.Pan(-keyPanCells*cw, 0)
	case msg.Code == tea.KeyRight && shift:
		m.plot.Pan(keyPanCells*cw, 0)
	case msg.Code == tea.KeyUp && shift:
		m.plot.Pan(0, -keyPanCells*ch)
	case msg.Code == tea.KeyDown && shift:
		m.plot.Pan(0, keyPanCells*ch)
	case msg.Code == tea.KeyLeft:
		m.plot.Rotate(keyRotateStep, 0)
	case msg.Code == tea.KeyRight:
		m.plot.Rotate(-keyRotateStep, 0)
	case msg.Code == tea.KeyUp:
		m.plot.Rotate(0, keyRotateStep)
	case msg.Code == tea.KeyDown:
		m.plot.Rotate(0, -keyRotateStep)
	case msg.Code == 'r':
		m.plot.Reset()
	default:
		return m, nil
	}
	m.dirty = true
	return m, m.refresh()
}

func sameElement(a, b *plotui.Element) bool {
	if a == nil || b == nil {
		return a == b
	}
	return *a == *b
}
