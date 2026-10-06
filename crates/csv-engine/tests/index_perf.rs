//! ENG-1 acceptance: 1 GB indexed in under 10 s with the index under 200 MB.
//!
//! Needs the corpus (`python3 scripts/gen_corpus.py`). Timing targets are for the shipped
//! (optimized) build, so they are asserted only without debug assertions:
//! `cargo test --release -p csv-engine --test index_perf -- --ignored --nocapture`
//! A debug run still checks row counts and index size.

use csv_engine::{Dialect, RowIndex, Source, SparseRowIndex};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

const TARGET: Duration = Duration::from_secs(10);
const INDEX_BUDGET: usize = 200 * 1024 * 1024;

fn within_target(took: Duration) -> bool {
    if cfg!(debug_assertions) {
        eprintln!("debug build: {took:?} not checked against {TARGET:?}; use --release");
        return true;
    }
    took < TARGET
}

fn corpus(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus")
        .join(name)
}

/// Drop the file from the page cache so the measurement includes reading it from disk.
fn evict(path: &Path) {
    let f = std::fs::File::open(path).unwrap();
    // SAFETY: valid fd; advisory call with no memory effects.
    #[allow(unsafe_code)]
    unsafe {
        libc::posix_fadvise(f.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED);
    }
}

fn index_cold(path: &Path) -> (Arc<SparseRowIndex>, Duration) {
    evict(path);
    let t = Instant::now();
    let source = Arc::new(Source::open(path).unwrap());
    let index = SparseRowIndex::new(source, &Dialect::default());
    index.build(&AtomicBool::new(false)).unwrap();
    (index, t.elapsed())
}

fn report(label: &str, path: &Path, index: &SparseRowIndex, took: Duration) {
    let mb = std::fs::metadata(path).unwrap().len() as f64 / 1e6;
    eprintln!(
        "{label}: {mb:.0} MB, {} rows, indexed cold in {:.2} s ({:.0} MB/s), index {:.2} MiB",
        index.row_count(),
        took.as_secs_f64(),
        mb / took.as_secs_f64(),
        index.heap_bytes() as f64 / (1024.0 * 1024.0),
    );
}

#[test]
#[ignore = "needs corpus/; run with --ignored"]
fn corpus_1gb_indexes_under_10s_in_under_200mb() {
    let path = corpus("rows_1024mb.csv");
    let (index, took) = index_cold(&path);
    report("rows_1024mb.csv", &path, &index, took);

    // The corpus has no quoted fields, so every newline ends a row.
    let bytes = std::fs::read(&path).unwrap();
    let expected = memchr::memchr_iter(b'\n', &bytes).count() as u64;
    assert!(index.is_complete());
    assert_eq!(index.row_count(), expected);
    assert!(within_target(took), "took {took:?}");
    assert!(
        index.heap_bytes() < INDEX_BUDGET,
        "index {} bytes",
        index.heap_bytes()
    );
}

#[test]
#[ignore = "writes a 1 GiB temp file; run with --ignored"]
fn quote_heavy_1gb_indexes_under_10s() {
    let path = std::env::temp_dir().join(format!("eng1-quoted-{}.csv", std::process::id()));
    let mut rows = 0u64;
    {
        let mut w = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
        writeln!(w, "id,name,note,quote,amount").unwrap();
        rows += 1;
        let mut written = 0u64;
        while written < 1 << 30 {
            let line = format!(
                "{rows},\"Surname, Given {}\",\"line one\nline two, with comma\",\"she said \"\"hi\"\" {}\",{}.{:02}\n",
                rows % 977,
                rows % 13,
                rows % 100_000,
                rows % 100
            );
            w.write_all(line.as_bytes()).unwrap();
            written += line.len() as u64;
            rows += 1;
        }
    }
    let (index, took) = index_cold(&path);
    report("quote-heavy", &path, &index, took);
    let ok = index.row_count() == rows && within_target(took) && index.heap_bytes() < INDEX_BUDGET;
    std::fs::remove_file(&path).unwrap();
    assert!(
        ok,
        "rows {} (expected {rows}), took {took:?}, index {} bytes",
        index.row_count(),
        index.heap_bytes()
    );
}
