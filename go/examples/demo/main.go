// Bubble Tea demo: an interactive 3D scatter (a double helix, mirroring
// examples/textual_demo.py).
//
//	cargo build -p plotui-ffi --release   # once, at the repo root
//	go run ./examples/demo                # from go/
//
// Drag to rotate, shift-drag to pan, scroll to zoom, r to reset, q to quit.
// Requires a terminal with Kitty graphics (Kitty, Ghostty, iTerm2 ≥ 3.5,
// WezTerm).
package main

import (
	"fmt"
	"math"
	"os"

	tea "charm.land/bubbletea/v2"
	plotui "github.com/sebaheg/plotui/go"
	"github.com/sebaheg/plotui/go/teaplot"
)

func makePlot() *plotui.Plot {
	p := plotui.New()
	n := 1600
	xs := make([]float32, n)
	ys := make([]float32, n)
	zs := make([]float32, n)
	for i := range xs {
		t := float64(i) / float64(n) * 6 * math.Pi
		strand := 1.0
		if i%2 == 1 {
			strand = -1.0
		}
		xs[i] = float32(math.Cos(t) * strand)
		ys[i] = float32(t/(6*math.Pi)*2 - 1)
		zs[i] = float32(math.Sin(t) * strand)
	}
	if _, err := p.AddScatter3D(xs, ys, zs, plotui.WithSize(2.0)); err != nil {
		panic(err)
	}
	return p
}

type model struct {
	plot teaplot.Model
}

func (m model) Init() tea.Cmd { return m.plot.Init() }

func (m model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.KeyPressMsg:
		if msg.Code == 'q' || msg.Code == tea.KeyEscape {
			// Delete the image before quitting, or the last frame outlives
			// the app on terminals that keep placements around.
			return m, tea.Sequence(m.plot.CleanupCmd(), tea.Quit)
		}
	case tea.WindowSizeMsg:
		cmd := m.plot.SetSize(msg.Width, msg.Height)
		return m, cmd
	}
	var cmd tea.Cmd
	m.plot, cmd = m.plot.Update(msg)
	return m, cmd
}

func (m model) View() tea.View {
	return tea.View{
		Content:   m.plot.View(),
		MouseMode: tea.MouseModeAllMotion,
		AltScreen: true,
	}
}

func main() {
	m := model{plot: teaplot.New(makePlot(), teaplot.WithAutoRotate())}
	if _, err := tea.NewProgram(m).Run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
