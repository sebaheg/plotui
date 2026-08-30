#!/usr/bin/env python3
"""End-to-end Textual example: a live force-directed dependency graph.

    python examples/textual_deps.py

plotui's own crate graph, laid out by the core ``ForceLayout`` simulation:

- Watch the clusters pull themselves together — the layout runs on a timer
  until it settles, then the app stops repainting on its own.
- Every few seconds a late-arriving crate flies in beside the crate that
  needs it (``ForceLayout.add_node`` + ``PlotWidget.extend_graph``).
- Hover a crate: it and everything it transitively depends on stay at full
  color while the rest of the graph dims (``PlotWidget.set_graph_colors``).
- Drag to rotate, scroll to zoom, `r` to reset, `q` to quit.

The physics is the same Rust simulation behind ``plotui example deps`` — the
host only owns the timer.
"""
from __future__ import annotations

from textual import on
from textual.app import App, ComposeResult
from textual.widgets import Footer, Header

from plotui import ForceLayout, Plot
from plotui.textual import PlotWidget

# plotui's crate graph, snapshotted from `cargo metadata`. Groups: workspace
# crate, direct external dependency, transitive dependency. An edge (a, b)
# means a depends on b.
NODES = [
    ("plotui", "w"), ("plotui-bind", "w"), ("plotui-core", "w"),
    ("plotui-ffi", "w"), ("plotui-protocol", "w"), ("plotui-py", "w"),
    ("plotui-ratatui", "w"), ("plotui-term", "w"), ("plotui-wasm", "w"),
    ("base64", "e"), ("clap", "e"), ("crossterm", "e"), ("flate2", "e"),
    ("numpy", "e"), ("pyo3", "e"), ("ratatui", "e"), ("rustix", "e"),
    ("wasm-bindgen", "e"),
    ("bitflags", "t"), ("cfg-if", "t"), ("clap_builder", "t"),
    ("clap_derive", "t"), ("crc32fast", "t"), ("crossterm_winapi", "t"),
    ("derive_more", "t"), ("document-features", "t"), ("errno", "t"),
    ("filedescriptor", "t"), ("indoc", "t"), ("instability", "t"),
    ("libc", "t"), ("linux-raw-sys", "t"), ("memoffset", "t"),
    ("miniz_oxide", "t"), ("mio", "t"), ("ndarray", "t"),
    ("num-complex", "t"), ("num-integer", "t"), ("num-traits", "t"),
    ("once_cell", "t"), ("parking_lot", "t"), ("portable-atomic", "t"),
    ("pyo3-ffi", "t"), ("pyo3-macros", "t"), ("ratatui-core", "t"),
    ("ratatui-crossterm", "t"), ("ratatui-macros", "t"),
    ("ratatui-termina", "t"), ("ratatui-termwiz", "t"),
    ("ratatui-widgets", "t"), ("rustc-hash", "t"), ("serde", "t"),
    ("signal-hook", "t"), ("signal-hook-mio", "t"), ("unindent", "t"),
    ("wasm-bindgen-macro", "t"), ("wasm-bindgen-shared", "t"),
    ("winapi", "t"), ("windows-sys", "t"),
]
EDGES = [
    (0, 2), (0, 6), (0, 7), (0, 10), (0, 11), (0, 15), (1, 2), (3, 1), (3, 2),
    (3, 4), (3, 7), (4, 2), (4, 9), (4, 12), (5, 1), (5, 2), (5, 4), (5, 7),
    (5, 13), (5, 14), (6, 2), (6, 4), (6, 7), (6, 11), (7, 2), (7, 4), (7, 16),
    (8, 1), (8, 2), (8, 17), (10, 20), (10, 21), (11, 16), (11, 18), (11, 23),
    (11, 24), (11, 25), (11, 27), (11, 34), (11, 40), (11, 52), (11, 53),
    (11, 57), (12, 22), (12, 33), (13, 14), (13, 30), (13, 35), (13, 36),
    (13, 37), (13, 38), (13, 50), (14, 28), (14, 30), (14, 32), (14, 39),
    (14, 41), (14, 42), (14, 43), (14, 54), (15, 29), (15, 44), (15, 45),
    (15, 46), (15, 47), (15, 48), (15, 49), (15, 51), (16, 18), (16, 26),
    (16, 30), (16, 31), (16, 58), (17, 19), (17, 39), (17, 55), (17, 56),
]
# The last FLY_IN crates arrive live instead of at t=0; their edges only
# reference earlier indices, so insertion order is safe.
FLY_IN = 8
SEED = 20260830
SETTLED = 1e-3

STYLE = {  # color, size, shape per group
    "w": ((230, 60, 120), 5.0, "disc"),
    "e": ((69, 200, 209), 4.0, "ring"),
    "t": ((240, 161, 60), 2.6, "dot"),
}


def dim(c):
    return (c[0] // 4, c[1] // 4, c[2] // 4)


class DepsApp(App):
    TITLE = "plotui — live dependency graph"
    BINDINGS = [("q", "quit", "Quit"), ("r", "reset_view", "Reset view")]

    def __init__(self) -> None:
        super().__init__()
        self.n = len(NODES) - FLY_IN
        self.edges = [(a, b) for a, b in EDGES if a < self.n and b < self.n]
        self.layout = ForceLayout(self.n, self.edges, seed=SEED)
        for _ in range(30):  # warm up past the initial explosion
            self.layout.step()
        self.energy = float("inf")
        self.base = [STYLE[g][0] for _, g in NODES[: self.n]]
        self.plot = Plot()
        self.plot.set_show_box(False)
        self.plot.set_bounds((-1.25, -1.25, -1.25), (1.25, 1.25, 1.25))
        xs, ys, zs = self.layout.positions()
        self.handle = self.plot.add_graph3d(
            xs, ys, zs,
            edges=self.edges,
            node_colors=self.base,
            size=2.6,  # fallback for flown-in nodes (all transitive)
            node_sizes=[STYLE[g][1] for _, g in NODES[: self.n]],
            node_shapes=[STYLE[g][2] for _, g in NODES[: self.n]],
        )

    def compose(self) -> ComposeResult:
        yield Header()
        yield PlotWidget(self.plot, id="plot", pickable=True)
        yield Footer()

    def on_mount(self) -> None:
        self.query_one("#plot", PlotWidget).focus()
        self.set_interval(1 / 30, self._tick)
        self.set_interval(2.5, self._spawn)

    def _tick(self) -> None:
        if self.energy < SETTLED:
            return
        self.energy = self.layout.step()
        widget = self.query_one("#plot", PlotWidget)
        xs, ys, zs = self.layout.positions()
        widget.set_graph_positions(self.handle, xs, ys, zs)

    def _spawn(self) -> None:
        if self.n >= len(NODES):
            return
        idx = self.n
        new_edges = [(a, b) for a, b in EDGES if a == idx or b == idx]
        neighbors = [b if a == idx else a for a, b in new_edges]
        self.layout.add_node(neighbors)
        name, group = NODES[idx]
        color = STYLE[group][0]
        xs, ys, zs = self.layout.positions()
        widget = self.query_one("#plot", PlotWidget)
        widget.extend_graph(
            self.handle, [xs[idx]], [ys[idx]], [zs[idx]],
            node_colors=[color], edges=new_edges,
        )
        self.base.append(color)
        self.edges.extend(new_edges)
        self.n += 1
        self.energy = float("inf")  # re-heated: keep rendering
        self.sub_title = f"+ {name}"

    def _reachable(self, i: int) -> set[int]:
        seen, stack = {i}, [i]
        while stack:
            a = stack.pop()
            for x, y in self.edges:
                if x == a and y not in seen:
                    seen.add(y)
                    stack.append(y)
        return seen

    @on(PlotWidget.ElementHovered)
    def _hovered(self, message: PlotWidget.ElementHovered) -> None:
        widget = self.query_one("#plot", PlotWidget)
        el = message.element
        if el is not None and el[0] == "node" and el[1] < self.n:
            i = el[1]
            on_path = self._reachable(i)
            node_colors = [
                self.base[j] if j in on_path else dim(self.base[j])
                for j in range(self.n)
            ]
            edge_colors = [
                (170, 175, 185) if a in on_path and b in on_path else (34, 38, 48)
                for a, b in self.edges
            ]
            widget.set_graph_colors(self.handle, node_colors, edge_colors)
            self.sub_title = f"{NODES[i][0]} · {len(on_path) - 1} deps"
        else:
            widget.set_graph_colors(self.handle, list(self.base))
            self.sub_title = ""

    def action_reset_view(self) -> None:
        self.plot.reset()
        self.query_one("#plot", PlotWidget).invalidate()


if __name__ == "__main__":
    DepsApp().run()
