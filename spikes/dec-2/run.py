#!/usr/bin/env python3
"""DEC-2 measurement matrix. Throwaway. Run from the repo root after
`cargo build --release` in spikes/dec-2 and `python3 scripts/gen_corpus.py`."""
import json
import os
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent
FILE = "corpus/rows_1024mb.csv"
CONFIGS = [
    ("mmap-sparse", {"DEC2_STRIDE": "1"}),
    ("mmap-sparse", {"DEC2_STRIDE": "64"}),
    ("chunked-arena", {"DEC2_BUDGET_MB": "64"}),
    ("columnar", {}),
]
REPS = 2


def main():
    results = []
    for binary, env in CONFIGS:
        for rep in range(REPS):
            out = subprocess.run(
                [str(HERE / "target/release" / binary), FILE],
                check=True, capture_output=True, text=True, env={**os.environ, **env},
            ).stdout
            r = json.loads(out)
            r["rep"] = rep
            results.append(r)
            f = lambda k: f"{r[k]['p50']:.3f}/{r[k]['p99']:.3f}/{r[k]['max']:.3f}"
            print(
                f"{r['store']:13} {r['params']:26} rows={r['rows']} cold={r['index_cold_ms']}ms warm={r['index_warm_ms']}ms "
                f"index={r['index_bytes'] / 2**20:.1f}MiB anon={r['rss_anon_mib']}MiB file={r['rss_file_mib']}MiB "
                f"row={f('fetch_row_ms')} 100rows={f('fetch_100_rows_ms')} 100rows_cold={f('fetch_100_rows_cold_ms')}",
                flush=True,
            )
    (HERE / "results.json").write_text(json.dumps(results, indent=1) + "\n")


if __name__ == "__main__":
    main()
