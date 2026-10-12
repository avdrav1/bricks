//! BENCH-1: `cargo bench` measures the apps/brick/docs/BENCHMARKS.md rows on the corpus and prints that
//! table with the Latest column filled in. Rows and targets come from the doc itself, so the
//! two cannot drift apart; a row without a measurement prints why.
//!
//! Needs `python3 scripts/gen_corpus.py` (1 GB file plus the nasty set). Rows that open a
//! window (cold start, first rows, scroll, RAM) need a graphical session and print
//! "skipped: no display" without one, e.g. in CI. Keep the windows visible while it runs.
//! Numbers count toward gates only on the reference box named in the doc.
//!
//! The XLSX and formula rows (BENCH-2) also need `corpus/formulas.csv` (gen_corpus.py) and
//! `corpus/rows_1m.xlsx` (`cargo run --release -p xlsx --example gen_corpus_xlsx`), and the
//! styled rows (BENCH-3) `corpus/styled_1m.xlsx` (the same with `-- --styled`); without
//! them those rows say what is missing.

use csv_engine::{Dialect, RowIndex, Source, SparseRowIndex};
use data_model::{
    CellRef, ColId, Compare, CsvTable, DelimiterChoice, Edit, Filter, Origin, Query, RowId,
    SaveProgress, SearchProgress, SheetId, SortOrder, Step, TableSource, Test, Workbook,
};
use formula::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

const APP: &str = env!("CARGO_BIN_EXE_brick");
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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
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

/// Find, sort, and filter on the 1 GB table, each as the app runs it on a snapshot on the
/// job pool (SRCH-1, SORT-1, FILT-2). Median of [`RUNS`]; page cache warm.
fn table_jobs(big: &Path) -> Vec<(&'static str, Measured)> {
    let no = AtomicBool::new(false);
    let mut t = CsvTable::open(big, DelimiterChoice::Auto).unwrap();
    t.index().build(&no).unwrap();
    let types = t.infer_types(&no).unwrap();
    t.set_types(types);
    let timed = |f: &mut dyn FnMut()| {
        median(
            (0..RUNS)
                .map(|_| {
                    let start = Instant::now();
                    f();
                    start.elapsed().as_secs_f64()
                })
                .collect(),
        )
    };

    // Find: the first match from the top, counting every match as the app does; the
    // slowest of a value near the end, one not there, and one in 2.7M cells.
    let mut find = Vec::new();
    for text in ["19094000", "Zanzibar", "portland"] {
        let q = Query {
            text: text.into(),
            match_case: false,
            col: None,
        };
        let s = timed(&mut || {
            let mut snap = t.snapshot();
            let m = snap.search(&q, &no, &SearchProgress::default()).unwrap();
            m.step(&mut snap, (0, 0), Step::Here, None, &no).unwrap();
        });
        find.push((text, s));
    }
    let worst = find.iter().map(|(_, s)| *s).fold(0.0, f64::max);
    let detail: Vec<String> = find
        .iter()
        .map(|(q, s)| format!("{q} {:.0} ms", s * 1e3))
        .collect();

    // Sort: the score column (decimals) of every row, and of the first 2M rows.
    let sort = |t: &CsvTable| {
        t.snapshot()
            .sort_rows(4, SortOrder::Ascending, &no, &AtomicU64::new(0))
            .unwrap();
    };
    let all = t.row_count();
    let full = timed(&mut || sort(&t));
    let mut small = t.snapshot();
    let cut = small.delete_rows(2_000_000, all - 2_000_000).unwrap();
    small.apply(cut);
    let two_m = timed(&mut || sort(&small));
    drop(small);

    // Two-column filter: city = Portland, then revenue > 50,000.
    let mut shown = 0;
    let filter = timed(&mut || {
        let mut s = t.snapshot();
        for (col, test) in [
            (2, Test::Equals("Portland".into())),
            (3, Test::Number(Compare::Gt, 50_000.0)),
        ] {
            let f = Filter {
                col: s.col_id(col),
                test,
            };
            let rows = s
                .snapshot()
                .filter_rows(&f, &no, &AtomicU64::new(0))
                .unwrap();
            s.set_filter(f, rows);
        }
        shown = s.row_count();
    });

    vec![
        (
            "Find first match",
            ok(
                format!("{:.0} ms slowest ({})", worst * 1e3, detail.join(", ")),
                worst < 1.0,
            ),
        ),
        (
            "Sort numeric column",
            ok(
                format!("{full:.2} s for {all} rows, {:.0} ms at 2M", two_m * 1e3),
                full < 10.0 && two_m < 3.0,
            ),
        ),
        (
            "Two-column filter",
            ok(
                format!("{filter:.2} s: Portland, then revenue > 50,000 ({shown} rows)"),
                filter < 3.0,
            ),
        ),
    ]
}

/// The frame CPU p99 row from a `--bench-scroll 600` run of `file` (`what` describes it).
fn scroll_row(file: &Path, what: &str) -> Measured {
    let lines = match run_app(
        &[file.as_os_str(), "--bench-scroll".as_ref(), "600".as_ref()],
        Duration::from_secs(120),
    ) {
        Ok(lines) => lines,
        Err(e) => return note(e),
    };
    let Some((_, j)) = lines.iter().find(|(_, l)| l.starts_with('{')) else {
        return note("no report from the app");
    };
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
    ok(
        format!(
            "{cpu:.1} ms frame CPU p99; {fps:.1} fps, {late:.0} of 600 frames late; {} px grid, {what}",
            window.replace(',', "×")
        ),
        cpu < 16.0,
    )
}

/// A table of `path`, indexed, with its formula cells found, as the app opens it.
fn indexed(path: &Path) -> CsvTable {
    let no = AtomicBool::new(false);
    let index = SparseRowIndex::new(Arc::new(Source::open(path).unwrap()), &Dialect::default());
    index.build(&no).unwrap();
    let mut t = CsvTable::new(index, Dialect::default());
    let found = t.scan_formulas(&no, &AtomicU64::new(0)).unwrap();
    t.set_formula_cells(found);
    t
}

fn sheet(name: &str, table: CsvTable) -> (String, CsvTable, Origin, data_model::CachedResults) {
    (name.into(), table, Origin::Csv, Default::default())
}

fn cell(b: &Workbook, sheet: SheetId, row: u64, col: u32) -> CellRef {
    let t = &b.sheet(sheet).unwrap().table;
    CellRef {
        row: t.row_id(row),
        col: t.col_id(col),
    }
}

/// Compute `cells` on a fresh snapshot of `b`, as the app's calculation job does; seconds.
fn evaluate(b: &Workbook, cells: &[(SheetId, CellRef)]) -> (f64, Vec<Value>) {
    let mut snap = b.snapshot();
    let start = Instant::now();
    let values = snap
        .evaluate_all(cells, Arc::new(AtomicBool::new(false)), &AtomicU64::new(0))
        .unwrap();
    let s = start.elapsed().as_secs_f64();
    (s, values.into_iter().map(|(_, v)| v).collect())
}

/// The formula rows (BENCH-2), each as the app's calculation job computes them.
fn formula_jobs(corpus: &Path) -> Vec<(&'static str, Measured)> {
    let mut out = Vec::new();

    // `=SUM(rows!D:D)` on a second sheet, over the 1 GB file's revenue column.
    let mut sum = CsvTable::untitled();
    let edit = sum.insert_rows(0, 1).unwrap();
    sum.apply(edit);
    let at = CellRef {
        row: sum.row_id(0),
        col: sum.col_id(0),
    };
    sum.apply(Edit::set(at, "=SUM(rows!D:D)"));
    let rows = indexed(&corpus.join("rows_1024mb.csv"));
    let n = rows.total_rows() - 1;
    let b = Workbook::from_sheets(vec![sheet("rows", rows), sheet("Sheet2", sum)], Vec::new());
    let target = b.sheets()[1].id;
    let s = median((0..RUNS).map(|_| evaluate(&b, &[(target, at)]).0).collect());
    out.push((
        "Recalculate SUM over a 1 GB column",
        ok(format!("{s:.2} s: =SUM(rows!D:D), {n} rows"), s < 5.0),
    ));
    drop(b);

    // The running total `=E{n-1}+D{n}` of formulas.csv, at its last row: every row's
    // `=B*C` and every earlier total, on a fresh calculation.
    let path = corpus.join("formulas.csv");
    if path.exists() {
        let t = indexed(&path);
        let last = t.total_rows() - 1;
        let b = Workbook::from_sheets(vec![sheet("formulas", t)], Vec::new());
        let id = b.sheets()[0].id;
        let end = [(id, cell(&b, id, last, 4))];
        let runs: Vec<(f64, Vec<Value>)> = (0..RUNS).map(|_| evaluate(&b, &end)).collect();
        let total = runs[0].1[0].clone();
        assert!(
            matches!(total, Value::Number(_)),
            "running total: {total:?}"
        );
        let s = median(runs.into_iter().map(|r| r.0).collect());
        out.push((
            "Running total, 1,000,000 rows",
            ok(
                format!(
                    "{s:.2} s: formulas.csv E{} (`=E{last}+D{}`)",
                    last + 1,
                    last + 1
                ),
                s < 5.0,
            ),
        ));
    } else {
        out.push((
            "Running total, 1,000,000 rows",
            note("missing corpus/formulas.csv: run python3 scripts/gen_corpus.py"),
        ));
    }

    // 10,000 `=VLOOKUP(k,data!A:B,2,FALSE)` into 1,000,000 shuffled keys, the first one
    // building the lookup index, as FML-7's lookup_speed test does.
    const ROWS: u64 = 1_000_000;
    const LOOKUPS: u64 = 10_000;
    let key = |n: u64| n * 7_919 % ROWS + 1;
    let tmp = std::env::temp_dir().join(format!("bench-vlookup-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let mut data = String::with_capacity(20 * ROWS as usize);
    for n in 0..ROWS {
        let k = key(n);
        data.push_str(&format!("{k},v{k}\n"));
    }
    std::fs::write(tmp.join("data.csv"), data).unwrap();
    let wanted: Vec<u64> = (0..LOOKUPS).map(|i| key(i * 97 % ROWS)).collect();
    let formulas: String = wanted
        .iter()
        .map(|k| format!("\"=VLOOKUP({k},data!A:B,2,FALSE)\"\n"))
        .collect();
    std::fs::write(tmp.join("lookups.csv"), formulas).unwrap();
    let b = Workbook::from_sheets(
        vec![
            sheet("data", indexed(&tmp.join("data.csv"))),
            sheet("lookups", indexed(&tmp.join("lookups.csv"))),
        ],
        Vec::new(),
    );
    std::fs::remove_dir_all(&tmp).unwrap();
    let id = b.sheets()[1].id;
    let cells: Vec<(SheetId, CellRef)> = (0..LOOKUPS).map(|r| (id, cell(&b, id, r, 0))).collect();
    let runs: Vec<(f64, Vec<Value>)> = (0..RUNS).map(|_| evaluate(&b, &cells)).collect();
    for (v, k) in runs[0].1.iter().zip(&wanted) {
        assert_eq!(*v, Value::text(format!("v{k}")));
    }
    let s = median(runs.into_iter().map(|r| r.0).collect());
    out.push((
        "10,000 exact VLOOKUPs into 1,000,000 rows",
        ok(format!("{s:.2} s incl. building the lookup index"), s < 2.0),
    ));
    out
}

/// Saving a 1,048,576-row workbook as XLSX: `file` imported, one cell edited, then saved
/// next to the corpus as the app's save job does: the package patched, the sheet written
/// again with every unchanged cell copied (ADR 0010), atomic with fsync.
fn xlsx_save(file: &Path, corpus: &Path) -> Measured {
    let no = AtomicBool::new(false);
    let imported = match xlsx::import(file, &no, &xlsx::Progress::default()) {
        Ok(i) => i,
        Err(e) => return note(format!("cannot import {}: {e}", file.display())),
    };
    let package = imported.package.clone();
    let styles = imported.styles.clone();
    let sheets = imported
        .sheets
        .into_iter()
        .map(|s| (s.name, s.table, Origin::Xlsx(s.index), s.cached))
        .collect();
    let mut book = Workbook::from_sheets(sheets, imported.names);
    book.set_styles(styles);
    let rows = book.active().total_rows();
    let at = CellRef {
        row: RowId::source(rows / 2),
        col: ColId::source(1),
    };
    book.active_mut().apply(Edit::set(at, "edited"));
    let dest = corpus.join(format!(".bench-save-{}.xlsx", std::process::id()));
    let start = Instant::now();
    let saved = xlsx::SaveBook::from_workbook(
        book,
        Some(package),
        &HashMap::new(),
        Arc::new(AtomicBool::new(false)),
        &AtomicU64::new(0),
    )
    .expect("not cancelled");
    let stats = xlsx::save(&dest, saved, &SaveProgress::default(), &no).unwrap();
    let secs = start.elapsed().as_secs_f64();
    std::fs::remove_file(&dest).unwrap();
    ok(
        format!(
            "{secs:.2} s: {rows} rows, one cell edited, {} MiB, incl. fsync",
            stats.bytes >> 20
        ),
        secs < 30.0,
    )
}

/// `brick FILE.xlsx --bench-open`: from spawn until every sheet is imported and drawn.
fn xlsx_open(file: &Path) -> Result<Measured, String> {
    let mut times = Vec::new();
    let mut opened = String::new();
    for _ in 0..RUNS {
        let lines = run_app(
            &[file.as_os_str(), "--bench-open".as_ref()],
            Duration::from_secs(60),
        )?;
        let (ms, json) = lines
            .iter()
            .find_map(|(ms, l)| Some((*ms, l.strip_prefix("OPENED ")?)))
            .ok_or("no OPENED")?;
        times.push(ms / 1e3);
        opened = json.to_owned();
    }
    let s = median(times);
    let field = |k| json_field(&opened, k).unwrap_or(f64::NAN);
    Ok(ok(
        format!(
            "{s:.2} s (median of {RUNS}, warm cache): {} rows, {} sheet(s), {:.0} MiB peak RSS",
            field("rows"),
            field("sheets"),
            field("vm_hwm_kib") / 1024.0
        ),
        s < 10.0,
    ))
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
            col: ColId::source(((x >> 13) % 8) as u32),
        };
        table.apply(Edit::set(at, format!("edit {k}")));
    }
    let t = Instant::now();
    file_format::save_csv(
        &copy,
        &table.save_job().unwrap(),
        &data_model::SaveProgress::default(),
        &AtomicBool::new(false),
    )
    .unwrap();
    let s = t.elapsed().as_secs_f64();
    drop(table);
    std::fs::remove_file(&copy).unwrap();
    m.insert(
        "Save with 1,000 edits",
        ok(format!("{s:.2} s incl. fsync and verify"), s < 15.0),
    );

    for (name, measured) in table_jobs(&big) {
        m.insert(name, measured);
    }
    for (name, measured) in formula_jobs(corpus) {
        m.insert(name, measured);
    }
    let xlsx_file = corpus.join("rows_1m.xlsx");
    let no_xlsx =
        "missing corpus/rows_1m.xlsx: run cargo run --release -p xlsx --example gen_corpus_xlsx";
    m.insert(
        "Save 1,048,576-row workbook as XLSX",
        if xlsx_file.exists() {
            xlsx_save(&xlsx_file, corpus)
        } else {
            note(no_xlsx)
        },
    );
    // The styled corpus (BENCH-3): a bold header on a fill, number formats per column, an
    // italic coloured column, a bold total row with a fill and a border every 100 rows.
    let styled = corpus.join("styled_1m.xlsx");
    let no_styled = "missing corpus/styled_1m.xlsx: run cargo run --release -p xlsx --example gen_corpus_xlsx -- --styled";
    m.insert(
        "Save 1,048,576-row styled XLSX after one edit",
        if styled.exists() {
            xlsx_save(&styled, corpus)
        } else {
            note(no_styled)
        },
    );

    let gui = [
        "Cold start to empty window",
        "Open to first rows visible",
        "Scroll frame time p99",
        "Peak RAM after open",
        "Open 1,048,576-row XLSX, all sheets ready",
        "Scroll frame time p99, 60 visible formulas",
        "Open 1,048,576-row styled XLSX",
        "Scroll frame time p99, styled sheet",
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
    m.insert(gui[2], scroll_row(&big, "19.1M rows"));

    // The XLSX open: import, then the first frame of the workbook (XLSX-1); warm cache.
    m.insert(
        gui[4],
        if !xlsx_file.exists() {
            note(no_xlsx)
        } else {
            match xlsx_open(&xlsx_file) {
                Ok(measured) => measured,
                Err(e) => note(e),
            }
        },
    );
    let formulas = corpus.join("formulas.csv");
    m.insert(
        gui[5],
        if formulas.exists() {
            scroll_row(
                &formulas,
                "formulas.csv (1M rows of formulas, 4 columns of them)",
            )
        } else {
            note("missing corpus/formulas.csv: run python3 scripts/gen_corpus.py")
        },
    );
    if styled.exists() {
        m.insert(gui[6], xlsx_open(&styled).unwrap_or_else(note));
        m.insert(
            gui[7],
            scroll_row(
                &styled,
                "styled_1m.xlsx (fills, borders, number formats, a styled total row every 100)",
            ),
        );
    } else {
        m.insert(gui[6], note(no_styled));
        m.insert(gui[7], note(no_styled));
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
    let doc = std::fs::read_to_string(root().join("apps/brick/docs/BENCHMARKS.md"))
        .expect("read apps/brick/docs/BENCHMARKS.md");
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
