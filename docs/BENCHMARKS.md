# Benchmarks

Reference box: 16 GB RAM, 8-core x86_64, NVMe SSD, Arch Linux on Wayland. Numbers from other machines are reported but do not pass or fail a gate.

Corpus: `python3 scripts/gen_corpus.py` (the 1 GB file is 19.1M rows).

Run `cargo bench` to measure every row and print this table with Latest filled in (`app/benches/corpus.rs`). Rows that open a window need a graphical session; CI prints them as skipped. Latest below: `cargo bench` on 2026-10-10, Ryzen 9 5900 (24 threads), 126 GiB RAM, NVMe btrfs, Hyprland. This is not the reference box. The XLSX and formula rows (BENCH-2, 2026-10-10) were measured only under the 8-core approximation; they need `corpus/formulas.csv` and `corpus/rows_1m.xlsx` (`cargo run --release -p xlsx --example gen_corpus_xlsx`). "8 cores" is the same run under `systemd-run --user --scope -p MemoryMax=16G taskset -c 0-7`, an approximation of the reference box.

| Benchmark (1 GB unless noted) | Target | Latest | LibreOffice |
| --- | --- | --- | --- |
| Cold start to empty window | < 300 ms | 267 ms (median of 3); 8 cores: 292 ms | |
| Open to first rows visible | < 500 ms | 314 ms, cold cache (median of 3); 8 cores: 310 ms | |
| Full index complete | < 10 s | 0.39 s cold, single thread; 8 cores: 0.41 s | |
| Scroll frame time p99 | < 16 ms | 2.8 ms frame CPU p99; 60.0 fps, 0 of 600 frames late; 1887×960 px grid. 8 cores: 3.0 ms p99. Two of five scroll runs that day each had one multi-second gap between frames (fps 6.9 and 17.8) while frame CPU stayed under 6 ms; the other three ran at 59.7–60.0 fps. Likely the compositor pausing a hidden window; watch it on the reference box | |
| Peak RAM after open | < 1.5x file size | 1.14x (1164 MiB peak RSS incl. mapped file pages; 31 MiB anonymous) | |
| Find first match | < 1 s | 224 ms slowest of an id near the end (187 ms), no match (182 ms), and `portland` (2,725,768 cells, 224 ms); 8 cores: 178 ms. On a sorted table: 335–512 ms (`search_1gb`). Cancel within 10 ms, also on a keystroke (`find_typing`) | |
| Sort numeric column | < 10 s (< 3 s at 2M rows) | 0.82 s for 19.1M rows, 128 ms at 2M; 8 cores: 1.02 s and 149 ms. Saving the sorted file: 3.7 s (SORT-1) | |
| Two-column filter | < 3 s | 0.34 s: city = Portland, then revenue > 50,000 (1,362,719 rows); 8 cores: 0.55 s | |
| Save with 1,000 edits | < 15 s | 0.92 s incl. fsync and verify; 8 cores: 0.93 s. In the app with the UI scrolling at 60 fps: 0.77 s, main loop p99 1.1 ms late (`save_responsive`, ENG-8) | |
| Open 1,048,576-row XLSX, all sheets ready | < 10 s | 8 cores: 3.69 s from spawn to the imported workbook drawn (median of 3, warm cache), `corpus/rows_1m.xlsx` (1,048,576 rows, 1 sheet); 196 MiB peak RSS | |
| Save 1,048,576-row workbook as XLSX | < 30 s | 8 cores: 6.68 s incl. fsync, one cell edited and the package patched (STYLE-5; BENCH-2 measured a fresh write: 8.19 s) | |
| Scroll frame time p99, 60 visible formulas | < 16 ms | 8 cores: 2.1 ms frame CPU p99; 59.5 fps, 1 of 600 frames late; 1887×884 px grid on `corpus/formulas.csv` (1M rows, 4 formula columns incl. a running total) | |
| Recalculate SUM over a 1 GB column | < 5 s | 8 cores: 3.77 s, `=SUM(rows!D:D)` over 19.1M rows (median of 3) | |
| Running total, 1,000,000 rows | < 5 s | 8 cores: 3.75 s, `formulas.csv` E1000001 on a fresh calculation: every `=E{n-1}+D{n}` and `=B*C` above it (median of 3). 10.98 s before the span cache (BENCH-2) | |
| 10,000 exact VLOOKUPs into 1,000,000 rows | < 2 s | 8 cores: 0.24 s incl. building the lookup index (median of 3) | |
| Open 1,048,576-row styled XLSX | < 10 s | 8 cores: 3.92 s from spawn to the imported workbook drawn (median of 3, warm cache), `corpus/styled_1m.xlsx` (`gen_corpus_xlsx --styled`); 300 MiB peak RSS | |
| Save 1,048,576-row styled XLSX after one edit | < 30 s | 8 cores: 7.36 s incl. fsync: the package patched, the sheet written again with unchanged cells copied (47 MiB) | |
| Scroll frame time p99, styled sheet | < 16 ms | 8 cores: 10.0 ms frame CPU p99; 59.9 fps, 1 of 600 frames late; 1887×846 px grid. The 1 GB CSV scroll measured 8.2 ms p99 in the same run, and the STYLE-2 binary (before styles were drawn) 8.8 ms on the same machine state, so the gap to BENCH-2's 2.0 ms is the machine, not the styles | |
