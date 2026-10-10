# Rust Spreadsheet: Backlog Execution Plan

Oct 5, 2026 · @Avniel Dravid

## How this plan works

V0.1 ships at the end of M3; the curl-installer release (per user, on Arch, Ubuntu 24.04, and Fedora) follows at the end of M4. Each milestone closes with a gate tied to the benchmark suite, and nothing moves forward until the gate passes.

**Sequencing rules**

- Retire the two largest risks first: the storage model at 1 GB and grid smoothness. Every later story depends on both.
- Land the benchmark harness in M0 and run it on every merge, so a regression shows up the day it happens.
- Put the data layer behind a trait. The grid asks for cells by logical row and column and never touches the file.
- Design undo with the first mutation in M2. Retrofitting it later costs more than building it.
- Untouched rows save byte-identical to the source. This one rule protects `00123`, odd quoting and every value type inference might mangle.

**Conventions**

- Story IDs: `M2-04` (milestone, sequence). Spikes: `SP-1`.
- Sizes: **S** = a day or less, **M** = two to four days, **L** = one to two weeks. Split any L before starting it.
- Crate tags match the PRD workspace: `csv-engine`, `data-model`, `grid`, `commands`, `app`, plus `bench` for the harness.
- Every story carries acceptance criteria. A story is done when its criteria pass in CI on the test corpus.

The roadmap below shows each milestone and the gate that closes it.

## Decision spikes

Six decisions shape everything downstream; SP-1 and SP-2 must close inside M0. Each spike is time-boxed and ends in a short written decision record in the repo.

| ID | Decision | Options | Current lean | Exit criteria | Size |
| --- | --- | --- | --- | --- | --- |
| SP-1 | GUI toolkit | egui/eframe, iced, Slint, GTK4 (gtk-rs), custom wgpu grid | Build the same grid spike in two finalists, likely GTK4 and egui. GTK4 wins on native menus, portals and themes; egui wins on iteration speed. | Smooth scroll over 10M virtual rows, native Wayland, working text input and IME, native file dialog, an accessibility path | L |
| SP-2 | Storage model | Full parse into Arrow/Polars; mmap plus row offset index; chunked parse with cache | mmap plus a `u64` offset per row, cells parsed on demand, columnar caches built lazily per column for sort and filter | 1 GB opens with first rows under 1 s; offset index for 10M rows fits in about 80 MB | L |
| SP-3 | Parallel indexing with quoted newlines | Single-threaded `csv-core` pass; speculative chunked parse with validation | Speculative chunks, verified at boundaries, single-thread fallback when a chunk guesses wrong | Correct row count on the adversarial corpus; faster than single-thread on 1 GB | M |
| SP-4 | Encoding scope for V0.1 | UTF-8 only; UTF-8 plus BOM variants; add Windows-1252 and UTF-16 via `encoding_rs` | UTF-8 and BOM detection on the mmap path; transcode other encodings to a temp file on open | Round-trip save preserves the original encoding and BOM | M |
| SP-5 | Row order and filter representation | Physical reorder; logical row map plus visible index | Stable `RowId`s, a logical row map that sort rewrites, and a filter that yields a visible subset of that map | Save under an active filter writes every row in logical order | M |
| SP-6 | Undo memory budget | Full snapshots; inverse commands; compact diffs with a byte cap | Inverse commands; sort stores the prior row map; bulk replace stores compact diffs; history capped by bytes | Undo of sort and replace-all on 1 GB stays within the memory target | S |

## Milestone 0: Technical prototype

M0 proves the thesis on ugly UI: a 1 GB CSV opens, scrolls smoothly, takes an edit and saves safely. It also closes SP-1 through SP-3.

| ID | Story | Crate | Size | Acceptance criteria |
| --- | --- | --- | --- | --- |
| M0-01 | Workspace scaffold with CI running fmt, clippy and tests | all | S | Crates match the PRD layout; CI blocks merges on failure |
| M0-02 | Test corpus generator | bench | M | Seeded output at 1 MB, 100 MB and 1 GB; narrow and 500-column variants; quoted newlines, embedded delimiters, BOM, ragged rows, leading zeros |
| M0-03 | Benchmark harness | bench | M | Records open time, time to first rows, index time, scroll frame times and peak memory; results stored per commit |
| M0-04 | Row offset indexer over mmap | csv-engine | L | Correct row count on every corpus file; parallel path per SP-3 |
| M0-05 | Cell fetch API | data-model | M | `cell(row, col)` returns the raw field; small LRU of parsed rows; no allocation per visible cell on a cache hit |
| M0-06 | Virtualized grid spike in each toolkit finalist | grid | L | Renders only visible cells plus a buffer; scroll and jump to row 10,000,000 stay smooth |
| M0-07 | Minimal edit overlay | data-model | S | Edited cells read from the overlay; source file untouched |
| M0-08 | Safe save | csv-engine | M | Writes to a temp file in the same directory, fsyncs, renames over the original; untouched rows byte-identical; killing the process mid-save leaves the original intact |

**Gate:** SP-1 and SP-2 decided and recorded. The 1 GB corpus file opens, scrolls and round-trips an edit within the proposed targets in the benchmark section.
