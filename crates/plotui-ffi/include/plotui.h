#ifndef PLOTUI_H
#define PLOTUI_H

/* Generated from crates/plotui-ffi by cbindgen — do not edit. Regenerate with: PLOTUI_REGEN_HEADER=1 cargo test -p plotui-ffi header_is_fresh */

#include <stdarg.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>

#define PLOTUI_OK 0

#define PLOTUI_ERR_INVALID_ARG 1

#define PLOTUI_ERR_UNKNOWN_HANDLE 2

#define PLOTUI_ERR_STRUCTURAL 3

#define PLOTUI_ERR_NULL 4

#define PLOTUI_RENDER_PLACEHOLDER 0

#define PLOTUI_RENDER_DIRECT 1

#define PLOTUI_RENDER_UNSUPPORTED 2

/**
 * An opaque 3D force-directed layout (`plotui_core::ForceLayout`): pure
 * math on the host's timer, deterministic for a given seed. Not
 * thread-safe: one thread at a time, like `PlotuiPlot`.
 */
typedef struct PlotuiLayout PlotuiLayout;

/**
 * An opaque plot handle: data + camera + this plot's Kitty image id.
 */
typedef struct PlotuiPlot PlotuiPlot;

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

/**
 * A fresh plot with its own Kitty image id (the first plot in a process
 * gets the protocol's default id, 4242).
 */
struct PlotuiPlot *plotui_new(void);

/**
 * Free a plot. NULL is a no-op.
 *
 * # Safety
 * `p` must be a pointer from `plotui_new` not yet freed.
 */
void plotui_free(struct PlotuiPlot *p);

/**
 * The message for the last failing call on this thread ("" when none).
 * Borrowed: valid until the next failing call on the same thread.
 */
const char *plotui_last_error(void);

/**
 * Free a string returned by this library. NULL is a no-op.
 *
 * # Safety
 * `s` must be a string returned by this library, not yet freed.
 */
void plotui_string_free(char *s);

/**
 * Parse a color shorthand — "#rrggbb" (or bare "rrggbb") hex, or a name
 * like "red" — into 3 bytes at `out_rgb`. Stateless; the accepted names
 * and the error message are the shared `plotui-bind` rule.
 *
 * # Safety
 * `s` must be a NUL-terminated string; `out_rgb` must point at 3 writable
 * bytes.
 */
int32_t plotui_parse_color(const char *s, uint8_t *out_rgb);

/**
 * Swap the color sequence assigned to traces added without an explicit
 * color: `rgbs` is `3 * n` bytes of (r, g, b) triples; `n` must be at
 * least 1. Traces already added keep their colors.
 *
 * # Safety
 * Pointer arguments follow the crate conventions.
 */
int32_t plotui_set_colorway(struct PlotuiPlot *p, const uint8_t *rgbs, size_t n);

/**
 * Swap the color sequence to a built-in colorway by name: "plotui" (the
 * default), "muted", or "vivid".
 *
 * # Safety
 * Pointer arguments follow the crate conventions.
 */
int32_t plotui_set_colorway_name(struct PlotuiPlot *p, const char *name);

/**
 * # Safety
 * Pointer arguments follow the crate conventions (see the module docs).
 */
int32_t plotui_add_scatter3d(struct PlotuiPlot *p,
                             const float *xs,
                             size_t nx,
                             const float *ys,
                             size_t ny,
                             const float *zs,
                             size_t nz,
                             const uint8_t *rgb,
                             float size,
                             const char *name,
                             size_t *out_handle);

/**
 * # Safety
 * Pointer arguments follow the crate conventions. `edges` is `2 * n_edges`
 * u32s as (i, j) pairs; `node_rgbs`/`edge_rgbs` are `3 * n` byte triples;
 * `node_shapes` is an array of NUL-terminated shape names.
 */
int32_t plotui_add_graph3d(struct PlotuiPlot *p,
                           const float *xs,
                           size_t nx,
                           const float *ys,
                           size_t ny,
                           const float *zs,
                           size_t nz,
                           const uint32_t *edges,
                           size_t n_edges,
                           const uint8_t *node_rgbs,
                           size_t n_node_rgbs,
                           const uint8_t *rgb,
                           float size,
                           const float *node_sizes,
                           size_t n_node_sizes,
                           const uint8_t *edge_rgbs,
                           size_t n_edge_rgbs,
                           const char *const *node_shapes,
                           size_t n_shapes,
                           const char *name,
                           size_t *out_handle);

/**
 * # Safety
 * Pointer arguments follow the crate conventions.
 */
int32_t plotui_add_line3d(struct PlotuiPlot *p,
                          const float *xs,
                          size_t nx,
                          const float *ys,
                          size_t ny,
                          const float *zs,
                          size_t nz,
                          const uint8_t *rgb,
                          float width,
                          const char *name,
                          size_t *out_handle);

/**
 * `zs` is the flat row-major grid: `zs[j * nx + i]` = height at
 * `(xs[i], ys[j])`; `nz` must equal `nx * ny`. `colormap` NULL means a
 * solid color.
 *
 * # Safety
 * Pointer arguments follow the crate conventions.
 */
int32_t plotui_add_surface3d(struct PlotuiPlot *p,
                             const float *xs,
                             size_t nx,
                             const float *ys,
                             size_t ny,
                             const float *zs,
                             size_t nz,
                             const uint8_t *rgb,
                             const char *colormap,
                             bool wireframe,
                             const char *name,
                             size_t *out_handle);

/**
 * # Safety
 * Pointer arguments follow the crate conventions. `axis` is "y", "y2" or
 * "y3" (NULL = "y").
 */
int32_t plotui_add_scatter2d(struct PlotuiPlot *p,
                             const float *xs,
                             size_t nx,
                             const float *ys,
                             size_t ny,
                             const uint8_t *rgb,
                             float size,
                             const char *name,
                             const char *axis,
                             size_t *out_handle);

/**
 * # Safety
 * Pointer arguments follow the crate conventions.
 */
int32_t plotui_add_line2d(struct PlotuiPlot *p,
                          const float *xs,
                          size_t nx,
                          const float *ys,
                          size_t ny,
                          const uint8_t *rgb,
                          float width,
                          const char *name,
                          const char *axis,
                          size_t *out_handle);

/**
 * # Safety
 * Pointer arguments follow the crate conventions.
 */
int32_t plotui_add_bar2d(struct PlotuiPlot *p,
                         const float *xs,
                         size_t nx,
                         const float *heights,
                         size_t nh,
                         const uint8_t *rgb,
                         const char *name,
                         const char *axis,
                         size_t *out_handle);

/**
 * Append points to a trace: `(xs, ys)` for 2D traces (`zs` NULL), `(xs,
 * ys, zs)` for 3D scatter/line traces.
 *
 * # Safety
 * Pointer arguments follow the crate conventions.
 */
int32_t plotui_extend(struct PlotuiPlot *p,
                      size_t handle,
                      const float *xs,
                      size_t nx,
                      const float *ys,
                      size_t ny,
                      const float *zs,
                      size_t nz);

/**
 * Move every node of a graph trace at once — the per-frame call of a
 * force-directed layout (pair with the `plotui_layout_*` functions). The
 * point count (min of nx/ny/nz) must match the trace's node count;
 * structure, indices, hover, and selection stay valid.
 *
 * # Safety
 * Pointer arguments follow the crate conventions.
 */
int32_t plotui_set_graph_positions(struct PlotuiPlot *p,
                                   size_t handle,
                                   const float *xs,
                                   size_t nx,
                                   const float *ys,
                                   size_t ny,
                                   const float *zs,
                                   size_t nz);

/**
 * Recolor a graph trace in place — the host-side highlight primitive.
 * `node_rgbs` is `3 * n_nodes` bytes (one color per node); `edge_rgbs` is
 * `3 * n_edges` bytes or NULL (with n 0) to restore the default dimmed
 * endpoint blend.
 *
 * # Safety
 * Pointer arguments follow the crate conventions.
 */
int32_t plotui_set_graph_colors(struct PlotuiPlot *p,
                                size_t handle,
                                const uint8_t *node_rgbs,
                                size_t n_nodes,
                                const uint8_t *edge_rgbs,
                                size_t n_edges);

/**
 * Append nodes and edges to a graph trace (pair with
 * `plotui_layout_add_node`). `edges` is `2 * n_edges` u32s as (i, j) pairs
 * referencing old or new node indices; `node_rgbs` is `3 * n` byte triples
 * coloring the appended nodes (renderer default where missing). Appending
 * to a graph that is not the last node-bearing trace shifts downstream
 * flat node/edge indices, as with `plotui_extend` on a 3D scatter.
 *
 * # Safety
 * Pointer arguments follow the crate conventions.
 */
int32_t plotui_extend_graph(struct PlotuiPlot *p,
                            size_t handle,
                            const float *xs,
                            size_t nx,
                            const float *ys,
                            size_t ny,
                            const float *zs,
                            size_t nz,
                            const uint8_t *node_rgbs,
                            size_t n_node_rgbs,
                            const uint32_t *edges,
                            size_t n_edges);

/**
 * # Safety
 * Pointer arguments follow the crate conventions.
 */
int32_t plotui_set_visible(struct PlotuiPlot *p, size_t handle, bool visible, bool *out_changed);

/**
 * Select an element (`kind`: 0 none, 1 node, 2 edge).
 *
 * # Safety
 * `p` must be a live plot handle.
 */
int32_t plotui_set_selected(struct PlotuiPlot *p, int32_t kind, size_t index);

/**
 * Hover an element (`kind` as in `plotui_set_selected`); `out_changed`
 * tells the frontend whether a repaint is needed.
 *
 * # Safety
 * `p` must be a live plot handle.
 */
int32_t plotui_set_hovered(struct PlotuiPlot *p, int32_t kind, size_t index, bool *out_changed);

/**
 * Set the 2D crosshair position in framebuffer pixels (`has_px` false
 * clears it). Returns whether the state changed (repaint needed).
 *
 * # Safety
 * `p` must be a live plot handle.
 */
bool plotui_set_hover2d(struct PlotuiPlot *p, bool has_px, float px);

/**
 * Set the explicit 2D x window in data coordinates (`has` false clears it);
 * `out_changed` tells the frontend whether a repaint is needed.
 *
 * # Safety
 * `p` must be a live plot handle; `out_changed` may be NULL.
 */
int32_t plotui_set_x_window(struct PlotuiPlot *p,
                            bool has,
                            double lo,
                            double hi,
                            bool *out_changed);

/**
 * Read the current x window into `out_lo`/`out_hi`; returns whether one is
 * set (outputs untouched when not).
 *
 * # Safety
 * `p` must be a live plot handle; out pointers may be NULL.
 */
bool plotui_x_window(const struct PlotuiPlot *p, double *out_lo, double *out_hi);

/**
 * Toggle the range-slider strip. Returns whether the state changed
 * (repaint needed).
 *
 * # Safety
 * `p` must be a live plot handle.
 */
bool plotui_set_range_slider(struct PlotuiPlot *p, bool on);

/**
 * Set the time-axis epoch base, seconds UTC (`has` false clears it): x
 * values then mean seconds since this base, x ticks become calendar dates.
 * `out_changed` tells the frontend whether a repaint is needed.
 *
 * # Safety
 * `p` must be a live plot handle; `out_changed` may be NULL.
 */
int32_t plotui_set_x_epoch(struct PlotuiPlot *p, bool has, double epoch, bool *out_changed);

/**
 * Read the time-axis epoch base into `out_epoch`; returns whether one is
 * set.
 *
 * # Safety
 * `p` must be a live plot handle; `out_epoch` may be NULL.
 */
bool plotui_x_epoch(const struct PlotuiPlot *p, double *out_epoch);

/**
 * What the range-slider strip has under `(px, py)` framebuffer pixels at a
 * `w`×`h` frame, within `tol_px`: `out_part` is 0 none, 1 left handle,
 * 2 right handle, 3 window body, 4 track.
 *
 * # Safety
 * `p` must be a live plot handle; `out_part` may be NULL.
 */
int32_t plotui_range_slider_hit(const struct PlotuiPlot *p,
                                size_t px_w,
                                size_t px_h,
                                float px,
                                float py,
                                float tol_px,
                                int32_t *out_part);

/**
 * Drag the grabbed strip `part` (1 left, 2 right, 3 window, 4 track) by
 * `dx_px` framebuffer pixels; `out_changed` tells the frontend whether a
 * repaint is needed.
 *
 * # Safety
 * `p` must be a live plot handle; `out_changed` may be NULL.
 */
int32_t plotui_drag_x_window(struct PlotuiPlot *p,
                             size_t px_w,
                             size_t px_h,
                             int32_t part,
                             float dx_px,
                             bool *out_changed);

/**
 * Center the window on the strip position under `px` framebuffer pixels (a
 * track click). Returns whether the window changed (repaint needed).
 *
 * # Safety
 * `p` must be a live plot handle.
 */
bool plotui_jump_x_window(struct PlotuiPlot *p, size_t px_w, size_t px_h, float px);

/**
 * Slide a set window by a plot-area drag of `dx_px` framebuffer pixels.
 * Returns whether the window changed (repaint needed).
 *
 * # Safety
 * `p` must be a live plot handle.
 */
bool plotui_pan_x_window(struct PlotuiPlot *p, size_t px_w, size_t px_h, float dx_px);

/**
 * Zoom the window about the data x under `px` framebuffer pixels
 * (`factor > 1` zooms in). Returns whether the window changed (repaint
 * needed).
 *
 * # Safety
 * `p` must be a live plot handle.
 */
bool plotui_zoom_x_window(struct PlotuiPlot *p, size_t px_w, size_t px_h, float px, double factor);

/**
 * Slide a set window by `frac` of its own span (positive = later x) — the
 * keyboard step, needing no pixel geometry. Returns whether the window
 * changed (repaint needed).
 *
 * # Safety
 * `p` must be a live plot handle.
 */
bool plotui_shift_x_window(struct PlotuiPlot *p, double frac);

/**
 * Pick under `(px, py)`: nearest node within `node_radius`, else nearest
 * graph edge within `edge_radius` (negative = the default,
 * 0.75 × node_radius). `out_kind` is 0/1/2 as in `plotui_set_selected`.
 *
 * # Safety
 * `p` must be a live plot handle; out pointers may be NULL.
 */
int32_t plotui_pick_element_px(const struct PlotuiPlot *p,
                               size_t px_w,
                               size_t px_h,
                               float px,
                               float py,
                               float node_radius,
                               float edge_radius,
                               int32_t *out_kind,
                               size_t *out_index);

/**
 * Nearest node within `radius` of `(px, py)`, nodes only. Returns whether
 * one was found; the index lands in `out_index`.
 *
 * # Safety
 * `p` must be a live plot handle; `out_index` may be NULL.
 */
bool plotui_pick_px(const struct PlotuiPlot *p,
                    size_t px_w,
                    size_t px_h,
                    float px,
                    float py,
                    float radius,
                    size_t *out_index);

/**
 * # Safety
 * `p` must be a live plot handle.
 */
void plotui_rotate(struct PlotuiPlot *p, double d_yaw, double d_pitch);

/**
 * # Safety
 * `p` must be a live plot handle.
 */
void plotui_zoom_by(struct PlotuiPlot *p, double factor);

/**
 * Pan in framebuffer pixels.
 *
 * # Safety
 * `p` must be a live plot handle.
 */
void plotui_pan(struct PlotuiPlot *p, double dx, double dy);

/**
 * # Safety
 * `p` must be a live plot handle.
 */
void plotui_reset(struct PlotuiPlot *p);

/**
 * Remap what drag gestures do. Each name is a camera control — "yaw",
 * "pitch", "pan_x", "pan_y", "zoom" or "off" — or NULL to keep that
 * axis's current binding. The default map is drag = rotate (yaw/pitch),
 * shift-drag = pan. Returns 0, or -1 with `plotui_last_error()` set.
 *
 * # Safety
 * `p` must be a live plot handle; names must be NULL or valid C strings.
 */
int32_t plotui_set_input_map(struct PlotuiPlot *p,
                             const char *drag_x,
                             const char *drag_y,
                             const char *shift_drag_x,
                             const char *shift_drag_y);

/**
 * Route a drag through the input map (see `plotui_set_input_map`):
 * `(dx, dy)` pointer deltas in whatever unit the scales are calibrated
 * for — `rotate_scale` radians per unit, `pan_*_scale` framebuffer pixels
 * per unit, `zoom_scale` log-zoom per unit. `shift` nonzero selects the
 * shift-drag bindings.
 *
 * # Safety
 * `p` must be a live plot handle.
 */
void plotui_apply_drag(struct PlotuiPlot *p,
                       double dx,
                       double dy,
                       int32_t shift,
                       double rotate_scale,
                       double pan_x_scale,
                       double pan_y_scale,
                       double zoom_scale);

/**
 * Write the camera state (yaw, pitch, zoom, pan_x, pan_y) into `out[5]`.
 *
 * # Safety
 * `p` must be a live plot handle; `out` must hold 5 doubles.
 */
void plotui_camera_state(const struct PlotuiPlot *p, double *out);

/**
 * # Safety
 * `p` must be a live plot handle.
 */
void plotui_set_camera_state(struct PlotuiPlot *p,
                             double yaw,
                             double pitch,
                             double zoom,
                             double pan_x,
                             double pan_y);

/**
 * Pin the 3D data frame to `(lo, hi)` corners (each NULL or 3 floats);
 * pass both NULL to restore auto-fit. Like the Python binding, anything
 * short of two corners means auto-fit.
 *
 * # Safety
 * `p` must be a live plot handle.
 */
int32_t plotui_set_bounds(struct PlotuiPlot *p, const float *lo, const float *hi);

/**
 * # Safety
 * `p` must be a live plot handle.
 */
void plotui_set_show_box(struct PlotuiPlot *p, bool show);

/**
 * Recolour the non-data chrome; each pointer is NULL (keep) or 3 RGB bytes.
 *
 * # Safety
 * `p` must be a live plot handle.
 */
void plotui_set_chrome(struct PlotuiPlot *p,
                       const uint8_t *bg,
                       const uint8_t *frame,
                       const uint8_t *grid,
                       const uint8_t *ink,
                       const uint8_t *ink_bright);

/**
 * # Safety
 * `p` must be a live plot handle.
 */
bool plotui_is_3d(const struct PlotuiPlot *p);

/**
 * # Safety
 * `p` must be a live plot handle.
 */
size_t plotui_node_count(const struct PlotuiPlot *p);

/**
 * # Safety
 * `p` must be a live plot handle.
 */
size_t plotui_vertex_count(const struct PlotuiPlot *p);

/**
 * This plot's Kitty image id.
 *
 * # Safety
 * `p` must be a live plot handle.
 */
uint32_t plotui_image_id(const struct PlotuiPlot *p);

/**
 * Project every node to screen space for a `px_w`×`px_h` framebuffer:
 * writes `plotui_node_count(p) * 3` floats — (x_px, y_px, depth) per node,
 * flat-index order.
 *
 * # Safety
 * `out` must hold `plotui_node_count(p) * 3` floats.
 */
int32_t plotui_project_nodes(const struct PlotuiPlot *p, size_t px_w, size_t px_h, float *out);

/**
 * Render as a Kitty escape for `cols`×`rows` cells of `cell_w`×`cell_h`
 * pixels, using this plot's image id. Same contract as the Python
 * binding's `render_kitty` (`compat_chunks` for the iTerm2 tier, `scale`
 * for reduced-resolution interaction frames, `replace` to skip the
 * delete-before-transmit). Free the string with `plotui_string_free`.
 *
 * # Safety
 * `p` must be a live plot handle; `out_escape` must be a valid pointer.
 */
int32_t plotui_render_kitty(const struct PlotuiPlot *p,
                            uint16_t cols,
                            uint16_t rows,
                            uint16_t cell_w,
                            uint16_t cell_h,
                            bool compat_chunks,
                            double scale,
                            bool replace,
                            char **out_escape);

/**
 * Render one frame and write the raw RGBA8 pixels: `px_w * px_h * 4`
 * bytes, row-major, alpha 0 for undrawn pixels.
 *
 * # Safety
 * `out` must hold `px_w * px_h * 4` bytes.
 */
int32_t plotui_render_rgba(const struct PlotuiPlot *p, size_t px_w, size_t px_h, uint8_t *out);

/**
 * Render a Kitty Unicode-placeholder frame and return its *metadata*: the
 * transmit escape (free with `plotui_string_free`), the placeholder
 * foreground color in `out_id_rgb[3]`, and the high id byte in
 * `out_extra`. The caller synthesizes each cell as U+10EEEE + the row
 * diacritic + the column diacritic (+ the extra-byte diacritic when
 * `out_extra` is nonzero) from `plotui_diacritics` — identical to what
 * `kitty_placeholder_cells` would return, without marshaling cols×rows
 * strings per frame.
 *
 * # Safety
 * `p` must be a live plot handle; out pointers must be valid
 * (`out_id_rgb` holds 3 bytes).
 */
int32_t plotui_render_kitty_placeholder_meta(const struct PlotuiPlot *p,
                                             uint16_t cols,
                                             uint16_t rows,
                                             uint16_t cell_w,
                                             uint16_t cell_h,
                                             double scale,
                                             char **out_transmit,
                                             uint8_t *out_id_rgb,
                                             uint8_t *out_extra);

/**
 * The escape that deletes this plot's image from the terminal (emit on
 * exit). Free with `plotui_string_free`.
 *
 * # Safety
 * `p` must be a live plot handle.
 */
char *plotui_kitty_cleanup(const struct PlotuiPlot *p);

/**
 * The spec-defined placeholder diacritic table: `out_len` codepoints,
 * `table[n]` encoding row/column index `n`. Static — never free it.
 *
 * # Safety
 * `out_len` must be a valid pointer or NULL.
 */
const uint32_t *plotui_diacritics(size_t *out_len);

/**
 * Best render path for this terminal (honors `PLOTUI_RENDER`): 0
 * placeholder, 1 direct, 2 unsupported.
 */
int32_t plotui_detect_render_mode(void);

/**
 * The terminal's device pixels per cell via ioctl. Returns false (and
 * leaves the outputs alone) when no stream reports a pixel size — use the
 * 12×24 fallback then.
 *
 * # Safety
 * Out pointers must be valid or NULL.
 */
bool plotui_detect_cell_px(uint16_t *out_w, uint16_t *out_h);

/**
 * True when `PLOTUI_KITTY_REPLACE` asks direct mode to skip the
 * delete-before-transmit.
 */
bool plotui_kitty_replace_env(void);

/**
 * Wrap an escape for tmux passthrough when `$TMUX` is set (otherwise a
 * plain copy). Free with `plotui_string_free`.
 *
 * # Safety
 * `escape` must be a NUL-terminated string.
 */
char *plotui_tmux_wrap(const char *escape);

/**
 * Resolution multiplier for the next frame: `configured_scale` only for
 * large 3D plots (≥ `PLOTUI_LARGE_VERTEX_COUNT` vertices) while
 * `interacting`, else 1.0 — the shared half-resolution policy.
 *
 * # Safety
 * `p` must be a live plot handle.
 */
double plotui_interactive_scale(const struct PlotuiPlot *p,
                                bool interacting,
                                double configured_scale);

/**
 * A layout over `n_nodes` with seeded initial positions in the unit ball.
 * `edges` is `2 * n_edges` u32s as (i, j) index pairs. Free with
 * `plotui_layout_free`. Returns NULL only on a malformed edge slice.
 *
 * # Safety
 * Pointer arguments follow the crate conventions.
 */
struct PlotuiLayout *plotui_layout_new(size_t n_nodes,
                                       const uint32_t *edges,
                                       size_t n_edges,
                                       uint32_t seed);

/**
 * Free a layout. NULL is a no-op.
 *
 * # Safety
 * `l` must be a pointer from `plotui_layout_new` not yet freed.
 */
void plotui_layout_free(struct PlotuiLayout *l);

/**
 * One simulation tick; returns the mean displacement ("energy") — hosts
 * stop repainting once it drops below ~1e-3. A NULL layout returns 0.
 *
 * # Safety
 * `l` must be a live layout handle or NULL.
 */
float plotui_layout_step(struct PlotuiLayout *l);

/**
 * The layout's node count (grows with `plotui_layout_add_node`) — the
 * `out` size contract for `plotui_layout_positions`.
 *
 * # Safety
 * `l` must be a live layout handle or NULL (which counts 0).
 */
size_t plotui_layout_node_count(const struct PlotuiLayout *l);

/**
 * Write the current positions as flat `[x0, y0, z0, x1, …]` into `out`,
 * which must hold `plotui_layout_node_count(l) * 3` floats — feed them to
 * `plotui_set_graph_positions`.
 *
 * # Safety
 * `l` must be a live layout handle; `out` must point at enough floats.
 */
int32_t plotui_layout_positions(const struct PlotuiLayout *l, float *out);

/**
 * Warm insertion of one node connected to `neighbors` (existing indices,
 * `n_neighbors` u32s): it spawns beside its first neighbor and re-heats
 * the simulation. Writes the new node's index to `out_index`; pair with
 * `plotui_extend_graph`.
 *
 * # Safety
 * Pointer arguments follow the crate conventions.
 */
int32_t plotui_layout_add_node(struct PlotuiLayout *l,
                               const uint32_t *neighbors,
                               size_t n_neighbors,
                               size_t *out_index);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* PLOTUI_H */
