//! STYLE-3 acceptance: scrolling a styled 1,048,576-row workbook (bold header on a fill,
//! number formats per column, an italic coloured column, a bold total row with a fill and
//! a border every 100 rows) keeps frame time p99 under 16 ms.
//!
//! Opens a real window, so it needs a graphical session and `corpus/styled_1m.xlsx`
//! (`cargo run --release -p xlsx --example gen_corpus_xlsx -- --styled`). Timing is
//! asserted only in optimized builds:
//! `cargo test --release -p brick --test styled_scroll -- --ignored --nocapture`

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
fn styled_scroll_frames_stay_under_16_ms() {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_none() {
        panic!("needs a graphical session");
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/styled_1m.xlsx");
    assert!(
        path.exists(),
        "missing {}; run cargo run --release -p xlsx --example gen_corpus_xlsx -- --styled",
        path.display()
    );
    let out = Command::new(env!("CARGO_BIN_EXE_brick"))
        .arg(&path)
        .args(["--bench-scroll", "600"])
        .output()
        .expect("run brick");
    let line = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    assert!(
        out.status.success() && line.starts_with('{'),
        "{line}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    eprintln!("styled_1m.xlsx: {line}");
    let p99 = cpu_p99(&line);
    eprintln!("frame CPU p99: {p99:.2} ms (target < 16 ms)");
    if !cfg!(debug_assertions) {
        assert!(p99 < 16.0, "{line}");
    }
}
