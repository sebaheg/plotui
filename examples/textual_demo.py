#!/usr/bin/env python3
"""Textual demo: an interactive 3D scatter embedded in a Textual app.

    python examples/textual_demo.py

Textual owns the loop and input; the PlotWidget forwards events to the plot and
renders full-resolution Kitty graphics. Drag to rotate, scroll to zoom, r to reset.
"""
import math

from textual.app import App, ComposeResult
from textual.widgets import Footer, Header

from plotui import Plot
from plotui.textual import PlotWidget


def make_plot() -> Plot:
    plot = Plot()
    xs, ys, zs = [], [], []
    n = 1600
    for i in range(n):
        t = i / n * 6.0 * math.pi
        strand = 1.0 if i % 2 == 0 else -1.0
        xs.append(math.cos(t) * strand)
        ys.append(t / (6.0 * math.pi) * 2.0 - 1.0)
        zs.append(math.sin(t) * strand)
    plot.add_scatter3d(xs, ys, zs, color=(230, 60, 120), size=2.0)
    return plot


class PlotApp(App):
    TITLE = "plotui — Textual demo"
    BINDINGS = [("q", "quit", "Quit"), ("r", "reset", "Reset view")]

    def compose(self) -> ComposeResult:
        yield Header()
        yield PlotWidget(make_plot(), auto_rotate=True, id="plot")
        yield Footer()

    def action_reset(self) -> None:
        self.query_one("#plot", PlotWidget)._plot.reset()
        self.query_one("#plot", PlotWidget).refresh()


if __name__ == "__main__":
    PlotApp().run()
