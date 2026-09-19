# Changelog

Notable changes per release. Versions before 0.5.0 predate this file; their
history is in the git log and the GitHub releases.

## 0.6.0 — 2026-09-19

### Added

**The legend row under the pointer is lit.** `Plot.legend_hover` /
`set_legend_hover(px_w, px_h, px, py)` (and `legend_row_at`) light the
legend row under a pixel with a faint band, so the reader sees which row a
click would toggle; the Textual widget sets it on every mouse move and
clears it on leave, repainting only when the lit row changes.

**Legend rows a host declares.** `Plot.legend_entries` (Python
`set_legend_entries([(label, swatch, color, border, visible), …])`) replaces
the one-row-per-named-trace legend with rows the host names — for plots
whose categories are not traces, such as one graph whose nodes fall into
stages. Each row carries a swatch that mirrors what it names: `square` (the
trace chip), `line`, `disc` — inside a ring of `border` when given, drawn
with the node rasteriser — `ring` or `star`; a row with `visible=False`
stays, drained. `legend_corner` / `set_legend_corner("top-left")` moves the
box to the top left, and `legend_entry_hit` says which row a click landed
on (the trace `legend_hit` stands down while host rows are shown). The
hover readout keeps off the box wherever it sits.

**The hover readout keeps off the host's text.** `Plot.keep_out` (Python
`set_keep_out(rects)` / `keep_out()`) lists boxes, as frame fractions, that
the 2D crosshair readout must not cover — text a host draws over the plot in
its own layer, which the renderer could not see. The Textual widget sets it
from its overlay spans automatically, so a legend or label drawn with
`set_overlay` is dodged the way the in-canvas legend already was.

**Several plots on one screen.** `PlotWidget(image_slot=n)` places a
widget's frames under its own pair of Kitty image ids (`image_ids_for(n)`;
slot 0 is the default pair), so two widgets side by side no longer show one
picture in both places (placeholder mode) or delete each other's frames
(direct mode). `PlotWidget.kitty_cleanup()` names every slot taken in the
process; the new `kitty_cleanup_own()` is what one widget emits on unmount.
`Plot.render_kitty_placeholder_cells` grew an `image_id` argument. Direct
frames now use the image id as their placement id too (`p=<image id>`
instead of `p=1`): iTerm2 keys placements by `p=` alone, so two images
placed as `p=1` replaced each other.

### Changed

**Graphs are laid out in text columns.** A `LayeredLayout` now places nodes
in layout units — one unit is one text column — and takes the `labels` (or
explicit sizes) its boxes will be drawn with, plus `node_sep`/`rank_sep`, so
box widths are part of the layout and no two boxes touch. A graph frame draws
at that scale instead of stretching the layout to fill the pane: it picks the
largest text scale at which the graph fits, shrinks only until boxes would
touch, and past that overflows for the user to pan. Long edges that share an
endpoint are bundled into one trunk with the other ends joining it, rather
than one wire per edge stacked into a comb, and boxes line up under the boxes
they are wired to.

**A plain drag pans a 2D plot.** The input map's yaw/pitch controls have
nothing to turn on a flat picture, so they pan there; shift-drag is unchanged.

The C ABI's `plotui_layered_layout_new` grew `labels`, `n_labels`, `node_sep`
and `rank_sep` parameters; the Go, Python and JavaScript signatures gained
the same as optional arguments.

**Direct-mode frames no longer flicker.** The Textual widget's direct path
(iTerm2, WezTerm, Konsole) used to delete the previous image before
transmitting the next, which blanked the plot until the terminal had decoded
the new frame — a flash on every interactive repaint. Frames now alternate
between two image ids: the new frame is placed first and only then is the
other id's placement deleted (`kitty_compat_swap`, `render_kitty(image_id=,
retire_id=)`). `PlotWidget.kitty_cleanup()` deletes both buffers; hosts that
emitted `Plot.kitty_cleanup()` when covering a plot should use it.

**The crosshair repaints only when its picture changes.** `set_hover2d(px,
px_w, px_h)` snaps the cursor the way the renderer will and reports a change
only when the snapped sample differs, so a mouse move inside one sample's
basin costs nothing instead of a full re-rasterize, upload and decode.
`Plot.hover2d_snap_px` exposes the same query. The readout keeps three
significant digits below 1 (`0.0156`, not `0.02`).

### Added

**Labels and borders on 3D graph nodes, and a star.** `set_graph_labels`
draws a string inside each mark — centred, at the frame's text scale, in an
ink that contrasts with the fill (or one colour for all) — steps the text
down to the largest scale the mark can hold, and skips any label that fits
at none, so a dense graph grows its numbers as the host zooms in. `set_graph_borders` strokes an outline ring around each node, a
second category next to the fill. `"star"` joins the node shapes, and
`mark_scale(px_w)` exposes the factor radii are drawn at so a host can size
marks to a pixel budget (nodes that never overlap at the current zoom).
Rust, Python.

**`legend_visible`.** A plot can name its traces without drawing the
in-canvas legend box, so a host that draws its own legend still gets series
names in the crosshair readout instead of `series 3`.

**Draggable nodes.** A drag that starts on a 2D graph node's box moves the
node, its edges following, and the rest of the graph stays still. On by
default in every frontend: Textual posts `NodeMoved(index, x, y)`, Ratatui
returns `PlotEvent::NodeMoved`, Bubble Tea sends `NodeMovedMsg`, and the
primitive behind them, `Plot::drag_node(px_w, px_h, index, dx_px, dy_px)`,
is on every binding.

## 0.5.1 — 2026-09-28

### Added

**Windows wheels.** `pip install plotui` on Windows (x86_64) gets a prebuilt
wheel instead of a source build that needs a Rust toolchain. The wheel bundles
the CLI as `plotui/_bin/plotui.exe`, and the `plotui` console script runs it as
a child process, since Windows has no `exec`. CI builds and tests the wheel on
windows-latest.

## 0.5.0 — 2026-09-03

The first release with the 2D chart set, DAG rendering, and live feeds. Every
feature below reaches Python, Rust (Ratatui), Go (Bubble Tea), the C ABI, the
browser (WASM), and the CLI, with argument validation and error strings shared
so the frontends cannot drift.

### Added

**2D charts.** Step, histogram, box, heatmap, and band traces join scatter,
line and bar, with grouped and stacked bar modes, error bars, per-point color,
size and marker shape, categorical axes, and a colorbar. Chart text is set in
Martian Mono.

**Axis semantics.** Chart and axis titles — the y title drawn rotated in the
left margin — plus explicit `x`/`y` ranges and log₁₀ scales. A range pins the
extent only, so zoom and pan still compose on top of it; a log axis ticks in
powers of ten and simply does not place values at or below zero.

**DAG and pipeline rendering.** Directed graphs as labelled boxes wired by
arrows, laid out by rank with a Sugiyama layout (cycle removal, longest-path
ranking, dummy nodes, barycenter sweeps, priority coordinate passes). Nodes
pick and hover, colors repaint live so a running pipeline shows its state, and
`reachable` lights everything a task waits on. A DOT subset reads straight in.

**Live feeds.** `plotui --follow` keeps reading rows after the first frame and
appends them in place, so pan, zoom and the range slider survive an update.
`--window <N>` and `--last <span>` keep the view on the newest data, drawn on
the range slider against the whole run; a drag hands the view over and `f`
takes it back.

**Time series.** A range-slider strip with draggable handles, an explicit x
window that autoscales y to what is inside it, and calendar axes from ISO-8601
or `datetime` x columns.

**3D.** `Mesh3d` triangle meshes with a marching-cubes utility, and a
force-directed 3D graph layout.

**CLI.** `dag`, `step`, `hist`, `box`, `--title`, `--x-title`, `--y-title`,
`--x-range`, `--y-range`, `--log-x`, `--log-y`, `--follow`, `--window`,
`--last`, `--range-slider`, and `--out` for exporting a frame or recording an
animation (`.png`, `.mp4`, `.gif`, `.webm`). New example scenes: `pipeline`,
`aizawa`, `mandelbulb`, `lidar`, `protein`.

### Changed

- The website runs every demo on the real engine compiled to WASM, rather than
  on a reimplementation, and gained a 2D gallery and a DAG card.
- The Go bindings are built and tested in CI.

### Fixed

- `plotui.__version__` reported `0.3.0` against 0.4.x wheels. It now comes from
  the native module, which takes it from the crate, so there is one version in
  the build and nothing left to drift.
