//! Row and column inserts and deletes as reversible commands (EDIT-3, EDIT-4). Like
//! [`crate::SetCell`], the command holds one [`Edit`] and swaps it for the inverse the
//! table returns: a delete's inverse holds the removed ids, so undo brings the rows or
//! columns back with their edits.

use crate::Command;
use data_model::{CellRef, CsvTable, Edit};

pub struct Reshape {
    edit: Edit,
    label: &'static str,
    /// The cursor cell when the command was made: the other coordinate of the focus.
    cursor: CellRef,
    /// First row or column put back in by the last run (an insert, or the undo of a
    /// delete).
    focus: Option<CellRef>,
}

impl Reshape {
    /// Wrap an edit from `CsvTable::insert_rows`, `delete_rows`, `insert_cols`, or
    /// `delete_cols`.
    pub fn new(edit: Edit, cursor: CellRef) -> Self {
        let label = match edit {
            Edit::InsertRows { .. } => "Insert rows",
            Edit::DeleteRows { .. } => "Delete rows",
            Edit::InsertCols { .. } => "Insert columns",
            Edit::DeleteCols { .. } | Edit::Cell { .. } => "Delete columns",
        };
        Self {
            edit,
            label,
            cursor,
            focus: None,
        }
    }

    fn swap(&mut self, table: &mut CsvTable) {
        let edit = std::mem::replace(&mut self.edit, Edit::none());
        self.focus = match &edit {
            Edit::InsertRows { rows, .. } => rows.first().map(|r| CellRef {
                row: r.first,
                ..self.cursor
            }),
            Edit::InsertCols { cols, .. } => {
                cols.first().map(|&col| CellRef { col, ..self.cursor })
            }
            _ => None,
        };
        self.edit = table.apply(edit);
    }
}

impl Command<CsvTable> for Reshape {
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
