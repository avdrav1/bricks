#!/usr/bin/env python3
"""Read and press what the app shows, through the accessibility tree (AT-SPI).

    a11y_helpers.py text          every name and text in the app's windows, one per line
    a11y_helpers.py press LABEL   activate the button labelled LABEL

Used by scripts/check_malformed.sh (APP-8) to read dialog text exactly as GTK shows it.
Needs python-gobject and at-spi2-core, inside the app's D-Bus session.
"""

import sys

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi  # noqa: E402

APP = "spreadsheet"


def app():
    desktop = Atspi.get_desktop(0)
    for i in range(desktop.get_child_count()):
        a = desktop.get_child_at_index(i)
        if a is not None and a.get_name() == APP:
            return a
    sys.exit(f"no accessible app named {APP!r}")


def walk(node, depth=0):
    if node is None or depth > 60:
        return
    yield node
    for i in range(node.get_child_count()):
        yield from walk(node.get_child_at_index(i), depth + 1)


def texts(node):
    name = node.get_name()
    if name:
        yield name
    text = node.get_text_iface()
    if text is not None:
        content = Atspi.Text.get_text(text, 0, Atspi.Text.get_character_count(text))
        if content and content != name:
            yield content


def main():
    if sys.argv[1:] == ["text"]:
        for node in walk(app()):
            for t in texts(node):
                print(t)
        return
    if len(sys.argv) == 3 and sys.argv[1] == "press":
        label = sys.argv[2]
        for node in walk(app()):
            if node.get_role() == Atspi.Role.BUTTON and node.get_name() == label:
                action = node.get_action_iface()
                if action is None:
                    sys.exit(f"button {label!r} has no action")
                Atspi.Action.do_action(action, 0)
                return
        sys.exit(f"no button {label!r}")
    sys.exit(__doc__)


if __name__ == "__main__":
    main()
