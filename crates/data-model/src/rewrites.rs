//! Formulas that follow the workbook's structure: when a sheet is renamed or deleted
//! (SHEET-2), rows or columns are inserted or deleted, or rows are sorted (FML-10), the
//! formulas referring to them are rewritten. The rewrites are plain cell edits, so the
//! caller applies them together with the structural change as one undoable step.

use crate::formulas::RowPositions;
use crate::workbook::same_name;
use crate::{CellRef, CsvTable, Edit, RowOrder, SheetId, Workbook};
use csv_engine::RowIndex;
use formula::{adjust, parse, render, Axis, Change};
use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Formula cells visited between cancel checks.
const CANCEL_EVERY: u64 = 4_096;

/// Cells the rewrites change, over all sheets: compare it with
/// [`crate::REPLACE_MAX_CELLS`] before applying them.
pub fn rewrite_count(edits: &[(SheetId, Edit)]) -> u64 {
    edits
        .iter()
        .map(|(_, e)| match e {
            Edit::Cells(cells) => cells.len() as u64,
            Edit::Cell { .. } => 1,
            _ => 0,
        })
        .sum()
}

impl Workbook {
    /// The formula edits that make every sheet's formulas follow `change`, a renamed or
    /// deleted sheet: one [`Edit::Cells`] per sheet with changed formulas, setting each to
    /// its rewritten text (`='Q1 Sales'!B2` -> `='Q2'!B2`, or `=#REF!` once the sheet is
    /// gone). For [`Change::DeleteSheet`] the deleted sheet's own cells are left alone.
    /// Formulas that don't parse are left as they are. Run it on a snapshot on the job
    /// pool before applying the change; it scans sheets for formulas first where that
    /// hasn't been done. `None` once `cancel` is set, or while a sheet isn't indexed.
    pub fn sheet_rewrites(
        &mut self,
        change: &Change,
        cancel: &AtomicBool,
    ) -> Option<Vec<(SheetId, Edit)>> {
        let deleted = match *change {
            Change::DeleteSheet { name } => Some(name),
            _ => None,
        };
        self.rewrite_formulas(
            std::slice::from_ref(change),
            &|sheet, _| deleted.is_some_and(|d| same_name(&sheet.name, d)),
            cancel,
        )
    }

    /// The formula edits that make every sheet's formulas follow `edit`, a structural
    /// edit on sheet `sheet` that is about to be applied: inserted rows or columns move
    /// the references past them, deleted ones turn references into `#REF!` or shrink
    /// ranges ([`formula::adjust`]). Formulas on other sheets naming `sheet` follow too.
    /// Cells in deleted rows or columns are left alone. The edits key cells by id, so
    /// they are right before or after `edit`. Edits other than row and column inserts and
    /// deletes need nothing. Run it on a snapshot on the job pool; `None` once `cancel` is
    /// set, or while a sheet isn't indexed.
    pub fn structure_rewrites(
        &mut self,
        sheet: SheetId,
        edit: &Edit,
        cancel: &AtomicBool,
    ) -> Option<Vec<(SheetId, Edit)>> {
        let Some(name) = self.sheet(sheet).map(|s| s.name.clone()) else {
            return Some(Vec::new());
        };
        let name = name.as_str();
        let (changes, gone): (Vec<Change>, Vec<(Axis, Range<u64>)>) = match edit {
            Edit::InsertRows { at, rows } => {
                let count = rows.iter().map(|r| r.len).sum();
                (vec![insert(name, Axis::Rows, *at, count)], Vec::new())
            }
            Edit::DeleteRows { at, count } => (
                vec![delete(name, Axis::Rows, *at..at + count)],
                vec![(Axis::Rows, *at..at + count)],
            ),
            // Highest range first, so the lower ranges' positions still hold.
            Edit::DeleteRowRanges(ranges) => (
                ranges
                    .iter()
                    .rev()
                    .map(|r| delete(name, Axis::Rows, r.clone()))
                    .collect(),
                ranges.iter().map(|r| (Axis::Rows, r.clone())).collect(),
            ),
            Edit::InsertCols { at, cols } => (
                vec![insert(name, Axis::Cols, u64::from(*at), cols.len() as u64)],
                Vec::new(),
            ),
            Edit::DeleteCols { at, count } => {
                let span = u64::from(*at)..u64::from(*at) + u64::from(*count);
                (
                    vec![delete(name, Axis::Cols, span.clone())],
                    vec![(Axis::Cols, span)],
                )
            }
            _ => return Some(Vec::new()),
        };
        self.rewrite_formulas(
            &changes,
            &|s, (row, col)| {
                s.id == sheet
                    && gone.iter().any(|(axis, span)| {
                        span.contains(&match axis {
                            Axis::Rows => row,
                            Axis::Cols => u64::from(col),
                        })
                    })
            },
            cancel,
        )
    }

    /// Rewrite every sheet's formulas for `changes`, applied in order to each formula.
    /// `skip(sheet, (row position, view column))` leaves a cell out.
    pub(crate) fn rewrite_formulas(
        &mut self,
        changes: &[Change],
        skip: &dyn Fn(&crate::Sheet, (u64, u32)) -> bool,
        cancel: &AtomicBool,
    ) -> Option<Vec<(SheetId, Edit)>> {
        let ids: Vec<SheetId> = self.sheets().iter().map(|s| s.id).collect();
        for id in ids {
            let table = &mut self.sheet_mut(id).expect("listed just now").table;
            if !table.formulas_scanned() {
                let found = table.scan_formulas(cancel, &AtomicU64::new(0))?;
                table.set_formula_cells(found);
            }
        }
        let mut out = Vec::new();
        let mut seen = 0u64;
        for sheet in self.sheets() {
            // A formula on another sheet reaches the changed one only through `Sheet!`.
            let local = changes.iter().any(|c| match *c {
                Change::Insert { sheet: s, .. } | Change::Delete { sheet: s, .. } => {
                    same_name(s, &sheet.name)
                }
                _ => false,
            });
            let table = &sheet.table;
            let mut cells = Vec::new();
            for (at, pos) in table.formula_cells_at().expect("scanned just now") {
                seen += 1;
                if seen.is_multiple_of(CANCEL_EVERY) && cancel.load(Ordering::Relaxed) {
                    return None;
                }
                if skip(sheet, pos) {
                    continue;
                }
                let Some(raw) = table.cell_of(at.row, at.col) else {
                    continue;
                };
                if !local && !raw.contains('!') {
                    continue;
                }
                let Some(mut expr) = raw.strip_prefix('=').and_then(|f| parse(f).ok()) else {
                    continue;
                };
                let mut changed = false;
                for change in changes {
                    changed |= adjust(&mut expr, &sheet.name, change);
                }
                if changed {
                    cells.push((at, Some(format!("={}", render(&expr)).into_boxed_str())));
                }
            }
            if !cells.is_empty() {
                out.push((sheet.id, Edit::Cells(cells)));
            }
        }
        (!cancel.load(Ordering::Relaxed)).then_some(out)
    }
}

fn insert(sheet: &str, axis: Axis, at: u64, count: u64) -> Change<'_> {
    Change::Insert {
        sheet,
        axis,
        at,
        count,
    }
}

fn delete(sheet: &str, axis: Axis, span: Range<u64>) -> Change<'_> {
    Change::Delete {
        sheet,
        axis,
        at: span.start,
        count: span.end - span.start,
    }
}

impl CsvTable {
    /// The formula edits that go with putting rows in order `new` (a sort's
    /// [`Edit::Reorder`], not applied yet): a formula moved by `n` rows shifts its
    /// relative row references by `n`, as if it were cut and pasted (Excel's rule), so
    /// `=B2*2` keeps reading its own row. References into rows that didn't move are not
    /// followed. The edits key cells by id: apply them after the reorder, in the same step.
    /// Scans the file for formulas first if that hasn't been done.
    pub fn sort_rewrites(&self, new: &Option<RowOrder>) -> Vec<(CellRef, Option<Box<str>>)> {
        let scanned;
        let file: &[(u64, u32)] = if self.formulas.complete {
            &self.formulas.file
        } else {
            scanned = self.scan_formulas(&AtomicBool::new(false), &AtomicU64::new(0));
            match &scanned {
                Some(found) => &found.file,
                None => return Vec::new(), // not indexed: there is no sort either
            }
        };
        let after = RowPositions::of(new.as_ref(), self.index.row_count());
        let mut out = Vec::new();
        for (at, (before, _)) in self.formula_cells_in(file) {
            let Some(now) = after.get(at.row).filter(|&p| p != before) else {
                continue;
            };
            let Some(raw) = self.cell_of(at.row, at.col) else {
                continue;
            };
            let Some(mut expr) = raw.strip_prefix('=').and_then(|f| parse(f).ok()) else {
                continue;
            };
            let shift = Change::Shift {
                rows: now as i64 - before as i64,
                cols: 0,
            };
            if adjust(&mut expr, "", &shift) {
                out.push((at, Some(format!("={}", render(&expr)).into_boxed_str())));
            }
        }
        out
    }
}
