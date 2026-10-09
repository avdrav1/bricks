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
| Find first match | < 1 s | 0.17–0.21 s for any query (the whole file is counted each time): an id near the end 183 ms, no match 170 ms, `portland` (2,725,768 cells) 204 ms, match case 83 ms (`search_1gb`, 23-thread pool). Sorted by score: 335–512 ms (the walk and the count read rows out of file order). In the app: 241 ms to "1 of 2,725,768"; next/previous through the counted matches 0.05–1.3 ms. Cancel within 10 ms, also on a keystroke (`find_typing`) (SRCH-1/SRCH-2, 2026-10-09, same Ryzen box) | |
| Sort numeric column | < 10 s (< 3 s at 2M rows) | 0.90 s for 19.1M rows, 144 ms at 2M (`sort_1gb`, 23-thread pool); 1.13 s and 170 ms on 8 cores (`taskset -c 0-7`). Saving the sorted file: 3.7 s (SORT-1, 2026-10-09, same Ryzen box) | |
| Two-column filter | < 3 s | 0.37 s: city = Portland (2,725,768 rows) then revenue > 50,000 (1,362,719 rows), 0.17 s + 0.20 s (`filter_1gb`, 23-thread pool); 0.78 s on 8 cores (FILT-1, 2026-10-09, same Ryzen box) | |
| Save with 1,000 edits | < 15 s | 0.90–0.95 s incl. fsync and verify (`save_perf`, NVMe, 3 runs); 0.77 s in the app with the UI scrolling at 60 fps, main loop p99 1.1 ms late (`save_responsive`, tmpfs; ENG-8, 2026-10-09, same Ryzen box) | |
