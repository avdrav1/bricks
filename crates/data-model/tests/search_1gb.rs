//! SRCH-1 acceptance: the first match in the 1 GB file in under a second. The search goes
//! over the whole file every time (it marks every row with a match), so the worst cases
//! are a value near the end, one that isn't there, and one in millions of rows.
//!
//! Needs the corpus (`python3 scripts/gen_corpus.py`); wants an optimized build:
//! `cargo test --release -p data-model --test search_1gb -- --ignored --nocapture`

use data_model::{CsvTable, DelimiterChoice, Query, TableSource};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::time::{Duration, Instant};

#[test]
#[ignore = "needs corpus/rows_1024mb.csv; run with --release --ignored"]
fn first_match_in_1gb_under_a_second() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/rows_1024mb.csv");
    assert!(
        path.exists(),
        "missing {}; run python3 scripts/gen_corpus.py",
        path.display()
    );
    let t = CsvTable::open(&path, DelimiterChoice::Auto).unwrap();
    t.index().build(&AtomicBool::new(false)).unwrap();
    let last = t.row_count() - 1;

    let q = |text: &str, match_case, col: Option<u32>| Query {
        text: text.into(),
        match_case,
        col: col.map(|c| t.col_id(c)),
    };
    let cases = [
        (
            "an id near the end",
            q("19094000", false, None),
            Some((19_094_000, 0)),
        ),
        ("not there", q("Zanzibar", false, None), None),
        (
            "Portland (2.7M rows)",
            q("portland", false, None),
            Some((5, 2)),
        ),
        ("match case", q("PORTLAND", true, None), None),
        ("one column", q("00999", false, Some(7)), None),
    ];
    let mut slowest = Duration::ZERO;
    for (what, query, want) in cases {
        let start = Instant::now();
        let found = t
            .snapshot()
            .find(&query, (0, 0), &AtomicBool::new(false), &AtomicU64::new(0))
            .unwrap();
        let took = start.elapsed();
        eprintln!("{what}: {found:?} in {:.0} ms", took.as_secs_f64() * 1e3);
        if what != "one column" {
            assert_eq!(found, want, "{what}");
        } else {
            assert!(
                found.is_some_and(|(r, c)| c == 7 && r <= last),
                "{what}: {found:?}"
            );
        }
        slowest = slowest.max(took);
    }
    if !cfg!(debug_assertions) {
        assert!(slowest < Duration::from_secs(1), "{slowest:?}");
    }
}
