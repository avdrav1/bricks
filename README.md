# Spreadsheet

A fast, lightweight spreadsheet for Linux, written in Rust. V0.1 goal: open, browse, edit, sort, filter, search, and save ~1 GB CSV files faster than LibreOffice Calc.

## Start here

1. Install Rust stable (`rustup`), Python 3, GTK 4.14+ dev files (`gtk4` on Arch, `libgtk-4-dev` on Debian/Ubuntu), and Node (for `npx wrangler` when shipping).
2. `cargo test --workspace` should pass on a fresh clone.
3. Paste the product spec into `docs/SPEC.md`.
4. `python3 scripts/gen_corpus.py` builds test files in `corpus/` (about 5 GB at full size; use `--max-mb 100` to start small).
5. Open the repo in Claude Code and say **"next story"**.

## Run it

```sh
cargo run --release -p spreadsheet -- path/to/file.csv
cargo run --release -p spreadsheet                      # start window with an Open button
```

Ctrl+O (or the folder button in the header bar) opens the desktop's file dialog through xdg-desktop-portal. Each file opens in its own window; opening a file that is already open brings its window forward.

Move the cell cursor like LibreOffice Calc: arrows, Tab/Shift+Tab, Enter/Shift+Enter, Home/End, Ctrl+Home/End, Page Up/Down, and Shift with any of them to select (checklist: `docs/navigation-checklist.md`). Select with the mouse too: click a cell, Shift+click or drag for a range (dragging past an edge scrolls), click or drag row numbers and column letters for whole rows and columns, and the corner for everything. Shift+Space selects rows, Ctrl+Space columns, Shift+Ctrl+Space everything. The mouse wheel and scrollbars scroll. Ctrl+S saves over the file atomically: untouched rows are written byte for byte, and a crash mid-save leaves the original intact. The status bar shows the row count (growing, with a percentage, while the file indexes), unsaved edits and save progress, and the delimiter and encoding; the title marks unsaved edits with •. The delimiter (comma, tab, semicolon, pipe) is detected on open; the dropdown in the header bar overrides it without a restart (save edits first). Encodings: UTF-8 (with or without BOM), UTF-16 LE/BE, and Windows-1252 ("Latin-1") are detected on open and kept on save, BOM included.

Edit in the cell, as in Calc: start typing to replace a cell's content, or press F2 (or double-click) to change it. Enter commits and moves down, Shift+Enter up, Tab and Shift+Tab across (Enter after a run of Tabs returns to the column you started in); Esc cancels. After typing, arrow keys also commit and move; after F2 they move the text cursor. Clicking elsewhere keeps what you typed, and Ctrl+S saves it.

Insert and delete rows with Ctrl++ (as many empty rows as the selection spans, above it) and Ctrl+- (the selected rows); with whole columns selected (click the column letters), the same keys insert columns to the left and delete them. Right-click a cell or header for Insert Rows Above / Below, Delete Rows, Insert Columns Left / Right, and Delete Columns. Nothing is rewritten until you save: untouched rows are then still copied byte for byte, unless columns changed, in which case every row is re-assembled from its raw fields (3.5 s for 1 GB). Rows can be inserted and deleted once the file is fully indexed (under a second for 1 GB).

Ctrl+Z undoes the last edit and moves the cursor to the cell it changed; Ctrl+Shift+Z or Ctrl+Y redoes it. History keeps the last 1,000 steps within 50 MB, whatever the file size, dropping the oldest first. Undo waits while a save runs, and the history starts over after a save.

Resize a column or row by dragging its border in the column letters or row numbers; selected whole columns resize together. Double-click a column border to fit the text on screen, or a row border to restore its height. Sizes stay with their rows and columns through inserts, deletes, undo, and saving, and last while the window is open (CSV has nowhere to store them).

A header row is detected on open: when the first row names the columns, it becomes the column titles (next to the letters) and isn't counted as data. The **Header row** button in the header bar flips it; your choice holds when the delimiter changes or the file is saved. The file itself never changes: a save writes the first row as it is.

## Layout

```
crates/csv-engine    parsing, dialects, encodings, row index
crates/data-model    cells, edit overlay, types, sort/filter views
crates/grid          viewport, selection, navigation (no toolkit)
crates/commands      undo/redo
crates/file-format   atomic save
app/                 the desktop app (GTK4, ADR 0001)
BACKLOG.md           the work queue, read by scripts/backlog.py
docs/                spec, benchmarks, architecture decisions
site/                download page deployed to Cloudflare Pages
scripts/             backlog helper, corpus generator, release pipeline
.claude/skills/      next-story and ship-it skills for Claude Code
```

## Claude Code skills

- **next-story**: picks the next unblocked story from `BACKLOG.md`, writes a failing test from its acceptance criterion, implements it, verifies fmt/clippy/tests/benchmarks, updates the backlog, and commits locally. Stops at milestone gates and decision reviews for your approval.
- **ship-it**: preflight, version bump, release build, upload to Cloudflare R2, deploy the download page to Cloudflare Pages, tag. Asks before anything leaves your machine. Needs `.env.ship` (see `.env.ship.example`).

## License

MIT OR Apache-2.0 (open question in the plan; change before the first release if needed).
