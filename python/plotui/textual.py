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

import math
import os

from rich.cells import cell_len
from rich.color import Color
from rich.segment import Segment
from rich.style import Style
from textual import events
from textual.message import Message
from textual.strip import Strip
from textual.widget import Widget

from ._plotui import Plot
from ._plotui import detect_cell_px as _detect_cell_px
from ._plotui import detect_render_mode as _detect_render_mode
from ._plotui import tmux_wrap as _tmux_wrap

# Fallback cell size in device pixels, used only when the terminal doesn't
# report its own (see `detect_cell_px`). The image is scaled to the cell grid
# by the terminal, so a too-small guess renders below native resolution and
# gets upscaled — soft edges. Detection avoids that.
_CELL_W, _CELL_H = 12, 24

# Above this node count, 3D plots drop to half resolution *while interacting*
# (dragging or auto-rotating) and snap back to full resolution when still.
_LARGE_NODE_COUNT = 400

# Radians of yaw per auto-rotate tick, matching plotui_term's constant for
# the Rust terminal frontends. `Plot.spin` owns the *direction*; this is
# only how fast.
AUTO_ROTATE_STEP = 0.02


def tmux_wrap(escape: str) -> str:
    """Wrap a terminal escape for tmux passthrough when running inside tmux.

    tmux intercepts control sequences it doesn't model (like the Kitty
    graphics APC), so an image drawn by direct placement never reaches the
    outer terminal. tmux's passthrough — ``\\ePtmux;<payload>\\e\\`` with every
    ESC in the payload doubled — hands the raw bytes to the outer terminal.
    Requires ``set -g allow-passthrough on`` in tmux. A no-op outside tmux
    (``$TMUX`` unset), so normal terminals are unaffected."""
    return _tmux_wrap(escape)


def detect_cell_px(fallback: tuple[int, int] = (_CELL_W, _CELL_H)) -> tuple[int, int]:
    """The terminal's pixel-per-cell size, queried via the TIOCGWINSZ ioctl
    (``ws_xpixel``/``ws_ypixel``). Kitty, Ghostty, iTerm2, and WezTerm all
    report it — and report *device* pixels, so this yields the true retina
    resolution. Returns `fallback` when the terminal reports no pixel size
    (or on platforms without termios, e.g. Windows)."""
    return _detect_cell_px(fallback)

# An overlay span: (row, col, text, style) — text drawn over the plot in
# terminal cells (labels, badges). See `PlotWidget.set_overlay`.
OverlaySpan = tuple[int, int, str, Style | None]


RENDER_MODES = ("placeholder", "direct")

# What the widget shows instead of a degraded plot when the terminal has no
# Kitty graphics support (centered in the plot area). Change these strings
# only in lockstep with UNSUPPORTED_MESSAGE in crates/plotui-term/src/policy.rs
# — the Rust frontends center the same notice.
_UNSUPPORTED_MESSAGE: tuple[tuple[str, Style | None], ...] = (
    ("Plotting requires a terminal that supports the Kitty graphics protocol.", Style(bold=True)),
    ("", None),
    ("Supported terminals include Kitty, Ghostty, iTerm2 (3.5+), WezTerm, and Konsole.", None),
    ("If yours does support it, force a path with PLOTUI_RENDER=placeholder|direct.", Style(dim=True)),
)


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
    return _detect_render_mode(None if env is None else dict(env))


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

    class RangeChanged(Message):
        """Posted when the x window changes through a finished gesture (a
        released range-slider drag, a scroll zoom, or an ``[``/``]`` key).

        `window` is the new ``(lo, hi)`` in data coordinates, or `None` for
        the full extent.
        """

        def __init__(
            self, plot_widget: "PlotWidget", window: tuple[float, float] | None
        ) -> None:
            super().__init__()
            self.plot_widget = plot_widget
            self.window = window

    def __init__(
        self,
        plot: Plot,
        *,
        auto_rotate: bool = False,
        cell_px: tuple[int, int] | None = None,
        pickable: bool = False,
        crosshair: bool = True,
        range_slider: bool = False,
        render_mode: str = "auto",
        interactive_scale: float = 0.5,
        **kwargs,
    ):
        """``pickable=True`` turns on interactive picking: moving the mouse
        over a node or a graph edge lights it up white (it can be clicked),
        and clicking posts :class:`ElementPicked` with what was hit. Off by
        default so plots without click semantics pay no per-mouse-move cost.

        ``crosshair`` (default on) gives 2D plots a hover crosshair: a
        vertical guide snapped to the nearest sample x, a marker per series,
        and a value readout. 2D renders are cheap enough to repaint per
        mouse-move. 3D plots are unaffected.

        ``range_slider`` gives 2D plots a Plotly-style range slider: a
        full-extent overview strip under the plot whose window (drag its
        handles or body, click the track, scroll over the plot, ``[``/``]``)
        sets the plot's x view; gestures post :class:`RangeChanged`. It also
        works when the wrapped plot already has ``range_slider`` enabled.

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
        if range_slider:
            plot.set_range_slider(True)
        self._dragging = False
        self._moved = False
        self._last_pos = (0, 0)
        # The strip part grabbed by the active drag ("left"/"right"/"window"),
        # if the drag started on the range slider.
        self._range_drag: str | None = None
        self._auto = auto_rotate
        self._cell_w, self._cell_h = cell_px if cell_px is not None else detect_cell_px()
        self._pickable = pickable
        self._crosshair = crosshair
        self._interactive_scale = min(1.0, max(0.05, interactive_scale))
        # Vertex count, not node count: line vertices and surface grids load
        # the rasterizer just as much as pickable nodes do.
        self._large = plot.vertex_count() >= _LARGE_NODE_COUNT
        # Direct mode deletes the prior image before each frame to stop iTerm2
        # stacking placements. Terminals whose Kitty decoder replaces a same-id
        # image (xterm.js addon-image) flicker from that delete; PLOTUI_KITTY_
        # REPLACE=1 skips it. See render_kitty(replace=...).
        self._kitty_replace = os.environ.get("PLOTUI_KITTY_REPLACE", "").strip() in ("1", "true")
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
    def plot(self) -> Plot:
        """The wrapped :class:`Plot`. Mutate it freely — camera calls, or
        ``extend``/``set_visible`` by trace handle — then call
        :meth:`invalidate` (or use the widget-level :meth:`extend` /
        :meth:`set_visible`, which do both)."""
        return self._plot

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
        # Not routed through apply_rotate: that hook is for input paths
        # (drag, scroll, keys), and an idle spin is not one. `spin` also
        # owns the direction — it turns the way a rightward drag pushes the
        # object, so letting go of a grabbed plot does not send it back the
        # way it came.
        self._plot.spin(AUTO_ROTATE_STEP)
        self.invalidate()

    # ---- overridable interaction primitives ----
    # Every built-in input path (mouse drag, scroll, keys) routes through
    # these, so a subclass can hook camera changes and clicks WITHOUT
    # overriding Textual event handlers — Textual dispatches on_* handlers to
    # every class in the MRO, so an override would run in addition to this
    # class's handler, not instead of it.

    def _apply_mapped_drag(
        self,
        dx: float,
        dy: float,
        shift: bool,
        rotate: float,
        pan_x: float,
        pan_y: float,
        zoom: float,
    ) -> None:
        """One drag gesture, decomposed through the plot's input map into the
        camera moves it maps to — and issued as `apply_*` calls.

        The breakdown happens here rather than in `plot.apply_drag` so a drag
        still lands on the hooks below, which a subclass may have overridden.
        Moving the camera in the core directly would go behind that
        subclass's back: a view that locks rotation (a flat tree that pans
        instead of tilting) would tilt anyway, and a host that repaints an
        overlay on camera changes would keep drawing a stale one. Scales and
        signs mirror `Plot::apply_drag` exactly; at most one call per camera
        kind, so a diagonal drag is a single rotate.
        """
        controls = self._plot.input_map()
        d_yaw = d_pitch = pan_dx = pan_dy = 0.0
        factor = 1.0
        for control, d in zip(controls[2:] if shift else controls[:2], (dx, dy)):
            if control.startswith("-"):
                control, d = control[1:], -d
            if control == "yaw":
                d_yaw -= d * rotate
            elif control == "pitch":
                d_pitch -= d * rotate
            elif control == "pan_x":
                pan_dx += d * pan_x
            elif control == "pan_y":
                pan_dy += d * pan_y
            elif control == "zoom":
                factor *= math.exp(-d * zoom)
        if d_yaw or d_pitch:
            self.apply_rotate(d_yaw, d_pitch)
        if pan_dx or pan_dy:
            self.apply_pan(pan_dx, pan_dy)
        if factor != 1.0:
            self.apply_zoom(factor)

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
        if self._plot.set_x_window(None):
            self.post_message(self.RangeChanged(self, None))
        self.invalidate()

    def apply_x_window(self, window: tuple[float, float] | None) -> None:
        """Set (or clear) the 2D x view programmatically; repaints on change.
        Interactive changes post :class:`RangeChanged` instead."""
        if self._plot.set_x_window(window):
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
        # Streamed data can grow a plot past the reduced-resolution threshold
        # long after mount; vertex_count is O(traces), so re-checking on every
        # invalidation is safe even mid-drag.
        self._large = self._plot.vertex_count() >= _LARGE_NODE_COUNT
        self._version += 1
        self.refresh()

    def extend(self, handle: int, xs, ys, zs=None) -> None:
        """Append points to a trace by handle (see :meth:`Plot.extend`) and
        repaint. Multiple extends between frames coalesce into one repaint."""
        if zs is None:
            self._plot.extend(handle, xs, ys)
        else:
            self._plot.extend(handle, xs, ys, zs)
        self.invalidate()

    def set_visible(self, handle: int, visible: bool) -> bool:
        """Show or hide a trace by handle; repaints only when the state
        actually changed. Returns True when it did."""
        changed = self._plot.set_visible(handle, visible)
        if changed:
            self.invalidate()
        return changed

    def set_graph_positions(self, handle: int, xs, ys, zs) -> None:
        """Move every node of a graph trace at once and repaint — the
        per-frame call of a force-directed layout (see
        :meth:`Plot.set_graph_positions`, pair with ``ForceLayout``)."""
        self._plot.set_graph_positions(handle, xs, ys, zs)
        self.invalidate()

    def set_graph_routes(self, handle: int, routes) -> None:
        """Replace a 2D graph's edge waypoints and repaint — the second half
        of a relayout, after :meth:`set_graph_positions` has moved the nodes
        (see :meth:`Plot.set_graph_routes`, pair with ``LayeredLayout``)."""
        self._plot.set_graph_routes(handle, [list(r) for r in routes])
        self.invalidate()

    def set_graph_colors(self, handle: int, node_colors, edge_colors=None) -> None:
        """Recolor a graph trace in place and repaint — dim everything,
        brighten a hovered dependency path, restore (see
        :meth:`Plot.set_graph_colors`)."""
        self._plot.set_graph_colors(handle, node_colors, edge_colors)
        self.invalidate()

    def extend_graph(
        self, handle: int, xs, ys, zs, node_colors=None, edges=(), labels=None
    ) -> None:
        """Append nodes and edges to a graph trace and repaint (see
        :meth:`Plot.extend_graph`, pair with ``ForceLayout.add_node``).
        ``labels`` names the new boxes of a 2D graph; a 3D one ignores it."""
        self._plot.extend_graph(
            handle,
            xs,
            ys,
            zs,
            node_colors=node_colors,
            edges=list(edges),
            labels=labels,
        )
        self.invalidate()

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
                    w, h, self._cell_w, self._cell_h, compat_chunks=True, scale=scale,
                    replace=self._kitty_replace
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
        # A press on the range-slider strip grabs it instead of the camera; a
        # track press jumps the window there and then drags it as the body.
        if not self._plot.is_3d() and self._plot.range_slider():
            px_w, px_h, px, py, _ = self._pixel_geometry(event.x, event.y)
            hit = self._plot.range_slider_hit(px_w, px_h, px, py, float(self._cell_w))
            if hit is not None:
                if hit == "track":
                    if self._plot.jump_x_window(px_w, px_h, px):
                        self.invalidate()
                    hit = "window"
                self._range_drag = hit
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
            if self._range_drag is not None:
                px_w, px_h, *_ = self._pixel_geometry(event.x, event.y)
                if self._plot.drag_x_window(
                    px_w, px_h, self._range_drag, dx * self._cell_w
                ):
                    self.invalidate()
            elif (
                not event.shift
                and not self._plot.is_3d()
                and self._plot.x_window() is not None
            ):
                # With a window set, a plain plot-area drag slides the window
                # (the camera is superseded).
                px_w, px_h, *_ = self._pixel_geometry(event.x, event.y)
                if self._plot.pan_x_window(px_w, px_h, dx * self._cell_w):
                    self.invalidate()
            else:
                # Routed through the plot's input map: drag rotates
                # (trackball — drag right turns the object right),
                # shift-drag pans, unless remapped via plot.set_input_map.
                # Pan is in full-resolution image pixels, so one dragged
                # cell is one cell's worth of pixels and the plot stays
                # under the pointer.
                self._apply_mapped_drag(
                    dx, dy, event.shift, 0.03, self._cell_w, self._cell_h, 0.15
                )
        elif not self._plot.is_3d():
            if self._crosshair:
                px_w, px_h, px, py, _ = self._pixel_geometry(event.x, event.y)
                if self._plot.set_hover2d(px):
                    self.invalidate()
        elif self._pickable:
            self._set_hover(self._pick_at(event.x, event.y))

    def on_leave(self, event: events.Leave) -> None:
        if self._pickable:
            self._set_hover(None)
        if self._crosshair and self._plot.set_hover2d(None):
            self.invalidate()

    def on_mouse_up(self, event: events.MouseUp) -> None:
        was_click = self._dragging and not self._moved
        was_drag = self._dragging and self._moved
        was_range = self._dragging and self._range_drag is not None
        self._dragging = False
        self._range_drag = None
        self.release_mouse()
        if was_range:
            # The strip gesture ended: one message with the result.
            self.invalidate()
            self.post_message(self.RangeChanged(self, self._plot.x_window()))
        elif was_click:
            self.on_click_at(event)
        elif was_drag:
            # The gesture ended: repaint so a half-res interaction frame is
            # replaced by a crisp full-res one.
            self.invalidate()

    def _scroll(self, event, factor: float) -> None:
        # With an x window set on a 2D plot the wheel zooms the window about
        # the cursor; otherwise it zooms the camera.
        if not self._plot.is_3d() and self._plot.x_window() is not None:
            px_w, px_h, px, *_ = self._pixel_geometry(event.x, event.y)
            if self._plot.zoom_x_window(px_w, px_h, px, factor):
                self.invalidate()
                self.post_message(self.RangeChanged(self, self._plot.x_window()))
            return
        self.apply_zoom(factor)

    def on_mouse_scroll_down(self, event: events.MouseScrollDown) -> None:
        if self._mode == "unsupported":
            return
        self._scroll(event, 0.9)

    def on_mouse_scroll_up(self, event: events.MouseScrollUp) -> None:
        if self._mode == "unsupported":
            return
        self._scroll(event, 1.1)

    def on_key(self, event: events.Key) -> None:
        if self._mode == "unsupported":
            return
        key = event.key
        if key in ("plus", "equals_sign", "="):
            self.apply_zoom(1.1)
        elif key in ("minus", "-"):
            self.apply_zoom(0.9)
        elif key == "left":
            self.apply_rotate(0.1, 0.0)
        elif key == "right":
            self.apply_rotate(-0.1, 0.0)
        elif key == "up":
            self.apply_rotate(0.0, 0.1)
        elif key == "down":
            self.apply_rotate(0.0, -0.1)
        elif key == "shift+left":
            self.apply_pan(-2.0 * self._cell_w, 0.0)
        elif key == "shift+right":
            self.apply_pan(2.0 * self._cell_w, 0.0)
        elif key == "shift+up":
            self.apply_pan(0.0, -2.0 * self._cell_h)
        elif key == "shift+down":
            self.apply_pan(0.0, 2.0 * self._cell_h)
        elif key in ("left_square_bracket", "right_square_bracket", "[", "]"):
            if self._plot.x_window() is None:
                return
            frac = -0.1 if key in ("left_square_bracket", "[") else 0.1
            if self._plot.shift_x_window(frac):
                self.invalidate()
                self.post_message(self.RangeChanged(self, self._plot.x_window()))
        elif key == "r":
            self.apply_reset()
        else:
            return
        event.stop()
