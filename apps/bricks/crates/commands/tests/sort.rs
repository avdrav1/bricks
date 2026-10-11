//! SORT-1: type-aware sort, ascending and descending; stable; undoable. The header row
//! stays put, rows move whole, and a sorted table saves its rows in the new order, each
//! byte for byte as it was.

use commands::{Batch, Reshape, SetCell, UndoStack};
use data_model::{
    CellRef, CsvTable, DelimiterChoice, RowBlock, SaveProgress, SortError, SortOrder, TableSource,
};
use std::sync::atomic::{AtomicBool, AtomicU64};

const CSV: &str = "\
id,name,score,note
1,ann,10,\"x, y\"
2,Bob,2,
3,cy,n/a,z
4,Ann,10,\"line one
line two\"
5,bob,,w
6,Alexandria,-3.5,v
7,alexandra,2,u
";

fn open(name: &str, text: &str) -> (std::path::PathBuf, CsvTable) {
    let path = std::env::temp_dir().join(format!("sort1-{}-{name}.csv", std::process::id()));
    std::fs::write(&path, text).unwrap();
    let mut t = CsvTable::open(&path, DelimiterChoice::Auto).unwrap();
    t.index().build(&AtomicBool::new(false)).unwrap();
    let types = t.infer_types(&AtomicBool::new(false)).unwrap();
    t.set_types(types);
    (path, t)
}

fn column(t: &mut CsvTable, col: u32) -> Vec<String> {
    let n = t.row_count();
    let mut b = RowBlock::full_text();
    t.read_rows(0..n, &mut b);
    (0..n)
        .map(|r| b.cell(r, col).unwrap_or("").to_owned())
        .collect()
}

fn sort(t: &mut CsvTable, undo: &mut UndoStack<CsvTable>, col: u32, order: SortOrder) -> bool {
    let edit = t
        .snapshot()
        .sort_rows(col, order, &AtomicBool::new(false), &AtomicU64::new(0))
        .unwrap();
    let Some(edit) = edit else { return false };
    let focus = CellRef {
        row: t.row_id(0),
        col: t.col_id(col),
    };
    undo.execute(Box::new(Batch::new("Sort", vec![edit], focus)), t);
    true
}

fn saved(t: &CsvTable) -> String {
    let mut out = Vec::new();
    t.save_job()
        .unwrap()
        .write_to(&mut out, &SaveProgress::default(), &AtomicBool::new(false))
        .unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn numbers_sort_by_value_with_misfits_then_blanks_and_ties_in_place() {
    let (path, mut t) = open("numbers", CSV);
    let mut undo = UndoStack::default();
    assert!(sort(&mut t, &mut undo, 2, SortOrder::Ascending));
    // 10 (ann) before 10 (Ann) and 2 (Bob) before 2 (Alexandra): their file order.
    assert_eq!(column(&mut t, 0), ["6", "2", "7", "1", "4", "3", "5"]);
    assert!(sort(&mut t, &mut undo, 2, SortOrder::Descending));
    assert_eq!(column(&mut t, 0), ["3", "1", "4", "2", "7", "6", "5"]);
    assert_eq!(
        t.header_cell(2).as_deref(),
        Some("score"),
        "the header stays"
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn text_ignores_case_compares_long_values_whole_and_sorts_stably() {
    let (path, mut t) = open("text", CSV);
    let mut undo = UndoStack::default();
    assert!(sort(&mut t, &mut undo, 1, SortOrder::Ascending));
    assert_eq!(
        column(&mut t, 1),
        ["alexandra", "Alexandria", "Ann", "ann", "Bob", "bob", "cy"],
        "alexandra/Alexandria share their first 8 bytes but for case: compared whole"
    );
    // Sorting by score now keeps that name order among equal scores.
    assert!(sort(&mut t, &mut undo, 2, SortOrder::Ascending));
    assert_eq!(
        column(&mut t, 1),
        ["Alexandria", "alexandra", "Bob", "Ann", "ann", "cy", "bob"]
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn undo_puts_the_rows_back_and_redo_sorts_again() {
    let (path, mut t) = open("undo", CSV);
    let mut undo = UndoStack::default();
    let before = column(&mut t, 0);
    assert_eq!(t.changes(), 0);
    assert!(sort(&mut t, &mut undo, 1, SortOrder::Descending));
    let sorted = column(&mut t, 0);
    assert_ne!(sorted, before);
    assert_eq!(t.changes(), 1, "a sorted order is an unsaved change");
    undo.undo(&mut t).unwrap();
    assert_eq!(column(&mut t, 0), before);
    assert_eq!(t.changes(), 0);
    assert_eq!(saved(&t), CSV, "an undone sort saves the file as it was");
    undo.redo(&mut t).unwrap();
    assert_eq!(column(&mut t, 0), sorted);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn a_sorted_table_saves_whole_rows_in_the_new_order() {
    let (path, mut t) = open("save", CSV);
    let mut undo = UndoStack::default();
    assert!(sort(&mut t, &mut undo, 2, SortOrder::Descending));
    let lines: Vec<&str> = CSV.split_inclusive('\n').collect();
    let row = |id: usize| -> String {
        // Row 4's note spans two lines.
        match id {
            4 => format!("{}{}", lines[4], lines[5]),
            id if id > 4 => lines[id + 1].to_owned(),
            id => lines[id].to_owned(),
        }
    };
    let want: String = std::iter::once(lines[0].to_owned())
        .chain([3, 1, 4, 2, 7, 6, 5].map(row))
        .collect();
    assert_eq!(saved(&t), want);

    // Edits, inserts, and deletes work on a sorted order and save with it.
    let at = CellRef {
        row: t.row_id(0),
        col: t.col_id(3),
    };
    undo.execute(Box::new(SetCell::new(at, "edited")), &mut t);
    let insert = t.insert_rows(1, 1).unwrap();
    undo.execute(Box::new(Reshape::new(insert, at)), &mut t);
    let delete = t.delete_rows(3, 1).unwrap(); // row 4
    undo.execute(Box::new(Reshape::new(delete, at)), &mut t);
    let want: String = [
        lines[0].to_owned(),
        "3,cy,n/a,edited\n".to_owned(),
        ",,,\n".to_owned(),
        row(1),
        row(2),
        row(7),
        row(6),
        row(5),
    ]
    .concat();
    assert_eq!(saved(&t), want);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn sorting_rows_already_in_order_changes_nothing() {
    let (path, mut t) = open("sorted", CSV);
    let mut undo = UndoStack::default();
    assert!(!sort(&mut t, &mut undo, 0, SortOrder::Ascending));
    assert!(sort(&mut t, &mut undo, 0, SortOrder::Descending));
    // Back to file order: the table is unchanged again.
    assert!(sort(&mut t, &mut undo, 0, SortOrder::Ascending));
    assert_eq!(t.changes(), 0);
    assert_eq!(saved(&t), CSV);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn a_cancelled_sort_returns_nothing() {
    let (path, t) = open("cancel", CSV);
    let r = t.sort_rows(
        1,
        SortOrder::Ascending,
        &AtomicBool::new(true),
        &AtomicU64::new(0),
    );
    assert_eq!(r, Err(SortError::Cancelled));
    std::fs::remove_file(path).unwrap();
}
