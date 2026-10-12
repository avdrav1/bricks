//! Several edits as one command, so one undo step (CLIP-2's paste: rows added at the end,
//! then many cells set). Each run applies the edits in order and keeps their inverses in
//! the opposite order, ready for the next run, which undoes them last-first.

use crate::Command;
use data_model::{CellRef, CsvTable, Edit};

pub struct Batch {
    edits: Vec<Edit>,
    label: &'static str,
    /// The cell to show after it runs or is undone.
    focus: CellRef,
}

impl Batch {
    pub fn new(label: &'static str, edits: Vec<Edit>, focus: CellRef) -> Self {
        Self {
            edits,
            label,
            focus,
        }
    }

    fn swap(&mut self, table: &mut CsvTable) {
        let mut inverses: Vec<Edit> = std::mem::take(&mut self.edits)
            .into_iter()
            .map(|e| table.apply(e))
            .collect();
        inverses.reverse();
        self.edits = inverses;
    }
}

impl Command<CsvTable> for Batch {
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
        Some(self.focus)
    }

    fn heap_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.edits.capacity() * std::mem::size_of::<Edit>()
            + self.edits.iter().map(Edit::heap_bytes).sum::<usize>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UndoStack;
    use data_model::{parse_tsv, DelimiterChoice, PasteError, RowBlock, TableSource};
    use std::sync::atomic::AtomicBool;

    fn rows(table: &mut CsvTable) -> Vec<Vec<String>> {
        let n = table.row_count();
        let mut block = RowBlock::full_text();
        table.read_rows(0..n, &mut block);
        (0..n)
            .map(|r| {
                (0..block.cells_in_row(r))
                    .map(|c| block.cell(r, c).unwrap().to_owned())
                    .collect()
            })
            .collect()
    }

    fn row(cells: &[&str]) -> Vec<String> {
        cells.iter().map(|s| s.to_string()).collect()
    }

    /// CLIP-2: a paste lands raw at the cursor, makes short rows longer, adds rows at the
    /// end when it runs past the last one, and is one undo step; redo pastes it again.
    #[test]
    fn paste_expands_the_table_and_undoes_in_one_step() {
        let path = std::env::temp_dir().join(format!("commands-paste-{}.csv", std::process::id()));
        std::fs::write(&path, "a,b,c\n1,2,3\n4,5\n").unwrap();
        let mut table = CsvTable::open(&path, DelimiterChoice::Auto).unwrap();
        table.index().build(&AtomicBool::new(false)).unwrap();
        let before = rows(&mut table);
        let mut undo = UndoStack::default();

        let clip = parse_tsv("x\t00123\ty\n5\t=1+1\tline\"q\"\n\"two\nlines\"\t\tz\n");
        let edits = table.paste(1, 1, &clip).unwrap();
        let focus = CellRef {
            row: table.row_id(1),
            col: table.col_id(1),
        };
        undo.execute(Box::new(Batch::new("Paste", edits, focus)), &mut table);
        let pasted = vec![
            row(&["1", "2", "3"]),
            row(&["4", "x", "00123", "y"]),
            row(&["", "5", "=1+1", "line\"q\""]),
            row(&["", "two\nlines", "", "z"]),
        ];
        assert_eq!(rows(&mut table), pasted);

        let back = undo.undo(&mut table).unwrap();
        assert_eq!(back.focus(), Some(focus));
        assert_eq!(
            rows(&mut table),
            before,
            "one undo takes the whole paste back"
        );
        assert_eq!(table.changes(), 0);
        undo.redo(&mut table).unwrap();
        assert_eq!(rows(&mut table), pasted);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_paste_past_the_end_waits_for_indexing_and_size_is_bounded() {
        let path = std::env::temp_dir().join(format!("commands-paste2-{}.csv", std::process::id()));
        std::fs::write(&path, "a\n1\n").unwrap();
        let mut table = CsvTable::open(&path, DelimiterChoice::Auto).unwrap();
        let two = parse_tsv("x\ny\nz\n");
        assert_eq!(table.paste(0, 0, &two), Err(PasteError::NotIndexed));
        table.index().build(&AtomicBool::new(false)).unwrap();
        let wide = vec![vec![String::new(); 1 << 11]; 1 << 10];
        assert!(table.paste(0, 0, &wide).is_ok(), "exactly the limit");
        let too_wide = vec![vec![String::new(); (1 << 11) + 1]; 1 << 10];
        assert_eq!(
            table.paste(0, 0, &too_wide),
            Err(PasteError::TooLarge((1 << 21) + (1 << 10)))
        );
        std::fs::remove_file(path).unwrap();
    }
}
