// stream-live.js — the streaming section, drawn by the real engine.
//
// The footer under this demo advertises `widget.extend` and `set_visible`;
// this file is what makes that claim true. Points are appended through trace
// handles at 20 Hz — never a rebuild — the legend calls `set_visible`, and the
// view slides by moving the engine's own x window rather than by dropping
// data on the floor.
//
// If the wasm module fails to load, the hand-drawn mockup (stream-demo.js) is
// injected as a fallback, the same way gallery-live.js falls back.

import { engine } from './wasm-engine.js';

const DPR_MAX = 2;
const STEP = 0.25;     // data units per tick
const WINDOW = 44;     // data units visible at once
const TICK_MS = 50;    // 20 Hz, matching examples/textual_stream.py

// Appending forever would grow the buffers without bound on a tab left open
// overnight. Past this the plot is rebuilt once from the visible tail: a
// compaction every few minutes, not a per-frame rebuild — which would be the
// very thing this section exists to say you don't need.
const MAX_POINTS = 4000;

function showFallback() {
  const s = document.createElement('script');
  s.src = 'js/stream-demo.js';
  document.body.appendChild(s);
}

/* ---------- the feed (the same scene the mockup drew) ---------- */

function base(t) {
  return Math.sin(t * 0.4) * 2 + Math.sin(t * 0.09) * 4;
}

// Sum of four uniforms: a cheap bell, so "observed" scatters around the
// forecast the way a measurement does rather than jittering uniformly.
function noise() {
  return (Math.random() + Math.random() + Math.random() + Math.random() - 2) * 0.55;
}

(async () => {
  const canvas = document.getElementById('stream2d');
  if (!canvas) return;

  let Plot, memory;
  try {
    ({ Plot, memory } = await engine());
  } catch (e) {
    console.error('plotui wasm failed to load, falling back to the mockup:', e);
    showFallback();
    return;
  }

  const css = getComputedStyle(document.documentElement);
  const C1 = css.getPropertyValue('--trace-1').trim();
  const C2 = css.getPropertyValue('--trace-2').trim();
  const C3 = css.getPropertyValue('--trace-3').trim();
  const reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

  const ctx = canvas.getContext('2d');
  let w = 1, h = 1, dpr = 1, dirty = true, visible = true;

  function readFrame(plot, fw, fh) {
    // Fresh view over wasm memory per blit; putImageData copies it out before
    // any further wasm call could grow (and detach) the buffer.
    const view = new Uint8ClampedArray(memory.buffer, plot.frame_ptr(), plot.frame_len());
    return new ImageData(view, fw, fh);
  }

  /* ---------- the plot ---------- */

  let plot, hF, hO, hL, t, len;
  // Series are unnamed on purpose: the section has its own HTML legend above
  // the canvas, and a second one inside the frame would say the same thing
  // twice.
  function build(seedTs, seedF, seedO, seedL) {
    plot = new Plot();
    hF = plot.add_line2d(seedTs, seedF, C2, 2.0, undefined, undefined);
    hO = plot.add_scatter2d(seedTs, seedO, C1, 2.1, undefined, undefined);
    hL = plot.add_line2d(seedTs, seedL, C3, 1.4, undefined, 'y2');
    len = seedTs.length;
    for (let i = 0; i < hidden.length; i++) {
      if (hidden[i]) plot.set_visible([hF, hO, hL][i], false);
    }
  }

  const hidden = [false, false, false]; // forecast, observed, load(y2)

  // The tail we keep so a compaction can reseed the visible window exactly.
  const tailTs = [], tailF = [], tailO = [], tailL = [];

  function push() {
    t += STEP;
    const b = base(t);
    const f = b;
    const o = b + noise();
    const l = 40 + 12 * Math.sin(t * 0.13 + 1);
    plot.extend_xy(hF, new Float32Array([t]), new Float32Array([f]));
    plot.extend_xy(hO, new Float32Array([t]), new Float32Array([o]));
    plot.extend_xy(hL, new Float32Array([t]), new Float32Array([l]));
    len++;
    tailTs.push(t); tailF.push(f); tailO.push(o); tailL.push(l);
    // Keep a little more than one window, so the reseed has a full screen.
    const keep = Math.ceil(WINDOW / STEP) + 8;
    while (tailTs.length > keep) {
      tailTs.shift(); tailF.shift(); tailO.shift(); tailL.shift();
    }
    if (len > MAX_POINTS) {
      build(
        new Float32Array(tailTs), new Float32Array(tailF),
        new Float32Array(tailO), new Float32Array(tailL),
      );
    }
    // The window is the view: the data stays put and the frame moves over it.
    plot.set_x_window(t - WINDOW, t);
  }

  t = 0;
  build(new Float32Array(0), new Float32Array(0), new Float32Array(0), new Float32Array(0));
  // Reduced motion: the window arrives pre-filled and then holds still.
  const prefill = reduced ? Math.ceil(WINDOW / STEP) + 4 : 10;
  for (let i = 0; i < prefill; i++) push();

  /* ---------- legend → set_visible ---------- */

  for (const item of document.querySelectorAll('#stream-legend .li')) {
    item.addEventListener('click', () => {
      const i = Number(item.dataset.trace);
      hidden[i] = !hidden[i];
      item.classList.toggle('off', hidden[i]);
      item.setAttribute('aria-pressed', String(!hidden[i]));
      plot.set_visible([hF, hO, hL][i], !hidden[i]);
      dirty = true;
    });
  }

  /* ---------- canvas plumbing ---------- */

  new ResizeObserver(() => {
    dpr = Math.min(window.devicePixelRatio || 1, DPR_MAX);
    w = Math.max(1, Math.round(canvas.clientWidth * dpr));
    h = Math.max(1, Math.round(canvas.clientHeight * dpr));
    canvas.width = w;
    canvas.height = h;
    dirty = true;
  }).observe(canvas);

  new IntersectionObserver((entries) => {
    visible = entries.some((en) => en.isIntersecting);
    if (visible) dirty = true;
  }).observe(canvas);

  function draw() {
    plot.render(w, h);
    ctx.putImageData(readFrame(plot, w, h), 0, 0);
  }

  let acc = 0, last = null;
  (function tick(now) {
    requestAnimationFrame(tick);
    // A hidden tab stops the clock rather than banking time it would then
    // fast-forward through on return.
    if (document.hidden || !visible) { last = null; return; }
    if (!reduced) {
      if (last == null) { last = now; }
      else {
        acc += Math.min(250, now - last); // cap catch-up after a hidden tab
        last = now;
        while (acc >= TICK_MS) { push(); acc -= TICK_MS; dirty = true; }
      }
    }
    if (!dirty) return;
    dirty = false;
    draw();
  })(performance.now());
})();
