# 0010: Save XLSX by patching the original package

- Story: DEC-10
- Status: accepted
- Date: 2026-10-11

## Question

How does saving an .xlsx keep everything Bricks doesn't edit (charts, images, comments, conditional formats, validation, pivots, macros, rich text) while writing Bricks' edits and styles?

## Options

| Option | Measured result | Cost / risk |
| --- | --- | --- |
| A. Patch: copy the original package's parts, rewrite only what changed (sheets, styles, workbook) | Not measured; zip 8.6 copies entries without recompressing (`raw_copy_file`) and streams to a plain writer (`ZipWriter::new_stream`) | Bricks must keep the package's parts consistent (relationships, content types, sheet ids, calcChain) |
| B. Rewrite with rust_xlsxwriter, writing the styles Bricks models | Today's export, plus formats | Charts, images, comments, conditional formats, and pivots are still dropped |

## Spike

None; decided with the user on 2026-10-11.

## Decision

**Option A.**

1. At import the original package is copied to the cache directory, so saving over the original, or the original changing on disk, can't break a save.
2. The writer streams a new package into `suite_file::save_atomic`. Parts no edit touches are copied as compressed bytes. A changed sheet is streamed through quick-xml: everything outside its cell data, dimension, columns, merges, and range slots is copied as is. While rows and columns keep their order, unchanged cells are copied verbatim (rich text, shared strings, cell metadata survive) and only changed cells are written; after inserts, deletes, or a sort, every cell of that sheet is written from the model, and rich text in it is lost. Shared formulas in a changed sheet are written expanded.
3. `xl/styles.xml` keeps every original record; appended records go before the closing tags with counts updated. `xl/workbook.xml` gets the current sheet list (existing sheets keep their attributes), defined names from their slots, and `fullCalcOnLoad`; `xl/calcChain.xml` is dropped. New sheets get a part, relationship, content type, and sheet id; deleted sheets lose theirs. Once sheets are added, removed, or renamed, `docProps/app.xml` loses its sheet titles (they're optional, and stale ones mislead).
4. An `.xlsm` saved as `.xlsm` keeps its macros; saved as `.xlsx` it drops `vbaProject.bin` and switches the workbook's content type.
5. A workbook without a package (from CSV, or new) is written by the same writer from a built-in minimal package. rust_xlsxwriter leaves the save path; the corpus example keeps it as a dev-dependency, so benchmark workbooks come from a writer other than Bricks'.
6. The package is written with each part's sizes in its local header (LibreOffice can't read a zip streamed with data descriptors), so `suite_file::save_atomic` hands writers a seekable file. After writing, calamine reopens the temp file and checks sheet names and each written sheet's dimension, then the app re-imports it as today.
7. XLSX-2's losses bar and save-over prompt are removed: a save no longer drops content.

## Consequences

- Saving an unedited sheet costs a copy of its compressed bytes.
- Bricks owns package consistency; a mistake there makes Excel offer to repair the file. Tests compare part lists and LibreOffice dumps, and the gate opens a saved file in LibreOffice.
- Rich text runs are lost only in sheets whose rows or columns were reordered.
- **Revisit if:** Excel reports a saved file as needing repair, or a workbook part Bricks must rewrite isn't understood.
