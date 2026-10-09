//! ENG-8 acceptance: every long job cancels within 200 ms. Index, save, and copy each run
//! as a job on the shared pool over the 1 GB corpus and are cancelled at several points:
//! early, midway, late, and for the save also while it fsyncs and while it verifies. The
//! time counted is from `Job::cancel` until the job's result is back, which is when the UI
//! hears of it.
//!
//! Needs the corpus (`python3 scripts/gen_corpus.py`) and about 1 GB free under `target/`.
//! The save writes there, on a real disk, so its fsync is real:
//! `cargo test --release -p jobs --test cancel_1gb -- --ignored --nocapture`

use csv_engine::{Dialect, IndexError, RowIndex, Source, SparseRowIndex};
use data_model::{CellRef, CsvTable, DelimiterChoice, Edit, SaveProgress, TableSource};
use file_format::{save_csv, SaveError, TEMP_SUFFIX};
use jobs::{Job, Stopped};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const TARGET: Duration = Duration::from_millis(200);

fn corpus() -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/rows_1024mb.csv");
    assert!(
        path.exists(),
        "missing {}; run python3 scripts/gen_corpus.py",
        path.display()
    );
    path
}

fn open(path: &Path) -> CsvTable {
    let t = CsvTable::open(path, DelimiterChoice::Auto).unwrap();
    t.index().build(&AtomicBool::new(false)).unwrap();
    t
}

/// Wait until `reached`, cancel, and time until the job's result is back: its own, or
/// `None` if the cancel came before it started.
fn cancel_when<T>(job: &Job<T>, reached: impl Fn() -> bool) -> (Duration, Option<T>) {
    let give_up = Instant::now() + Duration::from_secs(120);
    while !reached() {
        assert!(Instant::now() < give_up, "the job never reached the point");
        std::thread::sleep(Duration::from_micros(200));
    }
    let t = Instant::now();
    job.cancel();
    let out = match job.wait() {
        Ok(out) => Some(out),
        Err(Stopped::Cancelled) => None,
        Err(Stopped::Panicked) => panic!("the job panicked"),
    };
    (t.elapsed(), out)
}

fn check(times: &[(String, Duration)]) {
    for (what, took) in times {
        eprintln!("{what}: cancelled in {:.2} ms", took.as_secs_f64() * 1e3);
    }
    if cfg!(debug_assertions) {
        eprintln!("debug build: not checked against {TARGET:?}; use --release");
        return;
    }
    for (what, took) in times {
        assert!(*took < TARGET, "{what}: {took:?}");
    }
}

#[test]
#[ignore = "needs corpus/rows_1024mb.csv; run with --release --ignored"]
fn indexing_cancels_within_200ms() {
    let path = corpus();
    let len = std::fs::metadata(&path).unwrap().len();
    let mut times = Vec::new();
    for pct in [0, 10, 50, 90] {
        let index =
            SparseRowIndex::new(Arc::new(Source::open(&path).unwrap()), &Dialect::default());
        let job = jobs::spawn({
            let index = index.clone();
            move |cancel| index.build(cancel.flag())
        });
        let (took, out) = cancel_when(&job, || index.bytes_indexed() * 100 >= len * pct);
        assert!(
            matches!(out, None | Some(Err(IndexError::Cancelled))),
            "at {pct}%: {out:?}"
        );
        assert!(!index.is_complete());
        times.push((format!("index at {pct}%"), took));
    }
    check(&times);
}

#[test]
#[ignore = "needs corpus/rows_1024mb.csv and ~1 GB free under target/; run with --release --ignored"]
fn saving_cancels_within_200ms_in_every_phase() {
    let corpus = corpus();
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let path = dir.join(format!("eng8-save-{}.csv", std::process::id()));
    std::fs::copy(&corpus, &path).unwrap();
    let len = std::fs::metadata(&path).unwrap().len();
    let mut table = open(&path);
    let rows = table.row_count();
    for i in 0..1_000u64 {
        let at = CellRef {
            row: table.row_id(1 + i * (rows / 1_000 - 1)),
            col: table.col_id((i % 8) as u32),
        };
        table.apply(Edit::set(at, "edited"));
    }

    let mut times = Vec::new();
    // By share written; then once writing has stopped (the fsync); then while verifying.
    let points = ["10%", "50%", "90%", "fsync", "verify"];
    for point in points {
        let save = table.save_job().unwrap();
        let estimate = save.estimated_bytes();
        let progress = Arc::new(SaveProgress::default());
        let job = jobs::spawn({
            let (path, progress) = (path.clone(), progress.clone());
            move |cancel| save_csv(&path, &save, &progress, cancel.flag())
        });
        let last = AtomicU64::new(0);
        let still = AtomicU64::new(0);
        let (took, out) = cancel_when(&job, || match point {
            "verify" => progress.is_checking(),
            "fsync" => {
                // Every byte out and the count still for 2 ms, but not verifying yet.
                let w = progress.written();
                let unchanged = last.swap(w, Ordering::Relaxed) == w;
                let n = if unchanged {
                    still.fetch_add(1, Ordering::Relaxed) + 1
                } else {
                    still.store(0, Ordering::Relaxed);
                    0
                };
                w * 100 >= estimate * 99 && n >= 10 && !progress.is_checking()
            }
            pct => {
                let pct: u64 = pct.trim_end_matches('%').parse().unwrap();
                progress.written() * 100 >= estimate * pct
            }
        });
        assert!(
            matches!(out, Some(Err(SaveError::Cancelled))),
            "{point}: {out:?}"
        );
        assert_eq!(std::fs::metadata(&path).unwrap().len(), len, "{point}");
        times.push((format!("save at {point}"), took));
    }
    let leftovers: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            let p = p.to_string_lossy();
            p.contains("eng8-save") && p.ends_with(TEMP_SUFFIX)
        })
        .collect();
    assert_eq!(leftovers, Vec::<PathBuf>::new());
    drop(table);
    std::fs::remove_file(&path).unwrap();
    check(&times);
}

#[test]
#[ignore = "needs corpus/rows_1024mb.csv; run with --release --ignored"]
fn copying_cancels_within_200ms() {
    let table = open(&corpus());
    let rows = table.row_count();
    let mut times = Vec::new();
    for pct in [0, 10, 50, 90] {
        let mut snapshot = table.snapshot();
        let done = Arc::new(AtomicU64::new(0));
        let job = jobs::spawn({
            let done = done.clone();
            move |cancel| snapshot.copy_range(0..rows, 0.., cancel.flag(), &done)
        });
        let (took, out) = cancel_when(&job, || done.load(Ordering::Relaxed) * 100 >= rows * pct);
        assert!(matches!(out, None | Some(None)), "at {pct}%: copied anyway");
        times.push((format!("copy of every row at {pct}%"), took));
    }
    check(&times);
}
