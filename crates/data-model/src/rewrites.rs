//! Formulas that follow the workbook's structure (SHEET-2): when a sheet is renamed or
//! deleted, every formula naming it is rewritten. The rewrites are plain cell edits, so
//! the caller applies them together with the layout change as one undoable step.

use crate::workbook::same_name;
use crate::{Edit, SheetId, Workbook};
use formula::{adjust, parse, render, Change};
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
