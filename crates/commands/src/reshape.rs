//! Row and column inserts and deletes as reversible commands (EDIT-3, EDIT-4). Like
//! [`crate::SetCell`], the command holds one [`Edit`] and swaps it for the inverse the
//! table returns: a delete's inverse holds the removed ids, so undo brings the rows or
//! columns back with their edits. Merged cells that move with them (STYLE-4) come in the
//! same step.

use crate::Command;
use data_model::{CellRef, CsvTable, Edit};

pub struct Reshape {
    edit: Edit,
    /// The [`Edit::Merges`] that goes with it ([`CsvTable::merges_after`]), if any. The
    /// merge list doesn't depend on row order, so it swaps after `edit` both ways.
    merges: Option<Edit>,
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
        Self {
            label: Self::label_of(&edit),
            edit,
            merges: None,
            cursor,
            focus: None,
        }
    }

    /// The merges move with the rows or columns in the same step (STYLE-4).
    pub fn with_merges(mut self, merges: Option<Edit>) -> Self {
        self.merges = merges;
        self
    }

    /// The undo label of a row or column insert or delete.
    pub fn label_of(edit: &Edit) -> &'static str {
        match edit {
            Edit::InsertRows { .. } => "Insert rows",
            Edit::DeleteRows { .. } | Edit::DeleteRowRanges(_) => "Delete rows",
            Edit::InsertCols { .. } => "Insert columns",
            _ => "Delete columns",
        }
    }

    fn swap(&mut self, table: &mut CsvTable) {
        let edit = std::mem::replace(&mut self.edit, Edit::none());
        self.focus =
            match &edit {
                Edit::InsertRows { rows, .. } => rows.first().map(|r| CellRef {
                    row: r.first,
                    ..self.cursor
                }),
                Edit::InsertRowRanges(parts) => parts
                    .first()
                    .and_then(|(_, runs)| runs.first())
                    .map(|r| CellRef {
                        row: r.first,
                        ..self.cursor
                    }),
                Edit::InsertCols { cols, .. } => {
                    cols.first().map(|&col| CellRef { col, ..self.cursor })
                }
                _ => None,
            };
        self.edit = table.apply(edit);
        if let Some(merges) = self.merges.take() {
            self.merges = Some(table.apply(merges));
        }
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
        std::mem::size_of::<Self>()
            + self.edit.heap_bytes()
            + self.merges.as_ref().map_or(0, Edit::heap_bytes)
    }
}
