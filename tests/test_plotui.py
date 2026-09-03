"""End-to-end tests through the Python API (the wheel's public surface).

Pixel-level assertions go through ``render_rgba`` — raw RGBA8 out of the
engine, no escape sequences to parse.
"""

import math

import pytest

from plotui import LayeredLayout, Plot, from_dot, reachable

W, H = 120, 80  # default framebuffer size for pixel assertions


def pixels(data: bytes):
    return (data[i : i + 4] for i in range(0, len(data), 4))


def has_color(plot: Plot, color: tuple[int, int, int], w: int = W, h: int = H) -> bool:
    target = bytes(color) + b"\xff"
    return any(px == target for px in pixels(plot.render_rgba(w, h)))


def drawn_count(plot: Plot, w: int = W, h: int = H) -> int:
    return sum(1 for px in pixels(plot.render_rgba(w, h)) if px[3] == 255)


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


def test_render_rgba_shape_2d_and_3d():
    for plot in (demo_2d(), demo_3d()):
        data = plot.render_rgba(W, H)
        assert isinstance(data, bytes)
        assert len(data) == W * H * 4
        assert any(px[3] == 255 for px in pixels(data)), "something is drawn"
        assert any(px[3] == 0 for px in pixels(data)), "background stays transparent"


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
    before = plot.render_rgba(W, H)
    plot.zoom_by(2.0)
    assert plot.render_rgba(W, H) != before
    plot.reset()
    assert plot.render_rgba(W, H) == before


def test_bars_and_explicit_colors():
    plot = Plot()
    plot.add_bar([0.0, 1.0, 2.0], [3.0, 1.0, 2.0], color=(201, 133, 0), name="load")
    assert has_color(plot, (201, 133, 0)), "explicit bar color must reach the pixels"


def test_default_palette_assigns_distinct_colors():
    plot = Plot()
    plot.add_line([0.0, 1.0], [0.0, 1.0])
    plot.add_line([0.0, 1.0], [1.0, 0.0])
    assert has_color(plot, (230, 60, 120)), "colorway slot 1 (pink)"
    assert has_color(plot, (69, 200, 209)), "colorway slot 2 (cyan)"


def test_color_shorthands_parse_names_and_hex():
    plot = Plot()
    plot.add_line([0.0, 1.0], [0.0, 1.0], color="red")
    plot.add_line([0.0, 1.0], [1.0, 0.0], color="#45c8d1")
    assert has_color(plot, (255, 0, 0)), "named color must reach the pixels"
    assert has_color(plot, (69, 200, 209)), "hex color must reach the pixels"
    with pytest.raises(ValueError, match="unknown color"):
        Plot().add_line([0.0, 1.0], [0.0, 1.0], color="blurple")


def test_scatter3d_without_color_takes_colorway_slots():
    # Depth fog dims 3D marks, so compare byte-identical renders against
    # explicitly colored traces instead of probing for pure palette pixels.
    xs, ys, zs = [0.0, 1.0], [0.0, 1.0], [0.0, 1.0]
    default = Plot()
    default.add_scatter3d(xs, ys, zs)
    default.add_scatter3d(ys, zs, xs)
    explicit = Plot()
    explicit.add_scatter3d(xs, ys, zs, color=(230, 60, 120))
    explicit.add_scatter3d(ys, zs, xs, color=(69, 200, 209))
    assert default.render_rgba(W, H) == explicit.render_rgba(W, H)


def test_set_colorway_by_name_and_list():
    plot = Plot()
    plot.set_colorway("vivid")
    plot.add_line([0.0, 1.0], [0.0, 1.0])
    assert has_color(plot, (255, 30, 120)), "vivid slot 1"

    custom = Plot()
    custom.set_colorway(["red", (0, 200, 0)])
    custom.add_line([0.0, 1.0], [0.0, 1.0])
    custom.add_line([0.0, 1.0], [1.0, 0.0])
    assert has_color(custom, (255, 0, 0)), "custom slot 1 (shorthand)"
    assert has_color(custom, (0, 200, 0)), "custom slot 2 (tuple)"

    with pytest.raises(ValueError, match="unknown colorway"):
        Plot().set_colorway("neon")
    with pytest.raises(ValueError, match="at least one color"):
        Plot().set_colorway([])


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
        plot.pick_px(160, 96, float(x), float(y), 6.0)
        for x in range(0, 160, 4)
        for y in range(0, 96, 4)
    }
    assert hits - {None}, "at least one node is pickable on screen"
    plot.set_selected(0)
    plot.set_selected(None)


def test_mismatched_lengths_are_truncated_not_fatal():
    plot = Plot()
    plot.add_line([0.0, 1.0, 2.0], [0.0, 1.0])  # extra x is dropped
    plot.add_scatter([], [])  # empty series is legal
    assert len(plot.render_rgba(40, 24)) == 40 * 24 * 4


def test_nan_data_does_not_crash():
    plot = Plot()
    plot.add_line([0.0, 1.0, float("nan"), 3.0], [0.0, float("inf"), 2.0, 3.0])
    assert len(plot.render_rgba(40, 24)) == 40 * 24 * 4


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
    plain = plot.render_rgba(W, H)
    assert not has_color(plot, (255, 255, 255))

    assert plot.set_hovered(("edge", 0)) is True
    assert plot.set_hovered(("edge", 0)) is False, "unchanged hover reports False"
    assert has_color(plot, (255, 255, 255)), "hovered edge is white"

    assert plot.set_hovered(("node", 1)) is True
    assert has_color(plot, (255, 255, 255)), "hovered node is white"

    assert plot.set_hovered(None) is True
    assert plot.render_rgba(W, H) == plain


def test_set_selected_accepts_legacy_ints_and_element_tuples():
    plot = Plot()
    plot.add_graph3d([0.0, 5.0], [0.0, 5.0], [0.0, 5.0], edges=[(0, 1)])
    plot.set_selected(0)  # legacy: bare int is a node
    assert has_color(plot, (255, 255, 255))
    plot.set_selected(("edge", 0))
    assert has_color(plot, (255, 255, 255))
    plot.set_selected(None)
    assert not has_color(plot, (255, 255, 255))
    with pytest.raises(ValueError):
        plot.set_selected(("vertex", 0))


def test_camera_state_roundtrip_and_clamps():
    plot = demo_3d()
    plot.set_camera_state(2.0, 9.9, 500.0, 3.0, -4.0)
    assert plot.camera_state() == (2.0, 1.55, 50.0, 3.0, -4.0)
    before = plot.render_rgba(W, H)
    other = demo_3d()
    other.set_camera_state(*plot.camera_state())
    assert other.render_rgba(W, H) == before, "restored camera renders identically"


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
    assert has_color(plot, (9, 250, 9), 160, 80), "explicit edge color reaches the pixels"
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
    assert drawn_count(plot, 160, 80) > drawn_count(uniform, 160, 80), "node_sizes grows a node"


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


def test_set_bounds_pins_the_projection_across_rebuilds():
    from plotui import Plot

    def first_node(pts, pin):
        plot = Plot()
        plot.set_show_box(False)
        xs, ys, zs = zip(*pts)
        plot.add_graph3d(list(xs), list(ys), list(zs), edges=[])
        if pin:
            plot.set_bounds((-4.0, -4.0, 0.0), (4.0, 4.0, 0.0))
        return plot.project_nodes(200, 200)[0][:2]

    full = [(1.0, 1.0, 0.0), (-4.0, -4.0, 0.0)]
    part = [(1.0, 1.0, 0.0)]
    assert first_node(full, False) != first_node(part, False)
    assert first_node(full, True) == first_node(part, True)


def test_node_shapes_change_the_mark_and_reject_unknown_names():
    from plotui import Plot

    def lit(shape):
        plot = Plot()
        plot.set_show_box(False)
        plot.add_graph3d([0.0], [0.0], [0.0], edges=[], size=6.0, node_shapes=[shape])
        rgba = plot.render_rgba(200, 200)
        return sum(1 for i in range(3, len(rgba), 4) if rgba[i] > 0)

    counts = {s: lit(s) for s in ("disc", "ring", "square", "triangle", "diamond", "diamond-open", "dot")}
    assert counts["square"] > counts["disc"] > counts["ring"]
    assert counts["diamond"] > counts["diamond-open"] > 0
    assert counts["dot"] < counts["disc"]
    assert counts["triangle"] < counts["square"]

    with pytest.raises(ValueError, match='unknown node shape "blob"'):
        Plot().add_graph3d([0.0], [0.0], [0.0], edges=[], node_shapes=["blob"])


def test_set_chrome_recolours_the_grid():
    """The grid is drawn in the chrome's grid colour, so a host can make it
    recede into its own background; omitted colours keep their defaults."""
    a = Plot()
    a.add_line([0.0, 1.0, 2.0], [0.0, 1.0, 0.5], color=(255, 255, 255))
    assert has_color(a, (45, 50, 66), 160, 100)
    assert not has_color(a, (200, 10, 10), 160, 100)
    b = Plot()
    b.add_line([0.0, 1.0, 2.0], [0.0, 1.0, 0.5], color=(255, 255, 255))
    b.set_chrome(grid=(200, 10, 10))
    assert has_color(b, (200, 10, 10), 160, 100)
    assert not has_color(b, (45, 50, 66), 160, 100)


def test_axis_y2_y3_accepted_and_change_render():
    """Right-hand axes change the frame: a rule and a tick-label gutter appear
    on the right, and each additional axis widens it."""

    def build(axes: tuple[str, ...]) -> bytes:
        plot = Plot()
        xs = [float(i) for i in range(20)]
        plot.add_line(xs, [math.sin(x * 0.5) + 1 for x in xs])
        for i, axis in enumerate(axes):
            plot.add_line(xs, [x * 10.0 ** (i + 2) for x in xs], axis=axis)
        return plot.render_rgba(300, 200)

    renders = [build(()), build(("y2",)), build(("y2", "y3"))]
    assert renders[0] != renders[1]
    assert renders[0] != renders[2]
    assert renders[1] != renders[2]


def test_default_axis_matches_explicit_y():
    """`axis="y"` is the default — the pre-y2 call shape stays byte-identical."""
    implicit, explicit = Plot(), Plot()
    xs = [float(i) for i in range(10)]
    ys = [math.sin(x) for x in xs]
    implicit.add_line(xs, ys, name="signal")
    explicit.add_line(xs, ys, name="signal", axis="y")
    assert implicit.render_rgba(W, H) == explicit.render_rgba(W, H)


def test_invalid_axis_raises():
    xs, ys = [0.0, 1.0], [0.0, 1.0]
    for method in ("add_line", "add_scatter"):
        with pytest.raises(ValueError, match="'y', 'y2' or 'y3'"):
            getattr(Plot(), method)(xs, ys, axis="y4")
    with pytest.raises(ValueError, match="'y', 'y2' or 'y3'"):
        Plot().add_bar(xs, ys, axis="z")


def test_line3d_draws_and_stays_unpickable():
    plot = Plot()
    plot.add_line3d([0.0, 1.0, 2.0], [0.0, 1.0, 0.0], [0.0, 0.5, 1.0], color=(9, 250, 9))
    assert plot.is_3d()
    assert has_color(plot, (9, 250, 9)) or drawn_count(plot) > 0
    assert plot.node_count() == 0
    assert plot.vertex_count() == 3
    assert plot.pick_px(W, H, W / 2, H / 2, 50.0) is None


def test_line3d_nan_breaks_the_line():
    solid, gapped = Plot(), Plot()
    for p, mid in ((solid, 0.5), (gapped, math.nan)):
        p.add_line3d([-1.0, 0.0, 1.0], [0.0, mid, 0.0], [0.0, 0.0, 0.0])
        p.set_bounds((-1.0, -1.0, -1.0), (1.0, 1.0, 1.0))
        p.set_show_box(False)
    assert drawn_count(gapped) < drawn_count(solid)


def test_surface3d_draws_with_default_viridis():
    plot = Plot()
    plot.add_surface3d(
        [0.0, 1.0, 2.0],
        [0.0, 1.0, 2.0],
        [[math.sin(x + y) for x in range(3)] for y in range(3)],
    )
    assert plot.is_3d()
    assert plot.node_count() == 0
    assert plot.vertex_count() == 9
    assert drawn_count(plot) > 100, "a surface fills real area"


def test_surface3d_solid_color_and_wireframe():
    def build(wireframe: bool) -> Plot:
        plot = Plot()
        plot.add_surface3d(
            [0.0, 1.0, 2.0],
            [0.0, 1.0, 2.0],
            [[0.0] * 3] * 3,
            color=(200, 60, 60),
            colormap=None,
            wireframe=wireframe,
        )
        plot.set_show_box(False)
        return plot

    assert build(False).render_rgba(W, H) != build(True).render_rgba(W, H)


def test_surface3d_named_trace_gets_a_legend():
    def build(name):
        plot = Plot()
        plot.add_surface3d([0.0, 1.0], [0.0, 1.0], [[0.0, 1.0], [1.0, 0.0]], name=name)
        plot.set_show_box(False)
        return plot

    assert drawn_count(build("terrain"), 300, 200) > drawn_count(build(None), 300, 200)


def test_surface3d_rejects_bad_grids_and_colormaps():
    with pytest.raises(ValueError, match="grid"):
        Plot().add_surface3d([0.0, 1.0], [0.0, 1.0], [[0.0, 1.0]])
    with pytest.raises(ValueError, match="grid"):
        Plot().add_surface3d([0.0, 1.0], [0.0, 1.0], [[0.0], [1.0]])
    with pytest.raises(ValueError, match="viridis, plasma"):
        Plot().add_surface3d([0.0, 1.0], [0.0, 1.0], [[0.0, 1.0], [1.0, 0.0]], colormap="magma")


def test_add_box_groups_and_validation():
    a = [float(i) for i in range(1, 10)] + [100.0]
    b = [float(i) * 2 for i in range(1, 10)]

    plot = Plot()
    plot.add_box([a, b], color="red", name="groups")
    plot.set_categories("x", ["a", "b"])
    assert drawn_count(plot, 320, 240) > 0

    # Horizontal boxes read their groups off the y axis.
    sideways = Plot()
    sideways.add_box([a, b], orientation="horizontal")
    sideways.set_categories("y", ["a", "b"])
    assert drawn_count(sideways, 320, 240) > 0

    with pytest.raises(ValueError, match="at least one group"):
        Plot().add_box([])
    with pytest.raises(ValueError, match="orientation"):
        Plot().add_box([a], orientation="sideways")

    # It is structural: the boxes are derived, so the plot is rebuilt to change.
    h = plot.add_box([a])
    with pytest.raises(ValueError, match="rebuild the plot"):
        plot.extend(h, [1.0], [1.0])


def test_band_and_error_bars():
    xs = [0.0, 1.0, 2.0]
    plot = Plot()
    plot.add_band(xs, [1.0, 0.0, 1.0], [4.0, 5.0, 4.0], color="blue", name="ci")
    plot.add_line(xs, [2.5, 2.5, 2.5], color="red")
    assert drawn_count(plot, 240, 180) > 0

    # Error bars belong to a series and reach past its points, so the axis
    # has to grow for them.
    # Count the series colour, not every drawn pixel: widening the axis
    # rescales the whole frame, so totals move for reasons unrelated to bars.
    def red(plot, w=240, h=180):
        rgba = plot.render_rgba(w, h)
        return sum(
            1
            for i in range(0, len(rgba), 4)
            if rgba[i + 3] and (rgba[i], rgba[i + 1], rgba[i + 2]) == (255, 0, 0)
        )

    bare = Plot()
    h = bare.add_scatter([1.0, 2.0], [1.0, 1.0], color="red")
    before = red(bare)
    bare.set_error_bars(h, y_plus=[3.0, 3.0])
    assert red(bare) > before

    # Asymmetric, and x bars too.
    bare.set_error_bars(h, y_plus=[3.0, 3.0], y_minus=[0.0, 0.0])
    bare.set_error_bars(h, x_plus=[0.5, 0.5])
    # Clearing both axes is how they go away.
    bare.set_error_bars(h)

    with pytest.raises(ValueError, match="scatter and line"):
        bars = bare.add_bar([0.0], [1.0])
        bare.set_error_bars(bars, y_plus=[1.0])
    with pytest.raises(ValueError, match="unknown trace handle"):
        bare.set_error_bars(99, y_plus=[1.0])


def test_barmode_stack_and_group():
    """Overlay hides a shorter series behind a taller one; group and stack
    are the two ways of not doing that."""

    def visible_red(mode, w=300, h=300):
        plot = Plot()
        assert plot.set_barmode(mode) is (mode != "overlay")
        plot.add_bar([0.0, 1.0], [3.0, 4.0], color="red")
        plot.add_bar([0.0, 1.0], [2.0, 5.0], color="blue")
        rgba = plot.render_rgba(w, h)
        return sum(
            1
            for i in range(0, len(rgba), 4)
            if rgba[i + 3] and (rgba[i], rgba[i + 1], rgba[i + 2]) == (255, 0, 0)
        )

    overlay = visible_red("overlay")
    assert visible_red("group") > overlay, "grouping must stop blue covering red"
    assert visible_red("stack") > overlay, "stacking must too"

    with pytest.raises(ValueError, match="barmode"):
        Plot().set_barmode("pile")


def test_horizontal_bars_and_categories():
    def extent(plot, w=300, h=300):
        """(widest row, tallest column) of bar-coloured pixels."""
        rgba = plot.render_rgba(w, h)
        hit = lambda x, y: (
            rgba[(y * w + x) * 4 + 3]
            and (rgba[(y * w + x) * 4], rgba[(y * w + x) * 4 + 1], rgba[(y * w + x) * 4 + 2])
            == (255, 0, 0)
        )
        widest = max(sum(1 for x in range(w) if hit(x, y)) for y in range(h))
        tallest = max(sum(1 for y in range(h) if hit(x, y)) for x in range(w))
        return widest, tallest

    vert = Plot()
    vert.add_bar([0.0, 1.0, 2.0], [3.0, 5.0, 4.0], color="red")
    wv, tv = extent(vert)
    assert tv > wv, "vertical bars are tall columns"

    horiz = Plot()
    horiz.add_bar([0.0, 1.0, 2.0], [3.0, 5.0, 4.0], color="red", orientation="horizontal")
    wh, th = extent(horiz)
    assert wh > th, "horizontal bars are wide rows"

    with pytest.raises(ValueError, match="orientation"):
        Plot().add_bar([0.0], [1.0], orientation="sideways")

    # Categories label the rows; setting them reports whether anything moved.
    assert horiz.set_categories("y", ["alpha", "beta", "gamma"]) is True
    assert horiz.set_categories("y", ["alpha", "beta", "gamma"]) is False
    assert horiz.set_categories("y", []) is True
    with pytest.raises(ValueError, match="axis 'x' or 'y'"):
        horiz.set_categories("z", ["a"])


def test_add_heatmap_grid_colorbar_and_holes():
    xs, ys = [0.0, 1.0, 2.0], [0.0, 1.0]
    zs = [[0.0, 1.0, 2.0], [3.0, 4.0, 5.0]]

    plot = Plot()
    plot.add_heatmap(xs, ys, zs, name="grid")
    solid = drawn_count(plot, 300, 200)
    assert solid > 0

    # A colorbar is added by default and takes its own margin, so the same
    # grid without one covers more of the frame.
    bare = Plot()
    bare.add_heatmap(xs, ys, zs, colorbar=False)
    assert drawn_count(bare, 300, 200) != solid

    # NaN is a hole, not a zero.
    holed = Plot()
    holed.add_heatmap(xs, ys, [[0.0, 1.0, 2.0], [3.0, float("nan"), 5.0]], colorbar=False)
    assert drawn_count(holed, 300, 200) < drawn_count(bare, 300, 200)

    # The grid rule is the one add_surface3d already enforces.
    with pytest.raises(ValueError, match="grid"):
        Plot().add_heatmap(xs, ys, [[0.0, 1.0]])
    with pytest.raises(ValueError, match="viridis, plasma"):
        Plot().add_heatmap(xs, ys, zs, colormap="magma")


def test_add_histogram_bins_and_streaming():
    values = [float(i % 20) for i in range(200)]
    plot = Plot()
    h = plot.add_histogram(values, bins=8, color="red")
    assert drawn_count(plot) > 0

    # Streaming rebins; a new extreme widens the range and so changes pixels.
    before = plot.render_rgba(240, 180)
    plot.extend_values(h, [500.0])
    assert plot.render_rgba(240, 180) != before

    # The two knobs are mutually exclusive.
    with pytest.raises(ValueError, match="not both"):
        Plot().add_histogram(values, bins=8, bin_width=1.0)
    with pytest.raises(ValueError, match="at least 1"):
        Plot().add_histogram(values, bins=0)
    with pytest.raises(ValueError, match="positive"):
        Plot().add_histogram(values, bin_width=-1.0)

    # A histogram is structural for the coordinate extend path, and says so.
    with pytest.raises(ValueError, match="extend_values"):
        plot.extend(h, [1.0], [1.0])
    with pytest.raises(ValueError, match="unknown trace handle"):
        plot.extend_values(99, [1.0])
    line = plot.add_line([0.0], [0.0])
    with pytest.raises(ValueError, match="histogram"):
        plot.extend_values(line, [1.0])


def test_add_step_modes_and_validation():
    """A step holds its value across a flat run; a straight line never does."""

    def widest_red_row(plot, w=240, h=240):
        rgba = plot.render_rgba(w, h)
        best = 0
        for y in range(h):
            row = 0
            for x in range(w):
                i = (y * w + x) * 4
                if rgba[i + 3] and (rgba[i], rgba[i + 1], rgba[i + 2]) == (255, 0, 0):
                    row += 1
            best = max(best, row)
        return best

    straight = Plot()
    straight.add_line([0.0, 1.0, 2.0], [0.0, 1.0, 0.0], color="red")
    diagonal = widest_red_row(straight)

    for where in ("pre", "post", "mid"):
        stepped = Plot()
        stepped.add_step([0.0, 1.0, 2.0], [0.0, 1.0, 0.0], color="red", where_=where)
        assert widest_red_row(stepped) > diagonal * 4, where

    with pytest.raises(ValueError, match="step mode"):
        Plot().add_step([0.0], [0.0], where_="stairs")


def test_set_point_styles_channels_are_independent():
    plot = Plot()
    h = plot.add_scatter([0.0, 1.0, 2.0], [1.0, 1.0, 1.0])
    base = drawn_count(plot)

    # Sizes alone: bigger marks must cover more pixels.
    plot.set_point_styles(h, sizes=[8.0, 8.0, 8.0])
    assert drawn_count(plot) > base

    # Colors alone, given as tuples and as shorthand strings.
    plot.set_point_styles(h, colors=[(10, 200, 30), "red", "#0000ff"])
    plot.set_point_styles(h, shapes=["disc", "ring", "square"])

    with pytest.raises(ValueError, match="shape"):
        plot.set_point_styles(h, shapes=["blob"])
    with pytest.raises(ValueError, match="scatter"):
        bars = plot.add_bar([0.0], [1.0])
        plot.set_point_styles(bars, colors=["red"])
    with pytest.raises(ValueError, match="unknown trace handle"):
        plot.set_point_styles(99, colors=["red"])


def test_mesh3d_draws_and_rejects_bad_indices():
    xs, ys, zs = [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, 0.0]
    plot = Plot()
    plot.add_mesh3d(xs, ys, zs, [(0, 1, 2)], name="tri")
    plot.set_show_box(False)
    assert drawn_count(plot, 300, 200) > 0

    with pytest.raises(ValueError, match="names no vertex"):
        Plot().add_mesh3d(xs, ys, zs, [(0, 1, 9)])
    with pytest.raises(ValueError, match="viridis, plasma"):
        Plot().add_mesh3d(xs, ys, zs, [(0, 1, 2)], colormap="magma")
    with pytest.raises((ValueError, TypeError)):
        Plot().add_mesh3d(xs, ys, zs, [(0, 1)])


def test_hover2d_crosshair_draws_and_clears():
    plot = demo_2d()
    plain = plot.render_rgba(300, 200)
    assert plot.set_hover2d(150.0) is True
    assert plot.set_hover2d(150.0) is False, "unchanged hover reports no change"
    hovered = plot.render_rgba(300, 200)
    assert hovered != plain
    assert plot.set_hover2d(None) is True
    assert plot.render_rgba(300, 200) == plain


def test_hover2d_ignored_by_3d_plots():
    plot = demo_3d()
    plain = plot.render_rgba(300, 200)
    plot.set_hover2d(150.0)
    assert plot.render_rgba(300, 200) == plain


# --- streaming append: handles, extend, set_visible ---


def test_add_returns_sequential_handles():
    plot = Plot()
    assert plot.add_line([0.0], [0.0]) == 0
    assert plot.add_scatter([0.0], [0.0]) == 1
    assert plot.add_bar([0.0, 1.0], [1.0, 2.0]) == 2
    assert plot.add_scatter3d([0.0], [0.0], [0.0]) == 3
    assert plot.add_line3d([0.0], [0.0], [0.0]) == 4


def test_extend_matches_one_shot_build():
    xs = [float(i) for i in range(20)]
    ys = [math.sin(x * 0.4) * 3 + 5 for x in xs]
    zs = [math.cos(x * 0.3) for x in xs]

    whole = Plot()
    whole.add_line(xs, ys, color=(10, 20, 30))
    whole.add_scatter(xs, ys, color=(40, 50, 60))
    whole.add_bar(xs[:8], ys[:8], color=(70, 80, 90))
    inc = Plot()
    line = inc.add_line(xs[:5], ys[:5], color=(10, 20, 30))
    scat = inc.add_scatter(xs[:1], ys[:1], color=(40, 50, 60))
    bar = inc.add_bar(xs[:3], ys[:3], color=(70, 80, 90))
    inc.extend(line, xs[5:], ys[5:])
    inc.extend(scat, xs[1:], ys[1:])
    inc.extend(bar, xs[3:8], ys[3:8])
    assert whole.render_rgba(W, H) == inc.render_rgba(W, H)

    whole3 = Plot()
    whole3.add_scatter3d(xs, ys, zs, color=(10, 20, 30))
    whole3.add_line3d(xs, ys, zs, color=(40, 50, 60))
    inc3 = Plot()
    scat3 = inc3.add_scatter3d(xs[:7], ys[:7], zs[:7], color=(10, 20, 30))
    line3 = inc3.add_line3d(xs[:2], ys[:2], zs[:2], color=(40, 50, 60))
    inc3.extend(scat3, xs[7:], ys[7:], zs[7:])
    inc3.extend(line3, xs[2:], ys[2:], zs[2:])
    assert whole3.render_rgba(W, H) == inc3.render_rgba(W, H)


def test_extend_error_paths():
    plot = Plot()
    line = plot.add_line([0.0], [0.0])
    scat3 = plot.add_scatter3d([0.0], [0.0], [0.0])
    graph = plot.add_graph3d([0.0], [0.0], [0.0], edges=[])
    surf = plot.add_surface3d([0.0, 1.0], [0.0, 1.0], [[0.0, 0.0], [0.0, 0.0]])

    with pytest.raises(ValueError, match="unknown trace handle"):
        plot.extend(99, [1.0], [1.0])
    with pytest.raises(ValueError, match="zs is for 3D"):
        plot.extend(line, [1.0], [1.0], [1.0])
    with pytest.raises(ValueError, match="needs xs, ys and zs"):
        plot.extend(scat3, [1.0], [1.0])
    with pytest.raises(ValueError, match="structural"):
        plot.extend(graph, [1.0], [1.0], [1.0])
    with pytest.raises(ValueError, match="structural"):
        plot.extend(surf, [1.0], [1.0], [1.0])
    with pytest.raises(ValueError, match="unknown trace handle"):
        plot.set_visible(99, False)

    graph2d = plot.add_graph2d([0.0], [0.0], edges=[])
    with pytest.raises(ValueError, match="graph2d traces are structural"):
        plot.extend(graph2d, [1.0], [1.0])


def test_extend_tolerates_nan_and_ragged_input():
    plot = Plot()
    line = plot.add_line([0.0, 1.0], [0.0, 1.0])
    plot.extend(line, [2.0, float("nan"), 3.0], [2.0, 5.0])  # ragged + NaN
    plot.extend(line, [], [])
    assert drawn_count(plot) > 0


def test_set_visible_matches_never_added():
    xs = [float(i) for i in range(12)]
    ys = [x * 0.5 + 1 for x in xs]
    y2 = [100 - x * 3 for x in xs]

    bare = Plot()
    bare.add_line(xs, ys, color=(10, 20, 30), name="a")
    toggled = Plot()
    toggled.add_line(xs, ys, color=(10, 20, 30), name="a")
    h = toggled.add_line(xs, y2, color=(40, 50, 60), name="b", axis="y2")
    before = toggled.render_rgba(W, H)

    assert toggled.set_visible(h, False) is True
    assert toggled.set_visible(h, False) is False  # repeat is a no-op
    assert toggled.render_rgba(W, H) == bare.render_rgba(W, H)
    assert toggled.set_visible(h, True) is True
    assert toggled.render_rgba(W, H) == before


def test_hidden_trace_keeps_palette_slot():
    # Hide-then-add and add-then-hide must agree: if hiding a trace freed its
    # palette slot, the second (default-colored) series would pick up slot 0
    # in one ordering and slot 1 in the other, and the renders would differ.
    xs, up, down = [0.0, 1.0], [0.0, 1.0], [1.0, 0.0]
    hide_first = Plot()
    h = hide_first.add_line(xs, up)
    hide_first.set_visible(h, False)
    hide_first.add_line(xs, down)
    hide_after = Plot()
    h2 = hide_after.add_line(xs, up)
    hide_after.add_line(xs, down)
    hide_after.set_visible(h2, False)
    assert hide_first.render_rgba(W, H) == hide_after.render_rgba(W, H)


def test_numpy_arrays_render_identically_to_lists():
    np = pytest.importorskip("numpy")
    xs = [float(i) for i in range(15)]
    ys = [math.sin(x) * 2 + 3 for x in xs]
    zs = [math.cos(x) for x in xs]

    from_lists = Plot()
    from_lists.add_line(xs, ys, color=(10, 20, 30))
    from_lists.add_scatter3d(xs, ys, zs, color=(40, 50, 60))
    for dtype in (np.float32, np.float64):
        from_np = Plot()
        from_np.add_line(np.array(xs, dtype=dtype), np.array(ys, dtype=dtype), color=(10, 20, 30))
        h = from_np.add_scatter3d(
            np.array(xs[:5], dtype=dtype), np.array(ys[:5], dtype=dtype), np.array(zs[:5], dtype=dtype),
            color=(40, 50, 60),
        )
        from_np.extend(
            h, np.array(xs[5:], dtype=dtype), np.array(ys[5:], dtype=dtype), np.array(zs[5:], dtype=dtype)
        )
        assert from_np.render_rgba(W, H) == from_lists.render_rgba(W, H), str(dtype)

    # Non-contiguous views take the strided path and must still match.
    strided = Plot()
    both = np.array([v for pair in zip(xs, ys) for v in pair], dtype=np.float64)
    strided.add_line(both[0::2], both[1::2], color=(10, 20, 30))
    lists = Plot()
    lists.add_line(xs, ys, color=(10, 20, 30))
    assert strided.render_rgba(W, H) == lists.render_rgba(W, H)


# --- force-directed graphs: ForceLayout + graph mutators ---


def _graph_plot(xs, ys, zs, edges):
    plot = Plot()
    h = plot.add_graph3d(xs, ys, zs, edges=edges, node_colors=[(200, 120, 90)] * len(xs))
    return plot, h


def test_force_layout_is_deterministic_and_settles():
    from plotui import ForceLayout

    edges = [(0, 1), (1, 2), (0, 2), (3, 4), (4, 5), (3, 5)]
    a, b = ForceLayout(6, edges, seed=9), ForceLayout(6, edges, seed=9)
    for _ in range(50):
        a.step()
        b.step()
    assert a.positions() == b.positions()
    energy = float("inf")
    for _ in range(600):
        energy = a.step()
    assert energy < 1e-3


def test_set_graph_positions_matches_one_shot_build():
    edges = [(0, 1), (1, 2)]
    target = ([0.5, -0.5, 0.0], [0.0, 0.5, -0.5], [-0.5, 0.0, 0.5])
    wide = ([5.0, -5.0, 0.0], [0.0, 5.0, -5.0], [-5.0, 0.0, 5.0])
    oneshot, _ = _graph_plot(*target, edges)
    moved, h = _graph_plot(*wide, edges)
    moved.set_graph_positions(h, *target)
    assert moved.render_rgba(W, H) == oneshot.render_rgba(W, H)


def test_set_graph_colors_recolors_and_restores():
    plot, h = _graph_plot([0.0, 1.0], [0.0, 1.0], [0.0, 1.0], [(0, 1)])
    before = plot.render_rgba(W, H)
    plot.set_graph_colors(h, [(9, 250, 9)] * 2, [(250, 9, 9)])
    assert plot.render_rgba(W, H) != before
    plot.set_graph_colors(h, [(200, 120, 90)] * 2)
    assert plot.render_rgba(W, H) == before


def test_extend_graph_matches_one_shot_build():
    oneshot, _ = _graph_plot(
        [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, 0.0], [(0, 1), (1, 2)]
    )
    inc, h = _graph_plot([0.0, 1.0], [0.0, 0.0], [0.0, 0.0], [(0, 1)])
    inc.extend_graph(h, [0.0], [1.0], [0.0], node_colors=[(200, 120, 90)], edges=[(1, 2)])
    assert inc.render_rgba(W, H) == oneshot.render_rgba(W, H)


def test_graph_mutator_error_messages_are_canonical():
    plot, h = _graph_plot([0.0, 1.0], [0.0, 1.0], [0.0, 1.0], [(0, 1)])
    s = plot.add_scatter3d([0.0], [0.0], [0.0])
    with pytest.raises(ValueError, match="length must match"):
        plot.set_graph_positions(h, [0.0], [0.0], [0.0])
    with pytest.raises(ValueError, match="length must match"):
        plot.set_graph_colors(h, [(1, 2, 3)])
    with pytest.raises(ValueError, match="unknown trace handle"):
        plot.set_graph_positions(99, [0.0], [0.0], [0.0])
    with pytest.raises(ValueError, match="wrong trace kind"):
        plot.extend_graph(s, [0.0], [0.0], [0.0])


# --- the x window and range slider ---


def test_x_window_windows_and_autoscales():
    plot = demo_2d()
    plain = plot.render_rgba(300, 200)
    assert plot.set_x_window((2.0, 8.0)) is True
    assert plot.set_x_window((2.0, 8.0)) is False, "unchanged window reports no change"
    assert plot.x_window() == (2.0, 8.0)
    assert plot.render_rgba(300, 200) != plain
    assert plot.set_x_window(None) is True
    assert plot.render_rgba(300, 200) == plain


def test_x_window_validation_message():
    plot = demo_2d()
    with pytest.raises(ValueError, match=r"x_window needs finite lo < hi, got \(5, 5\)"):
        plot.set_x_window((5.0, 5.0))


def test_range_slider_strip_draws_and_clears():
    plot = demo_2d()
    plain = plot.render_rgba(300, 200)
    assert plot.set_range_slider(True) is True
    assert plot.range_slider() is True
    assert plot.render_rgba(300, 200) != plain, "the strip changes pixels"
    assert plot.set_range_slider(False) is True
    assert plot.render_rgba(300, 200) == plain


def test_datetime_x_sets_epoch_and_offsets():
    from datetime import datetime, timedelta, timezone

    plot = Plot()
    start = datetime(2026, 3, 10, 6, 0, tzinfo=timezone.utc)
    xs = [start + timedelta(hours=i) for i in range(48)]
    plot.add_line(xs, [float(i) for i in range(48)])
    # The epoch pins to the first timestamp's UTC midnight.
    assert plot.x_epoch() == datetime(2026, 3, 10, tzinfo=timezone.utc).timestamp()
    # Naive datetimes read as UTC wall time: same values, same plot.
    naive = Plot()
    naive.add_line([t.replace(tzinfo=None) for t in xs], [float(i) for i in range(48)])
    assert naive.render_rgba(300, 200) == plot.render_rgba(300, 200)
    # Mixing numeric x onto a time axis is one canonical error.
    with pytest.raises(ValueError, match="cannot mix datetime and numeric x"):
        plot.add_line([0.0, 1.0], [0.0, 1.0])
    # And datetime x onto a numeric plot likewise.
    numeric = demo_2d()
    with pytest.raises(ValueError, match="cannot mix datetime and numeric x"):
        numeric.add_line(xs, [float(i) for i in range(48)])


def test_numpy_datetime64_x():
    np = pytest.importorskip("numpy")
    plot = Plot()
    xs = np.arange("2026-01-01", "2026-01-11", dtype="datetime64[D]")
    plot.add_line(xs, np.arange(10, dtype="float32"))
    assert plot.x_epoch() == 1_767_225_600.0  # 2026-01-01T00:00Z
    assert len(plot.render_rgba(300, 200)) == 300 * 200 * 4


def test_drag_x_window_parts_roundtrip():
    plot = demo_2d()
    plot.set_range_slider(True)
    # 400x240: the strip is live; a right-handle drag from the full extent
    # materializes a window.
    assert plot.drag_x_window(400, 240, "right", -120.0) is True
    lo, hi = plot.x_window()
    assert hi < 25.0, f"right handle must have pulled the window in, got {hi}"
    assert plot.range_slider_hit(400, 240, 200.0, 224.0, 4.0) is not None
    assert plot.jump_x_window(400, 240, 300.0) is True
    assert plot.pan_x_window(400, 240, 25.0) is True
    assert plot.zoom_x_window(400, 240, 200.0, 2.0) is True
    assert plot.shift_x_window(0.5) is True
    with pytest.raises(ValueError, match="range part must be"):
        plot.drag_x_window(400, 240, "middle", 1.0)


# --- 2D graphs: pipelines and DAGs ---

PIPELINE_EDGES = [(0, 1), (1, 2), (0, 2)]


def test_add_graph2d_labels_edges_and_pick():
    plot = Plot()
    layout = LayeredLayout(3, PIPELINE_EDGES)
    xs, ys = layout.positions()
    handle = plot.add_graph2d(
        xs,
        ys,
        PIPELINE_EDGES,
        labels=["fetch", "clean", "publish"],
        node_colors=[(250, 10, 10), (10, 250, 10), (10, 10, 250)],
        node_shapes=["rounded", "box", "ellipse"],
        routes=layout.routes(),
    )
    assert handle == 0
    assert plot.node_count() == 3
    # Every node colour reaches the pixels, and so does the label ink.
    for color in [(250, 10, 10), (10, 250, 10), (10, 10, 250)]:
        assert has_color(plot, color, 400, 300)
    assert has_color(plot, (205, 210, 220), 400, 300), "labels are drawn into the frame"

    # A node's projected centre picks that node — the two are solved once.
    nodes = plot.project_nodes(400, 300)
    assert len(nodes) == 3
    for i, (px, py, depth) in enumerate(nodes):
        assert depth == 0.0
        assert plot.pick_element_px(400, 300, px, py, 0.0, 0.0) == ("node", i)
    assert plot.pick_element_px(400, 300, 2.0, 2.0, 4.0, 4.0) is None

    # And the edge between two boxes picks the edge, not a node.
    mid = ((nodes[0][0] + nodes[1][0]) / 2, (nodes[0][1] + nodes[1][1]) / 2)
    assert plot.pick_element_px(400, 300, mid[0], mid[1], 0.0, 3.0) == ("edge", 0)


def test_graph2d_hides_axes_and_show_axes_is_a_tri_state():
    frame = (70, 78, 96)
    plot = Plot()
    plot.add_graph2d([0.0, 0.0], [1.0, 0.0], [(0, 1)], labels=["a", "b"])
    assert plot.show_axes is None
    assert not has_color(plot, frame, 300, 220)
    plot.show_axes = True
    assert has_color(plot, frame, 300, 220)
    plot.show_axes = None
    assert not has_color(plot, frame, 300, 220)


def test_graph2d_mutators_recolour_and_reroute():
    plot = Plot()
    handle = plot.add_graph2d([0.0, 0.0], [1.0, 0.0], [(0, 1)], labels=["a", "b"])
    plot.set_graph_colors(handle, [(9, 250, 9), (9, 250, 9)], [(250, 9, 9)])
    assert has_color(plot, (9, 250, 9), 300, 220)
    plot.set_graph_positions(handle, [0.0, 1.0], [1.0, 0.0], [0.0, 0.0])
    plot.set_graph_routes(handle, [[(0.5, 0.9)]])
    plot.set_graph_routes(handle, [[]])
    plot.extend_graph(
        handle,
        [1.0],
        [-1.0],
        [0.0],
        node_colors=[(80, 80, 80)],
        edges=[(1, 2)],
        labels=["c"],
    )
    assert plot.node_count() == 3

    with pytest.raises(ValueError, match="unknown node shape"):
        plot.add_graph2d([0.0], [0.0], [], node_shapes=["blob"])


def test_layered_layout_is_deterministic():
    a = LayeredLayout(3, PIPELINE_EDGES)
    b = LayeredLayout(3, PIPELINE_EDGES)
    assert a.positions() == b.positions()
    assert a.ranks() == b.ranks() == [0, 1, 2]
    # The edge that skips a rank gets one waypoint; the others are straight.
    assert [len(r) for r in a.routes()] == [0, 0, 1]

    # LR is TB turned a quarter turn.
    lr = LayeredLayout(3, PIPELINE_EDGES, rankdir="LR")
    tb_xs, tb_ys = a.positions()
    lr_xs, lr_ys = lr.positions()
    assert lr_xs == [-y for y in tb_ys]
    assert lr_ys == [-x for x in tb_xs]

    with pytest.raises(ValueError, match="unknown rankdir"):
        LayeredLayout(2, [(0, 1)], rankdir="sideways")


def test_from_dot():
    plot = from_dot("digraph nightly { a -> b -> c; a -> c }")
    assert plot.node_count() == 3
    assert plot.show_axes is False, "a graph's coordinates are not a scale"
    assert drawn_count(plot, 300, 220) > 0

    # rankdir is honoured and overridable.
    lr = from_dot("digraph { rankdir=LR; a -> b }")
    assert lr.project_nodes(400, 300)[0][0] < lr.project_nodes(400, 300)[1][0]
    forced = from_dot("digraph { rankdir=LR; a -> b }", rankdir="TB")
    assert forced.project_nodes(400, 300)[0][1] < forced.project_nodes(400, 300)[1][1]

    # Errors carry the shared line:col message.
    with pytest.raises(ValueError, match=r"1:13: '--' joins nodes in a graph"):
        from_dot("digraph { a -- b }")
    with pytest.raises(ValueError, match="HTML labels"):
        from_dot("digraph { a [label=<b>x</b>] }")


def test_reachable_follows_direction():
    assert reachable(3, PIPELINE_EDGES, 2) == [True, True, True]
    assert reachable(3, PIPELINE_EDGES, 2, upstream=False) == [False, False, True]
    assert reachable(3, PIPELINE_EDGES, 0, upstream=False) == [True, True, True]
    assert reachable(3, PIPELINE_EDGES, 99) == [False, False, False]


def test_titles_round_trip_and_draw():
    plot = demo_2d()
    plain = drawn_count(plot, 600, 400)
    assert plot.set_title("p99 latency") is True
    assert plot.set_x_title("requests") is True
    assert plot.set_y_title("ms") is True
    assert (plot.title(), plot.x_title(), plot.y_title()) == ("p99 latency", "requests", "ms")
    assert drawn_count(plot, 600, 400) > plain, "titles put ink on the frame"
    # Setting the same text again is not a change; "" clears, so a host can
    # pass a user's empty input straight through.
    assert plot.set_title("p99 latency") is False
    assert plot.set_title("") is True
    assert plot.title() is None


def test_explicit_range_replaces_autoscale():
    plot = demo_2d()
    assert plot.set_y_range((-10.0, 10.0)) is True
    assert plot.y_range() == (-10.0, 10.0)
    # A range is not a window: it pins the extent and leaves the camera alone.
    assert plot.x_window() is None
    assert plot.set_x_range((0.0, 3.0)) is True
    pinned = plot.render_rgba(300, 200)
    plot.zoom_by(2.0)
    assert plot.render_rgba(300, 200) != pinned, "zoom composes over a range"
    assert plot.set_x_range(None) is True
    assert plot.x_range() is None

    with pytest.raises(ValueError, match="needs finite lo < hi"):
        plot.set_x_range((5.0, 5.0))


def test_log_axes():
    plot = Plot()
    plot.add_line([1.0, 2.0, 3.0, 4.0], [1.0, 10.0, 100.0, 1000.0], name="rps")
    assert plot.set_y_log(True) is True
    assert plot.y_log() is True and plot.x_log() is False
    assert len(plot.render_rgba(400, 300)) == 400 * 300 * 4

    # A log axis has no coordinate for zero, so a range reaching one is
    # refused rather than quietly lifted at render time.
    with pytest.raises(ValueError, match="log y axis needs a positive range"):
        plot.set_y_range((0.0, 100.0))
    assert plot.set_y_range((0.5, 2000.0)) is True

    # Zero and negative samples simply do not place: the plot still renders,
    # and the axis stays positive.
    zeros = Plot()
    zeros.add_line([1.0, 2.0, 3.0], [0.0, -5.0, 100.0])
    zeros.set_y_log(True)
    assert len(zeros.render_rgba(400, 300)) == 400 * 300 * 4


def test_log_defers_to_a_categorical_axis():
    plot = Plot()
    plot.add_bar([0.0, 1.0, 2.0], [3.0, 9.0, 27.0])
    plot.set_categories("x", ["alpha", "beta", "gamma"])
    plot.set_x_log(True)
    # The flag is remembered, but names own the coordinate, so the frame is
    # the categorical one — identical to never having asked for log.
    assert plot.x_log() is True
    plain = Plot()
    plain.add_bar([0.0, 1.0, 2.0], [3.0, 9.0, 27.0])
    plain.set_categories("x", ["alpha", "beta", "gamma"])
    assert plot.render_rgba(400, 300) == plain.render_rgba(400, 300)
