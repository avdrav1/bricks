//! SAVE-3: a cancelled save leaves the original file as it was and no temp file behind.
//!
//! The 1 GB case measures how fast a cancel takes effect mid-write:
//! `cargo test --release -p file-format --test save_cancel -- --ignored --nocapture`

use data_model::{CellRef, CsvTable, DelimiterChoice, Edit, SaveProgress, TableSource};
use file_format::{save_csv, SaveError, TEMP_SUFFIX};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn open(path: &Path) -> CsvTable {
    let t = CsvTable::open(path, DelimiterChoice::Auto).unwrap();
    t.index().build(&AtomicBool::new(false)).unwrap();
    t
}

fn set(t: &mut CsvTable, row: u64, col: u32, value: &str) {
    let at = CellRef {
        row: t.row_id(row),
        col: t.col_id(col),
    };
    t.apply(Edit::set(at, value));
}

fn temp_files(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().ends_with(TEMP_SUFFIX))
        .collect()
}

#[test]
fn a_cancelled_save_leaves_the_original_and_no_temp_file() {
    let dir = std::env::temp_dir().join(format!("save3-cancel-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.csv");
    let original = b"id,name\n1,Ana\n2,Bo\n";
    std::fs::write(&path, original).unwrap();
    let mut table = open(&path);
    set(&mut table, 1, 1, "Zoe");

    let progress = SaveProgress::default();
    progress.cancel();
    let r = save_csv(&path, &table.save_job().unwrap(), &progress);

    assert!(matches!(r, Err(SaveError::Cancelled)), "{r:?}");
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(temp_files(&dir), Vec::<PathBuf>::new());
    drop(table);
    std::fs::remove_dir_all(dir).unwrap();
}

/// The first and last `n` bytes of a file.
fn ends(path: &Path, n: u64) -> (Vec<u8>, Vec<u8>) {
    let mut f = std::fs::File::open(path).unwrap();
    let (mut head, mut tail) = (vec![0; n as usize], vec![0; n as usize]);
    f.read_exact(&mut head).unwrap();
    f.seek(SeekFrom::End(-(n as i64))).unwrap();
    f.read_exact(&mut tail).unwrap();
    (head, tail)
}

#[test]
#[ignore = "needs corpus/ and 1 GB of free space in the temp dir; run with --ignored"]
fn cancel_stops_a_1gb_save_within_200ms() {
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/rows_1024mb.csv");
    let dir = std::env::temp_dir();
    let path = dir.join(format!("save3-cancel-1gb-{}.csv", std::process::id()));
    std::fs::copy(&corpus, &path).unwrap();
    let mut table = open(&path);
    let rows = table.row_count();
    for i in 0..1_000u64 {
        set(
            &mut table,
            1 + i * (rows / 1_000 - 1),
            (i % 8) as u32,
            "edited",
        );
    }

    let job = table.save_job().unwrap();
    let progress = Arc::new(SaveProgress::default());
    let saving = std::thread::spawn({
        let (path, progress) = (path.clone(), progress.clone());
        move || save_csv(&path, &job, &progress)
    });
    while progress.written() <= 200 << 20 {
        assert!(!saving.is_finished(), "the save ended before 200 MiB");
        std::thread::sleep(Duration::from_millis(1));
    }
    let t = Instant::now();
    progress.cancel();
    let r = saving.join().unwrap();
    let took = t.elapsed();
    eprintln!("cancel took effect in {took:.2?}");

    assert!(matches!(r, Err(SaveError::Cancelled)), "{r:?}");
    let len = std::fs::metadata(&corpus).unwrap().len();
    assert_eq!(std::fs::metadata(&path).unwrap().len(), len);
    assert!(
        ends(&path, 4 << 20) == ends(&corpus, 4 << 20),
        "the file changed"
    );
    let leftovers: Vec<PathBuf> = temp_files(&dir)
        .into_iter()
        .filter(|p| p.to_string_lossy().contains("save3-cancel-1gb"))
        .collect();
    assert_eq!(leftovers, Vec::<PathBuf>::new());
    drop(table);
    std::fs::remove_file(&path).unwrap();

    if cfg!(debug_assertions) {
        eprintln!("debug build: {took:?} not checked against 200 ms; use --release");
    } else {
        assert!(took < Duration::from_millis(200), "cancel took {took:?}");
    }
}
