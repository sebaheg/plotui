(function () {
  var canvas = document.getElementById('chart2d');
  if (!canvas) return;
  var ctx = canvas.getContext('2d');
  var css = getComputedStyle(document.documentElement);
  var C1 = css.getPropertyValue('--trace-1').trim();
  var C2 = css.getPropertyValue('--trace-2').trim();
  var GRID = css.getPropertyValue('--grid').trim();
  var FRAME = css.getPropertyValue('--frame').trim();
  var INK = css.getPropertyValue('--ink').trim();
  var BRIGHT = css.getPropertyValue('--bright').trim();

  function mulberry32(a) {
    return function () {
      a |= 0; a = a + 0x6D2B79F5 | 0;
      var t = Math.imul(a ^ a >>> 15, 1 | a);
      t = t + Math.imul(t ^ t >>> 7, 61 | t) ^ t;
      return ((t ^ t >>> 14) >>> 0) / 4294967296;
    };
  }
  var rnd = mulberry32(7);
  var HOURS = 48;
  var observed = [], forecast = [];
  for (var i = 0; i < HOURS; i++) {
    var base = 42 + 26 * Math.sin(i / 7.6) + 9 * Math.sin(i / 2.9);
    observed.push(base + (rnd() - .5) * 9);
    forecast.push(base + (rnd() - .5) * 3.5);
  }

  var hoverI = null; // snapped sample index, like the widget's crosshair
  var w = 0, h = 0, dpr = 1;

  function fmt(v) {
    var a = Math.abs(v);
    var s = a >= 100 ? v.toFixed(0) : a >= 10 ? v.toFixed(1) : v.toFixed(2);
    return s.indexOf('.') >= 0 ? s.replace(/\.?0+$/, '') : s;
  }

  function readout(px, header, rows, x0, x1, y0) {
    ctx.font = '10.5px ui-monospace, monospace';
    var wmax = ctx.measureText(header).width;
    rows.forEach(function (r) {
      wmax = Math.max(wmax, ctx.measureText(r[0] + '  ' + fmt(r[2])).width);
    });
    var bw = wmax + 32, bh = (rows.length + 1) * 16 + 8;
    var bx = px + 10;
    if (bx + bw > x1) bx = Math.max(x0 + 2, px - 10 - bw);
    var by = y0 + 6;
    ctx.fillStyle = 'rgba(11, 14, 17, .92)';
    ctx.fillRect(bx, by, bw, bh);
    ctx.strokeStyle = GRID; ctx.strokeRect(bx + .5, by + .5, bw - 1, bh - 1);
    ctx.textAlign = 'left'; ctx.textBaseline = 'middle';
    ctx.fillStyle = INK;
    ctx.fillText(header, bx + 8, by + 12);
    rows.forEach(function (r, i) {
      ctx.fillStyle = r[1];
      ctx.fillRect(bx + 8, by + 24 + i * 16, 8, 8);
      ctx.fillStyle = BRIGHT;
      ctx.fillText(r[0] + '  ' + fmt(r[2]), bx + 22, by + 28 + i * 16);
    });
  }

  function marker(px, py, color) {
    ctx.fillStyle = '#fff';
    ctx.beginPath(); ctx.arc(px, py, 4.4, 0, Math.PI * 2); ctx.fill();
    ctx.fillStyle = color;
    ctx.beginPath(); ctx.arc(px, py, 2.8, 0, Math.PI * 2); ctx.fill();
  }

  function axes(x0, y0, x1, y1, ymax) {
    ctx.strokeStyle = FRAME; ctx.lineWidth = 1;
    ctx.beginPath(); ctx.moveTo(x0, y0); ctx.lineTo(x0, y1); ctx.lineTo(x1, y1); ctx.stroke();
    ctx.font = '10px ui-monospace, monospace';
    ctx.fillStyle = INK; ctx.textAlign = 'right'; ctx.textBaseline = 'middle';
    for (var t = 0; t <= 4; t++) {
      var v = ymax * t / 4, y = y1 - (y1 - y0) * t / 4;
      ctx.strokeStyle = GRID; ctx.globalAlpha = t === 0 ? 0 : .5;
      ctx.beginPath(); ctx.moveTo(x0, y); ctx.lineTo(x1, y); ctx.stroke();
      ctx.globalAlpha = 1;
      ctx.fillText(String(Math.round(v)), x0 - 6, y);
    }
  }

  function legend(entries, x1, y0) {
    ctx.font = '10.5px ui-monospace, monospace';
    var wmax = 0;
    entries.forEach(function (e) { wmax = Math.max(wmax, ctx.measureText(e[0]).width); });
    var bw = wmax + 30, bh = entries.length * 16 + 8;
    var bx = x1 - bw - 6, by = y0 + 6;
    ctx.fillStyle = 'rgba(11, 14, 17, .82)';
    ctx.fillRect(bx, by, bw, bh);
    ctx.strokeStyle = GRID; ctx.strokeRect(bx + .5, by + .5, bw - 1, bh - 1);
    entries.forEach(function (e, i) {
      ctx.fillStyle = e[1];
      ctx.fillRect(bx + 8, by + 8 + i * 16, 8, 8);
      ctx.fillStyle = BRIGHT; ctx.textAlign = 'left'; ctx.textBaseline = 'middle';
      ctx.fillText(e[0], bx + 22, by + 12 + i * 16);
    });
  }

  function draw() {
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    var x0 = 34, y0 = 12, x1 = w - 10, y1 = h - 18;
    var ymax = 90;
    axes(x0, y0, x1, y1, ymax);
    var sx = function (i) { return x0 + (x1 - x0) * i / (HOURS - 1); };
    var sy = function (v) { return y1 - (y1 - y0) * v / ymax; };
    var series = [['observed', C1, observed], ['forecast', C2, forecast]];
    series.forEach(function (s) {
      ctx.strokeStyle = s[1]; ctx.lineWidth = 2;
      ctx.beginPath();
      for (var i2 = 0; i2 < HOURS; i2++) {
        if (i2 === 0) ctx.moveTo(sx(i2), sy(s[2][i2])); else ctx.lineTo(sx(i2), sy(s[2][i2]));
      }
      ctx.stroke();
    });
    legend(series.map(function (s) { return [s[0], s[1]]; }), x1, y0);
    if (hoverI != null) {
      var hx = sx(hoverI);
      ctx.strokeStyle = INK; ctx.lineWidth = 1;
      ctx.beginPath(); ctx.moveTo(hx, y0); ctx.lineTo(hx, y1); ctx.stroke();
      series.forEach(function (s) { marker(hx, sy(s[2][hoverI]), s[1]); });
      readout(hx, 'hour  ' + hoverI,
        series.map(function (s) { return [s[0], s[1], s[2][hoverI]]; }), x0, x1, y0);
    }
  }

  function resize() {
    dpr = Math.min(window.devicePixelRatio || 1, 2);
    w = canvas.clientWidth; h = canvas.clientHeight;
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    draw();
  }

  // Hover crosshair, snapped to the nearest sample — the widget's 2D
  // behavior in the terminal, mirrored here.
  canvas.addEventListener('pointermove', function (e) {
    var r = canvas.getBoundingClientRect();
    var px = e.clientX - r.left;
    var x0 = 34, x1 = w - 10;
    var i = null;
    if (px >= x0 && px <= x1) {
      i = Math.max(0, Math.min(HOURS - 1, Math.round((px - x0) / (x1 - x0) * (HOURS - 1))));
    }
    if (i !== hoverI) { hoverI = i; draw(); }
  });
  canvas.addEventListener('pointerleave', function () {
    if (hoverI != null) { hoverI = null; draw(); }
  });

  new ResizeObserver(resize).observe(canvas);
  resize();
})();
