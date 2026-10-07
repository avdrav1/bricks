#!/usr/bin/env python3
"""Download the ENG-4 real-world delimiter sample into corpus/realworld/ (gitignored).

Usage: fetch_delimiter_sample.py [--out corpus/realworld]

Reads scripts/delimiter_sample.tsv (name, true delimiter, source URL) and saves the first
256 KiB of each file. The data stays with its publishers; only the manifest is committed.
Some sources are live feeds, so contents drift, but their format does not.

Each label was set by reading the file, not by the detector under test. As a sanity check,
this script confirms that the labelled delimiter splits most records (skipping `#`
comment lines) into one consistent number of fields, and reports any file where it does not.
"""
import argparse
import csv
import io
import sys
import urllib.request
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "scripts" / "delimiter_sample.tsv"
DELIMS = {"comma": ",", "semicolon": ";", "tab": "\t", "pipe": "|"}
UA = "bricks-spreadsheet-test-corpus/0.1 (delimiter detection sample; github.com/avdrav1/bricks)"
SIZE = 256 * 1024


def manifest():
    with MANIFEST.open(newline="") as f:
        return list(csv.DictReader(f, delimiter="\t"))


def fetch(url):
    req = urllib.request.Request(url, headers={"User-Agent": UA, "Range": f"bytes=0-{SIZE - 1}"})
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.read(SIZE)


def label_consistency(data, delim):
    """Share of records with the most common field count, and that count."""
    text = data.decode("utf-8", errors="replace")
    lines = [l for l in text.splitlines(keepends=True)[:-1] if not l.startswith("#") and l.strip()]
    counts = Counter(len(r) for r in csv.reader(io.StringIO("".join(lines)), delimiter=delim))
    if not counts:
        return 0.0, 0
    width, n = counts.most_common(1)[0]
    return n / sum(counts.values()), width


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", type=Path, default=ROOT / "corpus" / "realworld")
    args = ap.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    failed = 0
    for row in manifest():
        try:
            data = fetch(row["url"])
        except Exception as e:  # noqa: BLE001 - report and carry on
            print(f"FAIL  {row['name']}: {e}")
            failed += 1
            continue
        (args.out / row["name"]).write_bytes(data)
        share, width = label_consistency(data, DELIMS[row["delimiter"]])
        flag = "" if share >= 0.9 and width >= 2 else "  <-- label check: inconsistent"
        print(f"ok    {row['name']}: {len(data)} bytes, {row['delimiter']}: {width} fields in {share:.0%}{flag}")
    print(f"{len(manifest()) - failed} of {len(manifest())} files in {args.out}")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
