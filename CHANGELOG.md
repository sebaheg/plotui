# Changelog

Notable changes per release. Versions before 0.5.0 predate this file; their
history is in the git log and the GitHub releases.

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
