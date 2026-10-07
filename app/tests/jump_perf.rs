//! GRID-2 acceptance: a scrollbar jump to row 1.9M renders in under 100 ms.
//!
//! A scrollbar drag moves the vertical adjustment; `--bench-jump` does the same, then
//! times from the move to the end of the first frame that paints the target row at the top.
//! Opens a real window, so it needs a graphical session and the corpus. Keep the window
//! visible while it runs. Timing is asserted only in optimized builds:
//! `cargo test --release -p spreadsheet --test jump_perf -- --ignored --nocapture`

use std::path::Path;
use std::process::Command;

const TARGET_MS: f64 = 100.0;

fn field(json: &str, key: &str) -> f64 {
    let start = json
        .find(&format!("\"{key}\":"))
        .unwrap_or_else(|| panic!("no {key} in {json}"))
        + key.len()
        + 3;
    let rest = &json[start..];
    rest[..rest.find([',', '}']).unwrap()].parse().unwrap()
}

#[test]
#[ignore = "opens a window and needs corpus/; run with --ignored"]
fn jump_to_row_1_9m_renders_under_100ms() {
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
        .args(["--bench-jump", "1900000", "--jumps", "100"])
        .output()
        .expect("run spreadsheet");
    let line = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    assert!(
        out.status.success() && line.starts_with('{'),
        "{line}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    eprintln!("{line}");

    // Every jump lands: the painted top row is the requested one.
    assert_eq!(field(&line, "landed"), field(&line, "jumps"), "{line}");
    let first = field(&line, "first_jump_ms");
    let worst = field(&line, "max");
    if cfg!(debug_assertions) {
        eprintln!("debug build: {first} ms / worst {worst} ms not checked against {TARGET_MS} ms; use --release");
        return;
    }
    // A visible window gets a frame every refresh; a gap of seconds means it was hidden
    // (another workspace, minimized) and the run says nothing about the grid.
    assert!(
        worst < 1_000.0,
        "a jump waited {worst} ms for a frame: keep the window visible and rerun"
    );
    assert!(first < TARGET_MS, "jump to row 1,900,000 took {first} ms");
    assert!(
        worst < TARGET_MS,
        "slowest of the random jumps took {worst} ms"
    );
}
