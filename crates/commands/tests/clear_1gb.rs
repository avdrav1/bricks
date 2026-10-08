//! EDIT-5 at scale: Delete on a whole column of the 1 GB file (19 million cells) is one
//! command holding one block, not an edit per cell, so it is instant, adds next to no
//! memory, and one undo takes it back. The save writes the column empty in every row and
//! everything else as it was.
//!
//! Needs the corpus (`python3 scripts/gen_corpus.py`) and about 1 GB free in the temp
//! directory; wants an optimized build:
//! `cargo test --release -p commands --test clear_1gb -- --ignored --nocapture`

use commands::{ClearCells, UndoStack};
use data_model::{CellRef, CsvTable, DelimiterChoice, RowBlock, TableSource};
use std::io::{BufRead, BufReader, BufWriter};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

fn rss_anon() -> usize {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let kib: usize = status
        .lines()
        .find_map(|l| l.strip_prefix("RssAnon:"))
        .and_then(|v| v.split_whitespace().next()?.parse().ok())
        .unwrap();
    kib * 1024
}

fn row(table: &mut CsvTable, r: u64) -> Vec<String> {
    let mut block = RowBlock::default();
    table.read_rows(r..r + 1, &mut block);
    (0..block.cells_in_row(r))
        .map(|c| block.cell(r, c).unwrap().to_owned())
        .collect()
}

#[test]
#[ignore = "needs corpus/rows_1024mb.csv and ~1 GB of temp space; run with --release --ignored"]
fn clear_a_whole_column_of_1gb_in_one_step() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/rows_1024mb.csv");
    assert!(
        path.exists(),
        "missing {}; run python3 scripts/gen_corpus.py",
        path.display()
    );
    let mut table = CsvTable::open(&path, DelimiterChoice::Auto).unwrap();
    table.index().build(&AtomicBool::new(false)).unwrap();
    let rows = table.row_count();
    let last = rows - 1;
    let before = row(&mut table, last);
    let mut undo = UndoStack::default();
    let cursor = CellRef {
        row: table.row_id(0),
        col: table.col_id(2),
    };

    // Column C (city), every row.
    let mem = rss_anon();
    let t = Instant::now();
    let clear = table.clear_cells(0..rows, 2..3).unwrap();
    undo.execute(Box::new(ClearCells::new(clear, cursor)), &mut table);
    let clear_ms = t.elapsed().as_secs_f64() * 1e3;
    let grown = rss_anon().saturating_sub(mem);

    let mut want = before.clone();
    want[2].clear();
    assert_eq!(
        row(&mut table, last),
        want,
        "the last row reads with city empty"
    );
    assert_eq!(table.cell_value(0, 2).as_deref(), Some(""));
    assert_eq!(
        table.header_cell(2).as_deref(),
        Some("city"),
        "the header is kept"
    );
    assert!(clear_ms < 50.0, "the clear took {clear_ms:.1} ms");
    assert!(grown < 1 << 20, "the clear grew memory by {grown} bytes");
    let held = undo.heap_bytes();

    let t = Instant::now();
    undo.undo(&mut table).unwrap();
    let undo_ms = t.elapsed().as_secs_f64() * 1e3;
    assert_eq!(
        row(&mut table, last),
        before,
        "one undo brings every value back"
    );
    assert_eq!(table.changes(), 0);
    undo.redo(&mut table).unwrap();

    let out = std::env::temp_dir().join(format!("clear-1gb-{}.csv", std::process::id()));
    let t = Instant::now();
    let stats = table
        .save_job()
        .unwrap()
        .write_to(
            &mut BufWriter::new(std::fs::File::create(&out).unwrap()),
            &data_model::SaveProgress::default(),
        )
        .unwrap();
    let save_s = t.elapsed().as_secs_f64();
    eprintln!(
        "clear column C of {rows} rows: {clear_ms:.3} ms, +{} KiB, {held} B of history; undo {undo_ms:.3} ms; save (every row re-assembled): {save_s:.2} s, {} MB",
        grown / 1024,
        stats.bytes >> 20
    );

    let (mut n, mut first, mut tail, mut nonempty) = (0u64, String::new(), String::new(), 0u64);
    for line in BufReader::new(std::fs::File::open(&out).unwrap()).lines() {
        let line = line.unwrap();
        if n == 0 {
            first = line.clone();
        } else if !line.split(',').nth(2).unwrap_or_default().is_empty() {
            nonempty += 1;
        }
        tail = line;
        n += 1;
    }
    std::fs::remove_file(&out).unwrap();
    assert_eq!(n, rows + 1, "every row saved, header included");
    assert_eq!(first, "id,name,city,revenue,score,date,active,code");
    assert_eq!(nonempty, 0, "no data row keeps a city");
    assert_eq!(tail, want.join(","));
}
