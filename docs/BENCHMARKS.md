# Benchmarks

Reference box: 16 GB RAM, 8-core x86_64, NVMe SSD, Arch Linux on Wayland. Numbers from other machines are reported but do not pass or fail a gate.

Corpus: `python3 scripts/gen_corpus.py` (1 GB file is about 10M rows).

| Benchmark (1 GB unless noted) | Target | Latest | LibreOffice |
| --- | --- | --- | --- |
| Cold start to empty window | < 300 ms | | |
| Open to first rows visible | < 500 ms | | |
| Full index complete | < 10 s | | |
| Scroll frame time p99 | < 16 ms | | |
| Peak RAM after open | < 1.5x file size | | |
| Find first match | < 1 s | | |
| Sort numeric column | < 10 s (< 3 s at 2M rows) | | |
| Two-column filter | < 3 s | | |
| Save with 1,000 edits | < 15 s | | |
