"""Textual integration for plotui.

``PlotWidget`` embeds an interactive plot inside a Textual app. Textual owns the
loop and input; the widget forwards mouse/key events to the plot's camera and
asks the Rust core for a fresh frame on each refresh.

Rendering picks the best path for the terminal (see `detect_render_mode`):

- **"placeholder"** (Kitty, Ghostty): full-resolution pixel images composited
  via Kitty's Unicode placeholders — flicker-free, and text overlays splice
  cleanly into the image.
- **"direct"** (iTerm2 ≥ 3.5, WezTerm, Konsole): these speak the Kitty
  graphics protocol but not Unicode placeholders, so the image is drawn
  directly at the widget's origin instead — still full resolution.

plotui only draws real pixels: terminals without Kitty graphics support get
a message naming supported terminals rather than a degraded plot.

Inside tmux, direct-mode image escapes are wrapped for tmux passthrough (see
`tmux_wrap`), so the picture reaches the outer terminal — e.g. a browser
xterm.js with the image addon. This needs ``set -g allow-passthrough on`` and
``PLOTUI_RENDER=direct`` (xterm.js's Kitty support is direct-placement only).

Set the ``PLOTUI_RENDER`` environment variable (or the widget's
``render_mode`` parameter) to override detection.
"""

from __future__ import annotations

import os
import sys

from rich.cells import cell_len
from rich.color import Color
from rich.segment import Segment
from rich.style import Style
from textual import events
from textual.message import Message
from textual.strip import Strip
from textual.widget import Widget

from ._plotui import Plot

# Fallback cell size in device pixels, used only when the terminal doesn't
# report its own (see `detect_cell_px`). The image is scaled to the cell grid
# by the terminal, so a too-small guess renders below native resolution and
# gets upscaled — soft edges. Detection avoids that.
_CELL_W, _CELL_H = 12, 24

# Above this node count, 3D plots drop to half resolution *while interacting*
# (dragging or auto-rotating) and snap back to full resolution when still.
_LARGE_NODE_COUNT = 400


def tmux_wrap(escape: str) -> str:
    """Wrap a terminal escape for tmux passthrough when running inside tmux.

    tmux intercepts control sequences it doesn't model (like the Kitty
    graphics APC), so an image drawn by direct placement never reaches the
    outer terminal. tmux's passthrough — ``\\ePtmux;<payload>\\e\\`` with every
    ESC in the payload doubled — hands the raw bytes to the outer terminal.
    Requires ``set -g allow-passthrough on`` in tmux. A no-op outside tmux
    (``$TMUX`` unset), so normal terminals are unaffected."""
    if not os.environ.get("TMUX"):
        return escape
    return "\x1bPtmux;" + escape.replace("\x1b", "\x1b\x1b") + "\x1b\\"


def detect_cell_px(fallback: tuple[int, int] = (_CELL_W, _CELL_H)) -> tuple[int, int]:
    """The terminal's pixel-per-cell size, queried via the TIOCGWINSZ ioctl
    (``ws_xpixel``/``ws_ypixel``). Kitty, Ghostty, iTerm2, and WezTerm all
    report it — and report *device* pixels, so this yields the true retina
    resolution. Returns `fallback` when the terminal reports no pixel size
    (or on platforms without termios, e.g. Windows)."""
    try:
        import fcntl
        import struct
        import termios
    except ImportError:
        return fallback
    for stream in (sys.__stdout__, sys.__stderr__, sys.__stdin__):
        try:
            fd = stream.fileno()
            packed = fcntl.ioctl(fd, termios.TIOCGWINSZ, b"\0" * 8)
        except (AttributeError, ValueError, OSError):
            continue
        rows, cols, xpix, ypix = struct.unpack("HHHH", packed)
        if rows and cols and xpix and ypix:
            return (max(1, xpix // cols), max(1, ypix // rows))
    return fallback

# An overlay span: (row, col, text, style) — text drawn over the plot in
# terminal cells (labels, badges). See `PlotWidget.set_overlay`.
OverlaySpan = tuple[int, int, str, Style | None]


RENDER_MODES = ("placeholder", "direct")

# What the widget shows instead of a degraded plot when the terminal has no
# Kitty graphics support (centered in the plot area).
_UNSUPPORTED_MESSAGE: tuple[tuple[str, Style | None], ...] = (
    ("Plotting requires a terminal that supports the Kitty graphics protocol.", Style(bold=True)),
    ("", None),
    ("Supported terminals include Kitty, Ghostty, iTerm2 (3.5+), and WezTerm.", None),
    ("If yours does support it, force a path with PLOTUI_RENDER=placeholder|direct.", Style(dim=True)),
)


def _version_at_least(version: str, minimum: tuple[int, int]) -> bool:
    try:
        parts = version.split(".")
        return (int(parts[0]), int(parts[1] if len(parts) > 1 else 0)) >= minimum
    except (ValueError, IndexError):
        return False


def detect_render_mode(env: dict[str, str] | None = None) -> str:
    """Pick the best render path for this terminal.

    - ``"placeholder"``: Kitty graphics via Unicode placeholders (`U=1`) —
      Kitty and Ghostty. Flicker-free and splices with text overlays.
    - ``"direct"``: Kitty graphics drawn at the widget origin — for terminals
      that speak the protocol but not placeholders: iTerm2 ≥ 3.5, WezTerm,
      Konsole. Still full resolution.
    - ``"unsupported"``: no Kitty graphics — the widget shows a message
      naming supported terminals instead of degrading the plot.

    ``PLOTUI_RENDER`` overrides detection with ``placeholder`` or ``direct``
    ("kitty" is accepted as an alias for "placeholder").
    """
    env = os.environ if env is None else env
    forced = env.get("PLOTUI_RENDER", "").strip().lower()
    if forced == "kitty":
        return "placeholder"
    if forced in RENDER_MODES:
        return forced

    # Placeholder tier: Kitty sets KITTY_WINDOW_ID / TERM=xterm-kitty;
    # Ghostty speaks the same protocol (placeholders included).
    if (
        env.get("KITTY_WINDOW_ID")
        or "kitty" in env.get("TERM", "")
        or "ghostty" in env.get("TERM", "")
        or env.get("TERM_PROGRAM", "").lower() == "ghostty"
        or env.get("GHOSTTY_RESOURCES_DIR")
    ):
        return "placeholder"

    # Direct tier: Kitty graphics without Unicode placeholders.
    term_program = env.get("TERM_PROGRAM", "")
    if term_program == "iTerm.app" or env.get("LC_TERMINAL") == "iTerm2":
        version = env.get("TERM_PROGRAM_VERSION") or env.get("LC_TERMINAL_VERSION") or ""
        if _version_at_least(version, (3, 5)):
            return "direct"
        return "unsupported"
    if term_program == "WezTerm" or env.get("WEZTERM_EXECUTABLE") or env.get("KONSOLE_VERSION"):
        return "direct"

    return "unsupported"


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

    class ElementHovered(Message):
        """Posted when the hovered element changes (`pickable=True` only).

        `element` is `("node", index)`, `("edge", index)`, or `None`.
        """

        def __init__(self, plot_widget: "PlotWidget", element: tuple[str, int] | None) -> None:
            super().__init__()
            self.plot_widget = plot_widget
            self.element = element

    class ElementPicked(Message):
        """Posted on click when `pickable=True`.

        `element` is `("node", index)`, `("edge", index)`, or `None` if empty
        space was clicked.
        """

        def __init__(self, plot_widget: "PlotWidget", element: tuple[str, int] | None) -> None:
            super().__init__()
            self.plot_widget = plot_widget
            self.element = element

    def __init__(
        self,
        plot: Plot,
        *,
        auto_rotate: bool = False,
        cell_px: tuple[int, int] | None = None,
        pickable: bool = False,
        render_mode: str = "auto",
        interactive_scale: float = 0.5,
        **kwargs,
    ):
        """``pickable=True`` turns on interactive picking: moving the mouse
        over a node or a graph edge lights it up white (it can be clicked),
        and clicking posts :class:`ElementPicked` with what was hit. Off by
        default so plots without click semantics pay no per-mouse-move cost.

        ``render_mode`` is ``"auto"`` (detect, honoring ``PLOTUI_RENDER``) or
        ``"placeholder"`` / ``"direct"`` to force a path — see
        :func:`detect_render_mode`.

        ``cell_px`` sets the device pixels per terminal cell (rendering
        resolution). Default ``None`` detects the terminal's true cell size so
        plots render at native resolution — see :func:`detect_cell_px`.

        ``interactive_scale`` is the resolution multiplier used for large 3D
        plots *while interacting* (dragging or auto-rotating); ``1.0`` disables
        it. Full resolution is restored the moment interaction stops.
        """
        super().__init__(**kwargs)
        self._plot = plot
        self._dragging = False
        self._moved = False
        self._last_pos = (0, 0)
        self._auto = auto_rotate
        self._cell_w, self._cell_h = cell_px if cell_px is not None else detect_cell_px()
        self._pickable = pickable
        self._interactive_scale = min(1.0, max(0.05, interactive_scale))
        self._large = plot.node_count() >= _LARGE_NODE_COUNT
        self._hovered: tuple[str, int] | None = None
        if render_mode != "auto":
            if render_mode not in (*RENDER_MODES, "unsupported"):
                raise ValueError(f"render_mode must be 'auto' or one of {RENDER_MODES}")
            self._mode = render_mode
        else:
            self._mode = detect_render_mode()
        # Frame cache, keyed on (w, h, version, mode) so we rasterize once per
        # change rather than once per rendered line.
        self._version = 0
        self._key = None
        self._transmit = ""
        self._cells: list[list[str]] | None = None
        self._style: Style | None = None
        # Text overlay, row -> non-overlapping spans sorted by column. Kept
        # outside the frame cache: changing it never re-rasterizes the image.
        self._overlay: dict[int, list[tuple[int, str, Style | None]]] = {}

    @property
    def dragging(self) -> bool:
        """True while the user is actively dragging (rotating/panning) — a
        hook for hosts that want to defer expensive work mid-gesture."""
        return self._dragging and self._moved

    def _active_scale(self) -> float:
        """Resolution multiplier for the next frame: reduced only for large 3D
        plots while interacting (an active drag, or continuous auto-rotate),
        else 1.0 — so a still plot is always at full resolution."""
        if self._interactive_scale >= 1.0 or not self._large or not self._plot.is_3d():
            return 1.0
        return self._interactive_scale if (self.dragging or self._auto) else 1.0

    def on_mount(self) -> None:
        if self._auto:
            self.set_interval(1 / 30, self._tick)

    def on_unmount(self) -> None:
        # Delete our image placements so nothing outlives the app: without
        # this, terminals that keep Kitty-graphics placements around (iTerm2
        # in particular) leave the last frame painted over the shell.
        if self._mode in ("placeholder", "direct"):
            driver = getattr(self.app, "_driver", None)
            if driver is not None:
                try:
                    driver.write(tmux_wrap(Plot.kitty_cleanup()))
                except Exception:
                    pass

    def _tick(self) -> None:
        self.apply_rotate(0.02, 0.0)

    # ---- overridable interaction primitives ----
    # Every built-in input path (mouse drag, scroll, keys) routes through
    # these, so a subclass can hook camera changes and clicks WITHOUT
    # overriding Textual event handlers — Textual dispatches on_* handlers to
    # every class in the MRO, so an override would run in addition to this
    # class's handler, not instead of it.

    def apply_rotate(self, d_yaw: float, d_pitch: float) -> None:
        self._plot.rotate(d_yaw, d_pitch)
        self.invalidate()

    def apply_pan(self, dx: float, dy: float) -> None:
        self._plot.pan(dx, dy)
        self.invalidate()

    def apply_zoom(self, factor: float) -> None:
        self._plot.zoom_by(factor)
        self.invalidate()

    def apply_reset(self) -> None:
        self._plot.reset()
        self.invalidate()

    def on_click_at(self, event: events.MouseUp) -> None:
        """Click semantics (a press-and-release without movement). The default
        picks and selects; subclasses override for their own click behavior."""
        if self._pickable:
            element = self._pick_at(event.x, event.y)
            self._plot.set_selected(element)
            self.invalidate()
            self.post_message(self.ElementPicked(self, element))
            # Keep the node-only message for handlers that predate edges.
            if element is None or element[0] == "node":
                self.post_message(self.NodePicked(self, element[1] if element else None))
            return
        px_w, px_h, px, py, radius = self._pixel_geometry(event.x, event.y)
        idx = self._plot.pick_px(px_w, px_h, px, py, radius)
        self._plot.set_selected(idx)
        self.invalidate()
        self.post_message(self.NodePicked(self, idx))

    def invalidate(self) -> None:
        """Mark the view dirty and repaint (call after mutating the plot)."""
        self._version += 1
        self.refresh()

    def set_overlay(self, spans: list[OverlaySpan]) -> None:
        """Draw text over the plot: each span is `(row, col, text, style)` in
        widget cells. Spans replace the image at the cells they cover (labels
        sit on the terminal background). Overlapping or off-widget spans are
        clipped/dropped. Repaints without re-rasterizing the image."""
        w, h = self.size.width, self.size.height
        overlay: dict[int, list[tuple[int, str, Style | None]]] = {}
        for row, col, text, style in sorted(spans, key=lambda s: (s[0], s[1])):
            if row < 0 or row >= h or col < 0 or not text:
                continue
            text = text[: max(0, w - col)]
            if not text:
                continue
            row_spans = overlay.setdefault(row, [])
            if row_spans:
                prev_col, prev_text, _ = row_spans[-1]
                if col < prev_col + cell_len(prev_text):
                    continue  # overlaps the previous span — first one wins
            row_spans.append((col, text, style))
        self._overlay = overlay
        self.refresh()

    # ---- rendering ----
    def _ensure_frame(self) -> None:
        if self._mode == "unsupported":
            return  # nothing to rasterize; render_line shows the notice
        w, h = self.size.width, self.size.height
        if w <= 0 or h <= 0:
            return
        scale = self._active_scale()
        key = (w, h, self._version, self._mode, scale)
        if key == self._key:
            return
        self._key = key
        if self._mode == "placeholder":
            transmit, id_rgb, cells = self._plot.render_kitty_placeholder_cells(
                w, h, self._cell_w, self._cell_h, scale=scale
            )
            self._transmit = transmit
            self._cells = cells
            self._style = Style(color=Color.from_rgb(*id_rgb))
        elif self._mode == "direct":
            # One escape draws the full-res image at the widget's origin,
            # scaled to span its cell region. The fixed image id makes each
            # frame replace the previous one atomically. compat_chunks: the
            # direct tier exists for terminals (iTerm2) that need the image
            # id repeated on every data chunk to assemble the transmission.
            # tmux_wrap passes the APC through tmux (a no-op outside tmux),
            # so the image reaches a browser terminal like xterm.js.
            self._transmit = tmux_wrap(
                self._plot.render_kitty(
                    w, h, self._cell_w, self._cell_h, compat_chunks=True, scale=scale
                )
            )

    def _kitty_row_segments(self, y: int, w: int) -> list[Segment]:
        """One row of placeholder cells with overlay spans spliced in. Every
        placeholder cell is self-addressed (it carries its own position
        diacritics), so cells after a text gap still map to the right part of
        the image."""
        cells = self._cells[y] if self._cells is not None and y < len(self._cells) else None
        if cells is None:
            return [Segment(" " * max(0, w))]
        spans = self._overlay.get(y)
        if not spans:
            return [Segment("".join(cells), self._style)]
        segments: list[Segment] = []
        cursor = 0
        for col, text, style in spans:
            if cursor < col:
                segments.append(Segment("".join(cells[cursor:col]), self._style))
            segments.append(Segment(text, style))
            cursor = min(col + cell_len(text), len(cells))
        if cursor < len(cells):
            segments.append(Segment("".join(cells[cursor:]), self._style))
        return segments

    def _spliced_strip(self, strip: Strip, y: int, w: int) -> Strip:
        """A row strip with overlay spans spliced in (direct-mode rows)."""
        spans = self._overlay.get(y)
        if not spans:
            return strip
        segments: list[Segment] = []
        cursor = 0
        for col, text, style in spans:
            if cursor < col:
                segments.extend(strip.crop(cursor, col))
            segments.append(Segment(text, style))
            cursor = col + cell_len(text)
        if cursor < w:
            segments.extend(strip.crop(cursor, w))
        return Strip(segments, w)

    def _unsupported_line(self, y: int, w: int) -> Strip:
        """One row of the centered "this terminal can't do pixels" notice."""
        h = self.size.height
        top = max(0, (h - len(_UNSUPPORTED_MESSAGE)) // 2)
        index = y - top
        if 0 <= index < len(_UNSUPPORTED_MESSAGE):
            text, style = _UNSUPPORTED_MESSAGE[index]
            text = text[: max(0, w)]
            pad_left = max(0, (w - len(text)) // 2)
            pad_right = max(0, w - pad_left - len(text))
            return Strip(
                [Segment(" " * pad_left), Segment(text, style), Segment(" " * pad_right)], w
            )
        return Strip([Segment(" " * max(0, w))], w)

    def render_line(self, y: int) -> Strip:
        self._ensure_frame()
        w = self.size.width
        if self._mode == "unsupported":
            return self._unsupported_line(y, w)
        if self._mode == "placeholder":
            segments = []
            if y == 0 and self._transmit:
                # Zero-width control segment carries the image upload. Reusing a
                # fixed image id makes the terminal replace the frame atomically.
                segments.append(Segment(self._transmit, None, [(0,)]))
            segments.extend(self._kitty_row_segments(y, w))
            return Strip(segments, w)
        if self._mode == "direct":
            segments = []
            if y == 0 and self._transmit:
                # The cursor sits at the widget's top-left when line 0 is
                # written, which is exactly where the image escape draws
                # (it saves/restores the cursor itself).
                segments.append(Segment(self._transmit, None, [(0,)]))
            # The cells under the image stay blank; overlays are still
            # spliced, though most terminals draw the image above them —
            # prefer the placeholder path for text-over-plot.
            return Strip(
                [*segments, *self._spliced_strip(Strip([Segment(" " * max(0, w))], w), y, w)], w
            )
        return Strip([Segment(" " * max(0, w))], w)

    # ---- interaction ----
    def _pixel_geometry(self, x: int, y: int) -> tuple[int, int, float, float, float]:
        """Map a cell coordinate into the framebuffer's pixel space:
        `(px_w, px_h, px, py, node_radius)`."""
        w, h = self.size.width, self.size.height
        return (
            w * self._cell_w,
            h * self._cell_h,
            x * self._cell_w + self._cell_w / 2,
            y * self._cell_h + self._cell_h / 2,
            float(self._cell_h),
        )

    def _pick_at(self, x: int, y: int) -> tuple[str, int] | None:
        px_w, px_h, px, py, radius = self._pixel_geometry(x, y)
        return self._plot.pick_element_px(px_w, px_h, px, py, radius)

    def _set_hover(self, element: tuple[str, int] | None) -> None:
        if element == self._hovered:
            return
        self._hovered = element
        if self._plot.set_hovered(element):
            self.invalidate()
        self.post_message(self.ElementHovered(self, element))

    def on_mouse_down(self, event: events.MouseDown) -> None:
        if self._mode == "unsupported":
            return
        self._dragging = True
        self._moved = False
        self._last_pos = (event.screen_x, event.screen_y)
        self.capture_mouse()
        self.focus()

    def on_mouse_move(self, event: events.MouseMove) -> None:
        if self._mode == "unsupported":
            return
        if self._dragging:
            # Deltas computed from screen coordinates, not event.delta_*:
            # those are unreliable under mouse capture and in test pilots.
            dx = event.screen_x - self._last_pos[0]
            dy = event.screen_y - self._last_pos[1]
            self._last_pos = (event.screen_x, event.screen_y)
            if dx or dy:
                self._moved = True
            if event.shift:
                self.apply_pan(dx * 4.0, dy * 4.0)
            else:
                self.apply_rotate(dx * 0.03, dy * 0.03)
        elif self._pickable:
            self._set_hover(self._pick_at(event.x, event.y))

    def on_leave(self, event: events.Leave) -> None:
        if self._pickable:
            self._set_hover(None)

    def on_mouse_up(self, event: events.MouseUp) -> None:
        was_click = self._dragging and not self._moved
        was_drag = self._dragging and self._moved
        self._dragging = False
        self.release_mouse()
        if was_click:
            self.on_click_at(event)
        elif was_drag:
            # The gesture ended: repaint so a half-res interaction frame is
            # replaced by a crisp full-res one.
            self.invalidate()

    def on_mouse_scroll_down(self, event: events.MouseScrollDown) -> None:
        if self._mode == "unsupported":
            return
        self.apply_zoom(0.9)

    def on_mouse_scroll_up(self, event: events.MouseScrollUp) -> None:
        if self._mode == "unsupported":
            return
        self.apply_zoom(1.1)

    def on_key(self, event: events.Key) -> None:
        if self._mode == "unsupported":
            return
        key = event.key
        if key in ("plus", "equals_sign", "="):
            self.apply_zoom(1.1)
        elif key in ("minus", "-"):
            self.apply_zoom(0.9)
        elif key == "left":
            self.apply_rotate(-0.1, 0.0)
        elif key == "right":
            self.apply_rotate(0.1, 0.0)
        elif key == "up":
            self.apply_rotate(0.0, -0.1)
        elif key == "down":
            self.apply_rotate(0.0, 0.1)
        elif key == "shift+left":
            self.apply_pan(-16.0, 0.0)
        elif key == "shift+right":
            self.apply_pan(16.0, 0.0)
        elif key == "shift+up":
            self.apply_pan(0.0, -16.0)
        elif key == "shift+down":
            self.apply_pan(0.0, 16.0)
        elif key == "r":
            self.apply_reset()
        else:
            return
        event.stop()
