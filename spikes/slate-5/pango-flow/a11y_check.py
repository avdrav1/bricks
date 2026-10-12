#!/usr/bin/env python3
"""What a screen reader gets from the pango-flow spike over AT-SPI (SLATE-5).

    a11y_check.py [APP]                  role, states, caret, text at the caret
    a11y_check.py APP listen SECONDS     print text-changed / caret-moved events
"""

import sys
import time

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi  # noqa: E402

APP = sys.argv[1] if len(sys.argv) > 1 else "pango-flow"


def find_app():
    desktop = Atspi.get_desktop(0)
    for i in range(desktop.get_child_count()):
        a = desktop.get_child_at_index(i)
        if a is not None and a.get_name() == APP:
            return a
    sys.exit(f"no accessible app named {APP!r}")


def walk(node, depth=0):
    if node is None or depth > 60:
        return
    node.clear_cache()
    yield node
    for i in range(node.get_child_count()):
        yield from walk(node.get_child_at_index(i), depth + 1)


def main():
    node = next(
        (n for n in walk(find_app()) if n.get_role() in (Atspi.Role.TEXT, Atspi.Role.ENTRY)
         and n.get_text_iface() is not None),
        None,
    )
    if node is None:
        sys.exit("no text node")
    states = node.get_state_set()
    st = [s.value_nick for s in Atspi.StateType.__enum_values__.values() if states.contains(s)]
    print(f"role {node.get_role_name()!r} name {node.get_name()!r} states {st}")
    t = node.get_text_iface()
    t0 = time.monotonic()
    count = Atspi.Text.get_character_count(t)
    print(f"character count {count} ({(time.monotonic() - t0) * 1000:.1f} ms)")
    caret = Atspi.Text.get_caret_offset(t)
    print(f"caret offset {caret}")
    print(f"n selections {Atspi.Text.get_n_selections(t)}")
    for name in ("CHAR", "WORD", "SENTENCE", "LINE", "PARAGRAPH"):
        g = getattr(Atspi.TextGranularity, name)
        t0 = time.monotonic()
        r = Atspi.Text.get_string_at_offset(t, caret, g)
        ms = (time.monotonic() - t0) * 1000
        s = r.content
        shown = s if len(s) <= 90 else s[:60] + " ... " + s[-25:]
        print(f"{name:9} [{r.start_offset}, {r.end_offset}) {ms:5.1f} ms {shown!r}")
    s = Atspi.Text.get_text(t, max(0, caret - 10), caret + 30)
    print(f"get_text(caret-10, caret+30) {s!r}")
    t0 = time.monotonic()
    whole = Atspi.Text.get_text(t, 0, -1)
    print(f"get_text(0, -1): {len(whole)} chars ({(time.monotonic() - t0) * 1000:.1f} ms)")


def listen(seconds):
    """Print text-changed and caret-moved events (what drives Orca's echo) for a while."""
    from gi.repository import GLib

    def cb(e):
        print(f"{e.type} detail1={e.detail1} detail2={e.detail2} any={e.any_data!r}", flush=True)

    lis = Atspi.EventListener.new(cb)
    for t in ("object:text-changed:insert", "object:text-changed:delete", "object:text-caret-moved"):
        lis.register(t)
    print("listening", flush=True)
    GLib.timeout_add(int(seconds * 1000), lambda: Atspi.event_quit())
    Atspi.event_main()


if __name__ == "__main__":
    if len(sys.argv) > 2 and sys.argv[2] == "listen":
        listen(float(sys.argv[3]))
    else:
        main()
