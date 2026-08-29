// The M0 de-risk spike for the Bubble Tea placeholder strategy, kept for
// regression debugging: renders one static placeholder frame through the
// v2 renderer and emits the transmit via tea.Raw.
//
// Run it in Kitty or Ghostty:
//
//	go run ./internal/spike
//
// Pass criteria: the plot's pixels appear inside the frame, aligned to the
// cell grid; they survive a full repaint (resize the window) and the
// overlay text splices in without shifting the image right of the gap.
package main

import (
	"fmt"
	"os"

	tea "charm.land/bubbletea/v2"
	plotui "github.com/sebaheg/plotui/go"
	"github.com/sebaheg/plotui/go/teaplot"
)

type model struct{ plot teaplot.Model }

func (m model) Init() tea.Cmd { return nil }

func (m model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.KeyPressMsg:
		if msg.Code == 'q' {
			return m, tea.Sequence(m.plot.CleanupCmd(), tea.Quit)
		}
	case tea.WindowSizeMsg:
		cmd := m.plot.SetSize(msg.Width, msg.Height)
		over := m.plot.SetOverlay([]teaplot.OverlaySpan{
			{Row: 1, Col: 4, Text: " overlay splice test "},
		})
		return m, tea.Batch(cmd, over)
	}
	var cmd tea.Cmd
	m.plot, cmd = m.plot.Update(msg)
	return m, cmd
}

func (m model) View() tea.View {
	return tea.View{Content: m.plot.View(), MouseMode: tea.MouseModeAllMotion, AltScreen: true}
}

func main() {
	p := plotui.New()
	if _, err := p.AddLine([]float32{0, 1, 2, 3, 4}, []float32{0, 3, 1, 4, 2},
		plotui.WithName("spike")); err != nil {
		panic(err)
	}
	m := model{plot: teaplot.New(p)}
	if _, err := tea.NewProgram(m).Run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
