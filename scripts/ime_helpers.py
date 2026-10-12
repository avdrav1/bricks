"""An input method for scripts/check_slate.sh (SLATE-13), driven by commands.

  ime_helpers.py serve FIFO LOG    under sway: an input method on the seat
                                   (zwp_input_method_v2, scripts/protocols/), which the
                                   compositor relays to the focused window's text input
                                   (zwp_text_input_v3). It reads one command per line from
                                   FIFO and writes what it does and sees to LOG:
      preedit TEXT CURSOR          show TEXT as the preedit, the cursor CURSOR bytes in
      commit TEXT                  replace the preedit with TEXT, committed
      surrounding                  log the window's last surrounding text: "surrounding:
                                   TEXT|CURSOR|ANCHOR" (byte offsets, as the protocol has them)
      delete BEFORE AFTER          delete BEFORE bytes before the cursor and AFTER after it

Needs the generated `im_proto` package (pywayland scanner over
scripts/protocols/input-method-unstable-v2.xml) on the path. The commands come from a FIFO
rather than keys, so composition is exact: no dictionary, no candidate choice.
"""

import os
import selectors
import sys


def serve(fifo, log_path):
    from pywayland.client import Display
    from pywayland.protocol.wayland import WlSeat
    from im_proto.input_method_unstable_v2 import ZwpInputMethodManagerV2

    log = open(log_path, "a", buffering=1, encoding="utf-8")
    display = Display()
    display.connect()
    found = {}

    def on_global(registry, name, interface, version):
        if interface == "zwp_input_method_manager_v2":
            found["manager"] = registry.bind(name, ZwpInputMethodManagerV2, 1)
        elif interface == "wl_seat":
            found["seat"] = registry.bind(name, WlSeat, 1)

    registry = display.get_registry()
    registry.dispatcher["global"] = on_global
    display.roundtrip()
    im = found["manager"].get_input_method(found["seat"])
    state = {"serial": 0, "active": False, "pending": None, "surrounding": None}

    def on_activate(_):
        state["active"] = True

    def on_deactivate(_):
        state["active"] = False

    def on_surrounding(_, text, cursor, anchor):
        state["pending"] = (text, cursor, anchor)

    def on_done(_):
        state["serial"] += 1
        if state["pending"] is not None:
            state["surrounding"] = state["pending"]
        log.write(f"done {state['serial']} active={state['active']}\n")

    def on_unavailable(_):
        log.write("unavailable: another input method holds the seat\n")

    im.dispatcher["activate"] = on_activate
    im.dispatcher["deactivate"] = on_deactivate
    im.dispatcher["surrounding_text"] = on_surrounding
    im.dispatcher["done"] = on_done
    im.dispatcher["unavailable"] = on_unavailable
    display.roundtrip()
    log.write("ready\n")

    def run(line):
        cmd, _, rest = line.partition(" ")
        if cmd == "preedit":
            text, _, cursor = rest.rpartition(" ")
            im.set_preedit_string(text, int(cursor), int(cursor))
            im.commit(state["serial"])
        elif cmd == "commit":
            im.set_preedit_string("", 0, 0)
            im.commit_string(rest)
            im.commit(state["serial"])
        elif cmd == "surrounding":
            s = state["surrounding"]
            log.write("surrounding: " + ("none" if s is None else f"{s[0]}|{s[1]}|{s[2]}") + "\n")
        elif cmd == "delete":
            before, after = rest.split()
            im.delete_surrounding_text(int(before), int(after))
            im.commit(state["serial"])
        log.write(f"did {line}\n")

    fd = os.open(fifo, os.O_RDWR | os.O_NONBLOCK)
    sel = selectors.DefaultSelector()
    sel.register(display.get_fd(), selectors.EVENT_READ, "wayland")
    sel.register(fd, selectors.EVENT_READ, "commands")
    buffered = b""
    while True:
        display.dispatch(block=False)
        display.flush()
        for key, _ in sel.select(timeout=1.0):
            if key.data == "wayland":
                display.dispatch(block=True)
                continue
            buffered += os.read(fd, 4096)
            while b"\n" in buffered:
                line, buffered = buffered.split(b"\n", 1)
                if line.strip():
                    run(line.decode("utf-8"))
        display.flush()


if __name__ == "__main__":
    if len(sys.argv) == 4 and sys.argv[1] == "serve":
        serve(sys.argv[2], sys.argv[3])
    else:
        sys.exit(__doc__)
