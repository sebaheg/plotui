// stream-demo.js — the streaming section's fallback mockup.
//
// Hand-drawn Canvas 2D, not plotui. stream-live.js drives this section with
// the real engine and injects this file only if the wasm module fails to
// load, so what ships is a degraded picture rather than an empty frame.
(function () {
  var canvas = document.getElementById('stream2d');
  if (!canvas) return;
  var ctx = canvas.getContext('2d');
  var css = getComputedStyle(document.documentElement);
  var C1 = css.getPropertyValue('--trace-1').trim();
  var C2 = css.getPropertyValue('--trace-2').trim();
  var C3 = css.getPropertyValue('--trace-3').trim();
  var GRID = css.getPropertyValue('--grid').trim();
  var FRAME = css.getPropertyValue('--frame').trim();
  var INK = css.getPropertyValue('--ink').trim();
  var reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

  // The same feed as examples/textual_stream.py: one point per series per
  // tick, 20 Hz, appended through trace handles — never a rebuild.
  var STEP = 0.25, WINDOW = 44, TICK_MS = 50;
  var t = 0;
  var ts = [], forecast = [], observed = [], load = [];
  var hidden = [false, false, false]; // forecast, observed, load(y2)

  function base(t) { return Math.sin(t * 0.4) * 2 + Math.sin(t * 0.09) * 4; }
  function noise() { return (Math.random() + Math.random() + Math.random() + Math.random() - 2) * 0.55; }
  function push() {
    t += STEP;
    ts.push(t);
    var b = base(t);
    forecast.push(b);
    observed.push(b + noise());
    load.push(40 + 12 * Math.sin(t * 0.13 + 1));
    var lo = t - WINDOW - STEP;
    while (ts.length && ts[0] < lo) { ts.shift(); forecast.shift(); observed.shift(); load.shift(); }
  }

  function fmt(v) {
    var a = Math.abs(v);
    var s = a >= 100 ? v.toFixed(0) : a >= 10 ? v.toFixed(1) : v.toFixed(2);
    return s.indexOf('.') >= 0 ? s.replace(/\.?0+$/, '') : s;
  }
  function niceTicks(lo, hi, n) {
    var span = (hi - lo) || 1;
    var step = Math.pow(10, Math.floor(Math.log(span / n) / Math.LN10));
    var err = span / n / step;
    if (err >= 7.5) step *= 10; else if (err >= 3.5) step *= 5; else if (err >= 1.5) step *= 2;
    var out = [];
    for (var v = Math.ceil(lo / step) * step; v <= hi + step * 1e-6; v += step) out.push(v);
    return out;
  }
  function range(arrs) {
    var lo = Infinity, hi = -Infinity;
    arrs.forEach(function (a) {
      for (var i = 0; i < a.length; i++) { lo = Math.min(lo, a[i]); hi = Math.max(hi, a[i]); }
    });
    if (lo === Infinity) { lo = -1; hi = 1; }
    var pad = (hi - lo) > 0 ? (hi - lo) * 0.08 : 1;
    return [lo - pad, hi + pad];
  }

  var w = 0, h = 0, dpr = 1;
  function draw() {
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    var hasY2 = !hidden[2];
    // Top margin clears the shell-prompt overlay line.
    var x0 = 38, y0 = 30, x1 = w - (hasY2 ? 44 : 12), y1 = h - 20;

    var xhi = Math.max(WINDOW, t), xlo = xhi - WINDOW;
    var prim = [];
    if (!hidden[0]) prim.push(forecast);
    if (!hidden[1]) prim.push(observed);
    var yr = range(prim), ylo = yr[0], yhi = yr[1];
    var y2r = range(hasY2 ? [load] : []), y2lo = y2r[0], y2hi = y2r[1];

    var sx = function (v) { return x0 + (x1 - x0) * (v - xlo) / (xhi - xlo); };
    var sy = function (v) { return y1 - (y1 - y0) * (v - ylo) / (yhi - ylo); };
    var sy2 = function (v) { return y1 - (y1 - y0) * (v - y2lo) / (y2hi - y2lo); };

    ctx.font = '10px ui-monospace, monospace';
    // Horizontal grid + left labels (the grid belongs to the primary axis).
    var yt = niceTicks(ylo, yhi, 5);
    yt.forEach(function (v) {
      var py = sy(v);
      if (py <= y0 || py >= y1) return;
      ctx.strokeStyle = GRID; ctx.globalAlpha = .6;
      ctx.beginPath(); ctx.moveTo(x0, py); ctx.lineTo(x1, py); ctx.stroke();
      ctx.globalAlpha = 1;
      ctx.fillStyle = INK; ctx.textAlign = 'right'; ctx.textBaseline = 'middle';
      ctx.fillText(fmt(v), x0 - 6, py);
    });
    // Open L frame, plus the right rule only while a y2 series is visible.
    ctx.strokeStyle = FRAME; ctx.lineWidth = 1;
    ctx.beginPath(); ctx.moveTo(x0, y0); ctx.lineTo(x0, y1); ctx.lineTo(x1, y1); ctx.stroke();
    if (hasY2) {
      ctx.beginPath(); ctx.moveTo(x1, y0); ctx.lineTo(x1, y1); ctx.stroke();
      ctx.fillStyle = C3; ctx.textAlign = 'left';
      niceTicks(y2lo, y2hi, 5).forEach(function (v) {
        var py = sy2(v);
        if (py < y0 || py > y1) return;
        ctx.fillText(fmt(v), x1 + 6, py);
      });
    }
    // Sliding x ticks.
    ctx.fillStyle = INK; ctx.textAlign = 'center'; ctx.textBaseline = 'top';
    niceTicks(xlo, xhi, 6).forEach(function (v) {
      var px = sx(v);
      if (px < x0 || px > x1) return;
      ctx.fillText(fmt(v), px, y1 + 6);
    });

    ctx.save();
    ctx.beginPath(); ctx.rect(x0 + 1, y0, x1 - x0 - 2, y1 - y0); ctx.clip();
    if (!hidden[2]) {
      ctx.strokeStyle = C3; ctx.lineWidth = 1.4;
      ctx.beginPath();
      for (var k = 0; k < ts.length; k++) {
        if (k === 0) ctx.moveTo(sx(ts[k]), sy2(load[k])); else ctx.lineTo(sx(ts[k]), sy2(load[k]));
      }
      ctx.stroke();
    }
    if (!hidden[0]) {
      ctx.strokeStyle = C2; ctx.lineWidth = 2;
      ctx.beginPath();
      for (var i = 0; i < ts.length; i++) {
        if (i === 0) ctx.moveTo(sx(ts[i]), sy(forecast[i])); else ctx.lineTo(sx(ts[i]), sy(forecast[i]));
      }
      ctx.stroke();
    }
    if (!hidden[1]) {
      ctx.fillStyle = C1;
      for (var j = 0; j < ts.length; j++) {
        ctx.beginPath(); ctx.arc(sx(ts[j]), sy(observed[j]), 2.1, 0, Math.PI * 2); ctx.fill();
      }
    }
    // The freshest point gets a bright cap, so the feed reads as *arriving*.
    if (ts.length && !hidden[0]) {
      var lastX = sx(ts[ts.length - 1]), lastY = sy(forecast[forecast.length - 1]);
      ctx.fillStyle = '#fff';
      ctx.beginPath(); ctx.arc(lastX, lastY, 3, 0, Math.PI * 2); ctx.fill();
    }
    ctx.restore();
  }

  function resize() {
    dpr = Math.min(window.devicePixelRatio || 1, 2);
    w = canvas.clientWidth; h = canvas.clientHeight;
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    draw();
  }

  // Reduced motion: the window arrives pre-filled and holds still.
  var prefill = reduced ? Math.ceil(WINDOW / STEP) + 4 : 10;
  for (var p = 0; p < prefill; p++) push();

  var legendItems = document.querySelectorAll('#stream-legend .li');
  Array.prototype.forEach.call(legendItems, function (item) {
    item.addEventListener('click', function () {
      var i = Number(item.dataset.trace);
      hidden[i] = !hidden[i];
      item.classList.toggle('off', hidden[i]);
      item.setAttribute('aria-pressed', String(!hidden[i]));
      draw();
    });
  });

  new ResizeObserver(resize).observe(canvas);
  resize();

  if (!reduced) {
    var acc = 0, last = null;
    (function tick(now) {
      requestAnimationFrame(tick);
      if (document.hidden) { last = null; return; }
      if (last == null) { last = now; return; }
      acc += Math.min(250, now - last); // cap catch-up after a hidden tab
      last = now;
      var moved = false;
      while (acc >= TICK_MS) { push(); acc -= TICK_MS; moved = true; }
      if (moved) draw();
    })(performance.now());
  }
})();
