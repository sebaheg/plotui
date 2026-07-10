"""plotui — interactive 2D/3D terminal plots.

The heavy lifting (data model, 3D camera, rasterization) lives in a Rust core
exposed as the native ``plotui._plotui`` module. Python owns the event loop and
input; the native side is a stateless-ish rendering engine.

Quick start::

    from plotui import Plot
    plot = Plot()
    plot.add_scatter3d(xs, ys, zs, color=(230, 60, 120))
    # In a raw loop: escape = plot.render_kitty(cols, rows, cell_w, cell_h)
    # In Textual:   use plotui.textual.PlotWidget(plot)
"""

from ._plotui import Plot

__all__ = ["Plot"]
__version__ = "0.1.0"
