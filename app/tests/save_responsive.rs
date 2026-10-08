//! SAVE-3 acceptance: the UI stays responsive during a 1 GB save, and the save takes under
//! 15 s.
//!
//! Opens a real window on a copy of `corpus/rows_1024mb.csv` in the temp directory, pastes
//! 1,000 cells, saves, and scrolls while the save runs (`--bench-save`). Needs a graphical
//! session (Wayland, X11, or Broadway via `GDK_BACKEND`) and the corpus
//! (`python3 scripts/gen_corpus.py`). Main-loop lateness is asserted only in optimized
//! builds: `cargo test --release -p spreadsheet --test save_responsive -- --ignored --nocapture`
//!
//! Frame rates are printed, not asserted: Broadway paints at a few frames a second.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::Command;

const ROWS: u64 = 19_094_580;

/// A number in the bench's JSON: `key` at the top level, or inside the object `within`.
fn field(json: &str, within: Option<&str>, key: &str) -> f64 {
    let from = within.map_or(0, |o| {
        json.find(&format!("\"{o}\":{{"))
            .unwrap_or_else(|| panic!("no {o} in {json}"))
    });
    let start = from
        + json[from..]
            .find(&format!("\"{key}\":"))
            .unwrap_or_else(|| panic!("no {key} in {json}"))
        + key.len()
        + 3;
    let rest = &json[start..];
    let end = rest.find([',', '}']).unwrap();
    rest[..end].parse().unwrap()
}

#[test]
#[ignore = "opens a window, needs corpus/ and 1 GB in the temp dir; run with --ignored"]
fn ui_stays_responsive_during_a_1gb_save() {
    assert!(
        ["WAYLAND_DISPLAY", "DISPLAY", "GDK_BACKEND"]
            .iter()
            .any(|v| std::env::var_os(v).is_some()),
        "needs a graphical session"
    );
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../corpus/rows_1024mb.csv");
    let copy = std::env::temp_dir().join(format!("save-responsive-{}.csv", std::process::id()));
    std::fs::copy(&corpus, &copy).expect("copy corpus/rows_1024mb.csv (run gen_corpus.py)");

    let out = Command::new(env!("CARGO_BIN_EXE_spreadsheet"))
        .arg(&copy)
        .args(["--bench-save", "1000"])
        .output()
        .expect("run spreadsheet");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout
        .lines()
        .find_map(|l| l.strip_prefix("SAVE "))
        .unwrap_or_else(|| {
            panic!(
                "no SAVE line; stdout: {stdout}\nstderr: {}",
                String::from_utf8_lossy(&out.stderr)
            )
        })
        .to_owned();
    eprintln!("{line}");

    let (mut lines, mut edited) = (0u64, 0u64);
    for l in BufReader::with_capacity(1 << 20, std::fs::File::open(&copy).unwrap()).split(b'\n') {
        let l = l.unwrap();
        lines += 1;
        edited += u64::from(l.windows(6).any(|w| w == b"edited"));
    }
    std::fs::remove_file(&copy).unwrap();

    assert!(!line.contains("\"error\""), "{line}");
    assert!(field(&line, None, "progress_samples") >= 2.0, "{line}");
    let save_ms = field(&line, None, "save_ms");
    assert!(save_ms < 15_000.0, "save took {save_ms} ms");
    assert_eq!(lines, ROWS);
    assert_eq!(edited, 1_000);
    let (p99, max) = (
        field(&line, Some("loop_late_ms"), "p99"),
        field(&line, Some("loop_late_ms"), "max"),
    );
    if cfg!(debug_assertions) {
        eprintln!("debug build: lateness p99 {p99} ms, max {max} ms not checked; use --release");
    } else {
        assert!(p99 < 16.0, "main loop p99 late {p99} ms");
        assert!(max < 100.0, "main loop max late {max} ms");
    }
}
