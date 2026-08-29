#!/usr/bin/env python3
"""Streaming demo: live data appended to a plot 20 times a second.

    python examples/textual_stream.py

Every `add_*` call returns a trace handle; `widget.extend(handle, xs, ys)`
appends points and repaints — no plot rebuild, cost proportional to the new
points only. `1`/`2`/`3` toggle series visibility by handle, q quits.
"""
import math
import random

from textual.app import App, ComposeResult
from textual.widgets import Footer, Header

from plotui import Plot
from plotui.textual import PlotWidget


class StreamApp(App):
    TITLE = "plotui — streaming demo"
    BINDINGS = [
        ("q", "quit", "Quit"),
        ("1", "toggle(0)", "Toggle forecast"),
        ("2", "toggle(1)", "Toggle observed"),
        ("3", "toggle(2)", "Toggle load"),
    ]

    def compose(self) -> ComposeResult:
        plot = Plot()
        self.h_forecast = plot.add_line([], [], name="forecast")
        self.h_observed = plot.add_scatter([], [], name="observed", size=1.8)
        self.h_load = plot.add_line([], [], name="load", axis="y2", width=1.0)
        self.handles = [self.h_forecast, self.h_observed, self.h_load]
        yield Header()
        yield PlotWidget(plot, crosshair=True, id="plot")
        yield Footer()

    def on_mount(self) -> None:
        self.t = 0.0
        self.shown = {h: True for h in self.handles}
        self.set_interval(1 / 20, self.feed)

    def feed(self) -> None:
        widget = self.query_one("#plot", PlotWidget)
        t = self.t = self.t + 0.25
        base = math.sin(t * 0.4) * 2.0 + math.sin(t * 0.09) * 4.0
        widget.extend(self.h_forecast, [t], [base])
        widget.extend(self.h_observed, [t], [base + random.gauss(0.0, 0.5)])
        widget.extend(self.h_load, [t], [40.0 + 12.0 * math.sin(t * 0.13 + 1.0)])

    def action_toggle(self, handle: int) -> None:
        self.shown[handle] = not self.shown[handle]
        self.query_one("#plot", PlotWidget).set_visible(handle, self.shown[handle])


if __name__ == "__main__":
    StreamApp().run()
