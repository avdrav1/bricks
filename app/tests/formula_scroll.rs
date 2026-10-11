//! FML-4 acceptance: scrolling a sheet whose visible rows are formulas (60 visible formula
//! cells and more) keeps frame time p99 under 16 ms.
//!
//! Opens a real window, so it needs a graphical session and `corpus/formulas.csv`
//! (`python3 scripts/gen_corpus.py`): 1,000,000 rows of `=B{n}*C{n}`, a running total
//! `=E{n-1}+D{n}`, and `=IF(...)`. The scroll starts mid-file, where the running total
//! first needs a background job; frames are measured after it settles. Timing is asserted
//! only in optimized builds:
//! `cargo test --release -p bricks --test formula_scroll -- --ignored --nocapture`

use std::path::Path;
use std::process::Command;

fn cpu_p99(json: &str) -> f64 {
    let at = json
        .find("\"frame_cpu_ms\":")
        .expect("frame_cpu_ms in the report");
    let rest = &json[at..];
    let p99 = rest.find("\"p99\":").expect("p99") + 6;
    let end = rest[p99..].find([',', '}']).unwrap();
    rest[p99..p99 + end].parse().unwrap()
}

#[test]
#[ignore = "opens a window and needs corpus/; run with --ignored"]
fn formula_scroll_frames_stay_under_16_ms() {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_none() {
        panic!("needs a graphical session");
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../corpus/formulas.csv");
    assert!(
        path.exists(),
        "missing {}; run python3 scripts/gen_corpus.py",
        path.display()
    );
    let out = Command::new(env!("CARGO_BIN_EXE_bricks"))
        .arg(&path)
        .args(["--bench-scroll", "600"])
        .output()
        .expect("run bricks");
    let line = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    assert!(
        out.status.success() && line.starts_with('{'),
        "{line}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    eprintln!("formulas.csv: {line}");
    let p99 = cpu_p99(&line);
    eprintln!("frame CPU p99: {p99:.2} ms (target < 16 ms)");
    if !cfg!(debug_assertions) {
        assert!(p99 < 16.0, "{line}");
    }
}
