# 0009: Cell styles

- Story: DEC-9
- Status: accepted
- Date: 2026-10-11

## Question

How does Bricks hold, show, and edit an XLSX workbook's cell styles (fonts, fills, borders, alignment, number formats, merged cells, column widths, row heights) without materializing per-cell data for million-row sheets?

## Options

| Option | Measured result | Cost / risk |
| --- | --- | --- |
| A. A workbook style pool with the file's indices, and per-sheet layers (cell runs, row styles, column styles) keyed by row and column ids | Not measured; the real-world file that prompted this (12,571 rows) and LibreOffice's fixture both style whole column runs, so runs stay few | A run per style change; a worst case (a different style on every cell) costs a run per cell |
| B. A style id per cell beside each cell's text in the imported copy | Not measured | Every cell pays for a style, the copy format changes, and edits still need an overlay |
| C. An ordered list of format operations folded at lookup | Not measured | Lookup cost grows with every action in the session; save walks every cell through every operation |

## Spike

None; decided with the user on 2026-10-11 (keep, show, and edit styles; patch the original package on save; before v0.2.0). calamine 0.36.1 exposes no fonts, fills, borders, or style index, so Bricks reads `xl/styles.xml`, `xl/theme/theme1.xml`, and each sheet's `s` attributes itself with quick-xml 0.41 (already in the tree through calamine).

## Decision

**Option A.**

1. **One pool per workbook** (`data_model::style`): fonts, fills, borders, number formats, and cell formats (`xf`) under the file's own indices, so `StyleId` is the file's `cellXfs` index and an unchanged cell keeps its `s`. Colours stay as written (automatic, RGB, theme with tint, indexed) and resolve when drawn. A record Bricks doesn't model (a gradient fill) is kept by index and drawn approximately. New records are appended and deduplicated; the pool only grows, so undo never invalidates an id.
2. **Excel's layering per sheet**, stored in `CsvTable` so snapshots, edits, undo, and the journal carry it: cell styles as runs of row ids per column, then row styles, then column styles, each in two layers (the file's, keyed by source position, and the user's, keyed by id). A cell's style is the first layer that has one, else 0. Memory follows style changes, not cells.
3. **Format edits are operations on ranges** (`Edit::Styles`), with an exact inverse. A whole-column action sets the column style and restyles only the cells that carry their own style in that column, as Excel does, so it never walks every row; whole rows likewise. Cell-range actions are capped like replace all.
4. **Sheet layout from the file**: column widths and row heights by id, merged ranges, and the other range-bearing parts of a sheet (conditional formats, data validation, hyperlinks, autofilter) as range slots in all-rows positions; their formulas, chart series, and defined names as formula slots. Structure edits adjust slots in the same command as FML-10's formula rewrites. Sorting rows that a merge spans is refused; other slots stay put on sort, as in Excel.
5. **Display**: a cell in the workbook's default font draws in Bricks' UI font, so a plain workbook looks as today; another font changes family when its name differs and scales size by its point size over the default's. Row heights and column widths scale from Excel's units to Bricks' row height and digit width. General alignment right-aligns numbers, dates, and numeric results on every sheet, CSV too. Text that doesn't wrap overflows into empty neighbours. Automatic colours follow the GTK theme; an explicit text colour under 3:1 contrast against the cell's background falls back to automatic, so dark mode stays readable; fills draw as written. Number formats change only what a cell shows: raw text stays authoritative (invariant 1), and find and filters still match cell text and formula results, not formatted text. Hidden rows and columns still show; saving keeps them hidden.

## Consequences

- Sort, filter, insert, and delete carry styles with no extra work, since styles follow row and column ids.
- Format actions are cheap on whole columns and rows; a style per cell is possible but costs a run per cell.
- Import reads each sheet part twice (calamine for values, quick-xml for styles and layout).
- Excel's conditional formats, data bars, and cell styles by name are kept but not drawn.
- **As built in STYLE-4 (merged cells):** `CsvTable` holds the merged ranges as a sorted list in all-rows positions (the file's to begin with); `Edit::Merges` replaces the list, so Merge, Unmerge, and the merges that follow row and column inserts and deletes (Excel's range rules, in the structural edit's undo step) are each one operation with an exact inverse. Lookups binary-search the list, reaching back by the tallest merge's height. Navigation stays toolkit-free: `grid::Merges` gives `Selection` the merges in grid cells, the cursor sits on a merge's anchor, keys step out from its far edge, and ranges grow to cover the merges they touch. Sorting rows a merge covers is refused; merging waits for file order (no sort, no filter) and reads at most 65,536 cells on the UI thread. The recovery journal carries merges from STYLE-6 on.
- **As built in STYLE-6 (editing formats):** `CsvTable` holds the user's layers (`UserStyles`: cell runs per column, row runs, column styles, keyed by row and column id values) and `Edit::Styles` sets ranges with an exact inverse. `CsvTable::format_edit` turns a `StylePatch` into one edit by cutting the selection at every layer boundary, so cost follows runs, not cells; `StylePool::apply` appends records (reusing equal ones). Whole columns and rows also restyle cells where a row (column) style meets them, and border presets give whole columns their first and last table rows and whole rows their first and last columns, as Excel does. A patched save rewrites restyled cells' `s` in place (rich text survives) and adds rows and cells the file lacks. The journal (`BRICKSJ3`) carries the layers, the merges, and the pool's added records; restoring re-interns them, so formats made before restoring don't clash.
- **Revisit if:** a real workbook's runs exceed 10 per row on average, or showing conditional formats becomes a goal.
