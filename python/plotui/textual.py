"""Textual integration for plotui.

``PlotWidget`` embeds an interactive plot inside a Textual app. Textual owns the
loop and input; the widget forwards mouse/key events to the plot's camera and
asks the Rust core for a fresh frame on each refresh.

Rendering picks the best path for the terminal:

- **Kitty graphics protocol** (Kitty, Ghostty, WezTerm): full-resolution pixel
  images, composited into the widget via Kitty's Unicode placeholders (a fixed
  image id, so frames replace atomically — no flicker).
- **Half-block fallback**: colored `▀` cells, for any other truecolor terminal.
"""

from __future__ import annotations

import os

from rich.color import Color
from rich.segment import Segment
from rich.style import Style
from rich.text import Text
from textual import events
from textual.message import Message
from textual.strip import Strip
from textual.widget import Widget

from ._plotui import Plot

# Approximate terminal cell size in pixels for the Kitty image. The terminal
# scales the source image to the cell grid, so this only sets supersampling —
# generous values keep it crisp.
_CELL_W, _CELL_H = 12, 24


def _terminal_is_kitty() -> bool:
    return bool(os.environ.get("KITTY_WINDOW_ID")) or "kitty" in os.environ.get("TERM", "")


class PlotWidget(Widget, can_focus=True):
    """A Textual widget hosting an interactive plotui plot."""

    # A plot isn't text — dragging over it rotates, it doesn't select text.
    ALLOW_SELECT = False

    DEFAULT_CSS = """
    PlotWidget { width: 1fr; height: 1fr; }
    """

    class NodePicked(Message):
        """Posted when the user clicks (without dragging).

        `index` is the flat node index, or `None` if empty space was clicked.
        """

        def __init__(self, plot_widget: "PlotWidget", index: int | None) -> None:
            super().__init__()
            self.plot_widget = plot_widget
            self.index = index

    def __init__(
        self,
        plot: Plot,
        *,
        auto_rotate: bool = False,
        cell_px: tuple[int, int] = (_CELL_W, _CELL_H),
        force_halfblock: bool = False,
        **kwargs,
    ):
        super().__init__(**kwargs)
        self._plot = plot
        self._dragging = False
        self._moved = False
        self._auto = auto_rotate
        self._cell_w, self._cell_h = cell_px
        self._kitty = _terminal_is_kitty() and not force_halfblock
        # Frame cache, keyed on (w, h, version, mode) so we rasterize once per
        # change rather than once per rendered line.
        self._version = 0
        self._key = None
        self._transmit = ""
        self._rows: list[str] | None = None
        self._style: Style | None = None
        self._strips: list[Strip] | None = None

    def on_mount(self) -> None:
        if self._auto:
            self.set_interval(1 / 30, self._tick)

    def _tick(self) -> None:
        self._plot.rotate(0.02, 0.0)
        self.invalidate()

    def invalidate(self) -> None:
        """Mark the view dirty and repaint (call after mutating the plot)."""
        self._version += 1
        self.refresh()

    # ---- rendering ----
    def _ensure_frame(self) -> None:
        w, h = self.size.width, self.size.height
        if w <= 0 or h <= 0:
            return
        key = (w, h, self._version, self._kitty)
        if key == self._key:
            return
        self._key = key
        if self._kitty:
            transmit, id_rgb, rows = self._plot.render_kitty_placeholder(
                w, h, self._cell_w, self._cell_h
            )
            self._transmit = transmit
            self._rows = rows
            self._style = Style(color=Color.from_rgb(*id_rgb))
        else:
            ansi = self._plot.render_halfblock(w, h)
            console = self.app.console
            self._strips = [
                Strip(list(Text.from_ansi(line).render(console, end="")))
                for line in ansi.split("\n")
            ]

    def render_line(self, y: int) -> Strip:
        self._ensure_frame()
        w = self.size.width
        if self._kitty:
            segments = []
            if y == 0 and self._transmit:
                # Zero-width control segment carries the image upload. Reusing a
                # fixed image id makes the terminal replace the frame atomically.
                segments.append(Segment(self._transmit, None, [(0,)]))
            if self._rows is not None and y < len(self._rows):
                segments.append(Segment(self._rows[y], self._style))
            else:
                segments.append(Segment(" " * max(0, w)))
            return Strip(segments, w)
        if self._strips is not None and y < len(self._strips):
            return self._strips[y]
        return Strip([Segment(" " * max(0, w))], w)

    # ---- interaction ----
    def on_mouse_down(self, event: events.MouseDown) -> None:
        self._dragging = True
        self._moved = False
        self.capture_mouse()
        self.focus()

    def on_mouse_move(self, event: events.MouseMove) -> None:
        if not self._dragging:
            return
        if event.delta_x or event.delta_y:
            self._moved = True
        if event.shift:
            self._plot.pan(event.delta_x * 4.0, event.delta_y * 4.0)
        else:
            self._plot.rotate(event.delta_x * 0.03, event.delta_y * 0.03)
        self.invalidate()

    def on_mouse_up(self, event: events.MouseUp) -> None:
        was_click = self._dragging and not self._moved
        self._dragging = False
        self.release_mouse()
        if not was_click:
            return
        # Map the clicked cell into the framebuffer's pixel space for the active
        # render mode, then pick the nearest node.
        w, h = self.size.width, self.size.height
        if self._kitty:
            px_w, px_h = w * self._cell_w, h * self._cell_h
            px = event.x * self._cell_w + self._cell_w / 2
            py = event.y * self._cell_h + self._cell_h / 2
            radius = float(self._cell_h)
        else:
            px_w, px_h = w, h * 2
            px = event.x + 0.5
            py = event.y * 2 + 1
            radius = 5.0
        idx = self._plot.pick_px(px_w, px_h, float(px), float(py), radius)
        self._plot.set_selected(idx)
        self.invalidate()
        self.post_message(self.NodePicked(self, idx))

    def on_mouse_scroll_down(self, event: events.MouseScrollDown) -> None:
        self._plot.zoom_by(0.9)
        self.invalidate()

    def on_mouse_scroll_up(self, event: events.MouseScrollUp) -> None:
        self._plot.zoom_by(1.1)
        self.invalidate()

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
        self.invalidate()
