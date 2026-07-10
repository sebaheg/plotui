"""Textual integration for plotui.

``PlotWidget`` embeds an interactive plot inside a Textual app. Textual owns the
loop and input; the widget forwards mouse/key events to the plot's camera and
asks the Rust core for a fresh frame on each refresh.

This first version renders through the **half-block** path, which composites
natively through Textual's normal cell pipeline (it's just colored characters),
so it works in any truecolor terminal today. The pixel-perfect Kitty-image path
(``Plot.render_kitty``) is proven by ``examples/raw_demo.py``; wiring it into a
Textual widget via Kitty's Unicode-placeholder placement is the next iteration.
"""

from __future__ import annotations

from rich.text import Text
from textual import events
from textual.widget import Widget

from ._plotui import Plot


class PlotWidget(Widget, can_focus=True):
    """A Textual widget hosting an interactive plotui plot."""

    DEFAULT_CSS = """
    PlotWidget {
        width: 1fr;
        height: 1fr;
    }
    """

    def __init__(self, plot: Plot, *, auto_rotate: bool = False, **kwargs):
        super().__init__(**kwargs)
        self._plot = plot
        self._dragging = False
        self._auto = auto_rotate

    def on_mount(self) -> None:
        if self._auto:
            # ~30 fps spin.
            self.set_interval(1 / 30, self._tick)

    def _tick(self) -> None:
        self._plot.rotate(0.02, 0.0)
        self.refresh()

    # ---- rendering: half-block, composited by Textual ----
    def render(self) -> Text:
        w, h = self.size.width, self.size.height
        if w <= 0 or h <= 0:
            return Text("")
        ansi = self._plot.render_halfblock(w, h)
        return Text.from_ansi(ansi)

    # ---- interaction ----
    def on_mouse_down(self, event: events.MouseDown) -> None:
        self._dragging = True
        self.capture_mouse()
        self.focus()

    def on_mouse_up(self, event: events.MouseUp) -> None:
        self._dragging = False
        self.release_mouse()

    def on_mouse_move(self, event: events.MouseMove) -> None:
        if not self._dragging:
            return
        if event.shift:
            self._plot.pan(event.delta_x * 4.0, event.delta_y * 4.0)
        else:
            self._plot.rotate(event.delta_x * 0.03, event.delta_y * 0.03)
        self.refresh()

    def on_mouse_scroll_down(self, event: events.MouseScrollDown) -> None:
        self._plot.zoom_by(0.9)
        self.refresh()

    def on_mouse_scroll_up(self, event: events.MouseScrollUp) -> None:
        self._plot.zoom_by(1.1)
        self.refresh()

    def on_key(self, event: events.Key) -> None:
        key = event.key
        if key in ("plus", "equals_sign", "="):
            self._plot.zoom_by(1.1)
        elif key in ("minus", "-"):
            self._plot.zoom_by(0.9)
        elif key == "left":
            self._plot.rotate(-0.1, 0.0)
        elif key == "right":
            self._plot.rotate(0.1, 0.0)
        elif key == "up":
            self._plot.rotate(0.0, -0.1)
        elif key == "down":
            self._plot.rotate(0.0, 0.1)
        elif key == "r":
            self._plot.reset()
        else:
            return
        event.stop()
        self.refresh()
