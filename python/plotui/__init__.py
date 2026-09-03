"""plotui — interactive 2D/3D terminal plots.

The heavy lifting (data model, 3D camera, rasterization) lives in a Rust core
exposed as the native ``plotui._plotui`` module. Python owns the event loop and
input; the native side is a stateless-ish rendering engine.

Quick start::

    from plotui import Plot
    plot = Plot()
    h = plot.add_line(xs, ys, name="forecast") # 2D: axes/ticks/legend appear
    plot.add_line(xs, costs, axis="y2")        # independent right-hand axis
    plot.add_line(xs, temps, color="red")      # shorthands: "red", "#e63c78"
    plot.set_colorway("vivid")                 # or a list of colors
    plot.add_scatter3d(xs, ys, zs, name="A")   # any 3D trace -> orbit camera
    plot.add_line3d(xs, ys, zs)                # 3D trajectory/curve
    plot.add_surface3d(xs, ys, Z)              # grid surface, viridis by default
    plot.extend(h, more_xs, more_ys)           # stream data by trace handle

Pipelines and DAGs::

    from plotui import LayeredLayout, Plot, from_dot, reachable
    plot = from_dot("digraph { fetch -> clean -> publish }")  # a DOT subset
    # or lay one out yourself and colour it live:
    layout = LayeredLayout(n_nodes, edges)     # rankdir="TB" or "LR"
    xs, ys = layout.positions()
    h = plot.add_graph2d(xs, ys, edges, labels=names,
                         routes=layout.routes())
    plot.set_graph_colors(h, states)           # repaint as the run advances
    lit = reachable(n_nodes, edges, hovered)   # everything it waits on
    # In a raw loop: escape = plot.render_kitty(cols, rows, cell_w, cell_h)
    # In Textual:   use plotui.textual.PlotWidget(plot)
"""

from ._plotui import (
    ForceLayout,
    LayeredLayout,
    Plot,
    __version__,
    from_dot,
    reachable,
)

# `__version__` comes from the native module, which takes it from the crate:
# one number for the whole build. Repeating it here is how it drifted to
# 0.3.0 while the crates were on 0.4.2.
__all__ = ["ForceLayout", "LayeredLayout", "Plot", "__version__", "from_dot", "reachable"]
