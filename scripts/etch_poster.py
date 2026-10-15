#!/usr/bin/env python3
"""Design a poster in Etch for cronkite.cc's gallery (scripts/site_shots.sh).

Usage: etch_poster.py SCREEN.png WIDTH HEIGHT SCALE

Run under the headless sway that shows Etch's blank 1024x768 image at 100% (Ctrl+0),
inside the app's D-Bus session with at-spi2-core. SCREEN.png is a screenshot taken
before painting (physical pixels, SCALE per logical pixel), in which the white image
(the white run through the screen's middle) is found. Everything is done the way a hand
would do it, in Etch's own tools: colours typed into the Hex field (Alt+H), sizes into
the Size field (Alt+S), tools picked by their keys, the shape style (Outline, Filled),
Bold, and the font family picked in the tool bar (found and pressed through the
accessibility tree), and the pointer moved, clicked, and dragged through the
wlr-virtual-pointer protocol (scripts/dnd_helpers.py, WIDTH x HEIGHT logical pixels).

The poster, for a spring bike ride: a deep green ground (the fill), an amber sun partly
off the top right (a filled ellipse), an off-white bicycle on an amber road (outlined
ellipses for the wheels, lines for the road, frame, saddle, and handlebar), the headline,
date and place, and a quieter line on the road in Adwaita Sans (the text tool), and a
coral tag (a filled rectangle) saying the ride is free. It ends with the rectangle tool
on, Filled, in coral.
"""
import os
import subprocess
import sys
import time

import gi

gi.require_version("Atspi", "2.0")
gi.require_version("GdkPixbuf", "2.0")
from gi.repository import Atspi, GdkPixbuf  # noqa: E402

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import dnd_helpers  # noqa: E402

GREEN = "#10392E"
AMBER = "#F2B134"
CORAL = "#E4572E"
PAPER = "#F4F1EA"
FAMILY = "Adwaita Sans"
# The left margin everything lines up on, in image pixels.
LEFT = 72


def image_box(path, scale):
    """The white image's left, top, width, and height, in logical pixels: the only white
    across the screen's middle, from its first white pixel to its last, each way."""
    p = GdkPixbuf.Pixbuf.new_from_file(path)
    w, h, rs, n, px = p.get_width(), p.get_height(), p.get_rowstride(), p.get_n_channels(), p.get_pixels()

    def white(x, y):
        i = y * rs + x * n
        return min(px[i:i + 3]) >= 250

    row = [x for x in range(w) if white(x, h // 2)]
    left, right = row[0], row[-1]
    col = [y for y in range(h) if white((left + right) // 2, y)]
    top, bottom = col[0], col[-1]
    return left / scale, top / scale, (right - left + 1) / scale, (bottom - top + 1) / scale


def accessible(name, role=None):
    """The first of Etch's widgets (breadth first) named or described NAME, of role ROLE
    (its role name) if given, and on screen."""
    desktop = Atspi.get_desktop(0)
    queue = [desktop.get_child_at_index(i) for i in range(desktop.get_child_count())]
    queue = [a for a in queue if a is not None and a.get_name() == "etch"]
    while queue:
        node = queue.pop(0)
        if node is None:
            continue
        node.clear_cache()
        if name in (node.get_name(), node.get_description()) and (role is None or node.get_role_name() == role):
            component = node.get_component_iface()
            if component is not None and component.get_extents(Atspi.CoordType.WINDOW).width > 0:
                return node
        queue.extend(node.get_child_at_index(i) for i in range(node.get_child_count()))
    sys.exit(f"nothing named {name!r}")


def main():
    shot, width, height, scale = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), int(sys.argv[4])
    bx, by, bw, bh = image_box(shot, scale)
    if (round(bw), round(bh)) != (1024, 768):
        sys.exit(f"the image shows {bw}x{bh}, not 1024x768 at 100%")
    # The pointer in physical pixels, so it can sit in the middle of an image pixel.
    move_px, button, _, done = dnd_helpers.pointer(width * scale, height * scale)

    def move(x, y):
        move_px(x * scale, y * scale)

    def at(x, y):
        # The middle of image pixel (x, y) on screen.
        return bx + x + 0.5, by + y + 0.5

    def keys(*args):
        subprocess.run(["wtype", "-s", "120", *args, "-s", "120"], check=True)

    def click(x, y):
        move(x, y)
        time.sleep(0.25)
        button(True, hold=0.05)
        button(False, hold=0.4)

    def drag(*points):
        # Press at the first image pixel, move through the rest in small steps, release.
        points = [at(*p) for p in points]
        move(*points[0])
        time.sleep(0.25)
        button(True, hold=0.1)
        for (x1, y1), (x2, y2) in zip(points, points[1:]):
            steps = max(4, int(max(abs(x2 - x1), abs(y2 - y1)) / 6))
            for i in range(1, steps + 1):
                move(x1 + (x2 - x1) * i / steps, y1 + (y2 - y1) * i / steps)
        time.sleep(0.2)
        button(False, hold=0.4)

    def colour(hex_):
        keys("-M", "alt", "h", "-m", "alt")
        keys(hex_)
        keys("-k", "Return")

    def size(n):
        keys("-M", "alt", "s", "-m", "alt")
        keys("-M", "ctrl", "a", "-m", "ctrl")
        keys(str(n))
        keys("-k", "Return")

    def toggle(name, on=True):
        node = accessible(name)
        states = node.get_state_set()
        if states.contains(Atspi.StateType.CHECKED) or states.contains(Atspi.StateType.PRESSED):
            if on:
                return
        elif not on:
            return
        Atspi.Action.do_action(node.get_action_iface(), 0)
        time.sleep(0.4)

    def centre(node):
        e = node.get_component_iface().get_extents(Atspi.CoordType.WINDOW)
        return e.x + e.width / 2, e.y + e.height / 2

    def text(x, y, *lines):
        # A click at image pixel (x, y) starts the text there; Shift+Enter breaks a line
        # and Enter puts it on the image.
        click(*at(x, y))
        for i, line in enumerate(lines):
            if i:
                keys("-M", "shift", "-k", "Return", "-m", "shift")
            keys(line)
        keys("-k", "Return")
        time.sleep(0.3)

    # The ground: deep green, filled from the middle.
    colour(GREEN)
    keys("f")
    click(*at(512, 600))

    # The sun: a filled amber circle, dragged from its box's bottom left past the image's
    # top right corner (as far above the image as the screen goes).
    colour(AMBER)
    keys("o")
    toggle("Filled")
    top, d = -min(50, int(by) - 2), 400
    drag((1096 - d, top + d), (1096, top))

    # The road: a thin amber line the bike stands on.
    keys("l")
    size(4)
    drag((LEFT + 1, 696), (1024 - LEFT - 3, 696))

    # The bicycle in off-white: wheels, then the frame, saddle, and handlebar, a line each.
    colour(PAPER)
    keys("o")
    toggle("Outline")
    size(14)
    rear, front, r = (595, 598), (855, 598), 96
    for cx, cy in (rear, front):
        drag((cx - r, cy - r), (cx + r, cy + r))
    keys("l")
    size(12)
    crank, seat, head, fork = (713, 606), (679, 458), (833, 458), (838, 490)
    for a, b in (
        (rear, crank),                    # the chain stay
        (crank, seat),                    # the seat tube
        (seat, rear),                     # the seat stay
        (seat, head),                     # the top tube
        (crank, fork),                    # the down tube
        (head, front),                    # the head tube and fork
        (seat, (673, 430)),               # the seat post
        ((649, 428), (697, 428)),         # the saddle
        (head, (829, 430)),               # the stem
        ((811, 430), (855, 430)),         # the handlebar
    ):
        drag(a, b)
    keys("o")
    toggle("Filled")
    for (cx, cy), half in ((rear, 10), (front, 10), (crank, 15)):
        drag((cx - half, cy - half), (cx + half, cy + half))

    # The words: the headline, then the date and place, in Adwaita Sans.
    keys("t")
    click(*centre(accessible("Font")))
    time.sleep(0.5)
    keys(FAMILY)
    time.sleep(1)
    click(*centre(accessible(FAMILY, "label")))
    time.sleep(0.5)
    toggle("Bold")
    size(128)
    text(LEFT - 5, 48, "Spring", "Ride")
    toggle("Bold", False)
    colour(AMBER)
    size(30)
    text(LEFT - 2, 372, "Saturday 12 April · 9 am · Riverside Park")
    # A quiet line on the road: off-white at 70%.
    colour(PAPER + "B3")
    size(20)
    text(LEFT - 2, 652, "Bring a bike · All ages welcome")

    # The tag: a coral box with the words in it.
    colour(CORAL)
    keys("r")
    toggle("Filled")
    drag((LEFT, 436), (LEFT + 145, 480))
    keys("t")
    toggle("Bold")
    colour(PAPER)
    size(22)
    text(LEFT + 18, 445, "Free entry")

    # Mid-session: the rectangle tool, Filled, in coral; the pointer at rest in the tool
    # column, off the image.
    keys("r")
    colour(CORAL)
    move(28, height - 160)
    time.sleep(0.5)
    done()


if __name__ == "__main__":
    main()
