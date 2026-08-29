(function () {
  var canvas = document.getElementById('surf3d');
  if (!canvas) return;
  var ctx = canvas.getContext('2d');
  var reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

  // Gaussian peaks — the hillclimb terrain, on the viridis ramp.
  var PEAKS = [[0.15, -0.10, 0.95, 0.35], [-0.75, 0.55, 0.48, 0.16], [0.55, 0.85, 0.25, 0.12]];
  var EXT = 1.25, N = 40;
  function heightAt(x, y) {
    var h = 0;
    for (var k = 0; k < PEAKS.length; k++) {
      var p = PEAKS[k];
      var dx = x - p[0], dy = y - p[1];
      h += p[2] * Math.exp(-(dx * dx + dy * dy) / (2 * p[3] * p[3]));
    }
    return h;
  }
  var VIRIDIS = [[68, 1, 84], [59, 82, 139], [33, 145, 140], [94, 201, 98], [253, 231, 37]];
  function ramp(t) {
    t = Math.max(0, Math.min(1, t)) * 4;
    var i = Math.min(3, Math.floor(t)), f = t - i;
    var a = VIRIDIS[i], b = VIRIDIS[i + 1];
    return [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f];
  }

  // Grid vertices: position, height, and a data-space lambert shade.
  var L = [-0.5, -0.6, 0.62], ln = Math.hypot(L[0], L[1], L[2]);
  var vx = [], vy = [], vz = [], shade = [];
  var zmax = 0;
  for (var j = 0; j <= N; j++) {
    for (var i = 0; i <= N; i++) {
      var x = -EXT + 2 * EXT * i / N, y = -EXT + 2 * EXT * j / N;
      var z = heightAt(x, y);
      zmax = Math.max(zmax, z);
      var e = .01;
      var gx = (heightAt(x + e, y) - heightAt(x - e, y)) / (2 * e);
      var gy = (heightAt(x, y + e) - heightAt(x, y - e)) / (2 * e);
      var nl = Math.hypot(gx, gy, 1);
      var lam = Math.abs((-gx * L[0] - gy * L[1] + L[2]) / (nl * ln));
      vx.push(x); vy.push(y); vz.push(z);
      shade.push(.55 + .45 * lam);
    }
  }

  var DEF_YAW = .55, DEF_PITCH = .48;
  var yaw = DEF_YAW, pitch = DEF_PITCH;
  var dragging = false, lastX = 0, lastY = 0;
  var w = 0, h = 0, dpr = 1;

  // Height goes into the screen-up slot, the grid spans x and depth — the
  // same orientation the hero scene uses, so the drag directions match too.
  function project(x, y, z) {
    var cy = Math.cos(yaw), sy = Math.sin(yaw);
    var cp = Math.cos(pitch), sp = Math.sin(pitch);
    var up = z - .45; // center the height range
    var rx = x * cy + y * sy;
    var rz = -x * sy + y * cy;
    var ry = up * cp - rz * sp;
    rz = up * sp + rz * cp;
    var f = 3 / (3 + rz);
    var scale = Math.min(w, h) * .40;
    return { x: w / 2 + rx * scale * f, y: h / 2 - ry * scale * f, z: rz };
  }

  function draw() {
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    var proj = [];
    for (var k = 0; k < vx.length; k++) proj.push(project(vx[k], vy[k], vz[k]));
    var quads = [];
    for (var j = 0; j < N; j++) {
      for (var i = 0; i < N; i++) {
        var a = j * (N + 1) + i, b = a + 1, c = a + N + 2, d = a + N + 1;
        var depth = (proj[a].z + proj[c].z) / 2;
        var t = (vz[a] + vz[b] + vz[c] + vz[d]) / 4 / zmax;
        var s = (shade[a] + shade[b] + shade[c] + shade[d]) / 4;
        var fogT = Math.max(0, Math.min(1, (depth + 1) / 2)) * .45;
        var col = ramp(t);
        var r = col[0] * s * (1 - fogT) + 26 * fogT;
        var g = col[1] * s * (1 - fogT) + 30 * fogT;
        var bl = col[2] * s * (1 - fogT) + 44 * fogT;
        quads.push([depth, a, b, c, d, 'rgb(' + (r | 0) + ',' + (g | 0) + ',' + (bl | 0) + ')']);
      }
    }
    quads.sort(function (p, q) { return q[0] - p[0]; }); // back to front
    for (var q = 0; q < quads.length; q++) {
      var e = quads[q];
      ctx.fillStyle = e[5];
      ctx.strokeStyle = e[5]; // hairline stroke hides seams between quads
      ctx.beginPath();
      ctx.moveTo(proj[e[1]].x, proj[e[1]].y);
      ctx.lineTo(proj[e[2]].x, proj[e[2]].y);
      ctx.lineTo(proj[e[3]].x, proj[e[3]].y);
      ctx.lineTo(proj[e[4]].x, proj[e[4]].y);
      ctx.closePath();
      ctx.fill(); ctx.stroke();
    }
  }

  function resize() {
    dpr = Math.min(window.devicePixelRatio || 1, 2);
    w = canvas.clientWidth; h = canvas.clientHeight;
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    draw();
  }

  canvas.addEventListener('pointerdown', function (e) {
    dragging = true; lastX = e.clientX; lastY = e.clientY;
    canvas.classList.add('dragging');
    canvas.setPointerCapture(e.pointerId);
  });
  canvas.addEventListener('pointermove', function (e) {
    if (!dragging) return;
    yaw -= (e.clientX - lastX) * .006;
    pitch -= (e.clientY - lastY) * .006;
    pitch = Math.max(.08, Math.min(1.35, pitch));
    lastX = e.clientX; lastY = e.clientY;
    draw();
  });
  canvas.addEventListener('pointerup', function () {
    dragging = false; canvas.classList.remove('dragging');
  });
  canvas.addEventListener('dblclick', function () {
    yaw = DEF_YAW; pitch = DEF_PITCH; draw();
  });

  new ResizeObserver(resize).observe(canvas);
  resize();
  if (!reduced) {
    (function tick() {
      if (!dragging && !document.hidden) { yaw += .0012; draw(); }
      requestAnimationFrame(tick);
    })();
  }
})();
