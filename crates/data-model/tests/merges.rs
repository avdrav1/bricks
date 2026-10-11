//! STYLE-4: merged cells in the table: the file's merges, `Edit::Merges` and its inverse,
//! merges following row and column inserts and deletes by Excel's rules, Merge and
//! Unmerge plans, and the sort refusal.

use csv_engine::{Dialect, Source, SparseRowIndex};
use data_model::{
    CellArea, CsvTable, Edit, FileLayout, FileStyles, MergeError, SortOrder, MERGE_MAX_CELLS,
};
use formula::{Axis, Change};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;

fn area(first_row: u64, first_col: u32, last_row: u64, last_col: u32) -> CellArea {
    CellArea {
        first_row,
        first_col,
        last_row,
        last_col,
    }
}

/// A table of `content` whose file merges are `merges`.
fn table(name: &str, content: &str, merges: Vec<CellArea>) -> CsvTable {
    let path = std::env::temp_dir().join(format!("merges-{}-{name}", std::process::id()));
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

/// Ten rows of four columns, `r{row}c{col}` in each cell.
fn grid_csv() -> String {
    (0..10)
        .map(|r| {
            (0..4)
                .map(|c| format!("r{r}c{c}"))
                .collect::<Vec<_>>()
                .join(",")
                + "\n"
        })
        .collect()
}

fn insert(axis: Axis, at: u64, count: u64) -> Change<'static> {
    Change::Insert {
        sheet: "",
        axis,
        at,
        count,
    }
}

fn delete(axis: Axis, at: u64, count: u64) -> Change<'static> {
    Change::Delete {
        sheet: "",
        axis,
        at,
        count,
    }
}

#[test]
fn the_files_merges_are_the_tables_sorted_and_found_by_cell() {
    // Out of order, with a one-cell "merge" that merges nothing.
    let t = table(
        "file.csv",
        &grid_csv(),
        vec![area(7, 4, 8, 6), area(0, 0, 0, 3), area(5, 1, 5, 1)],
    );
    assert_eq!(t.merges(), &[area(0, 0, 0, 3), area(7, 4, 8, 6)]);
    assert_eq!(t.changes(), 0, "the file's merges are no change");
    assert_eq!(t.merge_at(0, 2), Some(area(0, 0, 0, 3)));
    assert_eq!(
        t.merge_at(8, 5),
        Some(area(7, 4, 8, 6)),
        "a covered cell finds it"
    );
    assert_eq!(t.merge_at(8, 3), None);
    assert_eq!(t.merge_at(1, 0), None);
    assert_eq!(
        t.merges_in(0..=9, 3..=4).collect::<Vec<_>>(),
        vec![area(0, 0, 0, 3), area(7, 4, 8, 6)]
    );
    assert_eq!(t.merges_in(1..=6, 0..=9).count(), 0);
}

#[test]
fn a_tall_merge_is_found_from_its_last_row() {
    // Many merges after a tall one: the binary search must still reach back to it.
    let mut merges = vec![area(0, 0, 500, 0)];
    merges.extend((1..400).map(|r| area(r, 2, r, 3)));
    let t = table("tall.csv", &grid_csv(), merges);
    assert_eq!(t.merge_at(450, 0), Some(area(0, 0, 500, 0)));
    assert_eq!(t.merge_at(450, 1), None);
    assert_eq!(t.merge_at(300, 3), Some(area(300, 2, 300, 3)));
}

#[test]
fn a_merges_edit_undoes_and_counts_as_one_change() {
    let mut t = table("edit.csv", &grid_csv(), vec![area(0, 0, 0, 3)]);
    let before = t.revision();
    let inverse = t.apply(Edit::Merges(Arc::new(vec![area(2, 1, 3, 2)])));
    assert_ne!(t.revision(), before, "what the grid shows changed");
    assert_eq!(t.merges(), &[area(2, 1, 3, 2)]);
    assert_eq!(t.merge_at(0, 1), None);
    assert_eq!(t.merge_at(3, 1), Some(area(2, 1, 3, 2)));
    assert_eq!(t.changes(), 1);
    assert_eq!(inverse, Edit::Merges(Arc::new(vec![area(0, 0, 0, 3)])));
    t.apply(inverse);
    assert_eq!(t.merges(), &[area(0, 0, 0, 3)]);
    assert_eq!(t.changes(), 0, "back as the file has them");
}

#[test]
fn row_inserts_move_or_grow_merges() {
    let t = table(
        "rows.csv",
        &grid_csv(),
        vec![area(0, 0, 0, 3), area(4, 0, 6, 1)],
    );
    // Above row 1: both move down.
    assert_eq!(
        t.adjusted_merges(&insert(Axis::Rows, 0, 1)),
        Some(vec![area(1, 0, 1, 3), area(5, 0, 7, 1)])
    );
    // At a merge's first row it moves; inside it grows; below it nothing happens.
    assert_eq!(
        t.adjusted_merges(&insert(Axis::Rows, 5, 2)),
        Some(vec![area(0, 0, 0, 3), area(4, 0, 8, 1)])
    );
    assert_eq!(
        t.adjusted_merges(&insert(Axis::Rows, 4, 1)),
        Some(vec![area(0, 0, 0, 3), area(5, 0, 7, 1)])
    );
    assert_eq!(t.adjusted_merges(&insert(Axis::Rows, 7, 3)), None);
}

#[test]
fn row_deletes_shrink_or_remove_merges() {
    let t = table(
        "del.csv",
        &grid_csv(),
        vec![area(0, 0, 0, 3), area(4, 0, 6, 1)],
    );
    // Part of the tall merge: it shrinks; rows above it move it up.
    assert_eq!(
        t.adjusted_merges(&delete(Axis::Rows, 5, 1)),
        Some(vec![area(0, 0, 0, 3), area(4, 0, 5, 1)])
    );
    assert_eq!(
        t.adjusted_merges(&delete(Axis::Rows, 1, 2)),
        Some(vec![area(0, 0, 0, 3), area(2, 0, 4, 1)])
    );
    // Its first rows: it starts where they were.
    assert_eq!(
        t.adjusted_merges(&delete(Axis::Rows, 3, 2)),
        Some(vec![area(0, 0, 0, 3), area(3, 0, 4, 1)])
    );
    // All of a merge: gone.
    assert_eq!(
        t.adjusted_merges(&delete(Axis::Rows, 0, 1)),
        Some(vec![area(3, 0, 5, 1)])
    );
    // Shrunk to one row of one... still two columns wide: kept.
    assert_eq!(
        t.adjusted_merges(&delete(Axis::Rows, 4, 2)),
        Some(vec![area(0, 0, 0, 3), area(4, 0, 4, 1)])
    );
    assert_eq!(t.adjusted_merges(&delete(Axis::Rows, 8, 2)), None);
}

#[test]
fn column_inserts_and_deletes_follow_the_same_rules() {
    let t = table(
        "cols.csv",
        &grid_csv(),
        vec![area(0, 0, 0, 3), area(4, 1, 6, 2)],
    );
    assert_eq!(
        t.adjusted_merges(&insert(Axis::Cols, 0, 1)),
        Some(vec![area(0, 1, 0, 4), area(4, 2, 6, 3)])
    );
    assert_eq!(
        t.adjusted_merges(&insert(Axis::Cols, 2, 2)),
        Some(vec![area(0, 0, 0, 5), area(4, 1, 6, 4)])
    );
    assert_eq!(t.adjusted_merges(&insert(Axis::Cols, 4, 1)), None);
    // Deleting column B: the title shrinks; the other merge loses half its width.
    assert_eq!(
        t.adjusted_merges(&delete(Axis::Cols, 1, 1)),
        Some(vec![area(0, 0, 0, 2), area(4, 1, 6, 1)])
    );
    // Deleting B:C removes the second merge.
    assert_eq!(
        t.adjusted_merges(&delete(Axis::Cols, 1, 2)),
        Some(vec![area(0, 0, 0, 1)])
    );
    // A single-row merge shrunk to one column is a single cell: no longer a merge.
    assert_eq!(t.adjusted_merges(&delete(Axis::Cols, 1, 3)), Some(vec![]));
}

#[test]
fn structural_edits_bring_their_merges_edit() {
    let mut t = table("struct.csv", &grid_csv(), vec![area(0, 0, 0, 2)]);
    let insert = t.insert_rows(0, 1).unwrap();
    let merges = t.merges_after(&insert).unwrap();
    assert_eq!(merges, Edit::Merges(Arc::new(vec![area(1, 0, 1, 2)])));
    let undo_rows = t.apply(insert);
    let undo_merges = t.apply(merges);
    assert_eq!(t.merge_at(1, 1), Some(area(1, 0, 1, 2)));
    t.apply(undo_merges);
    t.apply(undo_rows);
    assert_eq!(t.merges(), &[area(0, 0, 0, 2)]);
    assert_eq!(t.changes(), 0);
    // Deleting rows below it needs no merges edit.
    let below = t.delete_rows(4, 2).unwrap();
    assert_eq!(t.merges_after(&below), None);
}

#[test]
fn merging_absorbs_merges_and_empties_the_other_cells() {
    let mut t = table("merge.csv", &grid_csv(), vec![area(2, 2, 3, 3)]);
    // B2:C3 touches C3:D4: the merge grows to B2:D4.
    let plan = t.merge_plan(area(1, 1, 2, 2)).unwrap();
    assert_eq!(plan.area, area(1, 1, 3, 3));
    assert!(plan.drops_text);
    let [Edit::Merges(list), Edit::Cells(cleared)] = plan.edits.as_slice() else {
        panic!("a merges edit and the cells emptied: {:?}", plan.edits);
    };
    assert_eq!(**list, vec![area(1, 1, 3, 3)]);
    assert_eq!(cleared.len(), 8, "every cell of B2:D4 but B2");
    let mut inverses: Vec<Edit> = plan.edits.into_iter().map(|e| t.apply(e)).collect();
    assert_eq!(
        t.cell_value(1, 1).unwrap(),
        "r1c1",
        "the top-left cell keeps its text"
    );
    assert_eq!(t.cell_value(1, 2).unwrap(), "");
    assert_eq!(t.cell_value(3, 3).unwrap(), "");
    assert_eq!(t.cell_value(4, 3).unwrap(), "r4c3", "outside the merge");
    inverses.reverse();
    for e in inverses {
        t.apply(e);
    }
    assert_eq!(t.merges(), &[area(2, 2, 3, 3)]);
    assert_eq!(t.cell_value(1, 2).unwrap(), "r1c2");
    assert_eq!(t.cell_value(3, 3).unwrap(), "r3c3");
    assert_eq!(t.changes(), 0);
}

#[test]
fn merging_empty_cells_drops_nothing_and_unmerge_removes_what_it_touches() {
    let mut t = table("empty.csv", "title,,\n,,\n", vec![area(1, 0, 1, 1)]);
    let plan = t.merge_plan(area(0, 0, 0, 2)).unwrap();
    assert!(!plan.drops_text);
    assert_eq!(plan.edits.len(), 1);
    assert_eq!(t.merge_plan(area(0, 1, 0, 1)), Err(MergeError::OneCell));
    for e in plan.edits {
        t.apply(e);
    }
    assert_eq!(t.merges(), &[area(0, 0, 0, 2), area(1, 0, 1, 1)]);
    // A range touching both unmerges both; one touching neither does nothing.
    assert_eq!(t.unmerge(area(5, 5, 6, 6)), None);
    let unmerge = t.unmerge(area(0, 1, 1, 1)).unwrap();
    let back = t.apply(unmerge);
    assert!(t.merges().is_empty());
    t.apply(back);
    assert_eq!(t.merges(), &[area(0, 0, 0, 2), area(1, 0, 1, 1)]);
}

#[test]
fn merging_is_refused_out_of_file_order_and_over_the_cap() {
    let mut t = table("refuse.csv", &grid_csv(), Vec::new());
    let rows = MERGE_MAX_CELLS / 4 + 1;
    assert_eq!(
        t.merge_plan(area(0, 0, rows - 1, 3)),
        Err(MergeError::TooManyCells(rows * 4))
    );
    let sort = t
        .sort_rows(
            0,
            SortOrder::Descending,
            &AtomicBool::new(false),
            &AtomicU64::new(0),
        )
        .unwrap()
        .unwrap();
    t.apply(sort);
    assert_eq!(
        t.merge_plan(area(0, 0, 0, 1)),
        Err(MergeError::NotInFileOrder)
    );
}

#[test]
fn sorting_is_blocked_by_merges_on_sorted_rows_only() {
    let t = table("sort.csv", &grid_csv(), vec![area(0, 0, 0, 3)]);
    assert!(
        t.merges_block_sort(),
        "row 1 is a data row while there is no header"
    );
    let mut t = t;
    t.set_header(true);
    assert!(
        !t.merges_block_sort(),
        "a merge in the header row never moves"
    );
    let tall = table("sort2.csv", &grid_csv(), vec![area(0, 0, 1, 0)]);
    let mut tall = tall;
    tall.set_header(true);
    assert!(tall.merges_block_sort(), "it reaches a data row");
}

#[test]
fn merge_rows_are_the_table_rows_showing_it() {
    let mut t = table(
        "view.csv",
        &grid_csv(),
        vec![area(0, 0, 0, 3), area(3, 0, 5, 1)],
    );
    assert_eq!(t.merge_rows(&area(3, 0, 5, 1)), Some(3..6));
    t.set_header(true);
    assert_eq!(
        t.merge_rows(&area(0, 0, 0, 3)),
        None,
        "the header row isn't a table row"
    );
    assert_eq!(t.merge_rows(&area(3, 0, 5, 1)), Some(2..5));
}
