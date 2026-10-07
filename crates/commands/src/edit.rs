//! Cell edits as reversible commands (invariant 5). The command stores the operation, not a
//! snapshot: one [`Edit`], swapped for its inverse each time it runs.

use crate::Command;
use data_model::{CellRef, CsvTable, Edit};

/// Set one cell's raw value, or clear its edit so the source value shows again.
/// Applying and reverting are the same swap: [`CsvTable::apply`] returns the inverse.
pub struct SetCell {
    edit: Edit,
}

impl SetCell {
    pub fn new(at: CellRef, value: impl Into<Box<str>>) -> Self {
        Self {
            edit: Edit::set(at, value),
        }
    }

    pub fn clear(at: CellRef) -> Self {
        Self {
            edit: Edit::clear(at),
        }
    }

    fn swap(&mut self, table: &mut CsvTable) {
        // The placeholder is never applied; it only stands in while `apply` runs.
        let at = self.edit.cell();
        let edit = std::mem::replace(&mut self.edit, Edit::clear(at));
        self.edit = table.apply(edit);
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

    fn focus(&self) -> Option<CellRef> {
        Some(self.edit.cell())
    }

    fn heap_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.edit.heap_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UndoStack;
    use csv_engine::{Dialect, Source, SparseRowIndex};
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    /// Every cell's value, row by row: what a user could see.
    fn snapshot(table: &CsvTable, rows: u64, cols: u32) -> Vec<Option<String>> {
        (0..rows)
            .flat_map(|r| (0..cols).map(move |c| (r, c)))
            .map(|(r, c)| table.cell_value(r, c).map(|v| v.into_owned()))
            .collect()
    }

    /// CMD-1 acceptance: every edit is a reversible command object. A random run of sets
    /// and clears, some on the same cells, some past the end of short rows, goes through
    /// the undo stack; undoing any number of steps gives back exactly the table as it was
    /// before them, and redoing all of them gives back the end state.
    #[test]
    fn every_edit_reverts_exactly() {
        let path = std::env::temp_dir().join(format!("commands-cmd1-{}.csv", std::process::id()));
        std::fs::write(&path, "a,b,c\n1,2,3\n4,5\n6\n7,8,9\n").unwrap();
        let index =
            SparseRowIndex::new(Arc::new(Source::open(&path).unwrap()), &Dialect::default());
        index.build(&AtomicBool::new(false)).unwrap();
        let mut table = CsvTable::new(index, Dialect::default());
        let (rows, cols) = (5, 5);
        let mut undo = UndoStack::default();

        let (mut states, mut cells) = (vec![snapshot(&table, rows, cols)], Vec::new());
        let mut x = 0x2545_f491_4f6c_dd1du64; // xorshift: the same run every time
        for step in 0..300 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let at = CellRef {
                row: table.row_id(x % rows),
                col: ((x >> 8) % u64::from(cols)) as u32,
            };
            let cmd = if (x >> 16) % 4 == 0 {
                SetCell::clear(at)
            } else {
                SetCell::new(at, format!("v{step}"))
            };
            undo.execute(Box::new(cmd), &mut table);
            states.push(snapshot(&table, rows, cols));
            cells.push(at);
        }

        for (before, at) in states.iter().rev().skip(1).zip(cells.iter().rev()) {
            let undone = undo.undo(&mut table).expect("one undo per edit");
            assert_eq!(undone.focus(), Some(*at), "undo shows the cell it changed");
            assert_eq!(&snapshot(&table, rows, cols), before);
        }
        assert!(undo.undo(&mut table).is_none(), "nothing left to undo");
        assert_eq!(table.overlay().len(), 0, "no edit left behind");
        while undo.redo(&mut table).is_some() {}
        assert_eq!(&snapshot(&table, rows, cols), states.last().unwrap());
        std::fs::remove_file(path).unwrap();
    }
}
