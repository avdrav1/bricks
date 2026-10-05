#!/usr/bin/env python3
"""DEC-5 measurement matrix. Throwaway. Run from the repo root after
`cargo build --release` in spikes/dec-5 and `python3 scripts/gen_corpus.py`.

Configs: all 24 logical CPUs, and CPUs 0-7 (8 physical cores, approximating the 8-core
reference box) with the pool sized to all 8 or to 7 (one core left for the UI thread).
"""
import json
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent
CONFIGS = [("all-24", None, 24), ("8-core", "0-7", 8), ("8-core", "0-7", 7)]
REPS = 2


def run(engine, cpus, threads):
    cmd = [str(HERE / f"target/release/{engine}-jobs"), "--threads", str(threads), "--runs", "20", "--cancel-secs", "5"]
    if cpus:
        cmd = ["taskset", "-c", cpus] + cmd
    out = subprocess.run(cmd, check=True, capture_output=True, text=True).stdout
    return json.loads(out.replace("NaN", "null"))


def main():
    results = []
    for label, cpus, threads in CONFIGS:
        for engine in ("rayon", "tokio"):
            for rep in range(REPS):
                r = run(engine, cpus, threads)
                r.update(config=label, rep=rep)
                results.append(r)
                s, c = r["phases"]["search"], r["phases"]["cancel"]
                warm = sorted(r["search_ms"][1:])
                print(
                    f"{label:7} {engine:5} t={threads:2} rep={rep} "
                    f"search p50={warm[len(warm) // 2]:.0f}ms "
                    f"first-hit p50={r['first_hit_ms']['p50']:.1f} "
                    f"cancel ui p50/p99/max={r['cancel']['ui_observed_ms']['p50']:.1f}/"
                    f"{r['cancel']['ui_observed_ms']['p99']:.1f}/{r['cancel']['ui_observed_ms']['max']:.1f} "
                    f"| search frames={s['frames']} done p99={s['done_after_deadline_ms']['p99']:.1f} over={s['over_budget']} "
                    f"| cancel frames={c['frames']} late p99={c['start_late_ms']['p99']:.1f} "
                    f"done p99/max={c['done_after_deadline_ms']['p99']:.1f}/{c['done_after_deadline_ms']['max']:.1f} "
                    f"over={c['over_budget']} skipped={c['skipped_vsyncs']} lag p99={c['progress_lag_ms']['p99']:.1f}",
                    flush=True,
                )
    (HERE / "results.json").write_text(json.dumps(results, indent=1) + "\n")


if __name__ == "__main__":
    main()
