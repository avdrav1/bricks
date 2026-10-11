//! What saving by patching the original file needs to know about a table (STYLE-5,
//! ADR 0010): which of the file's cells changed, and where the file's rows and columns
//! are now.

use crate::{CellRef, ColId, CsvTable, RowId, RowOrder};
use std::borrow::Cow;
use std::collections::BTreeMap;

impl CsvTable {
    /// Rows and columns are still in file order: nothing inserted, deleted, or sorted.
    pub fn in_file_order(&self) -> bool {
        self.rows.is_none() && self.cols.is_none()
    }

    /// The rows were sorted: their order no longer follows the file's.
    pub fn is_sorted(&self) -> bool {
        matches!(self.rows, Some(RowOrder::Sorted(_)))
    }

    /// The raw text of the cell `at` (edits included); `None` past its row's end.
    pub fn cell_at(&self, at: CellRef) -> Option<Cow<'_, str>> {
        self.cell_of(at.row, at.col)
    }

    /// The cell's text differs from the file's: it was edited or cleared.
    pub fn is_edited(&self, at: CellRef) -> bool {
        self.overlay.get(at).is_some()
            || (!at.row.is_inserted()
                && self
                    .cleared
                    .at(at.row.0)
                    .is_some_and(|c| c.contains(at.col)))
    }

    /// Edited cells of the file's rows and columns, by source row, columns sorted: the
    /// cells a patch writes beside or over the file's own.
    pub fn file_edits(&self) -> BTreeMap<u64, Vec<u32>> {
        let mut out: BTreeMap<u64, Vec<u32>> = BTreeMap::new();
        for (at, _) in self.overlay.cells() {
            if !at.row.is_inserted() && !at.col.is_inserted() {
                out.entry(at.row.0).or_default().push(at.col.0);
            }
        }
        for cols in out.values_mut() {
            cols.sort_unstable();
        }
        out
    }

    /// Where file rows `first..=last` are now, as positions among all rows: from the
    /// first of them still there to the last, taking in rows inserted between. After a
    /// sort, rows keep their positions (Excel's rule for ranges). `None` once all of them
    /// are deleted.
    pub fn file_rows_now(&self, first: u64, last: u64) -> Option<(u64, u64)> {
        if self.rows.is_none() || self.is_sorted() {
            return Some((first, last));
        }
        let at = |r: u64| self.position_of(RowId::source(r));
        let start = (first..=last).find_map(at)?;
        let end = (first..=last).rev().find_map(at)?;
        Some((start, end))
    }

    /// How many view columns the file's first `file_cols` columns span now, inserted
    /// columns among them included.
    pub fn view_cols(&self, file_cols: u32) -> u32 {
        match &self.cols {
            None => file_cols,
            Some(m) => m.width(file_cols, |id| id.is_inserted()),
        }
    }

    /// Where file columns `first..=last` are now (view columns), as for rows.
    pub fn file_cols_now(&self, first: u32, last: u32) -> Option<(u32, u32)> {
        if self.cols.is_none() {
            return Some((first, last));
        }
        let at = |c: u32| self.col_of(ColId::source(c));
        let start = (first..=last).find_map(at)?;
        let end = (first..=last).rev().find_map(at)?;
        Some((start, end))
    }
}
