//! Cell edits as reversible commands (invariant 5). The command stores the operation, not a
//! snapshot: the cell, and the value to swap in.

use crate::Command;
use data_model::{CellRef, CsvTable};

/// Set one cell's raw value, or with `None` clear its edit so the source value shows again.
/// Applying and reverting are the same swap: each stores the value it replaced.
pub struct SetCell {
    at: CellRef,
    value: Option<Box<str>>,
}

impl SetCell {
    pub fn new(at: CellRef, value: impl Into<Box<str>>) -> Self {
        Self {
            at,
            value: Some(value.into()),
        }
    }

    fn swap(&mut self, table: &mut CsvTable) {
        self.value = match self.value.take() {
            Some(v) => table.set_cell(self.at, v),
            None => table.clear_cell(self.at),
        };
    }
}

impl Command<CsvTable> for SetCell {
    fn apply(&mut self, table: &mut CsvTable) {
        self.swap(table);
    }

    fn revert(&mut self, table: &mut CsvTable) {
        self.swap(table);
    }

    fn label(&self) -> &str {
        "Edit cell"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UndoStack;
    use csv_engine::{Dialect, Source, SparseRowIndex};
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    #[test]
    fn undo_and_redo_swap_values_through_edit_layers() {
        let path = std::env::temp_dir().join(format!("commands-test-{}.csv", std::process::id()));
        std::fs::write(&path, "a,b\n1,2\n").unwrap();
        let index =
            SparseRowIndex::new(Arc::new(Source::open(&path).unwrap()), &Dialect::default());
        index.build(&AtomicBool::new(false)).unwrap();
        let mut table = CsvTable::new(index, Dialect::default());
        let at = CellRef {
            row: table.row_id(1),
            col: 0,
        };
        let mut undo = UndoStack::new(100);

        undo.execute(Box::new(SetCell::new(at, "x")), &mut table);
        undo.execute(Box::new(SetCell::new(at, "y")), &mut table);
        assert_eq!(table.cell_value(1, 0).as_deref(), Some("y"));
        assert!(undo.undo(&mut table));
        assert_eq!(table.cell_value(1, 0).as_deref(), Some("x"));
        assert!(undo.undo(&mut table));
        assert_eq!(
            table.cell_value(1, 0).as_deref(),
            Some("1"),
            "back to the source value"
        );
        assert_eq!(table.overlay().len(), 0);
        assert!(undo.redo(&mut table));
        assert!(undo.redo(&mut table));
        assert_eq!(table.cell_value(1, 0).as_deref(), Some("y"));
        std::fs::remove_file(path).unwrap();
    }
}
