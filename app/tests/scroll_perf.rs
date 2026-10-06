//! GRID-1 acceptance: constant memory regardless of row count; 60 fps scrolling at 2M rows.
//!
//! Opens a real window, so it needs a graphical session (Wayland or X11) and the corpus
//! (`python3 scripts/gen_corpus.py`). Keep the window visible while it runs: a hidden
//! window gets no frames. Frame timing is asserted only in optimized builds:
//! `cargo test --release -p spreadsheet --test scroll_perf -- --ignored --nocapture`

use std::path::Path;
use std::process::Command;

const FRAMES: &str = "600";

struct Run {
    rows: u64,
    refresh_hz: f64,
    frames: f64,
    mean_fps: f64,
    missed: f64,
    rss_anon_kib: f64,
    line: String,
}

fn field(json: &str, key: &str) -> f64 {
    let start = json
        .find(&format!("\"{key}\":"))
        .unwrap_or_else(|| panic!("no {key} in {json}"))
        + key.len()
        + 3;
    let rest = &json[start..];
    let end = rest.find([',', '}']).unwrap();
    rest[..end].parse().unwrap()
}

fn bench(file: &str) -> Run {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../corpus")
        .join(file);
    assert!(
        path.exists(),
        "missing {}; run python3 scripts/gen_corpus.py",
        path.display()
    );
    let out = Command::new(env!("CARGO_BIN_EXE_spreadsheet"))
        .arg(&path)
        .args(["--bench-scroll", FRAMES])
        .output()
        .expect("run spreadsheet");
    let line = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    assert!(
        out.status.success() && line.starts_with('{'),
        "{line}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    eprintln!("{file}: {line}");
    Run {
        rows: field(&line, "rows") as u64,
        refresh_hz: field(&line, "refresh_hz"),
        frames: field(&line, "frames"),
        mean_fps: field(&line, "mean_fps"),
        missed: field(&line, "missed_frames"),
        rss_anon_kib: field(&line, "rss_anon_kib"),
        line,
    }
}

#[test]
#[ignore = "opens a window and needs corpus/; run with --ignored"]
fn scrolls_at_display_rate_with_constant_memory() {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_none() {
        panic!("needs a graphical session");
    }
    let small = bench("rows_100mb.csv"); // 1.9M rows
    let large = bench("rows_1024mb.csv"); // 19.1M rows
    assert!(large.rows >= 2_000_000, "{}", large.line);

    // Constant memory: 10x the rows may only add the sparse index (8 bytes per 64 rows,
    // about 2.2 MiB here) plus allocator noise.
    let growth_mib = (large.rss_anon_kib - small.rss_anon_kib) / 1024.0;
    eprintln!(
        "anonymous memory: {:.1} MiB at {} rows, {:.1} MiB at {} rows (+{growth_mib:.1} MiB)",
        small.rss_anon_kib / 1024.0,
        small.rows,
        large.rss_anon_kib / 1024.0,
        large.rows
    );
    assert!(
        growth_mib < 16.0,
        "memory grew {growth_mib:.1} MiB from {} to {} rows",
        small.rows,
        large.rows
    );

    if cfg!(debug_assertions) {
        eprintln!("debug build: frame rate not checked; use --release");
        return;
    }
    for run in [&small, &large] {
        // 60 fps on a 60 Hz display means keeping up with every refresh: mean within 2% of
        // the refresh rate and at most 1% of frames late by more than half a frame.
        assert!(run.mean_fps >= run.refresh_hz * 0.98, "{}", run.line);
        assert!(run.missed <= run.frames * 0.01, "{}", run.line);
    }
}
