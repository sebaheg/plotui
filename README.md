# plotui

**Interactive 2D/3D plots in the terminal — Plotly-style — for Textual, Ratatui, and Bubble Tea, powered by a Rust core and the Kitty graphics protocol.**

`plotui` renders scatter plots (and, soon, lines / surfaces / bars) as real
pixel graphics inside a terminal, and lets you rotate, pan, and zoom them. It
drops into a [Textual](https://textual.textualize.io/),
[Ratatui](https://ratatui.rs/), or
[Bubble Tea](https://github.com/charmbracelet/bubbletea) app as a first-class
widget, with the rendering engine written in Rust so it stays fast in 2D and 3D.

> Status: **early scaffold.** Working today: 2D scatter/line/step/bar,
> histogram, box, heatmap, and band charts with axes, ticks, titles,
> explicit ranges, log scales, categorical labels, a colorbar and a legend;
> DAG/pipeline graphs with a layered layout and a DOT subset reader; a 3D
> scatter/graph/surface/mesh engine; a Kitty-image raw demo; and a Textual
> widget.

## Architecture

The one rule that shapes everything: **the Rust core owns pixels, not the
terminal.** It has no event loop and no input handling — the TUI framework
(Textual, Ratatui, or Bubble Tea) owns the loop, forwards input to the
camera, and asks for a frame.

```
f64 data ─▶ camera + rasterizer ─▶ RGBA buffer ─▶ Kitty escape bytes ─▶ your terminal's cell grid
```

```
crates/
  plotui-core/      pure engine: data model, 3D camera, rasterizer → RGBA
  plotui-protocol/  RGBA → terminal bytes (Kitty graphics protocol)
  plotui-term/      shared frontend glue: render-path detection, cell-pixel
                    probing, tmux passthrough, the per-frame render policy
  plotui-bind/      shared binding semantics: parsing, validation, defaults,
                    and their exact error messages (Python and Go agree)
  plotui-py/        PyO3 bindings → the `plotui._plotui` native module
  plotui-ratatui/   Ratatui widget (native Rust frontend)
  plotui-ffi/       C ABI (cdylib + staticlib) behind the Go bindings
python/plotui/      the Python package + Textual `PlotWidget`
go/                 Go bindings + `teaplot`, the Bubble Tea v2 component
examples/           raw_demo.py (Kitty images), textual_demo.py
```

`core` and `protocol` are pure and I/O-free, so the same engine can back every
frontend and be unit-tested by hashing pixel buffers. The protocol layer emits
Kitty graphics escapes — chunked, placement-aware, and wrapped for tmux
passthrough — as pure functions of the RGBA frame.

## Integrations

Each TUI framework gets a first-class widget, not a port. All frontends sit on
the same policy crates (`plotui-term` for detection/tmux/render policy,
`plotui-bind` for argument validation and its exact error strings), so a plot
looks and behaves identically whichever framework hosts it — down to the error
messages.

| Frontend | How it works | Where in the codebase | Try it |
| --- | --- | --- | --- |
| **Textual** (Python) | `PlotWidget` wraps the `plotui._plotui` native module (PyO3). Mouse events route to the camera, hover/click picking arrives as Textual messages, `extend` streams points in-place, and text overlays splice into the image without re-rasterizing. | `python/plotui/textual.py`; native module in `crates/plotui-py` | `python examples/textual_graph.py` |
| **Ratatui** (Rust) | A native `StatefulWidget` plus an app-owned `PlotState`: hand it crossterm events, draw it like any other widget — frames and Kitty placement ride ratatui's own buffer diff, flicker-free. | `crates/plotui-ratatui` | `cargo run -p plotui-ratatui --example demo` |
| **Bubble Tea** (Go) | `teaplot.New(plot)` returns an Elm-style model: `Update` consumes tea mouse/key events, `View` lays out the cell grid, and image escapes leave as `tea.Raw` commands. Links to the Rust engine statically over the `plotui-ffi` C ABI (cgo). | `go/` (bindings) + `go/teaplot` (component); ABI in `crates/plotui-ffi` | `go run ./examples/demo` from `go/` — see [go/README.md](go/README.md) |
| **Browser** (WASM) | The same engine compiled to WebAssembly drives the live demos on the website: pointer events feed the engine's own camera, and every frame is its RGBA bytes blitted onto a canvas. Not a plotting-in-the-browser product — it exists so the site can show the real renderer. | `crates/plotui-wasm`; consumed by `site/` | [plotui.xyz/examples.html](https://plotui.xyz/examples.html) |

The three TUI widgets have feature parity: render-path detection, tmux
passthrough, drag/zoom/pan/keys, picking + hover, the 2D crosshair, text
overlays, half-resolution interaction frames, and streaming extend.

## Install the CLI

`plotui` is also a command-line tool: pipe columns of numbers in, get a
real-pixel chart out — interactive on a TTY (pan, zoom, crosshair), a single
printed frame when piped or with `--static`.

```bash
curl -fsSL https://plotui.xyz/install.sh | sh   # prebuilt binary
brew install sebaheg/tap/plotui                 # Homebrew (macOS / Linux)
pip install plotui                              # prebuilt wheel: the library + the CLI
cargo install plotui                            # build from source
cargo binstall plotui                           # prebuilt, via cargo-binstall
```

```bash
seq 1 100 | LC_ALL=C awk '{print $1, sin($1/10)}' | plotui line
plotui scatter -H -d, data.csv                  # header row + comma-delimited
plotui bar counts.tsv                           # --horizontal, --stack, --group
plotui step states.txt                          # holds its value between samples
plotui hist samples.txt                         # binned automatically
plotui box -H measurements.tsv                  # one box per column
plotui dag pipeline.dot                         # a DAG from a DOT file; hover a task
                                                # to light everything it waits on
plotui line --log-y --title "queue depth" \
            --x-title minute --y-title items    # titles and log scales
plotui line --x-range 0:100 --y-range 0:1       # pin an extent, LO:HI
tail -f app.log | LC_ALL=C awk '{print $2}' \
                | plotui line -f --window 200   # live, on the last 200 samples
plotui example scatter                          # built-in demo scenes, no data needed
plotui example deps                             # plotui's own dependency graph, laid
                                                # out live by a force simulation
plotui example pipeline                         # a nightly forecast DAG, running
```

`--follow` (`-f`) keeps the reader open instead of plotting once at EOF: rows
are parsed as they arrive and appended in place, so the chart grows without a
redraw from scratch and pan/zoom survive. It needs piped input and a terminal
to draw into — a malformed line is skipped rather than fatal, and the count is
reported when you quit. The shape of the input (delimiter, column count,
whether x is a calendar) is settled by the first row and held for the rest of
the stream.

On a long feed the whole run compresses into a sliver, so `--window <N>` keeps
the view on the last N samples and `--last <span>` on the last `30s` / `5m` /
`2h` of x. Neither drops data: the window is a *view*, drawn on the range
slider (which a window switches on) against the entire run, so you can drag
back through everything that has arrived. Doing so hands the view over — a
reader who has scrolled back to an incident does not want the next row to
yank them forward — and **f** goes live again, jumping to the head.

Like every plotui frontend, the CLI needs a terminal with Kitty graphics
(supported terminals below); elsewhere it prints a notice and exits.

## Develop

Requires Rust and Python 3.9+. Build the native module into a virtualenv with
[maturin](https://www.maturin.rs/):

```bash
python -m venv .venv && source .venv/bin/activate
pip install maturin textual
maturin develop --release
```

Then, in a terminal with Kitty graphics support — **Kitty**, **Ghostty**,
**iTerm2 ≥ 3.5**, **WezTerm**, or **Konsole** — for the full-resolution pixel demos:

```bash
python examples/raw_demo.py        # 3D scatter via Kitty images
python examples/textual_demo.py    # embedded in Textual
python examples/textual_graph.py   # interactive graph: hover + click-to-inspect
```

The Textual widget picks its render path per terminal: Unicode-placeholder
Kitty graphics in Kitty/Ghostty, direct Kitty placement in iTerm2/WezTerm/
Konsole — plus Warp, Rio, and VS Code, whose younger Kitty decoders are
supported but still maturing (VS Code needs its
`terminal.integrated.enableImages` setting). plotui only draws
real pixels — terminals without Kitty graphics get a notice naming supported
terminals, never a degraded plot. Override with
`PLOTUI_RENDER=placeholder|direct` or `PlotWidget(..., render_mode=...)`.

## Python API

```python
from plotui import Plot

# 2D: axes, ticks, and a legend appear automatically. Traces added without a
# color take colorway slots in fixed order; `name=` puts a series in the
# legend. Colors accept (r, g, b) tuples or shorthands: "#e63c78", "red".
plot = Plot()
plot.add_line(xs, ys, name="forecast")
plot.add_scatter(xs2, ys2, name="observed")
plot.add_bar(xs3, heights)

# Secondary axes: axis="y2"/"y3" bind a series to an independent right-hand
# axis — its own autoscale and tick column, labels tinted to the series color
# (y2 innermost, y3 outermost). The grid stays with the left axis.
plot.add_line(xs, tokens, name="tokens", axis="y2")
plot.add_line(xs, cpu_minutes, name="cpu min", axis="y3")

# Titles: each buys its own margin, so the plot area shrinks rather than
# drawing over the data. The y title is drawn rotated in the left margin.
plot.set_title("p99 latency")
plot.set_x_title("requests")
plot.set_y_title("ms")

# Ranges and scales: an explicit range pins the *extent* only — no autoscale
# padding, and zoom/pan still compose on top of it (unlike set_x_window,
# which is the live window and supersedes the camera). A log axis ticks in
# powers of ten; values at or below zero have no log coordinate and neither
# set the range nor draw.
plot.set_x_range((0, 100))                         # None restores autoscale
plot.set_y_range((0.1, 1e4))
plot.set_y_log(True)                               # set_x_log for x

# DAGs and pipelines: labelled boxes wired by arrows, laid out by rank. Node
# centres are data coordinates; each box is sized in pixels from its label,
# so zooming spreads the graph apart while the text stays readable.
from plotui import LayeredLayout, from_dot, reachable

layout = LayeredLayout(len(tasks), edges)          # rankdir="TB" or "LR"
plot = Plot()
h = plot.add_graph2d(*layout.positions(), edges,
                     labels=tasks, routes=layout.routes())
plot.set_graph_colors(h, states)                   # repaint as the run advances
lit = reachable(len(tasks), edges, hovered)        # everything it waits on
plot = from_dot(open("pipeline.dot").read())       # or straight from DOT

# 3D: any 3D trace switches the plot to the orbit camera.
plot = Plot()
plot.add_scatter3d(xs, ys, zs, name="Cluster A")   # colors from the colorway

# Colorways: the default sequence is pink/cyan/orange-first; swap it with a
# built-in name or your own list before adding traces.
plot.set_colorway("vivid")                         # "plotui", "muted", "vivid"
plot.set_colorway(["#e63c78", "cyan", (240, 161, 60)])

# Streaming: every add_* returns a trace handle. Append through it instead
# of rebuilding — O(new points), autoscale follows; numpy arrays are read
# in one bulk copy. set_visible toggles a series without losing its handle,
# palette slot, or node indices.
h = plot.add_line([], [], name="loss")
plot.extend(h, xs, ys)                # 3D scatter/line: extend(h, xs, ys, zs)
plot.set_visible(h, False)

# Interaction (forward your framework's events to these):
plot.rotate(d_yaw, d_pitch)
plot.zoom_by(factor)
plot.pan(dx, dy)
plot.reset()

# Render (the frontend places the bytes):
escape = plot.render_kitty(cols, rows, cell_w, cell_h)   # Kitty pixel image
pixels = plot.render_rgba(px_w, px_h)                    # raw RGBA8 bytes
```

Graphs take per-element styling, and the camera/projection state is fully
scriptable — the hooks a host needs for label overlays, camera targeting, and
rebuilding a plot without losing the view:

```python
plot.add_graph3d(xs, ys, zs, edges=[(0, 1), (1, 2)],
                 node_colors=[...],          # one (r, g, b) per node
                 node_sizes=[...],           # per-node radius (else `size`)
                 edge_colors=[...],          # per-edge (r, g, b) (else derived)
                 node_shapes=[...])          # per-node "disc" | "ring" | "square" |
                                             #   "triangle" | "diamond" | "diamond-open" | "dot"
plot.set_show_box(False)                     # hide the 3D orientation cube
plot.set_bounds((x0, y0, z0), (x1, y1, z1))  # pin the data frame (else the nodes'
                                             #   bounding box); None, None restores
plot.set_chrome(grid=(26, 32, 36),           # recolour the non-data chrome to sit on
                frame=(43, 50, 55),          #   your own background: bg (legend box),
                ink=(103, 111, 118))         #   frame, grid, ink, ink_bright

state = plot.camera_state()                  # (yaw, pitch, zoom, pan_x, pan_y)
plot.set_camera_state(*state)                # restore (e.g. onto a new Plot)
plot.project_nodes(px_w, px_h)               # [(x_px, y_px, depth)] per node —
                                             # exact render/pick geometry
```

In Textual, use `plotui.textual.PlotWidget(plot)` and it handles the event
plumbing for you. Pass `pickable=True` to make 3D graph nodes and edges
interactive: hovering lights the element under the cursor up white, and
clicking posts an `ElementPicked` message with `("node", i)` or `("edge", i)`
(see `examples/textual_graph.py`, which opens a slide-in inspector from it).

The widget also supports **text overlays** — `widget.set_overlay([(row, col,
text, style), ...])` splices terminal-crisp text (labels, badges) over the
image in every render mode without re-rasterizing — and exposes a
`widget.dragging` property for hosts that defer work mid-gesture. To customize
interaction in a subclass, override the `apply_rotate` / `apply_pan` /
`apply_zoom` / `apply_reset` / `on_click_at` primitives that every input path
routes through — do **not** override the Textual `on_*` handlers (Textual
dispatches those to every class in the MRO, so both would run).

## License

MIT, except for one embedded asset: chart text is set in
[Martian Mono](https://github.com/evilmartians/mono) (Copyright 2020 The
Martian Mono Project Authors), used under the SIL Open Font License 1.1. The
glyph outlines are compiled into `plotui-core` as
`crates/plotui-core/src/glyphs.rs`; the license travels with them in
`crates/plotui-core/fonts/MartianMono-OFL.txt`. The font carries no Reserved
Font Name, and nothing about the OFL reaches your code — it covers the font
data, not the crate.
