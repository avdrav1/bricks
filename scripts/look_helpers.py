"""Helpers for scripts/check_look.sh (APP-6).

  look_helpers.py portal FILE      own org.freedesktop.portal.Desktop on the session bus
                                   and serve its Settings interface: color-scheme is the
                                   number in FILE; on SIGUSR1 it is read again and
                                   SettingChanged sent, as a desktop does when the user
                                   flips dark mode
  look_helpers.py grab DISPLAY OUT screenshot an X display to OUT (PNG)
  look_helpers.py theme PNG...     the grid's background and text brightness per shot
  look_helpers.py sharp BASE SCALE PNG
                                   how crisp the grid's cell text is in PNG, taken at
                                   SCALE; also for BASE (the same window at 1x) scaled
                                   up by SCALE, which is what a blurry, compositor-scaled
                                   window looks like
"""

import signal
import sys

NS, KEY = "org.freedesktop.appearance", "color-scheme"
# Logical (scale-independent) boxes in a maximized window showing check_look's CSV:
# cell text of rows 1-3 in columns A-C, and empty grid body below the rows.
TEXT = (34, 112, 380, 200)
EMPTY = (40, 260, 380, 400)


def portal(path):
    from gi.repository import Gio, GLib

    xml = """<node><interface name="org.freedesktop.portal.Settings">
      <method name="ReadOne"><arg type="s" direction="in"/><arg type="s" direction="in"/>
        <arg type="v" direction="out"/></method>
      <method name="Read"><arg type="s" direction="in"/><arg type="s" direction="in"/>
        <arg type="v" direction="out"/></method>
      <signal name="SettingChanged"><arg type="s"/><arg type="s"/><arg type="v"/></signal>
      <property name="version" type="u" access="read"/>
    </interface></node>"""
    iface = Gio.DBusNodeInfo.new_for_xml(xml).interfaces[0]
    state = {}

    def current():
        return GLib.Variant("u", int(open(path).read()))

    def on_call(conn, sender, obj, name, method, params, inv):
        ns, key = params.unpack()
        if (ns, key) != (NS, KEY):
            inv.return_dbus_error("org.freedesktop.portal.Error.NotFound", "no such setting")
            return
        value = current() if method == "ReadOne" else GLib.Variant("v", current())
        inv.return_value(GLib.Variant("(v)", (value,)))

    def on_property(conn, sender, obj, name, prop):
        return GLib.Variant("u", 2)

    def on_bus(conn, name):
        state["conn"] = conn
        conn.register_object("/org/freedesktop/portal/desktop", iface, on_call, on_property, None)

    def on_usr1():
        state["conn"].emit_signal(
            None, "/org/freedesktop/portal/desktop", "org.freedesktop.portal.Settings",
            "SettingChanged", GLib.Variant("(ssv)", (NS, KEY, current())),
        )
        return True

    Gio.bus_own_name(Gio.BusType.SESSION, "org.freedesktop.portal.Desktop",
                     Gio.BusNameOwnerFlags.NONE, on_bus, None, None)
    GLib.unix_signal_add(GLib.PRIORITY_DEFAULT, signal.SIGUSR1, on_usr1)
    GLib.MainLoop().run()


def grab(display, out):
    from PIL import ImageGrab

    ImageGrab.grab(xdisplay=display).save(out)


def gray(path, scale, box):
    import numpy as np
    from PIL import Image

    img = Image.open(path) if isinstance(path, str) else path
    x0, y0, x1, y1 = (int(round(v * scale)) for v in box)
    return np.asarray(img.convert("L").crop((x0, y0, x1, y1)), dtype=float) / 255


def theme(paths):
    for p in paths:
        bg = float(gray(p, 1, EMPTY).mean())
        text = gray(p, 1, TEXT)
        # The strongest ink: farthest from the background, whichever way.
        ink = float(text.max() if bg < 0.5 else text.min())
        print(f"{p}: background {bg:.2f}, text {ink:.2f}")


def ink_ratio(a):
    """Of the pixels at least half-way from background to ink, the share nearly at ink.
    Crisp text has solid strokes; blur turns them into ramps of in-between grays. Ink is
    whichever extreme lies farther from the background: dark text or light text."""
    bg = float(sorted(a.ravel())[a.size // 2])
    ink = float(a.min()) if bg - a.min() >= a.max() - bg else float(a.max())
    d = (a - bg) / (ink - bg) if ink != bg else a * 0
    half = int((d > 0.5).sum())
    return float((d > 0.85).sum()) / max(half, 1)


def sharp(base, scale, path):
    from PIL import Image

    scale = float(scale)
    b = Image.open(base)
    up = b.resize((round(b.size[0] * scale), round(b.size[1] * scale)), Image.BILINEAR)
    print(f"{path}: {ink_ratio(gray(path, scale, TEXT)):.2f}"
          f" base {ink_ratio(gray(base, 1, TEXT)):.2f}"
          f" upscaled {ink_ratio(gray(up, scale, TEXT)):.2f}")


if __name__ == "__main__":
    cmd, args = sys.argv[1], sys.argv[2:]
    if cmd == "portal":
        portal(*args)
    elif cmd == "grab":
        grab(*args)
    elif cmd == "theme":
        theme(args)
    elif cmd == "sharp":
        sharp(*args)
    else:
        sys.exit(__doc__)
