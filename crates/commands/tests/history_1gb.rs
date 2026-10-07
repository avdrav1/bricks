//! CMD-2 acceptance: a 1,000-step history stays under 50 MB on a 1 GB file, because it
//! holds operations, not snapshots.
//!
//! Needs the corpus (`python3 scripts/gen_corpus.py`); indexing 1 GB wants an optimized
//! build: `cargo test --release -p commands --test history_1gb -- --ignored --nocapture`

use commands::{SetCell, UndoStack, HISTORY_BYTES};
use data_model::{CellRef, CsvTable, DelimiterChoice, TableSource};
use std::path::Path;
use std::sync::atomic::AtomicBool;

/// Anonymous memory of this process (heap, not the mapped file), in bytes.
fn rss_anon() -> usize {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let kib: usize = status
        .lines()
        .find_map(|l| l.strip_prefix("RssAnon:"))
        .and_then(|v| v.split_whitespace().next()?.parse().ok())
        .unwrap();
    kib * 1024
}

#[test]
#[ignore = "needs corpus/rows_1024mb.csv; run with --release --ignored"]
fn thousand_step_history_on_1gb_stays_under_50mb() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/rows_1024mb.csv");
    assert!(
        path.exists(),
        "missing {}; run python3 scripts/gen_corpus.py",
        path.display()
    );
    let mut table = CsvTable::open(&path, DelimiterChoice::Auto).unwrap();
    table.index().build(&AtomicBool::new(false)).unwrap();
    let rows = table.row_count();
    assert!(rows > 19_000_000, "{rows} rows");

    let before = rss_anon();
    let mut undo = UndoStack::default();
    let mut cells = std::collections::HashSet::new();
    for i in 0..1_000u64 {
        // Spread over the whole file; every tenth step edits again the cell edited nine
        // steps earlier, so its inverse holds a value.
        let k = if i % 10 == 9 { i - 9 } else { i };
        let at = CellRef {
            row: table.row_id(k * 19_001 % rows),
            col: (k % 8) as u32,
        };
        cells.insert(at);
        let value = format!("edit {i}: {}", "x".repeat(100));
        undo.execute(Box::new(SetCell::new(at, value)), &mut table);
    }
    let grown = rss_anon().saturating_sub(before);
    eprintln!(
        "1,000 steps: history {} KiB by its own count, process anonymous memory +{} KiB",
        undo.heap_bytes() / 1024,
        grown / 1024
    );

    assert_eq!(undo.len(), 1_000, "every step kept");
    assert!(undo.heap_bytes() < HISTORY_BYTES);
    assert!(
        grown < HISTORY_BYTES,
        "edits and history grew the process by {grown} bytes"
    );

    // All of it undoes, and redoes.
    while undo.undo(&mut table).is_some() {}
    assert_eq!(table.overlay().len(), 0);
    while undo.redo(&mut table).is_some() {}
    assert_eq!(table.overlay().len(), cells.len());
}
