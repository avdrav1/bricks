#!/usr/bin/env python3
"""DEC-1 measurement driver. Throwaway.

Runs each spike under Hyprland, floats it at a fixed 1600x1000, and measures:
  - frame time (interval between finished frames, and toolkit-reported frame CPU)
    under two autopilots: smooth fast scroll and random jumps (scrollbar drag stress);
  - input latency: Page_Down injected through the compositor (hyprctl sendshortcut over
    the IPC socket, timestamped right before the write) until the first finished frame
    showing the new top row;
  - cold start (spawn -> first finished frame) and peak RSS.

All timestamps are CLOCK_MONOTONIC, shared with the spikes (see common.rs).

Usage: python3 measure.py [--rows 1000000] [--only gtk4|egui] [--out results.json]
"""
import argparse
import json
import os
import queue
import random
import socket
import statistics
import subprocess
import threading
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
SPIKES = {
    "gtk4": (HERE / "target/release/gtk4-grid", "dev.bricks.Dec1Gtk4"),
    "egui": (HERE / "target/release/egui-grid", "dev.bricks.Dec1Egui"),
}
WIN_W, WIN_H = 1600, 1000
WARMUP_S = 1.5


def hypr(cmd: str) -> str:
    sock = Path(os.environ["XDG_RUNTIME_DIR"]) / "hypr" / os.environ["HYPRLAND_INSTANCE_SIGNATURE"] / ".socket.sock"
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
        s.connect(str(sock))
        s.sendall(cmd.encode())
        out = b""
        while chunk := s.recv(65536):
            out += chunk
    return out.decode()


def sel(app_id: str) -> str:
    """Window selector as a Lua long string (Hyprland 0.56 dispatchers are Lua)."""
    return "[[class:^(" + app_id.replace(".", r"\.") + ")$]]"


def dispatch(lua: str):
    out = hypr("dispatch " + lua)
    if out.strip() != "ok":
        raise RuntimeError(f"hyprctl dispatch {lua!r}: {out}")


class Run:
    def __init__(self, name: str, mode: str, rows: int, frames: int = 0):
        binary, self.app_id = SPIKES[name]
        args = [str(binary), "--mode", mode, "--rows", str(rows)]
        if frames:
            args += ["--frames", str(frames)]
        env = dict(os.environ, RUST_LOG="error")
        self.t_spawn = time.monotonic_ns()
        self.proc = subprocess.Popen(args, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, env=env)
        self.lines: "queue.Queue[str]" = queue.Queue()
        threading.Thread(target=self._pump, daemon=True).start()
        self.first_frame_ns = None

    def _pump(self):
        for line in self.proc.stdout:
            self.lines.put(line.rstrip("\n"))
        self.lines.put("EOF")

    def next(self, timeout=10.0) -> str:
        return self.lines.get(timeout=timeout)

    def wait_ready(self):
        while True:
            line = self.next()
            if line.startswith("READY"):
                continue
            elif line.startswith("F") and self.first_frame_ns is None:
                self.first_frame_ns = int(line.split()[1])
                break
            elif line == "EOF":
                raise RuntimeError("spike exited before first frame")
        # Some toolkits paint before the compositor maps the toplevel; wait until Hyprland
        # lists it, then float/resize until the geometry sticks.
        s = sel(self.app_id)
        deadline = time.monotonic() + 5.0
        while True:
            c = self.client()
            if c and c["floating"] and c["size"] == [WIN_W, WIN_H]:
                break
            if time.monotonic() > deadline:
                raise RuntimeError(f"could not place window: {c}")
            if c:
                if not c["floating"]:
                    dispatch(f'hl.dsp.window.float({{action="enable", window={s}}})')
                dispatch(f"hl.dsp.window.resize({{x={WIN_W}, y={WIN_H}, exact=true, window={s}}})")
                dispatch(f"hl.dsp.window.move({{x=40, y=40, exact=true, window={s}}})")
            time.sleep(0.05)
        # Pinned floating windows stay visible across workspace switches; a hidden
        # window gets no frame callbacks and would stall the run.
        dispatch(f'hl.dsp.window.pin({{action="enable", window={s}}})')
        dispatch(f"hl.dsp.focus({{window={s}}})")
        self.placed_ns = time.monotonic_ns()

    def finish(self) -> int:
        if self.proc.poll() is None:
            self.proc.terminate()
        _, _, ru = os.wait4(self.proc.pid, 0)
        return ru.ru_maxrss  # KiB

    def client(self):
        for c in json.loads(hypr("j/clients")):
            if c["class"] == self.app_id:
                return c
        return None

    def geometry(self):
        c = self.client()
        return c and c["size"]


def pct(xs, p):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(round(p / 100 * (len(xs) - 1))))]


def summary(xs):
    return {
        "n": len(xs),
        "p50": round(pct(xs, 50), 2),
        "p95": round(pct(xs, 95), 2),
        "p99": round(pct(xs, 99), 2),
        "max": round(max(xs), 2),
        "mean": round(statistics.fmean(xs), 2),
    }


def frame_run(name, mode, rows, frames=1000):
    r = Run(name, mode, rows, frames)
    r.wait_ready()
    geom = r.geometry()
    frames_ns, cpu_us, snap_us = [], [], []
    t_measure = r.placed_ns + int(0.5e9)
    while (line := r.next(30)) not in ("DONE", "EOF"):
        parts = line.split()
        if parts[0] == "F" and int(parts[1]) >= t_measure:
            frames_ns.append(int(parts[1]))
            cpu_us.append(int(parts[3]))
            if len(parts) > 4:
                snap_us.append(int(parts[4]))
    rss = r.finish()
    intervals = [(b - a) / 1e6 for a, b in zip(frames_ns, frames_ns[1:])]
    return {
        "mode": mode,
        "rows": rows,
        "window": geom,
        "interval_ms": summary(intervals),
        "cpu_ms": summary([c / 1000 for c in cpu_us]),
        "missed_vsync": sum(1 for i in intervals if i > 16.7 * 1.5),
        "cold_start_ms": round((r.first_frame_ns - r.t_spawn) / 1e6, 1),
        "peak_rss_mib": round(rss / 1024, 1),
        "snapshot_ms": summary([s / 1000 for s in snap_us]) if snap_us else None,
    }


def latency_run(name, rows, presses=80):
    r = Run(name, "interactive", rows)
    r.wait_ready()
    time.sleep(WARMUP_S)
    s = sel(r.app_id)
    # Drain anything produced by the float/resize.
    top = None
    try:
        while True:
            line = r.lines.get(timeout=0.3)
            if line.startswith("F"):
                top = line.split()[2]
    except queue.Empty:
        pass
    geom = r.geometry()
    to_frame, to_event, misses = [], [], 0
    for _ in range(presses):
        t0 = time.monotonic_ns()
        dispatch(f'hl.dsp.send_shortcut({{mods="", key="Page_Down", window={s}}})')
        t_key = None
        deadline = time.monotonic() + 1.0
        while True:
            try:
                line = r.lines.get(timeout=max(0.0, deadline - time.monotonic()))
            except queue.Empty:
                misses += 1
                break
            parts = line.split()
            if parts[0] == "K" and t_key is None:
                t_key = int(parts[1])
            elif parts[0] == "F" and parts[2] != top:
                top = parts[2]
                to_frame.append((int(parts[1]) - t0) / 1e6)
                if t_key is not None:
                    to_event.append((t_key - t0) / 1e6)
                break
        time.sleep(random.uniform(0.08, 0.15))  # de-phase from vsync
    r.finish()
    return {
        "rows": rows,
        "window": geom,
        "input_to_frame_ms": summary(to_frame),
        "input_to_app_event_ms": summary(to_event) if to_event else None,
        "misses": misses,
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--rows", type=int, default=1_000_000)
    ap.add_argument("--win", default="1600x1000", help="floating window size WxH")
    ap.add_argument("--only", choices=list(SPIKES))
    ap.add_argument("--modes", default="scroll,jump,latency")
    ap.add_argument("--out", default=str(HERE / "results.json"))
    a = ap.parse_args()
    global WIN_W, WIN_H
    WIN_W, WIN_H = map(int, a.win.split("x"))
    names = [a.only] if a.only else list(SPIKES)
    results = {}
    for name in names:
        res = {}
        for mode in a.modes.split(","):
            res[mode] = latency_run(name, a.rows) if mode == "latency" else frame_run(name, mode, a.rows)
            print(name, mode, json.dumps(res[mode]), flush=True)
        results[name] = res
    Path(a.out).write_text(json.dumps(results, indent=1) + "\n")


if __name__ == "__main__":
    main()
