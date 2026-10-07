//! BENCH-1: `cargo bench` measures the docs/BENCHMARKS.md rows on the corpus and prints that
//! table with the Latest column filled in. Rows and targets come from the doc itself, so the
//! two cannot drift apart; a row without a measurement prints why.
//!
//! Needs `python3 scripts/gen_corpus.py` (1 GB file plus the nasty set). Rows that open a
//! window (cold start, first rows, scroll, RAM) need a graphical session and print
//! "skipped: no display" without one, e.g. in CI. Keep the windows visible while it runs.
//! Numbers count toward gates only on the reference box named in the doc.

use csv_engine::{Dialect, RowIndex, Source, SparseRowIndex};
use data_model::{CellRef, CsvTable, Edit};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

const APP: &str = env!("CARGO_BIN_EXE_spreadsheet");
const RUNS: usize = 3;

struct Measured {
    latest: String,
    pass: Option<bool>,
}

fn ok(latest: String, pass: bool) -> Measured {
    Measured {
        latest,
        pass: Some(pass),
    }
}

fn note(latest: impl Into<String>) -> Measured {
    Measured {
        latest: latest.into(),
        pass: None,
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Drop `path` from the page cache so the next read comes from disk.
fn evict(path: &Path) {
    let f = std::fs::File::open(path).unwrap();
    // SAFETY: valid fd; advisory call with no memory effects.
    #[allow(unsafe_code)]
    unsafe {
        libc::posix_fadvise(f.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED);
    }
}

fn has_display() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some()
}

fn median(mut xs: Vec<f64>) -> f64 {
    xs.sort_by(f64::total_cmp);
    xs[xs.len() / 2]
}

/// Run the app; return each stdout line with the ms since spawn. Gives up after `limit`.
fn run_app(args: &[&std::ffi::OsStr], limit: Duration) -> Result<Vec<(f64, String)>, String> {
    let t0 = Instant::now();
    let mut child = Command::new(APP)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = tx.send((t0.elapsed().as_secs_f64() * 1e3, line));
        }
    });
    let mut lines = Vec::new();
    loop {
        match rx.recv_timeout(limit.saturating_sub(t0.elapsed())) {
            Ok(l) => lines.push(l),
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("timed out after {limit:?} (window hidden?)"));
            }
        }
    }
    child.wait().map_err(|e| e.to_string())?;
    Ok(lines)
}

fn json_field(json: &str, key: &str) -> Option<f64> {
    let start = json.find(&format!("\"{key}\":"))? + key.len() + 3;
    let rest = &json[start..];
    rest[..rest.find([',', '}'])?].parse().ok()
}

fn first_frame_ms(file: &Path, cold: bool) -> Result<(f64, String), String> {
    let mut times = Vec::new();
    let mut opened = String::new();
    for _ in 0..RUNS {
        if cold {
            evict(file);
        }
        let lines = run_app(
            &[file.as_os_str(), "--bench-open".as_ref()],
            Duration::from_secs(60),
        )?;
        let first = lines
            .iter()
            .find(|(_, l)| l.starts_with("FIRST_FRAME"))
            .ok_or("no FIRST_FRAME")?;
        times.push(first.0);
        opened = lines
            .iter()
            .find_map(|(_, l)| l.strip_prefix("OPENED "))
            .unwrap_or_default()
            .to_owned();
    }
    Ok((median(times), opened))
}

fn measure_all(corpus: &Path) -> HashMap<&'static str, Measured> {
    let mut m = HashMap::new();
    let big = corpus.join("rows_1024mb.csv");
    let size = std::fs::metadata(&big).unwrap().len() as f64;
    let display = has_display();

    // Full index complete: cold page cache, one thread.
    let mut times = Vec::new();
    let mut rows = 0;
    for _ in 0..RUNS {
        evict(&big);
        let t = Instant::now();
        let index = SparseRowIndex::new(Arc::new(Source::open(&big).unwrap()), &Dialect::default());
        index.build(&AtomicBool::new(false)).unwrap();
        times.push(t.elapsed().as_secs_f64());
        rows = index.row_count();
    }
    let s = median(times);
    m.insert(
        "Full index complete",
        ok(
            format!("{s:.2} s cold, {rows} rows, single thread"),
            s < 10.0,
        ),
    );

    // Save with 1,000 edits: on a copy next to the corpus (same disk, real fsync).
    let copy = corpus.join(format!(".bench-save-{}.csv", std::process::id()));
    std::fs::copy(&big, &copy).unwrap();
    let index = SparseRowIndex::new(Arc::new(Source::open(&copy).unwrap()), &Dialect::default());
    index.build(&AtomicBool::new(false)).unwrap();
    let mut table = CsvTable::new(index, Dialect::default());
    let mut x = 1u64;
    for k in 0..1_000 {
        x = x
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let at = CellRef {
            row: table.row_id(1 + (x >> 33) % (rows - 1)),
            col: ((x >> 13) % 8) as u32,
        };
        table.apply(Edit::set(at, format!("edit {k}")));
    }
    let t = Instant::now();
    file_format::save_csv(&copy, &table.save_job().unwrap()).unwrap();
    let s = t.elapsed().as_secs_f64();
    drop(table);
    std::fs::remove_file(&copy).unwrap();
    m.insert(
        "Save with 1,000 edits",
        ok(format!("{s:.2} s incl. fsync and verify"), s < 15.0),
    );

    for name in [
        "Find first match",
        "Sort numeric column",
        "Two-column filter",
    ] {
        let story = match name {
            "Find first match" => "SRCH-1",
            "Sort numeric column" => "SORT-1",
            _ => "FILT-2",
        };
        m.insert(name, note(format!("not built yet ({story})")));
    }

    let gui = [
        "Cold start to empty window",
        "Open to first rows visible",
        "Scroll frame time p99",
        "Peak RAM after open",
    ];
    if !display {
        for name in gui {
            m.insert(name, note("skipped: no display"));
        }
        return m;
    }

    match first_frame_ms(&corpus.join("nasty/empty.csv"), false) {
        Ok((ms, _)) => m.insert(
            gui[0],
            ok(format!("{ms:.0} ms (median of {RUNS})"), ms < 300.0),
        ),
        Err(e) => m.insert(gui[0], note(e)),
    };
    match first_frame_ms(&big, true) {
        Ok((ms, opened)) => {
            m.insert(
                gui[1],
                ok(
                    format!("{ms:.0} ms, cold cache (median of {RUNS})"),
                    ms < 500.0,
                ),
            );
            let field = |k| json_field(&opened, k).unwrap_or(f64::NAN) * 1024.0;
            let (hwm, anon) = (field("vm_hwm_kib"), field("rss_anon_kib"));
            m.insert(
                gui[3],
                ok(
                    format!(
                        "{:.2}x ({:.0} MiB peak RSS incl. mapped file pages; {:.0} MiB anonymous)",
                        hwm / size,
                        hwm / 1048576.0,
                        anon / 1048576.0
                    ),
                    hwm < 1.5 * size,
                ),
            );
        }
        Err(e) => {
            m.insert(gui[1], note(e.clone()));
            m.insert(gui[3], note(e));
        }
    }
    match run_app(
        &[big.as_os_str(), "--bench-scroll".as_ref(), "600".as_ref()],
        Duration::from_secs(120),
    ) {
        Ok(lines) => {
            match lines.iter().find(|(_, l)| l.starts_with('{')) {
                Some((_, j)) => {
                    let cpu = j
                        .split("\"frame_cpu_ms\":")
                        .nth(1)
                        .and_then(|r| json_field(r, "p99"))
                        .unwrap_or(f64::NAN);
                    let (fps, late) = (
                        json_field(j, "mean_fps").unwrap_or(0.0),
                        json_field(j, "missed_frames").unwrap_or(0.0),
                    );
                    // `"window":[w,h]` from the app's report.
                    let window = j
                        .split("\"window\":[")
                        .nth(1)
                        .and_then(|r| r.split(']').next())
                        .unwrap_or("?,?");
                    m.insert(
                        gui[2],
                        ok(
                            format!(
                                "{cpu:.1} ms frame CPU p99; {fps:.1} fps, {late:.0} of 600 frames late; {} px grid, 19.1M rows",
                                window.replace(',', "×")
                            ),
                            cpu < 16.0,
                        ),
                    );
                }
                None => {
                    m.insert(gui[2], note("no report from the app"));
                }
            }
        }
        Err(e) => {
            m.insert(gui[2], note(e));
        }
    }
    m
}

fn machine() -> String {
    let cpu = std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("model name"))?
                .split(':')
                .nth(1)
                .map(|s| s.trim().to_owned())
        })
        .unwrap_or_else(|| "unknown CPU".into());
    let threads = std::thread::available_parallelism().map_or(0, |n| n.get());
    let mem_gib = std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .next()?
                .split_whitespace()
                .nth(1)?
                .parse::<f64>()
                .ok()
        })
        .map_or(0.0, |kib| kib / 1048576.0);
    format!("{cpu}, {threads} threads, {mem_gib:.0} GiB RAM")
}

fn main() {
    // `cargo bench` passes `--bench`; filters are not supported.
    let corpus =
        std::env::var_os("BENCH_CORPUS_DIR").map_or_else(|| root().join("corpus"), PathBuf::from);
    if !corpus.join("rows_1024mb.csv").exists() || !corpus.join("nasty/empty.csv").exists() {
        eprintln!(
            "missing {}: run python3 scripts/gen_corpus.py",
            corpus.display()
        );
        std::process::exit(1);
    }
    let doc = std::fs::read_to_string(root().join("docs/BENCHMARKS.md"))
        .expect("read docs/BENCHMARKS.md");
    let measured = measure_all(&corpus);

    println!(
        "\nMachine: {} ({})",
        machine(),
        if has_display() {
            "with display"
        } else {
            "no display"
        }
    );
    println!(
        "Corpus: {}\n",
        corpus.canonicalize().unwrap_or(corpus.clone()).display()
    );
    let mut in_table = false;
    for line in doc.lines() {
        if !line.starts_with('|') {
            if in_table {
                break;
            }
            continue;
        }
        let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        if !in_table || cells[0].starts_with("---") {
            in_table = true;
            println!("{line}");
            continue;
        }
        let (latest, verdict) = match measured.get(cells[0]) {
            Some(Measured {
                latest,
                pass: Some(true),
            }) => (latest.clone(), " ✓"),
            Some(Measured {
                latest,
                pass: Some(false),
            }) => (latest.clone(), " ✗ MISSES TARGET"),
            Some(Measured { latest, pass: None }) => (latest.clone(), ""),
            None => ("no measurement for this row yet".into(), ""),
        };
        let libre = cells.get(3).copied().unwrap_or("");
        println!(
            "| {} | {} | {latest}{verdict} | {libre} |",
            cells[0], cells[1]
        );
    }
}
