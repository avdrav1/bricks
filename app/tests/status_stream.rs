//! APP-2 acceptance: the status bar's row count updates while indexing streams.
//!
//! `--bench-status` prints the status bar's row text on every status update (the first one
//! as the window is built, then every 100 ms) and quits once indexing is complete. Opens a
//! real window, so it needs a graphical session and the corpus:
//! `cargo test --release -p spreadsheet --test status_stream -- --ignored --nocapture`

use std::path::Path;
use std::process::Command;

/// Data rows in corpus/rows_1024mb.csv: 19,094,580 lines, the first a header (ENG-7).
const ROWS: u64 = 19_094_579;

#[test]
#[ignore = "opens a window and needs corpus/; run with --ignored"]
fn row_count_updates_while_indexing_streams() {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_none() {
        panic!("needs a graphical session");
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../corpus/rows_1024mb.csv");
    assert!(
        path.exists(),
        "missing {}; run python3 scripts/gen_corpus.py",
        path.display()
    );
    let out = Command::new(env!("CARGO_BIN_EXE_spreadsheet"))
        .arg(&path)
        .arg("--bench-status")
        .output()
        .expect("run spreadsheet");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    eprintln!("{stdout}");
    // (rows shown, still indexing?) per update, e.g. "STATUS 2,349,486 rows · indexing 12%".
    let updates: Vec<(u64, bool)> = stdout
        .lines()
        .filter_map(|l| l.strip_prefix("STATUS "))
        .map(|s| {
            let count = s.split(' ').next().unwrap().replace(',', "");
            (count.parse().unwrap(), s.contains("indexing"))
        })
        .collect();

    let (last, partial) = updates.split_last().expect("no status updates");
    assert_eq!(*last, (ROWS, false), "the final update shows every row");
    assert!(
        partial.iter().all(|&(_, indexing)| indexing),
        "only the last update is complete: {updates:?}"
    );
    assert!(
        partial.len() >= 2,
        "the count must change at least twice while indexing, saw {updates:?}"
    );
    assert!(
        partial.windows(2).all(|w| w[0].0 < w[1].0) && partial.last().unwrap().0 < ROWS,
        "the count grows with every update: {updates:?}"
    );
}
