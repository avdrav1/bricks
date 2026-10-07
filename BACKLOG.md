# Backlog

Source of truth for what to build next. The `next-story` skill reads and updates this file through `scripts/backlog.py`; edit it by hand freely, but keep the table shape.

- **Pri:** P0 blocks V0.1, P1 ships if time allows, P2 deferred (skipped unless asked).
- **Status:** `todo`, `in-progress`, `review`, `done`, `blocked`.
- **Depends:** story IDs that must be `done` first, comma-separated, or `-`.
- A milestone's stories unlock only when the previous milestone's gate is checked.

## Gates

- [x] S0: decision records DEC-1 to DEC-5 accepted; CI green on every PR
- [x] M0: a 1 GB CSV opens, scrolls smoothly, takes one edit, and saves safely
- [ ] M1: viewer passes the LibreOffice navigation checklist; first rows of 1 GB in under 500 ms
- [ ] M2: editor round-trips edits, paste, inserts, and undo on 1 GB without full rewrites before save
- [ ] M3: all V0.1 benchmarks in docs/BENCHMARKS.md pass on reference hardware (V0.1 release)
- [ ] M4: AUR package installs on a clean Arch VM and opens CSVs by double-click

## Stories

| ID | MS | Pri | Status | Depends | Story | Acceptance |
| --- | --- | --- | --- | --- | --- | --- |
| DEC-1 | S0 | P0 | done | - | Choose UI toolkit (GTK4 vs Slint vs egui vs Iced) | Spike renders a 1M-row virtual grid in the top two; ADR 0001 records frame time and input latency |
| DEC-2 | S0 | P0 | done | INFRA-2 | Choose source storage (mmap + sparse index vs chunked arena vs columnar) | Spike indexes 1 GB; ADR 0002 records index time, index RAM, random row fetch latency |
| DEC-3 | S0 | P0 | done | DEC-2 | Choose edit model (overlay + row map vs piece table) | Spike applies 10k random edits and an insert at row 500k; ADR 0003 shows O(1) or O(log n) per edit |
| DEC-4 | S0 | P0 | done | DEC-2 | Choose sort/filter representation | Spike sorts a 2M-row numeric column in under 3 s; ADR 0004 |
| DEC-5 | S0 | P0 | done | - | Choose concurrency model (Rayon + channels + cancel tokens vs async) | Spike runs a search while a timer simulates 60 fps; frame budget held; ADR 0005 |
| INFRA-1 | S0 | P0 | done | - | CI: fmt, clippy -D warnings, test on every PR | Workflow in .github/workflows/ci.yml passes on main |
| INFRA-2 | S0 | P0 | done | - | Test corpus generator | `scripts/gen_corpus.py` builds 10 MB, 100 MB, 1 GB files and the nasty set; corpus/ is gitignored |
| ENG-1 | M0 | P0 | done | DEC-2 | Stream-index a CSV into a row offset index | 1 GB indexed in under 10 s; index under 200 MB RAM |
| ENG-2 | M0 | P0 | done | ENG-1 | Fetch any row range by logical index | 100 visible rows in under 5 ms from anywhere in the file |
| ENG-3 | M0 | P0 | done | ENG-1 | RFC 4180 parsing incl. quoted fields with embedded newlines | Nasty corpus parses with no row misalignment |
| GRID-1 | M0 | P0 | done | DEC-1, ENG-2 | Virtualized grid renders visible cells plus a buffer | Constant memory regardless of row count; 60 fps scrolling at 2M rows |
| GRID-2 | M0 | P0 | done | GRID-1 | Scrollbar drag jumps to arbitrary positions | Jump to row 1.9M renders in under 100 ms |
| EDIT-1 | M0 | P0 | done | DEC-3, ENG-2 | Single-cell edit stored in overlay | Edit visible immediately; source file untouched |
| SAVE-1 | M0 | P0 | done | EDIT-1 | Atomic save: temp file, verify, rename | kill -9 mid-save leaves original intact; output byte-identical except edited cells |
| BENCH-1 | M0 | P0 | done | INFRA-1, INFRA-2, ENG-1 | Benchmark harness over the corpus | `cargo bench` runs in CI and prints the docs/BENCHMARKS.md table |
| ENG-4 | M1 | P0 | done | ENG-3 | Auto-detect delimiter: comma, tab, semicolon, pipe | Correct on 95%+ of a 50-file real-world sample |
| ENG-5 | M1 | P0 | done | ENG-4, DEC-5 | Manual delimiter override, re-index in background | Override applies without restart |
| ENG-6 | M1 | P0 | todo | ENG-3 | Encoding detection: UTF-8, UTF-8 BOM, UTF-16, Latin-1 | Encoding and BOM round-trip on save |
| ENG-7 | M1 | P1 | todo | ENG-4 | Header row detection with toggle | User can flip "first row is header" |
| GRID-3 | M1 | P0 | todo | GRID-1 | Keyboard navigation: arrows, Tab, Enter, Shift variants, Ctrl+Home/End, PgUp/PgDn | Matches LibreOffice on the side-by-side checklist |
| GRID-4 | M1 | P0 | todo | GRID-1 | Cell, range, row, and column selection | Click, Shift+click, drag, header click all work |
| GRID-5 | M1 | P0 | todo | GRID-1 | Resize columns and rows; double-click autofits column | Autofit samples visible rows only |
| APP-1 | M1 | P0 | todo | DEC-1 | Open via native file dialog (xdg-desktop-portal) | Works under Hyprland, GNOME, KDE |
| APP-2 | M1 | P0 | todo | ENG-1 | Status bar: row count, encoding, delimiter, save state | Row count updates while indexing streams |
| APP-3 | M1 | P0 | todo | ENG-1, DEC-5 | Show first rows before indexing finishes | First rows of 1 GB visible in under 500 ms |
| CMD-1 | M2 | P0 | todo | EDIT-1 | Command pattern for all mutations | Every edit is a reversible command object |
| CMD-2 | M2 | P0 | todo | CMD-1 | Undo/redo stores operations, not snapshots | 1,000-step history under 50 MB on a 1 GB file |
| EDIT-2 | M2 | P0 | todo | GRID-4, CMD-1 | In-cell editor; F2 or typing starts edit | Enter commits and moves down; Esc cancels |
| EDIT-3 | M2 | P0 | todo | CMD-1 | Insert and delete rows | Insert at row 1M in under 50 ms |
| EDIT-4 | M2 | P0 | todo | CMD-1 | Insert and delete columns | Works on 1 GB without a full rewrite before save |
| EDIT-5 | M2 | P0 | todo | CMD-1, GRID-4 | Delete key clears selected cells | Undoable as one step |
| CLIP-1 | M2 | P0 | todo | GRID-4 | Copy/cut range as TSV plus text/html | Pastes cleanly into LibreOffice, Excel web, a text editor |
| CLIP-2 | M2 | P0 | todo | CLIP-1, CMD-1 | Paste TSV into a range, expanding as needed | 10k rows from LibreOffice paste in under 1 s |
| SAVE-2 | M2 | P0 | todo | SAVE-1, ENG-6 | Save, Save As, New | Save As can change delimiter and encoding |
| SAVE-3 | M2 | P0 | todo | SAVE-1, DEC-5 | Background save with progress and cancel | UI responsive during 1 GB save; save under 15 s |
| SAVE-4 | M2 | P1 | todo | SAVE-1 | Preserve quoting style and line endings of untouched rows | Saving an unedited file produces an empty diff |
| ENG-8 | M3 | P0 | todo | DEC-5 | Background job framework: progress and cancel | Every long job cancels within 200 ms |
| TYPE-1 | M3 | P0 | todo | ENG-2 | Infer column types by sampling | Raw value never altered; `00123` stays `00123` |
| TYPE-2 | M3 | P1 | todo | TYPE-1 | Show inferred type in header; allow override | Override changes sort and filter only |
| SORT-1 | M3 | P0 | todo | TYPE-1, DEC-4, CMD-1, ENG-8 | Type-aware sort, ascending and descending | 2M-row numeric sort under 3 s; stable; undoable |
| SORT-2 | M3 | P0 | todo | SORT-1, ENG-7 | Sort moves whole rows and honors the header | Header row never moves |
| FILT-1 | M3 | P0 | todo | TYPE-1, DEC-4 | Per-column filter from header dropdown | Exact, contains, not contains, empty, non-empty, numeric compare |
| FILT-2 | M3 | P0 | todo | FILT-1 | Combine filters across columns (AND) | Status bar shows "x of y rows" |
| FILT-3 | M3 | P0 | todo | FILT-1, SAVE-1 | Saving with an active filter keeps hidden rows | Test confirms on-disk row count unchanged |
| FILT-4 | M3 | P1 | todo | FILT-2, CLIP-2 | Edits and paste respect the filtered view | Paste into a filtered range touches visible rows only |
| SRCH-1 | M3 | P0 | todo | ENG-8 | Find bar (Ctrl+F): whole file or current column, case toggle | First hit in 1 GB under 1 s, off the UI thread |
| SRCH-2 | M3 | P0 | todo | SRCH-1 | Next/previous result; hit count streams in | New keystroke cancels the running search |
| SRCH-3 | M3 | P0 | todo | SRCH-2, CMD-1 | Find and replace; replace all | Replace all on 100k hits undoes as one step |
| APP-4 | M4 | P0 | todo | APP-1 | Recent files menu | Last 10 files survive restart |
| APP-5 | M4 | P0 | todo | APP-1 | Drag and drop to open | Works on Wayland and X11 |
| APP-6 | M4 | P0 | todo | DEC-1 | Follow system dark/light theme; high-DPI | No blurry text at 1.5x and 2x |
| APP-7 | M4 | P1 | todo | CMD-2 | Crash recovery via periodic overlay journal | Relaunch after crash offers to restore edits |
| APP-8 | M4 | P1 | todo | ENG-3 | Clear errors for malformed files | Dialog shows line number and offending text |
| PKG-1 | M4 | P0 | todo | APP-1 | AUR package, .desktop file, text/csv MIME association | `yay -S` installs; double-click opens CSVs |
| PKG-2 | M4 | P2 | todo | PKG-1 | Flatpak manifest | Builds on Flathub CI |

## Notes

Story notes, decisions made mid-story, and follow-ups go here, newest first.

- **2026-10-06 ENG-5:** decisions:
  - The override is a header-bar dropdown: Auto, Comma, Tab, Semicolon, Pipe. Switching reuses the open file mapping with a new index (23 µs on the UI thread) and re-indexes on a worker thread (19.1M rows in 131 ms with the file cached), cancelling any index build still running. The choice persists across saves.
  - Switching is refused while edits are unsaved, and the dropdown is disabled with a tooltip, because edits are tied to the columns the old delimiter produced.
  - Smoke-tested in the real app by keyboard (focus the dropdown, Space, Home/Down, Enter). Injected pointer clicks focus the dropdown but don't open its list.
  - Process issue: one smoke step sent `wtype` keys while another app had focus. The driver now checks the active window before typing.
  - Follow-up: the delimiter choice isn't remembered between sessions (recent files, APP-4).
- **2026-10-06 ENG-4:** the 50-file sample is listed in `scripts/delimiter_sample.tsv`: 18 comma, 14 semicolon, 12 tab, 6 pipe, all from public portals. `scripts/fetch_delimiter_sample.py` downloads the first 256 KiB of each into `corpus/realworld/` (gitignored; no third-party data is committed). I set each label by reading the file; the fetch script cross-checks it with Python's `csv` module, and all 50 agree. A few sources are live feeds, so their content changes; their format doesn't. The detector got 50/50 correct on the first run, with no tuning against the sample. The real-world test is `#[ignore]` because it needs network access to fetch the sample, so CI runs only the unit tests. Detection reads the first 64 KiB, scores up to 200 rows, and skips `#` comment lines while scoring. The delimiter override (ENG-5) is the fallback when detection is wrong.
- **2026-10-06 BENCH-1:** the harness is `app/benches/corpus.rs` (`harness = false`), so it can launch the real app binary for the rows that need a window. It reads rows and targets from `docs/BENCHMARKS.md`; a new row prints "no measurement for this row yet" until code for it is added. CI has a separate `bench` job that generates the 1 GB corpus. On the runner (4 threads, 16 GiB, no display) it measured index 2.61 s and save 6.63 s, and printed the four window rows as skipped. Library and binary targets set `bench = false`, so `cargo bench` prints only the table. Not done: storing results per commit (the old plan's M0-03 wording; not in this criterion).
- **2026-10-06 SAVE-1:** decisions:
  - The save writes untouched rows as raw byte ranges. In an edited row, untouched fields keep their exact bytes (quoting included) and so does the row's line ending; only edited cells are encoded, with minimal quoting.
  - Verification re-indexes the temp file and checks its size and row count before the rename. That adds roughly 0.3–0.7 s on 1 GB.
  - Ctrl+S runs the save on a plain `std::thread`. Progress and cancel are SAVE-3, and the shared pool is ENG-8.
  - Editing is paused while a save runs. Afterwards the file is reopened and re-indexed, and the undo history starts over.
  - Follow-up: a save killed mid-write leaves its temp file (`.<name>.<pid>-<n>.bricks-save`) next to the original. Nothing cleans these up yet; pick that up with crash recovery (APP-7) or the next save.
- **2026-10-06 EDIT-1:** decisions:
  - `EditOverlay` is now keyed by `RowId` (ADR 0003) and grouped per row. `CellRef.row` is a `RowId`, and `CsvTable::row_id` is the identity map until EDIT-3 adds the row map.
  - Edits run as `commands::SetCell` through an `UndoStack`, which keeps invariant 5. No undo keys are bound yet; that's CMD-1/CMD-2.
  - The UI is a minimal stand-in: double-click opens a popover editor and Enter applies. The in-cell editor (F2/typing, Enter moves down) is EDIT-2.
  - Editing a non-UTF-8 cell prefills the editor with U+FFFD replacements until ENG-6.
  - Smoke tests can drive the app: on Hyprland, `hl.dsp.send_key_state({key="mouse:272", state="down"|"up"})` injects real clicks (`send_shortcut` with a mouse key does not reach GTK), and `wtype` types.
- **2026-10-06 GRID-2:** no grid change was needed; GRID-1's cache already reloads on a jump. `--bench-jump` and `app/tests/jump_perf.rs` time each jump from setting the vertical adjustment (which is what a scrollbar drag does) to the end of the first frame that paints the target row. No real pointer drag is injected; this machine has no tool for that. A run where the window was hidden showed one 81.5 s "jump"; the test now fails with "keep the window visible" for any wait over 1 s. Related open item from DEC-1: a continuous drag on a near-4K window redraws every cell each frame, which ran at about 30 fps in the DEC-1 spike. Not covered by this story's criterion.
- **2026-10-06 GRID-1:** `app/tests/scroll_perf.rs` opens a real window, so the window has to stay visible while it runs. On this shared desktop, one run stalled 33 s when the window was hidden, and Hyprland tiles the window at whatever size is free (1887×2086 or 1887×1029). Clean runs at 19.1M rows: 60.0 fps with 1–3 of 600 frames late, frame CPU p99 4.9–6.3 ms. One earlier run at 19.1M rows was slower (58.6 fps, CPU p99 15.9 ms) and didn't repeat; recheck on the reference box with BENCH-1. Mid-story choices: indexing runs on a plain `std::thread` until ENG-8 builds the shared job pool (ADR 0005); cell text shown in the grid is capped at 256 bytes (raw values untouched) so grid memory stays constant. The windows look slightly see-through on this machine because Omarchy sets 0.985/0.96 opacity on every window.
- **2026-10-06 ENG-2:** `fetch_perf` checks the 5 ms target against the page-cache-warm case, which is the app's state right after open (indexing reads the whole file). There the worst window took 9.7 µs. With the file evicted before every window (memory pressure), p50 is 2.1–2.4 ms and p99 is 2.8–3.8 ms, but the worst case is 8.3–8.6 ms, over 5 ms. That is one NVMe read; revisit with BENCH-1 on the reference box if it matters there.
- **2026-10-06 ENG-1:** perf acceptance tests (`#[ignore]`) assert timing only in optimized builds. Debug is about 10–25x slower: the quote-heavy file took 15 s to index in debug and 0.59 s in release. Run them with `cargo test --release --workspace -- --ignored`. The next-story skill's `cargo test --workspace -- --ignored` still checks counts and memory. Also: the `pull_request` trigger did not fire for PR #1 either (2026-10-06), so CI is started manually with `gh workflow run ci --ref <branch>` until Actions triggers are fixed in repo settings.
- **2026-10-05 INFRA-1:** CI passed on `main` at `6f30e9d` ([run 37393662522](https://github.com/avdrav1/bricks/actions/runs/37393662522), started manually through `workflow_dispatch`). Follow-up: none of four pushes to `main` created a `push`-event run, even though Actions is enabled and the workflow is active. The commit check-suites list only third-party apps, not `github-actions`. The `pull_request` trigger is still unverified until the first PR.
- **2026-10-05 DEC-1:** ADR 0001 proposes GTK4 (custom `snapshot` grid). Follow-up for GRID-1/GRID-2: at 3800x2080, a scrollbar jump that redraws every cell costs about 18 ms of Pango shaping per frame (30 fps). Plan to reuse per-row render nodes or fill cells progressively.
- **2026-10-05 baseline:** the scaffold fails `cargo fmt --all --check` (commands, data-model, grid) and `clippy -D warnings` (`byte_char_slices` in `csv-engine/src/lib.rs:33`) on Rust 1.99. Fixed in INFRA-1. The repo had no git history, so DEC-1 started from a fresh `git init` with a "Project scaffold" commit on `main`.
