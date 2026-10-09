//! FILT-1 at scale: filtering the 1 GB file, one column and then two (the benchmark's
//! "two-column filter": city = Portland and revenue > 50,000, under 3 s), and how long the
//! view takes to follow a change of row order (it runs on the UI thread).
//!
//! Needs the corpus (`python3 scripts/gen_corpus.py`); wants an optimized build:
//! `cargo test --release -p commands --test filter_1gb -- --ignored --nocapture`

use data_model::{
    Compare, CsvTable, DelimiterChoice, Filter, RowBlock, SortOrder, TableSource, Test,
};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::time::{Duration, Instant};

fn apply(t: &mut CsvTable, col: u32, test: Test) -> Duration {
    let start = Instant::now();
    let f = Filter {
        col: t.col_id(col),
        test,
    };
    let rows = t
        .snapshot()
        .filter_rows(&f, &AtomicBool::new(false), &AtomicU64::new(0))
        .unwrap();
    t.set_filter(f, rows);
    start.elapsed()
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

#[test]
#[ignore = "needs corpus/rows_1024mb.csv; run with --release --ignored"]
fn two_column_filter_on_1gb() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/rows_1024mb.csv");
    assert!(
        path.exists(),
        "missing {}; run python3 scripts/gen_corpus.py",
        path.display()
    );
    let mut t = CsvTable::open(&path, DelimiterChoice::Auto).unwrap();
    t.index().build(&AtomicBool::new(false)).unwrap();
    let types = t.infer_types(&AtomicBool::new(false)).unwrap();
    t.set_types(types);
    let all = t.row_count();

    let one = apply(&mut t, 2, Test::Equals("Portland".into()));
    let portland = t.row_count();
    let two = apply(&mut t, 3, Test::Number(Compare::Gt, 50_000.0));
    let both = t.row_count();
    eprintln!(
        "city = Portland: {:.0} ms, {portland} of {all} rows; and revenue > 50000: {:.0} ms, {both} rows; together {:.0} ms (target 3000)",
        ms(one),
        ms(two),
        ms(one + two)
    );
    // Every shown row passes both; the counts agree with a plain scan.
    let mut b = RowBlock::default();
    t.read_rows(0..both, &mut b);
    for r in 0..both {
        assert_eq!(b.cell(r, 2), Some("Portland"));
        assert!(b.cell(r, 3).unwrap().parse::<f64>().unwrap() > 50_000.0);
    }
    t.clear_filter(2);
    t.clear_filter(3);
    let mut scan = (0, 0);
    for start in (0..all).step_by(65_536) {
        let end = all.min(start + 65_536);
        t.read_rows(start..end, &mut b);
        for r in start..end {
            if b.cell(r, 2) == Some("Portland") {
                scan.0 += 1;
                if b.cell(r, 3).unwrap().parse::<f64>().unwrap() > 50_000.0 {
                    scan.1 += 1;
                }
            }
        }
    }
    assert_eq!((portland, both), scan);

    // The view follows a sort (rebuilt on the UI thread when the order changes).
    apply(&mut t, 2, Test::Equals("Portland".into()));
    apply(&mut t, 3, Test::Number(Compare::Gt, 50_000.0));
    let edit = t
        .snapshot()
        .sort_rows(
            4,
            SortOrder::Ascending,
            &AtomicBool::new(false),
            &AtomicU64::new(0),
        )
        .unwrap()
        .unwrap();
    let start = Instant::now();
    t.apply(edit);
    eprintln!(
        "view rebuilt after a sort of 19.1M rows: {:.1} ms",
        ms(start.elapsed())
    );
    assert_eq!(t.row_count(), both);

    if !cfg!(debug_assertions) {
        assert!(one + two < Duration::from_secs(3), "{:?}", one + two);
    }
}
