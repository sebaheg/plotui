"""End-to-end tests through the Python API (the wheel's public surface).

Pixel-level assertions go through ``render_rgba`` — raw RGBA8 out of the
engine, no escape sequences to parse.
"""

import math

import pytest

from plotui import Plot

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
    assert has_color(plot, (57, 135, 229)), "palette slot 1 (blue)"
    assert has_color(plot, (25, 158, 112)), "palette slot 2 (aqua)"


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
