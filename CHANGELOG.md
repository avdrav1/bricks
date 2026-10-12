# Changelog

Releases are added by the ship-it skill.

## Frame v0.1.0 (2026-10-12)

Frame's first release, milestones FR0 and FR1: a lightweight presentation app that opens, shows, edits, and saves PowerPoint decks (.pptx). Edit on the slide or in an outline, format with one inspector instead of a ribbon, and play the deck full screen. Saving patches the original file, so everything you didn't edit stays as it was. The spec's four-slide reference deck takes 15 keys and clicks; PowerPoint's shortest documented way takes 21.

- FRAME-1: Presentation model and PPTX package: slide order, size, shapes, placeholder inheritance
- FRAME-2: PPTX text: runs, fonts, sizes, colours, bullets, levels, alignment, inherited styles
- FRAME-3: PPTX pictures, fills, outlines, backgrounds, groups, and tables
- FRAME-4: Viewer window: slide list and canvas
- FRAME-5: Full-screen slideshow
- FRAME-6: Edit slide text on the slide with undo
- FRAME-7: Add, delete, duplicate, and reorder slides; layouts as thumbnails
- FRAME-8: Save .pptx by patching the original package
- FRAME-10: Select, move, resize, and nudge shapes with snapping and smart guides
- FRAME-11: Inspector sidebar for the selection, no formatting dialogs
- FRAME-12: Insert text boxes, shapes, pictures, and tables; text shrinks to fit
- FRAME-13: Outline view of titles and bullets
- FRAME-14: File → New from the built-in blank deck
- FRAME-15: Build the reference deck in fewer keys and clicks than PowerPoint
- FRAME-9: Desktop entry, icon, and installer

## Slate v0.1.0 (2026-10-12)

Slate's first release, milestones SL0 and SL1: a lightweight, pageless word processor that opens, edits, and saves Word documents (.docx), Markdown, and plain text. Saving a .docx patches the original file, so everything you didn't edit stays as it was; an edited Markdown file changes only the blocks you edited.

- SLATE-1: Document model and plain-text reader
- SLATE-2: Read Markdown into the document model
- SLATE-3: Read DOCX styles, runs, lists, tables, links, and pictures
- SLATE-4: Viewer window: open, drop, show formatted
- SLATE-10: Pageless column with a text width setting
- SLATE-5: Choose the editing engine: Slate's own Pango flow (ADR 0007)
- SLATE-11: Show documents in the Pango flow
- SLATE-6: Caret, typing, and formatting as undoable commands
- SLATE-12: Edit inside table cells
- SLATE-13: Input method composition
- SLATE-14: Selection and clipboard across paragraphs and tables
- SLATE-15: Screen reader access to the text, caret, looks, and edits
- SLATE-7: Save .txt and .md atomically, rewriting only edited Markdown blocks
- SLATE-8: Save .docx by patching the original package
- SLATE-9: Desktop entry, icon, and installer

## v0.2.1 (2026-10-11)

Fixes how real workbooks draw, and lists .xlsx and .xlsm in the desktop entry.

- STYLE-8: Real-workbook drawing fixes: centre across selection, grid lines off, tight rows
- XLSX-1: List .xlsx and .xlsm in the desktop entry (Open With)

## v0.2.0 (2026-10-11)

Milestone M5: XLSX workbooks, formulas, multiple sheets, and cell styles. Bricks opens .xlsx files with every sheet, shows their fonts, colours, fills, borders, alignment, number formats, and merged cells, lets you edit values, formulas, sheets, and formats, and saves by patching the original file so charts, comments, conditional formats, and macros (in .xlsm) survive.

- DEC-6: Workbook model: XLSX sheets as indexed CSV copies; references address file rows
- DEC-7: Formula engine: lazy evaluation over raw cells with a revision cache
- DEC-8: Multiple sheets: one workbook per window, one undo history
- FML-1: Formula language: parse, render, A1 references, sheet names, errors
- FML-2: Evaluator: values, coercion, operators, cycles, deep chains
- FML-3: Workbook and calculation in data-model: formula cells, cached results, computed values for jobs
- FML-4: Grid shows formula results; edits and undo go through the workbook
- XLSX-1: Open and save .xlsx: every sheet as a tab, typed values, formulas with results, names, crash recovery
- SHEET-1: Add, rename, delete, and move sheets; each step undoable
- SHEET-2: References across sheets; renaming a sheet updates them; deleting one gives #REF!
- XLSX-2: Warn about workbook content Bricks drops; ask before saving over it
- FML-5: Functions: math, statistics, logic, information
- FML-6: Functions: text, including TEXT format codes
- FML-7: Functions: lookup and reference
- FML-8: Functions: dates and times
- FML-9: Formula bar and name box
- FML-10: Formulas follow inserted, deleted, and sorted rows and columns
- FML-11: Sort, filter, find, type inference, and copy use formula results
- FML-12: Copy and paste formulas inside Bricks with relative references shifted
- BENCH-2: XLSX and formula rows in the benchmark table
- DEC-9: Styles model: workbook style pool, per-column style runs, Excel layering
- DEC-10: XLSX save patches the original package
- STYLE-1: Read styles, merges, column widths, and row heights from .xlsx
- STYLE-2: Excel number formats for display and TEXT
- STYLE-3: Grid shows fonts, colours, fills, borders, alignment, wrap, number formats, widths, and heights
- STYLE-4: Merged cells: drawing, navigation, merge and unmerge
- STYLE-5: Save .xlsx by patching the original package
- STYLE-6: Edit formatting: format bar, menu, and shortcuts, each undoable
- STYLE-7: Copy and paste carry formats inside Bricks
- BENCH-3: Styled XLSX rows in the benchmark table

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

## v0.1.1 (2026-10-10)

The app is now called Bricks: run `bricks`. Installing it replaces v0.1.0 (Spreadsheet) and keeps its recent files.

- PKG-4: Rename the app to Bricks: `bricks` command, window titles, app menu, state folder
- PKG-1: Download page and installer point at bricks.cronkite.cc and dl.cronkite.cc
