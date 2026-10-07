//! Row inserts and deletes as reversible commands (EDIT-3). Like [`crate::SetCell`], the
//! command holds one [`Edit`] and swaps it for the inverse the table returns: a delete's
//! inverse holds the removed rows' ids, so undo brings them back with their edits.

use crate::Command;
use data_model::{CellRef, Col, CsvTable, Edit};

pub struct ChangeRows {
    edit: Edit,
    label: &'static str,
    /// Column to show the affected rows at.
    col: Col,
    /// First row put back in by the last run (an insert, or the undo of a delete).
    focus: Option<CellRef>,
}

impl ChangeRows {
    /// Wrap an edit from [`CsvTable::insert_rows`] or [`CsvTable::delete_rows`].
    pub fn new(edit: Edit, col: Col) -> Self {
        let label = match edit {
            Edit::InsertRows { .. } => "Insert rows",
            _ => "Delete rows",
        };
        Self {
            edit,
            label,
            col,
            focus: None,
        }
    }

    fn swap(&mut self, table: &mut CsvTable) {
        let edit = std::mem::replace(&mut self.edit, Edit::none());
        self.focus = match &edit {
            Edit::InsertRows { rows, .. } => rows.first().map(|r| CellRef {
                row: r.first,
                col: self.col,
            }),
            _ => None,
        };
        self.edit = table.apply(edit);
    }
}

impl Command<CsvTable> for ChangeRows {
    fn apply(&mut self, table: &mut CsvTable) {
        self.swap(table);
    }

    fn revert(&mut self, table: &mut CsvTable) {
        self.swap(table);
    }

    fn label(&self) -> &str {
        self.label
    }

    fn focus(&self) -> Option<CellRef> {
        self.focus
    }

    fn heap_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.edit.heap_bytes()
    }
}
