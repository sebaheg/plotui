// examples.js — mounts the interactive example cards on examples.html.
//
// Every card is rendered by plotui-core compiled to WebAssembly: pointer
// events drive the engine's own camera, hover uses its pick/crosshair APIs,
// and each frame is the engine's RGBA bytes blitted onto a canvas. The RGBA
// view into wasm memory is rebuilt at every blit, so a wasm memory growth
// can never leave a stale (detached) buffer behind.

// Loaded dynamically in boot() so a missing/failed wasm module still runs
// the fallback path (a static import would abort this whole module).
let Plot = null;
let ForceLayout = null;
let LayeredLayout = null;
let plot_reachable = null;
let marching_cubes = null;
let catmull_rom = null;
let tube = null;
let ribbon = null;
let memory = null;
// The only scene with an input file: 1UBQ's backbone, fetched in boot()
// beside the engine. Null if it failed, which degrades that one card.
let PDB = null;
const DPR_MAX = 2;
// Radians of yaw per frame for a scene that turns on its own. `spin` (not
// `rotate`) because it owns the direction: it turns the way a rightward
// drag pushes the object, so letting go of a grabbed plot does not send it
// back the way it came.
const SPIN_STEP = 0.004;
const T1 = '#ec4c86', T2 = '#45c8d1', T3 = '#f0a13c', INK = '#676f76';

/* ---------- deterministic data ---------- */

function mulberry32(a) {
  return function () {
    a |= 0; a = a + 0x6D2B79F5 | 0;
    let t = Math.imul(a ^ a >>> 15, 1 | a);
    t = t + Math.imul(t ^ t >>> 7, 61 | t) ^ t;
    return ((t ^ t >>> 14) >>> 0) / 4294967296;
  };
}

function gaussianClusters() {
  const rand = mulberry32(230607);
  const gauss = () => (rand() + rand() + rand() + rand() - 2) / 2;
  const centers = [[-1.1, -0.5, 0.9], [1.0, 0.8, -0.6], [0.1, -1.1, -1.1]];
  const spread = [0.8, 0.7, 0.6];
  return centers.map((c, t) => {
    const xs = [], ys = [], zs = [];
    for (let i = 0; i < 110; i++) {
      xs.push(c[0] + gauss() * spread[t]);
      ys.push(c[1] + gauss() * spread[t]);
      zs.push(c[2] + gauss() * spread[t]);
    }
    return { xs, ys, zs };
  });
}

function rippleGrid(n) {
  const xs = [], ys = [], zs = [];
  for (let i = 0; i < n; i++) xs.push(-3 + 6 * i / (n - 1));
  for (let j = 0; j < n; j++) ys.push(-3 + 6 * j / (n - 1));
  for (let j = 0; j < n; j++) {
    for (let i = 0; i < n; i++) {
      const r = Math.hypot(xs[i], ys[j]);
      zs.push(Math.sin(r * 2.2) / (1 + r * r * 0.35));
    }
  }
  return { xs, ys, zs };
}

function sphereGraph(n) {
  // Fibonacci sphere nodes; edges = ring neighbours + a few long chords.
  const rand = mulberry32(42);
  const xs = [], ys = [], zs = [], edges = [];
  const phi = Math.PI * (3 - Math.sqrt(5));
  for (let i = 0; i < n; i++) {
    const y = 1 - 2 * i / (n - 1);
    const r = Math.sqrt(Math.max(0, 1 - y * y));
    xs.push(Math.cos(phi * i) * r);
    ys.push(y);
    zs.push(Math.sin(phi * i) * r);
  }
  for (let i = 0; i < n; i++) edges.push(i, (i + 1) % n);
  for (let i = 0; i < n; i += 4) edges.push(i, (i + 9) % n);
  for (let k = 0; k < 6; k++) edges.push((rand() * n) | 0, (rand() * n) | 0);
  return { xs, ys, zs, edges };
}

// plotui's crate graph, snapshotted from `cargo metadata` — the same scene
// as `plotui example deps`. Groups: w(orkspace), e(xternal), t(ransitive);
// an edge (a, b) means a depends on b. The last DEPS_FLY_IN nodes arrive
// live; their edges only reference earlier indices.
const DEPS_NODES = [
  ['plotui', 'w'], ['plotui-bind', 'w'], ['plotui-core', 'w'],
  ['plotui-ffi', 'w'], ['plotui-protocol', 'w'], ['plotui-py', 'w'],
  ['plotui-ratatui', 'w'], ['plotui-term', 'w'], ['plotui-wasm', 'w'],
  ['base64', 'e'], ['clap', 'e'], ['crossterm', 'e'], ['flate2', 'e'],
  ['numpy', 'e'], ['pyo3', 'e'], ['ratatui', 'e'], ['rustix', 'e'],
  ['wasm-bindgen', 'e'],
  ['bitflags', 't'], ['cfg-if', 't'], ['clap_builder', 't'],
  ['clap_derive', 't'], ['crc32fast', 't'], ['crossterm_winapi', 't'],
  ['derive_more', 't'], ['document-features', 't'], ['errno', 't'],
  ['filedescriptor', 't'], ['indoc', 't'], ['instability', 't'],
  ['libc', 't'], ['linux-raw-sys', 't'], ['memoffset', 't'],
  ['miniz_oxide', 't'], ['mio', 't'], ['ndarray', 't'],
  ['num-complex', 't'], ['num-integer', 't'], ['num-traits', 't'],
  ['once_cell', 't'], ['parking_lot', 't'], ['portable-atomic', 't'],
  ['pyo3-ffi', 't'], ['pyo3-macros', 't'], ['ratatui-core', 't'],
  ['ratatui-crossterm', 't'], ['ratatui-macros', 't'],
  ['ratatui-termina', 't'], ['ratatui-termwiz', 't'],
  ['ratatui-widgets', 't'], ['rustc-hash', 't'], ['serde', 't'],
  ['signal-hook', 't'], ['signal-hook-mio', 't'], ['unindent', 't'],
  ['wasm-bindgen-macro', 't'], ['wasm-bindgen-shared', 't'],
  ['winapi', 't'], ['windows-sys', 't'],
];
const DEPS_EDGES = [
  [0, 2], [0, 6], [0, 7], [0, 10], [0, 11], [0, 15], [1, 2], [3, 1], [3, 2],
  [3, 4], [3, 7], [4, 2], [4, 9], [4, 12], [5, 1], [5, 2], [5, 4], [5, 7],
  [5, 13], [5, 14], [6, 2], [6, 4], [6, 7], [6, 11], [7, 2], [7, 4], [7, 16],
  [8, 1], [8, 2], [8, 17], [10, 20], [10, 21], [11, 16], [11, 18], [11, 23],
  [11, 24], [11, 25], [11, 27], [11, 34], [11, 40], [11, 52], [11, 53],
  [11, 57], [12, 22], [12, 33], [13, 14], [13, 30], [13, 35], [13, 36],
  [13, 37], [13, 38], [13, 50], [14, 28], [14, 30], [14, 32], [14, 39],
  [14, 41], [14, 42], [14, 43], [14, 54], [15, 29], [15, 44], [15, 45],
  [15, 46], [15, 47], [15, 48], [15, 49], [15, 51], [16, 18], [16, 26],
  [16, 30], [16, 31], [16, 58], [17, 19], [17, 39], [17, 55], [17, 56],
];
const DEPS_FLY_IN = 8;

function lorenz(steps) {
  const xs = [], ys = [], zs = [];
  let x = 0.1, y = 0, z = 0;
  const dt = 0.006;
  for (let i = 0; i < steps; i++) {
    const dx = 10 * (y - x);
    const dy = x * (28 - z) - y;
    const dz = x * y - (8 / 3) * z;
    x += dx * dt; y += dy * dt; z += dz * dt;
    xs.push(x); ys.push(y); zs.push(z);
  }
  return { xs, ys, zs };
}

/* ---------- the example registry ---------- */

const fmt = (v) => (Math.abs(v) >= 100 ? v.toFixed(0) : v.toFixed(2));

const EXAMPLES = {
  scatter3d: {
    is3d: true, pick: 'node',
    setup(plot) {
      const clusters = gaussianClusters();
      const colors = [T1, T2, T3];
      const names = ['Cluster A', 'Cluster B', 'Cluster C'];
      clusters.forEach((c, t) => plot.add_scatter3d(c.xs, c.ys, c.zs, colors[t], 3.0));
      const per = clusters[0].xs.length;
      return {
        tooltip(hit) {
          const t = Math.floor(hit.index / per), i = hit.index % per;
          const c = clusters[t];
          return `<span class="sw" style="background:${colors[t]}"></span>${names[t]}`
            + `<br>x ${fmt(c.xs[i])}<br>y ${fmt(c.ys[i])}<br>z ${fmt(c.zs[i])}`;
        },
      };
    },
  },

  surface3d: {
    is3d: true,
    setup(plot, ui) {
      const g = rippleGrid(48);
      plot.add_surface3d(g.xs, g.ys, g.zs, undefined, 'viridis', false, undefined);
      const wire = plot.add_surface3d(g.xs, g.ys, g.zs, INK, undefined, true, undefined);
      plot.set_visible(wire, false);
      const btn = ui.card.querySelector('.wire-toggle');
      let on = false;
      btn.addEventListener('click', () => {
        on = !on;
        plot.set_visible(wire, on);
        btn.classList.toggle('on', on);
        btn.setAttribute('aria-pressed', String(on));
        ui.markDirty();
      });
      return {};
    },
  },

  graph3d: {
    is3d: true, pick: 'element',
    setup(plot) {
      const g = sphereGraph(60);
      plot.add_graph3d(g.xs, g.ys, g.zs, g.edges, T2, 3.5);
      return {
        tooltip(hit) {
          if (hit.isEdge) {
            const a = g.edges[hit.index * 2], b = g.edges[hit.index * 2 + 1];
            return `<span class="sw" style="background:${T2}"></span>edge ${a} — ${b}`;
          }
          const i = hit.index;
          return `<span class="sw" style="background:${T2}"></span>node ${i}`
            + `<br>x ${fmt(g.xs[i])}<br>y ${fmt(g.ys[i])}<br>z ${fmt(g.zs[i])}`;
        },
      };
    },
  },

  deps: {
    is3d: true, pick: 'element',
    setup(plot, ui) {
      const GROUP = { w: T1, e: T2, t: T3 };
      const GROUP_NAME = { w: 'workspace crate', e: 'direct dependency', t: 'transitive dependency' };
      const dimHex = (hex) => {
        const v = parseInt(hex.slice(1), 16);
        const q = (x) => x >> 2;
        return '#' + ((q(v >> 16 & 255) << 16) | (q(v >> 8 & 255) << 8) | q(v & 255))
          .toString(16).padStart(6, '0');
      };
      const flat = (es) => new Uint32Array(es.flat());

      let n = DEPS_NODES.length - DEPS_FLY_IN;
      let edges = DEPS_EDGES.filter(([a, b]) => a < n && b < n);
      const base = DEPS_NODES.slice(0, n).map(([, g]) => GROUP[g]);
      const lay = new ForceLayout(n, flat(edges), 20260830);
      for (let i = 0; i < 30; i++) lay.step(); // past the initial explosion
      let energy = Infinity;

      const posArrays = () => {
        const p = lay.positions();
        const m = p.length / 3;
        const xs = new Float32Array(m), ys = new Float32Array(m), zs = new Float32Array(m);
        for (let i = 0; i < m; i++) {
          xs[i] = p[i * 3]; ys[i] = p[i * 3 + 1]; zs[i] = p[i * 3 + 2];
        }
        return [xs, ys, zs];
      };

      const [xs, ys, zs] = posArrays();
      const h = plot.add_graph3d(xs, ys, zs, flat(edges), T2, 3.2, undefined);
      plot.set_graph_colors(h, base, undefined);
      plot.set_show_box(false);
      plot.set_bounds(-1.45, -1.45, -1.45, 1.45, 1.45, 1.45);

      // The transitive-dependency closure of node i over the current edges.
      const reach = (i) => {
        const seen = new Set([i]);
        const stack = [i];
        while (stack.length) {
          const a = stack.pop();
          for (const [x, y] of edges) {
            if (x === a && !seen.has(y)) { seen.add(y); stack.push(y); }
          }
        }
        return seen;
      };

      let lastSpawn = performance.now();
      return {
        tick() {
          if (energy >= 1e-3) {
            energy = lay.step();
            plot.set_graph_positions(h, ...posArrays());
            ui.markDirty();
          }
          if (n < DEPS_NODES.length && performance.now() - lastSpawn > 2500) {
            lastSpawn = performance.now();
            const idx = n;
            const newEdges = DEPS_EDGES.filter(([a, b]) => a === idx || b === idx);
            lay.add_node(new Uint32Array(newEdges.map(([a, b]) => (a === idx ? b : a))));
            const p = lay.positions();
            plot.extend_graph(
              h,
              [p[idx * 3]], [p[idx * 3 + 1]], [p[idx * 3 + 2]],
              [GROUP[DEPS_NODES[idx][1]]],
              flat(newEdges),
            );
            base.push(GROUP[DEPS_NODES[idx][1]]);
            edges = edges.concat(newEdges);
            n += 1;
            energy = Infinity; // re-heated: keep animating
            ui.markDirty();
          }
        },
        hover(hit) {
          if (hit && !hit.isEdge && hit.index < n) {
            const on = reach(hit.index);
            plot.set_graph_colors(
              h,
              base.map((c, j) => (on.has(j) ? c : dimHex(c))),
              edges.map(([a, b]) => (on.has(a) && on.has(b) ? '#aab0b8' : '#222630')),
            );
          } else {
            plot.set_graph_colors(h, base, undefined);
          }
          ui.markDirty();
        },
        tooltip(hit) {
          if (hit.isEdge) {
            const [a, b] = edges[hit.index];
            return `<span class="sw" style="background:${T2}"></span>`
              + `${DEPS_NODES[a][0]} → ${DEPS_NODES[b][0]}`;
          }
          const [nm, g] = DEPS_NODES[hit.index];
          const deps = reach(hit.index).size - 1;
          return `<span class="sw" style="background:${GROUP[g]}"></span>${nm}`
            + `<br>${GROUP_NAME[g]}<br>${deps} deps`;
        },
      };
    },
  },

  lidar: {
    is3d: true,
    setup(plot, ui) {
      // Mirrors crates/plotui-cli/src/lidar.rs constant-for-constant (same
      // PRNG, same seed), so this sweep is point-for-point the one in
      // `plotui example lidar`.
      const DEG = Math.PI / 180;
      const SENSOR = [0, 0, 0.8];
      const BEAMS = 16, ELEV_LO = -22 * DEG, ELEV_HI = 12 * DEG;
      const AZ_COLS = 400, TOTAL = AZ_COLS * 2;
      const AZ_STEP = 360 * DEG / AZ_COLS;
      const COL_MS = 15, MAX_RANGE = 9;
      const BOXES = [
        [[7.9, -8.1, 0.0], [8.1, 8.1, 2.5]],
        [[-8.1, -8.1, 0.0], [-7.9, 8.1, 2.5]],
        [[-8.1, 7.9, 0.0], [8.1, 8.1, 2.5]],
        [[-8.1, -8.1, 0.0], [8.1, -7.9, 2.5]],
        [[2.0, 1.0, 0.0], [3.2, 2.2, 1.2]],
        [[-4.5, 3.0, 0.0], [-3.3, 4.2, 2.0]],
        [[-2.0, -5.0, 0.0], [-0.4, -3.6, 0.9]],
        [[4.2, -3.5, 0.0], [5.0, -2.7, 2.5]],
        [[-6.2, -1.0, 0.0], [-5.4, 0.2, 1.6]],
      ];
      // Height bands (upper z bound → color): deep blue → cyan → amber.
      const BANDS = [
        [0.15, '#2d46a5'], [0.60, '#2878cd'], [1.10, '#2dafd7'],
        [1.60, '#6ecdb9'], [2.10, '#c8c878'], [Infinity, '#ebaf5a'],
      ];
      const rand = mulberry32(20260830);
      const gauss = () => (rand() + rand() + rand() + rand() - 2) / 2;

      const slab = (o, d, lo, hi) => {
        let t0 = 0, t1 = Infinity;
        for (let k = 0; k < 3; k++) {
          if (Math.abs(d[k]) < 1e-8) {
            if (o[k] < lo[k] || o[k] > hi[k]) return null;
          } else {
            let a = (lo[k] - o[k]) / d[k], b = (hi[k] - o[k]) / d[k];
            if (a > b) [a, b] = [b, a];
            t0 = Math.max(t0, a);
            t1 = Math.min(t1, b);
            if (t0 > t1) return null;
          }
        }
        return t0 > 1e-4 ? t0 : null;
      };
      const cast = (o, d) => {
        let best = MAX_RANGE, hit = false;
        if (d[2] < 0) {
          const t = -o[2] / d[2];
          if (t < best && Math.abs(o[0] + d[0] * t) <= 8 && Math.abs(o[1] + d[1] * t) <= 8) {
            best = t;
            hit = true;
          }
        }
        for (const [lo, hi] of BOXES) {
          const t = slab(o, d, lo, hi);
          if (t !== null && t < best) {
            best = t;
            hit = true;
          }
        }
        return hit ? best : null;
      };

      const handles = BANDS.map(([, c]) => plot.add_scatter3d([], [], [], c, 2.0));
      plot.set_bounds(-8.5, -8.5, -0.4, 8.5, 8.5, 3.6);

      const column = (az, out) => {
        const sinT = Math.sin(az * AZ_STEP), cosT = Math.cos(az * AZ_STEP);
        for (let b = 0; b < BEAMS; b++) {
          const phi = ELEV_LO + (ELEV_HI - ELEV_LO) * b / (BEAMS - 1);
          const sinP = Math.sin(phi), cosP = Math.cos(phi);
          const d = [cosP * cosT, cosP * sinT, sinP];
          const t = cast(SENSOR, d);
          if (t === null) { gauss(); continue; } // keep the noise stream aligned
          const r = t + gauss() * 0.02;
          const p = [SENSOR[0] + d[0] * r, SENSOR[1] + d[1] * r, SENSOR[2] + d[2] * r];
          const band = BANDS.findIndex(([top]) => p[2] < top);
          out[band < 0 ? BANDS.length - 1 : band].push(p);
        }
      };

      let last = performance.now();
      let az = 0, acc = 0;
      return {
        tick() {
          const now = performance.now();
          const dt = Math.min(now - last, 250);
          last = now;
          if (!ui.dragging()) plot.spin(SPIN_STEP);
          if (az < TOTAL) {
            acc += dt;
            const out = BANDS.map(() => []);
            while (acc >= COL_MS && az < TOTAL) {
              acc -= COL_MS;
              column(az++, out);
            }
            out.forEach((pts, i) => {
              if (!pts.length) return;
              plot.extend_xyz(
                handles[i],
                pts.map((p) => p[0]), pts.map((p) => p[1]), pts.map((p) => p[2]),
              );
            });
          }
          ui.markDirty(); // spinning even after the sweep completes
        },
      };
    },
  },

  mandelbulb: {
    is3d: true,
    setup(plot, ui) {
      // Mirrors crates/plotui-cli/src/mandelbulb.rs constant-for-constant
      // and shares its marching-cubes tables through wasm, so this is the
      // bulb from `plotui example mandelbulb`. The field is sampled in f32
      // (Math.fround) to track the Rust one, but acos/atan2/pow round
      // differently here, so a handful of samples near the surface land on
      // the other side of the iso value: expect the odd triangle to differ.
      const RES = 64, HALF = 1.2, CELL = 2 * HALF / (RES - 1), ISO = 0;
      const POWER = 8, ITERS = 14, BAILOUT = 2;
      const BANDS = 60, REVEAL_MS = 150;

      // The power-8 Mandelbulb distance estimator, in f32 throughout so the
      // field matches the Rust one sample for sample.
      const f = Math.fround;
      const distance = (cx, cy, cz) => {
        let zx = cx, zy = cy, zz = cz, dr = 1, r = 0, escaped = false;
        for (let i = 0; i < ITERS; i++) {
          r = f(Math.sqrt(f(f(f(zx * zx) + f(zy * zy)) + f(zz * zz))));
          if (r > BAILOUT) { escaped = true; break; }
          const theta = f(f(Math.acos(f(zz / r))) * POWER);
          const phi = f(f(Math.atan2(zy, zx)) * POWER);
          dr = f(f(f(f(Math.pow(r, POWER - 1)) * POWER) * dr) + 1);
          const zr = f(Math.pow(r, POWER));
          const st = f(Math.sin(theta));
          zx = f(f(f(zr * st) * f(Math.cos(phi))) + cx);
          zy = f(f(f(zr * st) * f(Math.sin(phi))) + cy);
          zz = f(f(zr * f(Math.cos(theta))) + cz);
        }
        const d = f(f(f(0.5 * f(Math.log(Math.max(r, 1e-9)))) * r) / dr);
        return escaped ? Math.max(d, 1e-6) : Math.min(d, -1e-6);
      };

      const values = new Float32Array(RES * RES * RES);
      const coord = (i) => f(-HALF + f(i * CELL));
      for (let k = 0, n = 0; k < RES; k++) {
        for (let j = 0; j < RES; j++) {
          for (let i = 0; i < RES; i++) values[n++] = distance(coord(i), coord(j), coord(k));
        }
      }
      const mesh = marching_cubes(values, RES, RES, RES,
        Float32Array.from([-HALF, -HALF, -HALF]), CELL, ISO);
      const [mx, my, mz, mt] = [mesh.xs(), mesh.ys(), mesh.zs(), mesh.tris()];

      // Deal the triangles into height bands by z centroid, then re-index
      // each band against its own vertices — plus the bulb's lowest and
      // highest vertex, so the Plasma ramp spans the whole bulb in every
      // slice instead of restarting inside each one.
      const bands = Array.from({ length: BANDS }, () => []);
      for (let t = 0; t < mt.length; t += 3) {
        const zc = (mz[mt[t]] + mz[mt[t + 1]] + mz[mt[t + 2]]) / 3;
        const b = Math.min(((zc + HALF) / (2 * HALF) * BANDS) | 0, BANDS - 1);
        bands[b].push(t);
      }
      let lo = 0, hi = 0;
      for (let v = 0; v < mz.length; v++) {
        if (mz[v] < mz[lo]) lo = v;
        if (mz[v] > mz[hi]) hi = v;
      }
      const local = new Int32Array(mz.length).fill(-1);
      const handles = bands.map((band) => {
        const xs = [mx[lo], mx[hi]], ys = [my[lo], my[hi]], zs = [mz[lo], mz[hi]];
        const tris = [], touched = [lo, hi];
        local[lo] = 0;
        local[hi] = 1;
        for (const t of band) {
          for (let e = 0; e < 3; e++) {
            const g = mt[t + e];
            if (local[g] < 0) {
              local[g] = xs.length;
              xs.push(mx[g]);
              ys.push(my[g]);
              zs.push(mz[g]);
              touched.push(g);
            }
            tris.push(local[g]);
          }
        }
        for (const g of touched) local[g] = -1;
        return plot.add_mesh3d(
          Float32Array.from(xs), Float32Array.from(ys), Float32Array.from(zs),
          Uint32Array.from(tris), undefined, 'plasma', undefined,
        );
      });
      // Pin the frame to the sampled box so the camera never breathes as
      // bands arrive.
      plot.set_bounds(-1.3, -1.3, -1.3, 1.3, 1.3, 1.3);
      for (let i = 1; i < handles.length; i++) plot.set_visible(handles[i], false);

      let last = performance.now();
      let shown = 1, acc = 0;
      return {
        tick() {
          const now = performance.now();
          const dt = Math.min(now - last, 250);
          last = now;
          if (!ui.dragging()) plot.spin(SPIN_STEP);
          if (shown < handles.length) {
            acc += dt;
            while (acc >= REVEAL_MS && shown < handles.length) {
              acc -= REVEAL_MS;
              plot.set_visible(handles[shown++], true);
            }
          }
          ui.markDirty(); // spinning even after the reveal completes
        },
      };
    },
  },

  aizawa: {
    is3d: true,
    setup(plot, ui) {
      // Mirrors crates/plotui-cli/src/aizawa.rs constant-for-constant. The
      // Rust side integrates in f64 for exactly this reason: nothing here is
      // transcendental, so both sides round identically and this is the same
      // orbit as `plotui example aizawa`, not just the same attractor.
      const A = 0.95, B = 0.7, C = 0.6, D = 3.5, E = 0.25, F = 0.1;
      const DT = 0.01, STEPS = 12000, STEP_MS = 1;
      const START = [0.1, 0, 0];
      const WIDTH = 1.4;
      // Speed bands (upper bound → color); colors are Plasma at 0.12 + 0.85 b / 5.
      const BANDS = [
        [0.85, '#430697'], [1.40, '#8a0ea0'], [2.20, '#c03c80'],
        [3.15, '#e3705b'], [4.30, '#f6a93a'], [Infinity, '#f1ed25'],
      ];

      const deriv = ([x, y, z]) => [
        (z - B) * x - D * y,
        D * x + (z - B) * y,
        C + A * z - z * z * z / 3 - (x * x + y * y) * (1 + E * z) + F * z * x * x * x,
      ];
      const step = (a, k, s) => [a[0] + k[0] * s, a[1] + k[1] * s, a[2] + k[2] * s];
      const rk4 = (p, dt) => {
        const k1 = deriv(p);
        const k2 = deriv(step(p, k1, dt * 0.5));
        const k3 = deriv(step(p, k2, dt * 0.5));
        const k4 = deriv(step(p, k3, dt));
        return [0, 1, 2].map((i) => p[i] + dt / 6 * (k1[i] + 2 * k2[i] + 2 * k3[i] + k4[i]));
      };
      // Math.sqrt of the sum, not Math.hypot: hypot is more accurate, which
      // would put a boundary point in a different band than Rust's sqrt does.
      const speed = (p) => { const d = deriv(p); return Math.sqrt(d[0]*d[0] + d[1]*d[1] + d[2]*d[2]); };
      const band = (p) => {
        const s = speed(p);
        const i = BANDS.findIndex(([top]) => s < top);
        return i < 0 ? BANDS.length - 1 : i;
      };

      const handles = BANDS.map(([, c]) => plot.add_line3d([], [], [], c, WIDTH));
      // Pin the frame to the attractor so the camera never breathes as the
      // curve grows into it.
      plot.set_bounds(-1.6, -1.6, -0.6, 1.6, 1.6, 1.9);

      let p = START, drawn = 0, acc = 0;
      const last = BANDS.map(() => -1);
      const advance = (out) => {
        const next = rk4(p, DT), b = band(p);
        // A NaN breaks the polyline, so the jump from wherever this band
        // left off is not drawn as a segment.
        if (last[b] === drawn) out[b].push(next);
        else out[b].push([NaN, NaN, NaN], p, next);
        drawn++;
        last[b] = drawn;
        p = next;
      };

      let lastTime = performance.now();
      return {
        tick() {
          const now = performance.now();
          const dt = Math.min(now - lastTime, 250);
          lastTime = now;
          if (!ui.dragging()) plot.spin(SPIN_STEP);
          if (drawn < STEPS) {
            acc += dt;
            const out = BANDS.map(() => []);
            while (acc >= STEP_MS && drawn < STEPS) {
              acc -= STEP_MS;
              advance(out);
            }
            out.forEach((pts, i) => {
              if (!pts.length) return;
              plot.extend_xyz(
                handles[i],
                pts.map((q) => q[0]), pts.map((q) => q[1]), pts.map((q) => q[2]),
              );
            });
          }
          ui.markDirty(); // spinning even after the curve completes
        },
      };
    },
  },

  protein: {
    is3d: true,
    // The same 1UBQ backbone file `plotui example protein` embeds, served
    // beside this script rather than pasted in here.
    requires: () => PDB !== null,
    missing: 'This example needs <code>1ubq.pdb</code>, which failed to load here.',
    setup(plot, ui) {
      // Mirrors crates/plotui-cli/src/protein.rs constant-for-constant,
      // reads the same file, and sweeps with the engine's own geometry
      // (`catmull_rom`, `ribbon` and `tube` through wasm) — so this is the
      // cartoon from `plotui example protein`. Not bit-identical the way
      // aizawa is: the normals are worked out in doubles here and in f32
      // there, which moves a few vertices by an ulp and nothing visible.
      const HELIX_W = 2.1, SHEET_W = 2.4, ARROW_W = 4.2, THICKNESS = 0.45;
      const COIL_R = 0.32, COIL_SIDES = 8;
      const SAMPLES = 12, ARROW_SAMPLES = 18, SMOOTH_PASSES = 2;
      const REVEAL_MS = 9000, MARGIN = 0.8;
      const COLOR = { helix: '#e66a5c', sheet: '#f0be5a', coil: '#7c94b2' };
      const LABEL = { helix: 'helix', sheet: 'sheet', coil: 'loop' };

      const sub = (a, b) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
      const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
      const cross = (a, b) => [
        a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0],
      ];
      const unit = (v, fallback) => {
        const n = Math.hypot(v[0], v[1], v[2]);
        return n > 1e-6 ? [v[0] / n, v[1] / n, v[2] / n] : fallback;
      };

      // --- the PDB, at its fixed column offsets ---
      const field = (l, a, b) => l.slice(a, b).trim();
      const seqAt = (l, a, b) => {
        const v = parseInt(field(l, a, b), 10);
        return Number.isNaN(v) ? null : v;
      };
      const lines = PDB.split(/\r?\n/);

      // HELIX and SHEET give inclusive residue ranges; their sequence fields
      // sit one column apart from each other, which is the format's doing.
      const spans = [];
      for (const l of lines) {
        let a, b, fold;
        if (l.startsWith('HELIX')) { a = seqAt(l, 21, 25); b = seqAt(l, 33, 37); fold = 'helix'; }
        else if (l.startsWith('SHEET')) { a = seqAt(l, 22, 26); b = seqAt(l, 33, 37); fold = 'sheet'; }
        else continue;
        if (a !== null && b !== null) spans.push([Math.min(a, b), Math.max(a, b), fold]);
      }

      // Atoms arrive grouped by residue, so a new sequence number opens one.
      const rows = [];
      for (const l of lines) {
        if (!l.startsWith('ATOM')) continue;
        const seq = seqAt(l, 22, 26);
        if (seq === null) continue;
        const xyz = [30, 38, 46].map((at) => parseFloat(field(l, at, at + 8)));
        if (xyz.some(Number.isNaN)) continue;
        if (!rows.length || rows[rows.length - 1].seq !== seq) rows.push({ seq, ca: null, o: null });
        const row = rows[rows.length - 1];
        if (field(l, 12, 16) === 'CA') row.ca = xyz;
        else if (field(l, 12, 16) === 'O') row.o = xyz;
      }

      const res = rows.filter((r) => r.ca && r.o).map((r) => {
        const span = spans.find(([a, b]) => r.seq >= a && r.seq <= b);
        return { ca: r.ca, o: r.o, fold: span ? span[2] : 'coil' };
      });
      // Recentre so the orbit sits on the molecule, not the crystal origin.
      const mid = [0, 1, 2].map((d) => res.reduce((a, r) => a + r.ca[d], 0) / res.length);
      for (const r of res) {
        for (let d = 0; d < 3; d++) { r.ca[d] -= mid[d]; r.o[d] -= mid[d]; }
      }

      // --- the peptide plane, flipped and smoothed ---
      // The flip removes the half-turn corkscrew a beta strand's alternating
      // carbonyls would otherwise put in the ribbon; the 1-2-1 passes cancel
      // the sheet's pleat, which the flip alone leaves at ~65 degrees per
      // residue. Helices keep winding, which is what a helical ribbon does.
      const nrm = [];
      for (let i = 0; i < res.length; i++) {
        const [a, b] = i + 1 < res.length ? [i, i + 1] : [i - 1, i];
        const step = sub(res[b].ca, res[a].ca);
        const prev = nrm.length ? nrm[nrm.length - 1] : [0, 0, 1];
        let n = unit(cross(step, sub(res[i].o, res[i].ca)), prev);
        if (nrm.length && dot(n, prev) < 0) n = [-n[0], -n[1], -n[2]];
        nrm.push(n);
      }
      for (let pass = 0; pass < SMOOTH_PASSES; pass++) {
        const src = nrm.slice();
        for (let i = 0; i < src.length; i++) {
          const a = src[Math.max(0, i - 1)], b = src[Math.min(src.length - 1, i + 1)];
          nrm[i] = unit([0, 1, 2].map((d) => a[d] + 2 * src[i][d] + b[d]), src[i]);
        }
      }

      // Runs of consecutive residues sharing one fold.
      const els = [];
      res.forEach((r, i) => {
        const last = els[els.length - 1];
        if (last && last.fold === r.fold) last.hi = i;
        else els.push({ fold: r.fold, lo: i, hi: i });
      });

      // Direction fields interpolate linearly, never through a spline: a
      // spline overshoots where its input turns hard, and the ribbon's face
      // flares into spikes at every turn.
      const resampleDirs = (v, per) => {
        if (v.length < 2 || per === 0) return v.slice();
        const out = [];
        for (let i = 0; i < v.length - 1; i++) {
          for (let k = 0; k < per; k++) {
            const t = k / per;
            out.push(unit([0, 1, 2].map((d) => v[i][d] * (1 - t) + v[i + 1][d] * t), v[i]));
          }
        }
        out.push(v[v.length - 1]);
        return out;
      };

      // Flat until the arrowhead, which flares wider than the ribbon before
      // tapering to a point at the strand's C-terminal end.
      const strandWidths = (samples) => {
        const arrow = Math.min(ARROW_SAMPLES, Math.floor(samples / 2));
        const base = samples - arrow;
        const out = new Float32Array(samples);
        for (let i = 0; i < samples; i++) {
          if (i < base || arrow === 0) { out[i] = SHEET_W; continue; }
          const t = (i - base) / Math.max(arrow - 1, 1);
          out[i] = ARROW_W * (1 - t) + 0.1 * t;
        }
        return out;
      };

      // One element's geometry. The path runs a residue past each end so
      // neighbours overlap and the chain reads as continuous.
      const elementMesh = (e) => {
        const lo = Math.max(0, e.lo - 1), hi = Math.min(e.hi + 1, res.length - 1);
        if (hi - lo + 1 < 2) return null;
        const path = new Float32Array((hi - lo + 1) * 3);
        for (let i = lo; i <= hi; i++) path.set(res[i].ca, (i - lo) * 3);
        const smooth = catmull_rom(path, SAMPLES);
        if (e.fold === 'coil') return tube(smooth, Float32Array.of(COIL_R), COIL_SIDES);
        const dirs = resampleDirs(nrm.slice(lo, hi + 1), SAMPLES);
        const up = new Float32Array(dirs.length * 3);
        dirs.forEach((d, i) => up.set(d, i * 3));
        const widths = e.fold === 'sheet'
          ? strandWidths(smooth.length / 3)
          : Float32Array.of(HELIX_W);
        return ribbon(smooth, up, widths, THICKNESS);
      };

      const labelled = new Set();
      const pieces = [];
      let extent = 0;
      for (const e of els) {
        const mesh = elementMesh(e);
        if (!mesh) continue;
        const xs = mesh.xs(), ys = mesh.ys(), zs = mesh.zs();
        for (const arr of [xs, ys, zs]) {
          for (const c of arr) extent = Math.max(extent, Math.abs(c));
        }
        // The legend wants one entry per fold, not one per element.
        let name;
        if (!labelled.has(e.fold)) { labelled.add(e.fold); name = LABEL[e.fold]; }
        pieces.push({
          handle: plot.add_mesh3d(xs, ys, zs, mesh.tris(), COLOR[e.fold], undefined, name),
          residues: e.hi - e.lo + 1,
        });
      }
      // Pin a cube about the centroid: the chain grows into the frame, so
      // without this the camera would zoom out under it as it folds.
      const r = extent + MARGIN;
      plot.set_bounds(-r, -r, -r, r, r, r);
      const handles = pieces.map((p) => p.handle);
      for (let i = 1; i < handles.length; i++) plot.set_visible(handles[i], false);

      // Each element is held in proportion to its length, so a long helix
      // does not fold in as fast as the two-residue turn beside it.
      const total = pieces.reduce((a, p) => a + p.residues, 0);
      const dwell = pieces.map((p) => REVEAL_MS * p.residues / total);

      let last = performance.now();
      let shown = 1, acc = 0;
      return {
        tick() {
          const now = performance.now();
          const dt = Math.min(now - last, 250);
          last = now;
          if (!ui.dragging()) plot.spin(SPIN_STEP);
          if (shown < handles.length) {
            acc += dt;
            while (shown < handles.length && acc >= dwell[shown]) {
              acc -= dwell[shown];
              plot.set_visible(handles[shown++], true);
            }
          }
          ui.markDirty(); // spinning even after the fold completes
        },
      };
    },
  },

  lorenz: {
    is3d: true,
    setup(plot) {
      const l = lorenz(2500);
      plot.add_line3d(l.xs, l.ys, l.zs, T1, 1.5, 'lorenz');
      return {};
    },
  },

  lines2d: {
    is3d: false, hover2d: true,
    setup(plot) {
      const rand = mulberry32(7);
      const xs = [], damped = [], carrier = [];
      for (let x = 0; x <= 12.0001; x += 0.05) {
        xs.push(x);
        damped.push(Math.exp(-x / 6) * Math.sin(2 * x));
        carrier.push(0.6 * Math.exp(-x / 8) * Math.cos(3 * x));
      }
      const sx = [], sy = [];
      for (let i = 0; i < xs.length; i += 12) {
        sx.push(xs[i]);
        sy.push(damped[i] + (rand() - 0.5) * 0.14);
      }
      plot.add_line2d(xs, damped, T2, 2.0, 'damped', undefined);
      plot.add_line2d(xs, carrier, T3, 2.0, 'carrier', undefined);
      plot.add_scatter2d(sx, sy, T1, 2.5, 'samples', undefined);
      return {};
    },
  },

  steps2d: {
    is3d: false, hover2d: true,
    setup(plot) {
      // A worker pool: the count *holds* between events, so the honest shape
      // is the right-angle path — a straight segment would draw the pool
      // gliding between sizes it never had.
      const rand = mulberry32(31);
      const ts = [], workers = [], queue = [];
      let n = 4;
      for (let t = 0; t <= 60; t++) {
        if (t % 5 === 0) n = Math.min(6, Math.max(2, n + (rand() < 0.5 ? 1 : -1)));
        // The queue eases down when workers arrive and back up when they
        // leave. Deliberately a bounded expression rather than an
        // accumulating one: a random walk either drifts off the top of the
        // frame or pins to its clamp, and both squash the step series this
        // card exists to show.
        ts.push(t);
        workers.push(n);
        queue.push(4.2 - (n - 4) * 0.55 + Math.sin(t / 5.5) * 1.1 + (rand() - 0.5) * 0.7);
      }
      plot.add_step2d(ts, workers, T2, 2.0, 'post', 'workers', undefined);
      plot.add_line2d(ts, queue, T3, 2.0, 'queue depth', undefined);
      return {};
    },
  },

  hist2d: {
    is3d: false, hover2d: true,
    setup(plot) {
      // Request latency: a long right tail is the whole point of plotting a
      // distribution rather than its mean.
      const rand = mulberry32(2029);
      const samples = [];
      for (let i = 0; i < 4000; i++) {
        // Sum-of-uniforms for the body, a rare slow path for the tail.
        let s = 0;
        for (let k = 0; k < 4; k++) s += rand();
        const ms = 40 + s * 22 + (rand() < 0.06 ? rand() * 260 : 0);
        samples.push(ms);
      }
      plot.add_histogram2d(samples, undefined, undefined, T2, 'latency (ms)', undefined);
      return {};
    },
  },

  heatmap2d: {
    is3d: false,
    setup(plot) {
      // Traffic by hour and weekday — the shape a terminal is unusually good
      // at showing, since a matrix wants width more than height.
      const days = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];
      const rand = mulberry32(515);
      const xs = [], ys = [], zs = [];
      for (let h = 0; h < 24; h++) xs.push(h);
      for (let d = 0; d < days.length; d++) ys.push(d);
      for (let d = 0; d < days.length; d++) {
        const weekend = d >= 5;
        for (let h = 0; h < 24; h++) {
          // Two commuter peaks on weekdays, one lazy hump at the weekend.
          const morning = Math.exp(-((h - 8.5) ** 2) / 6);
          const evening = Math.exp(-((h - 17.5) ** 2) / 8);
          const lazy = Math.exp(-((h - 13) ** 2) / 26);
          const base = weekend ? 0.55 * lazy : 0.85 * morning + 1.0 * evening;
          zs.push(base * 100 + rand() * 9);
        }
      }
      plot.add_heatmap2d(xs, ys, zs, 'viridis', true, 'requests/s', undefined);
      plot.set_categories('y', days);
      return {};
    },
  },

  stacked2d: {
    is3d: false, hover2d: true,
    setup(plot, ui) {
      // Generation mix: the question is what makes up the total, so the
      // series have to add rather than hide one another.
      const rand = mulberry32(88);
      const months = [], solar = [], wind = [], hydro = [];
      for (let m = 0; m < 12; m++) {
        const summer = Math.cos(((m - 6) / 12) * Math.PI * 2);
        months.push(m);
        solar.push(14 + summer * 9 + rand() * 3);
        wind.push(20 - summer * 6 + rand() * 4);
        hydro.push(11 + rand() * 3);
      }
      plot.set_barmode('stack');
      plot.add_bar2d(months, solar, T3, 'solar', undefined);
      plot.add_bar2d(months, wind, T2, 'wind', undefined);
      plot.add_bar2d(months, hydro, T1, 'hydro', undefined);
      plot.set_categories('x',
        ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun',
         'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']);

      // The same data reads two ways; the button is the argument for having
      // both modes rather than one.
      const btn = ui.card.querySelector('.mode-toggle');
      let stacked = true;
      btn?.addEventListener('click', () => {
        stacked = !stacked;
        plot.set_barmode(stacked ? 'stack' : 'group');
        btn.textContent = stacked ? 'stack' : 'group';
        btn.setAttribute('aria-pressed', String(stacked));
        ui.markDirty();
      });
      return {};
    },
  },

  band2d: {
    is3d: false, hover2d: true,
    setup(plot) {
      // A forecast and what it does not know. The band goes in first: draw
      // order is the only layering in 2D, so adding it after the line would
      // paint over the very thing it qualifies.
      const rand = mulberry32(404);
      const xs = [], mid = [], lo = [], hi = [];
      const obsX = [], obsY = [], err = [];
      let level = 30;
      for (let h = 0; h <= 48; h++) {
        level += (rand() - 0.5) * 3 + Math.sin(h / 7) * 0.9;
        const spread = 1.6 + h * 0.16;
        xs.push(h);
        mid.push(level);
        lo.push(level - spread);
        hi.push(level + spread);
        if (h % 6 === 0 && h <= 24) {
          obsX.push(h);
          obsY.push(level + (rand() - 0.5) * 2.2);
          err.push(1.2 + rand() * 0.8);
        }
      }
      plot.add_band2d(xs, lo, hi, '#1f4d55', '95% interval', undefined);
      plot.add_line2d(xs, mid, T2, 2.0, 'forecast', undefined);
      const obs = plot.add_scatter2d(obsX, obsY, T1, 3.0, 'observed', undefined);
      plot.set_error_bars(obs, err, undefined, undefined, undefined);
      return {};
    },
  },

  box2d: {
    is3d: false,
    setup(plot) {
      // Four build machines. The medians are close; the spread and the
      // stragglers are the story, which is exactly what a box shows and a
      // bar chart of means hides.
      const rand = mulberry32(9001);
      const names = ['ci-a', 'ci-b', 'ci-c', 'ci-d'];
      const spreads = [1.0, 2.4, 0.7, 1.6];
      const values = [], starts = [];
      names.forEach((_, g) => {
        starts.push(values.length);
        const n = 90;
        for (let i = 0; i < n; i++) {
          let s = 0;
          for (let k = 0; k < 4; k++) s += rand();
          let v = 62 + (s - 2) * 6 * spreads[g];
          // A few stragglers per machine — the points a whisker must not
          // swallow.
          if (rand() < 0.035) v += 20 + rand() * 45 * spreads[g];
          values.push(v);
        }
      });
      plot.add_box2d(values, starts, T2, 'vertical', 'build time (s)', undefined);
      plot.set_categories('x', names);
      return {};
    },
  },

  dag2d: {
    is3d: false, pick: 'element',
    setup(plot, ui) {
      // The nine-task nightly forecast the CLI's `example pipeline` runs.
      const TASKS = [
        'fetch_prices', 'fetch_weather', 'clean_prices', 'clean_weather',
        'join_frames', 'build_features', 'train_model', 'backtest', 'publish',
      ];
      // (from, to): `to` waits on `from`. The last one skips two ranks, so
      // the layout has a long edge to route around what is between them.
      const EDGES = [
        [0, 2], [1, 3], [2, 4], [3, 4], [4, 5], [5, 6], [5, 7], [6, 8], [7, 8], [2, 7],
      ];
      // train_model fails: it is not a leaf, so publish is stranded while
      // backtest beside it still finishes — the thing a status table cannot
      // show you.
      const FAILS = 6;
      const DONE = '#199e70', RUNNING = T2, FAILED = '#e66767', PENDING = '#3a4054';
      const STEP_MS = 1200, RESTART_MS = 2600;

      const layout = new LayeredLayout(9, new Uint32Array(EDGES.flat()), 'TB');
      const pos = layout.positions();
      const xs = new Float32Array(9), ys = new Float32Array(9);
      for (let i = 0; i < 9; i++) { xs[i] = pos[i * 2]; ys[i] = pos[i * 2 + 1]; }

      const h = plot.add_graph2d(
        xs, ys, new Uint32Array(EDGES.flat()),
        TASKS, true, undefined, undefined,
        TASKS.map((_, i) => (i === 8 ? 'ellipse' : 'rounded')),
        undefined, layout.route_pts(), layout.route_starts(), undefined,
      );
      // Four empty graph traces: no geometry, one legend row each. The
      // states need a key, and a key is what the legend is.
      for (const [name, c] of [['done', DONE], ['running', RUNNING], ['failed', FAILED], ['pending', PENDING]]) {
        plot.add_graph2d(new Float32Array(), new Float32Array(), new Uint32Array(),
          undefined, true, [c], undefined, undefined, undefined, undefined, undefined, name);
      }

      // A topological order — the order a scheduler with one worker runs in.
      const order = (() => {
        const indeg = new Array(9).fill(0);
        for (const [, b] of EDGES) indeg[b] += 1;
        const out = [], done = new Array(9).fill(false);
        while (out.length < 9) {
          const next = indeg.findIndex((d, i) => !done[i] && d === 0);
          if (next < 0) break;
          done[next] = true;
          out.push(next);
          for (const [a, b] of EDGES) if (a === next) indeg[b] -= 1;
        }
        return out;
      })();

      let states = new Array(9).fill(PENDING);
      let at = 0, last = performance.now();
      const paint = () => { plot.set_graph_colors(h, states, undefined); ui.markDirty(); };
      paint();

      const dim = (hex) => {
        const v = parseInt(hex.slice(1), 16);
        const q = (x) => (x >> 2) + 12;
        return '#' + ((q(v >> 16 & 255) << 16) | (q(v >> 8 & 255) << 8) | q(v & 255))
          .toString(16).padStart(6, '0');
      };
      // Everything the hovered task waits on, straight from the engine, so
      // the browser and the terminal agree on what a highlight means.
      const upstream = (i) => plot_reachable(9, new Uint32Array(EDGES.flat()), i, true);

      return {
        tick() {
          const now = performance.now();
          const over = at >= order.length && !states.includes(RUNNING);
          if (now - last < (over ? RESTART_MS : STEP_MS)) return;
          last = now;
          if (over) { states = new Array(9).fill(PENDING); at = 0; paint(); return; }
          states = states.map((s, i) => (s === RUNNING ? (i === FAILS ? FAILED : DONE) : s));
          while (at < order.length) {
            const next = order[at];
            at += 1;
            // A task whose upstream failed never runs — which is what makes
            // the failure legible on everything downstream of it.
            const blocked = EDGES.some(([a, b]) => b === next && states[a] !== DONE);
            if (!blocked) { states[next] = RUNNING; break; }
          }
          paint();
        },
        hover(hit) {
          if (hit && !hit.isEdge && hit.index < 9) {
            const on = upstream(hit.index);
            plot.set_graph_colors(
              h,
              states.map((c, j) => (on[j] ? c : dim(c))),
              EDGES.map(([a, b]) => (on[a] && on[b] ? '#aab0b8' : '#282d3c')),
            );
          } else {
            plot.set_graph_colors(h, states, undefined);
          }
          ui.markDirty();
        },
        tooltip(hit) {
          if (hit.isEdge) {
            const [a, b] = EDGES[hit.index];
            return `<span class="sw" style="background:${T2}"></span>${TASKS[a]} → ${TASKS[b]}`;
          }
          if (hit.index >= 9) return '';
          const waits = upstream(hit.index).reduce((n, on) => n + (on ? 1 : 0), 0) - 1;
          return `<span class="sw" style="background:${states[hit.index]}"></span>`
            + `${TASKS[hit.index]}<br>waits on ${waits}`;
        },
      };
    },
  },

  bars2d: {
    is3d: false, hover2d: true,
    setup(plot) {
      const rand = mulberry32(11);
      const xs = [], totals = [], trend = [];
      let level = 42;
      for (let m = 1; m <= 12; m++) {
        level += (rand() - 0.42) * 14;
        level = Math.max(8, level);
        xs.push(m);
        totals.push(level);
      }
      for (let i = 0; i < totals.length; i++) {
        const lo = Math.max(0, i - 2);
        const win = totals.slice(lo, i + 1);
        trend.push(win.reduce((a, b) => a + b, 0) / win.length);
      }
      plot.add_bar2d(xs, totals, T2, 'total', undefined);
      plot.add_line2d(xs, trend, T3, 2.0, 'trend', undefined);
      return {};
    },
  },
};

/* ---------- mounting ---------- */

const mounted = [];

function mountExample(card) {
  const def = EXAMPLES[card.dataset.example];
  if (!def) return;
  // A scene with an input file that did not arrive degrades on its own,
  // rather than taking the page down with it.
  if (def.requires && !def.requires()) { cardFallback(card, def.missing); return; }
  const canvas = card.querySelector('.ex-canvas');
  const tip = card.querySelector('.term-tip');
  const ctx = canvas.getContext('2d');
  const scratch = document.createElement('canvas');
  const sctx = scratch.getContext('2d');
  // A second buffer for the full-resolution legend composited over a
  // half-res drag frame — putImageData ignores alpha, drawImage does not.
  const overlay = document.createElement('canvas');
  const octx = overlay.getContext('2d');

  const plot = new Plot();
  let dirty = true, visible = true;
  const ui = { card, markDirty: () => { dirty = true; }, dragging: () => dragging };
  const api = def.setup(plot, ui) || {};
  const heavy = def.is3d && plot.vertex_count() > 400;

  let w = 1, h = 1, dpr = 1;
  let dragging = false, lastX = 0, lastY = 0;
  let hover = null; // {isEdge, index} for 3D picks

  const ro = new ResizeObserver(() => {
    dpr = Math.min(window.devicePixelRatio || 1, DPR_MAX);
    w = Math.max(1, Math.round(canvas.clientWidth * dpr));
    h = Math.max(1, Math.round(canvas.clientHeight * dpr));
    canvas.width = w;
    canvas.height = h;
    dirty = true;
  });
  ro.observe(canvas);

  const io = new IntersectionObserver((entries) => {
    visible = entries.some((e) => e.isIntersecting);
    if (visible) dirty = true;
  });
  io.observe(canvas);

  function frame() {
    if (!visible) return;
    if (api.tick) api.tick(); // a live scene may markDirty per frame
    if (!dirty) return;
    dirty = false;
    if (dragging && heavy) {
      const hw = Math.max(1, Math.round(w / 2)), hh = Math.max(1, Math.round(h / 2));
      plot.render_at(hw, hh, hw / w);
      scratch.width = hw;
      scratch.height = hh;
      sctx.putImageData(readFrame(hw, hh), 0, 0);
      ctx.clearRect(0, 0, w, h);
      ctx.imageSmoothingEnabled = true;
      ctx.drawImage(scratch, 0, 0, w, h);
      // The legend is what the eye is fixed on while dragging, so it is
      // rasterized at full resolution and composited on top. Without this the
      // downscale would drop the engine's chrome scale a step and the legend
      // would swap font, weight and padding the moment a drag started.
      plot.render_legend_overlay(w, h);
      if (overlay.width !== w || overlay.height !== h) {
        overlay.width = w;
        overlay.height = h;
      }
      octx.putImageData(readFrame(w, h), 0, 0);
      ctx.drawImage(overlay, 0, 0);
    } else {
      plot.render(w, h);
      ctx.putImageData(readFrame(w, h), 0, 0);
    }
  }

  function readFrame(fw, fh) {
    // Fresh view over wasm memory per blit; putImageData copies it out
    // before any further wasm call could grow (and detach) the buffer.
    const view = new Uint8ClampedArray(memory.buffer, plot.frame_ptr(), plot.frame_len());
    return new ImageData(view, fw, fh);
  }

  /* pointer coordinates in framebuffer (device) pixels */
  function fbCoords(e) {
    const r = canvas.getBoundingClientRect();
    return [(e.clientX - r.left) * dpr, (e.clientY - r.top) * dpr];
  }

  // Touch on 3D: one finger rotates; two fingers pan (their midpoint) and
  // pinch-zoom (their spread) — the finger analogues of shift-drag + wheel.
  const pointers = new Map(); // active pointers, id → {x, y}
  let panCX = 0, panCY = 0, pinchD = 0;
  function centroidDist() {
    const it = pointers.values();
    const a = it.next().value, b = it.next().value;
    return [(a.x + b.x) / 2, (a.y + b.y) / 2, Math.hypot(a.x - b.x, a.y - b.y)];
  }

  canvas.addEventListener('pointerdown', (e) => {
    // The legend owns its own rows: a press there shows or hides that series
    // and never becomes a drag or a pick. 2D cards get this too.
    const [lx, ly] = fbCoords(e);
    const row = plot.legend_hit(w, h, lx, ly);
    if (row !== undefined) {
      plot.toggle_muted(row);
      setHover(null, e);
      dirty = true;
      return;
    }
    if (!def.is3d) return;
    pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
    if (pointers.size === 2) [panCX, panCY, pinchD] = centroidDist();
    dragging = true;
    lastX = e.clientX;
    lastY = e.clientY;
    canvas.setPointerCapture(e.pointerId);
    setHover(null, e);
  });
  canvas.addEventListener('pointermove', (e) => {
    const p = pointers.get(e.pointerId);
    if (p) { p.x = e.clientX; p.y = e.clientY; }
    if (pointers.size >= 2) {
      const [cx, cy, d] = centroidDist();
      plot.pan((cx - panCX) * dpr, (cy - panCY) * dpr);
      if (pinchD > 0) plot.zoom_by(d / pinchD);
      panCX = cx; panCY = cy; pinchD = d;
      dirty = true;
      return;
    }
    if (dragging) {
      const dx = e.clientX - lastX, dy = e.clientY - lastY;
      // Through the input map, so the drag feel is stated once, in the core.
      plot.apply_drag(dx, dy, e.shiftKey, 0.006, dpr, 0.004);
      lastX = e.clientX;
      lastY = e.clientY;
      dirty = true;
      return;
    }
    const [px, py] = fbCoords(e);
    // Over the legend the canvas behaves like a row of buttons, not a plot.
    const overLegend = plot.legend_hit(w, h, px, py) !== undefined;
    canvas.style.cursor = overLegend ? 'pointer' : '';
    if (overLegend) {
      setHover(null, e);
      return;
    }
    if (def.pick) {
      let hit = null;
      if (def.pick === 'element') {
        const el = plot.pick_element(w, h, px, py, 8 * dpr, undefined);
        if (el !== undefined) {
          hit = { isEdge: el.is_edge, index: el.index };
          el.free();
        }
      } else {
        const i = plot.pick(w, h, px, py, 8 * dpr);
        if (i !== undefined) hit = { isEdge: false, index: i };
      }
      setHover(hit, e);
    } else if (def.hover2d) {
      if (plot.set_hover2d(px)) dirty = true;
    }
  });
  function endPointer(e) {
    pointers.delete(e.pointerId);
    if (pointers.size === 1) {
      // One finger stays down: continue it as a rotate drag from where it is.
      const rest = pointers.values().next().value;
      lastX = rest.x;
      lastY = rest.y;
      return;
    }
    if (!dragging || pointers.size > 0) return;
    dragging = false;
    dirty = true; // repaint full-res after a half-res drag
  }
  canvas.addEventListener('pointerup', endPointer);
  canvas.addEventListener('pointercancel', endPointer);
  canvas.addEventListener('pointerleave', (e) => {
    setHover(null, e);
    if (def.hover2d && plot.set_hover2d(undefined)) dirty = true;
  });
  if (def.is3d) {
    canvas.addEventListener('wheel', (e) => {
      e.preventDefault();
      plot.zoom_by(Math.exp(-e.deltaY * 0.002));
      dirty = true;
    }, { passive: false });
    canvas.addEventListener('dblclick', () => {
      plot.reset();
      dirty = true;
    });
  }

  function setHover(hit, e) {
    if (!hit && !hover) { tip.hidden = true; return; }
    const same = hover && hit && hover.isEdge === hit.isEdge && hover.index === hit.index;
    if (!same) {
      hover = hit;
      // core keeps one hovered element, so exactly one setter call
      let changed;
      if (!hit) changed = plot.set_hovered_node(undefined);
      else if (hit.isEdge) changed = plot.set_hovered_edge(hit.index);
      else changed = plot.set_hovered_node(hit.index);
      if (changed) dirty = true;
      if (api.hover) api.hover(hit); // scene-level highlight (may markDirty)
    }
    if (hit && api.tooltip) {
      tip.innerHTML = api.tooltip(hit);
      tip.hidden = false;
      const body = canvas.parentElement;
      const br = body.getBoundingClientRect();
      let tx = e.clientX - br.left + 14;
      let ty = e.clientY - br.top + 14;
      if (tx > br.width - 150) tx = e.clientX - br.left - 14 - tip.offsetWidth;
      if (ty > br.height - 90) ty = e.clientY - br.top - 14 - tip.offsetHeight;
      tip.style.left = tx + 'px';
      tip.style.top = ty + 'px';
    } else {
      tip.hidden = true;
    }
  }

  mounted.push({ frame, name: card.dataset.example, plot });
}

/* ---------- boot ---------- */

// The 2d/3d filter and the "py" code toggles work with or without wasm.
// The default (3d selected, 2d cards hidden) is baked into the HTML, so
// there is no flash of filtered-out cards before this script runs.
const dimButtons = document.querySelectorAll('.dim-toggle .dim-btn');
for (const btn of dimButtons) {
  btn.addEventListener('click', () => {
    const dim = btn.dataset.dim;
    for (const b of dimButtons) {
      b.classList.toggle('on', b === btn);
      b.setAttribute('aria-pressed', String(b === btn));
    }
    for (const card of document.querySelectorAll('.ex-card')) {
      card.hidden = dim !== 'all' && card.dataset.dim !== dim;
    }
  });
}

// The "py" code toggles work with or without wasm.
for (const btn of document.querySelectorAll('.ex-card .code-toggle')) {
  btn.addEventListener('click', () => {
    const code = btn.closest('.ex-card').querySelector('.ex-code');
    const show = code.hidden;
    code.hidden = !show;
    btn.classList.toggle('on', show);
    btn.setAttribute('aria-expanded', String(show));
  });
}

// The green title-bar "zoom" lights work with or without wasm: maximizing
// swaps the card into a fixed overlay; each canvas's ResizeObserver
// re-renders the plot at the new size. A placeholder keeps the grid from
// reflowing underneath.
const backdrop = document.createElement('div');
backdrop.className = 'ex-backdrop';
backdrop.hidden = true;
document.body.appendChild(backdrop);
let maxed = null; // { card, btn, placeholder }

function closeMax() {
  if (!maxed) return;
  maxed.card.classList.remove('max');
  maxed.btn.classList.remove('on');
  maxed.btn.setAttribute('aria-pressed', 'false');
  maxed.placeholder.remove();
  backdrop.hidden = true;
  document.body.style.overflow = '';
  maxed = null;
}

for (const btn of document.querySelectorAll('.ex-card .b-max')) {
  btn.addEventListener('click', () => {
    const card = btn.closest('.ex-card');
    if (maxed && maxed.card === card) { closeMax(); return; }
    closeMax();
    const placeholder = document.createElement('div');
    placeholder.style.height = card.getBoundingClientRect().height + 'px';
    card.before(placeholder);
    card.classList.add('max');
    btn.classList.add('on');
    btn.setAttribute('aria-pressed', 'true');
    backdrop.hidden = false;
    document.body.style.overflow = 'hidden'; // no page scroll behind the overlay
    maxed = { card, btn, placeholder };
  });
}
backdrop.addEventListener('click', closeMax);
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') closeMax();
});

function cardFallback(card, html) {
  const body = card.querySelector('.term-body');
  if (body) body.innerHTML = '<div class="ex-fallback">' + html + '</div>';
}

function showFallback() {
  for (const card of document.querySelectorAll('.ex-card')) {
    cardFallback(card, 'These live examples need WebAssembly, which failed to load here. '
      + 'The plots themselves run in any terminal — see the '
      + '<a href="https://github.com/sebaheg/plotui">GitHub README</a> for screenshots.');
  }
}

(async () => {
  // Started before the engine so the two overlap; a failure here leaves PDB
  // null and only the protein card falls back.
  const structure = fetch(new URL('./1ubq.pdb?v=1', document.baseURI))
    .then((r) => (r.ok ? r.text() : null))
    .catch(() => null);
  try {
    // ?v=1 flushes copies cached from before the site sent Cache-Control
    // headers; the explicit wasm URL keeps the pair in the same version.
    //
    // This page has exactly one consumer of the engine, so it initialises the
    // module itself. Anything else on this page that needs the engine must go
    // through js/wasm-engine.js instead: wasm-bindgen's init() builds a fresh
    // WebAssembly.Memory every call, and a second one here would leave these
    // trace handles pointing into an instance that no longer exists.
    const mod = await import('./pkg/plotui_wasm.js?v=1');
    const wasm = await mod.default({ module_or_path: new URL('./pkg/plotui_wasm_bg.wasm?v=1', document.baseURI) });
    Plot = mod.Plot;
    ForceLayout = mod.ForceLayout;
    LayeredLayout = mod.LayeredLayout;
    plot_reachable = mod.reachable;
    marching_cubes = mod.marching_cubes;
    catmull_rom = mod.catmull_rom;
    tube = mod.tube;
    ribbon = mod.ribbon;
    memory = wasm.memory;
    // Which engine build this page actually loaded. site/pkg/ is committed,
    // so it can lag the source; CI gates that, and this makes it visible
    // from the browser too.
    fetch(new URL('./pkg/BUILD_STAMP', document.baseURI))
      .then((r) => (r.ok ? r.text() : null))
      .then((v) => v && console.info('plotui engine build', v.trim()))
      .catch(() => {});
  } catch (e) {
    console.error('plotui wasm failed to load:', e);
    showFallback();
    return;
  }
  PDB = await structure;
  document.querySelectorAll('.ex-card').forEach(mountExample);
  // QA hook: `__plotuiCards.protein.plot.set_input_map('-yaw', '-pitch')`
  // flips the drag feel on one card live, for comparing the two by hand.
  window.__plotuiCards = Object.fromEntries(mounted.map((m) => [m.name, m]));
  (function tick() {
    for (const m of mounted) m.frame();
    requestAnimationFrame(tick);
  })();
})();
