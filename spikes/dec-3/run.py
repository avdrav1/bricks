#!/usr/bin/env python3
"""DEC-3 measurement matrix. Throwaway. Run from the repo root after
`cargo build --release` in spikes/dec-3 and `python3 scripts/gen_corpus.py`.

For each corpus size, runs every model with the same op sequence, checks that all saved
files are byte-identical, and (100 MB) that the save differs from the source only in
edited or inserted rows."""
import hashlib
import json
import os
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent
MODELS = [
    ("overlay-rowmap", {"DEC3_ROWMAP": "treap"}, "overlay-treap"),
    ("overlay-rowmap", {"DEC3_ROWMAP": "vec"}, "overlay-vec"),
    ("piece-table", {}, "piece-table"),
]
FILES = ["corpus/rows_100mb.csv", "corpus/rows_1024mb.csv"]


def sha(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        while chunk := f.read(1 << 24):
            h.update(chunk)
    return h.hexdigest()


def changed_lines(a, b):
    """Lines present on only one side of `diff` (inserted rows appear once, edited rows twice)."""
    out = subprocess.run(["diff", a, b], capture_output=True, text=True).stdout
    return sum(1 for l in out.splitlines() if l[:1] in "<>")


def main():
    results = []
    for file in FILES:
        digests = {}
        for binary, env, name in MODELS:
            out = subprocess.run(
                [str(HERE / "target/release" / binary), file],
                check=True, capture_output=True, text=True, env={**os.environ, **env},
            ).stdout
            r = json.loads(out)
            r["file"] = file
            digests[name] = sha(f"/tmp/dec3-{name}.csv")
            results.append(r)
            f = lambda k: f"{r[k]['p50']:.4f}/{r[k]['p99']:.4f}/{r[k]['max']:.4f}"
            print(
                f"{Path(file).name:16} {name:13} rows={r['rows']} edit={f('edit_ms')} ins500k={r['insert_500k_ms']:.4f} "
                f"insert={f('insert_ms')} window={f('fetch_100_rows_ms')} save={r['save_ms']}ms "
                f"mem={r['model_bytes'] / 2**20:.2f}MiB {r['shape']}",
                flush=True,
            )
        same = len(set(digests.values())) == 1
        print(f"{Path(file).name}: saved files identical across models: {same}", flush=True)
        if "100mb" in file:
            print(f"  diff vs source: {changed_lines(file, '/tmp/dec3-piece-table.csv')} changed lines", flush=True)
        for r in results:
            if r["file"] == file:
                r["outputs_identical"] = same
    for _, _, name in MODELS:
        Path(f"/tmp/dec3-{name}.csv").unlink(missing_ok=True)
    (HERE / "results.json").write_text(json.dumps(results, indent=1) + "\n")


if __name__ == "__main__":
    main()
