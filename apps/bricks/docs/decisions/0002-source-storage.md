# 0002: Source storage

- Story: DEC-2
- Status: accepted
- Date: 2026-10-05

## Question

How does `csv-engine` hold a large source file so that any row can be fetched quickly without materializing the file (invariant 2)?

## Options

All three options were spiked over `corpus/rows_1024mb.csv` (1,073,741,895 bytes, 19,094,580 rows including the header). They share one quote-aware row scanner (`memchr2` on `"` and `\n`) and one field splitter. Every fetch copies the row's raw field bytes out. Indexing is single-threaded in all three; the parallel indexer is SP-3/ENG-1 work.

| Option | Index time cold / warm | Index RAM | 100-row window warm p50 / p99 | 100-row window cold p50 / p99 / max | Cost / risk |
| --- | --- | --- | --- | --- | --- |
| A. mmap + sparse index (byte offset of every 64th row) | 0.87 s / 0.21 s | 4 MiB heap, 2.5 MiB resident | 0.026 / 0.039 ms | 2.5 / 3.8 / 8.3 ms | Truncating the file from another process while it is mapped kills the app with SIGBUS (reproduced, exit 135). File pages count toward RSS (1,026 MiB), but they are reclaimable page cache. |
| A'. mmap + dense index (u64 per row: the execution plan's lean) | 0.92–0.94 s / 0.22 s | 146 MiB resident (256 MiB `Vec` capacity) | 0.007 / 0.010 ms | 2.6–2.9 / 3.6–4.4 / 8.5 ms | Same SIGBUS risk. The index is 8 bytes per row, so a 2 GB file overshoots ENG-1's 200 MB budget. |
| B. Chunked arena (4 MiB row-aligned chunks, `pread` on demand, 64 MiB LRU) | 0.75–0.78 s / 0.24 s | 6 KiB of metadata plus the 64 MiB cache | 1.29 / 2.3 ms | 5.5 / 8.2 / 12.1 ms | Avoids SIGBUS. Every cache miss reads and rescans a full 4 MiB chunk, so a random single row costs 1.3 ms warm and a cold window misses the 5 ms target. |
| C. Columnar (full parse: per-column bytes plus u32 cell ends) | 3.0 s / 2.1 s | 1,472–1,478 MiB resident (2,304 MiB capacity) | 0.006 / 0.012 ms | same as warm (all in heap) | Materializes the whole file, which breaks invariant 2. RAM is about 1.4x the file size and grows with it. Indexing takes 3.4x longer than A cold and 10x longer warm. |

Single random row, warm p50 / p99: A 0.001 / 0.001 ms; A' 0.000 / 0.001 ms; B 1.26–1.28 / 2.3–2.4 ms; C 0.001 / 0.002 ms.

Targets this decision feeds:

- **ENG-1:** index 1 GB in under 10 s with under 200 MB index RAM. A, A' and B pass single-threaded. C passes on time, but only because RAM isn't counted against it.
- **ENG-2:** 100 visible rows in under 5 ms from anywhere. A, A' and C pass warm by more than 100x. B fails the cold case.
- **BENCHMARKS:** peak RAM under 1.5x file size. C sits at 1.4x before any feature adds memory on top.

## Spike

`spikes/dec-2/` holds `common.rs` (scanner, splitter, harness), `mmap-sparse/` (`DEC2_STRIDE`, default 64; 1 gives A'), `chunked-arena/` (`DEC2_BUDGET_MB`, default 64), `columnar/`, `run.py`, and `results.json`.

- **Hardware:** Ryzen 9 5900, 125 GB RAM, NVMe, Arch Linux, Rust 1.99. This is not the reference box (16 GB, 8 cores).
- **Cold:** each cold measurement first evicts the file with `posix_fadvise(DONTNEED)`, after unmapping or dropping the app's caches so the pages can actually leave. Cold index time is therefore a real disk read; warm is the scanner alone (about 5 GB/s on one core).
- **Fetches:** 10,000 random single rows and 1,000 random 100-row windows, warm. Then 100 random windows, each one cold.
- **Correctness:** every fetched row is checked: field 0 is the row id, and there are 8 fields. All options report the same row count, which matches a line count of the file.
- **Repetitions:** 2 reps per option; ranges above show both.
- **mmap edge cases:**
  - Truncating a mapped file in place and then reading the mapping raised SIGBUS and killed the process (`exit 135`).
  - Replacing the file with write-temp-then-`rename` (how SAVE-1 saves) left the old mapping intact and readable.

To rerun:

```sh
python3 scripts/gen_corpus.py
(cd spikes/dec-2 && cargo build --release)
python3 spikes/dec-2/run.py
```

## Decision

**A, mmap + sparse row index.** The index stores the byte offset of every 64th row start, and cells are parsed on demand from the mapping. Accepted 2026-10-06.

1. **It meets every target with the most headroom in RAM.** Index time is under 1 s cold for 1 GB. The index takes about 4 MiB for 19M rows, against ENG-1's 200 MB budget. A visible window costs 26 µs warm and about 2.5 ms p50 cold, against ENG-2's 5 ms. The dense variant buys about 20 µs per window for 146 MiB of RAM per GB, which isn't worth it. The chunked arena pays 1–5 ms per miss. Columnar breaks invariant 2.
2. **It keeps the source authoritative and untouched.** Bytes are read straight from the file and never copied or rewritten. The edit overlay and row map (DEC-3) refer to source rows by number, and the sparse index resolves those numbers.

The stride is a constant to tune with BENCH-1. A stride of 16 would still take only about 9 MiB per GB.

## Consequences

- **Easy:**
  - First rows can show before indexing finishes (APP-3), because row 0 starts at byte 0 of the mapping.
  - Index RAM barely grows with file size.
  - The parallel indexer (SP-3) only has to produce checkpoint offsets.
- **Hard: SIGBUS.** Another program truncating or rewriting the file in place while it is open will crash the app, and the edits in the overlay go with it.
  - ENG-1 must contain this, for example by treating the mapping as untrusted: install a SIGBUS handler around fetches that turns the fault into a "file changed on disk" error. Watching the file (inotify or mtime/size checks) can warn earlier.
  - Our own saves replace the file by rename, which keeps the old mapping valid. SAVE-1 must keep it that way and never rewrite in place.
- **Hard: memory reporting.** Page-cache pages show up in RSS (about 1x the file). The BENCHMARKS "peak RAM" row should report anonymous memory separately, or the 1.5x target will read as nearly used up.
- **Later work:**
  - Sort and filter (DEC-4) should extract the needed column into a compact cache with one sequential pass, rather than random-access rows through the sparse index.
  - Quote-heavy files make the scanner hit more bytes. The corpus has no quoted fields, so ENG-1 should measure a quoted variant.
- **Revisit if:** SIGBUS can't be contained cleanly, or network/FUSE filesystems turn out to be common for target users (mmap over those is slow and fragile). The chunked arena with a larger chunk index is the fallback.
