//! SORT-2: a sort moves whole rows and honors the header. With "first row is header" on,
//! the header row never moves, whatever it would sort as; with it off, the first row is
//! data and sorts with the rest. Every part of a row travels with it: its source fields,
//! edits, cleared cells, cells of inserted columns, and fields past the widest row.

use commands::{Batch, ClearCells, Reshape, SetCell, UndoStack};
use data_model::{
    CellRef, CsvTable, DelimiterChoice, RowBlock, SaveProgress, SortOrder, TableSource,
};
use std::sync::atomic::{AtomicBool, AtomicU64};

// "name" sorts between "bea" and "zed": a header taken for data would move.
const CSV: &str = "\
id,name,note
1,zed,\"two
lines\"
2,ann,x
3,bea,y,extra,fields
4,ola
";

fn open(name: &str) -> (std::path::PathBuf, CsvTable) {
    let path = std::env::temp_dir().join(format!("sort2-{}-{name}.csv", std::process::id()));
    std::fs::write(&path, CSV).unwrap();
    let mut t = CsvTable::open(&path, DelimiterChoice::Auto).unwrap();
    t.index().build(&AtomicBool::new(false)).unwrap();
    assert!(t.has_header(), "detected");
    infer(&mut t);
    (path, t)
}

fn infer(t: &mut CsvTable) {
    let types = t.infer_types(&AtomicBool::new(false)).unwrap();
    t.set_types(types);
}

fn sort(t: &mut CsvTable, undo: &mut UndoStack<CsvTable>, col: u32, order: SortOrder) {
    let edit = t
        .snapshot()
        .sort_rows(col, order, &AtomicBool::new(false), &AtomicU64::new(0))
        .unwrap()
        .expect("the order changes");
    let focus = CellRef {
        row: t.row_id(0),
        col: t.col_id(col),
    };
    undo.execute(Box::new(Batch::new("Sort", vec![edit], focus)), t);
}

/// Every row as shown, every cell of it.
fn rows(t: &mut CsvTable) -> Vec<Vec<String>> {
    let n = t.row_count();
    let mut b = RowBlock::full_text();
    t.read_rows(0..n, &mut b);
    (0..n)
        .map(|r| {
            (0..b.cells_in_row(r))
                .map(|c| b.cell(r, c).unwrap().to_owned())
                .collect()
        })
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

#[test]
fn the_header_row_never_moves() {
    let (path, mut t) = open("header");
    let mut undo = UndoStack::default();
    for order in [SortOrder::Ascending, SortOrder::Descending] {
        sort(&mut t, &mut undo, 1, order);
        assert_eq!(t.header_cell(1).as_deref(), Some("name"), "{order:?}");
        let names: Vec<String> = rows(&mut t).into_iter().map(|r| r[1].clone()).collect();
        let want = match order {
            SortOrder::Ascending => ["ann", "bea", "ola", "zed"],
            SortOrder::Descending => ["zed", "ola", "bea", "ann"],
        };
        assert_eq!(names, want, "{order:?}");
        assert!(saved(&t).starts_with("id,name,note\n"), "{order:?}");
    }
    // An edited title stays the title.
    let title = CellRef {
        row: t.row_id(0),
        col: t.col_id(1),
    };
    let header = CellRef {
        row: data_model::RowId::source(0),
        ..title
    };
    undo.execute(Box::new(SetCell::new(header, "Name")), &mut t);
    sort(&mut t, &mut undo, 1, SortOrder::Ascending);
    assert_eq!(t.header_cell(1).as_deref(), Some("Name"));
    assert!(saved(&t).starts_with("id,Name,note\n"));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn without_a_header_the_first_row_is_data_and_sorts_with_the_rest() {
    let (path, mut t) = open("no-header");
    let mut undo = UndoStack::default();
    t.set_header(false);
    infer(&mut t);
    sort(&mut t, &mut undo, 1, SortOrder::Ascending);
    let names: Vec<String> = rows(&mut t).into_iter().map(|r| r[1].clone()).collect();
    assert_eq!(names, ["ann", "bea", "name", "ola", "zed"]);
    // Showing it as a header again titles the columns with whatever is now first.
    let save = saved(&t);
    assert_eq!(save.lines().nth(2), Some("id,name,note"), "{save}");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn every_part_of_a_row_moves_with_it() {
    let (path, mut t) = open("whole");
    let mut undo = UndoStack::default();
    let focus = CellRef {
        row: t.row_id(0),
        col: t.col_id(0),
    };
    // A column inserted before "note", with a value in ann's row; ann's id edited;
    // zed's note cleared.
    let insert = t.insert_cols(2, 1).unwrap();
    undo.execute(Box::new(Reshape::new(insert, focus)), &mut t);
    let at = |t: &CsvTable, r, c| CellRef {
        row: t.row_id(r),
        col: t.col_id(c),
    };
    undo.execute(Box::new(SetCell::new(at(&t, 1, 2), "new")), &mut t);
    undo.execute(Box::new(SetCell::new(at(&t, 1, 0), "20")), &mut t);
    let clear = t.clear_cells(0..1, 3..4).unwrap();
    undo.execute(Box::new(ClearCells::new(clear, focus)), &mut t);
    let before = rows(&mut t);
    assert_eq!(before[0], ["1", "zed", "", ""]);
    assert_eq!(before[1], ["20", "ann", "new", "x"]);
    assert_eq!(before[2], ["3", "bea", "", "y", "extra", "fields"]);
    assert_eq!(before[3], ["4", "ola"], "a short row stays short");

    sort(&mut t, &mut undo, 1, SortOrder::Descending);
    let after = rows(&mut t);
    let by_name = |name: &str| before.iter().find(|r| r[1] == name).unwrap().clone();
    assert_eq!(
        after,
        ["zed", "ola", "bea", "ann"].map(by_name),
        "each row arrives whole"
    );
    assert_eq!(
        saved(&t),
        "id,name,,note\n1,zed,,\n4,ola\n3,bea,,y,extra,fields\n20,ann,new,x\n"
    );
    std::fs::remove_file(path).unwrap();
}
