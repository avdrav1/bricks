"""Helpers for scripts/check_drop.sh (APP-5): a drag source and a Wayland pointer.

  dnd_helpers.py source FILE           a GTK 4 window, titled "dragsrc", whose whole area
                                       drags FILE as text/uri-list, as file managers do
  dnd_helpers.py drag X1 Y1 X2 Y2 [HOLD]
                                       under a wlroots compositor: press at (X1, Y1), move
                                       to (X2, Y2) in steps, wait HOLD seconds (0.5),
                                       release; through the wlr-virtual-pointer protocol,
                                       so no real input device is needed. Needs the
                                       generated `wlr_proto` package (pywayland scanner) on
                                       the path.
  dnd_helpers.py click X Y [N]         the same pointer: a quick click at (X, Y), as a
                                       hand makes one (GTK takes a press held past the
                                       double-click time as a long press); N clicks in a
                                       row (1) make a double or triple click
  dnd_helpers.py wheel X Y NOTCHES     the same pointer at (X, Y) turns a mouse wheel
                                       NOTCHES notches down (negative: up)
"""

import sys
import time


def source(path):
    import gi

    gi.require_version("Gtk", "4.0")
    gi.require_version("Gdk", "4.0")
    from gi.repository import Gdk, Gio, GLib, Gtk

    uri = Gio.File.new_for_path(path).get_uri()

    def activate(app):
        win = Gtk.ApplicationWindow(application=app, title="dragsrc")
        win.set_default_size(600, 600)
        area = Gtk.Box(hexpand=True, vexpand=True)
        area.append(Gtk.Label(label="drag " + path, hexpand=True))
        drag = Gtk.DragSource(actions=Gdk.DragAction.COPY)
        drag.connect(
            "prepare",
            lambda *_: Gdk.ContentProvider.new_for_bytes(
                "text/uri-list", GLib.Bytes.new((uri + "\r\n").encode())
            ),
        )
        area.add_controller(drag)
        win.set_child(area)
        win.present()

    app = Gtk.Application(application_id="dev.brick.DragSource", flags=Gio.ApplicationFlags.NON_UNIQUE)
    app.connect("activate", activate)
    app.run([])


def pointer(width=1280, height=720):
    """A virtual pointer: its `move(x, y)`, `button(pressed, hold)`, `wheel(notch)`, and
    `done()`."""
    from pywayland.client import Display
    from pywayland.protocol.wayland import WlSeat
    from wlr_proto.wlr_virtual_pointer_unstable_v1 import ZwlrVirtualPointerManagerV1

    display = Display()
    display.connect()
    found = {}

    def on_global(registry, name, interface, version):
        if interface == "zwlr_virtual_pointer_manager_v1":
            found["manager"] = registry.bind(name, ZwlrVirtualPointerManagerV1, 1)
        elif interface == "wl_seat":
            found["seat"] = registry.bind(name, WlSeat, 1)

    registry = display.get_registry()
    registry.dispatcher["global"] = on_global
    display.roundtrip()
    pointer = found["manager"].create_virtual_pointer(found["seat"])
    display.roundtrip()
    t0 = time.monotonic()

    def ms():
        return int((time.monotonic() - t0) * 1000)

    def move(x, y):
        pointer.motion_absolute(ms(), int(x), int(y), width, height)
        pointer.frame()
        display.flush()
        time.sleep(0.03)

    def button(pressed, hold=0.2):
        pointer.button(ms(), 0x110, 1 if pressed else 0)  # BTN_LEFT
        pointer.frame()
        display.flush()
        time.sleep(hold)

    def wheel(notch):
        # One wheel notch: 15 px of vertical scroll (axis 0), as libinput reports it.
        pointer.axis_source(0)  # wheel
        pointer.axis_discrete(ms(), 0, 15.0 * notch, notch)
        pointer.frame()
        display.flush()
        time.sleep(0.03)

    def done():
        display.roundtrip()
        display.disconnect()

    return move, button, wheel, done


def drag(x1, y1, x2, y2, hold=0.5):
    move, button, _, done = pointer()
    move(x1, y1)
    time.sleep(0.5)
    button(True)
    steps = 40
    for i in range(1, steps + 1):
        move(x1 + (x2 - x1) * i / steps, y1 + (y2 - y1) * i / steps)
    time.sleep(hold)
    button(False)
    time.sleep(0.5)
    done()


def click(x, y, times=1):
    move, button, _, done = pointer()
    move(x, y)
    time.sleep(0.3)
    for _ in range(int(times)):
        button(True, hold=0.05)
        button(False, hold=0.08)
    time.sleep(0.3)
    done()


def wheel(x, y, notches):
    move, _, turn, done = pointer()
    move(x, y)
    time.sleep(0.3)
    for _ in range(abs(notches)):
        turn(1 if notches > 0 else -1)
    time.sleep(0.3)
    done()


if __name__ == "__main__":
    if sys.argv[1] == "source":
        source(sys.argv[2])
    elif sys.argv[1] == "drag":
        drag(*map(float, sys.argv[2:7]))
    elif sys.argv[1] == "click":
        click(*map(float, sys.argv[2:5]))
    elif sys.argv[1] == "wheel":
        wheel(float(sys.argv[2]), float(sys.argv[3]), int(sys.argv[4]))
    else:
        sys.exit(__doc__)
