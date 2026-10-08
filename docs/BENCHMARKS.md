# Benchmarks

Reference box: 16 GB RAM, 8-core x86_64, NVMe SSD, Arch Linux on Wayland. Numbers from other machines are reported but do not pass or fail a gate.

Corpus: `python3 scripts/gen_corpus.py` (the 1 GB file is 19.1M rows).

Run `cargo bench` to measure every row and print this table with Latest filled in (`app/benches/corpus.rs`). Rows that open a window need a graphical session; CI prints them as skipped. Latest below: `cargo bench` on 2026-10-06, Ryzen 9 5900 (24 threads), 126 GiB RAM, NVMe btrfs, Hyprland. This is not the reference box.

| Benchmark (1 GB unless noted) | Target | Latest | LibreOffice |
| --- | --- | --- | --- |
| Cold start to empty window | < 300 ms | 245 ms (median of 3) | |
| Open to first rows visible | < 500 ms | 259 ms, cold cache (median of 3) | |
| Full index complete | < 10 s | 0.62 s cold, single thread | |
| Scroll frame time p99 | < 16 ms | 1.8 ms frame CPU p99; 60.0 fps, 0 of 600 frames late; 1906×1048 px grid | |
| Peak RAM after open | < 1.5x file size | 1.13x (1158 MiB peak RSS incl. mapped file pages; 28 MiB anonymous) | |
| Find first match | < 1 s | not built yet (SRCH-1) | |
| Sort numeric column | < 10 s (< 3 s at 2M rows) | not built yet (SORT-1) | |
| Two-column filter | < 3 s | not built yet (FILT-2) | |
| Save with 1,000 edits | < 15 s | 0.99 s incl. fsync and verify (`save_perf`, NVMe); 0.84 s in the app with the UI scrolling at 60 fps, main loop p99 1.7 ms late (`save_responsive`, tmpfs; SAVE-3, 2026-10-08, same Ryzen box) | |
