# 0005: Concurrency model

- Story: DEC-5
- Status: accepted
- Date: 2026-10-05

## Question

How do long jobs (index, search, sort, filter, inference, save) run off the UI thread, report progress, and cancel, given that the UI thread is GTK's glib main loop (ADR 0001)?

## Options

| Option | Measured result (1 GB corpus, 60 fps simulated frames) | Cost / risk |
| --- | --- | --- |
| A. Rayon pool + `async-channel` to the glib loop + `Arc<AtomicBool>` cancel token | 0 of 1,455 frames over budget on 8 cores, and 1 of 843 frames with the pool on all 24 logical CPUs. Cancel acknowledged on the UI thread p99 ≤ 10.1 ms, max 11.0 ms. Warm search 49–100 ms. | Cancellation is cooperative, checked once per 4 MiB chunk. The unbounded progress channel needs coalescing in real jobs. Data-parallel helpers (`par_iter`, `par_sort`) are built in. |
| B. tokio multi-thread runtime + `tokio::sync::mpsc` + `CancellationToken` | 0 of 1,444 frames over budget on 8 cores, and 1 of 844 on all 24. Cancel p99 ≤ 9.9 ms, max 10.7 ms. Warm search 46–100 ms. | Adds a second executor next to glib's with no I/O-bound work to justify it. CPU jobs need hand-rolled chunk scheduling (an atomic counter, a `JoinSet`, and `yield_now`) or `spawn_blocking`. There is no parallel sort, which DEC-4 needs. |

The two options are within run-to-run noise on every metric. The decision rests on fit, not speed.

## Spike

`spikes/dec-5/` holds `harness.rs` (shared), `rayon-jobs/`, `tokio-jobs/`, `run.py`, and `results.json`.

- **UI thread:** a real `glib::MainLoop` (GTK's loop, without a window). A frame clock fires at 16.667 ms deadlines and spins 4 ms per frame, which is about the GTK grid's frame CPU from the DEC-1 spike. A frame is **over budget** if it finishes more than 16.667 ms after its deadline. Missed deadlines are skipped, as vsync would.
- **Job:** a case-insensitive ASCII search of `corpus/rows_1024mb.csv` (1 GiB, 19.1M rows) through `mmap`, split into 4 MiB chunks. Each chunk is lowercased into a scratch buffer and searched with `memchr::memmem`. Each chunk sends a progress message, which the UI receives with `MainContext::spawn_local`. Both engines find the same 2,725,768 hits for "portland".
- **Phases:**
  - *Baseline:* frames only for 2 s.
  - *Search:* 20 full searches with 200 ms gaps. The first search is excluded from the timings.
  - *Cancel:* 5 s of simulated typing. Each search is cancelled at 10–80% of the median search time, cancellation fires from a glib timeout on the UI thread, and the next prefix starts as soon as `Done` arrives. This keeps every pool thread busy continuously.
- **Hardware:** Ryzen 9 5900 (12 cores / 24 threads), 125 GB RAM, NVMe, Arch Linux, Rust 1.99. **8-core** means `taskset -c 0-7`, which is 8 physical cores and approximates the reference box's core count. This is not the reference box. The file was in the page cache. 2 reps per row.

| CPUs / pool | Engine | Warm search p50 (ms) | First hit p50 (ms) | Frames under load | Over budget | Frame done p99 / max (ms) | Frame start late p99 (ms) | Progress lag p99 (ms) | Cancel → `Done` on UI p50 / p99 / max (ms) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 24 / 24 | rayon | 100 | 9.4–9.8 | 843 | 1 | 11.0 / 16.9 | 3.9 | 5.3 | 5.8 / 10.1 / 11.0 (n=185) |
| 24 / 24 | tokio | 100 | 9.7–9.8 | 844 | 1 | 8.9 / 24.2 | 3.2 | 5.0 | 5.8 / 9.9 / 10.7 (n=186) |
| 8 / 8 | rayon | 53 | 2.6–3.3 | 733 | 0 | 7.0 / 8.7 | 2.6 | 4.4 | 1.2 / 5.4 / 5.5 (n=383) |
| 8 / 8 | tokio | 50 | 2.1–2.7 | 725 | 0 | 7.8 / 9.0 | 2.3 | 4.0 | 1.1 / 5.0 / 5.4 (n=409) |
| 8 / 7 | rayon | 49 | 2.2–2.4 | 722 | 0 | 6.4 / 6.9 | 2.0 | 4.0 | 1.0 / 4.9 / 6.0 (n=428) |
| 8 / 7 | tokio | 46 | 2.1–2.3 | 719 | 0 | 6.4 / 7.8 | 1.8 | 3.9 | 0.9 / 5.0 / 5.9 (n=440) |

Other observations:

- **Baseline (no job):** 1,429 frames, 0 over budget, frame done p99 5.1 ms.
- **Progress lag:** most of it is the 4 ms frame spin. Messages wait while the UI thread renders.
- **Peak RSS:** 1.06–1.22 GiB. This is mostly the file's page-cache pages counted through the mapping, not heap.
- **Pool size:** 24 threads on 24 logical CPUs searched more slowly (100 ms) than 8 threads on 8 physical cores (50 ms). This run doesn't show why. Pool size should be tuned in ENG-8 with BENCH-1.
- **Cancel races:** 1 search finished all its chunks after the cancel was requested, and 3 searches completed before their cancel timer fired.

To rerun:

```sh
python3 scripts/gen_corpus.py
(cd spikes/dec-5 && cargo build --release)
python3 spikes/dec-5/run.py
```

## Decision

**Option A, Rayon + channels + cancel tokens.** Accepted 2026-10-05.

1. **The work is data-parallel CPU over an mmap.** Index, parse, sort, filter, inference, and search all fit Rayon's `par_iter` and `par_sort` with work stealing. DEC-4's 2M-row sort needs a parallel sort that tokio doesn't have. File I/O goes through mmap page faults and plain blocking writes, which async can't make non-blocking.
2. **The UI thread already has an executor.** glib's `MainContext::spawn_local` awaits runtime-agnostic channels, and GTK's own async (portals, `GtkFileDialog`) runs on gio. A tokio runtime would be a second scheduler with nothing to schedule.

Measured behavior is the same for both options. Either holds the frame budget, and either cancels about 20x inside ENG-8's 200 ms target.

## Consequences

- **Contract for ENG-8:** a toolkit-free job module below `app`, so it never sees GTK.
  - A job gets a `CancelToken` (`Arc<AtomicBool>`) and checks it at least every few milliseconds of work (one 4 MiB chunk in the spike).
  - It streams `Progress`/`Done` over `async-channel`.
  - `app` awaits the receiver with `glib::MainContext::spawn_local`.
  - Rayon runs on one shared pool.
- **Pool size:** start at `available_parallelism() - 1`, so the UI thread keeps a core. Over-budget frames appeared only when the pool used every logical CPU. Tune with BENCH-1.
- **Progress coalescing:** progress must be coalesced (the latest value plus a wake-up) rather than queued per chunk, so a stalled UI cannot build an unbounded backlog.
- **Revisit if:** we add network or other I/O-bound features (remote files, collaboration, which is a non-goal), or a job cannot be chunked finely enough to cancel within 200 ms.
- **As built in ENG-8 (2026-10-09):** the `jobs` crate. `jobs::spawn(work)` runs `work(&CancelToken)` on one shared Rayon pool (`available_parallelism() - 1` threads, at least 1) and returns a `Job<T>`. The app awaits `Job::finished()` with `glib::spawn_future_local`; workers and tests use `Job::wait()`. Index, save, and copy run on it.
  - Progress is not sent over the channel. Each job keeps its own latest value in atomics (bytes indexed, bytes written, rows copied), and the UI reads them on its 100 ms status tick. That is the coalescing above, with the tick as the wake-up. Only the result goes over the channel (capacity 1).
  - A job cancelled while it is still queued never runs: it reports `Stopped::Cancelled` at once, so a full pool can't delay a cancel. A panicking job reports `Stopped::Panicked`, and the pool keeps working.
  - The save's fsync can't be interrupted and takes as long as the disk needs (136 ms for 1 GB on NVMe here), so it runs on a helper thread that the save stops waiting for on cancel.
  - Pool size is not tuned yet: index, save, and copy are each one serial loop, so the thread count only matters once a job uses `par_iter` or `par_sort`. Tune it with the first one (SORT-1 or SRCH-1).
- **As built in SRCH-1 (2026-10-09):** search is a job on a table snapshot like sort and filter. It reads the file in 65,536-row chunks in parallel, checking the cancel flag before each, and adds to a rows-done atomic for the status bar. A new search replaces a running one (the old job is cancelled), and Esc cancels. Cancel came back within 8 ms on 1 GB.
  - SRCH-2: each keystroke in the find bar cancels the running search at once (`changed`); the search for the new text starts when typing pauses (`search-changed`, 150 ms). Cancelled searches came back within 10 ms of the key (`app/tests/find_typing.rs`). The matches of the last search are kept until the table's revision changes, so next and previous only walk the rows.
