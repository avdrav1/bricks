#!/usr/bin/env python3
"""Paint a small scene in Etch for cronkite.cc's gallery (scripts/site_shots.sh).

Usage: etch_scene.py SCREEN.png WIDTH HEIGHT SCALE

Run under the headless sway that shows Etch, at 100% (Ctrl+0). SCREEN.png is a
screenshot taken before painting (physical pixels, SCALE per logical pixel), from which
the palette's swatches (each its exact colour) and the white image (the white run through
the screen's middle) are found. The scene is painted the way a hand would, through the
wlr-virtual-pointer protocol (scripts/dnd_helpers.py, WIDTH x HEIGHT logical pixels):
pick a colour, type a size into the Size field (Alt+S), and drag. A sky, a sun, a green
hill, a house with a roof, a door, and windows, a tree, and two birds.
"""
import math
import os
import subprocess
import sys
import time

import gi

gi.require_version("GdkPixbuf", "2.0")
from gi.repository import GdkPixbuf

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import dnd_helpers  # noqa: E402

PALETTE = {
    "Light Turquoise": (0x99, 0xD9, 0xEA),
    "Gold": (0xFF, 0xC9, 0x0E),
    "Orange": (0xFF, 0x7F, 0x27),
    "Lime": (0xB5, 0xE6, 0x1D),
    "Green": (0x22, 0xB1, 0x4C),
    "Red": (0xED, 0x1C, 0x24),
    "Dark Red": (0x88, 0x00, 0x15),
    "Brown": (0xB9, 0x7A, 0x57),
    "Light Yellow": (0xEF, 0xE4, 0xB0),
    "Grey": (0x7F, 0x7F, 0x7F),
}


def screen(path, scale):
    """Each swatch's middle and the white image's box, in logical pixels."""
    p = GdkPixbuf.Pixbuf.new_from_file(path)
    w, h, rs, n, px = p.get_width(), p.get_height(), p.get_rowstride(), p.get_n_channels(), p.get_pixels()

    def at(x, y):
        i = y * rs + x * n
        return px[i], px[i + 1], px[i + 2]

    found = {name: [0, 0, 0] for name in PALETTE}
    wanted = {rgb: name for name, rgb in PALETTE.items()}
    for y in range(0, h, 2):
        for x in range(0, w, 2):
            name = wanted.get(at(x, y))
            if name:
                f = found[name]
                f[0] += x
                f[1] += y
                f[2] += 1
    swatches = {}
    for name, (sx, sy, count) in found.items():
        if count == 0:
            sys.exit(f"no swatch for {name}")
        swatches[name] = (sx / count / scale, sy / count / scale)

    def white(x, y):
        return min(at(x, y)) >= 250

    # The image is the only white across the screen's middle: from the first white pixel
    # to the last, each way (faint lines inside it don't split it).
    my = h // 2
    row = [x for x in range(w) if white(x, my)]
    left, right = row[0], row[-1]
    mx = (left + right) // 2
    col = [y for y in range(h) if white(mx, y)]
    top, bottom = col[0], col[-1]
    box = (left / scale, top / scale, (right - left) / scale, (bottom - top) / scale)
    return swatches, box


def main():
    shot, width, height, scale = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), int(sys.argv[4])
    swatches, (bx, by, bw, bh) = screen(shot, scale)
    move, button, _, done = dnd_helpers.pointer(width, height)

    def at(fx, fy):
        return bx + fx * bw, by + fy * bh

    def keys(*args):
        subprocess.run(["wtype", "-s", "150", *args, "-s", "150"], check=True)

    def colour(name):
        x, y = swatches[name]
        move(x, y)
        time.sleep(0.2)
        button(True, hold=0.05)
        button(False, hold=0.2)

    def size(n):
        keys("-M", "alt", "s", "-m", "alt")
        keys("-M", "ctrl", "a", "-m", "ctrl")
        keys(str(n))
        keys("-k", "Return")

    def stroke(points):
        # Press at the first point and move through the rest a few pixels at a time.
        points = [at(*p) for p in points]
        move(*points[0])
        time.sleep(0.15)
        button(True, hold=0.05)
        for (x1, y1), (x2, y2) in zip(points, points[1:]):
            steps = max(1, int(math.hypot(x2 - x1, y2 - y1) / 8))
            for i in range(1, steps + 1):
                move(x1 + (x2 - x1) * i / steps, y1 + (y2 - y1) * i / steps)
        button(False, hold=0.15)

    def rows(x1, x2, y1, y2, gap):
        # Back and forth across a box, `gap` apart: a fill.
        pts, y, flip = [], y1, False
        while y <= y2 + 1e-9:
            pts += [(x2, y), (x1, y)] if flip else [(x1, y), (x2, y)]
            y += gap
            flip = not flip
        return pts

    def disk(cx, cy, r, gap):
        # A spiral from the rim in: a filled circle (r in image widths).
        pts, a, rr = [], 0.0, r
        while rr > 0:
            pts.append((cx + rr * math.cos(a), cy + rr * math.sin(a) * bw / bh))
            a += 0.25
            rr -= gap * 0.25 / (2 * math.pi)
        return pts

    # The sky, then the sun with its rays.
    colour("Light Turquoise")
    size(100)
    stroke(rows(-0.02, 1.02, 0.04, 0.62, 0.11))
    colour("Gold")
    size(30)
    stroke(disk(0.82, 0.2, 0.075, 0.025))
    colour("Orange")
    size(8)
    for k in range(10):
        a = k * math.pi / 5
        c, s = math.cos(a), math.sin(a) * bw / bh
        stroke([(0.82 + 0.1 * c, 0.2 + 0.1 * s), (0.82 + 0.135 * c, 0.2 + 0.135 * s)])
    # A hill, its top a gentle curve.
    colour("Lime")
    size(100)
    stroke([(x / 40, 0.66 - 0.05 * math.sin(math.pi * x / 40)) for x in range(-1, 42)]
           + rows(1.02, -0.02, 0.74, 1.0, 0.11))
    # The house: walls, roof, door, windows.
    colour("Red")
    size(24)
    stroke(rows(0.2, 0.42, 0.44, 0.7, 0.025))
    colour("Dark Red")
    size(16)
    roof = []
    for i in range(9):
        y = 0.29 + i * 0.017
        half = 0.13 * (y - 0.29) / 0.15
        roof += [(0.31 - half, y), (0.31 + half, y)] if i % 2 == 0 else [(0.31 + half, y), (0.31 - half, y)]
    stroke(roof)
    colour("Brown")
    size(16)
    stroke(rows(0.295, 0.325, 0.57, 0.7, 0.02))
    colour("Light Yellow")
    size(14)
    stroke(rows(0.225, 0.265, 0.48, 0.54, 0.018))
    stroke(rows(0.355, 0.395, 0.48, 0.54, 0.018))
    # A tree, and two birds.
    colour("Brown")
    size(22)
    stroke([(0.68, 0.74), (0.68, 0.5)])
    colour("Green")
    size(30)
    stroke(disk(0.68, 0.42, 0.075, 0.03))
    colour("Grey")
    size(4)
    for x, y in ((0.5, 0.16), (0.58, 0.22)):
        stroke([(x - 0.03, y - 0.015), (x, y + 0.01), (x + 0.03, y - 0.015)])
    # The pointer rests off the image.
    move(bx + bw + 40, by + 40)
    done()


if __name__ == "__main__":
    main()
