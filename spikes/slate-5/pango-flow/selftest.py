#!/usr/bin/env python3
"""Quick self-test of the pango-flow spike (measure.py does the real runs).

    selftest.py scroll|rewrap|type|a11y [extra args...]

Floats the window at 1600x1000 under Hyprland, collects the protocol lines, and prints a
summary. `type` injects a few keys; `a11y` reads the caret paragraph over AT-SPI before and
after typing.
"""

import json
import os
import socket
import statistics
import subprocess
import sys
import threading
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
BIN = HERE.parent / "target" / "release" / "pango-flow"
APP_ID = "dev.cronkite.Slate5PangoFlow"
W, H = 1600, 1000


def hypr(cmd: str) -> str:
    sock = Path(os.environ["XDG_RUNTIME_DIR"]) / "hypr" / os.environ["HYPRLAND_INSTANCE_SIGNATURE"] / ".socket.sock"
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
        s.connect(str(sock))
        s.sendall(cmd.encode())
        out = b""
        while chunk := s.recv(65536):
            out += chunk
    return out.decode()


SEL = "[[class:^(" + APP_ID.replace(".", r"\.") + ")$]]"


def dispatch(lua: str):
    out = hypr("dispatch " + lua)
    if out.strip() != "ok":
        raise RuntimeError(f"dispatch {lua!r}: {out}")


def client():
    for c in json.loads(hypr("j/clients")):
        if c["class"] == APP_ID:
            return c
    return None


class Run:
    def __init__(self, args):
        self.t0 = time.monotonic_ns()
        self.p = subprocess.Popen([str(BIN), *args], stdout=subprocess.PIPE, text=True)
        self.lines = []
        self.ready = threading.Event()
        self.done = threading.Event()
        threading.Thread(target=self._read, daemon=True).start()

    def _read(self):
        for line in self.p.stdout:
            self.lines.append(line.split())
            if line.startswith("READY"):
                self.ready.set()
        self.done.set()

    def place(self):
        deadline = time.monotonic() + 5
        while True:
            c = client()
            if c and c["floating"] and c["size"] == [W, H]:
                break
            if time.monotonic() > deadline:
                raise RuntimeError(f"could not place window: {c}")
            if c:
                if not c["floating"]:
                    dispatch(f'hl.dsp.window.float({{action="enable", window={SEL}}})')
                dispatch(f"hl.dsp.window.resize({{x={W}, y={H}, exact=true, window={SEL}}})")
                dispatch(f"hl.dsp.window.move({{x=40, y=40, exact=true, window={SEL}}})")
            time.sleep(0.05)
        dispatch(f'hl.dsp.window.pin({{action="enable", window={SEL}}})')
        dispatch(f"hl.dsp.focus({{window={SEL}}})")

    def key(self, key, mods=""):
        t = time.monotonic_ns()
        dispatch(f'hl.dsp.send_shortcut({{mods="{mods}", key="{key}", window={SEL}}})')
        return t

    def close(self):
        if self.p.poll() is None:
            self.p.terminate()
        self.p.wait(5)


def frames(lines, since=0):
    return [l for l in lines if l[0] == "F" and int(l[1]) >= since]


def pct(v, q):
    v = sorted(v)
    return v[min(len(v) - 1, int(q * len(v)))]


def summary_frames(fs, label):
    cpu = [int(f[4]) for f in fs]
    ts = [int(f[1]) for f in fs]
    iv = [(b - a) / 1e6 for a, b in zip(ts, ts[1:])]
    if not cpu:
        print(label, "no frames")
        return
    print(f"{label}: {len(fs)} frames; cpu_us p50 {pct(cpu, .5)} p95 {pct(cpu, .95)} max {max(cpu)};"
          f" interval ms p50 {pct(iv, .5):.1f} p95 {pct(iv, .95):.1f} max {max(iv):.1f}")


def main():
    mode = sys.argv[1]
    extra = sys.argv[2:]
    if mode in ("scroll", "rewrap"):
        r = Run(["--mode", mode, *extra])
        r.ready.wait(20)
        r.place()
        r.done.wait(120)
        r.close()
        ready = next(l for l in r.lines if l[0] == "READY")
        print("READY", *ready[1:])
        L = [l for l in r.lines if l[0] == "L"]
        print("L lines:", [(l[2], l[3]) for l in L])
        if mode == "scroll":
            summary_frames(frames(r.lines), "scroll")
        else:
            Ws = [l for l in r.lines if l[0] == "W"]
            summary_frames(frames(r.lines), "all frames")
            for w in Ws:
                t = int(w[1])
                col = w[2]
                first = next((f for f in r.lines if f[0] == "F" and int(f[1]) > t and f[3] == col), None)
                l = next((x for x in r.lines if x[0] == "L" and int(x[1]) > t and x[2] == col), None)
                nxt = next((x for x in Ws if int(x[1]) > t), None)
                if l and nxt and int(l[1]) > int(nxt[1]):
                    l = None
                print(f"W {col}: first frame {((int(first[1]) - t) / 1e6) if first else None:.1f} ms"
                      f" (cpu {first[4] if first else '-'} us); L {((int(l[1]) - t) / 1e6) if l else 'none before next W'}"
                      f" ms content_h {l[3] if l else '-'}")
        return
    r = Run(["--mode", "interactive", *extra])
    r.ready.wait(20)
    r.place()
    time.sleep(1.5)
    if mode == "a11y":
        print(subprocess.run([sys.executable, str(HERE / "a11y_check.py")], capture_output=True, text=True).stdout)
    lat = []
    for k in "hello":
        n0 = len(r.lines)
        t = r.key(k)
        deadline = time.monotonic() + 1
        while time.monotonic() < deadline:
            e = next((l for l in r.lines[n0:] if l[0] == "E"), None)
            if e:
                f = next((l for l in r.lines[n0:] if l[0] == "F" and int(l[2]) >= int(e[2])), None)
                if f:
                    lat.append(((int(f[1]) - t) / 1e6, int(f[4])))
                    break
            time.sleep(0.002)
        time.sleep(0.15)
    print("typing key->frame ms, frame cpu us:", lat)
    if mode == "a11y":
        print(subprocess.run([sys.executable, str(HERE / "a11y_check.py")], capture_output=True, text=True).stdout)
    for k in ("Left", "Left", "BackSpace", "Down", "Up"):
        r.key(k)
        time.sleep(0.1)
    if mode == "a11y":
        print(subprocess.run([sys.executable, str(HERE / "a11y_check.py")], capture_output=True, text=True).stdout)
    r.close()
    for l in r.lines:
        if l[0] in ("K", "E", "L", "W", "READY"):
            print(" ".join(l))


if __name__ == "__main__":
    main()
