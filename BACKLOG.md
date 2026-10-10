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
- [x] M2: editor round-trips edits, paste, inserts, and undo on 1 GB without full rewrites before save
- [x] M3: all V0.1 benchmarks in docs/BENCHMARKS.md pass on reference hardware (V0.1 release)
- [ ] M4: `curl -fsSL <site>/install.sh | sh` installs per user on clean Arch, Ubuntu 24.04, and Fedora VMs, and double-clicking a CSV opens it

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
| EDIT-3 | M2 | P0 | done | CMD-1 | Insert and delete rows | Insert at row 1M in under 50 ms |
| EDIT-4 | M2 | P0 | done | CMD-1 | Insert and delete columns | Works on 1 GB without a full rewrite before save |
| EDIT-5 | M2 | P0 | done | CMD-1, GRID-4 | Delete key clears selected cells | Undoable as one step |
| CLIP-1 | M2 | P0 | done | GRID-4 | Copy/cut range as TSV plus text/html | Pastes cleanly into LibreOffice, Excel web, a text editor |
| CLIP-2 | M2 | P0 | done | CLIP-1, CMD-1 | Paste TSV into a range, expanding as needed | 10k rows from LibreOffice paste in under 1 s |
| SAVE-2 | M2 | P0 | done | SAVE-1, ENG-6 | Save, Save As, New | Save As can change delimiter and encoding |
| SAVE-3 | M2 | P0 | done | SAVE-1, DEC-5 | Background save with progress and cancel | UI responsive during 1 GB save; save under 15 s |
| SAVE-4 | M2 | P1 | done | SAVE-1 | Preserve quoting style and line endings of untouched rows | Saving an unedited file produces an empty diff |
| APP-9 | M2 | P0 | done | SAVE-2 | Ask before closing with unsaved edits | Closing a window with edits offers Save / Don't Save / Cancel; nothing is lost silently |
| ENG-8 | M3 | P0 | done | DEC-5 | Background job framework: progress and cancel | Every long job cancels within 200 ms |
| TYPE-1 | M3 | P0 | done | ENG-2 | Infer column types by sampling | Raw value never altered; `00123` stays `00123` |
| TYPE-2 | M3 | P1 | done | TYPE-1 | Show inferred type in header; allow override | Override changes sort and filter only |
| SORT-1 | M3 | P0 | done | TYPE-1, DEC-4, CMD-1, ENG-8 | Type-aware sort, ascending and descending | 2M-row numeric sort under 3 s; stable; undoable |
| SORT-2 | M3 | P0 | done | SORT-1, ENG-7 | Sort moves whole rows and honors the header | Header row never moves |
| FILT-1 | M3 | P0 | done | TYPE-1, DEC-4 | Per-column filter from header dropdown | Exact, contains, not contains, empty, non-empty, numeric compare |
| FILT-2 | M3 | P0 | done | FILT-1 | Combine filters across columns (AND) | Status bar shows "x of y rows" |
| FILT-3 | M3 | P0 | done | FILT-1, SAVE-1 | Saving with an active filter keeps hidden rows | Test confirms on-disk row count unchanged |
| FILT-4 | M3 | P1 | done | FILT-2, CLIP-2 | Edits and paste respect the filtered view | Paste into a filtered range touches visible rows only |
| SRCH-1 | M3 | P0 | done | ENG-8 | Find bar (Ctrl+F): whole file or current column, case toggle | First hit in 1 GB under 1 s, off the UI thread |
| SRCH-2 | M3 | P0 | done | SRCH-1 | Next/previous result; hit count streams in | New keystroke cancels the running search |
| SRCH-3 | M3 | P0 | done | SRCH-2, CMD-1 | Find and replace; replace all | Replace all on 100k hits undoes as one step |
| APP-4 | M4 | P0 | done | APP-1 | Recent files menu | Last 10 files survive restart |
| APP-5 | M4 | P0 | todo | APP-1 | Drag and drop to open | Works on Wayland and X11 |
| APP-6 | M4 | P0 | todo | DEC-1 | Follow system dark/light theme; high-DPI | No blurry text at 1.5x and 2x |
| APP-7 | M4 | P1 | todo | CMD-2 | Crash recovery via periodic overlay journal | Relaunch after crash offers to restore edits |
| APP-8 | M4 | P1 | todo | ENG-3 | Clear errors for malformed files | Dialog shows line number and offending text |
| PKG-3 | M4 | P0 | done | - | Portable release build: link against Ubuntu 24.04's glibc (build in a container) | The release binary starts on clean Arch, Ubuntu 24.04, and Fedora |
| PKG-1 | M4 | P0 | done | APP-1, PKG-3 | curl installer (per user, no sudo): `install.sh` on the download site; .desktop file, icon, text/csv association; `--uninstall` | Piping `install.sh` from the site into `sh` installs on clean Arch, Ubuntu 24.04, Fedora; checksum verified; double-click opens CSVs; prints the distro's GTK 4 install command if missing |
| PKG-2 | M4 | P2 | todo | PKG-1 | Flatpak manifest | Builds on Flathub CI |

## Notes

Story notes, decisions made mid-story, and follow-ups go here, newest first.

- **2026-10-10 decision:** Flatpak (PKG-2) is deferred; the curl installer is the M4 distribution (user decision). PKG-2 stays P2 and todo, so it doesn't hold the M4 gate.
  - The work is parked, unmerged, on branch `story/pkg-2-flatpak` (ea4b959). Its manifest builds, installs, and runs in Flathub's own CI image (GNOME 51), checked by `scripts/check_flatpak.sh`.
  - Before a submission it needs: an app ID on a domain we control (or `io.github.<user>.<repo>`), a homepage URL, hosted screenshots, and either a Flathub exception for home-folder access or a move to portals.
- **2026-10-10 PKG-1:** curl installer. `curl -fsSL <page>/install.sh | sh` installs or updates for the user, without sudo; `| sh -s -- --uninstall` removes it.
  - `site/install.sh` (POSIX sh, since it is piped into `sh`): reads `latest.json` from the download bucket (`ship.sh site` fills in `DOWNLOAD_BASE_URL`; `SPREADSHEET_DOWNLOAD_URL` overrides it for tests). It downloads with curl or wget, checks the sha256, and installs nothing on a mismatch.
    - The binary goes to `~/.local/bin` by copy and rename, so a running copy isn't written into. The icon (new `packaging/spreadsheet.svg`, now in the tarball) goes to `~/.local/share/icons/hicolor/scalable/apps`.
    - The desktop entry goes to `~/.local/share/applications`, with `Exec=` the absolute path, so a launcher starts it even when `~/.local/bin` isn't on its PATH.
  - `text/csv` and `text/tab-separated-values` defaults go into `~/.config/mimeapps.list` directly (`xdg-mime` isn't always installed).
    - Every other line and section stays. Uninstall removes only the entries naming `spreadsheet.desktop`, so another app's default for CSV survives.
    - Two bugs found while reviewing before the container runs: uninstall dropped every CSV default, and the function reused the `tmp` variable the download's cleanup trap needed.
  - Then it runs `spreadsheet --version`. If the loader can't find GTK 4, it says so with the distro's command from `/etc/os-release` (`pacman -S gtk4`, `apt install libgtk-4-1`, `dnf install gtk4`, `zypper install libgtk-4-1`, else generic) and exits 2. It also says when `~/.local/bin` isn't on PATH.
  - Acceptance: `scripts/check_install.sh` (also run by `ship.sh verify`). A throwaway site on localhost serves the tarball, a tampered copy, and latest.json; Arch, Ubuntu 24.04, and Fedora containers run as a normal user:
    - Without GTK: installs, then prints the right package command; exit 2. With a tampered tarball: "checksum mismatch … nothing installed", and nothing is.
    - With GTK, and another app's `image/png` and `text/csv` defaults already there: installs; `gio mime text/csv` says spreadsheet.desktop.
    - `gio open /tmp/t.csv`, which is what a file manager's double-click calls, starts the app on the file: its recent-files list (APP-4) then holds `/tmp/t.csv`.
    - Reinstall updates. Uninstall leaves no binary or desktop entry and keeps the other app's `image/png` default.
    - All three ok. `VERBOSE=1` and `ONLY=<distro>` help when one fails.
  - Also changed: the download page shows the curl command for its own address, plus the uninstall line, and the AUR sentence is gone. The README has an Install section.
  - The ship-it skill now checks after deploy that the live `install.sh` names the download URL.
  - Not yet proven: the M4 gate asks for VMs with a desktop and a real double-click in a file manager. Containers prove the same path (`gio open` uses the same defaults a file manager does) but not a file manager itself.
- **2026-10-10 PKG-3:** the release binary is built in an Ubuntu 24.04 container and checked on clean Arch, Ubuntu 24.04, and Fedora.
  - `packaging/build.Dockerfile`: Ubuntu 24.04 (glibc 2.39, GTK 4.14, the oldest we support), `libgtk-4-dev`, and stable Rust with the components `rust-toolchain.toml` names.
    - `scripts/ship.sh build` runs cargo in it as the calling user, with its own `CARGO_HOME` and target dir under `target/container`, so host builds are untouched.
    - Preflight now checks for docker. `ship.sh` was mode 644 in git although the ship-it skill runs it directly; it is executable now.
  - Acceptance: `scripts/ship.sh verify` (`scripts/check_release.sh`). For each distro a fresh container installs only GTK 4's runtime from its own packages (`gtk4`; `libgtk-4-1` and `libgtk-4-bin` on Ubuntu). The binary then opens a CSV under Broadway with `--bench-open` and must print its first frame and quit. That loads every linked library and draws a window.
    - It also fails if any glibc symbol is newer than 2.39.
    - Result: Arch, Ubuntu 24.04, and Fedora all ok; the newest glibc symbol is GLIBC_2.34; tarball 873 KB.
  - Finding: a binary built on this Arch host passes the same check today. Rust's std targets old glibc symbols, and gtk-rs's `v4_14` feature keeps to GTK 4.14's API. My replan note said otherwise, and is corrected.
    - The container makes it a guarantee rather than luck: a newer toolchain or a -sys crate can't raise the floor unnoticed, and `verify` would catch it if they did.
  - `--version` added (`spreadsheet 0.0.1`) for the installer and the skill's build check. `packaging/aur/PKGBUILD` removed (no AUR).
- **2026-10-10 APP-4:** recent files: the last 10 files opened or saved, newest first, kept across restarts.
  - `app/src/recent.rs` (no GTK): one absolute path per line in `$XDG_STATE_HOME/spreadsheet/recent` (default `~/.local/state/spreadsheet/recent`), raw bytes, so any file name works except one with a newline.
    - Each change reads the file again first, because every launch is its own process (`NON_UNIQUE`), then writes a temporary file and renames it over the list.
    - Opening a file again moves it to the top, once.
  - Recorded: files opened from the dialog, the list, or the command line, and the target of a Save As. Runs with a `--bench-*` flag are left off, so tests and `cargo bench` don't fill the list with corpus files.
  - UI:
    - The start window lists them under "Recent" (name, then the folder with `~` for home; the full path as tooltip).
    - Each window's header has a ▾ menu next to Open (`app.open-recent` with the path, read again each time it opens), plus "Clear Recent Files".
    - A file that can't be opened shows the error and leaves the list. A path that isn't UTF-8 stays on the start window but can't go in the menu (action targets are strings).
  - Acceptance: `recent::tests::the_last_ten_files_survive_a_restart`. 12 files added, then a fresh load reads the newest 10 in order; opening one again moves it to the top without a duplicate.
    - Also tested: two windows' writes merge; remove and clear; relative and odd names; labels.
  - Smoke test via Broadway with `XDG_STATE_HOME` in /tmp:
    - Launches with alpha.csv, then beta.csv, wrote "beta, alpha". A launch with no file showed both on the start window.
    - Clicking alpha opened it and moved it to the top. The header ▾ listed alpha, beta, and Clear Recent Files.
    - With beta.csv deleted, choosing it showed "Cannot open /tmp/app4/beta.csv: No such file or directory" and left only alpha on the list.
- **2026-10-10 decision:** no AUR (user decision: it is dead). M4 ships a curl installer instead: `curl -fsSL <site>/install.sh | sh` on the Cloudflare download site.
  - It reads `latest.json`, downloads the tarball, and checks its sha256. It installs per user, without sudo: the binary in `~/.local/bin`, the .desktop file and icon in `~/.local/share`, and the text/csv default via `xdg-mime`.
  - Rerunning it updates; `--uninstall` removes it. It can't install GTK 4, so it prints the distro's install command when GTK 4 is missing.
  - The gate needs clean Arch, Ubuntu 24.04, and Fedora VMs (user decision). New story PKG-3 builds the release against Ubuntu 24.04's libraries, so it starts on the oldest system we support. PKG-1 is now the installer and depends on it.
- **2026-10-10 FILT-4:** edits and paste respect the filtered view, and the FILT-1 refusals are gone.
  - Typing, Delete, copy, and paste already reached only the rows shown (FILT-1). This story proves paste and lifts the refusals.
  - Decided with the user:
    - A paste that runs past the last row shown adds rows at the end of the file, as when unfiltered. They pass every filter until it is applied again, so they stay shown.
    - Deleting rows while filtered deletes only the selected rows shown, as one undo step; hidden rows between them stay. Inserting puts empty rows next to the row shown, and they stay shown too. Excel and Calc behave this way.
  - Data model:
    - `CsvTable::insert_rows(row, n)` puts rows before shown row `row`'s position, or at the end of the file when `row == row_count()`.
    - `delete_rows(row, n)` maps the shown rows to positions (`view_ranges`). One range is a plain `Edit::DeleteRows`. Several are the new `Edit::DeleteRowRanges(Vec<Range>)`, whose inverse is `Edit::InsertRowRanges(Vec<(at, runs)>)`.
    - `RowOrder::remove_ranges`/`insert_ranges` take all the ranges in one step: one copy pass for a sorted order (a delete per range would copy the whole permutation each time), one treap remove per range for runs. `Reshape` labels and focuses the new variants.
  - Removed: `PasteError::Filtered`, `ClipView::Filtered` and its status message, `GridView::refused_while_filtered`, and the filtered-only refusal in `insert_rows`/`delete_rows`.
    - The grid's edge row (type below the last row to add one) is back while filtered. `shown_rows` had dropped it; the smoke test found that.
  - **Bug fixed from EDIT-3 (row-map treap):** cutting a run in `split` gave the new right half a fresh random priority. That could outrank the ancestors it lands under, which breaks the heap order. Split after split, the tree became a chain: depth 7,230 for 80,000 runs, quadratic time, then a stack overflow when deleting the 1.36M rows shown in the 1 GB file.
    - The new half now draws a priority at or below the split node's (`node_below`). The same 80,000 runs: depth 47, 9 ms instead of 4.1 s.
    - `the_tree_stays_shallow_through_many_splits` (200,000 runs, depth < 120) would have failed before the fix.
    - `insert_1gb` is unchanged (insert and delete at row 1M in 0.03 ms).
  - Acceptance: `commands/tests/filter_edits.rs` `paste_into_a_filtered_range_touches_visible_rows_only`. A 3×2 paste over Denver rows 1, 3, and 5 changes those rows only; rows 2, 4, and 6 are saved unchanged, and undo restores the file byte for byte.
    - Also tested: a paste past the end adds rows at the end of the file, shown, and one undo removes them.
    - Deleting all shown rows is one step with hidden rows kept, in file order and in a sorted order; undo puts them back in place. Neighbours with nothing hidden between them are a plain `DeleteRows`.
    - Inserting puts rows next to the row shown and at the end.
    - `RowOrder` ranges round-trip against a flat vector.
    - The old FILT-1 assertions that pastes, inserts, and deletes are refused are gone.
  - 1 GB (`commands/tests/filter_1gb.rs` `deleting_the_rows_shown_on_1gb`, ignored): deleting the 1,362,719 rows shown (Portland, revenue > 50,000) takes 352 ms in file order (undo 505 ms) and 95 ms sorted (undo 100 ms), on the UI thread.
  - Smoke test via Broadway on a /tmp copy of the 10 MB file, filtered to Denver (27,431 of 193,311):
    - Rows 3–5 (ids 3, 10, 15, with hidden rows between them) deleted with Ctrl+- (27,428 of 193,308); Ctrl+Z brought them back.
    - Ctrl++ inserted an empty row, shown above id 3.
    - On a fresh copy, typing 999999 into the edge row added a shown row. After Ctrl+S, `diff` showed only `999999,,,,,,,` appended at the end of the file. The filter, applied again on the reopened file, then hid it (city is empty), as filters do.
  - Follow-up: deleting or undoing a million shown rows in file order takes 0.35–0.5 s on the UI thread (one treap operation per range). It could move to a job, or the runs could be rebuilt in one pass like the sorted order.
- **2026-10-09 TYPE-2:** each header shows its column's type, and the user can override it.
  - Decided with the user:
    - The type shows as a dim tag right of the title, left of ▾: 123, 1.5, abc, date, T/F. It is blue when overridden, and narrow columns drop it before the button.
    - The ▾ popover gets a "Type" dropdown: "Automatic (whole numbers)" (the inferred type, named), Text, Number, Date, True/false.
    - In a Number column, detected or overridden, number filter conditions read every cell that parses as a number, zero-padded included: "00999" > 500 passes. In other columns they take only plain numbers, as before.
    - The popover hides the number conditions for columns that aren't numbers. A number filter already on keeps them listed.
  - My calls:
    - Overrides aren't undoable, like filters: they change no data.
    - They stay with their column through header flips and new inferences.
    - A save gives them back by column position, before the filters, since filters read them.
    - Changing a column's type re-applies a number filter on that column. Text filters don't depend on the type.
  - `data_model`:
    - `CsvTable::type_overrides` (by `ColId`) sit apart from the inferred `types`. `column_type(col)` (what sort and filter use) returns the override first, then the inferred type.
    - Accessors: `inferred_type(col)`, `type_override(col)`, `set_type_override(col, Option<InferredType>)`.
    - The filter's `Matcher` takes the column's numberness from `type_of(filter.col)`. Its numeric reading accepts only digits, sign, point, and exponent: no "inf", "NaN", or "1,234".
  - Acceptance: `file-format/tests/type_override.rs`, on a file whose `code` column ("00999", 10, 9, abc, blank) is inferred Text.
    - Sort goes from 00999, 10, 9, abc to 9, 10, 00999, abc with Number. A number column read as Text sorts as text.
    - "> 500" goes from no rows to "00999".
    - Values, edits (none), and the saved file are unchanged (the saved bytes equal the source). Automatic brings the inferred order back.
    - A second test: an override survives a header flip and a new inference.
  - Smoke test via Broadway on a /tmp copy of the 10 MB file:
    - Tags showed on every column. Column H's ▾ showed "Automatic (text)" without number conditions.
    - Number turned the tag blue ("1.5") and listed them. "> (number) 990" showed 1,670 of 193,311 rows, codes 00991–00999.
    - An edit and Ctrl+S kept the blue tag and the 1,670 rows: the filter, applied again after the reopen, needed the override.
    - `diff` against the corpus file showed only the edited line; codes stayed zero-padded.
  - Follow-up: Date and True/false overrides change only the sort, since there are no date or boolean filter conditions yet.
- **2026-10-09 SRCH-3:** find and replace; replace all.
  - Decided with the user: replace all refuses above 2,097,152 cells (`REPLACE_MAX_CELLS`, the paste limit). It names the count: "Too many: 2,725,768 (max 2,097,152)". Every new value and its undo is a cell edit until saved. A compact rule-based store for bigger replaces (ADR 0003's "revisit if") is a follow-up, if wanted.
  - Rules, following the search's:
    - Every match in each matching cell is replaced: "Portland, PORTLAND" → "PDX, PDX" when case is ignored. The replacement goes in as typed, and an empty one deletes the text.
    - Only cells shown are changed: a filter's hidden rows and deleted rows and columns are left alone. Edits count as shown, and raw text stays raw ("00123" with 1 → 9 is "00923").
  - UI: Ctrl+H, or the replace toggle in the find bar, shows a second row with "Replace with", Replace, and Replace All. Ctrl+F leaves the replace row as it was.
    - Replace (or Enter in the field) replaces in the cell under the cursor if it is the match the last find went to, then goes to the next match. Otherwise it first goes to the match at or after the cursor, as Calc does.
    - Replace All runs as a job ("Replacing" in the status bar; Esc cancels) and says "Replaced 19,662 cells". Esc anywhere in the bar closes it.
  - `data_model`:
    - `Matches::replace_all(table, with, cancel, done) -> Result<Option<Edit>, ReplaceError::{TooMany, Stale, Cancelled}>` walks the shown rows for those with matches. It then rebuilds unedited file rows in parallel, one chunk of 65,536 file rows at a time, and rows with edits from their cells as shown. The result is one `Edit::Cells`, which the app runs as a `Batch` ("Replace all"), so it is one undo step.
    - `Matches::replace_in` does one cell.
    - The per-cell match test is shared with counting (`file_row_matches`, `edited_row_cols`), so the cells replaced are exactly the cells counted.
  - Acceptance, `commands/tests/replace.rs` `replace_all_on_100k_hits_undoes_as_one_step`: 100,000 matching cells among 120,000 rows. Replace all is one step (3.5 MB of undo). One undo restores every cell and leaves no edit; redo brings them all back.
    - Other tests: every match in a cell; match case; one column; raw text; quotes as shown; empty replacements; edits, hidden rows, and deleted rows; stale matches, cancel, and single cells.
  - 1 GB (`commands/tests/replace_1gb.rs`, ignored): codes containing "0099" in column H, 210,477 cells.
    - Search 175 ms, building the edit 65 ms (job pool), apply 61 ms, undo 29 ms, redo 29 ms (UI thread). Undo holds 12.3 MB.
    - "portland" is refused with `TooMany(2725768)`.
  - Smoke test via Broadway on the 1 GB file:
    - Ctrl+H, "portland" → "1 of 2,725,768". "PDX" and Enter replaced row 6 and moved to row 14, "1 of 2,725,767".
    - Replace All was refused with the limit note.
    - "00999" → "X0999": Replace All replaced 19,662 cells (55 ms to build, 4 ms to apply) and showed "19,663 unsaved edits". One Ctrl+Z took them back (row 536 read "00999" again, 1 edit left).
    - Esc from the Replace All button closes the bar (fixed during the smoke test: Esc was only handled in the fields).
  - Follow-ups:
    - The UI-thread apply grows with the cell count, as for paste: about 0.6 s near the 2M limit, extrapolated from 61 ms at 210k.
    - Replace recounts the whole file after each cell, since the edit makes the matches stale (about 0.2 s on 1 GB per click). Patching the counts in place would make it instant.
- **2026-10-09 SRCH-2:** search as you type, next/previous, and a count: "12 of 2,725,768".
  - Decided with the user:
    - Typing searches as you type from the cursor, the cursor's own cell first, so a longer text stays on a match that still fits (Firefox-style).
    - The count is of matching cells shown, with the cursor's place among them.
  - Keys and controls:
    - Enter, Ctrl+G, or ↓ goes to the next match; Shift+Enter, Ctrl+Shift+G, or ↑ to the previous one, wrapping at either end.
    - Changing "Match case" or the scope searches again from the cursor.
    - While matches are counted, the note shows "N so far…" from the 100 ms status tick.
  - Acceptance: every keystroke cancels the running search at once (`SearchEntry::changed` → `GridView::cancel_search`); the search for the new text starts when typing pauses (`search-changed`, GTK's 150 ms).
    - `app/tests/find_typing.rs` (ignored, needs a display; Broadway works) runs `--bench-find portland`, which types one character every 200 ms on the 1 GB file. Each search still running at a key came back cancelled, 5.5–10.4 ms after the key (7 of 7), and the last one landed on "1 of 2,725,768".
  - `data_model::search` reworked:
    - `CsvTable::search(query, cancel, &SearchProgress) -> Matches` counts matching cells per file row: one byte a row (19 MB on 1 GB), with counts of 255 or more in a `BTreeMap` and inserted rows in a `HashMap`. Counting replaced SRCH-1's bitmap.
    - `Matches::step(table, from, Step::{Here, Next, Previous}, hint, cancel) -> Hit { row, col, ordinal }`. The place is counted over the rows shown before the hit, or taken from `hint` ±1 when stepping from the previous hit.
    - `SearchProgress.found` streams a provisional count (rows passing the filters; deleted rows and edits settle at the end).
    - `CsvTable::find` is gone.
  - Staleness: `CsvTable` got a `revision`, from a process-wide counter, so a replaced table never repeats one. It changes on every `apply` (edits, undo, sort, inserts) and every `update_view` (header flip, filters). `GridView::found` keeps the last `Matches` while `is_current` holds and the query is the same, so stepping takes 0.05–1.3 ms in the app against 241 ms for counting.
  - **Bug fixed from SRCH-1:** the walk to the next hit called `runs_in` over every remaining row, which on a sorted table builds one `Run` per row (about 450 MB for 19M rows). The walk now goes 65,536 table rows at a time, either way, checking cancel between steps.
    - Sorted 1 GB: first match 335 ms; previous from the top, which counts every row shown, 512 ms.
  - Tests: `commands/tests/search.rs`, 9 tests, now also covering:
    - the place and total on every match of every tour, counted fresh and from the hint;
    - previous with wrap, and `Here`;
    - staleness after an edit, undo, header flip, filter, or another table;
    - deleted rows, and the streamed count.
    - `data-model/tests/search_1gb.rs` adds totals, previous, and a sorted table.
  - Smoke test via Broadway on the 1 GB file:
    - Typing "portland" went to row 6, "1 of 2,725,768"; Enter twice went to row 19, "3 of".
    - Shift+Enter three times went to 2, then 1, then wrapped to row 19,094,577, "2,725,768 of 2,725,768".
    - Editing that cell to "Paris" and pressing Enter again recounted: "1 of 2,725,767".
    - The note has a fixed width, so the field no longer jumps as the count grows.
  - Follow-up: in a sorted or filtered table, the first place after a jump costs a walk over the rows before it (up to 0.5 s on a sorted 1 GB), on the job pool.
- **2026-10-09 SRCH-1:** find bar (Ctrl+F) over the status bar: text field, "Match case", "Whole file" or "Current column", a "No matches" note, close button. Enter finds the next match from the cursor, and Esc closes the bar (and cancels a running search).
  - Rules, decided with the user: a match is the text anywhere in a cell's value as shown (edits included, quotes unescaped; no whole-cell option), row by row from the cursor, wrapping to the top, with the cursor's own cell last; the bar goes at the bottom.
    - "Shown" also means a filter's hidden rows and deleted columns are skipped, and the order is the sorted order when sorted.
    - Case folding is ASCII-only, as for sort and filter.
  - `data_model::search` (`CsvTable::find(query, from, cancel, done)`, `Query`, `SearchError`) is one parallel pass over the file that marks every row with a match in a bitmap. Per 65,536-row chunk, `memmem` over the raw bytes (lower-cased first when case is ignored) picks the rows worth splitting into fields. A query containing the quote character skips that filter and splits every row, since quotes are doubled in the file. Rows with edits are checked from their cells as shown, and inserted rows from the overlay. A walk through the shown rows (runs of the row order, the filter's ranges) from the cursor then finds the first marked row, and the cell in it.
  - The pass always reads the whole file, so the first hit costs the same wherever it is: 0.17–0.21 s on 1 GB (`data-model/tests/search_1gb.rs`, ignored), and 167–209 ms in the app. A search that stops at the first hit would be faster near the cursor, but would need the scan in view order (slow when sorted: random reads). SRCH-2's streaming hit count needs the full pass anyway.
  - The app: `GridView::find_next(text, match_case, column_only, done)` runs the job (`searching`, counted in `busy`, `cancel_job`, and `job_progress` as "Searching"), and on a hit moves the cursor and scrolls to it. Editing and other jobs wait, as for sort and filter. A search before indexing finishes says "Wait for indexing to finish".
  - Tests: `commands/tests/search.rs` (order and wrap, case, one column, edits/undo/inserted rows/deleted columns, sorted and filtered views, cancel); `jobs/tests/cancel_1gb.rs` `searching_cancels_within_200ms` (at most 8 ms).
  - Smoke test via Broadway on the 1 GB file: Ctrl+F opened the bar; "Zanzibar" said "No matches"; "portland" went to row 6, then Enter to row 14; "19094000" jumped to row 19,094,001; Esc closed the bar.
  - Follow-ups: the bitmap is thrown away after each search, so Enter repeats the whole pass (SRCH-2 can keep it per query and table generation). There is no Shift+Enter for the previous match yet (SRCH-2).
- **2026-10-09 FILT-3:** saving with an active filter keeps hidden rows. The save always wrote every row (it walks the row order, never the view: ADR 0004, invariant 7); this story proves it through the real atomic save, and keeps the filters across the save.
  - `file-format/tests/save_filtered.rs`:
    - A small file, filtered to 2 of 5 rows, with a shown row edited and the whole table sorted while filtered. `save_csv` writes all 5 rows in the sorted order with the edit, and the reopened file has 5 rows.
    - Ignored, 1 GB: filtered to 1,362,719 of 19,094,579 rows, one shown row edited, saved atomically. The reopened file has every row (19,094,580 lines with the header) and matches the corpus line for line except the edited one, which stays in place.
  - **Gap closed:** a save reopens the file as a new table, which dropped the filters, so the view jumped back to every row. `finish_save` now takes the filters by column position (`GridView::filters`) and gives them back (`restore_filters`). They are applied again in one job once the reopened file is indexed (`apply_pending_filters`, from the status tick next to type inference). The saved file's columns are the table's columns in order, so positions carry over, Save As included. A delimiter change still drops them, since its columns differ.
  - Being applied again means being evaluated again: a row edited so it no longer matches is hidden after the save, as with Apply.
  - `GridView::apply_filter` became `apply_filters`, one job for several filters.
  - Smoke test via Broadway on a /tmp copy of the 1 GB file:
    - City ▾ `portland` (2,725,768 rows), then `Edited` typed into B1, then Ctrl+S: `Saved in 0.8 s`.
    - The reopened table was indexed, re-inferred, and filtered again in 168 ms: `2,725,768 of 19,094,579 rows`, with C's ▾ highlighted.
    - On disk: 19,094,580 lines, `5,Edited,Portland,…` on line 7, every other line identical to the corpus.

- **2026-10-09 FILT-2:** combine filters across columns (AND), with "x of y rows" in the status bar. Both were built in FILT-1; this story proves them and fixes one bug.
  - Proof:
    - `status::tests::filtered_rows_read_x_of_y`: the spec's own example (`14,392 of 2,103,814 rows`), `0 of 19,094,579 rows`, `1 of 1 row`, and plain `1 row` with no filter.
    - `commands/tests/filter.rs::filters_combine_in_any_order_down_to_no_rows`: three filters narrow 6 rows to 3, 2, and 1, with `row_count` of `unfiltered_row_count` (what the bar shows) checked at each step. The same filters in the other order show the same row.
    - A fourth filter shows nothing: no rows to read, clear, or copy, and the save is the file byte for byte. Clearing filters one at a time brings back what the rest allow.
  - **Bug found and fixed:** a filter that hid every row also hid every column but A. The grid's column count comes from the rows it has loaded, so the ▾ needed to clear the filter was gone. The only way out was reopening the file. `CsvTable::min_width` (the header row's width, edits and inserted columns included, and every filtered column) is now a floor for `GridView::col_count`, used for drawing, navigation, scrolling, and column deletes.
  - Typing into the cell where no row is shown now gives the filter note instead of being dropped without one. With no rows, no editor opens, so nothing is lost.
  - Smoke test via Broadway on the 1 GB file: city equals `nowhere` showed `0 of 19,094,579 rows` with all eight columns and their ▾, C highlighted. Typing did nothing, with no crash. C's ▾ reopened with `nowhere`, and Clear brought back `19,094,579 rows`.

- **2026-10-09 FILT-1:** per-column filter from a header dropdown. Four user decisions: a ▾ on every column title (highlighted when that column is filtered); text conditions ignore case; a filter is evaluated when applied, so an edited row stays shown until it is applied again; and row inserts and deletes are refused while filtered, with a status note.
  - `data_model::Filter { col: ColId, test: Test }`, where `Test` is `Equals` (whole cell), `Contains`, `NotContains`, `Empty`, `NonEmpty`, or `Number(Compare, f64)` with `=` `≠` `<` `≤` `>` `≥`. Number tests take only cells TYPE-1 reads as numbers (`00123` is a code, not 123); any other cell fails them, `≠` included.
  - `CsvTable::filter_rows` evaluates a filter into a `RowSet` (a bitmap over file rows, plus the inserted rows that failed) on a snapshot. `set_filter` replaces any filter on that column and combines the rest with AND; `clear_filter`, `filter_on`, and `is_filtered` round it out. The table keeps the positions shown (ADR 0004 "As built"): reads, edits, clears, copies, and pastes reach shown rows only, while save, sort, and inference see every row. The header row is never filtered out; with the header off, the first row is data and is tested like the rest.
  - Refused while filtered: `insert_rows` and `delete_rows` (`None`), and a paste that would add rows (`PasteError::Filtered`). The status says "Clear the filters to add or delete rows", and the editable edge row is hidden.
  - App: each column header's ▾ opens a popover (`Show rows where column C`, a condition list, a value, Clear when filtered, Apply or Enter). The value field flags a number condition whose value is not a number. Applying runs as a job (`Filtering N%`, Esc cancels, edits wait). The status shows `x of y rows`. A save or delimiter change opens a new table, which drops the filters.
  - **Bug found and fixed in the smoke test:** closing the popover from Apply panicked with "RefCell already borrowed", because its `closed` handler takes it out of the cell that `popdown` was called through. `GridView::close_filter_menu` now clones the popover out first.
  - **Speed fixed:** rebuilding the view after a sort of a filtered 19.1M-row table took 183–207 ms on the UI thread. Folding the filters into one bitmap and visiting set bits takes it to 47 ms.
  - Proof:
    - `commands/tests/filter.rs`:
      - Every condition on a small file, with the header always kept.
      - Two columns AND; a new filter on a column replaces its old one.
      - An edit, a copy (`2\tboston\n6\tBoston`), a paste, and a clear each reach shown rows only, and the save writes every row.
      - An edited row stays until the filter is reapplied.
      - A sort while filtered sorts hidden rows too, keeps the filter, and undoes.
      - The header row is tested as data once the header is off.
      - Insert, delete, and a growing paste are refused, and a cancel returns nothing.
    - Unit tests cover case folding, substring edges, and every number comparison against codes, blanks, and words.
    - `commands/tests/filter_1gb.rs` (ignored): city = Portland, then revenue > 50,000, on 19.1M rows in 0.37 s (8 cores: 0.78 s; target 3 s). The 2,725,768 and 1,362,719 rows shown match a plain scan of the file. The view after a sort rebuilds in 47 ms.
    - `jobs/tests/cancel_1gb.rs`: filter cancels land within 8 ms.
  - Smoke test via Broadway on the 1 GB file:
    - City ▾ → `portland` → Enter: `2,725,768 of 19,094,579 rows`, all Portland, with C's ▾ highlighted.
    - Revenue ▾ → `> (number)` 50000: `1,362,719 of 19,094,579 rows`.
    - Right-click → Delete Rows: refused with the note, rows unchanged.
    - City ▾ reopens prefilled; Clear: `9,545,622 of 19,094,579 rows`, with C's ▾ dim again.
  - FILT-2's AND across columns, and its "x of y rows" status, are built and asserted here, so FILT-2 needs its own proof only.
  - Follow-ups (not fixed):
    - A save or delimiter change drops the filters, because the reopened table starts with none. FILT-3 (save with an active filter) can keep them.
    - Row numbers in the gutter count shown rows (1, 2, 3…), not file rows.
    - The 47 ms view rebuild after a sort or its undo at 19.1M rows is a one-frame hitch on the UI thread.

- **2026-10-09 SORT-2:** sort moves whole rows and honors the header. The sort already kept the header row in place (SORT-1); this story proves it and closes one gap.
  - `commands/tests/sort_header.rs`:
    - With the header on, sorting by a column whose title (`name`) sorts mid-column leaves the title on top in both directions, in the grid and as the saved file's first line. An edited title stays the title through a sort.
    - With the header off, the first row is data and sorts among the rest (it saves as line 3).
    - Every part of a row moves with it: its source fields, an edit, a cleared cell, a value in an inserted column, a ragged row's extra fields, a short row's missing ones, and a quoted cell spanning two lines. The grid and the saved bytes are checked.
  - **Fixed:** flipping "Header row" while a sort ran applied a result taken with the other setting. If the header was off when the sort started and on when it finished, a data row became the header. `GridView::set_header` now cancels the running sort, and its result is dropped (`BRICKS_TIMINGS` logs `(dropped)`).
  - Smoke test via Broadway on the 1 GB file: header off, Sort Descending on score, header on 250 ms later. The log shows `Err(Cancelled) ... (dropped)`, the grid shows the real titles over rows in file order, and there are no unsaved changes.

- **2026-10-09 SORT-1:** type-aware sort, ascending and descending. Three user decisions: text ignores ASCII case, with exact bytes breaking ties; empty cells are always last, while values that don't fit the column type (`n/a` in numbers) follow the typed ones ascending and lead descending; and the entries are "Sort Ascending" and "Sort Descending" in the right-click menu, sorting by the cursor's column.
  - `CsvTable::sort_rows(col, order, cancel, done)` returns one `Edit::Reorder` (or `Ok(None)` if the rows are already in that order); `commands::Batch` makes it one undo step. Keys follow TYPE-1's column type: numbers by value (as f64), dates and date-times by instant (offsets applied), `false` before `true`, text as above. Ties keep the current order, so the sort is stable, and repeated sorts compose. The header row never moves.
  - Row order is now `RowOrder::Runs` (the treap) or `RowOrder::Sorted` (4 B/row). Inserts, deletes, edits, clears, and save all work on a sorted order. A sorted order counts as one unsaved change, and sorting back to file order clears it. ADR 0004 has the as-built details (key layout, file-order key pass, the save's row-start table, cancel by unwinding).
  - App: `GridView::sort` runs on the job pool from a snapshot. The status shows `Sorting N%`, Esc cancels, and edits, undo, and Save wait while it runs (`GridView::busy`). A reopened table cancels a running sort. `BRICKS_TIMINGS` logs each result.
  - Proof:
    - `commands/tests/sort.rs`:
      - Numbers with misfits and blanks, both directions, with ties in file order.
      - Case-insensitive text, including `alexandra`/`Alexandria`, which tie on their first 8 bytes. A mutation that skipped the whole-value compare failed this test.
      - Sorting by name then by score keeps names in order within a score.
      - Undo restores file order with 0 changes and an identical save; redo sorts again.
      - A sorted save writes whole rows, a multi-line quoted cell included, in the new order; an edit, an insert, and a delete on the sorted order save too.
      - Already in order returns `None`, and sorting back to file order leaves 0 changes.
      - A cancel returns nothing.
    - Unit tests cover the key order for numbers, text, date-times with offsets, and booleans.
    - `commands/tests/sort_1gb.rs` (ignored, release): 2M-row numeric sort in 144 ms (8 cores: 170 ms; target 3 s), ascending, stable for equal values, every row once, and undo restores the order. 19.1M rows: numeric 0.90 s, then a text re-sort of the sorted table 0.89 s, then a save in sorted order in 3.7 s with every line checked.
    - `jobs/tests/cancel_1gb.rs` gains the sort: cancels at 0/10/50/90% of the key pass and inside the sort itself take 0/15/29/24/2 ms on 8 cores.
  - Smoke test via Broadway on a /tmp copy of the 1 GB file:
    - Right-click → Sort Descending on score: 0.86 s, then `1000.00` rows on top with ids ascending, • in the title, and `1 unsaved edit`.
    - Ctrl+Z restored file order and cleared the •.
    - Sort Ascending on city with Esc 200 ms in was cancelled, with the order unchanged.
    - Sort Descending again and Ctrl+S: saved in 3.6 s, reopened, and re-inferred. On disk: the header, then 19,094,579 rows with 0 order violations, the same multiset of lines as the corpus.
  - SORT-2's criterion (the header row never moves) already holds and is asserted in `sort.rs`. Rows move whole by construction.
  - Follow-ups (not fixed):
    - Work that walks a sorted table in order reads rows at random: copying the whole city column takes 15.3 s sorted, against 3.6 s in file order. It is a background job with progress and cancel, so the window stays responsive. Paste and Delete over huge sorted ranges are in the same position.
    - `CsvTable::row_of` is O(rows) on a sorted order; undo and redo use it to jump to the changed row.
    - Case folding is ASCII-only (`É` and `é` sort by bytes). Integers beyond 2^53 compare as f64.
    - A sort of an already-sorted 19M-row table holds 76 MB of undo, which drops older history past the 50 MB budget.

- **2026-10-09 TYPE-1:** infer column types by sampling. Three user decisions: integers with a leading zero (`00123`, `000`) are Text; a column has a type only if every non-empty sampled value has it (empty cells and the null markers `NA`, `N/A`, `null`, `-` don't count); and only plain formats count (`-12`, `+3`, `0.5`, `1e6`; ISO dates; ISO date-times with `T` or a space, optional seconds, fraction, and `Z`/`±HH:MM`; `true`/`false`/`yes`/`no` in any case).
  - `data_model::value_type(raw)` classifies one value. A column folds its values: Integer with Decimal makes Decimal, Date with DateTime makes DateTime, anything else mixed makes Text, and a column with no values is Text.
  - `CsvTable::infer_types(cancel)` samples the first 1,000 rows and 1,000 more at even steps to the last row, reading cells as shown (edits included). It returns `ColumnTypes` (`(ColId, InferredType)` pairs), or `None` once cancelled. `set_types` keeps them on the table by column identity, and `column_type(col)` reads them (`None` before inference and for inserted columns). Flipping the header clears them. Types never touch a value (invariant 1).
  - App: `GridView::infer_types` runs it as a job (ENG-8) on a snapshot when indexing completes. That includes a table reopened after a save or a delimiter change, through the status tick's generation check. A header flip runs it again, and a new table cancels a running one. Edits don't re-infer until the file is saved and reopened; TYPE-2's override covers the rest. Nothing shows the types yet (TYPE-2, SORT-1, FILT-1 use them). `BRICKS_TIMINGS` logs each result.
  - Proof: `data-model/tests/infer_types.rs`:
    - Ten columns get their expected types: zip and code codes are Text, a leap day is a Date, a date among date-times gives DateTime, `NA` and empty cells are ignored, and one word among numbers makes Text.
    - Every cell still reads as its source text (`00123` included), and an unedited save is byte-identical.
    - The spread sample reaches the last row.
    - A header flip clears the types, and re-inferring counts the title row.
    - A cancel returns nothing.
    - Unit tests cover 44 single values (signs, exponents, `00.5`, `.5`, `1,234`, invalid dates and times, offsets).
  - 1 GB: `data-model/tests/infer_1gb.rs` (ignored) finds the corpus types (Integer, Text, Text, Integer, Decimal, Date, Boolean, Text; `code` is `%05d`, so Text) in 1.4 ms from 2,000 rows. In the app, 1.7 ms after indexing.
  - Real-world survey of the 50 `corpus/realworld` and 13 `corpus/nasty` files (throwaway, deleted). Census FIPS codes (`01`, `00161526`) come out Text, titanic `True`/`no` Boolean, French fuel prices DateTime and Decimal, wine data Decimal. `comma-nyt-us-states.csv`'s date column is Text only because the 256 KiB sample ends mid-row (`2020-07-2`), and the spread sample includes the last row.
  - Smoke test via Broadway:
    - 1 GB: types inferred right after indexing. Header off re-inferred every column as Text (the title row is data); header on restored them.
    - 10 MB copy: typed `abc` into a revenue cell and saved. The reopened table re-inferred revenue as Text, and the file differs from the corpus only on that line (`00517` intact).

- **2026-10-09 ENG-8:** background job framework with progress and cancel: the new `jobs` crate (ADR 0005, "As built" added there).
  - `jobs::spawn(work)` runs `work(&CancelToken)` on one shared Rayon pool (`available_parallelism() - 1` threads) and returns a `Job<T>`. The app awaits `Job::finished()` with `glib::spawn_future_local`. A job cancelled while still queued never runs and reports `Stopped::Cancelled` at once; a panic reports `Stopped::Panicked`. Progress stays as each job's own atomics, read on the 100 ms status tick; only the result goes over the channel.
  - Moved onto it: indexing (was its own thread), save (its own thread, polled with `try_recv` every tick; now awaited), and copy (was `gio::spawn_blocking`). The save's cancel flag left `SaveProgress`: `SaveJob::write_to`, `save_csv`, and `save_atomic` now take `cancel: &AtomicBool`, as `build` and `copy_range` already did.
  - **Fixed:** cancelling during the save's fsync waited for the disk (136 ms for 1 GB on NVMe here; seconds on a slow disk). The fsync now runs on a helper thread, and the save stops waiting once cancel is set. The temp file is removed as before, and the helper finishes syncing the unlinked file. That cancel now takes 2.8 ms.
  - **Fixed (regression risk from the cutover):** with the save's finish no longer inside the status tick, a reopened file that indexes within one tick would have kept the scroll range of its first few rows (the SAVE-2 bug). `GridView::generation` counts `replace_table` calls, and the tick treats a new table as not yet complete. That also covers the delimiter-change path, which had the same pattern.
  - The workspace `rust-version` went from 1.75 to 1.92: the GTK crates already need 1.92, and rayon needs 1.80. With the real MSRV, clippy's `manual_is_multiple_of` flagged two spots (`commands/src/edit.rs` test, `app/src/status.rs`), now fixed.
  - Proof: `crates/jobs/tests/cancel_1gb.rs` (ignored, corpus, release) cancels each job on the 1 GB file and times `cancel()` until the result is back (target 200 ms):
    - Index at 0/10/50/90%: 0.0/0.6/0.5/0.4 ms.
    - Save (on disk under `target/`) at 10/50/90% written, during the fsync, and during verify: 0.7/0.7/0.7/2.8/32 ms. The original is unchanged and no temp file is left.
    - Copy of every row at 0/10/50/90%: 0.0/1.9/3.7/6.2 ms.
  - Unit tests in `jobs`: a running job returns its own result soon after cancel; a job queued behind a full pool reports at once and never runs; a panic is reported and the pool keeps working.
  - Save timing unchanged: `save_perf` 0.90–0.95 s over 3 runs (0.84–0.99 s before); `save_responsive` 0.77 s, main loop p99 1.1 ms late. All ignored release tests pass, except `status_stream` with a warm page cache (see the M2 gate note); it passes cold (4 updates).
  - Smoke test via Broadway on /tmp copies:
    - 1 GB: an edit, Ctrl+S, Esc shows `Save cancelled · 1 unsaved edit`, and `cmp` matches the corpus. Ctrl+S then saved in 0.6 s, and Ctrl+End reached row 19,094,579.
    - A whole-column copy (`Copying 3%`) replaced by a one-cell copy: Ctrl+V pasted the one cell.
    - Closing with 1 unsaved edit → Save: saved, closed, and exited 0, with the edit on disk.
    - 10 MB: an edit and save (reindexed within one tick), then Ctrl+End reached row 193,311 with a full-width gutter.
    - Broadway note: the headless browser tab freezes between tool calls, so its frames go stale; reload the page before a screenshot.
  - Follow-ups (not fixed):
    - Non-UTF-8 files are decoded in full (`csv_engine::open_text_as` → `transcode`) inside `CsvTable::open`, synchronously on the UI thread and with no cancel: on open, on a delimiter change, and when the file is reopened after a save. A big UTF-16 or Windows-1252 file blocks the window for the whole decode (invariant 4). It should become a job, with the window showing before it finishes.
    - Pool size is untuned (ADR 0005 asked for ENG-8 to tune it). No job is data-parallel yet, so there is nothing to measure. Tune it with the first `par_iter`/`par_sort` job (SORT-1 or SRCH-1).

- **2026-10-09 M2 gate:** approved by the user on this machine's numbers (Ryzen 9 5900, warm page cache).
  - Combined check (throwaway, deleted) on a copy of the 1 GB file: 1,000 edits (0.35 ms in total), a 100-row insert at row 5M, a 10k×3 paste at row 10M (6.8 ms; undo + redo 1.7 ms), a paste that added 3 rows at the end, and a column insert. Three steps were then undone.
  - Before saving: the process had written 0 bytes, the file's size and mtime were unchanged, and anonymous memory had grown by 7.4 MiB. The atomic save took 2.7 s. After reopening, all 19,094,682 data rows matched the editor's view with 0 mismatches, and none of the undone steps reached the file.
  - All ignored release tests pass, except `app/tests/status_stream.rs` (APP-2) when the 1 GB file is already in page cache. It indexes in 0.36 s, before a second 100 ms status update can fire. It passes cold (3 updates). Follow-up: make it independent of the page cache, or evict the file first.

- **2026-10-08 APP-9 (new story):** closing a window with unsaved edits asks first: Save / Don't Save / Cancel, in a `gtk::AlertDialog` (`Save changes to “name”?`, `N unsaved edits will be lost if you close without saving.`). This is a tester-safety story added for the v0.0.1 tester build.
  - `close-request` commits an open cell editor first, so typed text counts. With no edits the window closes as before. While a save runs, closing sets `close_after` on it and waits: the window closes when that save succeeds. A failed or cancelled save keeps the window and its edits.
  - Save goes through `start_save(.., close_after: true)`, or Save As for an untitled table (`save_as`/`choose_save_file` pass `close_after` through). A dismissed format dialog or file chooser starts nothing and closes nothing. Don't Save sets a flag and closes again.
  - No app-level Quit action exists, so `close-request` covers the titlebar button, Alt+F4 and compositor close.
  - Proved by smoke tests via Broadway, not by a unit test: the logic is all dialog wiring. On a /tmp copy of the 10 MB file, a cell typed but not committed, then closed, shows the dialog with `1 unsaved edit`. Cancel keeps the window. Don't Save exits, and `cmp` matches the corpus file. Save exits after saving, with the edit on line 7. On an untitled table, Save opens the format dialog and file chooser: dismissing the chooser keeps the window, and saving `untitled1` writes the file and exits. On a 1 GB copy, closing at `Saving 30%` shows no dialog and exits once the save is done, with all 19,094,580 lines and the edit.
  - Follow-up (not fixed): a file that another program is still writing when it is opened shows only the bytes present at open, and saving writes only those. `Source::changed` catches files that shrink, not ones that grow. Found when a smoke test launched the app while `cp` of the 1 GB file was still running: 3,466,742 rows, reported as complete.

- **2026-10-08 SAVE-4:** untouched rows keep their quoting and line endings. Nothing to fix: the writer already copies untouched rows and fields as raw bytes (SAVE-1, EDIT-4); this story proves it.
  - `file-format/tests/untouched_rows.rs` (ignored; needs `corpus/nasty` and `corpus/realworld`): for each of the 63 files, an unedited save is byte-identical; then (55 files with 2+ rows) one edit to the middle row's first cell changes only that row's physical lines in the file decoded with its own encoding. Covers BOM, CRLF, a last row without a newline, quoted newlines, ragged rows, Latin-1, UTF-16 LE, and the 256 KiB real-world samples that end mid-row.
  - Smoke test via Broadway on a /tmp copy of `comma-population.csv` (CRLF, quoted fields): typed `999` into E4 and saved. `diff` shows one line, `Aruba,ABW,1963,57002,999\r`.

- **2026-10-08 SAVE-3:** background save with progress and cancel. The save already ran on a thread (SAVE-1); this adds progress, cancel, and the proof.
  - `data_model::SaveProgress` (cancel flag, bytes written, checking flag) is shared between the save thread and the UI. `SaveJob::write_to` takes it: every `put` checks the flag first and counts bytes after. Untouched rows are copied in 4 MiB pieces (`COPY_CHUNK`) so a cancel is seen at least that often. `file_format::save_csv` takes it too, passes the flag to the verify re-index, and maps any error after a cancel to `SaveError::Cancelled`; `save_atomic` still removes the temp file.
  - App: `SaveState::Saving(Running)` holds the receiver, progress, size estimate (source bytes), and `close_after` (for APP-9). Status shows `Saving N%`, `Checking the saved file…`, then `Saved in X s`; a cancel shows `Save cancelled · N unsaved edits` and keeps the edits. A flat Cancel button sits next to the save status while saving; Esc cancels too.
  - `--bench-save EDITS` pastes EDITS cells, starts the save on the next frame, scrolls 37 px a frame while it runs, and prints `SAVE {json}` with save time, progress samples, 10 ms timer lateness, and frame stats. It refuses files outside the temp directory. `frame_stats` is shared with `--bench-scroll`'s report.
  - Proof: `save_responsive` on the Hyprland desktop: 1 GB with 1,000 edits saved in 0.84 s (tmpfs), 38 progress samples, main loop late p50 0.06 / p99 1.65 / max 1.70 ms, 60.6 fps with no missed frames. `save_cancel`: a cancel at 200 MiB written takes effect in 21 ms, the file is unchanged, no temp file. `save_perf` (NVMe): 0.99 s. Under Broadway the lateness p99 is ~42 ms, which is Broadway's own paint time (a plain `--bench-scroll` shows the same 46 ms per frame), so that assertion needs a real compositor.
  - **Stall found and fixed:** after every save, `GridView::replace_table` dropped the old table on the UI thread; unmapping 1 GB and freeing its index blocked the UI for 196 ms. The old table is now dropped on a worker thread.

- **2026-10-08 SAVE-2:** Save, Save As, New. Two user decisions: New gets an editable edge (one empty row and column past the data where typing grows the table), and Save As picks the format in a small app dialog before the desktop's file chooser.
  - `SaveJob::with_format(delimiter, encoding)`. The job keeps the source dialect for parsing and an output dialect for writing.
    - With another delimiter, every row is re-assembled (the EDIT-4 path, `reassemble_all`): each untouched field is decoded with the source quote and re-encoded with minimal quoting for the new delimiter. So `"Smith, Ann"` loses its quotes in TSV, and `x;y` gains them in semicolon output. Each row keeps its own line ending.
    - The UTF-8 BOM is now written from the target encoding, not from whether the source bytes started with one. Other encodings get theirs from `EncodeWriter` as before.
    - `CsvTable::new` (code-built tables) now takes a UTF-8 BOM in the bytes to mean UTF-8 with BOM, so the older save tests keep their bytes.
  - Untitled: `Source::empty()` and `CsvTable::untitled()` (comma, LF, UTF-8, no header, already indexed).
  - `CsvTable::open_as(path, choice, encoding)` reopens with the encoding a Save As wrote. Without it, a Windows-1252 file that is pure ASCII would reopen as UTF-8, and the next Ctrl+S would write UTF-8.
  - Acceptance: `file-format/tests/save_as.rs`.
    - The table is a BOM'd UTF-8 CRLF file with a quoted comma, escaped quotes, `x;y|z`, a tab, `Zoë`, an edit, an inserted row, and a cleared cell.
    - It is saved as tab/UTF-16 LE, semicolon/Windows-1252, pipe/UTF-8, and comma/UTF-8 with BOM. Each gives exact expected text and the right BOM, and reopens with that delimiter and encoding detected and the same cells.
    - The source file is untouched. `東京` to Windows-1252 fails the save, naming the character, and leaves no file.
  - Edge (grid/app):
    - `bounds()` and the viewports show one more column than the widest row seen, and one more row once indexing is complete.
    - Committing a cell on the edge row goes through `CsvTable::paste` (adds the row) as a `Batch` named "Edit cell", so it is one undo step.
    - `GridCache::widen` counts the new column before rows reload, so Tab can move past it at once.
    - Clear, delete rows, and delete columns clamp to the data.
    - End and Ctrl+End stop at the last data cell, not the edge.
    - Once indexing is done, a cursor left below a reopened file's rows (e.g. a header newly detected) moves back in.
  - App: `Session.path` is `Option` (untitled) and `Session.encoding` remembers a Save As encoding. The window registry holds sessions, so "already open" follows Save As, and Save As onto a file open in another window is refused with an alert. Title and delimiter dropdown follow the session each tick; `SaveState::Saving` carries the Save As target, which `finish_save` adopts before reopening. Header bar: Open, New, Save As.
  - **Fixed the EDIT-5 scrolling bug** (scroll range and gutter stuck after a fast-reindexing save): the status tick now runs `finish_save` before reading `is_complete`. Seen working in the smoke test: after Save As of the 10 MB file, Ctrl+End scrolled to row 193,312 with a full-width gutter.
  - Smoke test via Broadway (private D-Bus, `GDK_DEBUG=no-portals`, desktop display variables unset):
    - Started without a file and pressed New. Typed `name`/`city`, `Ann`/`Paris, FR`, `Bo`/`00123` into the edge.
    - Ctrl+S opened the format dialog; picked Tab + UTF-16 LE. GTK's own chooser offered `Untitled.tsv` in /tmp; saved as `save2-new.tsv`. The file starts `FF FE` and decodes to `name\tcity\nAnn\tParis, FR\nBo\t00123\n`. The window became "save2-new.tsv · Tab · UTF-16 LE" with the header detected.
    - On a /tmp copy of the 10 MB file, typed into the edge row below the last row (`appended`, `x;y`), then Save As semicolon + Windows-1252. The output has 193,313 lines, the original 193,312 identical cell for cell (`;` for `,`), and ends `appended;"x;y";;;;;;`. The source is untouched.
    - A later plain Ctrl+S on that file kept semicolons.
  - **Process slip:** the first smoke run used only a private D-Bus session. The session still activated a FileChooser portal, which opened a "Save As" dialog on the user's real desktop (workspace 2) for about a minute until I closed it. Smoke runs that open file dialogs now also set `GDK_DEBUG=no-portals` and unset `WAYLAND_DISPLAY`, `DISPLAY`, and `HYPRLAND_INSTANCE_SIGNATURE`; the reruns put nothing on the desktop (checked with `hyprctl clients`).
  - Follow-ups: an encoding chosen by Save As lasts for the window; a later fresh open re-detects (APP-4, recent files). Closing a window with unsaved edits still doesn't ask (APP-7). Typing non-ASCII through Broadway's keyboard injection doesn't arrive, so non-ASCII was covered by the Save As test, not the smoke test.
- **2026-10-07 CLIP-2:** paste TSV into a range, expanding as needed.
  - `data_model::parse_tsv`: rows end at `\n`, `\r\n`, or `\r`, and cells split at tabs. A cell starting with `"` is quoted with `""` escapes and may hold tabs and line breaks; a quote that doesn't close properly before a tab or line break is kept as text. A final line break doesn't add an empty row. `"\n"` is one empty cell (what Calc copies for one empty cell); `""` is nothing.
  - `CsvTable::paste(row, col, cells) -> Result<Vec<Edit>, PasteError>`:
    - If the paste runs past the last row, it adds rows with `insert_rows` (`NotIndexed` until indexing finishes), and cells in those rows are addressed by the new row ids.
    - Then one new `Edit::Cells(Vec<(CellRef, Option<Box<str>>)>)` sets every cell, raw. Its inverse holds the previous overlay values in reverse order.
    - Cells that already show the text are skipped, compared through full-text `read_rows` 4,096 rows at a time; so are empty values where a row has no such cell, so short rows don't grow trailing delimiters for nothing.
    - `PASTE_MAX_CELLS` = 2^21, roughly 100 MB of overlay, returns `TooLarge`.
  - `commands::Batch` runs several edits as one command (one undo step): it applies them in order and keeps the inverses reversed. This is the compound command CMD-1's note said would come with its first user.
  - App: Ctrl+V or Shift+Insert reads the clipboard's text (`read_text_future`) and pastes at the selection's top-left, then selects the pasted range, as Calc does. The status bar says "Pasted N cells", "Paste too large: …", or "Paste past the last row once indexing finishes" for 4 s; `CopyView` became `ClipView`. Paste is refused during a save or while the cell editor is open. HTML on the clipboard isn't read: LibreOffice, Excel, and this app all offer TSV text.
  - Acceptance: `commands/tests/paste_from_libreoffice.rs` (ignored; needs LibreOffice, the corpus, and release mode).
    - Headless Calc opens the 10 MB corpus file and copies A2:H10001; its clipboard text is 492 KB of TSV, with Calc's own number display, e.g. `TRUE` and `692` (`scripts/lo_copy.py`).
    - It is pasted at row 186,622, so 6,689 rows are added. Parse, plan, apply, and reading the screen back take 17.0 ms (target 1 s).
    - Every pasted cell reads back as Calc's text, and one undo restores the row count with no changes left.
  - Unit tests: the TSV parsing cases above; a paste with leading zeros, `=`, quotes, and a multi-line cell that lengthens a short row and adds rows, undone and redone in one step; paste past the end refused before indexing; the cell limit.
  - Smoke test via Broadway on a /tmp copy of the 10 MB file:
    - Copied B1:D3 and pasted at row 193,310. That added a row (193,312), with "Pasted 9 cells · 10 unsaved edits", the pasted range selected, and Ctrl+Z back to 193,311.
    - Copied column C (193,311 cells) and pasted at F1. The status showed "Pasted 193,311 cells" in the first screenshot, 300 ms after the key.
    - Undo, redo, and a save: column F equals column C on every line, and every other column is identical to the source.
  - Not tested yet: a real desktop paste from GUI LibreOffice into the app on Wayland (needs the user's OK to focus the app's window).
- **2026-10-07 CLIP-1:** copy and cut a range as TSV plus text/html.
  - `data_model::copy`: `CsvTable::copy_range(rows, cols: impl RangeBounds<Col>, cancel, done) -> Option<Copied { tsv, html, cells }>` reads rows in 4,096-row chunks through the normal read path (edits, row and column order, clears), into a new `RowBlock::full_text()` that doesn't cut cells at the 256-byte display limit. A bounded range pads short rows, so the copy is a rectangle; whole rows (`col..`) take each row's own cells.
  - TSV: a cell is quoted (`""` escapes) only when it holds a tab, a line break, or a leading quote; there is no trailing newline.
  - HTML: `<meta charset="utf-8">` and a plain `<table>`. Three kinds of value get `sdnum="1033;0;@"` (LibreOffice's text format) plus `mso-number-format:'\@'` (Excel's): digit strings with a leading zero, digit strings longer than 15 digits, and anything starting with `=`. A headless Calc check showed what goes wrong without the mark: `00692` became 692, a 19-digit ID became 1.23E+18, and `=1+1` became a formula. Spaces HTML would collapse are `&nbsp;` (Calc reads them back as plain spaces), tabs are `&#9;`, and line breaks are `<br>`. Past `HTML_MAX_CELLS` (2^20) the copy is plain text only, since spreadsheets don't take more rows and the HTML would triple the memory.
  - App: `CsvTable::snapshot()` (shares the index, clones the edits) runs on `gio::spawn_blocking`. The result goes onto the clipboard as a `ContentProvider` union of `text/html` and the TSV's UTF-8 bytes, offered as both `text/plain;charset=utf-8` and `text/plain`. Before that fix, a GTK string value was used, and GTK's own `text/plain` (no charset) turned non-ASCII into `\C3\AB` escapes, which the desktop check caught. The status bar shows "Copying N%" and then "Copied N cells" for 4 s. A new copy cancels a running one, and only the latest reaches the clipboard. A cut copies from the snapshot, then clears through EDIT-5's `clear_selected`, and is refused during a save. Whole columns wait for indexing. Keys: Ctrl+C, Ctrl+Insert, Ctrl+X, Shift+Delete; when the cell editor is open, these keys go to the editor's own text.
  - Acceptance:
    - **LibreOffice:** `data-model/tests/paste_libreoffice.rs` (ignored, needs LibreOffice). It copies a range with leading zeros, a 19-digit ID, `=1+1`, a quoted comma, a line break, a tab, runs of spaces, `& < > "`, non-ASCII text, an empty cell, a date, and a boolean. It puts both flavors on a headless Calc's clipboard through UNO (`scripts/lo_paste.py`) and pastes. Every cell comes back with the file's text; numbers come back as numbers and the marked values as text.
    - **Plain text, headless:** with only the TSV offered, Calc pastes nothing, because it wants its Text Import dialog. That is why the HTML matters.
    - **Desktop (Hyprland/Wayland, user-approved):** the app ran on a silent workspace with the same 4×5 fixture, and Hyprland's `send_shortcut` selected A1:E4 and pressed Ctrl+C. `wl-paste --list-types` gave `text/html`, `text/plain;charset=utf-8`, and `text/plain`; both text types give identical UTF-8 TSV, which is what a text editor receives.
    - **LibreOffice GUI, real system clipboard:** a GUI Calc (gtk3 VCL, focused briefly through UNO + `hyprctl`) ran Edit > Paste. Every cell matched the headless test: numbers as values; `00692`, the 19-digit ID, and `=1+1` as text; the line break, tab, runs of spaces, `& < > "`, and `Zoë 東京` intact.
    - **Excel web:** not tested; it needs the user's Microsoft login. It reads the same `text/html` table, and `mso-number-format` is Excel's own attribute [INFERENCE]. Owed: a manual paste by the user.
  - CI: opening a PR now starts a `pull_request` run by itself (PR #25 and #26), next to the manual dispatch. The ENG-1 and INFRA-1 notes about the trigger not firing are out of date.
  - Measured (release, this machine): copying 10k rows × 8 takes 6 ms; 1M cells takes 0.23 s (6 MB TSV, 24 MB HTML); the 1 GB file's whole column C (19,094,579 cells) takes 3.9 s (132 MB, text only).
  - Smoke test via Broadway, reading the clipboard back with temporary code that was removed afterwards:
    - Ctrl+C on B1:H3 offered `text/html`, `text/plain;charset=utf-8`, and `text/plain`, with the expected TSV and HTML (`00692` marked as text); the status read "Copied 21 cells".
    - Ctrl+X on C2:C3 copied "Denver" twice and emptied the cells; Ctrl+Z restored them.
    - A whole column of the 10 MB file copied 193,311 cells.
    - On a /tmp copy of the 1 GB file, the status bar showed "Copying 28%", then 38%. PageDown kept working during the copy, and it ended with "Copied 19,094,579 cells as plain text".
- **2026-10-07 EDIT-5:** Delete clears the selection as one undoable step, kept as blocks rather than an edit per cell (user's choice over capping per-cell edits).
  - `data_model::cleared`: a clear is a block of file rows (sorted, merged ranges) × columns by identity (`ColSet`: ids, plus with `rest` every file column from there on, for whole rows). Blocks are flattened into sorted, non-overlapping row segments with the union of their columns, so a lookup is one binary search however many blocks there are. Segments are rebuilt on each clear and undo (O(total ranges)).
  - Edits win over blocks. `Edit::ClearCells { rows: Vec<Run>, cols: ColSet }` takes out the cell edits it covers (inserted rows and columns included) and pushes the block. Its inverse `UnclearCells` holds those edits, takes the block back, and restores them. Typing into a cleared cell is a normal edit; undoing it shows the cleared cell again. Only existing fields are blanked, so short rows stay short. Rows and columns are stored by identity, so later inserts, deletes, and header flips don't move a clear.
  - `changes()` counts a block as one change. `commands::ClearCells` wraps the edit; its focus is the cursor when Delete was pressed. `CsvTable::clear_cells(rows, cols: impl RangeBounds<Col>)`: an open end (`2..`) reaches every column.
  - Save: rows covered by a segment are re-assembled (chunked span fetch, as in EDIT-4) with cleared fields empty; everything between them still copies as byte ranges. `write_source` now walks "dirty" ranges (edited rows and cleared segments) in order; the column-changed path is the same walk over the whole run.
  - App: Delete (or keypad Delete) with no modifiers; Shift+Delete and Ctrl+Delete are left alone (cut, delete word). Whole columns wait for indexing so they reach every row; `grid::Selection::is_whole_rows` sends whole rows as an open column range.
  - Acceptance: `commands::clear::tests::delete_clears_the_selection_and_undoes_in_one_step`: a 2 × 2 clear over an edited cell; one undo restores all four (the edit included), one redo clears them again. Model test `cleared_cells_read_and_save_empty_and_undo_exactly`: quoted field, short row, wider row with an open-ended clear, an edit inside a clear, an inserted row; exact saved bytes, and undoing everything gives back the file. `commands/tests/clear_1gb.rs` (ignored, corpus, release): clearing column C over 19,094,579 rows takes 0.004 ms, +0 KiB, 116 B of history; undo 0.001 ms; the save re-assembles every row in 2.9–3.8 s with city empty in every data row and the header kept.
  - Smoke test via Broadway on a /tmp copy of the 10 MB file: Delete on B2:C3, then on column D; two undos back to the original, two redos; Delete on row 5 (whole row); typed "hi" into cleared C2; saved. The file has `1,,hi,,31.78,…`, `2,,,,716.02,…`, `,,,,,,,` for row 5, an empty revenue in every row, and everything else identical.
  - **Bug found (not EDIT-5; fixed in SAVE-2):** after a save of a file that re-indexes within one 100 ms status tick (anything up to a few hundred MB), the scroll range and row-number gutter stay as they were when the reopened file had hardly any rows: Ctrl+End doesn't scroll, and row numbers overlap column A. Cause: in `app/src/main.rs` the tick computes `complete` before `finish_save` swaps in the reopened table, then sets `was_complete = complete`, so the reopened table's completion never triggers `update_adjustments`. Since SAVE-1.
- **2026-10-07 EDIT-4:** insert and delete columns, on a column map in front of the overlay (ADR 0003).
  - `ColId` gives columns a stable identity, like `RowId`: a file column index, or the high bit for inserted columns. `CellRef.col` is now a `ColId` (the type change made the compiler find every call site), and the overlay keys on it, so edits stay with their columns. Positions map through `CsvTable::col_id`/`col_of`.
  - `data_model::colmap::ColMap` holds explicit ids for the first positions, then file columns in order from `tail`. Rows are ragged and the widest isn't known up front, so a plain list can't work. An inserted column reads as `""` in every row, so header titles continue past it. `Edit::InsertCols`/`DeleteCols` with inverses; undo restores deleted columns with their edits.
  - `commands::ChangeRows` became `commands::Reshape` (rows and columns). Its focus is the first restored row or column, at the cursor's other coordinate.
  - Save: with columns changed, every source row is re-assembled. File fields are copied as their raw bytes (quotes intact), edits are encoded, and each row keeps its own line ending; short rows stay short. Spans are fetched 4,096 rows at a time with `row_spans`. Fetching them per row scans from a checkpoint for each one, and took 10.5 s on 1 GB; chunked, it is 3.5 s. File order still copies byte ranges.
  - Acceptance: `commands/tests/columns_1gb.rs` (ignored, corpus, release). On the 1 GB file, insert a column, edit it, and delete another: 0.004 ms and +0 KiB of anonymous memory; the last row reads with its columns moved. A save to /tmp re-assembles all 19,094,580 rows in 3.5 s, with the expected header and last line.
  - Model test: column inserts and deletes with a quoted field, a short row, and an inserted row save to exact bytes; undoing everything returns the original file.
  - App:
    - Ctrl++/− act on columns when whole columns are selected; the right-click menu has row and column sections.
    - User sizes are kept by `RowId`/`ColId` and laid out by position after every change (`refit_sizes`), so widths and heights follow their columns and rows through inserts, deletes, undo, header flips, and the reopen after a save (re-keyed by position).
    - The cache resets after reshapes, because a column delete can narrow the widest row.
  - Smoke test via Broadway on a /tmp copy of the 10 MB file:
    - Narrowed city, then inserted a column before it; city kept its width.
    - Typed into the new column, deleted score with Ctrl+-, and used Insert Columns Right from the menu.
    - Four undos back to the original, with city still narrow; four redos and a save.
    - The file reads `id,,name,,city,revenue,date,active,code`, with "hi" in place and score gone from every row.
- **2026-10-07 EDIT-3:** insert and delete rows on the ADR 0003 row map.
  - `data_model::rowmap::RowMap` is an order-statistic treap of `RowId` runs, adapted from the DEC-3 spike, with freed nodes reused (the spike leaked them). `CsvTable` holds `Option<RowMap>`: `None` means file order, which is unchanged and zero-cost; the map is made on the first insert or delete. Inserted rows get ids with the high bit set.
  - `Edit::InsertRows`/`DeleteRows` take positions among all rows, header included, so a header flip between an edit and its undo can't shift it. They are built by `CsvTable::insert_rows`/`delete_rows`, which return `None` until indexing completes; `apply` asserts that, because a map frozen early would drop the rest of the file on save. A delete's inverse holds the removed runs, so undo brings the rows back with their cell edits (the edits stay keyed by `RowId`). `commands::ChangeRows` wraps them; its focus is the first row put back in.
  - Save walks the runs:
    - Source runs copy as byte ranges, with edited rows re-encoded as before.
    - Inserted rows are encoded from their edits, at least as wide as the first row, so an empty one is `,,,` and not a blank line that pandas and others skip.
    - The BOM is written once, first, and a final source row without a line ending gets one when rows follow it.
  - "Unsaved edits" is now `CsvTable::changes()`: cells, plus rows inserted and source rows deleted. Delimiter re-reads wait for it too.
  - App: Ctrl++ inserts as many rows as the selection spans, above it; Ctrl+- deletes them; a right-click menu offers Insert Rows Above/Below and Delete Rows. Whole-column selections are left to EDIT-4. Row heights reset when the row count changes, since they belong to positions.
  - Acceptance: `commands/tests/insert_1gb.rs` (ignored, corpus, release). Inserting at row 1,000,000 of the 1 GB file plus reading back the 60-row screen took 0.027 ms (target 50 ms). The new row is empty and row 1M moved down one; delete and the two undos take about 1 µs.
  - Model tests:
    - A save round trip with BOM, CRLF, no final newline, inserts at top and end, a delete, and a quoted value. Undoing everything gives the original bytes.
    - A deleted row comes back with its edits.
    - Row edits are refused before indexing.
    - The row map matches a flat vector through 2,000 random inserts and removes.
  - Smoke test via Broadway on a /tmp copy of the 10 MB file: Ctrl++ at row 3, typing into it, Ctrl+- on rows 5–6, the right-click menu's Insert Rows Below, four undos back to the original (the deleted rows returned), four redos, and a save. The saved file has `,new,,,,,,`, the deleted rows gone, `,,,,,,,` for the empty row, the same line count, and everything after the edited area byte-identical.
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
