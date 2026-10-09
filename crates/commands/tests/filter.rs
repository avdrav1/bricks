//! FILT-1: per-column filters: exact, contains, not contains, empty, non-empty, and number
//! comparisons. A filter is a view: hidden rows stay in the file, and edits, clears,
//! copies, and pastes reach the rows shown.

use commands::{Batch, ClearCells, SetCell, UndoStack};
use data_model::{
    CellRef, Compare, CsvTable, DelimiterChoice, Filter, FilterError, PasteError, RowBlock,
    SaveProgress, SortOrder, TableSource, Test,
};
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

fn open(name: &str) -> (std::path::PathBuf, CsvTable) {
    let path = std::env::temp_dir().join(format!("filt1-{}-{name}.csv", std::process::id()));
    std::fs::write(&path, CSV).unwrap();
    let mut t = CsvTable::open(&path, DelimiterChoice::Auto).unwrap();
    t.index().build(&AtomicBool::new(false)).unwrap();
    let types = t.infer_types(&AtomicBool::new(false)).unwrap();
    t.set_types(types);
    (path, t)
}

fn filter(t: &mut CsvTable, col: u32, test: Test) {
    let f = Filter {
        col: t.col_id(col),
        test,
    };
    let rows = t
        .snapshot()
        .filter_rows(&f, &AtomicBool::new(false), &AtomicU64::new(0))
        .unwrap();
    t.set_filter(f, rows);
}

/// Column `col` of the rows shown.
fn column(t: &mut CsvTable, col: u32) -> Vec<String> {
    let n = t.row_count();
    let mut b = RowBlock::full_text();
    t.read_rows(0..n, &mut b);
    (0..n)
        .map(|r| b.cell(r, col).unwrap_or("").to_owned())
        .collect()
}

fn ids(t: &mut CsvTable) -> Vec<String> {
    column(t, 0)
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
fn each_condition_shows_the_rows_that_pass() {
    let (path, mut t) = open("conditions");
    let mut check = |col, test, want: &[&str]| {
        filter(&mut t, col, test);
        assert_eq!(ids(&mut t), want);
        assert_eq!(
            t.header_cell(1).as_deref(),
            Some("city"),
            "the header stays"
        );
        t.clear_filter(col);
        assert_eq!(t.row_count(), 6);
    };
    let s = |v: &str| v.to_owned();
    check(1, Test::Equals(s("denver")), &["1", "3"]);
    check(1, Test::Contains(s("BOS")), &["2", "6"]);
    check(1, Test::NotContains(s("denver")), &["2", "4", "6"]);
    check(3, Test::Empty, &["2", "5"]);
    check(3, Test::NonEmpty, &["1", "3", "4", "6"]);
    check(2, Test::Number(Compare::Gt, 7.0), &["1", "3", "5"]);
    check(2, Test::Number(Compare::Le, 7.5), &["3", "6"]);
    check(2, Test::Number(Compare::Ne, 10.0), &["3", "5", "6"]); // n/a and empty: not numbers
    std::fs::remove_file(path).unwrap();
}

#[test]
fn filters_on_several_columns_combine() {
    let (path, mut t) = open("and");
    filter(&mut t, 1, Test::Contains("denver".into()));
    filter(&mut t, 2, Test::Number(Compare::Gt, 8.0));
    assert_eq!(ids(&mut t), ["1", "5"]);
    // A new filter on a column replaces its old one.
    filter(&mut t, 2, Test::Number(Compare::Lt, 8.0));
    assert_eq!(ids(&mut t), ["3"]);
    t.clear_filter(2);
    assert_eq!(ids(&mut t), ["1", "3", "5"]);
    assert!(t.filter_on(1).is_some() && t.filter_on(2).is_none());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn edits_clears_copies_and_pastes_reach_the_rows_shown() {
    let (path, mut t) = open("view");
    let mut undo = UndoStack::default();
    filter(&mut t, 1, Test::Contains("bos".into())); // rows 2 and 6
    let at = |t: &CsvTable, r, c| CellRef {
        row: t.row_id(r),
        col: t.col_id(c),
    };
    undo.execute(Box::new(SetCell::new(at(&t, 1, 3), "edited")), &mut t);
    assert_eq!(column(&mut t, 3), ["", "edited"]);

    let copied = t
        .copy_range(0..2, 0..2, &AtomicBool::new(false), &AtomicU64::new(0))
        .unwrap();
    assert_eq!(copied.tsv, "2\tboston\n6\tBoston");

    let cells = vec![vec!["p".to_owned()], vec!["q".to_owned()]];
    let edits = t.paste(0, 0, &cells).unwrap();
    undo.execute(Box::new(Batch::new("Paste", edits, at(&t, 0, 0))), &mut t);
    assert_eq!(ids(&mut t), ["p", "q"]);
    let three = vec![vec!["x".to_owned()]; 3];
    assert_eq!(
        t.paste(0, 0, &three),
        Err(PasteError::Filtered),
        "no rows added"
    );
    assert!(t.insert_rows(0, 1).is_none() && t.delete_rows(0, 1).is_none());

    let clear = t.clear_cells(0..2, 1..2).unwrap();
    undo.execute(Box::new(ClearCells::new(clear, at(&t, 0, 0))), &mut t);
    t.clear_filter(1);
    assert_eq!(
        saved(&t),
        "id,city,score,note\n1,Denver,10,a\np,,n/a,\n3,DENVER,7.5,b\n4,Austin,,c\n\
         5,Denver East,12,\nq,,-3,edited\n",
        "only the rows shown changed; every row is saved"
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn an_edited_row_stays_until_the_filter_is_applied_again() {
    let (path, mut t) = open("stale");
    let mut undo = UndoStack::default();
    filter(&mut t, 1, Test::Equals("denver".into()));
    let at = CellRef {
        row: t.row_id(0),
        col: t.col_id(1),
    };
    undo.execute(Box::new(SetCell::new(at, "Paris")), &mut t);
    assert_eq!(ids(&mut t), ["1", "3"]);
    filter(&mut t, 1, Test::Equals("denver".into()));
    assert_eq!(ids(&mut t), ["3"]);
    assert_eq!(saved(&t).lines().count(), 7, "hidden rows are saved");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn sorting_a_filtered_table_sorts_every_row_and_keeps_the_filter() {
    let (path, mut t) = open("sort");
    let mut undo = UndoStack::default();
    filter(&mut t, 1, Test::Contains("en".into())); // 1, 3, 5
    let edit = t
        .snapshot()
        .sort_rows(
            2,
            SortOrder::Descending,
            &AtomicBool::new(false),
            &AtomicU64::new(0),
        )
        .unwrap()
        .unwrap();
    let focus = CellRef {
        row: t.row_id(0),
        col: t.col_id(2),
    };
    undo.execute(Box::new(Batch::new("Sort", vec![edit], focus)), &mut t);
    assert_eq!(ids(&mut t), ["5", "1", "3"]);
    assert!(
        saved(&t).starts_with("id,city,score,note\n2,boston,n/a,\n5,"),
        "hidden rows sorted too: {}",
        saved(&t)
    );
    undo.undo(&mut t).unwrap();
    assert_eq!(ids(&mut t), ["1", "3", "5"]);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn the_header_row_is_never_filtered_out_but_counts_once_it_is_data() {
    let (path, mut t) = open("header");
    filter(&mut t, 1, Test::Contains("e".into()));
    assert_eq!(ids(&mut t), ["1", "3", "5"]);
    t.set_header(false); // "city" has no "e": hidden as data
    assert_eq!(ids(&mut t), ["1", "3", "5"]);
    filter(&mut t, 1, Test::Contains("i".into()));
    assert_eq!(ids(&mut t), ["id", "4"]);
    t.set_header(true);
    assert_eq!(ids(&mut t), ["4"]);
    assert_eq!(t.header_cell(0).as_deref(), Some("id"));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn a_cancelled_filter_returns_nothing() {
    let (path, t) = open("cancel");
    let f = Filter {
        col: t.col_id(1),
        test: Test::Empty,
    };
    let r = t.filter_rows(&f, &AtomicBool::new(true), &AtomicU64::new(0));
    assert!(matches!(r, Err(FilterError::Cancelled)));
    std::fs::remove_file(path).unwrap();
}
