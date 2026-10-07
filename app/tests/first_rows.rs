//! APP-3 acceptance: the first rows of the 1 GB file are on screen in under 500 ms, before
//! indexing finishes.
//!
//! Drops the file from the page cache, then times from spawning `--bench-open` to its
//! `FIRST_FRAME` line, printed at the end of the first frame that painted rows, along with
//! how far indexing had got by then. Opens a real window, so it needs a graphical session
//! and the corpus. Timing is asserted only in optimized builds:
//! `cargo test --release -p spreadsheet --test first_rows -- --ignored --nocapture`

use std::io::{BufRead, BufReader};
use std::os::fd::AsRawFd;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

const TARGET_MS: f64 = 500.0;
const RUNS: usize = 3;

/// Drop `path` from the page cache so the open reads from disk.
fn evict(path: &Path) {
    let f = std::fs::File::open(path).unwrap();
    // SAFETY: valid fd; advisory call with no memory effects.
    #[allow(unsafe_code)]
    unsafe {
        libc::posix_fadvise(f.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED);
    }
}

fn field(json: &str, key: &str) -> String {
    let start = json
        .find(&format!("\"{key}\":"))
        .unwrap_or_else(|| panic!("no {key} in {json}"))
        + key.len()
        + 3;
    let rest = &json[start..];
    rest[..rest.find([',', '}']).unwrap()].to_owned()
}

#[test]
#[ignore = "opens a window and needs corpus/; run with --ignored"]
fn first_rows_of_1gb_visible_in_under_500ms_before_indexing_finishes() {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_none() {
        panic!("needs a graphical session");
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../corpus/rows_1024mb.csv");
    assert!(
        path.exists(),
        "missing {}; run python3 scripts/gen_corpus.py",
        path.display()
    );

    let mut times = Vec::new();
    for _ in 0..RUNS {
        evict(&path);
        let t = Instant::now();
        let mut child = Command::new(env!("CARGO_BIN_EXE_spreadsheet"))
            .arg(&path)
            .arg("--bench-open")
            .stdout(Stdio::piped())
            .spawn()
            .expect("run spreadsheet");
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let first = lines
            .by_ref()
            .map(Result::unwrap)
            .find(|l| l.starts_with("FIRST_FRAME"))
            .expect("no FIRST_FRAME");
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let opened = lines
            .map(Result::unwrap)
            .find(|l| l.starts_with("OPENED"))
            .expect("no OPENED");
        assert!(child.wait().unwrap().success());
        eprintln!("{ms:.0} ms: {first}");

        // Rows were painted while the index was still being built.
        let (indexed, total) = (field(&first, "rows_indexed"), field(&opened, "rows"));
        let (indexed, total): (u64, u64) = (indexed.parse().unwrap(), total.parse().unwrap());
        assert_eq!(field(&first, "complete"), "false", "{first}");
        assert!(
            indexed > 0 && indexed < total,
            "{indexed} of {total} rows indexed"
        );
        times.push(ms);
    }

    times.sort_by(f64::total_cmp);
    let median = times[RUNS / 2];
    if cfg!(debug_assertions) {
        eprintln!(
            "debug build: median {median:.0} ms not checked against {TARGET_MS} ms; use --release"
        );
        return;
    }
    assert!(
        median < TARGET_MS,
        "first rows took {median:.0} ms (median of {RUNS}, runs {times:?})"
    );
}
