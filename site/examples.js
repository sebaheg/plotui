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
    if (!visible || !dirty) return;
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
