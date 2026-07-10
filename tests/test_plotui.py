"""End-to-end tests through the Python API (the wheel's public surface)."""

import math

import pytest

from plotui import Plot


def demo_2d() -> Plot:
    plot = Plot()
    xs = [float(i) for i in range(30)]
    plot.add_line(xs, [math.sin(x * 0.4) * 3 + 5 for x in xs], name="signal")
    plot.add_scatter(xs, [math.cos(x * 0.4) * 2 + 5 for x in xs], name="samples")
    return plot


def demo_3d() -> Plot:
    plot = Plot()
    plot.add_scatter3d([0.0, 1.0, -1.0], [0.0, 2.0, 1.0], [0.0, 1.0, -2.0])
    return plot


def test_import_and_version():
    import plotui

    assert plotui.__version__
    assert plotui.Plot is Plot


def test_halfblock_shape_2d_and_3d():
    for plot in (demo_2d(), demo_3d()):
        out = plot.render_halfblock(80, 24)
        assert isinstance(out, str)
        assert len(out.split("\n")) == 24


def test_kitty_escape_structure():
    esc = demo_2d().render_kitty(40, 12, 10, 20)
    assert esc.startswith("\x1b[s\x1b_G")
    assert esc.endswith("\x1b[u")
    assert "i=4242" in esc


def test_kitty_placeholder_structure():
    transmit, id_rgb, rows = demo_2d().render_kitty_placeholder(40, 12, 10, 20)
    assert "U=1" in transmit
    assert id_rgb == (0, 16, 146)  # 4242 in the placeholder fg color
    assert len(rows) == 12
    assert all("\U0010eeee" in r for r in rows)


def test_2d_render_differs_from_camera_moves():
    plot = demo_2d()
    before = plot.render_halfblock(80, 24)
    plot.zoom_by(2.0)
    assert plot.render_halfblock(80, 24) != before
    plot.reset()
    assert plot.render_halfblock(80, 24) == before


def test_bars_and_explicit_colors():
    plot = Plot()
    plot.add_bar([0.0, 1.0, 2.0], [3.0, 1.0, 2.0], color=(201, 133, 0), name="load")
    out = plot.render_halfblock(60, 20)
    assert "201;133;0" in out, "explicit bar color must reach the terminal bytes"


def test_default_palette_assigns_distinct_colors():
    plot = Plot()
    plot.add_line([0.0, 1.0], [0.0, 1.0])
    plot.add_line([0.0, 1.0], [1.0, 0.0])
    out = plot.render_halfblock(60, 20)
    assert "57;135;229" in out, "palette slot 1 (blue)"
    assert "25;158;112" in out, "palette slot 2 (aqua)"


def test_pick_roundtrip_3d():
    plot = Plot()
    plot.add_graph3d(
        [0.0, 5.0, -5.0],
        [0.0, 5.0, -5.0],
        [0.0, 5.0, -5.0],
        edges=[(0, 1), (1, 2)],
    )
    # Some node must be pickable somewhere on screen: scan a coarse grid.
    hits = {
        plot.pick(80, 24, float(c), float(r), radius=3.0)
        for c in range(0, 80, 4)
        for r in range(0, 24, 2)
    }
    assert hits - {None}, "at least one node is pickable on screen"
    plot.set_selected(0)
    plot.set_selected(None)


def test_mismatched_lengths_are_truncated_not_fatal():
    plot = Plot()
    plot.add_line([0.0, 1.0, 2.0], [0.0, 1.0])  # extra x is dropped
    plot.add_scatter([], [])  # empty series is legal
    assert plot.render_halfblock(40, 12)


def test_nan_data_does_not_crash():
    plot = Plot()
    plot.add_line([0.0, 1.0, float("nan"), 3.0], [0.0, float("inf"), 2.0, 3.0])
    assert plot.render_halfblock(40, 12)


def test_kitty_cleanup_is_static():
    assert Plot.kitty_cleanup() == "\x1b_Ga=d,d=i,i=4242\x1b\\"



def test_pick_element_finds_nodes_and_edges():
    plot = Plot()
    plot.add_graph3d(
        [0.0, 5.0, -5.0, 5.0],
        [0.0, 5.0, -5.0, -5.0],
        [0.0, 5.0, -5.0, 0.0],
        edges=[(0, 1), (1, 2), (0, 3)],
    )
    kinds = set()
    for c in range(0, 300, 3):
        for r in range(0, 200, 3):
            el = plot.pick_element_px(300, 200, float(c), float(r), 4.0)
            if el is not None:
                kinds.add(el[0])
                assert el[1] >= 0
    assert kinds == {"node", "edge"}, f"both kinds reachable, got {kinds}"


def test_hover_lights_up_white_and_reports_changes():
    plot = Plot()
    plot.add_graph3d([0.0, 5.0], [0.0, 5.0], [0.0, 5.0], edges=[(0, 1)])
    plain = plot.render_halfblock(60, 20)
    assert "255;255;255" not in plain

    assert plot.set_hovered(("edge", 0)) is True
    assert plot.set_hovered(("edge", 0)) is False, "unchanged hover reports False"
    assert "255;255;255" in plot.render_halfblock(60, 20), "hovered edge is white"

    assert plot.set_hovered(("node", 1)) is True
    assert "255;255;255" in plot.render_halfblock(60, 20), "hovered node is white"

    assert plot.set_hovered(None) is True
    assert plot.render_halfblock(60, 20) == plain


def test_set_selected_accepts_legacy_ints_and_element_tuples():
    plot = Plot()
    plot.add_graph3d([0.0, 5.0], [0.0, 5.0], [0.0, 5.0], edges=[(0, 1)])
    plot.set_selected(0)  # legacy: bare int is a node
    assert "255;255;255" in plot.render_halfblock(60, 20)
    plot.set_selected(("edge", 0))
    assert "255;255;255" in plot.render_halfblock(60, 20)
    plot.set_selected(None)
    assert "255;255;255" not in plot.render_halfblock(60, 20)
    with pytest.raises(ValueError):
        plot.set_selected(("vertex", 0))

def test_camera_state_roundtrip_and_clamps():
    plot = demo_3d()
    plot.set_camera_state(2.0, 9.9, 500.0, 3.0, -4.0)
    assert plot.camera_state() == (2.0, 1.55, 50.0, 3.0, -4.0)
    before = plot.render_halfblock(60, 20)
    other = demo_3d()
    other.set_camera_state(*plot.camera_state())
    assert other.render_halfblock(60, 20) == before, "restored camera renders identically"


def test_project_nodes_agrees_with_pick():
    plot = Plot()
    plot.add_graph3d([0.0, 5.0, -5.0], [0.0, 5.0, -5.0], [0.0, 5.0, -5.0], edges=[(0, 1)])
    plot.rotate(0.4, -0.3)
    plot.zoom_by(1.5)
    plot.pan(7.0, -3.0)
    projected = plot.project_nodes(300, 200)
    assert len(projected) == 3
    for i, (sx, sy, _depth) in enumerate(projected):
        assert plot.pick_px(300, 200, sx, sy, 2.0) == i


def test_node_sizes_and_edge_colors():
    plot = Plot()
    plot.set_show_box(False)
    plot.add_graph3d(
        [0.0, 1.0],
        [0.0, 0.0],
        [0.0, 0.0],
        edges=[(0, 1)],
        node_colors=[(255, 255, 255), (255, 255, 255)],
        size=1.0,
        node_sizes=[6.0, 1.0],
        edge_colors=[(9, 250, 9)],
    )
    out = plot.render_halfblock(80, 40)
    assert "9;250;9" in out, "explicit edge color reaches the terminal bytes"
    uniform = Plot()
    uniform.set_show_box(False)
    uniform.add_graph3d(
        [0.0, 1.0],
        [0.0, 0.0],
        [0.0, 0.0],
        edges=[],
        node_colors=[(255, 255, 255), (255, 255, 255)],
        size=1.0,
    )
    # Depth fog dims far nodes, so count drawn cells rather than pure white.
    lit = lambda s: s.count("38;2;")  # noqa: E731
    assert lit(out) > lit(uniform.render_halfblock(80, 40)), "node_sizes grows a node"


def test_kitty_placeholder_cells_structure():
    transmit, id_rgb, cells = demo_2d().render_kitty_placeholder_cells(40, 12, 10, 20)
    assert "U=1" in transmit
    assert id_rgb == (0, 16, 146)
    assert len(cells) == 12
    for row in cells:
        assert len(row) == 40
        # Every cell self-addressed: placeholder + row + column diacritics.
        assert all(len(cell) == 3 and cell[0] == "\U0010eeee" for cell in row)
        assert len({cell[2] for cell in row}) == 40, "distinct column diacritics"


if __name__ == "__main__":
    raise SystemExit(pytest.main([__file__, "-v"]))
