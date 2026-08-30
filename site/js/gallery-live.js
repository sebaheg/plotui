// gallery-live.js — the Examples section, drawn by the real engine.
//
// Both gallery plots are rendered by plotui-core compiled to WebAssembly,
// the same way examples.js mounts the example cards: pointer events drive
// the engine's camera and 2D crosshair, and each frame is the engine's RGBA
// bytes blitted onto a canvas. The RGBA view into wasm memory is rebuilt at
// every blit, so a wasm memory growth can never leave a stale buffer behind.
//
// If the wasm module fails to load, the hand-drawn mockup renderers
// (gallery-surface3d.js / gallery-chart2d.js) are injected as a fallback.

const DPR_MAX = 2;

function showFallback() {
  for (const src of ['js/gallery-surface3d.js', 'js/gallery-chart2d.js']) {
    const s = document.createElement('script');
    s.src = src;
    document.body.appendChild(s);
  }
}

/* ---------- deterministic data (same scenes the mockups drew) ---------- */

function mulberry32(a) {
  return function () {
    a |= 0; a = a + 0x6D2B79F5 | 0;
    let t = Math.imul(a ^ a >>> 15, 1 | a);
    t = t + Math.imul(t ^ t >>> 7, 61 | t) ^ t;
    return ((t ^ t >>> 14) >>> 0) / 4294967296;
  };
}

function peaksTerrain() {
  // Gaussian peaks — the hillclimb terrain, on the viridis ramp.
  const PEAKS = [[0.15, -0.10, 0.95, 0.35], [-0.75, 0.55, 0.48, 0.16], [0.55, 0.85, 0.25, 0.12]];
  const EXT = 1.25, N = 48;
  const xs = [], ys = [], zs = [];
  for (let i = 0; i < N; i++) xs.push(-EXT + 2 * EXT * i / (N - 1));
  for (let j = 0; j < N; j++) ys.push(-EXT + 2 * EXT * j / (N - 1));
  for (let j = 0; j < N; j++) {
    for (let i = 0; i < N; i++) {
      let h = 0;
      for (const [px, py, amp, sig] of PEAKS) {
        const dx = xs[i] - px, dy = ys[j] - py;
        h += amp * Math.exp(-(dx * dx + dy * dy) / (2 * sig * sig));
      }
      zs.push(h);
    }
  }
  return { xs, ys, zs };
}

function energyData() {
  const rnd = mulberry32(7);
  const HOURS = 48;
  const hours = [], observed = [], forecast = [];
  for (let i = 0; i < HOURS; i++) {
    const base = 42 + 26 * Math.sin(i / 7.6) + 9 * Math.sin(i / 2.9);
    hours.push(i);
    observed.push(base + (rnd() - .5) * 9);
    forecast.push(base + (rnd() - .5) * 3.5);
  }
  const MONTHS = 12;
  const months = [], solar = [], wind = [], hydro = [];
  for (let m = 0; m < MONTHS; m++) {
    months.push(m + 1);
    solar.push(14 + 30 * Math.max(0, Math.sin((m + .5) / 12 * Math.PI)) + rnd() * 5);
    wind.push(30 + 16 * Math.cos((m + .5) / 12 * Math.PI * 2) + rnd() * 6);
    hydro.push(20 + 7 * Math.sin((m + 3) / 12 * Math.PI * 2) + rnd() * 3);
  }
  return { hours, observed, forecast, months, solar, wind, hydro };
}

/* ---------- boot ---------- */

(async () => {
  let Plot, memory;
  try {
    const mod = await import('../pkg/plotui_wasm.js');
    const wasm = await mod.default();
    Plot = mod.Plot;
    memory = wasm.memory;
  } catch (e) {
    console.error('plotui wasm failed to load, falling back to mockups:', e);
    showFallback();
    return;
  }

  const css = getComputedStyle(document.documentElement);
  const C1 = css.getPropertyValue('--trace-1').trim();
  const C2 = css.getPropertyValue('--trace-2').trim();
  const C3 = css.getPropertyValue('--trace-3').trim();
  const reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  const mounted = [];

  // Shared canvas plumbing: sizing, visibility, and the wasm-frame blit.
  function mountCanvas(canvas) {
    const ctx = canvas.getContext('2d');
    const scratch = document.createElement('canvas');
    const sctx = scratch.getContext('2d');
    const m = {
      canvas, w: 1, h: 1, dpr: 1, dirty: true, visible: true,
      markDirty() { m.dirty = true; },
      blit(plot, halfRes) {
        if (halfRes) {
          const hw = Math.max(1, Math.round(m.w / 2)), hh = Math.max(1, Math.round(m.h / 2));
          plot.render_at(hw, hh, hw / m.w);
          scratch.width = hw;
          scratch.height = hh;
          sctx.putImageData(readFrame(plot, hw, hh), 0, 0);
          ctx.clearRect(0, 0, m.w, m.h);
          ctx.imageSmoothingEnabled = true;
          ctx.drawImage(scratch, 0, 0, m.w, m.h);
        } else {
          plot.render(m.w, m.h);
          ctx.putImageData(readFrame(plot, m.w, m.h), 0, 0);
        }
      },
      fbCoords(e) {
        const r = canvas.getBoundingClientRect();
        return [(e.clientX - r.left) * m.dpr, (e.clientY - r.top) * m.dpr];
      },
    };
    new ResizeObserver(() => {
      m.dpr = Math.min(window.devicePixelRatio || 1, DPR_MAX);
      m.w = Math.max(1, Math.round(canvas.clientWidth * m.dpr));
      m.h = Math.max(1, Math.round(canvas.clientHeight * m.dpr));
      canvas.width = m.w;
      canvas.height = m.h;
      m.dirty = true;
    }).observe(canvas);
    new IntersectionObserver((entries) => {
      m.visible = entries.some((en) => en.isIntersecting);
      if (m.visible) m.dirty = true;
    }).observe(canvas);
    return m;
  }

  function readFrame(plot, fw, fh) {
    // Fresh view over wasm memory per blit; putImageData copies it out
    // before any further wasm call could grow (and detach) the buffer.
    const view = new Uint8ClampedArray(memory.buffer, plot.frame_ptr(), plot.frame_len());
    return new ImageData(view, fw, fh);
  }

  /* ---- the 3D surface ---- */

  const surfCanvas = document.getElementById('surf3d');
  if (surfCanvas) {
    const m = mountCanvas(surfCanvas);
    const plot = new Plot();
    const g = peaksTerrain();
    plot.add_surface3d(g.xs, g.ys, g.zs, undefined, 'viridis', false, undefined);
    // A gentle 3/4 view over the z-up terrain (the engine's turntable spins
    // about z, so horizontal drags orbit this view). Double-click restores it.
    const HOME = [0.55, 0.5, 1.0, 0, 0];
    plot.set_camera_state(HOME);
    const heavy = plot.vertex_count() > 400;
    const tip = document.getElementById('surf-tip');
    let dragging = false, touched = false, lastX = 0, lastY = 0, t = 0;
    let inside = false; // pointer over the canvas: sway pauses, hover picks
    let hover = null; // [x, y, z, x_px, y_px] from pick_surface, or null
    let pinned = null; // clicked [x, y, z]: tooltip stays through camera moves
    let downX = 0, downY = 0;

    function clearHover() {
      if (!hover) return;
      hover = null;
      plot.set_surface_hover(null);
      m.markDirty();
    }

    surfCanvas.addEventListener('pointerdown', (e) => {
      dragging = true;
      touched = true; // the visitor has the camera now; stop the idle sway
      lastX = downX = e.clientX;
      lastY = downY = e.clientY;
      clearHover(); // the tooltip yields to the camera grab
      surfCanvas.classList.add('dragging');
      surfCanvas.setPointerCapture(e.pointerId);
    });
    surfCanvas.addEventListener('pointermove', (e) => {
      if (dragging) {
        // Routed through the plot's input map (drag rotates, shift-drag
        // pans, by default) — remappable per plot via set_input_map.
        plot.apply_drag(e.clientX - lastX, e.clientY - lastY, e.shiftKey, 0.006, m.dpr, 0.004);
        lastX = e.clientX;
        lastY = e.clientY;
        m.markDirty();
        return;
      }
      if (pinned) return; // the pinned tooltip owns the guides until unpinned
      // While the pointer is inside, the sway is paused (below), so the
      // picked vertex stays put under the cursor. The engine draws the hover
      // guides (ring, floor shadow, axis guide lines); JS only owns the
      // tooltip.
      const [fx, fy] = m.fbCoords(e);
      const hit = plot.pick_surface(m.w, m.h, fx, fy, 14 * m.dpr) || null;
      if (plot.set_surface_hover(hit ? [hit[0], hit[1], hit[2]] : null)) m.markDirty();
      hover = hit;
    });
    surfCanvas.addEventListener('pointerup', (e) => {
      if (!dragging) return;
      dragging = false;
      surfCanvas.classList.remove('dragging');
      // A press released without real movement is a click: pin the point
      // under it, or unpin when it lands on empty space.
      if (Math.hypot(e.clientX - downX, e.clientY - downY) < 6) {
        const [fx, fy] = m.fbCoords(e);
        const hit = plot.pick_surface(m.w, m.h, fx, fy, 14 * m.dpr) || null;
        pinned = hit ? [hit[0], hit[1], hit[2]] : null;
        plot.set_surface_selected(pinned);
      }
      m.markDirty(); // repaint full-res after a half-res drag
    });
    surfCanvas.addEventListener('pointerenter', () => { inside = true; });
    surfCanvas.addEventListener('pointerleave', () => {
      inside = false;
      clearHover();
    });
    surfCanvas.addEventListener('dblclick', () => {
      plot.set_camera_state(HOME);
      m.markDirty();
    });

    mounted.push({
      frame() {
        if (!reduced && !touched && !inside && m.visible && !document.hidden) {
          t += 1 / 60; // gentle yaw sway about the home view
          plot.set_camera_state([HOME[0] + 0.12 * Math.sin(t * 0.5), HOME[1], HOME[2], HOME[3], HOME[4]]);
          m.dirty = true;
        }
        if (!m.visible || !m.dirty) return;
        m.dirty = false;
        m.blit(plot, dragging && heavy);
        // The engine drew the guides into the frame itself; JS just parks
        // the tooltip beside the shown point. A pinned point is reprojected
        // every frame so the tooltip follows it through rotation and pan.
        let show = null;
        if (pinned) {
          const s = plot.project_point(m.w, m.h, pinned[0], pinned[1], pinned[2]);
          show = [pinned[0], pinned[1], pinned[2], s[0], s[1]];
        } else if (hover) {
          show = hover;
        }
        if (show && tip) {
          const cx = show[3] / m.dpr, cy = show[4] / m.dpr;
          tip.innerHTML = 'x ' + show[0].toFixed(2)
            + '<br>y ' + show[1].toFixed(2)
            + '<br>z ' + show[2].toFixed(2);
          tip.hidden = false;
          let tx = surfCanvas.offsetLeft + cx + 14;
          let ty = surfCanvas.offsetTop + cy + 14;
          if (cx > surfCanvas.clientWidth - 110) tx = surfCanvas.offsetLeft + cx - 14 - tip.offsetWidth;
          if (cy > surfCanvas.clientHeight - 80) ty = surfCanvas.offsetTop + cy - 14 - tip.offsetHeight;
          tip.style.left = tx + 'px';
          tip.style.top = ty + 'px';
        } else if (tip) {
          tip.hidden = true;
        }
      },
    });
  }

  /* ---- the 2D chart types ---- */

  const chartCanvas = document.getElementById('chart2d');
  if (chartCanvas) {
    const m = mountCanvas(chartCanvas);
    const d = energyData();

    // One engine plot per chart type; the buttons switch which one renders.
    const plots = {
      line: (p) => {
        p.add_line2d(d.hours, d.observed, C1, 2.0, 'observed', undefined);
        p.add_line2d(d.hours, d.forecast, C2, 2.0, 'forecast', undefined);
      },
      scatter: (p) => {
        p.add_scatter2d(d.hours, d.observed, C1, 2.5, 'observed', undefined);
        p.add_scatter2d(d.hours, d.forecast, C2, 2.5, 'forecast', undefined);
      },
      bar: (p) => {
        p.add_bar2d(d.months, d.wind, C2, 'wind', undefined);
      },
      stacked: (p) => {
        // The painter's trick the API uses for stacks: totals first,
        // then the shorter cumulative bars on top.
        const sw = d.solar.map((v, i) => v + d.wind[i]);
        const total = sw.map((v, i) => v + d.hydro[i]);
        p.add_bar2d(d.months, total, C3, 'hydro', undefined);
        p.add_bar2d(d.months, sw, C2, 'wind', undefined);
        p.add_bar2d(d.months, d.solar, C1, 'solar', undefined);
      },
    };
    for (const kind of Object.keys(plots)) {
      const p = new Plot();
      plots[kind](p);
      plots[kind] = p;
    }
    let active = plots.line;

    chartCanvas.addEventListener('pointermove', (e) => {
      if (active.set_hover2d(m.fbCoords(e)[0])) m.markDirty();
    });
    chartCanvas.addEventListener('pointerleave', () => {
      if (active.set_hover2d(undefined)) m.markDirty();
    });

    const TITLES = {
      line: 'plot.add_line(hours, mw, name="observed")',
      scatter: 'plot.add_scatter(hours, mw, name="observed")',
      bar: 'plot.add_bar(months, mwh, name="wind")',
      stacked: 'plot.add_bar(months, totals)  # tallest first',
    };
    const title = document.getElementById('chart2d-title');
    const buttons = document.querySelectorAll('#gallery .term-foot .btn');
    for (const btn of buttons) {
      btn.addEventListener('click', () => {
        active.set_hover2d(undefined);
        active = plots[btn.dataset.kind];
        for (const b of buttons) b.classList.toggle('on', b === btn);
        if (title) title.textContent = TITLES[btn.dataset.kind];
        m.markDirty();
      });
    }

    mounted.push({
      frame() {
        if (!m.visible || !m.dirty) return;
        m.dirty = false;
        m.blit(active, false);
      },
    });
  }

  (function tick() {
    for (const mm of mounted) mm.frame();
    requestAnimationFrame(tick);
  })();
})();
