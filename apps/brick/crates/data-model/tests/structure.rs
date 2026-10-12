//! FML-10: formulas follow inserted, deleted, and sorted rows and columns, on their own
//! sheet and from other sheets; undo restores every formula's text.

use csv_engine::{Dialect, Source, SparseRowIndex};
use data_model::{BookEdit, Budget, CellRef, CsvTable, Edit, Origin, SheetId, SortOrder, Workbook};
use formula::Value;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;

fn table(name: &str, content: &str) -> CsvTable {
    let path = std::env::temp_dir().join(format!("structure-{}-{name}", std::process::id()));
    std::fs::write(&path, content).unwrap();
    let index = SparseRowIndex::new(Arc::new(Source::open(&path).unwrap()), &Dialect::default());
    index.build(&AtomicBool::new(false)).unwrap();
    std::fs::remove_file(path).unwrap();
    let mut t = CsvTable::new(index, Dialect::default());
    let found = t
        .scan_formulas(&AtomicBool::new(false), &AtomicU64::new(0))
        .unwrap();
    t.set_formula_cells(found);
    t
}

const S1: SheetId = SheetId(0);
const S2: SheetId = SheetId(1);

/// Sheet1: A1..A12 hold 1..12; B1..F1 hold formulas; B3 `=A12`. Sheet2: A1 `=Sheet1!A5`,
/// B1 `=A5`.
fn rows_book(tag: &str) -> Workbook {
    let mut csv = String::new();
    for n in 1..=12 {
        match n {
            1 => csv.push_str("1,=SUM(A1:A10),=A5,=$A$5,=A3,=A10\n"),
            3 => csv.push_str("3,=A12\n"),
            _ => csv.push_str(&format!("{n}\n")),
        }
    }
    let one = table(&format!("{tag}-1.csv"), &csv);
    let two = table(&format!("{tag}-2.csv"), "=Sheet1!A5,=A5\n");
    Workbook::from_sheets(
        vec![
            ("Sheet1".into(), one, Origin::Csv, Default::default()),
            ("Sheet2".into(), two, Origin::Csv, Default::default()),
        ],
        Vec::new(),
    )
}

/// Raw text of row `row`, columns `cols`, as the table holds it now.
fn raw(b: &Workbook, sheet: SheetId, row: u64, cols: std::ops::Range<u32>) -> Vec<String> {
    let t = &b.sheet(sheet).unwrap().table;
    cols.map(|c| t.cell_value(row, c).unwrap_or_default().into_owned())
        .collect()
}

/// The structural edit plus the formula rewrites, as one step; returns its inverse.
fn apply(b: &mut Workbook, sheet: SheetId, edit: Edit) -> BookEdit {
    let rewrites = b
        .structure_rewrites(sheet, &edit, &AtomicBool::new(false))
        .unwrap();
    let mut edits = vec![(sheet, edit)];
    edits.extend(rewrites);
    b.apply(BookEdit::Sheets(edits))
}

#[test]
fn inserted_rows_move_references_past_them() {
    let mut b = rows_book("insert");
    let before = (raw(&b, S1, 0, 1..6), raw(&b, S2, 0, 0..2));
    let edit = b.sheet_mut(S1).unwrap().table.insert_rows(4, 1).unwrap();
    let undo = apply(&mut b, S1, edit);
    assert_eq!(
        raw(&b, S1, 0, 1..6),
        ["=SUM(A1:A11)", "=A6", "=$A$6", "=A3", "=A11"]
    );
    assert_eq!(raw(&b, S1, 2, 1..2), ["=A13"]);
    assert_eq!(
        raw(&b, S2, 0, 0..2),
        ["=Sheet1!A6", "=A5"],
        "another sheet's reference follows; its own doesn't"
    );
    let v = b.value_of(
        S1,
        CellRef {
            row: data_model::RowId::source(0),
            col: data_model::ColId::source(1),
        },
        &mut Budget::cells(1_000),
    );
    assert_eq!(
        v.unwrap(),
        Value::Number(55.0),
        "the sum still covers 1..10"
    );

    b.apply(undo);
    assert_eq!((raw(&b, S1, 0, 1..6), raw(&b, S2, 0, 0..2)), before);
    assert_eq!(b.sheet(S1).unwrap().table.changes(), 0);
}

#[test]
fn deleted_rows_make_references_ref_errors_and_shrink_ranges() {
    let mut b = rows_book("delete");
    let before = raw(&b, S1, 0, 1..6);
    let edit = b.sheet(S1).unwrap().table.delete_rows(4, 1).unwrap();
    let undo = apply(&mut b, S1, edit);
    assert_eq!(
        raw(&b, S1, 0, 1..6),
        ["=SUM(A1:A9)", "=#REF!", "=#REF!", "=A3", "=A9"]
    );
    assert_eq!(raw(&b, S2, 0, 0..2), ["=#REF!", "=A5"]);
    b.apply(undo);
    assert_eq!(raw(&b, S1, 0, 1..6), before);
    assert_eq!(raw(&b, S2, 0, 0..2), ["=Sheet1!A5", "=A5"]);
}

#[test]
fn a_filtered_delete_of_two_ranges() {
    let mut b = rows_book("ranges");
    // Positions 2 (with B3's own formula) and 6..8 go.
    let edit = Edit::DeleteRowRanges(vec![2..3, 6..8]);
    let rewrites = b
        .structure_rewrites(S1, &edit, &AtomicBool::new(false))
        .unwrap();
    let cells: Vec<CellRef> = rewrites
        .iter()
        .flat_map(|(_, e)| match e {
            Edit::Cells(c) => c.iter().map(|(at, _)| *at).collect(),
            _ => Vec::new(),
        })
        .collect();
    assert!(
        !cells.contains(&CellRef {
            row: data_model::RowId::source(2),
            col: data_model::ColId::source(1)
        }),
        "a formula in a deleted row isn't rewritten"
    );
    let undo = apply(&mut b, S1, edit);
    assert_eq!(
        raw(&b, S1, 0, 1..6),
        ["=SUM(A1:A7)", "=A4", "=$A$4", "=#REF!", "=A7"]
    );
    assert_eq!(raw(&b, S2, 0, 0..2), ["=Sheet1!A4", "=A5"]);
    b.apply(undo);
    assert_eq!(
        raw(&b, S1, 0, 1..6),
        ["=SUM(A1:A10)", "=A5", "=$A$5", "=A3", "=A10"]
    );
}

/// Sheet1: row 1 holds 1..12 in A..L; row 2 holds formulas in A..E.
fn cols_book(tag: &str) -> Workbook {
    let values: Vec<String> = (1..=12).map(|n| n.to_string()).collect();
    let csv = format!("{}\n=SUM(A1:J1),=E1,=$E$1,=C1,=J1\n", values.join(","));
    let one = table(&format!("{tag}-1.csv"), &csv);
    let two = table(&format!("{tag}-2.csv"), "=Sheet1!E1,=E1\n");
    Workbook::from_sheets(
        vec![
            ("Sheet1".into(), one, Origin::Csv, Default::default()),
            ("Sheet2".into(), two, Origin::Csv, Default::default()),
        ],
        Vec::new(),
    )
}

#[test]
fn inserted_and_deleted_columns_mirror_rows() {
    let mut b = cols_book("cols");
    let before = raw(&b, S1, 1, 0..5);

    let edit = b.sheet_mut(S1).unwrap().table.insert_cols(4, 1).unwrap();
    let undo = apply(&mut b, S1, edit);
    assert_eq!(
        raw(&b, S1, 1, 0..6),
        ["=SUM(A1:K1)", "=F1", "=$F$1", "=C1", "", "=K1"],
        "the new column E is empty; `=J1` moved to F"
    );
    assert_eq!(raw(&b, S2, 0, 0..2), ["=Sheet1!F1", "=E1"]);
    b.apply(undo);
    assert_eq!(raw(&b, S1, 1, 0..5), before);

    let edit = b.sheet(S1).unwrap().table.delete_cols(4, 1).unwrap();
    let undo = apply(&mut b, S1, edit);
    // Column E goes, and its own formula `=J1` with it.
    assert_eq!(
        raw(&b, S1, 1, 0..4),
        ["=SUM(A1:I1)", "=#REF!", "=#REF!", "=C1"]
    );
    assert_eq!(raw(&b, S2, 0, 0..2), ["=#REF!", "=E1"]);
    b.apply(undo);
    assert_eq!(raw(&b, S1, 1, 0..5), before);
    assert_eq!(raw(&b, S2, 0, 0..2), ["=Sheet1!E1", "=E1"]);
}

#[test]
fn sorted_formulas_keep_reading_their_own_row() {
    const N: u64 = 1_000;
    let mut csv = String::new();
    for n in 1..=N {
        csv.push_str(&format!("{n},{},=B{n}*2\n", (n * 7919) % 1_009));
    }
    let mut b = Workbook::single("Sheet1".into(), table("sort.csv", &csv), Origin::Csv);
    let t = b.active();
    let reorder = t
        .sort_rows(
            1,
            SortOrder::Ascending,
            &AtomicBool::new(false),
            &AtomicU64::new(0),
        )
        .unwrap()
        .unwrap();
    let Edit::Reorder(order) = &reorder else {
        panic!("a sort reorders");
    };
    let rewrites = t.sort_rewrites(order);
    assert!(rewrites.len() as u64 > N / 2, "most rows moved");

    let t = b.active_mut();
    let undo_order = t.apply(reorder);
    let undo_cells = t.apply(Edit::Cells(rewrites));
    let sheet = b.active_id();
    for r in 0..N {
        let t = b.active();
        let at = CellRef {
            row: t.row_id(r),
            col: t.col_id(2),
        };
        let own: f64 = t.cell_value(r, 1).unwrap().parse().unwrap();
        let v = b.value_of(sheet, at, &mut Budget::cells(100)).unwrap();
        assert_eq!(v, Value::Number(own * 2.0), "row {r}");
    }
    assert_ne!(
        b.active().cell_value(0, 0).unwrap(),
        "1",
        "the rows did move"
    );

    let t = b.active_mut();
    t.apply(undo_cells);
    t.apply(undo_order);
    for r in 0..N {
        assert_eq!(
            t.cell_value(r, 2).unwrap(),
            format!("=B{}*2", r + 1),
            "row {r}"
        );
    }
    assert_eq!(t.changes(), 0);
}
