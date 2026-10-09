//! SORT-1 acceptance at scale: a 2M-row numeric sort finishes in under 3 s, is stable, and
//! undoes; the whole 1 GB file (19.1M rows) sorts within the 10 s benchmark target and
//! saves in its new order.
//!
//! Needs the corpus (`python3 scripts/gen_corpus.py`) and about 1 GB free in the temp
//! directory; wants an optimized build:
//! `cargo test --release -p commands --test sort_1gb -- --ignored --nocapture --test-threads=1`

use commands::{Batch, Reshape, UndoStack};
use data_model::{CellRef, CsvTable, DelimiterChoice, RowBlock, SortOrder, TableSource};
use std::io::{BufRead, BufReader, BufWriter};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::time::{Duration, Instant};

fn open() -> CsvTable {
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
    t
}

/// Columns `id` (0) and `col` of every row, in table order.
fn read(t: &mut CsvTable, col: u32) -> Vec<(u64, f64)> {
    let n = t.row_count();
    let mut b = RowBlock::default();
    let mut out = Vec::with_capacity(n as usize);
    for start in (0..n).step_by(65_536) {
        let end = n.min(start + 65_536);
        t.read_rows(start..end, &mut b);
        for r in start..end {
            out.push((
                b.cell(r, 0).unwrap().parse().unwrap(),
                b.cell(r, col).unwrap().parse().unwrap(),
            ));
        }
    }
    out
}

/// Sort through the undo stack, as the app does; returns the time the sort took.
fn sort(t: &mut CsvTable, undo: &mut UndoStack<CsvTable>, col: u32, order: SortOrder) -> Duration {
    let start = Instant::now();
    let edit = t
        .snapshot()
        .sort_rows(col, order, &AtomicBool::new(false), &AtomicU64::new(0))
        .unwrap()
        .expect("the order changes");
    let took = start.elapsed();
    let focus = CellRef {
        row: t.row_id(0),
        col: t.col_id(col),
    };
    undo.execute(Box::new(Batch::new("Sort", vec![edit], focus)), t);
    took
}

fn check(took: Duration, target: Duration, what: &str) {
    eprintln!(
        "{what}: {:.0} ms (target {:.0} ms)",
        took.as_secs_f64() * 1e3,
        target.as_secs_f64() * 1e3
    );
    if !cfg!(debug_assertions) {
        assert!(took < target, "{what} took {took:?}");
    }
}

#[test]
#[ignore = "needs corpus/rows_1024mb.csv; run with --release --ignored"]
fn two_million_row_numeric_sort_is_fast_stable_and_undoable() {
    let mut t = open();
    let mut undo = UndoStack::default();
    // Keep the first 2,000,000 rows, as ADR 0004's spike did.
    let rows = t.row_count();
    let cut = t.delete_rows(2_000_000, rows - 2_000_000).unwrap();
    let focus = CellRef {
        row: t.row_id(0),
        col: t.col_id(0),
    };
    undo.execute(Box::new(Reshape::new(cut, focus)), &mut t);
    assert_eq!(t.row_count(), 2_000_000);
    let before = read(&mut t, 3);

    // revenue: integers 0..99999, so about 20 rows share each value.
    let took = sort(&mut t, &mut undo, 3, SortOrder::Ascending);
    check(took, Duration::from_secs(3), "2M-row numeric sort");
    let after = read(&mut t, 3);
    assert_eq!(after.len(), before.len());
    for w in after.windows(2) {
        let ((id_a, a), (id_b, b)) = (w[0], w[1]);
        assert!(a <= b, "ascending");
        if a == b {
            assert!(id_a < id_b, "stable: equal values keep file order");
        }
    }
    let mut ids: Vec<u64> = after.iter().map(|r| r.0).collect();
    ids.sort_unstable();
    assert!(ids.iter().copied().eq(0..2_000_000), "every row once");

    let start = Instant::now();
    undo.undo(&mut t).unwrap();
    eprintln!("undo: {:.1} ms", start.elapsed().as_secs_f64() * 1e3);
    assert_eq!(read(&mut t, 3), before, "undo restores the order");
}

#[test]
#[ignore = "needs corpus/rows_1024mb.csv and ~1 GB of temp space; run with --release --ignored"]
fn the_whole_file_sorts_and_saves_in_its_new_order() {
    let mut t = open();
    let mut undo = UndoStack::default();
    let rows = t.row_count();
    let took = sort(&mut t, &mut undo, 4, SortOrder::Descending); // score, decimal
    check(took, Duration::from_secs(10), "19.1M-row numeric sort");
    let took = sort(&mut t, &mut undo, 2, SortOrder::Ascending); // city, text, re-sorted
    check(
        took,
        Duration::from_secs(10),
        "19.1M-row text sort of a sorted table",
    );

    let out = std::env::temp_dir().join(format!("sort1-1gb-{}.csv", std::process::id()));
    let start = Instant::now();
    let stats = t
        .save_job()
        .unwrap()
        .write_to(
            &mut BufWriter::new(std::fs::File::create(&out).unwrap()),
            &data_model::SaveProgress::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
    check(
        start.elapsed(),
        Duration::from_secs(15),
        "save of the sorted 1 GB file",
    );
    assert_eq!(stats.rows, rows + 1);

    // The saved file: the header, then city ascending, score descending within a city.
    let mut lines = BufReader::new(std::fs::File::open(&out).unwrap()).lines();
    assert_eq!(
        lines.next().unwrap().unwrap(),
        "id,name,city,revenue,score,date,active,code"
    );
    let (mut n, mut last) = (0u64, (String::new(), f64::INFINITY));
    for line in lines {
        let line = line.unwrap();
        let f: Vec<&str> = line.split(',').collect();
        let (city, score) = (f[2].to_owned(), f[4].parse::<f64>().unwrap());
        assert!(
            city > last.0 || (city == last.0 && score <= last.1),
            "row {n}: {line}"
        );
        last = (city, score);
        n += 1;
    }
    std::fs::remove_file(&out).unwrap();
    assert_eq!(n, rows);
}
