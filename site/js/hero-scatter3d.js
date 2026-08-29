(function () {
  var canvas = document.getElementById('plot3d');
  var ctx = canvas.getContext('2d');
  var reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

  var css = getComputedStyle(document.documentElement);
  var COLORS = [
    css.getPropertyValue('--trace-1').trim(),
    css.getPropertyValue('--trace-2').trim(),
    css.getPropertyValue('--trace-3').trim()
  ];
  var FRAME = css.getPropertyValue('--frame').trim();
  var GRID = css.getPropertyValue('--grid').trim();
  var INK = css.getPropertyValue('--ink').trim();
  var MONO = css.getPropertyValue('--mono').trim() || 'ui-monospace, monospace';

  // deterministic points — three gaussian-ish clusters, one per trace
  function mulberry32(a) {
    return function () {
      a |= 0; a = a + 0x6D2B79F5 | 0;
      var t = Math.imul(a ^ a >>> 15, 1 | a);
      t = t + Math.imul(t ^ t >>> 7, 61 | t) ^ t;
      return ((t ^ t >>> 14) >>> 0) / 4294967296;
    };
  }
  var rand = mulberry32(230607);
  function gauss() { return (rand() + rand() + rand() + rand() - 2) / 2; }

  var CENTERS = [[-.45, -.2, .35], [.4, .3, -.25], [.05, -.45, -.45]];
  var SPREAD = [.34, .3, .26];
  var points = [];
  for (var t = 0; t < 3; t++) {
    for (var i = 0; i < 85; i++) {
      points.push({
        x: CENTERS[t][0] + gauss() * SPREAD[t],
        y: CENTERS[t][1] + gauss() * SPREAD[t],
        z: CENTERS[t][2] + gauss() * SPREAD[t],
        c: t
      });
    }
  }

  var CUBE = [];
  [[-1,-1],[-1,1],[1,-1],[1,1]].forEach(function (p) {
    CUBE.push([[p[0], p[1], -1], [p[0], p[1], 1]]);
    CUBE.push([[p[0], -1, p[1]], [p[0], 1, p[1]]]);
    CUBE.push([[-1, p[0], p[1]], [1, p[0], p[1]]]);
  });
  var S = .72; // cube half-size in data space

  var DEF_YAW = .7, DEF_PITCH = .42, DEF_ZOOM = .85;
  var yaw = DEF_YAW, pitch = DEF_PITCH, zoom = DEF_ZOOM;
  var panX = 0, panY = 0;
  var hiddenTraces = [false, false, false];
  var dragging = false, lastX = 0, lastY = 0;
  var paused = false, pointerInside = false;
  var hoverX = null, hoverY = null;
  var selected = null; // a picked point: tooltip pins to it, auto-rotate pauses
  var tip = document.getElementById('tip');
  var TRACE_NAMES = ['Cluster A', 'Cluster B', 'Cluster C'];
  var w = 0, h = 0, dpr = 1;

  function resize() {
    dpr = Math.min(window.devicePixelRatio || 1, 2);
    w = canvas.clientWidth; h = canvas.clientHeight;
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    draw();
  }

  function project(x, y, z) {
    // right-handed data frame: x → right (at yaw 0), z → up, y → into the screen
    var vx = x, vy = z, vz = y;
    var cy = Math.cos(yaw), sy = Math.sin(yaw);
    var cp = Math.cos(pitch), sp = Math.sin(pitch);
    var rx = vx * cy + vz * sy;
    var rz = -vx * sy + vz * cy;
    var ry = vy * cp - rz * sp;
    rz = vy * sp + rz * cp;
    var f = 3 / (3 + rz);
    var scale = Math.min(w, h) * .42 * zoom;
    return {
      x: w / 2 + panX + rx * scale * f,
      y: h / 2 + panY - ry * scale * f,
      z: rz, f: f
    };
  }

  function draw() {
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);

    // orientation cube — filled + gridded far panes, depth-graded edges, corner dots, axis labels
    var NDIV = 4;
    function facePoint(ax, sgn, u, uu, v, vv) {
      var p = [0, 0, 0];
      p[ax] = sgn; p[u] = uu; p[v] = vv;
      return project(p[0] * S, p[1] * S, p[2] * S);
    }
    for (var ax = 0; ax < 3; ax++) {
      var cA = [0, 0, 0], cB = [0, 0, 0];
      cA[ax] = S; cB[ax] = -S;
      var sgn = project(cA[0], cA[1], cA[2]).z >= project(cB[0], cB[1], cB[2]).z ? 1 : -1;
      var u = (ax + 1) % 3, v = (ax + 2) % 3;

      // faint fill so the far panes read as walls
      var f1 = facePoint(ax, sgn, u, -1, v, -1);
      var f2 = facePoint(ax, sgn, u, 1, v, -1);
      var f3 = facePoint(ax, sgn, u, 1, v, 1);
      var f4 = facePoint(ax, sgn, u, -1, v, 1);
      ctx.globalAlpha = .05;
      ctx.fillStyle = FRAME;
      ctx.beginPath();
      ctx.moveTo(f1.x, f1.y); ctx.lineTo(f2.x, f2.y);
      ctx.lineTo(f3.x, f3.y); ctx.lineTo(f4.x, f4.y);
      ctx.closePath(); ctx.fill();

      // pane gridlines
      ctx.globalAlpha = .35;
      ctx.strokeStyle = GRID;
      ctx.lineWidth = 1;
      for (var g = 1; g < NDIV; g++) {
        var tt = -1 + 2 * g / NDIV;
        var g1 = facePoint(ax, sgn, u, tt, v, -1), g2 = facePoint(ax, sgn, u, tt, v, 1);
        ctx.beginPath(); ctx.moveTo(g1.x, g1.y); ctx.lineTo(g2.x, g2.y); ctx.stroke();
        var g3 = facePoint(ax, sgn, u, -1, v, tt), g4 = facePoint(ax, sgn, u, 1, v, tt);
        ctx.beginPath(); ctx.moveTo(g3.x, g3.y); ctx.lineTo(g4.x, g4.y); ctx.stroke();
      }
    }

    // edges, brighter and thicker the closer they are
    for (var e = 0; e < CUBE.length; e++) {
      var a = project(CUBE[e][0][0] * S, CUBE[e][0][1] * S, CUBE[e][0][2] * S);
      var b = project(CUBE[e][1][0] * S, CUBE[e][1][1] * S, CUBE[e][1][2] * S);
      var ed = Math.max(0, Math.min(1, (1 - (a.z + b.z) / 2) / 2));
      ctx.globalAlpha = .3 + .7 * ed;
      ctx.lineWidth = .8 + .9 * ed;
      ctx.strokeStyle = FRAME;
      ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke();
    }

    // coordinate axes through the origin, extending to 1.6× the box half-size
    for (var axn = 0; axn < 3; axn++) {
      var dirv = [0, 0, 0];
      dirv[axn] = 1.6 * S;
      var negP = project(-dirv[0], -dirv[1], -dirv[2]);
      var tipP = project(dirv[0], dirv[1], dirv[2]);
      var aed = Math.max(0, Math.min(1, (1 - (negP.z + tipP.z) / 2) / 2));
      ctx.globalAlpha = .3 + .7 * aed;
      ctx.lineWidth = .8 + .9 * aed;
      ctx.strokeStyle = FRAME;
      ctx.fillStyle = FRAME;
      ctx.beginPath(); ctx.moveTo(negP.x, negP.y); ctx.lineTo(tipP.x, tipP.y); ctx.stroke();
      // arrowhead on the positive end, oriented along the projected axis direction
      var adx = tipP.x - negP.x, ady = tipP.y - negP.y;
      var alen = Math.sqrt(adx * adx + ady * ady) || 1;
      adx /= alen; ady /= alen;
      var ah = 6 * tipP.f;
      ctx.beginPath();
      ctx.moveTo(tipP.x + adx * ah, tipP.y + ady * ah);
      ctx.lineTo(tipP.x - ady * ah * .5, tipP.y + adx * ah * .5);
      ctx.lineTo(tipP.x + ady * ah * .5, tipP.y - adx * ah * .5);
      ctx.closePath(); ctx.fill();
    }

    // axis labels just past the cube
    ctx.globalAlpha = .8;
    ctx.fillStyle = INK;
    ctx.font = '10px ' + MONO;
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    var AXES = [['x', [1.78, 0, 0]], ['y', [0, 1.78, 0]], ['z', [0, 0, 1.78]]];
    for (var l = 0; l < 3; l++) {
      var lp = project(AXES[l][1][0] * S, AXES[l][1][1] * S, AXES[l][1][2] * S);
      ctx.fillText(AXES[l][0], lp.x, lp.y);
    }
    ctx.globalAlpha = 1;

    // points, painter's order
    var proj = [];
    for (var i = 0; i < points.length; i++) {
      if (hiddenTraces[points[i].c]) continue;
      var p = project(points[i].x, points[i].y, points[i].z);
      p.c = points[i].c;
      p.data = points[i];
      proj.push(p);
    }
    proj.sort(function (a, b) { return b.z - a.z; });
    for (var j = 0; j < proj.length; j++) {
      var q = proj[j];
      var depth = Math.max(0, Math.min(1, (1 - q.z) / 2));
      ctx.globalAlpha = .35 + .65 * depth;
      ctx.fillStyle = COLORS[q.c];
      ctx.beginPath();
      ctx.arc(q.x, q.y, (1.1 + 2.2 * depth) * q.f, 0, Math.PI * 2);
      ctx.fill();
    }
    ctx.globalAlpha = 1;

    // The tooltip shows the picked (tap/click) point if there is one — it
    // pins to the dot and follows it through view changes — else, for a
    // mouse that isn't dragging, the frontmost point under the cursor.
    var shown = null, shownR = 0;
    if (selected && !hiddenTraces[selected.c]) {
      var sp = project(selected.x, selected.y, selected.z);
      var sd = Math.max(0, Math.min(1, (1 - sp.z) / 2));
      shown = { x: sp.x, y: sp.y, c: selected.c, data: selected };
      shownR = (1.1 + 2.2 * sd) * sp.f;
    } else if (!selected && hoverX !== null && !dragging) {
      for (var k = proj.length - 1; k >= 0; k--) {
        var hq = proj[k];
        var hd = Math.max(0, Math.min(1, (1 - hq.z) / 2));
        var hr = (1.1 + 2.2 * hd) * hq.f;
        var ddx = hq.x - hoverX, ddy = hq.y - hoverY;
        if (ddx * ddx + ddy * ddy <= (hr + 4) * (hr + 4)) {
          shown = hq; shownR = hr;
          break;
        }
      }
    }
    if (shown) {
      ctx.strokeStyle = COLORS[shown.c];
      ctx.lineWidth = 1.5;
      ctx.beginPath();
      ctx.arc(shown.x, shown.y, shownR + 3, 0, Math.PI * 2);
      ctx.stroke();
      tip.innerHTML = '<span class="sw" style="background:' + COLORS[shown.c] + '"></span>'
        + TRACE_NAMES[shown.c]
        + '<br>x ' + shown.data.x.toFixed(2)
        + '<br>y ' + shown.data.y.toFixed(2)
        + '<br>z ' + shown.data.z.toFixed(2);
      tip.hidden = false;
      var tx = canvas.offsetLeft + shown.x + 14;
      var ty = canvas.offsetTop + shown.y + 14;
      if (shown.x > w - 170) tx = canvas.offsetLeft + shown.x - 14 - tip.offsetWidth;
      if (shown.y > h - 100) ty = canvas.offsetTop + shown.y - 14 - tip.offsetHeight;
      tip.style.left = tx + 'px';
      tip.style.top = ty + 'px';
    } else {
      tip.hidden = true;
    }
  }

  // Frontmost visible point within picking range of canvas coords (px, py).
  function hitTest(px, py) {
    var best = null, bestZ = Infinity;
    for (var i = 0; i < points.length; i++) {
      if (hiddenTraces[points[i].c]) continue;
      var p = project(points[i].x, points[i].y, points[i].z);
      var d = Math.max(0, Math.min(1, (1 - p.z) / 2));
      var r = (1.1 + 2.2 * d) * p.f + 6;
      var dx = p.x - px, dy = p.y - py;
      if (dx * dx + dy * dy <= r * r && p.z < bestZ) {
        best = points[i]; bestZ = p.z;
      }
    }
    return best;
  }

  // ---- input: mouse + touch ----
  // Touch gestures: one finger rotates (and hovers any dot it rests on or
  // crosses), two fingers pinch-zoom, a quick double-tap resets the view.
  var pointers = new Map(); // active pointers, id → {x, y}
  var pinching = false, pinchDist = 0;
  var touchDragging = false; // the current drag comes from a finger
  var downX = 0, downY = 0, downAt = 0;
  var lastTapAt = 0, lastTapX = 0, lastTapY = 0;

  function twoPointerDist() {
    var it = pointers.values();
    var a = it.next().value, b = it.next().value;
    return Math.hypot(a.x - b.x, a.y - b.y);
  }
  function resetView() {
    yaw = DEF_YAW; pitch = DEF_PITCH; zoom = DEF_ZOOM; panX = 0; panY = 0;
    selected = null;
    draw();
  }
  function updateHover(e) {
    var rect = canvas.getBoundingClientRect();
    hoverX = e.clientX - rect.left;
    hoverY = e.clientY - rect.top;
  }
  // A tap or click: pick the dot under it (pins the tooltip, pauses the
  // rotation) — or, on empty space, clear the pick and let it spin again.
  function selectAt(clientX, clientY) {
    var rect = canvas.getBoundingClientRect();
    selected = hitTest(clientX - rect.left, clientY - rect.top);
    draw();
  }

  canvas.addEventListener('pointerdown', function (e) {
    pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
    try { canvas.setPointerCapture(e.pointerId); } catch (_) { /* Safari edge cases */ }
    if (pointers.size === 2) {
      // Second finger down: the rotate drag ends, pinch-zoom begins.
      pinching = true; dragging = false;
      pinchDist = twoPointerDist();
      hoverX = hoverY = null;
      draw();
      return;
    }
    if (pointers.size > 2) return;
    dragging = true;
    touchDragging = e.pointerType !== 'mouse';
    lastX = e.clientX; lastY = e.clientY;
    downX = e.clientX; downY = e.clientY; downAt = e.timeStamp;
    canvas.classList.add('dragging');
  });
  canvas.addEventListener('pointermove', function (e) {
    var p = pointers.get(e.pointerId);
    if (p) { p.x = e.clientX; p.y = e.clientY; }
    if (pinching && pointers.size >= 2) {
      var d = twoPointerDist();
      if (pinchDist > 0) zoom = Math.max(.4, Math.min(4, zoom * d / pinchDist));
      pinchDist = d;
    } else if (dragging) {
      var dx = e.clientX - lastX, dy = e.clientY - lastY;
      if (e.shiftKey) {
        panX += dx; panY += dy;
      } else {
        yaw -= dx * .006;
        pitch -= dy * .006;
        pitch = Math.max(-1.35, Math.min(1.35, pitch));
      }
      lastX = e.clientX; lastY = e.clientY;
    } else {
      updateHover(e);
    }
    draw();
  });
  canvas.addEventListener('pointerenter', function () {
    pointerInside = true;
  });
  canvas.addEventListener('pointerleave', function (e) {
    pointerInside = false;
    // Mouse only: a lifted finger keeps its tooltip (a tap on a dot would
    // otherwise flash it for a frame), and touch fires leave after every up.
    if (e.pointerType === 'mouse') {
      hoverX = hoverY = null;
      draw();
    }
  });
  window.addEventListener('keydown', function (e) {
    if (e.code !== 'Space' || e.repeat || !pointerInside) return;
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    e.preventDefault();
    paused = !paused;
  });
  function releasePointer(e) {
    pointers.delete(e.pointerId);
    if (pinching) {
      if (pointers.size < 2) {
        pinching = false;
        if (pointers.size === 1) {
          // One finger stays down: continue it as a rotate drag.
          var rest = pointers.values().next().value;
          dragging = true; touchDragging = true;
          lastX = rest.x; lastY = rest.y;
          downAt = 0; // a pinch is never a tap
        }
      }
      return;
    }
    if (pointers.size === 0) {
      dragging = false; canvas.classList.remove('dragging');
    }
    if (e.type !== 'pointerup') return;
    var isTouch = e.pointerType !== 'mouse';
    var movedPx = Math.hypot(e.clientX - downX, e.clientY - downY);
    // A tap: quick and still. A click: released without real movement.
    var isTap = downAt > 0 &&
      (isTouch ? (e.timeStamp - downAt < 300 && movedPx < 12) : movedPx < 6);
    if (!isTap) {
      lastTapAt = 0;
      return;
    }
    // Touch double-tap reset: a second quick tap close to the first.
    if (isTouch && e.timeStamp - lastTapAt < 350 &&
        Math.hypot(e.clientX - lastTapX, e.clientY - lastTapY) < 40) {
      lastTapAt = 0;
      resetView();
      return;
    }
    if (isTouch) {
      lastTapAt = e.timeStamp; lastTapX = e.clientX; lastTapY = e.clientY;
    }
    selectAt(e.clientX, e.clientY);
  }
  canvas.addEventListener('pointerup', releasePointer);
  canvas.addEventListener('pointercancel', releasePointer);
  canvas.addEventListener('dblclick', resetView);
  canvas.addEventListener('wheel', function (e) {
    e.preventDefault();
    zoom *= Math.exp(-e.deltaY * .002);
    zoom = Math.max(.4, Math.min(4, zoom));
    draw();
  }, { passive: false });

  var legendItems = document.querySelectorAll('#legend .li');
  Array.prototype.forEach.call(legendItems, function (item) {
    item.addEventListener('click', function () {
      var t = +item.dataset.trace;
      hiddenTraces[t] = !hiddenTraces[t];
      item.classList.toggle('off', hiddenTraces[t]);
      item.setAttribute('aria-pressed', String(!hiddenTraces[t]));
      if (selected && hiddenTraces[selected.c]) selected = null; // its dot is gone
      draw();
    });
  });

  var ro = new ResizeObserver(resize);
  ro.observe(canvas);
  resize();

  if (!reduced) {
    (function tick() {
      if (!dragging && !pinching && !paused && !selected && !document.hidden) {
        yaw += .0018; draw();
      }
      requestAnimationFrame(tick);
    })();
  }
})();
