"""Headless Textual tests for PlotWidget's opt-in hover/click picking.

Runs the real widget in Textual's test harness (placeholder mode — the
escapes are inspected as strings, no real terminal needed) and drives it
with a mouse pilot. Pixel checks go through Plot.render_rgba.
"""

from __future__ import annotations

import asyncio

import pytest

from textual.app import App, ComposeResult

from plotui import Plot
from plotui.textual import PlotWidget


def _has_white(plot: Plot, w: int = 200, h: int = 150) -> bool:
    data = plot.render_rgba(w, h)
    white = b"\xff\xff\xff\xff"
    return any(data[i : i + 4] == white for i in range(0, len(data), 4))


class _Harness(App):
    """Minimal app: one pickable plot, recording the widget's messages."""

    def __init__(self) -> None:
        super().__init__()
        self.plot = Plot()
        self.plot.add_graph3d(
            [0.0, 5.0, -5.0, 5.0],
            [0.0, 5.0, -5.0, -5.0],
            [0.0, 5.0, -5.0, 0.0],
            edges=[(0, 1), (1, 2), (0, 3)],
        )
        self.hovers: list[tuple[str, int] | None] = []
        self.picks: list[tuple[str, int] | None] = []

    def compose(self) -> ComposeResult:
        # Pin the mode: detection reads the developer's real terminal env.
        yield PlotWidget(self.plot, id="plot", pickable=True, render_mode="placeholder")

    def on_plot_widget_element_hovered(self, msg: PlotWidget.ElementHovered) -> None:
        self.hovers.append(msg.element)

    def on_plot_widget_element_picked(self, msg: PlotWidget.ElementPicked) -> None:
        self.picks.append(msg.element)


def _cell_over(plot: Plot, widget: PlotWidget, kind: str) -> tuple[int, int]:
    """Find a cell whose center sits over an element of `kind`, using the
    widget's own cell-to-pixel mapping."""
    w, h = widget.size.width, widget.size.height
    for y in range(h):
        for x in range(w):
            px_w, px_h, px, py, radius = widget._pixel_geometry(x, y)
            el = plot.pick_element_px(px_w, px_h, px, py, radius)
            if el is not None and el[0] == kind:
                return x, y
    raise AssertionError(f"no {kind} visible in a {w}x{h} widget")


async def _drive() -> None:
    app = _Harness()
    async with app.run_test(size=(80, 24)) as pilot:
        widget = app.query_one("#plot", PlotWidget)

        # Hover a node: message posted, plot lights up white.
        nx, ny = _cell_over(app.plot, widget, "node")
        await pilot.hover("#plot", offset=(nx, ny))
        assert app.hovers and app.hovers[-1][0] == "node"
        assert _has_white(app.plot), "hovered node lights up white"

        # Click it: ElementPicked carries the same node, selection persists.
        await pilot.click("#plot", offset=(nx, ny))
        assert app.picks and app.picks[-1] == app.hovers[-1]

        # Hover an edge.
        ex, ey = _cell_over(app.plot, widget, "edge")
        await pilot.hover("#plot", offset=(ex, ey))
        assert app.hovers[-1][0] == "edge"

        # Click empty space: pick message with None.
        await pilot.click("#plot", offset=(0, 0))
        assert app.picks[-1] is None


def test_hover_and_click_pipeline() -> None:
    asyncio.run(_drive())


def test_overlay_splices_text_into_kitty_placeholder_cells() -> None:
    async def drive() -> None:
        class Overlaid(App):
            def __init__(self) -> None:
                super().__init__()
                self.plot = Plot()
                self.plot.add_graph3d([0.0, 5.0], [0.0, 5.0], [0.0, 5.0], edges=[(0, 1)])

            def compose(self) -> ComposeResult:
                yield PlotWidget(self.plot, id="plot", render_mode="placeholder")

        app = Overlaid()
        async with app.run_test(size=(60, 20)) as pilot:
            widget = app.query_one("#plot", PlotWidget)
            widget.set_overlay([(2, 5, "label", None)])
            await pilot.pause()
            segments = list(widget.render_line(2))
            texts = [seg.text for seg in segments]
            assert "label" in texts
            # Cells on either side of the splice: 5 before, 60 - 10 after,
            # each placeholder cell being 3 chars (glyph + 2 diacritics).
            joined = "".join(t for t in texts if t != "label")
            assert joined.count("\U0010eeee") == 60 - len("label")
            # A row without overlay is one contiguous run of placeholders.
            plain = list(widget.render_line(3))
            assert sum(seg.text.count("\U0010eeee") for seg in plain) == 60

    asyncio.run(drive())


def test_dragging_property_reflects_gesture() -> None:
    async def drive() -> None:
        app = _Harness()
        async with app.run_test(size=(60, 20)) as pilot:
            widget = app.query_one("#plot", PlotWidget)
            assert widget.dragging is False
            await pilot.mouse_down("#plot", offset=(30, 10))
            assert widget.dragging is False, "a press without movement is not a drag"
            await pilot.hover("#plot", offset=(34, 12))
            assert widget.dragging is True
            await pilot.mouse_up("#plot", offset=(34, 12))
            assert widget.dragging is False

    asyncio.run(drive())


def test_not_pickable_stays_silent() -> None:
    async def drive() -> None:
        class Quiet(App):
            def __init__(self) -> None:
                super().__init__()
                self.plot = Plot()
                self.plot.add_graph3d([0.0, 5.0], [0.0, 5.0], [0.0, 5.0], edges=[(0, 1)])
                self.hovers: list = []

            def compose(self) -> ComposeResult:
                # pickable defaults off; mode pinned for env-independence
                yield PlotWidget(self.plot, id="plot", render_mode="placeholder")

            def on_plot_widget_element_hovered(self, msg) -> None:
                self.hovers.append(msg.element)

        app = Quiet()
        async with app.run_test(size=(60, 20)) as pilot:
            await pilot.hover("#plot", offset=(30, 10))
            assert app.hovers == [], "no hover messages when pickable is off"
            assert not _has_white(app.plot)

    asyncio.run(drive())


def test_render_mode_detection() -> None:
    from plotui.textual import detect_render_mode

    kitty = {"KITTY_WINDOW_ID": "1", "TERM": "xterm-kitty"}
    ghostty = {"TERM": "xterm-ghostty", "TERM_PROGRAM": "ghostty"}
    iterm_new = {"TERM_PROGRAM": "iTerm.app", "TERM_PROGRAM_VERSION": "3.6.11"}
    iterm_lc = {"LC_TERMINAL": "iTerm2", "LC_TERMINAL_VERSION": "3.5.0"}
    iterm_old = {"TERM_PROGRAM": "iTerm.app", "TERM_PROGRAM_VERSION": "3.4.19"}
    wezterm = {"TERM_PROGRAM": "WezTerm"}
    plain = {"TERM": "xterm-256color"}

    assert detect_render_mode(kitty) == "placeholder"
    assert detect_render_mode(ghostty) == "placeholder"
    assert detect_render_mode(iterm_new) == "direct", "iTerm2 >= 3.5 speaks Kitty graphics"
    assert detect_render_mode(iterm_lc) == "direct"
    # No degradation: unknown/old terminals get the notice — plotui only
    # draws real pixels.
    assert detect_render_mode(iterm_old) == "unsupported", "old iTerm2 lacks the protocol"
    assert detect_render_mode(plain) == "unsupported"
    assert detect_render_mode(wezterm) == "direct"
    # Explicit override beats every terminal signal; removed/unknown values
    # (like the retired "halfblock") are ignored and detection proceeds.
    assert detect_render_mode({**plain, "PLOTUI_RENDER": "direct"}) == "direct"
    assert detect_render_mode({**plain, "PLOTUI_RENDER": "kitty"}) == "placeholder"
    assert detect_render_mode({**kitty, "PLOTUI_RENDER": "halfblock"}) == "placeholder"
    assert detect_render_mode({**plain, "PLOTUI_RENDER": "halfblock"}) == "unsupported"


def test_unsupported_terminal_shows_a_notice_and_stays_inert() -> None:
    async def drive() -> None:
        class Unsupported(App):
            def __init__(self) -> None:
                super().__init__()
                self.plot = Plot()
                self.plot.add_graph3d([0.0, 5.0], [0.0, 5.0], [0.0, 5.0], edges=[(0, 1)])
                self.messages: list = []

            def compose(self) -> ComposeResult:
                yield PlotWidget(self.plot, id="plot", pickable=True, render_mode="unsupported")

            def on_plot_widget_element_hovered(self, msg) -> None:
                self.messages.append(msg)

            def on_plot_widget_element_picked(self, msg) -> None:
                self.messages.append(msg)

        app = Unsupported()
        async with app.run_test(size=(90, 24)) as pilot:
            widget = app.query_one("#plot", PlotWidget)
            whole = "\n".join(widget.render_line(y).text for y in range(24))
            assert "Kitty graphics protocol" in whole, "the notice names the requirement"
            assert "iTerm2" in whole and "Ghostty" in whole, "it suggests terminals"
            assert "PLOTUI_RENDER" in whole, "it names the override knob"
            assert "\x1b_G" not in whole, "no graphics escapes on an unsupported terminal"

            # Interaction is inert: no hover/pick messages, no crash.
            await pilot.hover("#plot", offset=(45, 12))
            await pilot.click("#plot", offset=(45, 12))
            assert app.messages == []

    asyncio.run(drive())


def test_direct_mode_emits_full_res_kitty_image() -> None:
    async def drive() -> None:
        class Direct(App):
            def __init__(self) -> None:
                super().__init__()
                self.plot = Plot()
                self.plot.add_graph3d([0.0, 5.0], [0.0, 5.0], [0.0, 5.0], edges=[(0, 1)])
                self.picks: list = []

            def compose(self) -> ComposeResult:
                yield PlotWidget(self.plot, id="plot", render_mode="direct", pickable=True)

            def on_plot_widget_element_picked(self, msg) -> None:
                self.picks.append(msg.element)

        app = Direct()
        async with app.run_test(size=(60, 20)) as pilot:
            widget = app.query_one("#plot", PlotWidget)
            line0 = "".join(seg.text for seg in widget.render_line(0))
            assert "\x1b[s\x1b_G" in line0, "image escape at the widget origin"
            assert "i=4242" in line0, "fixed id: frames replace atomically"
            assert "U=1" not in line0, "direct placement, no Unicode placeholders"
            assert "\U0010eeee" not in line0, "no placeholder glyphs"
            # Frames double-buffer: this one is placed under its own id and
            # only then is the other buffer's placement deleted, so the
            # region is never blank between frames (delete-first would
            # blank it until the next frame decodes — visible flicker).
            assert "i=4242,p=4242,a=T" in line0 or "i=4243,p=4243,a=T" in line0
            assert line0.count("a=T") == 1
            assert line0.count("a=d") == 1
            assert line0.index("a=T") < line0.index("a=d"), "placement precedes the delete"
            first_id = 4242 if "i=4242,p=4242,a=T" in line0 else 4243
            assert f"a=d,d=i,i={4242 + 4243 - first_id},q=2" in line0, "the other buffer is retired"
            # ...and the next frame swaps roles.
            widget.invalidate()
            line0_next = "".join(seg.text for seg in widget.render_line(0))
            other = 4242 + 4243 - first_id
            assert f"i={other},p={other},a=T" in line0_next
            assert f"a=d,d=i,i={first_id},q=2" in line0_next
            # Other lines carry no image payload, just blank cells.
            line1 = "".join(seg.text for seg in widget.render_line(1))
            assert "\x1b_G" not in line1

            # Picking uses the full-res pixel geometry: find a node cell via
            # the widget's own mapping and click it.
            w, h = widget.size.width, widget.size.height
            for y in range(h):
                for x in range(w):
                    px_w, px_h, px, py, r = widget._pixel_geometry(x, y)
                    if app.plot.pick_element_px(px_w, px_h, px, py, r):
                        await pilot.click("#plot", offset=(x, y))
                        assert app.picks and app.picks[-1] is not None
                        return
            raise AssertionError("no pickable cell found in direct mode")

    asyncio.run(drive())


def test_unmount_deletes_the_image_from_the_terminal() -> None:
    """Quitting the app must not leave the last frame painted over the shell."""

    async def drive() -> list[str]:
        class Direct(App):
            def __init__(self) -> None:
                super().__init__()
                self.plot = Plot()
                self.plot.add_graph3d([0.0, 5.0], [0.0, 5.0], [0.0, 5.0], edges=[(0, 1)])

            def compose(self) -> ComposeResult:
                yield PlotWidget(self.plot, id="plot", render_mode="direct")

        app = Direct()
        writes: list[str] = []
        async with app.run_test(size=(40, 12)):
            driver = app._driver
            original = driver.write

            def spy(data: str) -> None:
                writes.append(data)
                original(data)

            driver.write = spy  # type: ignore[method-assign]
        return writes

    writes = asyncio.run(drive())
    assert any(
        "\x1b_Ga=d,d=i,i=4242" in w for w in writes
    ), "unmount must emit the kitty delete escape"
    assert any(
        "\x1b_Ga=d,d=i,i=4243" in w for w in writes
    ), "both direct-mode buffers are deleted, whichever holds the last frame"
    assert PlotWidget.kitty_cleanup().count("a=d") == 2


def test_crosshair_repaints_only_when_the_snap_changes() -> None:
    """Every mouse move used to re-rasterize, re-upload and re-decode the
    whole frame. The crosshair is drawn from the nearest sample, so moves
    inside one sample's basin change nothing and must cost nothing."""

    async def drive() -> None:
        class Sparse(App):
            def __init__(self) -> None:
                super().__init__()
                self.plot = Plot()
                self.plot.add_line([1.0, 2.0, 3.0, 4.0], [0.0, 1.0, 1.0, 1.2])

            def compose(self) -> ComposeResult:
                yield PlotWidget(self.plot, id="plot", render_mode="direct")

        app = Sparse()
        async with app.run_test(size=(80, 24)) as pilot:
            widget = app.query_one("#plot", PlotWidget)
            w, h = widget.size.width, widget.size.height
            px_w, px_h, *_ = widget._pixel_geometry(0, 0)
            # Cells along the middle row, grouped by where they snap.
            snaps = []
            for x in range(w):
                _, _, px, _, _ = widget._pixel_geometry(x, h // 2)
                snaps.append(app.plot.hover2d_snap_px(px_w, px_h, px))
            basin = [x for x in range(w) if snaps[x] is not None and snaps[x] == snaps[w // 2]]
            other = next(x for x in range(w) if snaps[x] is not None and snaps[x] != snaps[w // 2])
            assert len(basin) >= 3, "a sparse line has wide basins"

            await pilot.hover("#plot", offset=(basin[0], h // 2))
            after_first = widget._version
            for x in basin[1:]:
                await pilot.hover("#plot", offset=(x, h // 2))
            assert widget._version == after_first, "moves inside the basin never repaint"
            await pilot.hover("#plot", offset=(other, h // 2))
            assert widget._version == after_first + 1, "crossing into another basin repaints once"

    asyncio.run(drive())


def test_detect_cell_px_uses_terminal_size() -> None:
    # The ioctl itself needs a real terminal; the size computation is exposed
    # as a pure function (native, shared with the Rust frontends).
    from plotui._plotui import cell_px_from_winsize

    # A terminal reporting 1920x1350 over 80x25 cells (retina iTerm2).
    assert cell_px_from_winsize(25, 80, 1920, 1350) == (24, 54)  # 1920//80, 1350//25
    # A terminal that reports no pixel size yields nothing → detect_cell_px
    # falls back (fallback plumbing covered by the widget default-path tests).
    assert cell_px_from_winsize(25, 80, 0, 0) is None


def test_interactive_scale_reduces_only_while_dragging_a_large_3d_plot() -> None:
    async def drive() -> None:
        class Big(App):
            def __init__(self) -> None:
                super().__init__()
                self.plot = Plot()
                n = 500
                xs = [float(i % 10) for i in range(n)]
                ys = [float(i // 10) for i in range(n)]
                zs = [float(i % 7) for i in range(n)]
                self.plot.add_graph3d(xs, ys, zs, edges=[])

            def compose(self) -> ComposeResult:
                yield PlotWidget(
                    self.plot, id="plot", render_mode="direct", interactive_scale=0.5
                )

        app = Big()
        async with app.run_test(size=(80, 24)) as pilot:
            widget = app.query_one("#plot", PlotWidget)
            assert widget._large is True
            assert widget._active_scale() == 1.0, "still plot renders full-res"

            await pilot.mouse_down("#plot", offset=(40, 12))
            await pilot.hover("#plot", offset=(46, 15))  # move → drag
            assert widget.dragging is True
            assert widget._active_scale() == 0.5, "large 3D drag drops to half-res"

            await pilot.mouse_up("#plot", offset=(46, 15))
            assert widget._active_scale() == 1.0, "full-res restored when the drag ends"

    asyncio.run(drive())


def test_small_plot_never_reduces_resolution() -> None:
    async def drive() -> None:
        app = _Harness()  # 4-node graph
        async with app.run_test(size=(80, 24)) as pilot:
            widget = app.query_one("#plot", PlotWidget)
            assert widget._large is False
            await pilot.mouse_down("#plot", offset=(40, 12))
            await pilot.hover("#plot", offset=(46, 15))
            assert widget._active_scale() == 1.0, "small graphs stay full-res while dragging"

    asyncio.run(drive())


def test_tmux_wrap_only_wraps_inside_tmux(monkeypatch) -> None:
    from plotui.textual import tmux_wrap

    esc = "\x1b_Gi=4242,a=T;data\x1b\\"

    # Outside tmux: untouched.
    monkeypatch.delenv("TMUX", raising=False)
    assert tmux_wrap(esc) == esc

    # Inside tmux: wrapped in the passthrough DCS with every ESC doubled.
    monkeypatch.setenv("TMUX", "/tmp/tmux-1000/default,123,0")
    wrapped = tmux_wrap(esc)
    assert wrapped.startswith("\x1bPtmux;")
    assert wrapped.endswith("\x1b\\")
    # The inner payload is the original with each ESC (0x1b) doubled.
    inner = wrapped[len("\x1bPtmux;") : -len("\x1b\\")]
    assert inner == esc.replace("\x1b", "\x1b\x1b")
    # Unwrapping (halving the doubled ESCs) recovers the original escape.
    assert inner.replace("\x1b\x1b", "\x1b") == esc


def test_direct_mode_wraps_transmit_for_tmux(monkeypatch) -> None:
    async def drive() -> None:
        class Direct(App):
            def __init__(self) -> None:
                super().__init__()
                self.plot = Plot()
                self.plot.add_graph3d([0.0, 5.0], [0.0, 5.0], [0.0, 5.0], edges=[(0, 1)])

            def compose(self) -> ComposeResult:
                yield PlotWidget(self.plot, id="plot", render_mode="direct")

        app = Direct()
        async with app.run_test(size=(40, 12)) as pilot:
            widget = app.query_one("#plot", PlotWidget)
            # Re-render the frame with TMUX set, then read line 0.
            monkeypatch.setenv("TMUX", "/tmp/tmux-1000/default,1,0")
            widget.invalidate()
            await pilot.pause()
            line0 = "".join(seg.text for seg in widget.render_line(0))
            assert line0.startswith("\x1bPtmux;"), "direct frame is tmux-wrapped"
            assert "\x1b\x1b_G" in line0, "inner APC has doubled ESCs"
            # The raw (unwrapped) APC must not appear un-doubled.
            assert "\x1b_G" not in line0.replace("\x1b\x1b", "")

    asyncio.run(drive())


def test_shift_drag_pans_the_plot_exactly_with_the_pointer() -> None:
    """Pan is in image pixels; one dragged cell must move the picture one cell
    (cell_w × cell_h pixels), so a node stays under the pointer that drags it."""
    from types import SimpleNamespace

    async def drive() -> None:
        app = _Harness()
        async with app.run_test(size=(60, 20)) as pilot:
            widget = app.query_one("#plot", PlotWidget)
            px_w, px_h, _px, _py, _r = widget._pixel_geometry(0, 0)
            before = app.plot.project_nodes(px_w, px_h)[0]

            await pilot.mouse_down("#plot", offset=(30, 10))
            move = SimpleNamespace(screen_x=30 + 7, screen_y=10 + 3, x=37, y=13, shift=True)
            widget.on_mouse_move(move)  # shift-drag 7 cells right, 3 down
            after = app.plot.project_nodes(px_w, px_h)[0]

            assert after[0] - before[0] == pytest.approx(7 * widget._cell_w)
            assert after[1] - before[1] == pytest.approx(3 * widget._cell_h)

    asyncio.run(drive())


# --- streaming: widget.extend / widget.set_visible ---


def test_widget_extend_invalidates_and_changes_the_frame() -> None:
    async def drive() -> None:
        class Stream(App):
            def __init__(self) -> None:
                super().__init__()
                self.plot = Plot()
                self.handle = self.plot.add_line([0.0, 1.0], [0.0, 1.0], color=(10, 20, 30))

            def compose(self) -> ComposeResult:
                yield PlotWidget(self.plot, id="plot", render_mode="placeholder")

        app = Stream()
        async with app.run_test(size=(80, 24)) as pilot:
            widget = app.query_one("#plot", PlotWidget)
            assert widget.plot is app.plot
            widget.render_line(0)
            version, key = widget._version, widget._key

            widget.extend(app.handle, [2.0, 3.0], [4.0, 0.5])
            await pilot.pause()
            widget.render_line(0)
            assert widget._version == version + 1, "extend marks the frame dirty"
            assert widget._key != key, "the frame cache re-keys after extend"

    asyncio.run(drive())


def test_widget_set_visible_repaints_only_on_change() -> None:
    async def drive() -> None:
        class Two(App):
            def __init__(self) -> None:
                super().__init__()
                self.plot = Plot()
                self.plot.add_line([0.0, 1.0], [0.0, 1.0])
                self.handle = self.plot.add_line([0.0, 1.0], [1.0, 0.0])

            def compose(self) -> ComposeResult:
                yield PlotWidget(self.plot, id="plot", render_mode="placeholder")

        app = Two()
        async with app.run_test(size=(80, 24)):
            widget = app.query_one("#plot", PlotWidget)
            version = widget._version
            assert widget.set_visible(app.handle, False) is True
            assert widget._version == version + 1
            assert widget.set_visible(app.handle, False) is False
            assert widget._version == version + 1, "a no-op toggle must not repaint"

    asyncio.run(drive())


def test_streamed_growth_flips_the_large_flag() -> None:
    async def drive() -> None:
        class Growing(App):
            def __init__(self) -> None:
                super().__init__()
                self.plot = Plot()
                self.handle = self.plot.add_scatter3d([0.0], [0.0], [0.0])

            def compose(self) -> ComposeResult:
                yield PlotWidget(
                    self.plot, id="plot", render_mode="direct", interactive_scale=0.5
                )

        app = Growing()
        async with app.run_test(size=(80, 24)):
            widget = app.query_one("#plot", PlotWidget)
            assert widget._large is False

            n = 500
            widget.extend(
                app.handle,
                [float(i % 10) for i in range(n)],
                [float(i // 10) for i in range(n)],
                [float(i % 7) for i in range(n)],
            )
            assert widget._large is True, "streamed growth re-evaluates the load metric"

    asyncio.run(drive())


# --- the range slider ---


class _RangeHarness(App):
    """A 2D plot with the range slider on and a mid-data window, recording
    RangeChanged messages."""

    def __init__(self) -> None:
        super().__init__()
        self.plot = Plot()
        xs = [float(i) for i in range(30)]
        self.plot.add_line(xs, [x * 0.5 for x in xs], name="signal")
        self.plot.set_x_window((5.0, 15.0))
        self.ranges: list[tuple[float, float] | None] = []

    def compose(self) -> ComposeResult:
        yield PlotWidget(
            self.plot, id="plot", range_slider=True, render_mode="placeholder"
        )

    def on_plot_widget_range_changed(self, msg: PlotWidget.RangeChanged) -> None:
        self.ranges.append(msg.window)


def _mouse_ev(x: int, y: int):
    from types import SimpleNamespace

    return SimpleNamespace(x=x, y=y, screen_x=x, screen_y=y, shift=False)


def _cell_on_strip(widget: PlotWidget, part: str) -> tuple[int, int]:
    """Find a cell whose center hit-tests to the given strip part."""
    w, h = widget.size.width, widget.size.height
    for y in range(h - 1, -1, -1):
        for x in range(w):
            px_w, px_h, px, py, _ = widget._pixel_geometry(x, y)
            hit = widget.plot.range_slider_hit(px_w, px_h, px, py, float(widget._cell_w))
            if hit == part:
                return x, y
    raise AssertionError(f"no {part} cell found in a {w}x{h} widget")


def test_range_slider_drag_posts_range_changed() -> None:
    async def drive() -> None:
        app = _RangeHarness()
        async with app.run_test(size=(80, 24)) as pilot:
            widget = app.query_one("#plot", PlotWidget)
            cx, cy = _cell_on_strip(widget, "window")
            widget.on_mouse_down(_mouse_ev(cx, cy))
            widget.on_mouse_move(_mouse_ev(cx + 3, cy))
            widget.on_mouse_up(_mouse_ev(cx + 3, cy))
            await pilot.pause()
            assert app.ranges, "a released strip drag posts RangeChanged"
            lo, hi = app.ranges[-1]
            assert lo > 5.0 and hi > 15.0, "the window slid right"
            assert abs((hi - lo) - 10.0) < 1e-6, "span preserved"

    asyncio.run(drive())


def test_plot_drag_pans_window_when_set() -> None:
    async def drive() -> None:
        app = _RangeHarness()
        async with app.run_test(size=(80, 24)) as pilot:
            widget = app.query_one("#plot", PlotWidget)
            cam = app.plot.camera_state()
            # A drag in the plot area (top rows), not on the strip.
            widget.on_mouse_down(_mouse_ev(40, 5))
            widget.on_mouse_move(_mouse_ev(37, 5))
            widget.on_mouse_up(_mouse_ev(37, 5))
            await pilot.pause()
            lo, hi = app.plot.x_window()
            assert lo > 5.0, "dragging left slides the view right (grab the data)"
            assert app.plot.camera_state() == cam, "the camera stays out of it"
            assert not app.ranges or app.ranges == [], "plain pans post no message"

    asyncio.run(drive())


# --- drag routes through the apply_* hooks, not around them ---


def _drag(widget, dx: int, dy: int, *, shift: bool = False) -> None:
    """One drag step from the position a preceding mouse_down recorded."""
    from types import SimpleNamespace

    x0, y0 = widget._last_pos
    widget.on_mouse_move(
        SimpleNamespace(
            screen_x=x0 + dx, screen_y=y0 + dy, x=x0 + dx, y=y0 + dy, shift=shift
        )
    )


def test_drag_reaches_the_overridable_hooks() -> None:
    """A subclass overrides apply_rotate/apply_pan to hook camera changes;
    a drag must arrive there. Going straight to the core would move the
    camera behind the subclass's back."""

    class Hooked(PlotWidget):
        def __init__(self, *args, **kwargs) -> None:
            super().__init__(*args, **kwargs)
            self.rotates: list[tuple[float, float]] = []
            self.pans: list[tuple[float, float]] = []

        def apply_rotate(self, d_yaw: float, d_pitch: float) -> None:
            self.rotates.append((d_yaw, d_pitch))
            super().apply_rotate(d_yaw, d_pitch)

        def apply_pan(self, dx: float, dy: float) -> None:
            self.pans.append((dx, dy))
            super().apply_pan(dx, dy)

    class Host(App):
        def __init__(self) -> None:
            super().__init__()
            self.plot = Plot()
            self.plot.add_graph3d([0.0, 5.0], [0.0, 5.0], [0.0, 5.0], edges=[(0, 1)])

        def compose(self) -> ComposeResult:
            yield Hooked(self.plot, id="plot", render_mode="placeholder")

    async def drive() -> None:
        app = Host()
        async with app.run_test(size=(60, 20)) as pilot:
            widget = app.query_one("#plot", Hooked)
            await pilot.mouse_down("#plot", offset=(30, 10))
            _drag(widget, 7, 3)
            assert widget.rotates, "a plain drag must reach apply_rotate"
            assert not widget.pans

            _drag(widget, 4, 2, shift=True)
            assert widget.pans, "a shift-drag must reach apply_pan"

    asyncio.run(drive())


def test_a_rotation_locked_view_pans_instead_of_tilting() -> None:
    """The pan-first mapping a flat view wants: a plain drag must move the
    camera's pan and leave yaw/pitch untouched."""

    async def drive() -> None:
        app = _Harness()
        async with app.run_test(size=(60, 20)) as pilot:
            widget = app.query_one("#plot", PlotWidget)
            app.plot.set_input_map("pan_x", "pan_y")
            before = app.plot.camera_state()
            await pilot.mouse_down("#plot", offset=(30, 10))
            _drag(widget, 7, 3)
            after = app.plot.camera_state()
            assert after[:2] == before[:2], "a pan-mapped drag must not rotate"
            assert after[3] - before[3] == pytest.approx(7 * widget._cell_w)
            assert after[4] - before[4] == pytest.approx(3 * widget._cell_h)

    asyncio.run(drive())


def test_hook_decomposition_matches_the_core_exactly() -> None:
    """Routing through the hooks must not change what a gesture does: the
    widget's decomposition and Plot.apply_drag land on the same camera."""

    async def drive() -> None:
        app = _Harness()
        async with app.run_test(size=(60, 20)) as pilot:
            widget = app.query_one("#plot", PlotWidget)
            await pilot.mouse_down("#plot", offset=(30, 10))
            _drag(widget, 7, -3)
            through_hooks = app.plot.camera_state()

            direct = Plot()
            direct.add_graph3d([0.0, 5.0], [0.0, 5.0], [0.0, 5.0], edges=[(0, 1)])
            direct.apply_drag(7, -3, False, 0.03, widget._cell_w, widget._cell_h, 0.15)
            assert through_hooks == pytest.approx(direct.camera_state())

    asyncio.run(drive())


def test_input_map_reads_back_what_was_set() -> None:
    plot = Plot()
    assert plot.input_map() == ("yaw", "pitch", "pan_x", "pan_y")
    plot.set_input_map("pan_x", "-pitch", "off")
    assert plot.input_map() == ("pan_x", "-pitch", "off", "pan_y")


# --- dragging a graph node ---


def test_dragging_a_graph_node_moves_it_and_posts_node_moved() -> None:
    """A press on a box grabs the box: the node follows the pointer cell for
    cell, the other node stays put, and the release posts NodeMoved with the
    node's new layout position. A press elsewhere still pans the plot."""
    from types import SimpleNamespace

    class Graph(App):
        def __init__(self) -> None:
            super().__init__()
            self.plot = Plot()
            self.plot.add_graph2d([0.0, 0.0], [8.0, 0.0], [(0, 1)], labels=["alpha", "beta"])
            self.moved: list[tuple[int, float, float]] = []

        def compose(self) -> ComposeResult:
            yield PlotWidget(self.plot, id="plot", render_mode="placeholder")

        def on_plot_widget_node_moved(self, msg: PlotWidget.NodeMoved) -> None:
            self.moved.append((msg.index, msg.x, msg.y))

    async def drive() -> None:
        app = Graph()
        async with app.run_test(size=(60, 20)) as pilot:
            widget = app.query_one("#plot", PlotWidget)
            px_w, px_h, _px, _py, _r = widget._pixel_geometry(0, 0)
            # The cell over node 0's centre.
            nx, ny = None, None
            for y in range(20):
                for x in range(60):
                    pw, ph, px, py, radius = widget._pixel_geometry(x, y)
                    if app.plot.pick_px(pw, ph, px, py, radius) == 0:
                        nx, ny = x, y
                        break
                if nx is not None:
                    break
            assert nx is not None, "node 0 must be under some cell"
            before = app.plot.project_nodes(px_w, px_h)

            await pilot.mouse_down("#plot", offset=(nx, ny))
            assert widget._node_drag == 0
            move = SimpleNamespace(screen_x=nx + 5, screen_y=ny + 2, x=nx + 5, y=ny + 2, shift=False)
            widget.on_mouse_move(move)  # drag 5 cells right, 2 down
            after = app.plot.project_nodes(px_w, px_h)
            assert after[0][0] - before[0][0] == pytest.approx(5 * widget._cell_w, abs=1.0)
            assert after[0][1] - before[0][1] == pytest.approx(2 * widget._cell_h, abs=1.0)
            assert after[1][0] == pytest.approx(before[1][0], abs=1.0), "node 1 stays put"
            assert after[1][1] == pytest.approx(before[1][1], abs=1.0)

            await pilot.mouse_up("#plot", offset=(nx + 5, ny + 2))
            await pilot.pause()
            assert len(app.moved) == 1 and app.moved[0][0] == 0
            assert app.moved[0][1] > 0.0, "x grew in layout units"
            assert widget._node_drag is None

            # A press on empty space pans instead (2D drag = pan).
            before = app.plot.project_nodes(px_w, px_h)
            await pilot.mouse_down("#plot", offset=(1, 1))
            assert widget._node_drag is None
            move = SimpleNamespace(screen_x=4, screen_y=1, x=4, y=1, shift=False)
            widget.on_mouse_move(move)
            after = app.plot.project_nodes(px_w, px_h)
            assert after[1][0] - before[1][0] == pytest.approx(3 * widget._cell_w, abs=1.0)
            await pilot.mouse_up("#plot", offset=(4, 1))
            await pilot.pause()
            assert len(app.moved) == 1, "a plain pan is not a node move"

    asyncio.run(drive())


def test_draggable_false_leaves_nodes_alone() -> None:
    async def drive() -> None:
        class Graph(App):
            def __init__(self) -> None:
                super().__init__()
                self.plot = Plot()
                self.plot.add_graph2d([0.0, 0.0], [8.0, 0.0], [(0, 1)], labels=["alpha", "beta"])

            def compose(self) -> ComposeResult:
                yield PlotWidget(self.plot, id="plot", render_mode="placeholder", draggable=False)

        app = Graph()
        async with app.run_test(size=(60, 20)) as pilot:
            widget = app.query_one("#plot", PlotWidget)
            await pilot.mouse_down("#plot", offset=(30, 10))
            assert widget._node_drag is None
            await pilot.mouse_up("#plot", offset=(30, 10))

    asyncio.run(drive())


def test_image_slots_keep_two_widgets_on_their_own_ids() -> None:
    """Two plots on one screen: each widget's frames must ride its own pair
    of Kitty image ids — shared ids show one picture in both places
    (placeholder mode) or delete each other's frames (direct mode) — and
    unmounting one must leave the other's images alone."""
    from plotui.textual import image_ids_for

    assert image_ids_for(0) == (4242, 4243)
    assert image_ids_for(1) == (4244, 4245)

    async def drive() -> list[str]:
        class Pair(App):
            def compose(self) -> ComposeResult:
                left, right = Plot(), Plot()
                left.add_line([0.0, 1.0], [0.0, 1.0])
                right.add_line([0.0, 1.0], [1.0, 0.0])
                yield PlotWidget(left, id="left", render_mode="placeholder")
                yield PlotWidget(right, id="right", render_mode="placeholder", image_slot=1)

        app = Pair()
        writes: list[str] = []
        async with app.run_test(size=(40, 24)):
            left = app.query_one("#left", PlotWidget)
            right = app.query_one("#right", PlotWidget)
            assert left.image_ids == (4242, 4243)
            assert right.image_ids == (4244, 4245)
            left._ensure_frame()
            right._ensure_frame()
            assert "i=4242," in left._transmit
            assert "i=4244," in right._transmit
            # the placeholder colour encodes the id: different ids, different colours
            assert left._style != right._style
            assert right.kitty_cleanup_own() == "\x1b_Ga=d,d=i,i=4244\x1b\\\x1b_Ga=d,d=i,i=4245\x1b\\"
            driver = app._driver
            original = driver.write

            def spy(data: str) -> None:
                writes.append(data)
                original(data)

            driver.write = spy  # type: ignore[method-assign]
            await right.remove()
        return writes

    writes = asyncio.run(drive())
    assert any("i=4244" in w for w in writes)
    assert not any("i=4242" in w and "a=d" in w for w in writes[:-1]), (
        "unmounting the right plot must not delete the left one's image"
    )
    # the process-wide cleanup names every slot a widget has taken
    assert "i=4244" in PlotWidget.kitty_cleanup() and "i=4242" in PlotWidget.kitty_cleanup()


def test_overlay_spans_become_readout_keep_out_boxes() -> None:
    """Text the widget draws over the plot is handed to the plot as
    `keep_out` fractions, so the crosshair readout dodges it like the
    legend — and a plot the host swaps in gets them on its next frame."""

    async def drive() -> None:
        class Labelled(App):
            def __init__(self) -> None:
                super().__init__()
                self.plot = Plot()
                self.plot.add_line([0.0, 1.0, 2.0], [0.0, 1.0, 0.5])

            def compose(self) -> ComposeResult:
                yield PlotWidget(self.plot, id="plot", render_mode="placeholder")

        app = Labelled()
        async with app.run_test(size=(40, 20)) as pilot:
            widget = app.query_one("#plot", PlotWidget)
            await pilot.pause()
            version = widget._version
            widget.set_overlay([(1, 4, "legend", None), (2, 4, "more", None)])
            boxes = [v for box in widget._plot.keep_out() for v in box]
            assert boxes == pytest.approx([
                4 / 40, 1 / 20, 10 / 40, 2 / 20,
                4 / 40, 2 / 20, 8 / 40, 3 / 20,
            ])
            assert widget._version == version + 1  # the readout may move: a new frame
            # the same spans again change nothing, so no frame is spent
            widget.set_overlay([(1, 4, "legend", None), (2, 4, "more", None)])
            assert widget._version == version + 1
            # a swapped-in plot inherits the boxes at its first frame
            fresh = Plot()
            fresh.add_line([0.0, 1.0], [0.0, 1.0])
            widget._plot = fresh
            widget.invalidate()
            widget._ensure_frame()
            assert fresh.keep_out() == widget._plot.keep_out() and len(fresh.keep_out()) == 2
            widget.set_overlay([])
            assert widget._plot.keep_out() == []

    asyncio.run(drive())


def test_mouse_over_the_legend_lights_its_row() -> None:
    """Moving over a legend row lights it (the core's `legend_hover`), a
    move off the box or leaving the widget clears it; each is one repaint."""

    async def drive() -> None:
        class Named(App):
            def __init__(self) -> None:
                super().__init__()
                self.plot = Plot()
                self.plot.add_line([0.0, 1.0], [0.0, 1.0], name="alpha")
                self.plot.add_line([0.0, 1.0], [1.0, 0.0], name="beta")

            def compose(self) -> ComposeResult:
                yield PlotWidget(self.plot, id="plot", render_mode="placeholder")

        app = Named()
        async with app.run_test(size=(80, 24)) as pilot:
            widget = app.query_one("#plot", PlotWidget)
            await pilot.pause()
            px_w, px_h, *_ = widget._pixel_geometry(0, 0)
            # find a cell on legend row 0 by asking the core
            cell = next(
                (x, y) for y in range(widget.size.height) for x in range(widget.size.width)
                if widget._plot.legend_row_at(px_w, px_h, *widget._pixel_geometry(x, y)[2:4]) == 0
            )
            version = widget._version
            await pilot.hover("#plot", offset=cell)
            await pilot.pause()
            assert widget._plot.legend_hover() == 0
            assert widget._version > version  # repainted (the crosshair may repaint on the same move)
            await pilot.hover("#plot", offset=(0, widget.size.height - 1))
            await pilot.pause()
            assert widget._plot.legend_hover() is None

    asyncio.run(drive())
