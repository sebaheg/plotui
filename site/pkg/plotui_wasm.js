/**
 * A `pick_element` hit: a node or an edge, by flat index (nodes across all
 * 3D traces in insertion order; edges across graph traces).
 */
export class PickHit {
    static __wrap(ptr) {
        const obj = Object.create(PickHit.prototype);
        obj.__wbg_ptr = ptr;
        PickHitFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        PickHitFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_pickhit_free(ptr, 0);
    }
    /**
     * @returns {number}
     */
    get index() {
        const ret = wasm.__wbg_get_pickhit_index(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {boolean}
     */
    get is_edge() {
        const ret = wasm.__wbg_get_pickhit_is_edge(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * @param {number} arg0
     */
    set index(arg0) {
        wasm.__wbg_set_pickhit_index(this.__wbg_ptr, arg0);
    }
    /**
     * @param {boolean} arg0
     */
    set is_edge(arg0) {
        wasm.__wbg_set_pickhit_is_edge(this.__wbg_ptr, arg0);
    }
}
if (Symbol.dispose) PickHit.prototype[Symbol.dispose] = PickHit.prototype.free;

/**
 * A plot handle: data + camera + last rendered frame. Held by the JS
 * frontend for a plot's life.
 */
export class Plot {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        PlotFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_plot_free(ptr, 0);
    }
    /**
     * Add a 2D bar series.
     * @param {Float32Array} xs
     * @param {Float32Array} heights
     * @param {string | null} [color]
     * @param {string | null} [name]
     * @param {string | null} [axis]
     * @returns {number}
     */
    add_bar2d(xs, heights, color, name, axis) {
        const ptr0 = passArrayF32ToWasm0(xs, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passArrayF32ToWasm0(heights, wasm.__wbindgen_malloc);
        const len1 = WASM_VECTOR_LEN;
        var ptr2 = isLikeNone(color) ? 0 : passStringToWasm0(color, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        var len2 = WASM_VECTOR_LEN;
        var ptr3 = isLikeNone(name) ? 0 : passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        var len3 = WASM_VECTOR_LEN;
        var ptr4 = isLikeNone(axis) ? 0 : passStringToWasm0(axis, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        var len4 = WASM_VECTOR_LEN;
        const ret = wasm.plot_add_bar2d(this.__wbg_ptr, ptr0, len0, ptr1, len1, ptr2, len2, ptr3, len3, ptr4, len4);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * Add a 3D graph: nodes at `xs/ys/zs`, `edges` as flat index pairs
     * `[a0, b0, a1, b1, …]`, a uniform node `color`, and marker `size`.
     * @param {Float32Array} xs
     * @param {Float32Array} ys
     * @param {Float32Array} zs
     * @param {Uint32Array} edges
     * @param {string | null} [color]
     * @param {number | null} [size]
     * @returns {number}
     */
    add_graph3d(xs, ys, zs, edges, color, size) {
        const ptr0 = passArrayF32ToWasm0(xs, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passArrayF32ToWasm0(ys, wasm.__wbindgen_malloc);
        const len1 = WASM_VECTOR_LEN;
        const ptr2 = passArrayF32ToWasm0(zs, wasm.__wbindgen_malloc);
        const len2 = WASM_VECTOR_LEN;
        const ptr3 = passArray32ToWasm0(edges, wasm.__wbindgen_malloc);
        const len3 = WASM_VECTOR_LEN;
        var ptr4 = isLikeNone(color) ? 0 : passStringToWasm0(color, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        var len4 = WASM_VECTOR_LEN;
        const ret = wasm.plot_add_graph3d(this.__wbg_ptr, ptr0, len0, ptr1, len1, ptr2, len2, ptr3, len3, ptr4, len4, isLikeNone(size) ? Number.MAX_SAFE_INTEGER : Math.fround(size));
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * Add a 2D line series.
     * @param {Float32Array} xs
     * @param {Float32Array} ys
     * @param {string | null} [color]
     * @param {number | null} [width]
     * @param {string | null} [name]
     * @param {string | null} [axis]
     * @returns {number}
     */
    add_line2d(xs, ys, color, width, name, axis) {
        const ptr0 = passArrayF32ToWasm0(xs, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passArrayF32ToWasm0(ys, wasm.__wbindgen_malloc);
        const len1 = WASM_VECTOR_LEN;
        var ptr2 = isLikeNone(color) ? 0 : passStringToWasm0(color, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        var len2 = WASM_VECTOR_LEN;
        var ptr3 = isLikeNone(name) ? 0 : passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        var len3 = WASM_VECTOR_LEN;
        var ptr4 = isLikeNone(axis) ? 0 : passStringToWasm0(axis, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        var len4 = WASM_VECTOR_LEN;
        const ret = wasm.plot_add_line2d(this.__wbg_ptr, ptr0, len0, ptr1, len1, ptr2, len2, isLikeNone(width) ? Number.MAX_SAFE_INTEGER : Math.fround(width), ptr3, len3, ptr4, len4);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * Add a 3D polyline. `name` puts it in the legend.
     * @param {Float32Array} xs
     * @param {Float32Array} ys
     * @param {Float32Array} zs
     * @param {string | null} [color]
     * @param {number | null} [width]
     * @param {string | null} [name]
     * @returns {number}
     */
    add_line3d(xs, ys, zs, color, width, name) {
        const ptr0 = passArrayF32ToWasm0(xs, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passArrayF32ToWasm0(ys, wasm.__wbindgen_malloc);
        const len1 = WASM_VECTOR_LEN;
        const ptr2 = passArrayF32ToWasm0(zs, wasm.__wbindgen_malloc);
        const len2 = WASM_VECTOR_LEN;
        var ptr3 = isLikeNone(color) ? 0 : passStringToWasm0(color, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        var len3 = WASM_VECTOR_LEN;
        var ptr4 = isLikeNone(name) ? 0 : passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        var len4 = WASM_VECTOR_LEN;
        const ret = wasm.plot_add_line3d(this.__wbg_ptr, ptr0, len0, ptr1, len1, ptr2, len2, ptr3, len3, isLikeNone(width) ? Number.MAX_SAFE_INTEGER : Math.fround(width), ptr4, len4);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * Add a 2D scatter series on `axis` "y" (default), "y2" or "y3".
     * @param {Float32Array} xs
     * @param {Float32Array} ys
     * @param {string | null} [color]
     * @param {number | null} [size]
     * @param {string | null} [name]
     * @param {string | null} [axis]
     * @returns {number}
     */
    add_scatter2d(xs, ys, color, size, name, axis) {
        const ptr0 = passArrayF32ToWasm0(xs, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passArrayF32ToWasm0(ys, wasm.__wbindgen_malloc);
        const len1 = WASM_VECTOR_LEN;
        var ptr2 = isLikeNone(color) ? 0 : passStringToWasm0(color, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        var len2 = WASM_VECTOR_LEN;
        var ptr3 = isLikeNone(name) ? 0 : passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        var len3 = WASM_VECTOR_LEN;
        var ptr4 = isLikeNone(axis) ? 0 : passStringToWasm0(axis, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        var len4 = WASM_VECTOR_LEN;
        const ret = wasm.plot_add_scatter2d(this.__wbg_ptr, ptr0, len0, ptr1, len1, ptr2, len2, isLikeNone(size) ? Number.MAX_SAFE_INTEGER : Math.fround(size), ptr3, len3, ptr4, len4);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * Add a 3D scatter series; returns the trace handle for
     * `extend_xyz`/`set_visible`.
     * @param {Float32Array} xs
     * @param {Float32Array} ys
     * @param {Float32Array} zs
     * @param {string | null} [color]
     * @param {number | null} [size]
     * @returns {number}
     */
    add_scatter3d(xs, ys, zs, color, size) {
        const ptr0 = passArrayF32ToWasm0(xs, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passArrayF32ToWasm0(ys, wasm.__wbindgen_malloc);
        const len1 = WASM_VECTOR_LEN;
        const ptr2 = passArrayF32ToWasm0(zs, wasm.__wbindgen_malloc);
        const len2 = WASM_VECTOR_LEN;
        var ptr3 = isLikeNone(color) ? 0 : passStringToWasm0(color, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        var len3 = WASM_VECTOR_LEN;
        const ret = wasm.plot_add_scatter3d(this.__wbg_ptr, ptr0, len0, ptr1, len1, ptr2, len2, ptr3, len3, isLikeNone(size) ? Number.MAX_SAFE_INTEGER : Math.fround(size));
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * Add a 3D surface over the grid `(xs[i], ys[j])` with flat heights
     * `zs[j * xs.len() + i]`. Colormapped ("viridis" by default, or
     * "plasma"), or solid when a `color` is given without a `colormap`.
     * @param {Float32Array} xs
     * @param {Float32Array} ys
     * @param {Float32Array} zs
     * @param {string | null} [color]
     * @param {string | null} [colormap]
     * @param {boolean | null} [wireframe]
     * @param {string | null} [name]
     * @returns {number}
     */
    add_surface3d(xs, ys, zs, color, colormap, wireframe, name) {
        const ptr0 = passArrayF32ToWasm0(xs, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passArrayF32ToWasm0(ys, wasm.__wbindgen_malloc);
        const len1 = WASM_VECTOR_LEN;
        const ptr2 = passArrayF32ToWasm0(zs, wasm.__wbindgen_malloc);
        const len2 = WASM_VECTOR_LEN;
        var ptr3 = isLikeNone(color) ? 0 : passStringToWasm0(color, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        var len3 = WASM_VECTOR_LEN;
        var ptr4 = isLikeNone(colormap) ? 0 : passStringToWasm0(colormap, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        var len4 = WASM_VECTOR_LEN;
        var ptr5 = isLikeNone(name) ? 0 : passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        var len5 = WASM_VECTOR_LEN;
        const ret = wasm.plot_add_surface3d(this.__wbg_ptr, ptr0, len0, ptr1, len1, ptr2, len2, ptr3, len3, ptr4, len4, isLikeNone(wireframe) ? 0xFFFFFF : wireframe ? 1 : 0, ptr5, len5);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * `[yaw, pitch, zoom, pan_x, pan_y]` — pass back to `set_camera_state`.
     * @returns {Float64Array}
     */
    camera_state() {
        const ret = wasm.plot_camera_state(this.__wbg_ptr);
        var v1 = getArrayF64FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 8, 8);
        return v1;
    }
    /**
     * Append points to a 2D trace by handle.
     * @param {number} handle
     * @param {Float32Array} xs
     * @param {Float32Array} ys
     */
    extend_xy(handle, xs, ys) {
        const ptr0 = passArrayF32ToWasm0(xs, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passArrayF32ToWasm0(ys, wasm.__wbindgen_malloc);
        const len1 = WASM_VECTOR_LEN;
        const ret = wasm.plot_extend_xy(this.__wbg_ptr, handle, ptr0, len0, ptr1, len1);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * Append points to a 3D scatter/line trace by handle.
     * @param {number} handle
     * @param {Float32Array} xs
     * @param {Float32Array} ys
     * @param {Float32Array} zs
     */
    extend_xyz(handle, xs, ys, zs) {
        const ptr0 = passArrayF32ToWasm0(xs, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passArrayF32ToWasm0(ys, wasm.__wbindgen_malloc);
        const len1 = WASM_VECTOR_LEN;
        const ptr2 = passArrayF32ToWasm0(zs, wasm.__wbindgen_malloc);
        const len2 = WASM_VECTOR_LEN;
        const ret = wasm.plot_extend_xyz(this.__wbg_ptr, handle, ptr0, len0, ptr1, len1, ptr2, len2);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @returns {number}
     */
    frame_len() {
        const ret = wasm.plot_frame_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    frame_ptr() {
        const ret = wasm.plot_frame_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {boolean}
     */
    is_3d() {
        const ret = wasm.plot_is_3d(this.__wbg_ptr);
        return ret !== 0;
    }
    constructor() {
        const ret = wasm.plot_new();
        this.__wbg_ptr = ret;
        PlotFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @returns {number}
     */
    node_count() {
        const ret = wasm.plot_node_count(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Pan by a screen-pixel delta (full-resolution framebuffer pixels).
     * @param {number} dx
     * @param {number} dy
     */
    pan(dx, dy) {
        wasm.plot_pan(this.__wbg_ptr, dx, dy);
    }
    /**
     * The 3D node under `(px, py)` framebuffer pixels, within `radius`.
     * Picks always use full-resolution geometry regardless of `render_at`.
     * @param {number} w
     * @param {number} h
     * @param {number} px
     * @param {number} py
     * @param {number} radius
     * @returns {number | undefined}
     */
    pick(w, h, px, py, radius) {
        const ret = wasm.plot_pick(this.__wbg_ptr, w, h, px, py, radius);
        return ret === Number.MAX_SAFE_INTEGER ? undefined : ret;
    }
    /**
     * The node or edge under `(px, py)`, nodes first; edge radius defaults
     * to 0.75 × `node_radius`.
     * @param {number} w
     * @param {number} h
     * @param {number} px
     * @param {number} py
     * @param {number} node_radius
     * @param {number | null} [edge_radius]
     * @returns {PickHit | undefined}
     */
    pick_element(w, h, px, py, node_radius, edge_radius) {
        const ret = wasm.plot_pick_element(this.__wbg_ptr, w, h, px, py, node_radius, isLikeNone(edge_radius) ? Number.MAX_SAFE_INTEGER : Math.fround(edge_radius));
        return ret === 0 ? undefined : PickHit.__wrap(ret);
    }
    /**
     * Projected node positions as flat `[x_px, y_px, depth]` triples, in
     * the same flat order `pick` uses.
     * @param {number} w
     * @param {number} h
     * @returns {Float32Array}
     */
    project_nodes(w, h) {
        const ret = wasm.plot_project_nodes(this.__wbg_ptr, w, h);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Rasterize at `w`×`h` device pixels into the internal frame buffer;
     * read it with a fresh `Uint8ClampedArray(memory.buffer, frame_ptr(),
     * frame_len())` per blit.
     * @param {number} w
     * @param {number} h
     */
    render(w, h) {
        wasm.plot_render(this.__wbg_ptr, w, h);
    }
    /**
     * Reduced-resolution render for interaction: `pan_scale` = rendered
     * width / full-resolution width, so a panned view stays centered.
     * @param {number} w
     * @param {number} h
     * @param {number} pan_scale
     */
    render_at(w, h, pan_scale) {
        wasm.plot_render_at(this.__wbg_ptr, w, h, pan_scale);
    }
    reset() {
        wasm.plot_reset(this.__wbg_ptr);
    }
    /**
     * @param {number} d_yaw
     * @param {number} d_pitch
     */
    rotate(d_yaw, d_pitch) {
        wasm.plot_rotate(this.__wbg_ptr, d_yaw, d_pitch);
    }
    /**
     * @param {Float64Array} state
     */
    set_camera_state(state) {
        const ptr0 = passArrayF64ToWasm0(state, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.plot_set_camera_state(this.__wbg_ptr, ptr0, len0);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * 2D crosshair: the hovered x in framebuffer pixels (`None` clears);
     * core snaps to the nearest sample and draws the readout.
     * @param {number | null} [x_px]
     * @returns {boolean}
     */
    set_hover2d(x_px) {
        const ret = wasm.plot_set_hover2d(this.__wbg_ptr, isLikeNone(x_px) ? Number.MAX_SAFE_INTEGER : Math.fround(x_px));
        return ret !== 0;
    }
    /**
     * @param {number | null} [index]
     * @returns {boolean}
     */
    set_hovered_edge(index) {
        const ret = wasm.plot_set_hovered_edge(this.__wbg_ptr, isLikeNone(index) ? Number.MAX_SAFE_INTEGER : (index) >>> 0);
        return ret !== 0;
    }
    /**
     * Highlight a node as hovered (`None` clears); returns whether a
     * repaint is needed.
     * @param {number | null} [index]
     * @returns {boolean}
     */
    set_hovered_node(index) {
        const ret = wasm.plot_set_hovered_node(this.__wbg_ptr, isLikeNone(index) ? Number.MAX_SAFE_INTEGER : (index) >>> 0);
        return ret !== 0;
    }
    /**
     * Mark a node selected (drawn with a glow; `None` clears).
     * @param {number | null} [index]
     * @returns {boolean}
     */
    set_selected_node(index) {
        const ret = wasm.plot_set_selected_node(this.__wbg_ptr, isLikeNone(index) ? Number.MAX_SAFE_INTEGER : (index) >>> 0);
        return ret !== 0;
    }
    /**
     * Show or hide a trace; returns whether visibility changed.
     * @param {number} handle
     * @param {boolean} visible
     * @returns {boolean}
     */
    set_visible(handle, visible) {
        const ret = wasm.plot_set_visible(this.__wbg_ptr, handle, visible);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] !== 0;
    }
    /**
     * @returns {number}
     */
    vertex_count() {
        const ret = wasm.plot_vertex_count(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} f
     */
    zoom_by(f) {
        wasm.plot_zoom_by(this.__wbg_ptr, f);
    }
}
if (Symbol.dispose) Plot.prototype[Symbol.dispose] = Plot.prototype.free;
function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg_Error_408e67f47ca7b58b: function(arg0, arg1) {
            const ret = Error(getStringFromWasm0(arg0, arg1));
            return ret;
        },
        __wbg___wbindgen_throw_bb96b2010945f0bc: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbindgen_init_externref_table: function() {
            const table = wasm.__wbindgen_externrefs;
            const offset = table.grow(4);
            table.set(0, undefined);
            table.set(offset + 0, undefined);
            table.set(offset + 1, null);
            table.set(offset + 2, true);
            table.set(offset + 3, false);
        },
    };
    return {
        __proto__: null,
        "./plotui_wasm_bg.js": import0,
    };
}

const PickHitFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pickhit_free(ptr, 1));
const PlotFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_plot_free(ptr, 1));

function getArrayF32FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getFloat32ArrayMemory0().subarray(ptr / 4, ptr / 4 + len);
}

function getArrayF64FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getFloat64ArrayMemory0().subarray(ptr / 8, ptr / 8 + len);
}

let cachedFloat32ArrayMemory0 = null;
function getFloat32ArrayMemory0() {
    if (cachedFloat32ArrayMemory0 === null || cachedFloat32ArrayMemory0.byteLength === 0) {
        cachedFloat32ArrayMemory0 = new Float32Array(wasm.memory.buffer);
    }
    return cachedFloat32ArrayMemory0;
}

let cachedFloat64ArrayMemory0 = null;
function getFloat64ArrayMemory0() {
    if (cachedFloat64ArrayMemory0 === null || cachedFloat64ArrayMemory0.byteLength === 0) {
        cachedFloat64ArrayMemory0 = new Float64Array(wasm.memory.buffer);
    }
    return cachedFloat64ArrayMemory0;
}

function getStringFromWasm0(ptr, len) {
    return decodeText(ptr >>> 0, len);
}

let cachedUint32ArrayMemory0 = null;
function getUint32ArrayMemory0() {
    if (cachedUint32ArrayMemory0 === null || cachedUint32ArrayMemory0.byteLength === 0) {
        cachedUint32ArrayMemory0 = new Uint32Array(wasm.memory.buffer);
    }
    return cachedUint32ArrayMemory0;
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

function isLikeNone(x) {
    return x === undefined || x === null;
}

function passArray32ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 4, 4) >>> 0;
    getUint32ArrayMemory0().set(arg, ptr / 4);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passArrayF32ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 4, 4) >>> 0;
    getFloat32ArrayMemory0().set(arg, ptr / 4);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passArrayF64ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 8, 8) >>> 0;
    getFloat64ArrayMemory0().set(arg, ptr / 8);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passStringToWasm0(arg, malloc, realloc) {
    if (realloc === undefined) {
        const buf = cachedTextEncoder.encode(arg);
        const ptr = malloc(buf.length, 1) >>> 0;
        getUint8ArrayMemory0().subarray(ptr, ptr + buf.length).set(buf);
        WASM_VECTOR_LEN = buf.length;
        return ptr;
    }

    let len = arg.length;
    let ptr = malloc(len, 1) >>> 0;

    const mem = getUint8ArrayMemory0();

    let offset = 0;

    for (; offset < len; offset++) {
        const code = arg.charCodeAt(offset);
        if (code > 0x7F) break;
        mem[ptr + offset] = code;
    }
    if (offset !== len) {
        if (offset !== 0) {
            arg = arg.slice(offset);
        }
        ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
        const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len);
        const ret = cachedTextEncoder.encodeInto(arg, view);

        offset += ret.written;
        ptr = realloc(ptr, len, offset, 1) >>> 0;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
}

function takeFromExternrefTable0(idx) {
    const value = wasm.__wbindgen_externrefs.get(idx);
    wasm.__externref_table_dealloc(idx);
    return value;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
cachedTextDecoder.decode();
const MAX_SAFARI_DECODE_BYTES = 2146435072;
let numBytesDecoded = 0;
function decodeText(ptr, len) {
    numBytesDecoded += len;
    if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
        cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
        cachedTextDecoder.decode();
        numBytesDecoded = len;
    }
    return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

const cachedTextEncoder = new TextEncoder();

if (!('encodeInto' in cachedTextEncoder)) {
    cachedTextEncoder.encodeInto = function (arg, view) {
        const buf = cachedTextEncoder.encode(arg);
        view.set(buf);
        return {
            read: arg.length,
            written: buf.length
        };
    };
}

let WASM_VECTOR_LEN = 0;

let wasmModule, wasmInstance, wasm;
function __wbg_finalize_init(instance, module) {
    wasmInstance = instance;
    wasm = instance.exports;
    wasmModule = module;
    cachedFloat32ArrayMemory0 = null;
    cachedFloat64ArrayMemory0 = null;
    cachedUint32ArrayMemory0 = null;
    cachedUint8ArrayMemory0 = null;
    wasm.__wbindgen_start();
    return wasm;
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (!module.ok) {
            throw new Error(`failed to fetch Wasm: ${module.status} ${module.statusText} fetching '${module.url}'`);
        }

        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);
            } catch (e) {
                const validResponse = expectedResponseType(module.type);

                if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else { throw e; }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);
    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };
        } else {
            return instance;
        }
    }

    function expectedResponseType(type) {
        switch (type) {
            case 'basic': case 'cors': case 'default': return true;
        }
        return false;
    }
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (module !== undefined) {
        if (Object.getPrototypeOf(module) === Object.prototype) {
            ({module} = module)
        } else {
            console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
        }
    }

    const imports = __wbg_get_imports();
    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }
    const instance = new WebAssembly.Instance(module, imports);
    return __wbg_finalize_init(instance, module);
}

async function __wbg_init(module_or_path) {
    if (wasm !== undefined) return wasm;


    if (module_or_path !== undefined) {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }

    if (module_or_path === undefined) {
        module_or_path = new URL('plotui_wasm_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
