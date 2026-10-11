# 0008: Sheets and undo

- Story: DEC-8
- Status: accepted
- Date: 2026-10-10

## Question

With several sheets in one window, how does undo work across them, and how are sheet-structure changes (add, rename, move, delete) undone?

## Options

| Option | Measured result | Cost / risk |
| --- | --- | --- |
| A. One `UndoStack<Workbook>`; existing `Command<CsvTable>` commands wrapped by `OnSheet` | Not measured | Undo switches sheets when the step happened elsewhere |
| B. One stack per sheet | Not measured | Sheet-structure edits and cross-sheet formula rewrites belong to no single sheet; Ctrl+Z order differs from what the user did |

## Spike

None; decided with the user while planning M5 from API research (calamine 0.36.1, rust_xlsxwriter 0.99.1, IronCalc 0.8.3 on docs.rs).

## Decision

**Option A.** One workbook per window, one undo history. `GridView` owns a `Workbook` (sheets, active sheet, defined names, calculation cache) and an `UndoStack<Workbook>`. Existing `Command<CsvTable>` commands are wrapped by `OnSheet`, which names the sheet it ran on; undo and redo switch to that sheet first. Sheet-structure edits (`BookEdit`: insert, remove, rename, move, and multi-sheet cell edits) are commands too (`SheetEdit`), each storing the operation and its inverse, never a snapshot (invariant 5).

## Consequences

- Ctrl+Z order matches what the user did across sheets.
- A rename that rewrites formulas on other sheets is one undo step.
- History is cleared when a save re-reads the file, as today.
- **Revisit if:** per-sheet history is requested.
