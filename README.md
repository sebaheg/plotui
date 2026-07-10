# plotui

**Interactive 2D/3D plots in the terminal — Plotly-style — for Textual, powered by a Rust core and the Kitty graphics protocol.**

`plotui` renders scatter plots (and, soon, lines / surfaces / bars) as real
pixel graphics inside a terminal, and lets you rotate, pan, and zoom them. It's
built to drop into a [Textual](https://textual.textualize.io/) app, with the
rendering engine written in Rust so it stays fast in 2D and 3D.

> Status: **early scaffold.** Working today: 2D scatter/line/bar charts with
> axes, ticks, and a legend; a 3D scatter/graph engine; a Kitty-image raw demo;
> and a Textual widget. See the roadmap below.

## Architecture

The one rule that shapes everything: **the Rust core owns pixels, not the
terminal.** It has no event loop and no input handling — the TUI framework
(Textual now; Bubble Tea / Ratatui later) owns the loop, forwards input to the
camera, and asks for a frame.

```
crates/
  plotui-core/      pure engine: data model, 3D camera, rasterizer → RGBA
  plotui-protocol/  RGBA → terminal bytes (Kitty graphics protocol)
  plotui-py/        PyO3 bindings → the `plotui._plotui` native module
python/plotui/      the Python package + Textual `PlotWidget`
examples/           raw_demo.py (Kitty images), textual_demo.py
```

`core` and `protocol` are pure and I/O-free, so the same engine can back every
frontend and be unit-tested by hashing pixel buffers.

## Develop

Requires Rust and Python 3.9+. Build the native module into a virtualenv with
[maturin](https://www.maturin.rs/):

```bash
python -m venv .venv && source .venv/bin/activate
pip install maturin textual
maturin develop --release
```

Then, in a terminal with Kitty graphics support — **Kitty**, **Ghostty**,
**iTerm2 ≥ 3.5**, or **WezTerm** — for the full-resolution pixel demos:

```bash
python examples/raw_demo.py        # 3D scatter via Kitty images
python examples/textual_demo.py    # embedded in Textual
python examples/textual_graph.py   # interactive graph: hover + click-to-inspect
```

The Textual widget picks its render path per terminal: Unicode-placeholder
Kitty graphics in Kitty/Ghostty, direct Kitty placement in iTerm2/WezTerm/
Konsole (they speak the protocol but not placeholders). plotui only draws
real pixels — terminals without Kitty graphics get a notice naming supported
terminals, never a degraded plot. Override with
`PLOTUI_RENDER=placeholder|direct` or `PlotWidget(..., render_mode=...)`.

## Python API

```python
from plotui import Plot

# 2D: axes, ticks, and a legend appear automatically. Traces added without a
# color take palette slots in fixed order; `name=` puts a series in the legend.
plot = Plot()
plot.add_line(xs, ys, name="forecast")
plot.add_scatter(xs2, ys2, name="observed")
plot.add_bar(xs3, heights)

# 3D: any 3D trace switches the plot to the orbit camera.
plot = Plot()
plot.add_scatter3d(xs, ys, zs, color=(230, 60, 120), size=2.0)

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
                 edge_colors=[...])          # per-edge (r, g, b) (else derived)
plot.set_show_box(False)                     # hide the 3D orientation cube

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
- [ ] 2D step trace; axis titles; time-formatted x ticks
- [ ] 3D surface / mesh; axis cube with labels
- [x] Interactive hover / pick for 3D graph nodes *and* edges (opt-in via
      `PlotWidget(..., pickable=True)`: hover lights the element up white,
      click posts `ElementPicked`)
- [ ] Hover / pick for 2D traces; spatial index for large graphs
- [ ] numpy zero-copy input
- [x] Graceful render-path auto-detection (placeholder / direct Kitty, with a
      supported-terminals notice elsewhere and a `PLOTUI_RENDER` override)
- [ ] Sixel + iTerm2 OSC 1337 encoders for terminals without Kitty graphics
- [ ] Prebuilt wheels (maturin + cibuildwheel)
- [ ] Bubble Tea (cgo) and Ratatui (native) frontends

## License

MIT
