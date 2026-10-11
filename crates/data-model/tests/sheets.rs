//! SHEET-2: references across sheets read the other sheet; renaming a sheet rewrites the
//! formulas naming it, deleting one makes them `#REF!`, and undo restores both.

use csv_engine::{Dialect, Source, SparseRowIndex};
use data_model::{rewrite_count, BookEdit, Budget, CellRef, CsvTable, Origin, SheetId, Workbook};
use formula::{Change, ErrorKind, Value};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn table(name: &str, content: &str) -> CsvTable {
    let path = std::env::temp_dir().join(format!("sheets-{}-{name}", std::process::id()));
    std::fs::write(&path, content).unwrap();
    let index = SparseRowIndex::new(Arc::new(Source::open(&path).unwrap()), &Dialect::default());
    index.build(&AtomicBool::new(false)).unwrap();
    std::fs::remove_file(path).unwrap();
    // Not scanned for formulas: the rewrites scan first.
    CsvTable::new(index, Dialect::default())
}

/// Sheet1: A1 `='Q1 Sales'!B2`, A2 `='q1 sales'!B2`, A3 `=B2`.
/// Q1 Sales: B2 `7`, C1 `=B2`, C2 `='Q1 Sales'!B2+1`.
fn book(tag: &str) -> Workbook {
    let one = table(
        &format!("{tag}-one.csv"),
        "='Q1 Sales'!B2\n='q1 sales'!B2\n=B2\n",
    );
    let q1 = table(&format!("{tag}-q1.csv"), "x,y,=B2\nz,7,='Q1 Sales'!B2+1\n");
    Workbook::from_sheets(
        vec![
            ("Sheet1".into(), one, Origin::Csv, Default::default()),
            ("Q1 Sales".into(), q1, Origin::Csv, Default::default()),
        ],
        Vec::new(),
    )
}

const ONE: SheetId = SheetId(0);
const Q1: SheetId = SheetId(1);

fn at(row: u64, col: u32) -> CellRef {
    CellRef {
        row: data_model::RowId::source(row),
        col: data_model::ColId::source(col),
    }
}

fn raw(b: &Workbook, sheet: SheetId, row: u64, col: u32) -> String {
    b.sheet(sheet)
        .unwrap()
        .table
        .cell_value(row, col)
        .unwrap()
        .into_owned()
}

fn value(b: &mut Workbook, sheet: SheetId, row: u64, col: u32) -> Value {
    b.value_of(sheet, at(row, col), &mut Budget::cells(1_000))
        .unwrap()
}

fn no() -> AtomicBool {
    AtomicBool::new(false)
}

#[test]
fn a_reference_to_another_sheet_reads_it() {
    let mut b = book("read");
    assert_eq!(value(&mut b, ONE, 0, 0), Value::Number(7.0));
    assert_eq!(
        value(&mut b, ONE, 1, 0),
        Value::Number(7.0),
        "names ignore case"
    );
    assert_eq!(value(&mut b, ONE, 2, 0), Value::Empty, "Sheet1's own B2");
    assert_eq!(value(&mut b, Q1, 1, 2), Value::Number(8.0));
}

#[test]
fn renaming_a_sheet_rewrites_references_and_undo_restores_them() {
    let mut b = book("rename");
    let before: Vec<String> = (0..3).map(|r| raw(&b, ONE, r, 0)).collect();

    let rewrites = b
        .sheet_rewrites(
            &Change::RenameSheet {
                from: "Q1 Sales",
                to: "Q2",
            },
            &no(),
        )
        .unwrap();
    assert_eq!(
        rewrite_count(&rewrites),
        3,
        "two on Sheet1, one on Q1 Sales"
    );
    let undo_cells = b.apply(BookEdit::Sheets(rewrites));
    let undo_name = b.apply(BookEdit::Rename {
        id: Q1,
        name: "Q2".into(),
    });

    // `Q2` reads as a cell reference, so it is quoted, as Excel writes it.
    assert_eq!(raw(&b, ONE, 0, 0), "='Q2'!B2");
    assert_eq!(raw(&b, ONE, 1, 0), "='Q2'!B2");
    assert_eq!(
        raw(&b, ONE, 2, 0),
        "=B2",
        "an unqualified reference is untouched"
    );
    assert_eq!(
        raw(&b, Q1, 0, 2),
        "=B2",
        "the renamed sheet's own references too"
    );
    assert_eq!(raw(&b, Q1, 1, 2), "='Q2'!B2+1");
    assert_eq!(value(&mut b, ONE, 0, 0), Value::Number(7.0));
    assert_eq!(value(&mut b, Q1, 1, 2), Value::Number(8.0));

    b.apply(undo_name);
    b.apply(undo_cells);
    let after: Vec<String> = (0..3).map(|r| raw(&b, ONE, r, 0)).collect();
    assert_eq!(after, before);
    assert_eq!(raw(&b, Q1, 1, 2), "='Q1 Sales'!B2+1");
    assert_eq!(b.sheet(Q1).unwrap().name, "Q1 Sales");
    assert_eq!(b.sheet(ONE).unwrap().table.changes(), 0);
    assert_eq!(value(&mut b, ONE, 0, 0), Value::Number(7.0));
}

#[test]
fn deleting_a_sheet_makes_references_ref_errors_and_undo_restores_them() {
    let mut b = book("delete");
    let rewrites = b
        .sheet_rewrites(&Change::DeleteSheet { name: "Q1 Sales" }, &no())
        .unwrap();
    assert_eq!(
        rewrites.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        [ONE],
        "the deleted sheet's own cells are left alone"
    );
    let undo_cells = b.apply(BookEdit::Sheets(rewrites));
    let undo_remove = b.apply(BookEdit::Remove { at: 1 });

    assert_eq!(b.sheets().len(), 1);
    assert_eq!(raw(&b, ONE, 0, 0), "=#REF!");
    assert_eq!(raw(&b, ONE, 1, 0), "=#REF!");
    assert_eq!(raw(&b, ONE, 2, 0), "=B2");
    assert_eq!(value(&mut b, ONE, 0, 0), Value::Error(ErrorKind::Ref));

    b.apply(undo_remove);
    b.apply(undo_cells);
    assert_eq!(b.sheets().len(), 2);
    assert_eq!(b.sheet(Q1).unwrap().name, "Q1 Sales");
    assert_eq!(raw(&b, ONE, 0, 0), "='Q1 Sales'!B2");
    assert_eq!(raw(&b, ONE, 1, 0), "='q1 sales'!B2");
    assert_eq!(value(&mut b, ONE, 0, 0), Value::Number(7.0));
}

#[test]
fn edited_formulas_follow_and_cancel_stops() {
    let mut b = book("edited");
    let cell = at(2, 0);
    b.sheet_mut(ONE)
        .unwrap()
        .table
        .apply(data_model::Edit::set(cell, "=SUM('Q1 Sales'!B1:B2)*2"));
    let rewrites = b
        .sheet_rewrites(
            &Change::RenameSheet {
                from: "q1 SALES",
                to: "Sales 2",
            },
            &no(),
        )
        .unwrap();
    b.apply(BookEdit::Sheets(rewrites));
    assert_eq!(raw(&b, ONE, 2, 0), "=SUM('Sales 2'!B1:B2)*2");

    assert!(b
        .sheet_rewrites(
            &Change::RenameSheet {
                from: "Q1 Sales",
                to: "x"
            },
            &AtomicBool::new(true)
        )
        .is_none());
}
