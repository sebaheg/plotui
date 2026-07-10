"""Headless Textual tests for PlotWidget's opt-in hover/click picking.

Runs the real widget in Textual's test harness (half-block mode, since there
is no Kitty terminal in CI) and drives it with a mouse pilot.
"""

from __future__ import annotations

import asyncio

from textual.app import App, ComposeResult

from plotui import Plot
from plotui.textual import PlotWidget


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
        # Pin the mode: detection reads the developer's real terminal env,
        # but these tests assert against half-block geometry.
        yield PlotWidget(self.plot, id="plot", pickable=True, render_mode="halfblock")

    def on_plot_widget_element_hovered(self, msg: PlotWidget.ElementHovered) -> None:
        self.hovers.append(msg.element)

    def on_plot_widget_element_picked(self, msg: PlotWidget.ElementPicked) -> None:
        self.picks.append(msg.element)


def _cell_over(plot: Plot, widget: PlotWidget, kind: str) -> tuple[int, int]:
    """Find a cell whose center sits over an element of `kind` (half-block
    geometry: 1px per column, 2px per row) — same mapping the widget uses."""
    w, h = widget.size.width, widget.size.height
    for y in range(h):
        for x in range(w):
            el = plot.pick_element_px(w, h * 2, x + 0.5, y * 2 + 1.0, 5.0)
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
        assert "255;255;255" in app.plot.render_halfblock(80, 24)

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


def test_overlay_splices_text_into_halfblock_strips() -> None:
    async def drive() -> None:
        class Overlaid(App):
            def __init__(self) -> None:
                super().__init__()
                self.plot = Plot()
                self.plot.add_graph3d([0.0, 5.0], [0.0, 5.0], [0.0, 5.0], edges=[(0, 1)])

            def compose(self) -> ComposeResult:
                yield PlotWidget(self.plot, id="plot", force_halfblock=True)

        app = Overlaid()
        async with app.run_test(size=(60, 20)) as pilot:
            widget = app.query_one("#plot", PlotWidget)
            widget.set_overlay([(4, 7, "hello", None), (4, 3, "x", None), (99, 0, "off", None)])
            await pilot.pause()
            strip = widget.render_line(4)
            text = strip.text
            assert len(text) == 60, "strip width unchanged by splicing"
            assert text[7:12] == "hello"
            assert text[3] == "x"
            plain = widget.render_line(5).text
            assert "hello" not in plain
            # Clearing the overlay restores the full plot row.
            widget.set_overlay([])
            assert "hello" not in widget.render_line(4).text

    asyncio.run(drive())


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
                yield PlotWidget(self.plot, id="plot", render_mode="halfblock")

            def on_plot_widget_element_hovered(self, msg) -> None:
                self.hovers.append(msg.element)

        app = Quiet()
        async with app.run_test(size=(60, 20)) as pilot:
            await pilot.hover("#plot", offset=(30, 10))
            assert app.hovers == [], "no hover messages when pickable is off"
            assert "255;255;255" not in app.plot.render_halfblock(60, 20)

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
    # No silent degradation: unknown/old terminals get the notice, never
    # the low-fi half-block renderer (that is opt-in only).
    assert detect_render_mode(iterm_old) == "unsupported", "old iTerm2 lacks the protocol"
    assert detect_render_mode(plain) == "unsupported"
    assert detect_render_mode(wezterm) == "direct"
    # Explicit override beats every terminal signal.
    assert detect_render_mode({**plain, "PLOTUI_RENDER": "direct"}) == "direct"
    assert detect_render_mode({**kitty, "PLOTUI_RENDER": "halfblock"}) == "halfblock"
    assert detect_render_mode({**plain, "PLOTUI_RENDER": "kitty"}) == "placeholder"


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
            assert "PLOTUI_RENDER=halfblock" in whole, "it names the escape hatch"
            assert "\x1b_G" not in whole and "▀" not in whole, "no pixels, no halfblocks"

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
            # Each frame must replace the previous placement, never stack:
            # delete-by-id first, then exactly one placement with a fixed p=.
            assert "\x1b_Ga=d,d=i,i=4242,q=2\x1b\\" in line0
            assert line0.index("a=d") < line0.index("a=T"), "delete precedes placement"
            assert "p=1,a=T" in line0
            assert line0.count("a=T") == 1
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
