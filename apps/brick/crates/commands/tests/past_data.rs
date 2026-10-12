//! GRID-6: the grid reaches past the data, so a cell can be typed or pasted well below the
//! last row. That adds the empty rows up to it and sets the cell, as one undo step, and
//! the save writes them.

use commands::{Batch, UndoStack};
use data_model::{CellRef, CsvTable, DelimiterChoice, SaveProgress, TableSource};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

struct Tmp(PathBuf);

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn open(name: &str, csv: &str) -> (Tmp, CsvTable) {
    let path = std::env::temp_dir().join(format!("grid6-{}-{name}.csv", std::process::id()));
    std::fs::write(&path, csv).unwrap();
    let t = CsvTable::open(&path, DelimiterChoice::Auto).unwrap();
    t.index().build(&AtomicBool::new(false)).unwrap();
    (Tmp(path), t)
}

fn saved(t: &CsvTable) -> String {
    let mut out = Vec::new();
    t.save_job()
        .unwrap()
        .write_to(&mut out, &SaveProgress::default(), &AtomicBool::new(false))
        .unwrap();
    String::from_utf8(out).unwrap()
}

/// Puts `text` at (`row`, `col`) as the grid does: a paste of one cell, one undo step.
fn put(t: &mut CsvTable, undo: &mut UndoStack<CsvTable>, row: u64, col: u32, text: &str) {
    let edits = t.paste(row, col, &[vec![text.to_owned()]]).unwrap();
    let at = CellRef {
        row: t.row_id(0),
        col: t.col_id(0),
    };
    undo.execute(Box::new(Batch::new("Edit cell", edits, at)), t);
}

#[test]
fn a_cell_far_past_the_last_row_adds_the_rows_up_to_it() {
    let csv = "a,b\n1,2\n3,4\n";
    let (_tmp, mut t) = open("far", csv);
    let mut undo = UndoStack::default();
    let rows = t.row_count();
    // E40 as the gutter counts it: 37 rows past the last one, column E.
    put(&mut t, &mut undo, rows + 36, 4, "x");
    assert_eq!(t.row_count(), rows + 37);
    let mut want = String::from(csv);
    want.push_str(&",\n".repeat(36));
    want.push_str(",,,,x\n");
    assert_eq!(saved(&t), want);
    assert_eq!(t.cell_value(rows + 36, 4).as_deref(), Some("x"));
    undo.undo(&mut t).unwrap();
    assert_eq!(t.row_count(), rows, "one step takes the rows back");
    assert_eq!(saved(&t), csv);
}

#[test]
fn a_blank_sheet_grows_to_a_cell_typed_anywhere() {
    let mut t = CsvTable::untitled();
    let mut undo = UndoStack::default();
    put(&mut t, &mut undo, 4, 2, "hi");
    assert_eq!(t.row_count(), 5);
    assert_eq!(t.cell_value(4, 2).as_deref(), Some("hi"));
    assert_eq!(t.cell_value(3, 2), None);
    undo.undo(&mut t).unwrap();
    assert_eq!(t.row_count(), 0);
}
