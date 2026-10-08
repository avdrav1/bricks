//! EDIT-4 acceptance: column inserts and deletes work on the 1 GB file without a full
//! rewrite before save. The edits change only the column map: they take microseconds and
//! no memory to speak of, and every row reads back with its columns moved. Only the save
//! re-assembles rows, once, on the way to disk.
//!
//! Needs the corpus (`python3 scripts/gen_corpus.py`) and about 1 GB free in the temp
//! directory; wants an optimized build:
//! `cargo test --release -p commands --test columns_1gb -- --ignored --nocapture`

use commands::{Reshape, SetCell, UndoStack};
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
fn insert_and_delete_columns_on_1gb_without_rewriting_until_save() {
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
        col: table.col_id(0),
    };

    let mem = rss_anon();
    let t = Instant::now();
    // id, name, NEW, city, revenue, [score deleted], date, active, code
    let insert = table.insert_cols(2, 1).unwrap();
    undo.execute(Box::new(Reshape::new(insert, cursor)), &mut table);
    let at = CellRef {
        row: table.row_id(last),
        col: table.col_id(2),
    };
    undo.execute(Box::new(SetCell::new(at, "new, \"quoted\"")), &mut table);
    let delete = table.delete_cols(5, 1).unwrap();
    undo.execute(Box::new(Reshape::new(delete, cursor)), &mut table);
    let edit_ms = t.elapsed().as_secs_f64() * 1e3;
    let grown = rss_anon().saturating_sub(mem);

    let mut want = before.clone();
    want.insert(2, "new, \"quoted\"".into());
    want.remove(5);
    assert_eq!(
        row(&mut table, last),
        want,
        "the last row reads with its columns moved"
    );
    assert_eq!(table.header_cell(5).as_deref(), Some("date"));
    assert!(edit_ms < 50.0, "the column edits took {edit_ms:.1} ms");
    assert!(
        grown < 1 << 20,
        "the column edits grew memory by {grown} bytes"
    );

    let out = std::env::temp_dir().join(format!("columns-1gb-{}.csv", std::process::id()));
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
        "insert + edit + delete columns: {edit_ms:.3} ms, +{} KiB; save (every row re-assembled): {save_s:.2} s, {} rows, {} MB",
        grown / 1024,
        stats.rows,
        stats.bytes >> 20
    );

    let lines = BufReader::new(std::fs::File::open(&out).unwrap()).lines();
    let (mut n, mut first, mut tail) = (0u64, String::new(), String::new());
    for line in lines {
        let line = line.unwrap();
        if n == 0 {
            first = line.clone();
        }
        tail = line;
        n += 1;
    }
    std::fs::remove_file(&out).unwrap();
    assert_eq!(n, rows + 1, "every row saved, header included");
    assert_eq!(first, "id,name,,city,revenue,date,active,code");
    let source_last: Vec<&str> = before.iter().map(String::as_str).collect();
    assert_eq!(
        tail,
        format!(
            "{},{},\"new, \"\"quoted\"\"\",{},{},{},{},{}",
            source_last[0],
            source_last[1],
            source_last[2],
            source_last[3],
            source_last[5],
            source_last[6],
            source_last[7]
        )
    );
}
