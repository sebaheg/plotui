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
let memory = null;
const DPR_MAX = 2;
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
  const canvas = card.querySelector('.ex-canvas');
  const tip = card.querySelector('.term-tip');
  const ctx = canvas.getContext('2d');
  const scratch = document.createElement('canvas');
  const sctx = scratch.getContext('2d');

  const plot = new Plot();
  let dirty = true, visible = true;
  const ui = { card, markDirty: () => { dirty = true; } };
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

  canvas.addEventListener('pointerdown', (e) => {
    if (!def.is3d) return;
    dragging = true;
    lastX = e.clientX;
    lastY = e.clientY;
    canvas.setPointerCapture(e.pointerId);
    setHover(null, e);
  });
  canvas.addEventListener('pointermove', (e) => {
    if (dragging) {
      const dx = e.clientX - lastX, dy = e.clientY - lastY;
      if (e.shiftKey) plot.pan(dx * dpr, dy * dpr);
      else plot.rotate(-dx * 0.006, -dy * 0.006);
      lastX = e.clientX;
      lastY = e.clientY;
      dirty = true;
      return;
    }
    const [px, py] = fbCoords(e);
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
  canvas.addEventListener('pointerup', () => {
    if (!dragging) return;
    dragging = false;
    dirty = true; // repaint full-res after a half-res drag
  });
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

  mounted.push({ frame });
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

function showFallback() {
  for (const card of document.querySelectorAll('.ex-card .term-body')) {
    card.innerHTML = '<div class="ex-fallback">These live examples need WebAssembly, which '
      + 'failed to load here. The plots themselves run in any terminal — see the '
      + '<a href="https://github.com/sebaheg/plotui">GitHub README</a> for screenshots.</div>';
  }
}

(async () => {
  try {
    const mod = await import('./pkg/plotui_wasm.js');
    const wasm = await mod.default();
    Plot = mod.Plot;
    ForceLayout = mod.ForceLayout;
    memory = wasm.memory;
  } catch (e) {
    console.error('plotui wasm failed to load:', e);
    showFallback();
    return;
  }
  document.querySelectorAll('.ex-card').forEach(mountExample);
  (function tick() {
    for (const m of mounted) m.frame();
    requestAnimationFrame(tick);
  })();
})();
