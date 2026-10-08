//! SAVE-1 on the 1 GB corpus: 1,000 edits save byte-identical except the edited cells, and
//! the save (write, fsync, verify, rename) fits the BENCHMARKS target of 15 s.
//!
//! Copies `corpus/rows_1024mb.csv` next to itself (same disk, so fsync is real; /tmp may
//! be RAM) and removes the copy afterwards. Timing is asserted only in optimized builds:
//! `cargo test --release -p file-format --test save_perf -- --ignored --nocapture`

use csv_engine::{Dialect, RowIndex, Source, SparseRowIndex};
use data_model::{CellRef, CsvTable, Edit};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn open(path: &Path) -> (Arc<SparseRowIndex>, CsvTable) {
    let index = SparseRowIndex::new(Arc::new(Source::open(path).unwrap()), &Dialect::default());
    index.build(&AtomicBool::new(false)).unwrap();
    (index.clone(), CsvTable::new(index, Dialect::default()))
}

#[test]
#[ignore = "needs corpus/ and 1 GB of free disk; run with --ignored"]
fn save_1gb_with_1000_edits() {
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/rows_1024mb.csv");
    let path = corpus.with_file_name(format!(".save-perf-{}.csv", std::process::id()));
    std::fs::copy(&corpus, &path).unwrap();
    let (old_index, mut table) = open(&path);
    let rows = old_index.row_count();

    let mut edits = BTreeMap::new();
    let mut x = 1u64;
    while edits.len() < 1_000 {
        x = x
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let (row, col) = (1 + (x >> 33) % (rows - 1), ((x >> 13) % 8) as u32);
        let value = format!("edit {}, \"{row}\"", edits.len());
        table.apply(Edit::set(
            CellRef {
                row: table.row_id(row),
                col: table.col_id(col),
            },
            value.as_str(),
        ));
        edits.insert((row, col), value);
    }

    let t = Instant::now();
    let stats = file_format::save_csv(
        &path,
        &table.save_job().unwrap(),
        &data_model::SaveProgress::default(),
    )
    .unwrap();
    let took = t.elapsed();
    eprintln!(
        "saved {} rows, {} bytes with 1,000 edits in {took:.2?} (incl. fsync + verify re-index)",
        stats.rows, stats.bytes
    );

    // The old mapping still reads the original bytes after the rename (ADR 0002).
    let (new_index, new_table) = open(&path);
    assert_eq!(new_index.row_count(), rows);
    let (old, new) = (old_index.source().bytes(), new_index.source().bytes());
    let (mut a, mut b) = (Vec::new(), Vec::new());
    let mut start = 0;
    while start < rows {
        let end = (start + 65_536).min(rows);
        old_index.row_spans(start..end, &mut a).unwrap();
        new_index.row_spans(start..end, &mut b).unwrap();
        for (r, (sa, sb)) in (start..end).zip(a.iter().zip(&b)) {
            if edits.range((r, 0)..(r + 1, 0)).next().is_none() {
                let (ra, rb) = (
                    &old[sa.start as usize..sa.end as usize],
                    &new[sb.start as usize..sb.end as usize],
                );
                assert!(ra == rb, "untouched row {r} changed");
            }
        }
        start = end;
    }
    for (&(r, c), v) in &edits {
        assert_eq!(
            new_table.cell_value(r, c).as_deref(),
            Some(v.as_str()),
            "edit at row {r} col {c}"
        );
    }
    drop((old_index, table, new_index, new_table));
    std::fs::remove_file(&path).unwrap();

    if cfg!(debug_assertions) {
        eprintln!("debug build: {took:?} not checked against 15 s; use --release");
    } else {
        assert!(took < Duration::from_secs(15), "save took {took:?}");
    }
}
