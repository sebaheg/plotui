#!/usr/bin/env python3
"""End-to-end Textual example: an interactive 3D graph with a click-to-inspect
sidebar.

    python examples/textual_graph.py

- Drag to rotate, scroll to zoom, `r` to reset.
- Click a node: a sidebar slides out on the right with that node's details.
- Click empty space or press `escape` to close it.

Textual owns the loop and input; `PlotWidget` forwards events to the Rust core,
renders the frame, and posts a `NodePicked` message on click. The node metadata
(labels, neighbours) lives here in Python — the Rust core only returns the index
of the node that was hit, keeping the engine purely numeric.
"""
from __future__ import annotations

import random

from textual import on
from textual.app import App, ComposeResult
from textual.containers import Horizontal, Vertical
from textual.widgets import Footer, Header, Static

from plotui import Plot
from plotui.textual import PlotWidget

PINK = (230, 60, 120)
GREEN = (70, 190, 120)


def _lerp(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(3))


def build_graph(n: int = 44, seed: int = 7):
    """A labelled 3D random geometric graph: nodes in a ball, edges within a
    radius, coloured pink→green by degree."""
    rng = random.Random(seed)
    pts = []
    while len(pts) < n:
        x, y, z = rng.uniform(-1, 1), rng.uniform(-1, 1), rng.uniform(-1, 1)
        if x * x + y * y + z * z <= 1.0:
            pts.append((x, y, z))
    radius = (9.0 / n) ** (1.0 / 3.0)
    edges = []
    adj = [[] for _ in range(n)]
    for i in range(n):
        for j in range(i + 1, n):
            d = sum((pts[i][k] - pts[j][k]) ** 2 for k in range(3)) ** 0.5
            if d < radius:
                edges.append((i, j))
                adj[i].append(j)
                adj[j].append(i)
    deg = [len(a) for a in adj]
    dmax = max(deg) or 1
    colors = [_lerp(PINK, GREEN, (deg[i] / dmax) ** 1.4) for i in range(n)]
    meta = [
        {
            "label": f"N{i:02d}",
            "degree": deg[i],
            "pos": pts[i],
            "neighbors": [f"N{j:02d}" for j in sorted(adj[i])],
        }
        for i in range(n)
    ]
    return pts, edges, colors, meta


class GraphApp(App):
    TITLE = "plotui — 3D graph"
    CSS = """
    #body { height: 1fr; }
    #plot { width: 1fr; height: 1fr; }

    #sidebar {
        width: 0;
        height: 1fr;
        display: none;
        background: $panel;
        padding: 0 1;
    }
    #sidebar.open {
        width: 40;
        display: block;
        border-left: thick $accent;
    }
    #sb-title { color: $accent; text-style: bold; padding: 1 0 0 0; }
    #sb-hint { color: $text-muted; }
    """

    BINDINGS = [
        ("q", "quit", "Quit"),
        ("r", "reset_view", "Reset view"),
        ("escape", "close_sidebar", "Close panel"),
    ]

    def __init__(self) -> None:
        super().__init__()
        pts, edges, colors, self.meta = build_graph()
        self.plot = Plot()
        self.plot.add_graph3d(
            [p[0] for p in pts],
            [p[1] for p in pts],
            [p[2] for p in pts],
            edges,
            node_colors=colors,
            size=4.0,
        )

    def compose(self) -> ComposeResult:
        yield Header()
        with Horizontal(id="body"):
            yield PlotWidget(self.plot, id="plot")
            with Vertical(id="sidebar"):
                yield Static("", id="sb-title")
                yield Static("", id="sb-body")
                yield Static("\nclick empty space or press esc to close", id="sb-hint")
        yield Footer()

    def on_mount(self) -> None:
        self.query_one("#plot", PlotWidget).focus()

    @on(PlotWidget.NodePicked)
    def _node_picked(self, message: PlotWidget.NodePicked) -> None:
        idx = message.index
        if idx is None or idx >= len(self.meta):
            self._close_sidebar()
            return
        m = self.meta[idx]
        px, py, pz = m["pos"]
        self.query_one("#sb-title", Static).update(m["label"])
        self.query_one("#sb-body", Static).update(
            f"node index : {idx}\n"
            f"degree     : {m['degree']}\n"
            f"position   : ({px:+.2f}, {py:+.2f}, {pz:+.2f})\n\n"
            f"neighbours ({len(m['neighbors'])}):\n"
            + (", ".join(m["neighbors"]) or "—")
        )
        self.query_one("#sidebar").add_class("open")

    def _close_sidebar(self) -> None:
        self.query_one("#sidebar").remove_class("open")
        self.plot.set_selected(None)
        self.query_one("#plot", PlotWidget).invalidate()

    def action_close_sidebar(self) -> None:
        self._close_sidebar()

    def action_reset_view(self) -> None:
        self.plot.reset()
        self.query_one("#plot", PlotWidget).invalidate()


if __name__ == "__main__":
    GraphApp().run()
