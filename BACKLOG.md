# Backlog

Source of truth for what to build next. The `next-story` skill reads and updates this file through `scripts/backlog.py`; edit it by hand freely, but keep the table shape.

- **Pri:** P0 blocks V0.1, P1 ships if time allows, P2 deferred (skipped unless asked).
- **Status:** `todo`, `in-progress`, `review`, `done`, `blocked`.
- **Depends:** story IDs that must be `done` first, comma-separated, or `-`.
- A milestone's stories unlock only when the previous milestone's gate is checked.

## Gates

- [ ] S0: decision records DEC-1 to DEC-5 accepted; CI green on every PR
- [ ] M0: a 1 GB CSV opens, scrolls smoothly, takes one edit, and saves safely
- [ ] M1: viewer passes the LibreOffice navigation checklist; first rows of 1 GB in under 500 ms
- [ ] M2: editor round-trips edits, paste, inserts, and undo on 1 GB without full rewrites before save
- [ ] M3: all V0.1 benchmarks in docs/BENCHMARKS.md pass on reference hardware (V0.1 release)
- [ ] M4: AUR package installs on a clean Arch VM and opens CSVs by double-click

## Stories

| ID | MS | Pri | Status | Depends | Story | Acceptance |
| --- | --- | --- | --- | --- | --- | --- |
| DEC-1 | S0 | P0 | review | - | Choose UI toolkit (GTK4 vs Slint vs egui vs Iced) | Spike renders a 1M-row virtual grid in the top two; ADR 0001 records frame time and input latency |
| DEC-2 | S0 | P0 | todo | INFRA-2 | Choose source storage (mmap + sparse index vs chunked arena vs columnar) | Spike indexes 1 GB; ADR 0002 records index time, index RAM, random row fetch latency |
| DEC-3 | S0 | P0 | todo | DEC-2 | Choose edit model (overlay + row map vs piece table) | Spike applies 10k random edits and an insert at row 500k; ADR 0003 shows O(1) or O(log n) per edit |
| DEC-4 | S0 | P0 | todo | DEC-2 | Choose sort/filter representation | Spike sorts a 2M-row numeric column in under 3 s; ADR 0004 |
| DEC-5 | S0 | P0 | todo | - | Choose concurrency model (Rayon + channels + cancel tokens vs async) | Spike runs a search while a timer simulates 60 fps; frame budget held; ADR 0005 |
| INFRA-1 | S0 | P0 | todo | - | CI: fmt, clippy -D warnings, test on every PR | Workflow in .github/workflows/ci.yml passes on main |
| INFRA-2 | S0 | P0 | todo | - | Test corpus generator | `scripts/gen_corpus.py` builds 10 MB, 100 MB, 1 GB files and the nasty set; corpus/ is gitignored |
| ENG-1 | M0 | P0 | todo | DEC-2 | Stream-index a CSV into a row offset index | 1 GB indexed in under 10 s; index under 200 MB RAM |
| ENG-2 | M0 | P0 | todo | ENG-1 | Fetch any row range by logical index | 100 visible rows in under 5 ms from anywhere in the file |
| ENG-3 | M0 | P0 | todo | ENG-1 | RFC 4180 parsing incl. quoted fields with embedded newlines | Nasty corpus parses with no row misalignment |
| GRID-1 | M0 | P0 | todo | DEC-1, ENG-2 | Virtualized grid renders visible cells plus a buffer | Constant memory regardless of row count; 60 fps scrolling at 2M rows |
| GRID-2 | M0 | P0 | todo | GRID-1 | Scrollbar drag jumps to arbitrary positions | Jump to row 1.9M renders in under 100 ms |
| EDIT-1 | M0 | P0 | todo | DEC-3, ENG-2 | Single-cell edit stored in overlay | Edit visible immediately; source file untouched |
| SAVE-1 | M0 | P0 | todo | EDIT-1 | Atomic save: temp file, verify, rename | kill -9 mid-save leaves original intact; output byte-identical except edited cells |
| BENCH-1 | M0 | P0 | todo | INFRA-1, INFRA-2, ENG-1 | Benchmark harness over the corpus | `cargo bench` runs in CI and prints the docs/BENCHMARKS.md table |
| ENG-4 | M1 | P0 | todo | ENG-3 | Auto-detect delimiter: comma, tab, semicolon, pipe | Correct on 95%+ of a 50-file real-world sample |
| ENG-5 | M1 | P0 | todo | ENG-4, DEC-5 | Manual delimiter override, re-index in background | Override applies without restart |
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

- **2026-10-05 DEC-1:** ADR 0001 proposes GTK4 (custom `snapshot` grid). Follow-up for GRID-1/GRID-2: at 3800x2080, a scrollbar jump that redraws every cell costs about 18 ms of Pango shaping per frame (30 fps). Plan to reuse per-row render nodes or fill cells progressively.
- **2026-10-05 baseline:** the scaffold fails `cargo fmt --all --check` (commands, data-model, grid) and `clippy -D warnings` (`byte_char_slices` in `csv-engine/src/lib.rs:33`) on Rust 1.99. Left for INFRA-1. The repo had no git history, so DEC-1 started from a fresh `git init` with a "Project scaffold" commit on `main`.
