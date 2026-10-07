//! EDIT-3 acceptance: inserting a row at row 1,000,000 of the 1 GB file takes under 50 ms,
//! including reading back the screen of rows around it the way the grid redraws.
//!
//! Needs the corpus (`python3 scripts/gen_corpus.py`); indexing 1 GB wants an optimized
//! build: `cargo test --release -p commands --test insert_1gb -- --ignored --nocapture`

use commands::{ChangeRows, UndoStack};
use data_model::{CsvTable, DelimiterChoice, RowBlock, TableSource};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

const TARGET_MS: f64 = 50.0;
const ROW: u64 = 1_000_000;

fn screen(table: &mut CsvTable) -> RowBlock {
    let mut block = RowBlock::default();
    table.read_rows(ROW - 30..ROW + 30, &mut block);
    block
}

fn row(block: &RowBlock, r: u64) -> Vec<String> {
    (0..block.cells_in_row(r))
        .map(|c| block.cell(r, c).unwrap().to_owned())
        .collect()
}

#[test]
#[ignore = "needs corpus/rows_1024mb.csv; run with --release --ignored"]
fn insert_at_row_1m_in_under_50ms() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/rows_1024mb.csv");
    assert!(
        path.exists(),
        "missing {}; run python3 scripts/gen_corpus.py",
        path.display()
    );
    let mut table = CsvTable::open(&path, DelimiterChoice::Auto).unwrap();
    table.index().build(&AtomicBool::new(false)).unwrap();
    let rows = table.row_count();
    let before = screen(&mut table);
    let mut undo = UndoStack::default();

    let t = Instant::now();
    let edit = table.insert_rows(ROW, 1).expect("indexed");
    undo.execute(Box::new(ChangeRows::new(edit, 0)), &mut table);
    let after = screen(&mut table);
    let insert_ms = t.elapsed().as_secs_f64() * 1e3;

    assert_eq!(table.row_count(), rows + 1);
    assert_eq!(
        row(&after, ROW),
        Vec::<String>::new(),
        "the new row is empty"
    );
    assert_eq!(
        row(&after, ROW + 1),
        row(&before, ROW),
        "row 1M moved down one"
    );
    assert_eq!(row(&after, ROW - 1), row(&before, ROW - 1));

    let t = Instant::now();
    let edit = table.delete_rows(ROW, 1).unwrap();
    undo.execute(Box::new(ChangeRows::new(edit, 0)), &mut table);
    let delete_ms = t.elapsed().as_secs_f64() * 1e3;
    let t = Instant::now();
    assert!(undo.undo(&mut table).is_some());
    assert!(undo.undo(&mut table).is_some());
    let undo_ms = t.elapsed().as_secs_f64() * 1e3;
    assert_eq!(table.row_count(), rows);
    assert_eq!(row(&screen(&mut table), ROW), row(&before, ROW));
    eprintln!("insert at row 1M + screen read: {insert_ms:.3} ms; delete {delete_ms:.3} ms; two undos {undo_ms:.3} ms");

    if cfg!(debug_assertions) {
        eprintln!(
            "debug build: {insert_ms:.1} ms not checked against {TARGET_MS} ms; use --release"
        );
        return;
    }
    assert!(insert_ms < TARGET_MS, "insert took {insert_ms:.1} ms");
}
