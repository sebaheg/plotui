# plotui

**Interactive 2D/3D plots in the terminal — Plotly-style — for Textual, powered by a Rust core and the Kitty graphics protocol.**

`plotui` renders scatter plots (and, soon, lines / surfaces / bars) as real
pixel graphics inside a terminal, and lets you rotate, pan, and zoom them. It's
built to drop into a [Textual](https://textual.textualize.io/) app, with the
rendering engine written in Rust so it stays fast in 2D and 3D.

> Status: **early scaffold.** Working today: a 3D scatter engine, a Kitty-image
> raw demo, and a Textual widget (half-block rendering). See the roadmap below.

## Architecture

The one rule that shapes everything: **the Rust core owns pixels, not the
terminal.** It has no event loop and no input handling — the TUI framework
(Textual now; Bubble Tea / Ratatui later) owns the loop, forwards input to the
camera, and asks for a frame.

```
crates/
  plotui-core/      pure engine: data model, 3D camera, rasterizer → RGBA
  plotui-protocol/  RGBA → terminal bytes (Kitty graphics; half-block fallback)
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

Then, in a **Kitty** or **Ghostty** terminal (for the pixel-graphics demo):

```bash
python examples/raw_demo.py        # 3D scatter via Kitty images
python examples/textual_demo.py    # embedded in Textual (half-block)
```

## Python API

```python
from plotui import Plot

plot = Plot()
plot.add_scatter3d(xs, ys, zs, color=(230, 60, 120), size=2.0)

# Interaction (forward your framework's events to these):
plot.rotate(d_yaw, d_pitch)
plot.zoom_by(factor)
plot.pan(dx, dy)
plot.reset()

# Render (the frontend places the bytes):
escape = plot.render_kitty(cols, rows, cell_w, cell_h)   # Kitty pixel image
lines  = plot.render_halfblock(cols, rows)               # universal text fallback
```

In Textual, use `plotui.textual.PlotWidget(plot)` and it handles the event
plumbing for you.

## Roadmap

- [ ] Flicker-free Kitty placement via Unicode-placeholder virtual placement
      (fixed image id, atomic replace) — wire the pixel path into the Textual widget
- [ ] 2D traces: scatter, line, bar, step; axes, ticks, tick labels, legend
- [ ] 3D surface / mesh; axis cube with labels
- [ ] Interactive hover / pick (inverse projection + spatial index)
- [ ] numpy zero-copy input
- [ ] Sixel + iTerm2 protocol encoders; graceful auto-detection
- [ ] Prebuilt wheels (maturin + cibuildwheel)
- [ ] Bubble Tea (cgo) and Ratatui (native) frontends

## License

MIT
# plotui
