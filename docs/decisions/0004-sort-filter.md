# 0004: Sort and filter representation

- Story: DEC-4
- Status: accepted
- Date: 2026-10-06

## Question

How do sort and filter compute and represent a view over the source (ADR 0002) and the overlay and row map (ADR 0003), so that a 2M-row numeric sort finishes in under 3 s, a two-column filter on 1 GB in under 3 s, and filtered rows are never lost on save?

## Options

Both options read cells the same way: through DEC-2's sparse index, with an overlay of 10,000 edited cells applied. Both run on Rayon (ADR 0005) and produce the same output: a flat `Vec<u32>` of RowIds, which ADR 0003 requires for sorted views. The column is `score` (decimal); the filter is `city == "Portland" AND revenue > 50000`. Input is the first 2,000,000 data rows of `corpus/rows_1024mb.csv`, and all 19,094,579 rows for the full-file runs.

| Option | 2M-row sort, 23 / 7 / 1 threads | 19.1M-row sort, 23 / 7 / 1 threads | Two-column filter 2M / 19.1M (23 threads) | Cost / risk |
| --- | --- | --- | --- | --- |
| A. Comparator fetch: each comparison re-reads both rows and parses the cell | 2.1 s / **4.5 s** / **25.3 s** | not run (41.4M comparisons at 2M rows; it grows as n log n) | 39 ms / not run | No extra memory. Misses the 3 s target on 8 cores and on one thread. |
| **B. Key column, then sort.** One parallel pass extracts an order-preserving `u64` key per row; `par_sort_unstable` on `(key, RowId)`; RowIds form the permutation | **36 / 56 / 202 ms** | **380 / 478 / 2,274 ms** | bitmap 10.6 / 82 ms; 1 thread: 80 / 779 ms | Transient memory: 8 B/row for keys plus 16 B/row for the pairs, about 450 MiB peak at 19.1M rows. The result is 4 B/row. |

Breakdown of option B at 19.1M rows on 7 threads (8 physical cores): the key pass took 227 ms, the sort 252 ms. Alternative sort strategies on the same keys:

| Sort strategy (19.1M rows) | 23 threads | 7 threads | 1 thread |
| --- | --- | --- | --- |
| Unstable sort on `(key, RowId)` (stable in effect, because RowId breaks ties) | 239 ms | 252 ms | 767 ms |
| Stable merge sort on key | 385 ms | 410 ms | 1,239 ms |
| Argsort (indices compared through the key column) | 519 ms | 1,277 ms | 9,931 ms |

Filter representation (19.1M rows, 1,362,719 visible):

| Representation | Build (23 threads) | Memory | 100 visible rows, p99 | Notes |
| --- | --- | --- | --- | --- |
| Bitmap over RowIds plus a rank index every 512 bits | 82 ms | 2.4 MiB (1 bit/row) | 0.047 ms (via select) | Doesn't depend on row order. Combining filters is a word-wise AND. The visible count is a popcount. |
| Visible list `Vec<u32>` | 79 ms | 5.2 MiB (4 B per visible row; up to 76 MiB if every row passes) | 0.045 ms | Direct indexing, but it must be rebuilt whenever the order changes. |
| Sorted and filtered view = row map ∩ bitmap | 10 ms | 4 B per visible row | n/a | One pass over the permutation that tests each bit. |

## Spike

`spikes/dec-4/` holds `common.rs` (source, cell reads with the overlay, numeric keys, checks), `key-column/`, `fetch-compare/`, `run.py`, and `results.json`.

- **Correctness:**
  - Every permutation is checked to be ascending by key, with equal keys in RowId order (stable), and to contain each RowId exactly once.
  - Both options produce the same 2M-row permutation (same digest) in every CPU configuration.
  - Bitmap, visible list and fetch-filter agree on the visible count (142,776 at 2M rows, 1,362,719 at 19.1M), and `select(k)` matches `list[k]` on every sampled window.
- **Typed keys:** numbers map to an order-preserving `u64`. Non-numeric and empty cells sort after all numbers, and raw bytes are never altered (invariant 1). Text and date keys are TYPE-1/SORT-1 work, with the same shape.
- **Hardware:** Ryzen 9 5900, 125 GB RAM, NVMe, Rust 1.99, file in the page cache. "7 threads" means `taskset -c 0-7` (8 physical cores, pool 7 per ADR 0005), which approximates the reference box. This is not the reference box.

To rerun:

```sh
python3 scripts/gen_corpus.py
(cd spikes/dec-4 && cargo build --release)
python3 spikes/dec-4/run.py
```

## Decision

**B, key column then sort, with filters as RowId bitmaps.** Accepted 2026-10-06.

1. **Sort:**
   - One parallel pass reads the column through source and overlay into a typed, order-preserving key per row.
   - `par_sort_unstable` sorts `(key, RowId)` pairs. The RowId tiebreak makes the result stable.
   - The output permutation becomes the view's row map (`Vec<u32>`, ADR 0003).
   - Measured: 56 ms for 2M rows on 8 cores (target 3 s), 478 ms for 19.1M (target 10 s), and 2.3 s for 19.1M on a single thread. Re-reading cells on every comparison makes comparisons O(n log n) row fetches, and that already misses 3 s at 2M rows on 8 cores.
2. **Filter:**
   - Each predicate evaluates in one parallel pass into a bitmap keyed by RowId. A bitmap is 1 bit per row, survives re-sorting, and combines across columns with AND (FILT-2), with popcount giving "x of y rows".
   - The grid reads a derived visible list (the row map filtered by the bitmap), which takes 10 ms to rebuild at 19.1M rows.

## Consequences

- **Saving keeps every row (invariant 7, FILT-3):** save walks the row map, never the visible list, so a filter cannot drop rows from disk. The row map carries the sort order, so a sorted file saves in sorted order (SORT-1).
- **Undo:** sort undo stores the previous row map (4 B/row, 76 MiB at 19.1M rows; CMD-2's 50 MB history budget for 1,000 steps needs a cap or compression for this). Filter undo stores nothing, because a filter is a view.
- **Memory:** sorting needs about 24 B/row of transient memory (about 450 MiB at 19.1M rows).
  - Key columns can be cached per column for re-sorts and invalidated when that column is edited. That costs 8 B/row per cached column, so caching needs an LRU and a byte budget.
  - Within the "1.5x file size" target, 1 GB of source leaves room for this, but not for several cached columns at once.
- **Off the UI thread:** the passes must check the cancel token (ENG-8) once per block (65,536 rows, about 1 ms of work).
- **Revisit if:** sorts over text columns, which need collation and longer keys, can't stay within budget with fixed-width key prefixes and a fallback comparison.
- **As built in SORT-1 (2026-10-09):**
  - **Order:** the row order is `RowOrder::Runs` (the treap of runs) or `RowOrder::Sorted` (`Arc<Vec<u32>>`: 4 B/row, inserted rows marked by bit 31). Inserts and deletes work on both; a sorted order copies its vector first if a snapshot or undo shares it. A sort back to file order becomes `None` again.
  - **Keys:** 24 B per row: `k1` (an order-preserving number or instant, or the ASCII case-folded first 8 bytes), `k2` (the exact first 8 bytes), the current position, a class (typed, misfit, blank), and a long-text bit. Text rows longer than 8 bytes with equal `k1` are re-sorted by whole value afterwards, a group at a time. A column of long values sharing a prefix (URLs) re-reads every value in that group.
  - **Reading:** the key pass reads the file in file order, a chunk of 65,536 rows per task, and keys each row with its current position. A sort of an already-sorted table therefore costs the same as the first one (0.88 s against 0.90 s at 19.1M rows).
  - **Stability:** the position, not the RowId, breaks ties, so a sort keeps the current order among equal values: sort by B, then by A, gives A then B.
  - **Saving a sorted order:** this would read rows at random, about 32 rows of scanning each from the 64-row checkpoints. So the save first finds every row's start in one parallel pass (8 B/row, transient) and copies rows from there: 3.7 s for 1 GB.
  - **Cancel:** the sort phase can't pause, so once cancelled its comparator unwinds with a private payload (`resume_unwind`, no panic message), which rayon's sort survives without losing elements. Cancels land in 2–38 ms in every phase (`jobs/tests/cancel_1gb.rs`).
  - **Undo:** a sort is one `Edit::Reorder` whose inverse is the previous order. From file order or a run order, that is a few bytes; only a sort of an already-sorted table holds 4 B/row (76 MiB at 19.1M), and that clears older history past CMD-2's 50 MB budget. The step just taken always stays.
  - **Peak memory at 19.1M rows:** keys 460 MB, plus the old order, each row's position, and the new order at 76 MB each, all transient.
