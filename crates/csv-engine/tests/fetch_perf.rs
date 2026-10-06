//! ENG-2 acceptance: 100 visible rows in under 5 ms from anywhere in the file.
//!
//! Needs the corpus (`python3 scripts/gen_corpus.py`). Timing is asserted only in
//! optimized builds: `cargo test --release -p csv-engine --test fetch_perf -- --ignored --nocapture`

use csv_engine::{Dialect, RowIndex, Source, SparseRowIndex};
use std::hint::black_box;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

const TARGET: Duration = Duration::from_millis(5);
const WINDOW: u64 = 100;

fn splitmix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// Drop the file from the page cache. Pages mapped into this process survive
/// `POSIX_FADV_DONTNEED`, so first drop our page-table entries for the mapping.
fn evict(path: &Path, mapped: &[u8]) {
    let f = std::fs::File::open(path).unwrap();
    // SAFETY: MADV_DONTNEED on a read-only shared file mapping only drops page-table
    // entries; the next access re-reads from the file. fadvise is advisory.
    #[allow(unsafe_code)]
    unsafe {
        libc::madvise(
            mapped.as_ptr() as *mut libc::c_void,
            mapped.len(),
            libc::MADV_DONTNEED,
        );
        libc::posix_fadvise(f.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED);
    }
}

/// Fetch one window and touch every byte of it, as a renderer splitting cells would.
fn window(index: &SparseRowIndex, start: u64, spans: &mut Vec<std::ops::Range<u64>>) -> Duration {
    let t = Instant::now();
    let n = index.row_spans(start..start + WINDOW, spans).unwrap();
    let bytes = index.source().bytes();
    let mut commas = 0;
    for s in spans.iter() {
        commas += memchr::memchr_iter(b',', &bytes[s.start as usize..s.end as usize]).count();
    }
    black_box(commas);
    assert_eq!(n as u64, WINDOW);
    t.elapsed()
}

fn pct(xs: &mut [Duration], p: f64) -> Duration {
    xs.sort();
    xs[((p / 100.0) * (xs.len() - 1) as f64).round() as usize]
}

#[test]
#[ignore = "needs corpus/; run with --ignored"]
fn hundred_rows_from_anywhere_under_5ms() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/rows_1024mb.csv");
    let index = SparseRowIndex::new(Arc::new(Source::open(&path).unwrap()), &Dialect::default());
    index.build(&AtomicBool::new(false)).unwrap();
    let rows = index.row_count();
    let mut spans = Vec::with_capacity(WINDOW as usize);

    // Warm: the file is in the page cache after indexing, as it is in the app after open.
    // Includes the very first and very last windows.
    let mut warm: Vec<Duration> = (0..1_000u64)
        .map(|k| match k {
            0 => 0,
            1 => rows - WINDOW,
            _ => splitmix(k) % (rows - WINDOW),
        })
        .map(|s| window(&index, s, &mut spans))
        .collect();

    // Cold: page cache dropped before every window (memory pressure, or a cold reopen).
    // Reported, not asserted: it measures the disk.
    let mut cold: Vec<Duration> = (0..100u64)
        .map(|k| {
            evict(&path, index.source().bytes());
            window(&index, splitmix(k ^ 0xC01D) % (rows - WINDOW), &mut spans)
        })
        .collect();

    let worst = *warm.iter().max().unwrap();
    eprintln!(
        "100-row window over {rows} rows: warm p50 {:?} p99 {:?} max {worst:?}; cold p50 {:?} p99 {:?} max {:?}",
        pct(&mut warm, 50.0),
        pct(&mut warm, 99.0),
        pct(&mut cold, 50.0),
        pct(&mut cold, 99.0),
        cold.iter().max().unwrap(),
    );
    if cfg!(debug_assertions) {
        eprintln!("debug build: timing not checked against {TARGET:?}; use --release");
    } else {
        assert!(worst < TARGET, "worst warm window {worst:?}");
    }
}
