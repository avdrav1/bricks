//! SRCH-3: find and replace. Replace all changes every matching cell shown, every match
//! in each cell, as one undo step; values are as shown (edits included), raw text stays
//! raw, and a filter's hidden rows and deleted rows are left alone.

use commands::{Batch, Reshape, SetCell, UndoStack};
use data_model::{
    CellRef, CsvTable, DelimiterChoice, Edit, Filter, Matches, Query, ReplaceError, SearchProgress,
    TableSource, Test,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64};

struct Tmp(PathBuf);

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn open(name: &str, csv: &str) -> (Tmp, CsvTable) {
    let path = std::env::temp_dir().join(format!("srch3-{}-{name}.csv", std::process::id()));
    std::fs::write(&path, csv).unwrap();
    let t = CsvTable::open(&path, DelimiterChoice::Auto).unwrap();
    t.index().build(&AtomicBool::new(false)).unwrap();
    (Tmp(path), t)
}

fn query(text: &str) -> Query {
    Query {
        text: text.into(),
        match_case: false,
        col: None,
    }
}

fn search(t: &CsvTable, q: &Query) -> Matches {
    t.snapshot()
        .search(q, &AtomicBool::new(false), &SearchProgress::default())
        .unwrap()
}

fn replace_all(t: &CsvTable, q: &Query, with: &str) -> Result<Option<Edit>, ReplaceError> {
    let snap = t.snapshot();
    search(t, q).replace_all(&snap, with, &AtomicBool::new(false), &AtomicU64::new(0))
}

/// Replace all as the app runs it: one undoable step.
fn run(t: &mut CsvTable, undo: &mut UndoStack<CsvTable>, q: &Query, with: &str) {
    let edit = replace_all(t, q, with)
        .unwrap()
        .expect("something to replace");
    let focus = CellRef {
        row: t.row_id(0),
        col: t.col_id(0),
    };
    undo.execute(Box::new(Batch::new("Replace all", vec![edit], focus)), t);
}

fn column(t: &CsvTable, col: u32) -> Vec<String> {
    (0..t.row_count())
        .map(|r| t.cell_value(r, col).unwrap_or_default().into_owned())
        .collect()
}

/// SRCH-3 acceptance: replace all on 100k hits undoes as one step.
#[test]
fn replace_all_on_100k_hits_undoes_as_one_step() {
    let mut csv = String::from("id,name,note\n");
    for i in 0..120_000 {
        let name = if i % 6 == 5 {
            "other".into()
        } else {
            format!("Foo{i}")
        };
        csv.push_str(&format!("{i},{name},x\n"));
    }
    let (_tmp, mut t) = open("100k", &csv);
    let mut undo = UndoStack::default();
    let foo = query("foo");
    assert_eq!(search(&t, &foo).total(), 100_000);
    let before = column(&t, 1);

    run(&mut t, &mut undo, &foo, "bar");
    assert_eq!(undo.len(), 1, "one step");
    assert_eq!(search(&t, &foo).total(), 0);
    assert_eq!(search(&t, &query("bar")).total(), 100_000);
    assert_eq!(t.cell_value(16, 1).as_deref(), Some("bar16"));
    assert_eq!(t.cell_value(5, 1).as_deref(), Some("other"));
    eprintln!("undo holds {} bytes for 100k cells", undo.heap_bytes());

    undo.undo(&mut t).unwrap();
    assert_eq!(column(&t, 1), before, "one undo restores every cell");
    assert_eq!(t.changes(), 0, "and leaves no edit behind");
    undo.redo(&mut t).unwrap();
    assert_eq!(search(&t, &query("bar")).total(), 100_000);
}

const CSV: &str = "\
id,city,note
1,Portland,\"Portland, PORTLAND\"
2,Boston,portlandia
3,portland,00123
4,Austin,\"say \"\"hi\"\"\"
";

#[test]
fn every_match_in_a_cell_case_as_searched() {
    let (_tmp, mut t) = open("cells", CSV);
    let mut undo = UndoStack::default();
    run(&mut t, &mut undo, &query("portland"), "PDX");
    assert_eq!(column(&t, 1), ["PDX", "Boston", "PDX", "Austin"]);
    assert_eq!(column(&t, 2), ["PDX, PDX", "PDXia", "00123", "say \"hi\""]);

    let (_tmp, mut t) = open("case", CSV);
    let mut q = query("Portland");
    q.match_case = true;
    run(&mut t, &mut undo, &q, "PDX");
    assert_eq!(column(&t, 1), ["PDX", "Boston", "portland", "Austin"]);
    assert_eq!(column(&t, 2)[0], "PDX, PORTLAND");
}

#[test]
fn one_column_raw_text_and_empty_replacements() {
    let (_tmp, mut t) = open("column", CSV);
    let mut undo = UndoStack::default();
    let mut q = query("portland");
    q.col = Some(t.col_id(2));
    run(&mut t, &mut undo, &q, "");
    assert_eq!(column(&t, 1)[0], "Portland", "other columns untouched");
    assert_eq!(column(&t, 2)[..2], [", ", "ia"]);
    run(&mut t, &mut undo, &query("1"), "9");
    assert_eq!(
        t.cell_value(2, 2).as_deref(),
        Some("00923"),
        "raw text stays raw"
    );
    run(&mut t, &mut undo, &query("\"hi\""), "'hi'");
    assert_eq!(
        t.cell_value(3, 2).as_deref(),
        Some("say 'hi'"),
        "quotes as shown"
    );
}

#[test]
fn edits_count_and_hidden_or_deleted_rows_are_left_alone() {
    let (_tmp, mut t) = open("view", CSV);
    let mut undo = UndoStack::default();
    let at = |t: &CsvTable, r, c| CellRef {
        row: t.row_id(r),
        col: t.col_id(c),
    };
    undo.execute(Box::new(SetCell::new(at(&t, 3, 1), "Portland too")), &mut t);
    let delete = t.delete_rows(1, 1).unwrap(); // Boston, portlandia
    undo.execute(Box::new(Reshape::new(delete, at(&t, 0, 0))), &mut t);
    let f = Filter {
        col: t.col_id(0),
        test: Test::NotContains("3".into()), // hides portland, 00123
    };
    let rows = t
        .snapshot()
        .filter_rows(&f, &AtomicBool::new(false), &AtomicU64::new(0))
        .unwrap();
    t.set_filter(f, rows);
    run(&mut t, &mut undo, &query("portland"), "PDX");
    assert_eq!(column(&t, 1), ["PDX", "PDX too"]);
    t.clear_filter(0);
    assert_eq!(
        column(&t, 1),
        ["PDX", "portland", "PDX too"],
        "the hidden row kept"
    );
    let steps = undo.len();
    undo.undo(&mut t).unwrap(); // the replace
    undo.undo(&mut t).unwrap(); // the delete
    assert_eq!(steps, 3);
    assert_eq!(
        column(&t, 2)[1],
        "portlandia",
        "the deleted row was never changed"
    );
}

#[test]
fn stale_matches_and_single_cells() {
    let (_tmp, mut t) = open("stale", CSV);
    let mut undo = UndoStack::default();
    let m = search(&t, &query("portland"));
    let at = CellRef {
        row: t.row_id(1),
        col: t.col_id(1),
    };
    undo.execute(Box::new(SetCell::new(at, "Salem")), &mut t);
    let r = m.replace_all(
        &t.snapshot(),
        "x",
        &AtomicBool::new(false),
        &AtomicU64::new(0),
    );
    assert_eq!(r, Err(ReplaceError::Stale));
    let r = m.replace_all(
        &t.snapshot(),
        "x",
        &AtomicBool::new(true),
        &AtomicU64::new(0),
    );
    assert_eq!(r, Err(ReplaceError::Stale), "stale before anything else");
    let m = search(&t, &query("portland"));
    let r = m.replace_all(
        &t.snapshot(),
        "x",
        &AtomicBool::new(true),
        &AtomicU64::new(0),
    );
    assert_eq!(r, Err(ReplaceError::Cancelled));
    assert_eq!(m.replace_in("a PORTLAND b", "x").as_deref(), Some("a x b"));
    assert_eq!(m.replace_in("Boston", "x"), None);
    assert_eq!(replace_all(&t, &query("Paris"), "x"), Ok(None));
}
