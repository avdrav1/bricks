# Changelog

Releases are added by the ship-it skill.

## v0.1.0 (2026-10-10)

First release: the large-CSV editor (milestones S0 to M3) plus the M4 desktop work and the curl installer.

- DEC-1: Choose UI toolkit (GTK4 vs Slint vs egui vs Iced)
- DEC-2: Choose source storage (mmap + sparse index vs chunked arena vs columnar)
- DEC-3: Choose edit model (overlay + row map vs piece table)
- DEC-4: Choose sort/filter representation
- DEC-5: Choose concurrency model (Rayon + channels + cancel tokens vs async)
- INFRA-1: CI: fmt, clippy -D warnings, test on every PR
- INFRA-2: Test corpus generator
- ENG-1: Stream-index a CSV into a row offset index
- ENG-2: Fetch any row range by logical index
- ENG-3: RFC 4180 parsing incl. quoted fields with embedded newlines
- GRID-1: Virtualized grid renders visible cells plus a buffer
- GRID-2: Scrollbar drag jumps to arbitrary positions
- EDIT-1: Single-cell edit stored in overlay
- SAVE-1: Atomic save: temp file, verify, rename
- BENCH-1: Benchmark harness over the corpus
- ENG-4: Auto-detect delimiter: comma, tab, semicolon, pipe
- ENG-5: Manual delimiter override, re-index in background
- ENG-6: Encoding detection: UTF-8, UTF-8 BOM, UTF-16, Latin-1
- ENG-7: Header row detection with toggle
- GRID-3: Keyboard navigation: arrows, Tab, Enter, Shift variants, Ctrl+Home/End, PgUp/PgDn
- GRID-4: Cell, range, row, and column selection
- GRID-5: Resize columns and rows; double-click autofits column
- APP-1: Open via native file dialog (xdg-desktop-portal)
- APP-2: Status bar: row count, encoding, delimiter, save state
- APP-3: Show first rows before indexing finishes
- CMD-1: Command pattern for all mutations
- CMD-2: Undo/redo stores operations, not snapshots
- EDIT-2: In-cell editor; F2 or typing starts edit
- EDIT-3: Insert and delete rows
- EDIT-4: Insert and delete columns
- EDIT-5: Delete key clears selected cells
- CLIP-1: Copy/cut range as TSV plus text/html
- CLIP-2: Paste TSV into a range, expanding as needed
- SAVE-2: Save, Save As, New
- SAVE-3: Background save with progress and cancel
- SAVE-4: Preserve quoting style and line endings of untouched rows
- APP-9: Ask before closing with unsaved edits
- ENG-8: Background job framework: progress and cancel
- TYPE-1: Infer column types by sampling
- TYPE-2: Show inferred type in header; allow override
- SORT-1: Type-aware sort, ascending and descending
- SORT-2: Sort moves whole rows and honors the header
- FILT-1: Per-column filter from header dropdown
- FILT-2: Combine filters across columns (AND)
- FILT-3: Saving with an active filter keeps hidden rows
- FILT-4: Edits and paste respect the filtered view
- SRCH-1: Find bar (Ctrl+F): whole file or current column, case toggle
- SRCH-2: Next/previous result; hit count streams in
- SRCH-3: Find and replace; replace all
- APP-4: Recent files menu
- APP-5: Drag and drop to open
- APP-6: Follow system dark/light theme; high-DPI
- APP-7: Crash recovery via periodic overlay journal
- APP-8: Clear errors for malformed files
- PKG-3: Portable release build: link against Ubuntu 24.04's glibc (build in a container)
- PKG-1: curl installer (per user, no sudo): `install.sh` on the download site; .desktop file, icon, text/csv association; `--uninstall`
