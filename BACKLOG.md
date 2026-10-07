# Backlog

Source of truth for what to build next. The `next-story` skill reads and updates this file through `scripts/backlog.py`; edit it by hand freely, but keep the table shape.

- **Pri:** P0 blocks V0.1, P1 ships if time allows, P2 deferred (skipped unless asked).
- **Status:** `todo`, `in-progress`, `review`, `done`, `blocked`.
- **Depends:** story IDs that must be `done` first, comma-separated, or `-`.
- A milestone's stories unlock only when the previous milestone's gate is checked.

## Gates

- [x] S0: decision records DEC-1 to DEC-5 accepted; CI green on every PR
- [x] M0: a 1 GB CSV opens, scrolls smoothly, takes one edit, and saves safely
- [x] M1: viewer passes the LibreOffice navigation checklist; first rows of 1 GB in under 500 ms
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
| ENG-6 | M1 | P0 | done | ENG-3 | Encoding detection: UTF-8, UTF-8 BOM, UTF-16, Latin-1 | Encoding and BOM round-trip on save |
| ENG-7 | M1 | P1 | done | ENG-4 | Header row detection with toggle | User can flip "first row is header" |
| GRID-3 | M1 | P0 | done | GRID-1 | Keyboard navigation: arrows, Tab, Enter, Shift variants, Ctrl+Home/End, PgUp/PgDn | Matches LibreOffice on the side-by-side checklist |
| GRID-4 | M1 | P0 | done | GRID-1 | Cell, range, row, and column selection | Click, Shift+click, drag, header click all work |
| GRID-5 | M1 | P0 | done | GRID-1 | Resize columns and rows; double-click autofits column | Autofit samples visible rows only |
| APP-1 | M1 | P0 | done | DEC-1 | Open via native file dialog (xdg-desktop-portal) | Works under Hyprland, GNOME, KDE |
| APP-2 | M1 | P0 | done | ENG-1 | Status bar: row count, encoding, delimiter, save state | Row count updates while indexing streams |
| APP-3 | M1 | P0 | done | ENG-1, DEC-5 | Show first rows before indexing finishes | First rows of 1 GB visible in under 500 ms |
| CMD-1 | M2 | P0 | done | EDIT-1 | Command pattern for all mutations | Every edit is a reversible command object |
| CMD-2 | M2 | P0 | done | CMD-1 | Undo/redo stores operations, not snapshots | 1,000-step history under 50 MB on a 1 GB file |
| EDIT-2 | M2 | P0 | done | GRID-4, CMD-1 | In-cell editor; F2 or typing starts edit | Enter commits and moves down; Esc cancels |
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

- **2026-10-07 EDIT-2:** in-cell editor.
  - A `gtk::Entry` child of the grid is laid over its cell in `size_allocate`, follows scrolling, and is clipped to the body. It replaces the popover, and a small CSS class lets it fit a 22 px row.
  - Key rules live in `app/src/editor.rs`, as pure functions with unit tests:
    - F2 or a printable key starts an edit (typing replaces the content; Ctrl/Alt/Super never start one).
    - Enter, Shift+Enter, Tab and Shift+Tab commit, then move through `Selection::press`, so a selected range and the Enter-after-Tabs return work as in Calc.
    - Esc cancels.
    - Arrows commit and move after typing, and move the text cursor after F2.
  - The editor's key controller runs in capture phase on the entry. The grid's own controller ignores keys while editing, because they bubble up from the entry: Up/Down would otherwise move the cursor mid-edit.
  - Clicking elsewhere commits (focus leave), and Ctrl+S commits first, then saves. Edits still go through `SetCell` and undo; the entry keeps its own text undo while open.
  - Smoke test via Broadway on a /tmp copy of the 10 MB file:
    - Typing then Enter committed and moved down; F2 then Esc left the cell unchanged.
    - a Tab b Tab Enter wrote B6/C6 and landed on B7.
    - Typing then Down committed and moved.
    - Double-click, append, then a click elsewhere committed.
    - Typing, then Ctrl+S, committed and saved ("Saved", • cleared); the saved file holds all five edits on the right lines.
  - Not yet: IME preedit in the grid (typing starts from the key's character), and an editor that grows past its column for long text.
- **2026-10-07 CMD-2:** history capped by steps and bytes (spec SP-6).
  - `Command::heap_bytes` is required, so every command reports what it holds. `UndoStack` keeps a running total over both undo and redo history, re-measured around each apply and revert, since an edit holds whichever value it would swap back in. Defaults: `HISTORY_STEPS` 1,000 and `HISTORY_BYTES` 50 MB (`UndoStack::default()`, used by the app).
  - Over a limit, the oldest steps go first: the far end of the undo history, then the far end of the redo history. The step just run or just undone always stays, so the last action can be undone even if it alone is over budget; the next action drops it. The stack moved from `Vec::remove(0)` to `VecDeque`.
  - Unit tests: 1,000 commands of 1 MB each (about 1 GB in total) stay at or under 50 MB, keeping the newest 49 steps; a 60 MB step survives as the only one; the step limit still applies.
  - Acceptance: `commands/tests/history_1gb.rs` (ignored, needs the corpus, release). 1,000 edits spread over the 1 GB file's 19.1M rows (100-byte values; every tenth re-edits the cell from nine steps earlier, so its inverse holds a value) keep all 1,000 steps. The history counts 41 KiB by its own count, and process anonymous memory grew 456 KiB, edit values included. Everything undoes and redoes.
- **2026-10-07 CMD-1:** every edit is a reversible command.
  - Data changes only through `CsvTable::apply(Edit) -> Edit`, which returns the inverse. The raw setters are gone from the public API: a `compile_fail` doc test on `apply` guards that, next to a compiling example with the same paths. `Edit::Cell` (set or clear) is the only operation so far; EDIT-3/4 and CLIP-2 add theirs to the enum.
  - `commands::SetCell` holds one `Edit` and swaps it for its inverse on each apply and revert (`SetCell::new`, `SetCell::clear`). `Command::focus` names the cell a command changes, and `UndoStack::undo`/`redo` return the command they ran.
  - Acceptance: `every_edit_reverts_exactly` runs 300 seeded random sets and clears through the stack, including repeats and cells past short rows. Every undo restores exactly the previous state (all cell values compared) and reports the cell it changed; after undoing everything no edit is left; redoing everything restores the final state. The old two-step test was a subset and is removed.
  - App: Ctrl+Z, and Ctrl+Shift+Z or Ctrl+Y, in the window key handler next to Ctrl+S. Undo is refused during a save, like edits; it jumps the cursor to the changed cell (`CsvTable::row_of`) and refreshes cached rows and titles. The cell editor's entry keeps its own text undo, since it handles the key first.
  - Smoke test via Broadway on a /tmp copy of the 10 MB file: two edits, two undos (each reverted one cell and moved the cursor there; the edit count went 2 → 1 → 0, the • cleared, the delimiter dropdown came back), then redo via Ctrl+Shift+Z and Ctrl+Y. The copy's sha256 was unchanged.
  - Not built yet: compound commands (one undo step for many cells) arrive with their first user (EDIT-5, CLIP-2, SRCH-3). The 50 MB memory bound is CMD-2.
- **2026-10-07 decision:** a news/sports plugin is out, permanently (user decision). Plugins are a V0.1 non-goal (CLAUDE.md invariant 8), and this one won't come back later either. Don't propose it again.
- **2026-10-07 M1 gate:** approved by the user on this machine's numbers.
  - Checklist: `lo_parity` replays all 49 LibreOffice-recorded scenarios, and all pass.
  - First rows of 1 GB with a cold cache: 259 ms (BENCH-1) and 281 ms (GRID-5) on the desktop, and 176–186 ms via Broadway after ENG-7. Indexing was still running at that frame in every run.
  - These come from a 12-core Ryzen 9 5900 with 126 GiB, not the reference box (8 cores, 16 GB) in docs/BENCHMARKS.md. The user accepted them anyway. A reference-box run is still owed before the M3 gate, which requires it.
- **2026-10-07 ENG-7:** header row detection with a toggle.
  - `csv_engine::detect_header` works in the spirit of Python's `csv.Sniffer.has_header`. Columns whose values below the first row are all numbers, or all the same length, vote on whether the first cell stands out (text above numbers, a different length). Free-text columns don't vote, and ties go to "header", because most CSVs have one. A file with no second row, or a `#` comment first, has none. `detect_dialect` sets `Dialect::has_header`; a fixed delimiter re-detects with that delimiter. `Dialect::default()` now has no header, so tables built in code keep every row as data.
  - Real-world check on the 50 files in `corpus/realworld/` (throwaway survey, deleted): 49 decided right. The miss is `tab-geonames-feature-codes.txt`, which is all free text with no header: a tie, so it's treated as having one.
  - The model is `CsvTable::has_header`/`set_header`. With a header, table row r is file row r+1 (via `row_id`), `row_count` leaves the header out, and `header_cell` gives the titles. Edits stay on their file rows, and a save writes every file row, the header included. The acceptance test is `flipping_first_row_is_header` in data-model: flip off and on, edits stay put, a column renamed while the header is off becomes its title, and save output and the file are checked.
  - In the app, column headers show the dimmed letter plus the title; titles are read once and refreshed on table changes, flips and edits, not every frame. Autofit includes the title. The **Header row** toggle is in the header bar. `Session::header` keeps the user's explicit choice across re-reads (delimiter change, save); the status tick syncs the button to the grid without recording a choice. Row heights reset on a flip, because rows shift by one.
  - The 1 GB corpus file now shows 19,094,579 data rows (`status_stream` updated); the index still counts 19,094,580 lines.
  - Smoke test via Broadway on the 1 GB file: the header was detected (titles "A id", "B name", …; 19,094,579 rows), flipped off (row 1 = "id, name, …"; 19,094,580) and back on. With the header off, a delimiter change to Comma kept it off.
  - Perf on Broadway, sequential runs at the same viewport: `jump_perf` p50 was 46.0/46.5 ms against 43.0/45.5 ms for `main`. That's inside Broadway's run-to-run spread; first jumps overlap (`main` 60/83, branch 67/115). `first_rows` passes (176–186 ms). The 100 ms jump target still needs a desktop run, as noted under GRID-5.
- **2026-10-07 APP-3:** first rows before indexing finishes. The behavior already existed: the index streams rows from ENG-1 on, and the grid paints whatever is indexed (GRID-1). This story adds the proof.
  - `--bench-open` now prints `FIRST_FRAME {"rows_indexed":N,"complete":false}`: the rows indexed when rows were first painted. `cargo bench` matches on the prefix.
  - Acceptance: `app/tests/first_rows.rs` (ignored, needs a display) evicts the 1 GB file from the page cache, times spawn to `FIRST_FRAME` (median of 3 under 500 ms, asserted in release builds), and requires indexing to have been incomplete then. Via Broadway: 152, 176 and 177 ms, with 3.5–3.6M of 19.1M rows indexed. The desktop `cargo bench` measured 281 ms for the same moment (GRID-5).
  - `cargo bench` on Broadway, to check the parser without opening desktop windows: first rows 161 ms. Its scroll p99 of 43.9 ms at 7 fps comes from Broadway: each frame is serialized to a headless browser. That is not a grid regression (2.2 ms on the desktop in GRID-5), and those numbers are not reference-box numbers for docs/BENCHMARKS.md.
- **2026-10-07 APP-2:** status bar.
  - The bar under the grid shows three parts. On the left, the row count with "indexing N%" while it grows; the percentage is bytes scanned over file size (`SparseRowIndex::bytes_indexed`). On the right, the save state: unsaved edits, Saving…, Saved in N s, Save failed, and File changed on disk. Next to it, the delimiter and encoding. The title now carries only the file name, with • for unsaved edits.
  - The text comes from `app/src/status.rs`, pure functions with unit tests for wording and precedence. A save result gives way to newer edits, and a failure keeps the edit count.
  - The status tick now also runs once as the window is built, so the bar is never empty and the earliest count shows. Before this change the 1 GB file showed only one intermediate value, because indexing takes about 0.4 s.
  - Acceptance: `app/tests/status_stream.rs` (ignored, needs a display) runs `--bench-status` on the 1 GB file and requires at least two growing counts while indexing, then the exact total. Passed via Broadway: 2,574,913 (13%), then 12,155,103 (63%), then 19,094,580.
  - Smoke test via Broadway on the 1 GB file: the row count, "1 unsaved edit" with a • in the title, the delimiter dropdown disabled, then after Ctrl+S "Saved in 1.1 s".
  - The smoke test's save wrote an edit into `corpus/rows_1024mb.csv`. I reverted the cell and saved again; the file's sha256 matches a fresh `gen_corpus.py --max-mb 1024`.
- **2026-10-07 APP-1:** open through the desktop's file dialog.
  - `gtk::FileDialog`, no new dependencies. GTK 4.22 uses `org.freedesktop.portal.FileChooser` whenever a portal with FileChooser version ≥ 3 is on the session bus, and falls back to its own chooser otherwise (`gdk_display_should_use_portal`). So each desktop shows its own dialog: GNOME's, KDE's, or xdg-desktop-portal-gtk on Hyprland, whose `hyprland-portals.conf` routes FileChooser to gtk.
  - Start window when launched without a file; Ctrl+O (`app.open`) and a header-bar button everywhere. Each file opens in its own window (one `Session` each); choosing an already-open file (canonical path) brings its window forward instead of a second window saving over the first. Errors (e.g. permission denied) show an alert; cancelling does nothing. The command-line file path still opens before GTK starts, which BENCH-1's cold start measures.
  - Smoke test on a private D-Bus session, with xdg-desktop-portal, xdg-desktop-portal-gtk on GTK 3 Broadway (:6), the app on GTK 4 Broadway (:5), and `dbus-monitor` running, all driven from a headless browser. The run showed:
    - The app called `portal.FileChooser.OpenFile` with the "CSV and text files" and "All files" filters; the portal forwarded it to the gtk backend, which drew its dialog.
    - Choosing `rows_100mb.csv` returned Response 0 and opened the file in a new window, and the start window closed.
    - A file with mode 000 showed "Permission denied (os error 13)".
    - A second file opened a second window.
    - Re-opening either file, including through a `corpus/../corpus/` path, opened no new window.
    - Escape returned Response 2 and nothing happened.
  - Hyprland: the user checked it by hand on the real session (Open… showed the desktop's dialog attached to the window, the chosen CSV opened in a new window and the start window closed, and Ctrl+O on the same file opened no second window). GNOME and KDE are not installed here; per the user, they are a follow-up.
  - Follow-up (before PKG-1 / first release): open a file through the dialog on GNOME and KDE (VMs), and confirm each desktop's own dialog appears.
- **2026-10-07 GRID-5:** resizing and autofit.
  - Sizes are sparse: `grid::Sizes` stores only the resized rows and columns, with running offsets. Position and hit-test cost one map lookup, or O(resized) for hit-testing, at any row count (invariant 2). `Viewport`/`ColumnViewport` became one axis `grid::Viewport` over `Sizes`.
  - Autofit (double-click a column border) measures only the rows on screen (`GridCache::widest`; test `autofit_measures_only_the_visible_rows` puts a wider value in the cache's off-screen buffer). It is capped at the body width; an empty column gets the default width.
  - Double-clicking a row border restores the default height: rows hold one line, so that is their optimal height.
  - Selected whole columns resize and autofit together, as in Calc. Rows resize one at a time: a whole-row selection can be millions of rows, which the sparse map would have to store one by one. Follow-up if wanted: range entries in `Sizes`.
  - Sizes are view state, not undoable commands: they don't change the data and aren't saved (CSV has no place for them). A save keeps them; a delimiter change resets the widths.
  - Perf: `cargo bench` all ✓ (scroll p99 2.2 ms frame CPU, was 1.8 ms; first rows 281 ms; index 0.64 s; save 1.05 s).
  - The jump test (`jump_perf`) failed its visibility guard on the desktop because the window was hidden while the user worked. Rerun on Broadway in a headless browser, it gives a first jump of 119.4 ms and p50 29.5 ms, against 122.5 ms and 29.5 ms for `main` under the same conditions. That's no regression; Broadway adds about 25 ms per frame, and the test still needs a visible desktop window to check the 100 ms target.
  - Smoke test via Broadway on the 1 GB file: narrowed column A by drag, autofit column C, made row 3 taller by drag and reset it with a double-click, clicked a cell after resizing (C5 hit correctly), and dragged one of D:F to resize all three.
- **2026-10-07 GRID-4:** mouse selection and whole rows/columns on GRID-3's model.
  - The LibreOffice recording grew to 49 scenarios with Shift/Ctrl+Space (49/49 match). Calc runs whole rows and columns to the sheet edge (XFD, row 1048576); the replay maps cells near that edge onto the data edge. Shift+Ctrl+Space gives Calc's data area, which here is the whole table.
  - Mouse clicks can't be driven through Calc's UITest API. Their behavior follows the same model (Shift+click and drag move only the far corner) and has unit tests. A header click puts the cursor in the first visible cell of that row or column [inferred from Calc, not recorded].
  - Whole rows and columns are flagged in `Selection`, and `Selection::grow` keeps them reaching the edge while indexing finds more rows, or after a save reopens the file.
  - The view never jumps along an axis the selection spans entirely (`Selection::reveal`). Without this, Ctrl+Space scrolled to the last row.
  - Dragging past the body's edge autoscrolls every 40 ms, faster the further out the pointer is.
  - Smoke test via Broadway in a headless browser on the 1 GB file (19.1M rows): click, Shift+click, drag, row-header Shift+click, column-header drag, corner, Ctrl/Shift+Space, autoscroll from row 57 to row 1, and double-click edit. Broadway doesn't report the pointer outside its window, so autoscroll was checked with the pointer over the column-header strip.
- **2026-10-06 GRID-3:** the side-by-side checklist is recorded, not written by hand.
  - `scripts/lo_navigation.py` drives headless LibreOffice Calc 26.8 through UNO and its built-in UITest service: it sends 39 key sequences and reads back the cursor and selection. It writes `crates/grid/tests/data/lo_navigation.tsv` and `docs/navigation-checklist.md`.
  - `grid/tests/lo_parity.rs` replays every sequence through `grid::Selection` and must match LibreOffice: 39/39 do.
  - Calc behaviors this captured:
    - Shift-extension moves only the far corner.
    - Tab/Enter cycle inside a selected range.
    - Enter after a run of Tabs returns to the starting column.
    - Page Up/Down move the cursor and the view together.
  - Differences, by design:
    - Movement stops at the last row and column of data; Calc has empty cells beyond.
    - The column count is the widest row seen so far.
  - Out of scope: Ctrl+arrows (jump to data edges) and Alt+Page Up/Down.
  - A single click only focuses the grid; selecting by click is GRID-4. Double-click (edit) also moves the cursor.
  - Smoke test: the real app was driven by keyboard through GTK's Broadway backend in a headless browser, because the desktop was in use; screenshots confirm cursor, range, Tab-in-range, Tab-Tab-Enter, Ctrl+End on 1.9M rows, and Page Up.
- **2026-10-06 ENG-6:** decisions:
  - "Latin-1" is handled as Windows-1252. Real "Latin-1" files usually are; it shows €/“”/– in 0x80–0x9F, and all 256 byte values round-trip (tested).
  - Non-UTF-8 files are decoded at open into a UTF-8 copy in `~/.cache/bricks/` (SP-4's lean). The copy is mapped and then unlinked, so nothing is left behind even after a crash. Measured: 200 MB of UTF-16 opens in 184 ms and indexes by 199 ms; saving it with 1,000 edits takes 427 ms.
  - The save re-encodes on the way out (`csv_engine::EncodeWriter`), keeping the BOM. Verification decodes the result the same way. A character the encoding can't hold (e.g. 東 in Windows-1252) fails the save with a message naming it, and the original is untouched.
  - New dependency: `encoding_rs` in csv-engine (and as a dev dependency in file-format). UTF-16 is encoded by hand, because `encoding_rs` only decodes it.
  - Limits:
    - UTF-16 without a BOM is detected by the pattern of zero bytes.
    - Malformed UTF-16 (lone surrogates) shows and saves as U+FFFD.
    - For decoded files, "file changed on disk" can't detect changes to the original, because the app reads the cached copy.
    - A Windows-1252 file whose first 64 KiB is plain ASCII is read as UTF-8. That's harmless while later bytes aren't edited, since untouched rows copy through as raw bytes, but such bytes display as U+FFFD.
  - The ENG-3 nasty-corpus test now opens files the same way as the app, and covers all 13 files, including `utf16le.csv`.
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
