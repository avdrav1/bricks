//! SRCH-1 and SRCH-2: find cells containing some text, row by row from the cursor and
//! wrapping, forwards or backwards; whole table or one column; match case or not. Values
//! are as shown: edits included, quotes unescaped, deleted columns and rows and a filter's
//! hidden rows left out. Each match knows its place among them ("2 of 4").

use commands::{Batch, Reshape, SetCell, UndoStack};
use data_model::{
    CellRef, CsvTable, DelimiterChoice, Filter, Hit, Matches, Query, SearchError, SearchProgress,
    SortOrder, Step, Test,
};
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
    let path = std::env::temp_dir().join(format!("srch-{}-{name}.csv", std::process::id()));
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

fn search(t: &CsvTable, q: &Query) -> Matches {
    t.snapshot()
        .search(q, &AtomicBool::new(false), &SearchProgress::default())
        .unwrap()
}

fn step(t: &CsvTable, m: &Matches, from: (u64, u32), step: Step, hint: Option<Hit>) -> Option<Hit> {
    m.step(&mut t.snapshot(), from, step, hint, &AtomicBool::new(false))
        .unwrap()
}

/// The next match after `from`.
fn find(t: &CsvTable, q: &Query, from: (u64, u32)) -> Option<(u64, u32)> {
    step(t, &search(t, q), from, Step::Next, None).map(|h| (h.row, h.col))
}

/// Every match in order, following Next from (0, 0) until it comes round again. Checks
/// each match's place on the way, counted from scratch and from the one before, and the
/// total.
fn tour(t: &CsvTable, q: &Query) -> Vec<(u64, u32)> {
    let m = search(t, q);
    let mut seen: Vec<Hit> = Vec::new();
    let mut at = (0, 0);
    while let Some(hit) = step(t, &m, at, Step::Next, None) {
        if seen.iter().any(|h| (h.row, h.col) == (hit.row, hit.col)) {
            break;
        }
        if let Some(&prev) = seen.last() {
            let hinted = step(t, &m, (prev.row, prev.col), Step::Next, Some(prev));
            assert_eq!(hinted, Some(hit), "from the match before");
        }
        seen.push(hit);
        at = (hit.row, hit.col);
    }
    let mut sorted = seen.clone();
    sorted.sort_by_key(|h| h.ordinal);
    let ordinals: Vec<u64> = sorted.iter().map(|h| h.ordinal).collect();
    assert_eq!(
        ordinals,
        (1..=seen.len() as u64).collect::<Vec<_>>(),
        "places"
    );
    assert_eq!(m.total(), seen.len() as u64, "total");
    sorted.iter().map(|h| (h.row, h.col)).collect()
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
fn previous_goes_back_and_wraps_to_the_last() {
    let (path, t) = open("back");
    let m = search(&t, &query("land"));
    let mut back = Vec::new();
    let (mut at, mut hint) = ((0, 0), None);
    while let Some(h) = step(&t, &m, at, Step::Previous, hint) {
        if back.contains(&(h.row, h.col, h.ordinal)) {
            break;
        }
        back.push((h.row, h.col, h.ordinal));
        (at, hint) = ((h.row, h.col), Some(h));
    }
    assert_eq!(back, [(3, 2, 4), (3, 1, 3), (1, 2, 2), (1, 1, 1)]);
    let from_cold = step(&t, &m, (3, 1), Step::Previous, None);
    assert_eq!(
        from_cold.map(|h| (h.row, h.col, h.ordinal)),
        Some((1, 2, 2))
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn typing_keeps_a_match_under_the_cursor() {
    let (path, t) = open("here");
    let m = search(&t, &query("land"));
    let here = |from| step(&t, &m, from, Step::Here, None).map(|h| (h.row, h.col, h.ordinal));
    assert_eq!(here((1, 1)), Some((1, 1, 1)), "the cursor's own cell first");
    assert_eq!(here((1, 0)), Some((1, 1, 1)));
    assert_eq!(here((3, 2)), Some((3, 2, 4)));
    assert_eq!(here((4, 0)), Some((1, 1, 1)), "nothing after it: wraps");
    let none = search(&t, &query("Paris"));
    assert_eq!(none.total(), 0);
    assert_eq!(step(&t, &none, (0, 0), Step::Here, None), None);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn matches_go_stale_when_the_table_changes() {
    let (path, mut t) = open("stale");
    let mut undo = UndoStack::default();
    let m = search(&t, &query("land"));
    assert!(m.is_current(&t) && m.is_current(&t.snapshot()));
    let at = CellRef {
        row: t.row_id(0),
        col: t.col_id(1),
    };
    undo.execute(Box::new(SetCell::new(at, "Highland")), &mut t);
    assert!(!m.is_current(&t), "an edit");
    let m = search(&t, &query("land"));
    undo.undo(&mut t).unwrap();
    assert!(!m.is_current(&t), "an undo");
    let m = search(&t, &query("land"));
    t.set_header(false);
    assert!(!m.is_current(&t), "the header row");
    let m = search(&t, &query("land"));
    let f = Filter {
        col: t.col_id(1),
        test: Test::NonEmpty,
    };
    let rows = t
        .snapshot()
        .filter_rows(&f, &AtomicBool::new(false), &AtomicU64::new(0))
        .unwrap();
    t.set_filter(f, rows);
    assert!(!m.is_current(&t), "a filter");
    let (path2, other) = open("stale-other");
    assert!(
        !search(&other, &query("land")).is_current(&t),
        "another table"
    );
    std::fs::remove_file(path).unwrap();
    std::fs::remove_file(path2).unwrap();
}

#[test]
fn deleted_rows_are_not_counted() {
    let (path, mut t) = open("deleted");
    let mut undo = UndoStack::default();
    let delete = t.delete_rows(1, 1).unwrap(); // Portland, landmark
    let at = CellRef {
        row: t.row_id(0),
        col: t.col_id(0),
    };
    undo.execute(Box::new(Reshape::new(delete, at)), &mut t);
    assert_eq!(tour(&t, &query("land")), [(2, 1), (2, 2)]);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn the_count_streams_and_a_cancelled_search_returns_nothing() {
    let (path, t) = open("cancel");
    let progress = SearchProgress::default();
    let m = t
        .snapshot()
        .search(&query("land"), &AtomicBool::new(false), &progress)
        .unwrap();
    assert_eq!(progress.found.into_inner(), 4);
    assert_eq!(
        progress.rows.into_inner(),
        6,
        "every file row, the header too"
    );
    assert_eq!(m.total(), 4);
    let r = t.snapshot().search(
        &query("land"),
        &AtomicBool::new(true),
        &SearchProgress::default(),
    );
    assert!(matches!(r, Err(SearchError::Cancelled)));
    std::fs::remove_file(path).unwrap();
}
