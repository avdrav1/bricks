//! SRCH-1 acceptance: the first match in the 1 GB file in under a second. The search goes
//! over the whole file every time (it counts every row's matches), so the worst cases are
//! a value near the end, one that isn't there, and one in millions of rows. SRCH-2 adds
//! the match's place and the total, and a sorted table, where finding the next match and
//! counting the ones before it read rows out of file order.
//!
//! Needs the corpus (`python3 scripts/gen_corpus.py`); wants an optimized build:
//! `cargo test --release -p data-model --test search_1gb -- --ignored --nocapture`

use data_model::{CsvTable, DelimiterChoice, Hit, Query, SearchProgress, SortOrder, Step};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::time::{Duration, Instant};

/// Cells with "portland" (the city column; no other column has it).
const PORTLAND: u64 = 2_725_768;

fn first(t: &CsvTable, q: &Query, from: (u64, u32), step: Step) -> (Option<Hit>, u64, Duration) {
    let no = AtomicBool::new(false);
    let start = Instant::now();
    let mut snap = t.snapshot();
    let m = snap.search(q, &no, &SearchProgress::default()).unwrap();
    let hit = m.step(&mut snap, from, step, None, &no).unwrap();
    (hit, m.total(), start.elapsed())
}

#[test]
#[ignore = "needs corpus/rows_1024mb.csv; run with --release --ignored"]
fn first_match_in_1gb_under_a_second() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/rows_1024mb.csv");
    assert!(
        path.exists(),
        "missing {}; run python3 scripts/gen_corpus.py",
        path.display()
    );
    let mut t = CsvTable::open(&path, DelimiterChoice::Auto).unwrap();
    t.index().build(&AtomicBool::new(false)).unwrap();
    let q = |t: &CsvTable, text: &str, match_case, col: Option<u32>| Query {
        text: text.into(),
        match_case,
        col: col.map(|c| t.col_id(c)),
    };
    let at = |h: Option<Hit>| h.map(|h| (h.row, h.col, h.ordinal));
    let mut slowest = Duration::ZERO;
    let mut check = |what: &str, (hit, total, took): (Option<Hit>, u64, Duration)| {
        eprintln!(
            "{what}: {:?} of {total} in {:.0} ms",
            at(hit),
            took.as_secs_f64() * 1e3
        );
        slowest = slowest.max(took);
        (at(hit), total)
    };

    let near_end = q(&t, "19094000", false, None);
    let portland = q(&t, "portland", false, None);
    let r = check(
        "an id near the end",
        first(&t, &near_end, (0, 0), Step::Here),
    );
    assert_eq!(r, (Some((19_094_000, 0, 1)), 1));
    let r = check(
        "not there",
        first(&t, &q(&t, "Zanzibar", false, None), (0, 0), Step::Here),
    );
    assert_eq!(r, (None, 0));
    let r = check("portland", first(&t, &portland, (0, 0), Step::Here));
    assert_eq!(r, (Some((5, 2, 1)), PORTLAND));
    let r = check(
        "portland, back from the top",
        first(&t, &portland, (0, 0), Step::Previous),
    );
    assert_eq!(r.0.map(|(_, c, n)| (c, n)), Some((2, PORTLAND)));
    let r = check(
        "match case",
        first(&t, &q(&t, "PORTLAND", true, None), (0, 0), Step::Here),
    );
    assert_eq!(r, (None, 0));
    let r = check(
        "one column",
        first(&t, &q(&t, "00999", false, Some(7)), (0, 0), Step::Here),
    );
    assert!(r.0.is_some_and(|(_, c, n)| c == 7 && n == 1), "{r:?}");

    // Sorted by score: rows out of file order.
    let types = t.infer_types(&AtomicBool::new(false)).unwrap();
    t.set_types(types);
    let order = t
        .snapshot()
        .sort_rows(
            4,
            SortOrder::Ascending,
            &AtomicBool::new(false),
            &AtomicU64::new(0),
        )
        .unwrap()
        .unwrap();
    t.apply(order);
    let r = check(
        "sorted: id near the end",
        first(&t, &near_end, (0, 0), Step::Here),
    );
    assert_eq!((r.0.map(|(_, c, n)| (c, n)), r.1), (Some((0, 1)), 1));
    let r = check("sorted: portland", first(&t, &portland, (0, 0), Step::Here));
    assert_eq!((r.0.map(|(_, c, n)| (c, n)), r.1), (Some((2, 1)), PORTLAND));
    let r = check(
        "sorted: portland, back from the top",
        first(&t, &portland, (0, 0), Step::Previous),
    );
    assert_eq!(r.0.map(|(_, c, n)| (c, n)), Some((2, PORTLAND)));

    if !cfg!(debug_assertions) {
        assert!(slowest < Duration::from_secs(1), "{slowest:?}");
    }
}
