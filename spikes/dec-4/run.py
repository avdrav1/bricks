#!/usr/bin/env python3
"""DEC-4 measurement matrix. Throwaway. Run from the repo root after
`cargo build --release` in spikes/dec-4 and `python3 scripts/gen_corpus.py`.

CPU configs: all 24 logical CPUs (pool 23), 8 physical cores via taskset 0-7 (pool 7,
approximating the reference box), and 1 thread."""
import json
import os
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent
FILE = "corpus/rows_1024mb.csv"
CPUS = [("24 cpus / 23", None, None), ("8 cores / 7", "0-7", "7"), ("1 thread", "0", "1")]
RUNS = [("key-column", 2_000_000), ("fetch-compare", 2_000_000), ("key-column", 0)]


def run(binary, rows, cpus, threads):
    cmd = [str(HERE / "target/release" / binary), FILE, str(rows)]
    if cpus:
        cmd = ["taskset", "-c", cpus] + cmd
    env = dict(os.environ)
    if threads:
        env["DEC4_THREADS"] = threads
    return json.loads(subprocess.run(cmd, check=True, capture_output=True, text=True, env=env).stdout)


def main():
    results = []
    for label, cpus, threads in CPUS:
        digests = set()
        for binary, rows in RUNS:
            r = run(binary, rows, cpus, threads)
            r["cpus"] = label
            results.append(r)
            if r["rows"] == 2_000_000:
                digests.add(r["perm_digest"])
            if binary == "key-column":
                best = min(r["sort_stable_pairs_ms"], r["sort_unstable_pairs_ms"], r["sort_argsort_ms"])
                print(
                    f"{label:13} key-column    rows={r['rows']:>8} extract={r['extract_ms']}ms "
                    f"sort stable/unstable/argsort={r['sort_stable_pairs_ms']}/{r['sort_unstable_pairs_ms']}/{r['sort_argsort_ms']}ms "
                    f"total={r['extract_ms'] + best:.0f}ms filter bitmap/list={r['filter_bitmap_ms']}/{r['filter_list_ms']}ms "
                    f"visible={r['visible']} sorted-window p99={r['sorted_window_ms']['p99']:.3f}ms",
                    flush=True,
                )
            else:
                print(
                    f"{label:13} fetch-compare rows={r['rows']:>8} sort={r['sort_ms']}ms compares={r['compares']} "
                    f"filter={r['filter_ms']}ms visible={r['visible']}",
                    flush=True,
                )
        print(f"{label}: 2M permutations identical across options: {len(digests) == 1}", flush=True)
    (HERE / "results.json").write_text(json.dumps(results, indent=1) + "\n")


if __name__ == "__main__":
    main()
