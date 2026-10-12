#!/usr/bin/env python3
"""SLATE-5 measurement driver for both editing-engine spikes. Throwaway; stdlib only.

For each spike binary present (target/release/textview, target/release/pango-flow) and each
window size (1600x1000 and 3800x2080, floating), all at Full width with the 150,000-word
document, it measures (protocol: common.rs header):
  open     spawn -> READY (first frame), spawn -> first L (whole document laid out), peak RSS
           (VmHWM from /proc/<pid>/status at the end of the typing run; wait4 maxrss for the
           self-exiting runs);
  typing   letters injected through Hyprland (send_shortcut over the IPC socket, timestamped
           just before the socket write), spaced 80-150 ms at random, cursor mid-document;
           latency = write -> first F whose revision reached the expected value; plus the
           write -> K, K -> E and E -> F splits;
  scroll   --mode scroll: frame interval, missed vsyncs (interval > 1.5 x 16.67 ms), frame CPU,
           over the frames after the first L;
  rewrap   --mode rewrap (Narrow <-> Full every 90 frames): per switch, W -> first F at the
           new column width, W -> L, and the worst frame interval / frame CPU between W and L;
           plus every L's content height per width (a premature L would make them disagree).
The window opens at its size through a named Hyprland window rule (float, size, move, pin),
so READY and the first L are at the measured size; the rule is disabled at the end.

The textview spike also logs vadjustment upper changes to stderr under SLATE5_CHECK_L=1 (set
in the rewrap runs); the result's "l_check" counts upper changes after an L (must be 0).

Every run sets GSK_DEBUG=renderer; the renderer GTK picked is recorded under "env".

Usage: python3 measure.py [--only textview|pango-flow] [--sizes 1600x1000,3800x2080]
                          [--quick] [--tables N]
Writes results-<size>.json (or results-tables<N>-<size>.json), merged per spike, and prints
a markdown table.
"""
import argparse
import json
import os
import queue
import random
import re
import socket
import statistics
import subprocess
import threading
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
SPIKES = {
    "textview": (HERE / "target/release/textview", "dev.cronkite.Slate5TextView"),
    "pango-flow": (HERE / "target/release/pango-flow", "dev.cronkite.Slate5PangoFlow"),
}
WORDS = 150_000
VSYNC_MS = 1000 / 60
RULE = "slate5-measure"


def hypr(cmd: str) -> str:
    sock = Path(os.environ["XDG_RUNTIME_DIR"]) / "hypr" / os.environ["HYPRLAND_INSTANCE_SIGNATURE"] / ".socket.sock"
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
        s.connect(str(sock))
        s.sendall(cmd.encode())
        out = b""
        while chunk := s.recv(65536):
            out += chunk
    return out.decode()


def class_re(app_id: str) -> str:
    return "^(" + app_id.replace(".", r"\\.") + ")$"


def sel(app_id: str) -> str:
    """Window selector as a Lua long string (Hyprland 0.56 dispatchers are Lua)."""
    return "[[class:^(" + app_id.replace(".", r"\.") + ")$]]"


def dispatch(lua: str):
    out = hypr("dispatch " + lua)
    if out.strip() != "ok":
        raise RuntimeError(f"hyprctl dispatch {lua!r}: {out}")


def lua(code: str) -> str:
    return hypr("eval " + code).strip()


def set_rule(app_id: str, w: int, h: int, enabled=True):
    """One named rule: re-declaring the name replaces it."""
    code = (
        f'hl.window_rule({{name="{RULE}", match={{class="{class_re(app_id)}"}}, float=true, '
        f"size={{{w},{h}}}, move={{20,40}}, pin=true}})"
    )
    if not enabled:
        code += ":set_enabled(false)"
    out = lua(code)
    if out not in ("", "ok"):
        raise RuntimeError(f"window rule: {out}")


class Run:
    def __init__(self, name: str, size, mode: str = "interactive", frames: int = 0, env=None):
        binary, self.app_id = SPIKES[name]
        self.w, self.h = size
        set_rule(self.app_id, self.w, self.h)
        args = [str(binary), "--mode", mode, "--words", str(WORDS), "--width", "full"]
        if frames:
            args += ["--frames", str(frames)]
        # SLATE5_WINDOW: the spike's default window size, so it opens at the measured size.
        env = dict(os.environ, GSK_DEBUG="renderer", SLATE5_WINDOW=f"{self.w}x{self.h}", **(env or {}))
        self.t_spawn = time.monotonic_ns()
        self.proc = subprocess.Popen(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=env)
        self.lines: "queue.Queue[str]" = queue.Queue()
        self.log: list[str] = []  # every stdout line, in order
        self.err: list[str] = []
        threading.Thread(target=self._pump, daemon=True).start()
        threading.Thread(target=self._pump_err, daemon=True).start()
        self.ready_ns = None
        self.ready = None

    def _pump(self):
        for line in self.proc.stdout:
            line = line.rstrip("\n")
            self.log.append(line)
            self.lines.put(line)
        self.lines.put("EOF")

    def _pump_err(self):
        for line in self.proc.stderr:
            self.err.append(line.rstrip("\n"))

    def next(self, timeout=30.0) -> str:
        return self.lines.get(timeout=timeout)

    def wait_ready(self):
        while True:
            line = self.next()
            if line.startswith("READY"):
                self.ready = line.split()[1:]
            elif line.startswith("F") and self.ready_ns is None:
                self.ready_ns = int(line.split()[1])
                break
            elif line == "EOF":
                raise RuntimeError("spike exited before first frame: " + "\n".join(self.err[-5:]))
        # The rule should have opened it at the size; otherwise float/resize until it sticks.
        s = sel(self.app_id)
        deadline = time.monotonic() + 5.0
        while True:
            c = self.client()
            if c and c["floating"] and c["size"] == [self.w, self.h]:
                break
            if time.monotonic() > deadline:
                raise RuntimeError(f"could not place window: {c}")
            if c:
                if not c["floating"]:
                    dispatch(f'hl.dsp.window.float({{action="enable", window={s}}})')
                dispatch(f"hl.dsp.window.resize({{x={self.w}, y={self.h}, exact=true, window={s}}})")
                dispatch(f"hl.dsp.window.move({{x=20, y=40, exact=true, window={s}}})")
            time.sleep(0.05)
        if not c["pinned"]:
            dispatch(f'hl.dsp.window.pin({{action="enable", window={s}}})')
        dispatch(f"hl.dsp.focus({{window={s}}})")
        self.placed_ns = time.monotonic_ns()

    def wait_layout(self, timeout=60.0):
        """Until an L at the column width of the latest frame; returns its time."""
        deadline = time.monotonic() + timeout
        last_l = None
        while time.monotonic() < deadline:
            ls = [l.split() for l in self.log if l.startswith("L")]
            fs = [l.split() for l in self.log if l.startswith("F")]
            if ls and fs and ls[-1][2] == fs[-1][3]:
                last_l = int(ls[-1][1])
                break
            time.sleep(0.05)
        if last_l is None:
            raise RuntimeError("no L at the current width")
        return last_l

    def drain(self):
        try:
            while True:
                self.lines.get_nowait()
        except queue.Empty:
            pass

    def vmhwm_kib(self):
        try:
            for line in Path(f"/proc/{self.proc.pid}/status").read_text().splitlines():
                if line.startswith("VmHWM:"):
                    return int(line.split()[1])
        except OSError:
            return None

    def finish(self, stop=True) -> int:
        """Stop the spike (or, with stop=False, let it exit by itself; 30 s at most); its
        peak RSS in KiB."""
        if stop:
            self.proc.terminate()
        else:
            t = threading.Timer(30.0, lambda: self.proc.poll() is None and self.proc.terminate())
            t.daemon = True
            t.start()
        _, _, ru = os.wait4(self.proc.pid, 0)
        self.proc.returncode = 0  # reaped by wait4; keep Popen from waiting again
        return ru.ru_maxrss

    def client(self):
        for c in json.loads(hypr("j/clients")):
            if c["class"] == self.app_id:
                return c
        return None

    def first_l_ns(self):
        for line in self.log:
            if line.startswith("L"):
                return int(line.split()[1])
        return None

    def renderer(self):
        return sorted({l.strip() for l in self.err if "renderer" in l.lower() or "llvmpipe" in l.lower()})


def pct(xs, p):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(round(p / 100 * (len(xs) - 1))))]


def summary(xs):
    if not xs:
        return None
    return {
        "n": len(xs),
        "p50": round(pct(xs, 50), 2),
        "p95": round(pct(xs, 95), 2),
        "p99": round(pct(xs, 99), 2),
        "max": round(max(xs), 2),
        "mean": round(statistics.fmean(xs), 2),
    }


def parse(log):
    out = []
    for line in log:
        p = line.split()
        if p and p[0] in "KEWFL" and len(p[0]) == 1:
            out.append(p)
    return out


def typing_run(name, size, presses):
    """Open + typing in one interactive run."""
    r = Run(name, size)
    r.wait_ready()
    t_l = r.wait_layout()
    time.sleep(0.5)
    r.drain()
    s = sel(r.app_id)
    rev = max((int(l.split()[2]) for l in r.log if l.startswith("F")), default=0)
    lat, to_k, k_to_e, e_to_f, misses, scrolled = [], [], [], [], 0, 0
    letters = "abcdefghijklmnopqrstuvwxyz"
    top = None
    for i in range(presses):
        key = letters[i % 26]
        want = rev + 1
        t0 = time.monotonic_ns()
        dispatch(f'hl.dsp.send_shortcut({{mods="", key="{key}", window={s}}})')
        t_k = t_e = None
        deadline = time.monotonic() + 1.0
        while True:
            try:
                line = r.lines.get(timeout=max(0.0, deadline - time.monotonic()))
            except queue.Empty:
                misses += 1
                break
            p = line.split()
            if p[0] == "K" and t_k is None and int(p[1]) >= t0:
                t_k = int(p[1])
            elif p[0] == "E" and int(p[2]) == want:
                t_e = int(p[1])
            elif p[0] == "F":
                if int(p[2]) >= want:
                    t_f = int(p[1])
                    lat.append((t_f - t0) / 1e6)
                    if t_k is not None:
                        to_k.append((t_k - t0) / 1e6)
                    if t_k is not None and t_e is not None:
                        k_to_e.append((t_e - t_k) / 1e6)
                    if t_e is not None:
                        e_to_f.append((t_f - t_e) / 1e6)
                    if top is not None and p[5] != top:
                        scrolled += 1
                    top = p[5]
                    rev = int(p[2])
                    break
                top = p[5]
        time.sleep(random.uniform(0.08, 0.15))
    time.sleep(0.3)
    hwm = r.vmhwm_kib()
    r.finish()
    ev = parse(r.log)
    return {
        "open": {
            "spawn_to_ready_ms": round((r.ready_ns - r.t_spawn) / 1e6, 1),
            "spawn_to_first_l_ms": round((r.first_l_ns() - r.t_spawn) / 1e6, 1) if r.first_l_ns() else None,
            "ready": r.ready,
            # The first frame was at the measured size (else open includes a resize).
            "ready_at_size": r.ready is not None and r.ready[:2] == [str(size[0]), str(size[1])],
            "content_h": next((int(p[3]) for p in ev if p[0] == "L"), None),
            "peak_rss_mib": round(hwm / 1024, 1) if hwm else None,
        },
        "typing": {
            "presses": presses,
            "latency_ms": summary(lat),
            "write_to_k_ms": summary(to_k),
            "k_to_e_ms": summary(k_to_e),
            "e_to_f_ms": summary(e_to_f),
            "misses": misses,
            "frames_that_scrolled": scrolled,
            "final_revision": rev,
        },
        "renderer": r.renderer(),
        "stderr_tail": [l for l in r.err if l.startswith(("load:", "build:"))],
    }


def frame_stats(fs):
    """fs: F events (parsed). Interval / CPU stats."""
    ts = [int(p[1]) for p in fs]
    iv = [(b - a) / 1e6 for a, b in zip(ts, ts[1:])]
    cpu = [int(p[4]) / 1000 for p in fs]
    return iv, cpu


def scroll_run(name, size, frames, env=None):
    r = Run(name, size, "scroll", frames, env)
    r.wait_ready()
    while (line := r.next(120)) not in ("DONE", "EOF"):
        pass
    rss = r.finish(stop=False)
    ev = parse(r.log)
    t_l = r.first_l_ns()
    start = max(r.placed_ns, t_l or 0) + int(0.3e9)
    fs = [p for p in ev if p[0] == "F" and int(p[1]) >= start]
    iv, cpu = frame_stats(fs)
    tops = [float(p[5]) for p in fs]
    return {
        "frames_measured": len(fs),
        "interval_ms": summary(iv),
        "missed_vsync": sum(1 for i in iv if i > 1.5 * VSYNC_MS),
        "cpu_ms": summary(cpu),
        "scrolled_px": round(sum(max(0.0, b - a) for a, b in zip(tops, tops[1:]))),
        "spawn_to_ready_ms": round((r.ready_ns - r.t_spawn) / 1e6, 1),
        "spawn_to_first_l_ms": round((t_l - r.t_spawn) / 1e6, 1) if t_l else None,
        "peak_rss_mib": round(rss / 1024, 1),
        "renderer": r.renderer(),
    }


def rewrap_run(name, size, frames):
    r = Run(name, size, "rewrap", frames, {"SLATE5_CHECK_L": "1"})
    r.wait_ready()
    while (line := r.next(120)) not in ("DONE", "EOF"):
        pass
    r.finish(stop=False)
    ev = parse(r.log)
    ws = [i for i, p in enumerate(ev) if p[0] == "W" and int(p[1]) > r.placed_ns]
    switches = []
    heights = {}
    for p in ev:
        if p[0] == "L":
            heights.setdefault(p[2], set()).add(int(p[3]))
    for n, wi in enumerate(ws):
        w = ev[wi]
        t_w, col = int(w[1]), w[2]
        end = ws[n + 1] if n + 1 < len(ws) else len(ev)
        seg = ev[wi + 1:end]
        first_f = next((p for p in seg if p[0] == "F" and p[3] == col), None)
        l = next((p for p in seg if p[0] == "L" and p[2] == col), None)
        prev_f = next((p for p in reversed(ev[:wi]) if p[0] == "F"), None)
        t_end = int(l[1]) if l else (int(ev[end][1]) if end < len(ev) else None)
        fs = ([prev_f] if prev_f else []) + [p for p in seg if p[0] == "F" and (t_end is None or int(p[1]) <= t_end)]
        iv, cpu = frame_stats(fs)
        switches.append({
            "column_w": int(col),
            "cut_by_end": l is None and n + 1 == len(ws),
            "w_to_first_f_ms": round((int(first_f[1]) - t_w) / 1e6, 2) if first_f else None,
            "w_to_l_ms": round((int(l[1]) - t_w) / 1e6, 2) if l else None,
            "worst_interval_ms": round(max(iv), 2) if iv else None,
            "worst_cpu_ms": round(max(cpu[1:] if len(cpu) > 1 else cpu), 2) if cpu else None,
            "missed_vsync_w_to_l": sum(1 for i in iv if i > 1.5 * VSYNC_MS),
        })
    def agg(key, cols=None):
        xs = [s[key] for s in switches if s[key] is not None and (cols is None or s["column_w"] in cols)]
        return {"n": len(xs), "p50": round(pct(xs, 50), 2), "max": round(max(xs), 2)} if xs else None
    by_width = {}
    for c in sorted({s["column_w"] for s in switches}):
        by_width[str(c)] = {k: agg(k, {c}) for k in ("w_to_first_f_ms", "w_to_l_ms", "worst_interval_ms", "worst_cpu_ms")}
    return {
        "switches": switches,
        "unfinished_relayouts": sum(1 for s in switches if s["w_to_l_ms"] is None and not s["cut_by_end"]),
        "w_to_first_f_ms": agg("w_to_first_f_ms"),
        "w_to_l_ms": agg("w_to_l_ms"),
        "worst_interval_ms": agg("worst_interval_ms"),
        "worst_cpu_ms": agg("worst_cpu_ms"),
        "by_width": by_width,
        "content_h_by_width": {k: sorted(v) for k, v in heights.items()},
        "l_check": l_check(r, ev),
        "renderer": r.renderer(),
    }


def l_check(r, ev):
    """textview only (SLATE5_CHECK_L): vadjustment upper changes to a value other than the
    L's content height, after an L and before the next W (a premature L would show some)."""
    ups = [(int(m.group(1)), float(m.group(2))) for l in r.err if (m := re.match(r"U (\d+) (\S+)", l))]
    if not ups:
        return None
    marks = [(int(p[1]), p[0], p[3] if p[0] == "L" else None) for p in ev if p[0] in "WL"]
    late = 0
    for i, (t, kind, h) in enumerate(marks):
        if kind != "L":
            continue
        t_next = next((t2 for t2, k2, _ in marks[i + 1:] if k2 == "W"), None)
        late += sum(1 for tu, u in ups if tu > t and (t_next is None or tu < t_next) and round(u) != int(h))
    checks = [l.split() for l in r.err if l.startswith("LCHECK")]
    mismatch = sum(1 for c in checks if int(float(c[2])) != int(c[3]) + int(c[4]))
    return {
        "l_events": sum(1 for m in marks if m[1] == "L"),
        "upper_notifies": len(ups),
        "upper_changes_after_l": late,
        "lcheck_end_line_mismatch": mismatch,
    }


def env_info():
    def sh(cmd):
        try:
            return subprocess.run(cmd, shell=True, capture_output=True, text=True, timeout=10).stdout.strip()
        except Exception as e:  # noqa: BLE001
            return f"error: {e}"
    return {
        "cpu": sh("lscpu | sed -n 's/^Model name: *//p'"),
        "gpu": sh("lspci | grep -iE 'vga|3d controller' | cut -d: -f3-"),
        "nvidia_smi": sh("nvidia-smi --query-gpu=name,driver_version --format=csv,noheader 2>&1 | head -2"),
        "nvidia_kernel_module": sh("sed -n 1p /proc/driver/nvidia/version 2>/dev/null"),
        "gtk4": sh("pkg-config --modversion gtk4"),
        "hyprland": sh("hyprctl version | head -1"),
        "monitor": sh("hyprctl monitors | grep -E '@'"),
        "GSK_RENDERER": os.environ.get("GSK_RENDERER"),
    }


def label(renderers):
    text = " ".join(renderers)
    if "llvmpipe" in text:
        return "llvmpipe (NVIDIA driver/library mismatch)" if "mismatch" in env_info()["nvidia_smi"].lower() else "llvmpipe"
    m = re.search(r"Using renderer '(\w+)'", text)
    return m.group(1) if m else "unknown"


def measure(name, size, a):
    q = a.quick
    res = {}
    if a.tables:
        env = {"SLATE5_TABLES": str(a.tables)}
        r = Run(name, size, env=env)
        r.wait_ready()
        r.wait_layout()
        time.sleep(0.3)
        hwm = r.vmhwm_kib()
        r.finish()
        res["open"] = {
            "spawn_to_ready_ms": round((r.ready_ns - r.t_spawn) / 1e6, 1),
            "spawn_to_first_l_ms": round((r.first_l_ns() - r.t_spawn) / 1e6, 1),
            "peak_rss_mib": round(hwm / 1024, 1) if hwm else None,
            "stderr_tail": [l for l in r.err if l.startswith(("load:", "build:"))],
        }
        res["scroll"] = scroll_run(name, size, 300 if q else 900, env)
        res["renderer"] = res["scroll"].pop("renderer")
        return res
    t = typing_run(name, size, 30 if q else 100)
    res["open"], res["typing"] = t["open"], t["typing"]
    res["open"]["stderr_tail"] = t["stderr_tail"]
    res["scroll"] = scroll_run(name, size, 300 if q else 900)
    # Switches at frames 90, 180, ...; 60 spare frames let the last relayout finish.
    res["rewrap"] = rewrap_run(name, size, 510 if q else 960)
    res["renderer"] = sorted(set(t["renderer"]) | set(res["scroll"].pop("renderer")) | set(res["rewrap"].pop("renderer")))
    return res


def fmt(x, key="p50"):
    if x is None:
        return "-"
    if isinstance(x, dict):
        return "-" if x.get(key) is None else f"{x[key]:g}"
    return f"{x:g}"


def table(all_results):
    cols = [(size, name) for size, per in all_results.items() for name in per if name != "env"]
    rows = [
        ("open: spawn -> READY ms", lambda r: fmt(r["open"]["spawn_to_ready_ms"])),
        ("open: spawn -> whole doc laid out (L) ms", lambda r: fmt(r["open"]["spawn_to_first_l_ms"])),
        ("peak RSS MiB", lambda r: fmt(r["open"]["peak_rss_mib"])),
        ("typing latency p50/p95/p99/max ms", lambda r: "/".join(fmt(r["typing"]["latency_ms"], k) for k in ("p50", "p95", "p99", "max")) if "typing" in r else "-"),
        ("typing K->E p50/p99 ms", lambda r: "/".join(fmt(r["typing"]["k_to_e_ms"], k) for k in ("p50", "p99")) if "typing" in r else "-"),
        ("typing E->F p50/p99 ms", lambda r: "/".join(fmt(r["typing"]["e_to_f_ms"], k) for k in ("p50", "p99")) if "typing" in r else "-"),
        ("typing misses", lambda r: fmt(r["typing"]["misses"]) if "typing" in r else "-"),
        ("scroll interval p50/p99 ms", lambda r: "/".join(fmt(r["scroll"]["interval_ms"], k) for k in ("p50", "p99"))),
        ("scroll missed vsyncs / frames", lambda r: f'{r["scroll"]["missed_vsync"]} / {r["scroll"]["frames_measured"]}'),
        ("scroll frame CPU p50/p99 ms", lambda r: "/".join(fmt(r["scroll"]["cpu_ms"], k) for k in ("p50", "p99"))),
        ("rewrap W -> first F p50/max ms", lambda r: "/".join(fmt(r["rewrap"]["w_to_first_f_ms"], k) for k in ("p50", "max")) if "rewrap" in r else "-"),
        ("rewrap W -> L p50/max ms", lambda r: "/".join(fmt(r["rewrap"]["w_to_l_ms"], k) for k in ("p50", "max")) if "rewrap" in r else "-"),
        ("rewrap worst interval W..L p50/max ms", lambda r: "/".join(fmt(r["rewrap"]["worst_interval_ms"], k) for k in ("p50", "max")) if "rewrap" in r else "-"),
        ("rewrap worst frame CPU W..L p50/max ms", lambda r: "/".join(fmt(r["rewrap"]["worst_cpu_ms"], k) for k in ("p50", "max")) if "rewrap" in r else "-"),
        ("rewrap unfinished relayouts", lambda r: fmt(r["rewrap"]["unfinished_relayouts"]) if "rewrap" in r else "-"),
        ("renderer", lambda r: r.get("renderer_label", "-")),
    ]
    out = ["| metric | " + " | ".join(f"{n} {s}" for s, n in cols) + " |", "|---" * (len(cols) + 1) + "|"]
    for title, f in rows:
        cells = []
        for s, n in cols:
            try:
                cells.append(f(all_results[s][n]))
            except (KeyError, TypeError):
                cells.append("-")
        out.append(f"| {title} | " + " | ".join(cells) + " |")
    return "\n".join(out)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--only", choices=list(SPIKES))
    ap.add_argument("--sizes", default="1600x1000,3800x2080")
    ap.add_argument("--quick", action="store_true", help="fewer samples")
    ap.add_argument("--tables", type=int, default=0, help="embed N tables (textview: SLATE5_TABLES); open + scroll only")
    a = ap.parse_args()
    names = [n for n in ([a.only] if a.only else SPIKES) if SPIKES[n][0].exists()]
    if not names:
        raise SystemExit("no spike binaries; cargo build --release first")
    sizes = [tuple(map(int, s.split("x"))) for s in a.sizes.split(",")]
    env = env_info()
    all_results = {}
    try:
        for w, h in sizes:
            key = f"{w}x{h}"
            path = HERE / (f"results-tables{a.tables}-{key}.json" if a.tables else f"results-{key}.json")
            data = json.loads(path.read_text()) if path.exists() else {}
            data["env"] = env
            for name in names:
                print(f"== {name} {key}", flush=True)
                res = measure(name, (w, h), a)
                res["renderer_label"] = label(res["renderer"])
                res["quick"] = a.quick
                res["measured_at"] = time.strftime("%Y-%m-%d %H:%M:%S")
                data[name] = res
                path.write_text(json.dumps(data, indent=1) + "\n")
                print(json.dumps({k: v for k, v in res.items() if k != "rewrap"}), flush=True)
            all_results[key] = data
    finally:
        for name in names:
            set_rule(SPIKES[name][1], 100, 100, enabled=False)
    print()
    print(table(all_results))


if __name__ == "__main__":
    main()
