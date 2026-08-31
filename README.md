# plotui

**Interactive 2D/3D plots in the terminal — Plotly-style — for Textual, Ratatui, and Bubble Tea, powered by a Rust core and the Kitty graphics protocol.**

`plotui` renders scatter plots (and, soon, lines / surfaces / bars) as real
pixel graphics inside a terminal, and lets you rotate, pan, and zoom them. It
drops into a [Textual](https://textual.textualize.io/),
[Ratatui](https://ratatui.rs/), or
[Bubble Tea](https://github.com/charmbracelet/bubbletea) app as a first-class
widget, with the rendering engine written in Rust so it stays fast in 2D and 3D.

> Status: **early scaffold.** Working today: 2D scatter/line/bar charts with
> axes, ticks, and a legend; a 3D scatter/graph engine; a Kitty-image raw demo;
> and a Textual widget. See the roadmap below.

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
plotui bar counts.tsv
plotui example scatter                          # built-in demo scenes, no data needed
plotui example deps                             # plotui's own dependency graph, laid
                                                # out live by a force simulation
```

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

## Roadmap

- [x] Flicker-free Kitty placement via Unicode-placeholder virtual placement
      (fixed image id, atomic replace) — wire the pixel path into the Textual widget
- [x] 2D traces: scatter, line, bar; axes, ticks, tick labels, legend
- [x] Independent right-hand y-axes (`axis="y2"`/`"y3"`) with tinted tick labels
- [x] Time axes: datetime x input (numpy `datetime64`, pandas, `datetime`,
      ISO-8601 CLI columns) with calendar-boundary ticks and date readouts
- [x] Range slider (Plotly-style): an x-window with windowed y-autoscale and
      an interactive overview strip — engine-drawn, so identical in every
      frontend (`plotui example timeseries`, `--range-slider`,
      `PlotWidget(..., range_slider=True)`)
- [ ] 2D step trace; axis titles
- [ ] Axis cube with labels
- [x] Interactive hover / pick for 3D graph nodes *and* edges (opt-in via
      `PlotWidget(..., pickable=True)`: hover lights the element up white,
      click posts `ElementPicked`)
- [ ] Hover / pick for 2D traces; spatial index for large graphs
- [x] Streaming append: trace handles, `extend`, `set_visible`, incremental bounds
- [x] numpy fast-path input (one bulk copy, no per-element conversion)
- [ ] Rolling window (`max_points`) for endless streams
- [x] Graceful render-path auto-detection (placeholder / direct Kitty, with a
      supported-terminals notice elsewhere and a `PLOTUI_RENDER` override)
- [ ] Sixel + iTerm2 OSC 1337 encoders for terminals without Kitty graphics
- [x] Prebuilt wheels (maturin + GitHub Actions): `pip install plotui` — macOS
      arm64/x86_64, Linux x86_64/aarch64, abi3 ≥ 3.9; the wheel bundles the
      CLI binary
- [x] Ratatui frontend (native): `plotui-ratatui` — StatefulWidget + app-owned
      PlotState, full parity with the Textual widget
      (`cargo run -p plotui-ratatui --example demo`)
- [x] Bubble Tea frontend (cgo): `go/` bindings over the `plotui-ffi` C ABI +
      the `teaplot` component for Bubble Tea v2 (see `go/README.md`)
- [x] CLI: `plotui line|scatter|bar` from stdin or a file — interactive on a
      TTY, one static frame when piped; installed via curl, Homebrew, cargo,
      or pip (see Install above)
- [x] CLI examples:
      `plotui example scatter|graph|stream|timeseries|deps|lidar|mandelbulb`
      — self-contained demo scenes, no input data needed
- [x] Force-directed graphs: a `ForceLayout` simulation in the core (exposed
      in Python, Go, and JS) plus in-place graph mutation — move, recolor,
      and grow a live graph without rebuilds (`plotui example deps`)
- [x] 3D surfaces and triangle meshes: Gouraud-shaded geometry with height
      colormaps, plus a `marching_cubes` helper that turns a sampled scalar
      field into a mesh (`plotui example mandelbulb`; meshes in Rust and JS
      so far, Python to follow)
- [x] Animation export: `--out file.mp4|gif|webm` records any animated
      example headlessly via ffmpeg (`plotui example lidar --out demo.mp4`);
      `--out file.png` takes one frame of any chart or example
- [ ] CLI v2: `--follow` streaming, `scatter3d`, histogram/density/count
      transforms
- [ ] Prebuilt static libs for the Go bindings (today: local source build)

## License

MIT
