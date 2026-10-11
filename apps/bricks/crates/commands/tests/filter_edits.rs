//! FILT-4: edits and paste respect the filtered view. A paste into a filtered range fills
//! the rows shown and leaves the hidden rows between them alone; one that runs past the
//! last row shown adds rows at the end of the file, which stay shown. Deleting rows takes
//! out only the rows shown, as one step; inserting puts rows next to the row shown. Every
//! hidden row is still saved, unchanged, and undo puts everything back.

use commands::{Batch, Reshape, UndoStack};
use data_model::{
    CellRef, CsvTable, DelimiterChoice, Edit, Filter, RowBlock, SaveProgress, SortOrder,
    TableSource, Test,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64};

const CSV: &str = "\
id,city,score,note
1,Denver,10,a
2,boston,n/a,
3,DENVER,7.5,b
4,Austin,,c
5,Denver East,12,
6,Boston,-3,d
";

struct Tmp(PathBuf);

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn open(name: &str) -> (Tmp, CsvTable) {
    let path = std::env::temp_dir().join(format!("filt4-{}-{name}.csv", std::process::id()));
    std::fs::write(&path, CSV).unwrap();
    let mut t = CsvTable::open(&path, DelimiterChoice::Auto).unwrap();
    t.index().build(&AtomicBool::new(false)).unwrap();
    let types = t.infer_types(&AtomicBool::new(false)).unwrap();
    t.set_types(types);
    (Tmp(path), t)
}

/// Show only the Denver rows: 1, 3, and 5 (2, 4, and 6 hidden).
fn denver(t: &mut CsvTable) {
    let f = Filter {
        col: t.col_id(1),
        test: Test::Contains("denver".into()),
    };
    let rows = t
        .snapshot()
        .filter_rows(&f, &AtomicBool::new(false), &AtomicU64::new(0))
        .unwrap();
    t.set_filter(f, rows);
}

fn ids(t: &mut CsvTable) -> Vec<String> {
    let n = t.row_count();
    let mut b = RowBlock::full_text();
    t.read_rows(0..n, &mut b);
    (0..n)
        .map(|r| b.cell(r, 0).unwrap_or("").to_owned())
        .collect()
}

fn saved(t: &CsvTable) -> String {
    let mut out = Vec::new();
    t.save_job()
        .unwrap()
        .write_to(&mut out, &SaveProgress::default(), &AtomicBool::new(false))
        .unwrap();
    String::from_utf8(out).unwrap()
}

fn cells(rows: &[&[&str]]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|r| r.iter().map(|s| s.to_string()).collect())
        .collect()
}

fn origin(t: &CsvTable) -> CellRef {
    CellRef {
        row: t.row_id(0),
        col: t.col_id(0),
    }
}

/// FILT-4 acceptance: paste into a filtered range touches visible rows only.
#[test]
fn paste_into_a_filtered_range_touches_visible_rows_only() {
    let (_tmp, mut t) = open("paste");
    let mut undo = UndoStack::default();
    denver(&mut t);
    let edits = t
        .paste(0, 2, &cells(&[&["x", "p"], &["y", "q"], &["z", "r"]]))
        .unwrap();
    undo.execute(Box::new(Batch::new("Paste", edits, origin(&t))), &mut t);
    assert_eq!(
        saved(&t),
        "id,city,score,note\n1,Denver,x,p\n2,boston,n/a,\n3,DENVER,y,q\n4,Austin,,c\n\
         5,Denver East,z,r\n6,Boston,-3,d\n",
        "rows 2, 4, and 6 untouched"
    );
    undo.undo(&mut t).unwrap();
    assert_eq!(saved(&t), CSV);
}

#[test]
fn a_paste_past_the_last_row_shown_adds_rows_that_stay_shown() {
    let (_tmp, mut t) = open("grow");
    let mut undo = UndoStack::default();
    denver(&mut t);
    let edits = t.paste(2, 0, &cells(&[&["5b"], &["7"], &["8"]])).unwrap();
    undo.execute(Box::new(Batch::new("Paste", edits, origin(&t))), &mut t);
    assert_eq!(ids(&mut t), ["1", "3", "5b", "7", "8"]);
    assert_eq!(t.unfiltered_row_count(), 8);
    assert!(
        saved(&t).ends_with("5b,Denver East,12,\n6,Boston,-3,d\n7,,,\n8,,,\n"),
        "added at the end of the file"
    );
    undo.undo(&mut t).unwrap();
    assert_eq!(ids(&mut t), ["1", "3", "5"]);
    assert_eq!(saved(&t), CSV);
}

#[test]
fn deleting_rows_takes_out_only_the_rows_shown() {
    let (_tmp, mut t) = open("delete");
    let mut undo = UndoStack::default();
    denver(&mut t);
    let edit = t.delete_rows(0, 3).unwrap();
    assert!(matches!(edit, Edit::DeleteRowRanges(_)), "{edit:?}");
    undo.execute(Box::new(Reshape::new(edit, origin(&t))), &mut t);
    assert_eq!(undo.len(), 1, "one step");
    assert_eq!(t.row_count(), 0);
    assert_eq!(
        saved(&t),
        "id,city,score,note\n2,boston,n/a,\n4,Austin,,c\n6,Boston,-3,d\n"
    );
    let back = undo.undo(&mut t).unwrap().focus().unwrap();
    assert_eq!(t.row_of(back.row), Some(0), "undo shows the first row back");
    assert_eq!(ids(&mut t), ["1", "3", "5"]);
    assert_eq!(saved(&t), CSV, "back in place");
    undo.redo(&mut t).unwrap();
    assert_eq!(t.unfiltered_row_count(), 3);

    // Neighbours with nothing hidden between them are one plain delete.
    let (_tmp2, mut t) = open("delete-one");
    denver(&mut t);
    t.clear_filter(1);
    let f = Filter {
        col: t.col_id(0),
        test: Test::NotContains("6".into()),
    };
    let rows = t
        .snapshot()
        .filter_rows(&f, &AtomicBool::new(false), &AtomicU64::new(0))
        .unwrap();
    t.set_filter(f, rows);
    assert!(matches!(
        t.delete_rows(1, 2),
        Some(Edit::DeleteRows { count: 2, .. })
    ));
}

#[test]
fn deleting_shown_rows_of_a_sorted_table() {
    let (_tmp, mut t) = open("sorted");
    let mut undo = UndoStack::default();
    let sort = t
        .snapshot()
        .sort_rows(
            0,
            SortOrder::Descending,
            &AtomicBool::new(false),
            &AtomicU64::new(0),
        )
        .unwrap()
        .unwrap();
    undo.execute(Box::new(Batch::new("Sort", vec![sort], origin(&t))), &mut t);
    denver(&mut t);
    assert_eq!(ids(&mut t), ["5", "3", "1"]);
    let edit = t.delete_rows(0, 2).unwrap(); // 5 and 3, with 4 hidden between
    undo.execute(Box::new(Reshape::new(edit, origin(&t))), &mut t);
    assert_eq!(ids(&mut t), ["1"]);
    assert_eq!(
        saved(&t),
        "id,city,score,note\n6,Boston,-3,d\n4,Austin,,c\n2,boston,n/a,\n1,Denver,10,a\n"
    );
    undo.undo(&mut t).unwrap();
    assert_eq!(ids(&mut t), ["5", "3", "1"]);
    undo.undo(&mut t).unwrap(); // the sort
    assert_eq!(saved(&t), CSV);
}

#[test]
fn inserting_rows_puts_them_next_to_the_row_shown() {
    let (_tmp, mut t) = open("insert");
    let mut undo = UndoStack::default();
    denver(&mut t);
    let edit = t.insert_rows(1, 1).unwrap(); // above DENVER (3), below hidden 2
    undo.execute(Box::new(Reshape::new(edit, origin(&t))), &mut t);
    let at_end = t.insert_rows(t.row_count(), 1).unwrap();
    undo.execute(Box::new(Reshape::new(at_end, origin(&t))), &mut t);
    assert_eq!(ids(&mut t), ["1", "", "3", "5", ""], "both shown");
    assert_eq!(
        saved(&t),
        "id,city,score,note\n1,Denver,10,a\n2,boston,n/a,\n,,,\n3,DENVER,7.5,b\n4,Austin,,c\n\
         5,Denver East,12,\n6,Boston,-3,d\n,,,\n"
    );
    undo.undo(&mut t).unwrap();
    undo.undo(&mut t).unwrap();
    assert_eq!(saved(&t), CSV);
}
