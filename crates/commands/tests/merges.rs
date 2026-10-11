//! STYLE-4: Merge, Unmerge, and a row insert that moves a merge are each one undo step
//! that puts the cells' text and the merge list back exactly, and redo again.

use commands::{Batch, Reshape, UndoStack};
use csv_engine::{Dialect, Source, SparseRowIndex};
use data_model::{CellArea, CellRef, CsvTable, FileLayout, FileStyles};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn area(first_row: u64, first_col: u32, last_row: u64, last_col: u32) -> CellArea {
    CellArea {
        first_row,
        first_col,
        last_row,
        last_col,
    }
}

fn table(name: &str, content: &str, merges: Vec<CellArea>) -> CsvTable {
    let path = std::env::temp_dir().join(format!("cmd-merges-{}-{name}", std::process::id()));
    std::fs::write(&path, content).unwrap();
    let index = SparseRowIndex::new(Arc::new(Source::open(&path).unwrap()), &Dialect::default());
    index.build(&AtomicBool::new(false)).unwrap();
    std::fs::remove_file(path).unwrap();
    let mut t = CsvTable::new(index, Dialect::default());
    let mut layout = FileLayout::default();
    layout.merges = merges;
    t.set_file_styles(FileStyles::default(), layout);
    t
}

/// Every cell's text, row by row, and the merges.
fn state(t: &CsvTable) -> (Vec<Vec<String>>, Vec<CellArea>) {
    let rows = (0..t.total_rows())
        .map(|r| {
            (0..3)
                .map(|c| t.cell_value(r, c).unwrap_or_default().into_owned())
                .collect()
        })
        .collect();
    (rows, t.merges().to_vec())
}

fn origin(t: &CsvTable) -> CellRef {
    CellRef {
        row: t.row_id(0),
        col: t.col_id(0),
    }
}

#[test]
fn merge_and_unmerge_undo_and_redo_exactly() {
    let mut t = table("mu.csv", "Title,x,\n,y,z\n", vec![area(1, 1, 1, 2)]);
    let mut undo = UndoStack::default();
    let before = state(&t);
    // A1:B2 touches B2:C2: it grows to A1:C2 and empties x, y, z.
    let plan = t.merge_plan(area(0, 0, 1, 1)).unwrap();
    assert!(plan.drops_text);
    let focus = origin(&t);
    undo.execute(
        Box::new(Batch::new("Merge cells", plan.edits, focus)),
        &mut t,
    );
    let merged = state(&t);
    assert_eq!(merged.1, vec![area(0, 0, 1, 2)]);
    assert_eq!(merged.0, vec![vec!["Title", "", ""], vec!["", "", ""]]);
    undo.undo(&mut t);
    assert_eq!(state(&t), before);
    assert_eq!(t.changes(), 0);
    undo.redo(&mut t);
    assert_eq!(state(&t), merged);

    let unmerge = t.unmerge(area(1, 2, 1, 2)).unwrap();
    undo.execute(
        Box::new(Batch::new("Unmerge cells", vec![unmerge], focus)),
        &mut t,
    );
    assert!(t.merges().is_empty());
    assert_eq!(state(&t).0, merged.0, "unmerging leaves the text as it is");
    undo.undo(&mut t);
    assert_eq!(state(&t), merged);
}

#[test]
fn inserting_a_row_moves_the_merge_in_the_same_step() {
    let mut t = table("ins.csv", "Title,,\na,b,c\n", vec![area(0, 0, 0, 2)]);
    let mut undo = UndoStack::default();
    let before = state(&t);
    let insert = t.insert_rows(0, 1).unwrap();
    let merges = t.merges_after(&insert);
    let cursor = origin(&t);
    undo.execute(
        Box::new(Reshape::new(insert, cursor).with_merges(merges)),
        &mut t,
    );
    assert_eq!(t.merges(), &[area(1, 0, 1, 2)]);
    assert_eq!(t.cell_value(1, 0).unwrap(), "Title");
    undo.undo(&mut t);
    assert_eq!(state(&t), before);
    undo.redo(&mut t);
    assert_eq!(t.merges(), &[area(1, 0, 1, 2)]);
}
