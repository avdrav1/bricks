# Benchmarks

Reference box: 16 GB RAM, 8-core x86_64, NVMe SSD, Arch Linux on Wayland. Numbers from other machines are reported but do not pass or fail a gate.

Corpus: `python3 scripts/gen_corpus.py` (1 GB file is about 10M rows).

| Benchmark (1 GB unless noted) | Target | Latest | LibreOffice |
| --- | --- | --- | --- |
| Cold start to empty window | < 300 ms | | |
| Open to first rows visible | < 500 ms | | |
| Full index complete | < 10 s | 0.70 s cold, single thread (ENG-1; Ryzen 5900, not the reference box) | |
| Scroll frame time p99 | < 16 ms | 4.9 ms frame CPU p99; 60.0 fps with 3 of 600 frames late, 19.1M rows (GRID-1; Ryzen 5900, not the reference box) | |
| Peak RAM after open | < 1.5x file size | | |
| Find first match | < 1 s | | |
| Sort numeric column | < 10 s (< 3 s at 2M rows) | | |
| Two-column filter | < 3 s | | |
| Save with 1,000 edits | < 15 s | | |
