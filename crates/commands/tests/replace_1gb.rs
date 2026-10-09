//! SRCH-3 on the 1 GB corpus: replace all on ~190k cells (codes 00990–00999 in column H)
//! is one undo step, and how long each part takes: the search, building the edit (job
//! pool), applying it and undoing and redoing it (UI thread). "portland" has 2,725,768
//! cells, over the limit, so it is refused.
//!
//! Needs the corpus (`python3 scripts/gen_corpus.py`):
//! `cargo test --release -p commands --test replace_1gb -- --ignored --nocapture`

use commands::{Batch, UndoStack};
use data_model::{
    CellRef, CsvTable, DelimiterChoice, Query, ReplaceError, SearchProgress, REPLACE_MAX_CELLS,
};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::time::{Duration, Instant};

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

#[test]
#[ignore = "needs corpus/rows_1024mb.csv; run with --release --ignored"]
fn replace_all_in_1gb_is_one_step() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/rows_1024mb.csv");
    assert!(
        path.exists(),
        "missing {}; run python3 scripts/gen_corpus.py",
        path.display()
    );
    let mut t = CsvTable::open(&path, DelimiterChoice::Auto).unwrap();
    t.index().build(&AtomicBool::new(false)).unwrap();
    let no = AtomicBool::new(false);
    let search = |t: &CsvTable, text: &str, col: Option<u32>| {
        let q = Query {
            text: text.into(),
            match_case: false,
            col: col.map(|c| t.col_id(c)),
        };
        t.snapshot()
            .search(&q, &no, &SearchProgress::default())
            .unwrap()
    };

    let portland = search(&t, "portland", None);
    let r = portland.replace_all(&t.snapshot(), "x", &no, &AtomicU64::new(0));
    assert_eq!(r, Err(ReplaceError::TooMany(2_725_768)));
    assert!(portland.total() > REPLACE_MAX_CELLS);

    let start = Instant::now();
    let m = search(&t, "0099", Some(7));
    let searched = start.elapsed();
    let hits = m.total();
    assert!((100_000..=REPLACE_MAX_CELLS).contains(&hits), "{hits}");
    let start = Instant::now();
    let edit = m
        .replace_all(&t.snapshot(), "ZZ99", &no, &AtomicU64::new(0))
        .unwrap()
        .unwrap();
    let built = start.elapsed();

    let mut undo = UndoStack::default();
    let focus = CellRef {
        row: t.row_id(0),
        col: t.col_id(7),
    };
    let start = Instant::now();
    undo.execute(
        Box::new(Batch::new("Replace all", vec![edit], focus)),
        &mut t,
    );
    let applied = start.elapsed();
    assert_eq!(undo.len(), 1, "one step");
    assert_eq!(search(&t, "ZZ99", Some(7)).total(), hits);
    assert_eq!(search(&t, "0099", Some(7)).total(), 0);

    let start = Instant::now();
    undo.undo(&mut t).unwrap();
    let undone = start.elapsed();
    assert_eq!(t.changes(), 0, "one undo takes back every cell");
    assert_eq!(search(&t, "0099", Some(7)).total(), hits);
    let start = Instant::now();
    undo.redo(&mut t).unwrap();
    let redone = start.elapsed();
    assert_eq!(search(&t, "ZZ99", Some(7)).total(), hits);

    eprintln!(
        "{hits} cells: search {:.0} ms, build {:.0} ms, apply {:.0} ms, undo {:.0} ms, redo {:.0} ms; undo holds {:.1} MB",
        ms(searched),
        ms(built),
        ms(applied),
        ms(undone),
        ms(redone),
        undo.heap_bytes() as f64 / 1e6
    );
}
