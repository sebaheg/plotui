#!/usr/bin/env python3
"""Raw-loop demo: interactive 3D scatter via the Kitty graphics protocol.

This owns the terminal directly (no TUI framework) to prove the full pipeline
end-to-end: Python owns the loop + input, the Rust core rasterizes, and the
Kitty protocol places the pixels. Run it in Kitty or Ghostty:

    python examples/raw_demo.py

Controls: h/j/k/l or arrows rotate · +/- zoom · space auto-spin · r reset · q quit
"""
import math
import os
import select
import sys
import termios
import time
import tty

from plotui import Plot


def make_plot() -> Plot:
    plot = Plot()
    # A double helix + a diffuse cloud, so 3D structure is obvious when rotating.
    xs, ys, zs = [], [], []
    n = 1600
    for i in range(n):
        t = i / n * 6.0 * math.pi
        strand = 1.0 if i % 2 == 0 else -1.0
        xs.append(math.cos(t) * strand)
        ys.append(t / (6.0 * math.pi) * 2.0 - 1.0)
        zs.append(math.sin(t) * strand)
    plot.add_scatter3d(xs, ys, zs, color=(230, 60, 120), size=2.0)
    return plot


def main() -> None:
    plot = make_plot()
    fd = sys.stdin.fileno()
    old = termios.tcgetattr(fd)
    out = sys.stdout
    # We can't reliably query cell pixel size here; Kitty scales the image to the
    # cell region via c=/r=, so an approximate cell size is fine.
    cell_w, cell_h = 10, 20
    auto = True
    try:
        tty.setraw(fd)
        out.write("\x1b[?1049h\x1b[?25l")  # alt screen, hide cursor
        last = time.time()
        while True:
            cols, rows = os.get_terminal_size()
            plot_rows = max(1, rows - 1)

            r, _, _ = select.select([fd], [], [], 0.02)
            if r:
                ch = os.read(fd, 16)
                if ch in (b"q", b"\x03"):
                    break
                elif ch in (b"+", b"="):
                    plot.zoom_by(1.1)
                elif ch == b"-":
                    plot.zoom_by(0.9)
                elif ch in (b"h", b"\x1b[D"):
                    plot.rotate(-0.12, 0.0)
                elif ch in (b"l", b"\x1b[C"):
                    plot.rotate(0.12, 0.0)
                elif ch in (b"k", b"\x1b[A"):
                    plot.rotate(0.0, -0.12)
                elif ch in (b"j", b"\x1b[B"):
                    plot.rotate(0.0, 0.12)
                elif ch == b"r":
                    plot.reset()
                elif ch == b" ":
                    auto = not auto

            now = time.time()
            if auto:
                plot.rotate((now - last) * 0.6, 0.0)
            last = now

            esc = plot.render_kitty(cols, plot_rows, cell_w, cell_h)
            header = (
                "\x1b[H\x1b[2K\x1b[38;5;213m plotui \x1b[0m"
                "3D scatter — h/j/k/l rotate · +/- zoom · space spin · q quit"
            )
            out.write(header + "\x1b[2;1H" + esc)
            out.flush()
    finally:
        out.write(Plot.kitty_cleanup() + "\x1b[?25h\x1b[?1049l")
        out.flush()
        termios.tcsetattr(fd, termios.TCSADRAIN, old)


if __name__ == "__main__":
    main()
