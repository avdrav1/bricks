# 0003: Edit model

- Story: DEC-3
- Status: accepted
- Date: 2026-10-06

## Question

How are cell edits and row inserts recorded over the read-only source (ADR 0002) so that each one is O(1) or O(log n), the source file is never touched, and save writes untouched rows byte-for-byte?

## Options

Both options sit on DEC-2's source (mmap + every-64th-row index) and use the same order-statistic treap (split/merge by row count). They differ only in what the treap holds. A plain `Vec<RowId>` row map was run as the O(n) baseline.

Op sequence for every model (same seed): 5,000 random cell edits, an insert at row 500,000, 5,000 more edits, 1,000 single-row inserts at random rows, 1,000 random 100-row reads, then a full save.

| Option | Cell edit p50 / p99 | Insert at row 500k | Random insert p50 / p99 | 100-row read p50 | Model RAM after 12k ops | Cost / risk |
| --- | --- | --- | --- | --- | --- | --- |
| **A. Overlay + row map.** Edits in `HashMap<(RowId, Col), raw value>`; row order is a treap of runs of stable RowIds | 0.1 / 0.5 µs | 1.8 µs | 0.3 / 2.2 µs | 32 µs | 0.73 MiB (2,003 runs, 11,008 edits) | Reading an edited row looks up each cell in the overlay. RowIds outlive any reordering, so later layers (sort, filter, undo, column map) can refer to rows and cells stably. |
| B. Row-aligned piece table. Treap of byte ranges over the file plus an append-only add buffer; an edit re-encodes its row into the add buffer | 2.3 / 5.2 µs | 1.4 µs | 1.6 / 4.4 µs | 33 µs | 3.0 MiB (21,988 pieces, 565 KB add buffer) | The document is bytes, so edits are CSV-encoded when they're made. Column insert/delete and sort have no natural representation: each needs a per-row rewrite or a second map on top (see Consequences). |
| Baseline: overlay + `Vec<RowId>` row map | 0.2 / 0.8 µs | 11.1 ms | 5.5 / 11.7 ms | 29 µs | 292 MiB | An insert shifts every later id: O(n). The cost grows 48x from 1.9M to 19.1M rows. |

Numbers are from the 1 GB corpus (19,095,581 rows after the inserts). The scaling evidence for "O(1) or O(log n) per edit":

| Per-op p50 | 1.9M rows (100 MB) | 19.1M rows (1 GB) | 10x rows → |
| --- | --- | --- | --- |
| A edit / insert | 0.1 / 0.3 µs | 0.1 / 0.3 µs | flat |
| B edit / insert | 2.1 / 1.4 µs | 2.3 / 1.6 µs | flat |
| Vec baseline insert | 114 µs | 5,525 µs | 48x |

## Spike

`spikes/dec-3/` holds `common.rs` (source, treap, harness), `overlay-rowmap/` (`DEC3_ROWMAP=treap|vec`), `piece-table/`, `run.py`, and `results.json`.

- **Correctness:**
  - Every edit is read back and compared. Each insert is read back, and the row that was at 500,000 must reappear intact at 500,001.
  - At both sizes, the three models' saved files are byte-identical (SHA-256).
  - At 100 MB, `diff` against the source shows 20,955 changed lines. That is consistent with at most 9,996 edited source rows shown twice (old and new) plus 1,001 inserted rows shown once. Every untouched row was written byte-for-byte.
- **Save:** A copies untouched source rows as byte ranges between edited rows. B writes its pieces in order. 1 GB save to tmpfs: A 463 ms, B 427 ms, baseline (row by row) 4.6 s. Disk speed and fsync are SAVE-1's concern.
- **Hardware:** Ryzen 9 5900, 125 GB RAM, NVMe, Rust 1.99, single-threaded. This is not the reference box.
- **Memory caveat:** the spike's treap never frees replaced nodes, so RAM figures include a little garbage. A real implementation would recycle them.

To rerun:

```sh
python3 scripts/gen_corpus.py
(cd spikes/dec-3 && cargo build --release)
python3 spikes/dec-3/run.py
```

## Decision

**A, overlay + row map.** Accepted 2026-10-06.

- **Cell edits:** a cell overlay keyed by a stable `RowId`. A `RowId` is either a source row number or an inserted-row id.
- **Row order:** a logical-to-`RowId` row map, stored as an order-statistic tree of id runs.

1. **Both options meet the bar; A fits the later stories.** Per-op cost is flat from 1.9M to 19.1M rows for both. A is about 20x cheaper per cell edit (0.1 vs 2.3 µs) and about 5x cheaper per insert.
   - The deciding factor is what comes next. Sort (SORT-1) is a permutation of `RowId`s, filter (FILT-*) is a subset of them, column insert/delete (EDIT-4) is a column map over the overlay, and undo (CMD-2) stores the overlay's previous value.
   - In a piece table, each of those is either a rewrite of every row or a second index on top of the bytes.
2. **Raw values stay raw.** The overlay stores exactly what the user typed. Encoding happens only at save, and untouched rows are copied byte-for-byte from the file, which is invariant 1 and SAVE-4.

## Consequences

- **The `data-model` scaffold needs to change:** `EditOverlay` keys on physical `CellRef { row }` today. It must key on `RowId`, because physical row numbers shift on insert. EDIT-1 makes that change.
- **Row map representation:** while the order is "source order plus local inserts and deletes", the row map stays a tree of runs (2,003 runs after 1,001 inserts, under 1 MiB).
  - A full sort makes every row its own run. From the struct layout, that would be about 40 bytes per node, around 760 MB for 19M rows.
  - DEC-4 must pick a flat permutation (`Vec<u32>` of `RowId`, about 76 MB) for sorted views. Stable `RowId`s make that switch local to the row map.
- **Column operations:** EDIT-4 adds a logical-to-physical column map in front of the overlay. No row bytes are rewritten until save.
- **Revisit if:** the overlay grows large enough for its per-cell lookups to dominate reads or memory. For example, replace-all over millions of cells (SRCH-3) might be better stored as rewritten rows. A row-level overlay that holds a whole re-encoded row (B's idea) can be added inside A for that case.
- **As built in SRCH-3 (2026-10-09):** replace all stays inside A, as cell edits in one `Edit::Cells`, and is capped at 2,097,152 cells (the paste limit, user decision). 210k cells on 1 GB held 12.3 MB of undo and applied in 61 ms. A row-level or rule-based store for bigger replaces is still the "revisit if" above.
- **As built in FILT-4 (2026-10-10):** deleting rows shown under a filter is one `Edit::DeleteRowRanges` over every position range, applied in one step (`RowOrder::remove_ranges`). Fixed then: the row-map treap's `split` gave the new half of a cut run a random priority that could outrank its new ancestors. Many cuts grew the tree into a chain (depth 7,230 at 80k runs); the half now takes a priority at or below the cut node's.
