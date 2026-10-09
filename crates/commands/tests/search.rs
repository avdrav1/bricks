//! SRCH-1: find the next cell containing some text, row by row from the cursor and
//! wrapping; whole table or one column; match case or not. Values are as shown: edits
//! included, quotes unescaped, deleted columns and a filter's hidden rows left out.

use commands::{Batch, Reshape, SetCell, UndoStack};
use data_model::{CellRef, CsvTable, DelimiterChoice, Filter, Query, SearchError, SortOrder, Test};
use std::sync::atomic::{AtomicBool, AtomicU64};

const CSV: &str = "\
id,city,note
1,Denver,\"say \"\"hi\"\"\"
2,Portland,landmark
3,Boston,
4,portland,\"two
lines Land\"
5,Austin,x
";

fn open(name: &str) -> (std::path::PathBuf, CsvTable) {
    let path = std::env::temp_dir().join(format!("srch1-{}-{name}.csv", std::process::id()));
    std::fs::write(&path, CSV).unwrap();
    let mut t = CsvTable::open(&path, DelimiterChoice::Auto).unwrap();
    t.index().build(&AtomicBool::new(false)).unwrap();
    let types = t.infer_types(&AtomicBool::new(false)).unwrap();
    t.set_types(types);
    (path, t)
}

fn query(text: &str) -> Query {
    Query {
        text: text.into(),
        match_case: false,
        col: None,
    }
}

fn find(t: &CsvTable, q: &Query, from: (u64, u32)) -> Option<(u64, u32)> {
    t.snapshot()
        .find(q, from, &AtomicBool::new(false), &AtomicU64::new(0))
        .unwrap()
}

/// Every match in order, following the search from (0, 0) until it comes round again.
fn tour(t: &CsvTable, q: &Query) -> Vec<(u64, u32)> {
    let mut seen = Vec::new();
    let mut at = (0, 0);
    while let Some(next) = find(t, q, at) {
        if seen.contains(&next) {
            break;
        }
        seen.push(next);
        at = next;
    }
    seen
}

#[test]
fn finds_row_by_row_from_the_cursor_and_wraps() {
    let (path, t) = open("order");
    let land = query("land");
    assert_eq!(tour(&t, &land), [(1, 1), (1, 2), (3, 1), (3, 2)]);
    assert_eq!(
        find(&t, &land, (3, 2)),
        Some((1, 1)),
        "past the last match: wraps"
    );
    assert_eq!(
        find(&t, &land, (1, 1)),
        Some((1, 2)),
        "the rest of the cursor's row first"
    );
    assert_eq!(find(&t, &query("Paris"), (0, 0)), None);
    assert_eq!(find(&t, &query(""), (0, 0)), None);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn match_case_and_one_column() {
    let (path, t) = open("case");
    let mut q = query("Land");
    q.match_case = true;
    assert_eq!(tour(&t, &q), [(3, 2)], "only \"lines Land\"");
    let mut q = query("LAND");
    q.col = Some(t.col_id(2));
    assert_eq!(tour(&t, &q), [(1, 2), (3, 2)], "notes only");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn values_are_as_shown() {
    let (path, mut t) = open("shown");
    let mut undo = UndoStack::default();
    // Quotes unescaped: the file has `""hi""`.
    assert_eq!(find(&t, &query("\"hi\""), (4, 2)), Some((0, 2)));
    // Edits count; the file's text under an edit doesn't.
    let at = |t: &CsvTable, r, c| CellRef {
        row: t.row_id(r),
        col: t.col_id(c),
    };
    undo.execute(Box::new(SetCell::new(at(&t, 4, 2), "Landing")), &mut t);
    undo.execute(Box::new(SetCell::new(at(&t, 1, 1), "Paris")), &mut t);
    assert_eq!(tour(&t, &query("land")), [(1, 2), (3, 1), (3, 2), (4, 2)]);
    undo.undo(&mut t).unwrap();
    assert_eq!(
        tour(&t, &query("land"))[0],
        (1, 1),
        "undone: Portland again"
    );
    // An inserted row with a value, and a deleted column.
    let insert = t.insert_rows(0, 1).unwrap();
    undo.execute(Box::new(Reshape::new(insert, at(&t, 0, 0))), &mut t);
    undo.execute(Box::new(SetCell::new(at(&t, 0, 1), "Highland")), &mut t);
    let delete = t.delete_cols(2, 1).unwrap();
    undo.execute(Box::new(Reshape::new(delete, at(&t, 0, 0))), &mut t);
    assert_eq!(tour(&t, &query("land")), [(0, 1), (2, 1), (4, 1)]);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn only_the_rows_shown_in_their_order() {
    let (path, mut t) = open("view");
    let mut undo = UndoStack::default();
    let edit = t
        .snapshot()
        .sort_rows(
            1,
            SortOrder::Ascending,
            &AtomicBool::new(false),
            &AtomicU64::new(0),
        )
        .unwrap()
        .unwrap();
    let focus = CellRef {
        row: t.row_id(0),
        col: t.col_id(1),
    };
    undo.execute(Box::new(Batch::new("Sort", vec![edit], focus)), &mut t);
    // Austin, Boston, Denver, Portland, portland
    assert_eq!(tour(&t, &query("land")), [(3, 1), (3, 2), (4, 1), (4, 2)]);

    let f = Filter {
        col: t.col_id(1),
        test: Test::NotContains("port".into()),
    };
    let rows = t
        .snapshot()
        .filter_rows(&f, &AtomicBool::new(false), &AtomicU64::new(0))
        .unwrap();
    t.set_filter(f, rows);
    assert_eq!(
        find(&t, &query("land"), (0, 0)),
        None,
        "the matches are hidden"
    );
    assert_eq!(
        tour(&t, &query("s")),
        [(0, 1), (1, 1), (2, 2)],
        "Austin, Boston, Denver's \"say\"; not the hidden \"lines\""
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn a_cancelled_search_returns_nothing() {
    let (path, t) = open("cancel");
    let r = t.snapshot().find(
        &query("land"),
        (0, 0),
        &AtomicBool::new(true),
        &AtomicU64::new(0),
    );
    assert_eq!(r, Err(SearchError::Cancelled));
    std::fs::remove_file(path).unwrap();
}
