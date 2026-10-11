//! Delete on a selection (EDIT-5): one command, so one undo step, however many cells it
//! empties. The command holds the table's compact [`Edit::ClearCells`] (rows × columns,
//! not a value per cell) and swaps it for the inverse, which also keeps the edits the
//! clear removed.

use crate::Command;
use data_model::{CellRef, CsvTable, Edit};

pub struct ClearCells {
    edit: Edit,
    /// The cursor cell when Delete was pressed: shown again on undo and redo.
    cursor: CellRef,
}

impl ClearCells {
    /// Wrap an edit from `CsvTable::clear_cells`.
    pub fn new(edit: Edit, cursor: CellRef) -> Self {
        Self { edit, cursor }
    }

    fn swap(&mut self, table: &mut CsvTable) {
        let edit = std::mem::replace(&mut self.edit, Edit::none());
        self.edit = table.apply(edit);
    }
}

impl Command<CsvTable> for ClearCells {
    fn apply(&mut self, table: &mut CsvTable) {
        self.swap(table);
    }

    fn revert(&mut self, table: &mut CsvTable) {
        self.swap(table);
    }

    fn label(&self) -> &str {
        "Clear cells"
    }

    fn focus(&self) -> Option<CellRef> {
        Some(self.cursor)
    }

    fn heap_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.edit.heap_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SetCell, UndoStack};
    use data_model::{DelimiterChoice, RowBlock, TableSource};

    fn rows(table: &mut CsvTable) -> Vec<Vec<String>> {
        let n = table.row_count();
        let mut block = RowBlock::default();
        table.read_rows(0..n, &mut block);
        (0..n)
            .map(|r| {
                (0..block.cells_in_row(r))
                    .map(|c| block.cell(r, c).unwrap().to_owned())
                    .collect()
            })
            .collect()
    }

    /// EDIT-5 acceptance: Delete on a selection is one undo step. A 2 × 2 clear over a
    /// cell edited before it empties all four cells; one undo brings back every value,
    /// the edit included, and one redo empties them again. A cell typed into after the
    /// clear shows its new value, and undoing that shows the cleared cell again.
    #[test]
    fn delete_clears_the_selection_and_undoes_in_one_step() {
        let path = std::env::temp_dir().join(format!("commands-clear-{}.csv", std::process::id()));
        std::fs::write(&path, "a,b,c\n1,2,3\n4,5,6\n7,8,9\n").unwrap();
        let mut table = CsvTable::open(&path, DelimiterChoice::Auto).unwrap();
        table
            .index()
            .build(&std::sync::atomic::AtomicBool::new(false))
            .unwrap();
        let mut undo = UndoStack::default();
        let at = |t: &CsvTable, r, c| CellRef {
            row: t.row_id(r),
            col: t.col_id(c),
        };
        undo.execute(
            Box::new(SetCell::new(at(&table, 1, 1), "edited")),
            &mut table,
        );
        let before = rows(&mut table);

        let clear = table.clear_cells(0..2, 1..3).unwrap();
        let cursor = at(&table, 0, 1);
        undo.execute(Box::new(ClearCells::new(clear, cursor)), &mut table);
        let cleared = vec![
            vec!["1".to_owned(), String::new(), String::new()],
            vec!["4".to_owned(), String::new(), String::new()],
            vec!["7".to_owned(), "8".to_owned(), "9".to_owned()],
        ];
        assert_eq!(rows(&mut table), cleared);

        let back = undo.undo(&mut table).unwrap();
        assert_eq!(back.focus(), Some(cursor));
        assert_eq!(rows(&mut table), before, "one undo restores all four cells");
        undo.redo(&mut table).unwrap();
        assert_eq!(rows(&mut table), cleared, "one redo clears them again");

        undo.execute(Box::new(SetCell::new(at(&table, 0, 2), "new")), &mut table);
        assert_eq!(table.cell_value(0, 2).as_deref(), Some("new"));
        undo.undo(&mut table).unwrap();
        assert_eq!(table.cell_value(0, 2).as_deref(), Some(""));
        undo.undo(&mut table).unwrap();
        undo.undo(&mut table).unwrap();
        assert_eq!(table.changes(), 0, "nothing left after undoing everything");
        std::fs::remove_file(path).unwrap();
    }
}
