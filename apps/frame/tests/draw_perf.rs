//! FRAME-4 acceptance: drawing a dense slide takes under 16 ms p99.
//!
//! Runs `frame FILE --bench-draw 300` on each test deck (the reader's fixtures): it shows
//! the deck's densest slide and redraws the canvas every frame, reporting each frame's CPU
//! time. Opens a real window, so it needs a graphical session. Timing is asserted only in
//! optimized builds:
//! `cargo test --release -p frame --test draw_perf -- --ignored --nocapture`

use std::path::Path;
use std::process::Command;

fn p99(json: &str, field: &str) -> f64 {
    let at = json
        .find(&format!("\"{field}\":"))
        .unwrap_or_else(|| panic!("{field} in the report"));
    let rest = &json[at..];
    let p = rest.find("\"p99\":").expect("p99") + 6;
    let end = rest[p..].find([',', '}']).unwrap();
    rest[p..p + end].parse().unwrap()
}

#[test]
#[ignore = "opens a window; run with --release --ignored"]
fn drawing_the_densest_slide_takes_under_16_ms_p99() {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_none() {
        panic!("needs a graphical session");
    }
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/pptx/tests/data");
    let mut decks: Vec<_> = std::fs::read_dir(&data)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "pptx"))
        .collect();
    decks.sort();
    assert!(!decks.is_empty());
    let mut worst = (0.0, String::new());
    for deck in &decks {
        let out = Command::new(env!("CARGO_BIN_EXE_frame"))
            .arg(deck)
            .args(["--bench-draw", "300"])
            .output()
            .expect("run frame");
        let line = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        let name = deck.file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            out.status.success() && line.starts_with('{'),
            "{name}: {line}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        eprintln!("{name}: {line}");
        let cpu = p99(&line, "frame_cpu_ms");
        if cpu > worst.0 {
            worst = (cpu, name);
        }
    }
    eprintln!(
        "worst frame CPU p99: {:.2} ms ({}; target < 16 ms)",
        worst.0, worst.1
    );
    if !cfg!(debug_assertions) {
        assert!(worst.0 < 16.0, "{} takes {:.2} ms p99", worst.1, worst.0);
    }
}
