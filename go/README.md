# plotui for Go / Bubble Tea

Go bindings for the plotui terminal plotting engine, plus `teaplot` — a
[Bubble Tea v2](https://github.com/charmbracelet/bubbletea) component with
full feature parity with the Python/Textual widget: render-path detection,
tmux passthrough, drag/zoom/pan/keys, picking + hover, the 2D crosshair,
text overlays, half-resolution interaction frames, and streaming `Extend`.

The engine is the same Rust core behind every plotui frontend, linked
statically through the `plotui-ffi` C ABI.

## Build (local source build)

Requires Rust and Go ≥ 1.24. From the repo root:

```bash
cargo build -p plotui-ffi --release   # once per Rust change
cd go
go test ./...
go run ./examples/demo                # in Kitty/Ghostty/iTerm2/WezTerm
```

The cgo directives link `../target/release/libplotui_ffi.a` by a
`${SRCDIR}`-relative path, so everything works from inside this repo with
no environment setup; the produced Go binary has **no runtime dependency**
on the Rust artifact. Consumers outside the repo should vendor the
staticlib and point `CGO_LDFLAGS` at it.

## Using the plot directly

```go
import plotui "github.com/sebaheg/plotui/go"

p := plotui.New()
defer p.Close()
h, _ := p.AddLine(xs, ys, plotui.WithName("forecast"))
p.AddScatter(xs2, ys2, plotui.WithName("observed"))
p.AddLine(xs, load, plotui.WithName("load"), plotui.WithAxis(plotui.AxisY2))
_ = p.Extend(h, moreXs, moreYs) // streaming append, O(new points)

escape, _ := p.RenderKitty(cols, rows, cellW, cellH, plotui.RenderOpts{})
// …emit with the cursor at the region's top-left; p.KittyCleanup() on exit.
```

2D x-window / range-slider / time-axis state mirrors the other bindings:
`SetXWindow`/`ClearXWindow`/`XWindow`, `SetRangeSlider`,
`SetXEpoch`/`ClearXEpoch`/`XEpoch` (x values as seconds since a UTC epoch
base → calendar ticks), and the gesture calls `RangeSliderHit`,
`DragXWindow`, `JumpXWindow`, `PanXWindow`, `ZoomXWindow`, `ShiftXWindow`
(see `window.go`). Interactive teaplot wiring is not built in yet — forward
mouse events to these calls from your `Update`.

Pipelines and DAGs go through `AddGraph2D` — labelled boxes wired by
directed edges, with per-node colour a running pipeline can repaint live:

```go
// Straight from a DOT file, laid out and ready to render:
p, h, err := plotui.PlotFromDOT(dot, "")   // "" honours the file's rankdir

// Or lay one out yourself:
l, _ := plotui.NewLayeredLayout(len(tasks), edges, "TB")
defer l.Close()
xs, ys, _ := l.Positions()
h, _ := p.AddGraph2D(xs, ys, edges,
    plotui.WithLabels(tasks),
    plotui.WithRoutes(l.Routes()),          // long edges route around ranks
    plotui.WithNodeColors(states))
_ = p.SetGraphColors(h, states, nil)        // repaint as the run advances

// Hover a task and light everything it waits on:
lit := plotui.Reachable(len(tasks), edges, hovered, true)
```

A graph-only frame draws no axes; `SetShowAxes(true)` forces them back on
and `SetShowAxesAuto()` restores the automatic rule.

Zero-option calls reproduce the Python binding's defaults exactly (palette
auto-assignment included), and error messages are byte-identical across
bindings.

## The Bubble Tea component

```go
import (
    tea "charm.land/bubbletea/v2"
    "github.com/sebaheg/plotui/go/teaplot"
)

type model struct{ plot teaplot.Model }

func (m model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
    switch msg := msg.(type) {
    case tea.WindowSizeMsg:
        return m, m.plot.SetSize(msg.Width, msg.Height)
    case tea.KeyPressMsg:
        if msg.Code == 'q' {
            return m, tea.Sequence(m.plot.CleanupCmd(), tea.Quit)
        }
    }
    var cmd tea.Cmd
    m.plot, cmd = m.plot.Update(msg)
    return m, cmd
}

func (m model) View() tea.View {
    return tea.View{Content: m.plot.View(), MouseMode: tea.MouseModeAllMotion}
}
```

**Host obligations** (all three matter):

1. `MouseMode: tea.MouseModeAllMotion` on your `tea.View` — hover and
   drags need motion events.
2. `SetSize` from your layout, and `SetPosition(x, y)` if the component
   doesn't sit at the terminal origin — mouse math and direct-mode
   placement both use it.
3. Run `CleanupCmd()` before `tea.Quit`, or the image outlives the app on
   terminals that keep Kitty placements around.

Image bytes travel through `tea.Raw` commands returned by
`Update`/`SetSize`/`Extend`/…, never through `View()` — return those
commands to the runtime.

Options: `WithAutoRotate`, `WithPickable` (hover glow + `ElementPickedMsg`
/ `ElementHoveredMsg`), `WithoutCrosshair`, `WithRenderMode`,
`WithCellPx`, `WithInteractiveScale`.

## Terminal matrix

| Terminal | Path | Notes |
| --- | --- | --- |
| Kitty, Ghostty | placeholder | flicker-free; overlays splice into the image |
| iTerm2 ≥ 3.5, WezTerm, Konsole | direct | image drawn at the component origin |
| tmux → xterm.js | direct | needs `set -g allow-passthrough on` + `PLOTUI_RENDER=direct` |
| anything else | unsupported | a notice, never a degraded plot |

`PLOTUI_RENDER=placeholder|direct` overrides detection;
`PLOTUI_KITTY_REPLACE=1` for replacing decoders (xterm.js addon-image).
If your terminal reports a color profile below truecolor, force
`PLOTUI_RENDER=direct` — placeholder mode encodes the image id in a
truecolor foreground.

## Manual verification checklist

Headless tests cover geometry, escapes, and the Update loop; actual pixels
need eyes:

- `go run ./internal/spike` in Kitty and Ghostty: image appears, aligns to
  the cell grid, survives a window resize, and the overlay text splices in
  without shifting the image to its right.
- `go run ./examples/demo`: drag rotates (shift-drag pans, wheel zooms,
  `r` resets); no flicker during drag; coarse frames while dragging snap
  crisp on release; `q` leaves no image behind.
- iTerm2/WezTerm: the same demo via direct placement.
- Inside tmux (`allow-passthrough on`, `PLOTUI_RENDER=direct`): the image
  reaches the outer terminal.

## Troubleshooting

- **Linker errors on macOS**: none expected (the staticlib is pure Rust);
  if `___isPlatformVersionAtLeast` ever appears, add
  `-framework CoreFoundation` via `CGO_LDFLAGS`.
- **`plotui.h` not found**: run `cargo build -p plotui-ffi --release` at
  the repo root first (the header is committed at
  `crates/plotui-ffi/include/plotui.h`; the `.a` is what's missing).
- **Blank plot in a supported terminal**: check `DetectRenderMode()`; a
  multiplexer or SSH hop that strips APC sequences blocks Kitty graphics.
