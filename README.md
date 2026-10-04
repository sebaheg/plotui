<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/sebaheg/plotui/main/.github/assets/plotui-logo.svg">
  <img alt="plotui" src="https://raw.githubusercontent.com/sebaheg/plotui/main/.github/assets/plotui-logo-light.svg" width="440">
</picture>

**Beautiful and interactive plots, right inside your terminal.**<br>
Plotly-style 2D and 3D charts drawn as real pixels by a Rust engine, from the
command line or inside Textual, Ratatui and Bubble Tea apps.

[Website](https://plotui.xyz) ·
[Docs](https://docs.plotui.xyz/docs) ·
[Quickstart](https://docs.plotui.xyz/docs/quickstart) ·
[Examples](https://plotui.xyz/examples) ·
[Changelog](https://github.com/sebaheg/plotui/blob/main/CHANGELOG.md)

[![PyPI](https://img.shields.io/pypi/v/plotui.svg)](https://pypi.org/project/plotui/)
[![crates.io](https://img.shields.io/crates/v/plotui.svg)](https://crates.io/crates/plotui)
[![CI](https://github.com/sebaheg/plotui/actions/workflows/ci.yml/badge.svg)](https://github.com/sebaheg/plotui/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/sebaheg/plotui/blob/main/LICENSE)

</div>

---

plotui draws line, scatter, bar, histogram, box, heatmap, 3D scatter, surface
and DAG charts as real pixel graphics over the Kitty graphics protocol. Rotate,
pan, zoom and hover them with the mouse, and stream new data into a chart
without redrawing it from scratch.

One engine sits behind every frontend, so a plot looks and behaves the same in
the CLI, in Python, in Rust and in Go.

## Quickstart

```bash
pip install plotui        # or: brew install sebaheg/tap/plotui · cargo install plotui
plotui example scatter    # a built-in demo, no data needed
```

Pipe columns of numbers in, get a chart out. Interactive on a TTY, a single
frame when piped or with `--static`:

```bash
seq 1 100 | LC_ALL=C awk '{print $1, sin($1/10)}' | plotui line
plotui scatter -H -d, data.csv                       # header row, comma-delimited
plotui dag pipeline.dot                              # a DAG from a DOT file
tail -f app.log | LC_ALL=C awk '{print $2}' | plotui line -f --window 200   # live
```

Or build a plot in Python and drop it into a Textual app:

```python
from textual.app import App
from plotui import Plot
from plotui.textual import PlotWidget

plot = Plot()
plot.add_line(hours, observed, name="observed")
plot.add_line(hours, forecast, name="forecast")

class Chart(App):
    def compose(self):
        yield PlotWidget(plot)

Chart().run()
```

## Frontends

| Frontend | Package | Try it |
|---|---|---|
| CLI | `plotui` (pip, brew, cargo) | `plotui example pipeline` |
| Textual (Python) | `pip install plotui` | `python examples/textual_graph.py` |
| Ratatui (Rust) | `plotui-ratatui` | `cargo run -p plotui-ratatui --example demo` |
| Bubble Tea (Go) | `github.com/sebaheg/plotui/go` | see [go/README.md](https://github.com/sebaheg/plotui/blob/main/go/README.md) |

plotui needs a terminal with Kitty graphics: **Kitty**, **Ghostty**,
**iTerm2 ≥ 3.5**, **WezTerm** or **Konsole**, with Warp, Rio and VS Code
supported but still maturing. Anywhere else it prints a notice instead of a
degraded plot. The full list is in [terminal support](https://docs.plotui.xyz/docs/terminals).

## Learn more

<table>
<tr>
<td width="50%" valign="top">

### [Website →](https://plotui.xyz)

Live demos running the real engine in your browser, and a
[gallery of examples](https://plotui.xyz/examples).

</td>
<td width="50%" valign="top">

### [Docs →](https://docs.plotui.xyz/docs)

[Installation](https://docs.plotui.xyz/docs/installation),
[2D charts](https://docs.plotui.xyz/docs/charts-2d),
[3D plots](https://docs.plotui.xyz/docs/plots-3d),
[interaction and streaming](https://docs.plotui.xyz/docs/interaction) and the
[Textual widget](https://docs.plotui.xyz/docs/textual-widget).

</td>
</tr>
</table>

## Develop

Requires Rust and Python 3.9+. Build the native module into a virtualenv with
[maturin](https://www.maturin.rs/):

```bash
python -m venv .venv && source .venv/bin/activate
pip install maturin textual
maturin develop --release
python examples/textual_demo.py
```

## License

[MIT](https://github.com/sebaheg/plotui/blob/main/LICENSE)
