#!/usr/bin/env python3
"""Generate the test corpus. Usage: gen_corpus.py [--max-mb 1024] [--out corpus/]

Output is byte-identical across runs and locales (fixed seed, explicit encodings)."""
import argparse
import random
from pathlib import Path

DEFAULT_OUT = Path(__file__).resolve().parent.parent / "corpus"
CITIES = ["Boston", "Philly", "Chicago", "Austin", "Denver", "Portland", "Miami"]
NAMES = ["Alice", "David", "Maya", "Omar", "Lena", "Yuki", "Priya", "Sam"]


def row(i, rng):
    return ",".join([
        str(i), rng.choice(NAMES), rng.choice(CITIES),
        str(rng.randint(0, 99999)), f"{rng.random() * 1000:.2f}",
        f"2026-{rng.randint(1, 12):02d}-{rng.randint(1, 28):02d}",
        rng.choice(["true", "false"]), f"{rng.randint(0, 999):05d}",
    ])


def sized(path, mb, rng):
    target = mb * 1024 * 1024
    with path.open("w", encoding="ascii", newline="") as f:
        f.write("id,name,city,revenue,score,date,active,code\n")
        i, written = 0, 0
        while written < target:
            line = row(i, rng) + "\n"
            f.write(line)
            written += len(line)
            i += 1
    print(f"{path.name}: {i:,} rows")


def nasty(out):
    files = {
        "quoted_newlines.csv": 'id,note\n1,"line one\nline two"\n2,"has ""quotes"""\n3,plain\n',
        "crlf.csv": "a,b\r\n1,2\r\n3,4\r\n",
        "bom.csv": "\ufeffa,b\n1,2\n",
        "ragged.csv": "a,b,c\n1,2\n3,4,5,6\n7,8,9\n",
        "leading_zeros.csv": "zip,code\n02134,00123\n07030,000\n",
        "semicolon.csv": "a;b;c\n1,5;2;3\n",
        "tab.tsv": "a\tb\n1\t2\n",
        "pipe.csv": "a|b\n1|2\n",
        "empty.csv": "",
        "header_only.csv": "a,b,c\n",
        "no_trailing_newline.csv": "a,b\n1,2",
    }
    d = out / "nasty"
    d.mkdir(parents=True, exist_ok=True)
    for name, body in files.items():
        (d / name).write_text(body, encoding="utf-8", newline="")
    (d / "latin1.csv").write_bytes("name\nJos\u00e9\nM\u00fcller\n".encode("latin-1"))
    (d / "utf16le.csv").write_bytes(b"\xff\xfe" + "a,b\n1,2\n".encode("utf-16-le"))
    print(f"nasty/: {len(files) + 2} files")


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--max-mb", type=int, default=1024)
    ap.add_argument("--out", type=Path, default=DEFAULT_OUT)
    args = ap.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    rng = random.Random(42)
    for mb in (10, 100, 1024, 4096):
        if mb <= args.max_mb:
            sized(args.out / f"rows_{mb}mb.csv", mb, rng)
    nasty(args.out)
