//! Merged cells (STYLE-4, ADR 0009): the table's merged ranges, in all-rows positions
//! (the header row included) and view columns. Merge and Unmerge replace the list with
//! one [`Edit::Merges`]; row and column inserts and deletes move, grow, or shrink merges
//! by Excel's rules in the same undo step ([`CsvTable::merges_after`]).

use crate::rewrites::structure_changes;
use crate::{CellArea, CellRef, Col, CsvTable, Edit, Row, RowBlock, TableSource};
use formula::{Axis, Change};
use std::ops::{Range, RangeInclusive};
use std::sync::Arc;

/// Cells one Merge reads at most: it looks at every cell of the range for text it would
/// drop, on the UI thread, so the range stays small (invariant 4).
pub const MERGE_MAX_CELLS: u64 = 1 << 16;

/// Rows read at a time while looking for text a merge drops.
const CHUNK: u64 = 4_096;

/// What merging a range does: the edits, as one undo step, and the area merged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergePlan {
    /// The area merged: the range asked for, grown over the merges it touches.
    pub area: CellArea,
    /// [`Edit::Merges`], then the other cells' text emptied (if any).
    pub edits: Vec<Edit>,
    /// Cells other than the top-left one hold text, which the merge empties: ask first.
    pub drops_text: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeError {
    /// The range is one cell: there is nothing to merge.
    OneCell,
    /// The range has this many cells, more than [`MERGE_MAX_CELLS`].
    TooManyCells(u64),
    /// Rows are sorted or filtered: merges are kept in file order, so they wait.
    NotInFileOrder,
}

impl CsvTable {
    /// The merged ranges, sorted by first row then column; they never overlap.
    pub fn merges(&self) -> &[CellArea] {
        &self.merges
    }

    /// The merge covering the cell at `row` (a position among all rows) and view column
    /// `col`, if any.
    pub fn merge_at(&self, row: u64, col: Col) -> Option<CellArea> {
        self.merges_in(row..=row, col..=col).next()
    }

    /// The merges that intersect rows `rows` (positions among all rows) and columns
    /// `cols`, in order. Finds the first by binary search: files can have thousands.
    pub fn merges_in(
        &self,
        rows: RangeInclusive<u64>,
        cols: RangeInclusive<Col>,
    ) -> impl Iterator<Item = CellArea> + '_ {
        let (top, bottom) = (*rows.start(), *rows.end());
        let from = top.saturating_sub(self.merge_reach);
        let start = self.merges.partition_point(|m| m.first_row < from);
        self.merges[start..]
            .iter()
            .take_while(move |m| m.first_row <= bottom)
            .filter(move |m| {
                m.last_row >= top && m.first_col <= *cols.end() && m.last_col >= *cols.start()
            })
            .copied()
    }

    /// The table rows that show merge `m`'s rows: a range, since table rows keep the
    /// order of positions. `None` when none show (the header row, or hidden by filters).
    pub fn merge_rows(&self, m: &CellArea) -> Option<Range<Row>> {
        let rows = match &self.visible {
            Some(v) => {
                let (a, b) = (m.first_row, m.last_row.saturating_add(1));
                v.partition_point(|&p| u64::from(p) < a) as u64
                    ..v.partition_point(|&p| u64::from(p) < b) as u64
            }
            None => {
                let first = self.first_row();
                let end = self.row_count();
                m.first_row.saturating_sub(first).min(end)
                    ..(m.last_row + 1).saturating_sub(first).min(end)
            }
        };
        (!rows.is_empty()).then_some(rows)
    }

    /// A sort would move rows a merge covers: refuse it ("Unmerge cells before sorting").
    /// Only merges entirely in the header row stay out of a sort's way.
    pub fn merges_block_sort(&self) -> bool {
        let first = self.first_row();
        self.merges.iter().any(|m| m.last_row >= first)
    }

    /// The merges after `change` (a row or column insert or delete on this table), by
    /// Excel's rules: an insert at or above a merge moves it, one inside grows it; a
    /// delete of part shrinks it, and a delete of all of it, or one that leaves a single
    /// cell, removes it. `None` when no merge changes.
    pub fn adjusted_merges(&self, change: &Change) -> Option<Vec<CellArea>> {
        adjust_list(&self.merges, change)
    }

    /// The [`Edit::Merges`] that goes with structural `edit` (from [`Self::insert_rows`],
    /// [`Self::delete_rows`], [`Self::insert_cols`], or [`Self::delete_cols`]) in the same
    /// undo step; `None` when no merge moves. The list doesn't depend on the row order, so
    /// it can apply before or after `edit`.
    pub fn merges_after(&self, edit: &Edit) -> Option<Edit> {
        let (changes, _) = structure_changes("", edit)?;
        let mut list: Option<Vec<CellArea>> = None;
        for change in &changes {
            let now = list.as_deref().unwrap_or(&self.merges);
            if let Some(next) = adjust_list(now, change) {
                list = Some(next);
            }
        }
        list.map(|l| Edit::Merges(Arc::new(l)))
    }

    /// Merge `area` (positions among all rows, view columns) as one undo step: merges it
    /// touches are absorbed (the area grows to cover them), and every non-empty cell but
    /// the top-left one is emptied. Refused while rows are sorted or filtered.
    pub fn merge_plan(&mut self, area: CellArea) -> Result<MergePlan, MergeError> {
        if self.is_sorted() || self.is_filtered() {
            return Err(MergeError::NotInFileOrder);
        }
        let mut area = area;
        loop {
            let mut grown = area;
            for m in self.merges_in(
                area.first_row..=area.last_row,
                area.first_col..=area.last_col,
            ) {
                grown.first_row = grown.first_row.min(m.first_row);
                grown.last_row = grown.last_row.max(m.last_row);
                grown.first_col = grown.first_col.min(m.first_col);
                grown.last_col = grown.last_col.max(m.last_col);
            }
            if grown == area {
                break;
            }
            area = grown;
        }
        let rows = area.last_row - area.first_row + 1;
        let cells = rows.saturating_mul(u64::from(area.last_col - area.first_col) + 1);
        if cells == 1 {
            return Err(MergeError::OneCell);
        }
        if cells > MERGE_MAX_CELLS {
            return Err(MergeError::TooManyCells(cells));
        }
        let end = (area.last_row + 1).min(self.total_rows());
        let mut cleared = Vec::new();
        let mut block = RowBlock::default();
        let mut at = area.first_row;
        while at < end {
            let to = (at + CHUNK).min(end);
            self.read_all(at..to, &mut block);
            for row in at..to {
                for col in area.first_col..=area.last_col {
                    let top_left = row == area.first_row && col == area.first_col;
                    if !top_left && block.cell(row, col).is_some_and(|t| !t.is_empty()) {
                        let at = CellRef {
                            row: self.id_at(row),
                            col: self.col_id(col),
                        };
                        cleared.push((at, Some(Box::<str>::from(""))));
                    }
                }
            }
            at = to;
        }
        let mut list: Vec<CellArea> = self
            .merges
            .iter()
            .filter(|m| !intersects(m, &area))
            .copied()
            .collect();
        let i =
            list.partition_point(|m| (m.first_row, m.first_col) < (area.first_row, area.first_col));
        list.insert(i, area);
        let drops_text = !cleared.is_empty();
        let mut edits = vec![Edit::Merges(Arc::new(list))];
        if drops_text {
            edits.push(Edit::Cells(cleared));
        }
        Ok(MergePlan {
            area,
            edits,
            drops_text,
        })
    }

    /// Unmerge every merge that intersects `area`, as one [`Edit::Merges`]; `None` if
    /// none does. The cells keep their text (only the top-left one has any after a merge).
    pub fn unmerge(&self, area: CellArea) -> Option<Edit> {
        let list: Vec<CellArea> = self
            .merges
            .iter()
            .filter(|m| !intersects(m, &area))
            .copied()
            .collect();
        (list.len() != self.merges.len()).then(|| Edit::Merges(Arc::new(list)))
    }

    /// Replace the merged ranges; returns the list before.
    pub(crate) fn put_merges(&mut self, list: Arc<Vec<CellArea>>) -> Arc<Vec<CellArea>> {
        self.merge_reach = list
            .iter()
            .map(|m| m.last_row - m.first_row)
            .max()
            .unwrap_or(0);
        std::mem::replace(&mut self.merges, list)
    }
}

fn intersects(a: &CellArea, b: &CellArea) -> bool {
    a.first_row <= b.last_row
        && b.first_row <= a.last_row
        && a.first_col <= b.last_col
        && b.first_col <= a.last_col
}

/// `list` after `change`; `None` if nothing changes. The maps keep order and keep merges
/// apart, so the result stays sorted.
fn adjust_list(list: &[CellArea], change: &Change) -> Option<Vec<CellArea>> {
    let (axis, at, count, insert) = match *change {
        Change::Insert {
            axis, at, count, ..
        } => (axis, at, count, true),
        Change::Delete {
            axis, at, count, ..
        } => (axis, at, count, false),
        _ => return None,
    };
    if count == 0 {
        return None;
    }
    let span = |s: u64, e: u64| -> Option<(u64, u64)> {
        if insert {
            let moved = |p: u64| if p >= at { p + count } else { p };
            return Some((moved(s), moved(e)));
        }
        let end = at + count;
        let ns = if s >= end {
            s - count
        } else if s >= at {
            at
        } else {
            s
        };
        let ne = if e >= end {
            e - count
        } else if e >= at {
            at.checked_sub(1)?
        } else {
            e
        };
        (ne >= ns).then_some((ns, ne))
    };
    let mut changed = false;
    let mut out = Vec::with_capacity(list.len());
    for m in list {
        let mut n = *m;
        let kept = match axis {
            Axis::Rows => span(m.first_row, m.last_row).map(|(s, e)| {
                (n.first_row, n.last_row) = (s, e);
            }),
            Axis::Cols => span(u64::from(m.first_col), u64::from(m.last_col)).map(|(s, e)| {
                (n.first_col, n.last_col) = (s as Col, e as Col);
            }),
        };
        changed |= kept.is_none() || n != *m;
        if kept.is_some() && (n.first_row != n.last_row || n.first_col != n.last_col) {
            out.push(n);
        } else if kept.is_some() {
            changed = true; // shrunk to one cell: no longer a merge
        }
    }
    changed.then_some(out)
}
