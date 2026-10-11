# 0007: Formula engine

- Story: DEC-7
- Status: accepted
- Date: 2026-10-10

## Question

How are formulas parsed, evaluated, and kept current after edits, on files with millions of rows, without slowing the UI thread?

## Options

| Option | Measured result | Cost / risk |
| --- | --- | --- |
| A. Own crate (`formula`) with lazy, revision-cached evaluation over raw cells | Not measured | Every edit recomputes visible formulas; large ranges cost a background pass after each edit |
| B. Dependency graph with incremental recalculation | Not measured | A graph over millions of cells costs memory and must be kept exact through inserts, deletes, sorts, and undo |
| C. IronCalc's engine | Not measured | Needs IronCalc's in-memory model (rejected in ADR 0006) |

## Spike

None; decided with the user while planning M5 from API research (calamine 0.36.1, rust_xlsxwriter 0.99.1, IronCalc 0.8.3 on docs.rs).

## Decision

**Option A.**

1. **A cell's raw text is authoritative; a formula is raw text starting with `=` that parses.** A formula cell's raw text is the formula (`=SUM(A1:A3)`); the grid shows its result. Raw text starting with `=` that fails to parse is shown as text. Nothing on disk changes unless the user edits.
2. **Evaluation is lazy and revision-cached.** Results are computed on demand for visible cells on the UI thread within a budget of 200,000 cell visits per frame; anything over budget shows `…` and is computed by a background job on a workbook snapshot. Any edit anywhere clears the cache (the workbook revision changes). No dependency graph. Deep chains (running totals) are evaluated with an explicit work stack, never deep recursion. Cycles give `#CIRC!`.
3. **Jobs see results through a `computed` override.** Sort, filter, find, type inference, and copy run on table snapshots; when a sheet has formula cells, the app first runs a "calculate sheet" job that produces `Computed` (result text for every formula cell) and sets it on the snapshot. Read paths that produce shown text consult it; raw paths (`cell_value`, save, journal) never do.

## Consequences

- Correct results without tracking dependencies.
- Every edit recomputes the visible formulas; a formula over a large range costs a background pass after each edit.
- The `formula` crate has no dependencies and no clock: the workbook supplies `TODAY()`/`NOW()` through its context.
- **Revisit if:** recalculation after an edit on a sheet with >100k formulas takes over 1 s on the reference box.
