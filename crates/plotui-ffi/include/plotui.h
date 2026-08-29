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

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* PLOTUI_H */
