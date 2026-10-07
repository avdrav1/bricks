//! Application shell: windows, menus, dialogs, OS integration. Toolkit: GTK4 (ADR 0001).
//!
//! Usage: `spreadsheet FILE.csv [--bench-scroll FRAMES | --bench-jump ROW [--jumps N]]`
//!
//! Benchmark modes print JSON and exit; the acceptance tests run them.
//! - `--bench-scroll` (GRID-1) waits for indexing, jumps to the middle of the file, scrolls
//!   37 px per frame (a fast flick) for FRAMES frames, and reports frame times and memory.
//! - `--bench-jump` (GRID-2) moves the vertical adjustment the way a scrollbar drag does:
//!   first to ROW, then to N-1 random rows, and reports how long each took to paint.

mod grid_view;

use csv_engine::{Dialect, Source, SparseRowIndex};
use data_model::CsvTable;
use grid_view::{GridView, ROW_H};
use gtk::{glib, prelude::*};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

const APP_ID: &str = "dev.bricks.Spreadsheet";

enum Bench {
    Scroll { frames: u32 },
    Jump { row: u64, jumps: u32 },
}

struct Args {
    file: PathBuf,
    bench: Option<Bench>,
}

fn parse_args() -> Result<Args, String> {
    let mut file = None;
    let mut bench = None;
    let mut jumps = 20;
    let mut it = std::env::args().skip(1);
    let number = |flag: &str, it: &mut dyn Iterator<Item = String>| -> Result<u64, String> {
        let v = it.next().ok_or(format!("{flag} needs a number"))?;
        v.parse()
            .map_err(|_| format!("bad number for {flag}: {v:?}"))
    };
    while let Some(a) = it.next() {
        match a.as_str() {
            "--bench-scroll" => {
                bench = Some(Bench::Scroll {
                    frames: number(&a, &mut it)? as u32,
                })
            }
            "--bench-jump" => {
                bench = Some(Bench::Jump {
                    row: number(&a, &mut it)?,
                    jumps: 0,
                })
            }
            "--jumps" => jumps = number(&a, &mut it)?.max(1) as u32,
            _ if file.is_none() && !a.starts_with("--") => file = Some(PathBuf::from(a)),
            _ => return Err(format!("unexpected argument {a:?}")),
        }
    }
    if let Some(Bench::Jump { jumps: j, .. }) = &mut bench {
        *j = jumps;
    }
    Ok(Args {
        file: file.ok_or("no file given")?,
        bench,
    })
}

fn main() -> glib::ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("spreadsheet: {e}\nusage: spreadsheet FILE.csv [--bench-scroll FRAMES | --bench-jump ROW [--jumps N]]");
            return glib::ExitCode::from(2);
        }
    };
    let source = match Source::open(&args.file) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            eprintln!("spreadsheet: cannot open {}: {e}", args.file.display());
            return glib::ExitCode::FAILURE;
        }
    };
    // Comma until delimiter detection (ENG-4).
    let dialect = Dialect::default();
    let index = SparseRowIndex::new(source, &dialect);
    // Index off the UI thread; rows become readable as it goes. Moves onto the shared job
    // pool when ENG-8 builds it (ADR 0005).
    std::thread::Builder::new()
        .name("index".into())
        .spawn({
            let index = index.clone();
            move || index.build(&AtomicBool::new(false))
        })
        .expect("spawn index thread");

    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let args = Rc::new(args);
    app.connect_activate(move |app| {
        build_window(app, &args, CsvTable::new(index.clone(), dialect))
    });
    app.run_with_args::<&str>(&[])
}

fn build_window(app: &gtk::Application, args: &Args, table: CsvTable) {
    let vadj = gtk::Adjustment::new(0.0, 0.0, 0.0, ROW_H, 0.0, 0.0);
    let hadj = gtk::Adjustment::new(0.0, 0.0, 0.0, 30.0, 0.0, 0.0);
    let grid = GridView::new(table, &vadj, &hadj);

    let layout = gtk::Grid::new();
    layout.attach(&grid, 0, 0, 1, 1);
    layout.attach(
        &gtk::Scrollbar::new(gtk::Orientation::Vertical, Some(&vadj)),
        1,
        0,
        1,
        1,
    );
    layout.attach(
        &gtk::Scrollbar::new(gtk::Orientation::Horizontal, Some(&hadj)),
        0,
        1,
        1,
        1,
    );

    let name = args.file.file_name().map_or_else(
        || args.file.display().to_string(),
        |n| n.to_string_lossy().into(),
    );
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title(&name)
        .default_width(1400)
        .default_height(900)
        .child(&layout)
        .build();

    // Mouse wheel and touchpad.
    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
    scroll.connect_scroll({
        let (vadj, hadj) = (vadj.clone(), hadj.clone());
        move |c, dx, dy| {
            let step = if c.unit() == gtk::gdk::ScrollUnit::Wheel {
                ROW_H * 3.0
            } else {
                1.0
            };
            vadj.set_value(vadj.value() + dy * step);
            hadj.set_value(hadj.value() + dx * step);
            glib::Propagation::Stop
        }
    });
    grid.add_controller(scroll);

    // Paging keys until full keyboard navigation (GRID-3).
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed({
        let vadj = vadj.clone();
        move |_, key, _, _| {
            use gtk::gdk::Key;
            let (v, page) = (vadj.value(), vadj.page_size());
            let target = match key {
                Key::Page_Down => v + page,
                Key::Page_Up => v - page,
                Key::Down => v + ROW_H,
                Key::Up => v - ROW_H,
                Key::Home => 0.0,
                Key::End => vadj.upper(),
                _ => return glib::Propagation::Proceed,
            };
            vadj.set_value(target);
            glib::Propagation::Stop
        }
    });
    window.add_controller(keys);

    // Follow the indexer: grow the scroll range and show progress in the title.
    let started = Instant::now();
    glib::timeout_add_local(Duration::from_millis(100), {
        let (grid, window) = (grid.downgrade(), window.downgrade());
        move || {
            let (Some(grid), Some(window)) = (grid.upgrade(), window.upgrade()) else {
                return glib::ControlFlow::Break;
            };
            grid.update_adjustments();
            let rows = grid.row_count();
            if grid.file_changed() {
                window.set_title(Some(&format!("{name} — file changed on disk")));
                return glib::ControlFlow::Break;
            }
            if grid.is_complete() {
                window.set_title(Some(&format!("{name} — {} rows", group_digits(rows))));
                if std::env::var_os("BRICKS_TIMINGS").is_some() {
                    eprintln!("indexed {rows} rows in {:?}", started.elapsed());
                }
                return glib::ControlFlow::Break;
            }
            window.set_title(Some(&format!(
                "{name} — {} rows (indexing…)",
                group_digits(rows)
            )));
            glib::ControlFlow::Continue
        }
    });

    window.present();
    grid.grab_focus();
    match args.bench {
        Some(Bench::Scroll { frames }) => bench_scroll(app, &grid, &vadj, frames),
        Some(Bench::Jump { row, jumps }) => bench_jump(app, &grid, &vadj, row, jumps),
        None => {}
    }
}

fn group_digits(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Scroll autopilot for the GRID-1 acceptance test. Measures the interval between finished
/// frames (frame clock after-paint) and the frame's CPU time (before- to after-paint).
fn bench_scroll(app: &gtk::Application, grid: &GridView, vadj: &gtk::Adjustment, frames: u32) {
    let Some(clock) = grid.frame_clock() else {
        return;
    };
    #[derive(Default)]
    struct Rec {
        started: bool,
        paint_start: Option<Instant>,
        ends: Vec<Instant>,
        cpu_ms: Vec<f64>,
        peak_cache: usize,
    }
    let rec = Rc::new(RefCell::new(Rec::default()));
    clock.connect_before_paint({
        let rec = rec.clone();
        move |_| rec.borrow_mut().paint_start = Some(Instant::now())
    });
    clock.connect_after_paint({
        let (rec, grid) = (rec.clone(), grid.downgrade());
        move |_| {
            let mut r = rec.borrow_mut();
            if let (true, Some(t0), Some(g)) = (r.started, r.paint_start, grid.upgrade()) {
                r.ends.push(Instant::now());
                r.cpu_ms.push(t0.elapsed().as_secs_f64() * 1e3);
                r.peak_cache = r.peak_cache.max(g.cache_bytes());
            }
        }
    });

    let ticks = Cell::new(0u32);
    let app = app.clone();
    let vadj = vadj.clone();
    grid.add_tick_callback(move |grid, _| {
        if !grid.is_complete() {
            return glib::ControlFlow::Continue;
        }
        let n = ticks.get();
        ticks.set(n + 1);
        if n == 0 {
            // Start mid-file, then let one second of frames settle before measuring.
            grid.update_adjustments();
            vadj.set_value(vadj.upper() / 2.0);
        } else if n == 60 {
            rec.borrow_mut().started = true;
        } else if n > 60 + frames {
            print_report(
                &rec.borrow().ends,
                &rec.borrow().cpu_ms,
                rec.borrow().peak_cache,
                grid,
                &vadj,
            );
            app.quit();
            return glib::ControlFlow::Break;
        }
        vadj.set_value(vadj.value() + 37.0);
        glib::ControlFlow::Continue
    });
}

/// Scrollbar-jump driver for the GRID-2 acceptance test. Each jump sets the vertical
/// adjustment, which is what dragging the scrollbar does, then waits for the first finished
/// frame (frame clock after-paint) whose top row is the target. Jumps are 300 ms apart so
/// each starts from an idle widget; the first target is `first`, the rest are random.
fn bench_jump(
    app: &gtk::Application,
    grid: &GridView,
    vadj: &gtk::Adjustment,
    first: u64,
    jumps: u32,
) {
    let Some(clock) = grid.frame_clock() else {
        return;
    };
    #[derive(Default)]
    struct Pending {
        target: u64,
        since: Option<Instant>,
        done_ms: Vec<f64>,
    }
    let pending = Rc::new(RefCell::new(Pending::default()));
    clock.connect_after_paint({
        let (pending, grid) = (pending.clone(), grid.downgrade());
        move |_| {
            let Some(grid) = grid.upgrade() else { return };
            let mut p = pending.borrow_mut();
            if let Some(t0) = p.since {
                if grid.painted_top_row() == Some(p.target) {
                    p.done_ms.push(t0.elapsed().as_secs_f64() * 1e3);
                    p.since = None;
                }
            }
        }
    });

    let (app, grid, vadj) = (app.clone(), grid.clone(), vadj.clone());
    let sent = Cell::new(0u32);
    let mut seed = 0x6A1D_0002_u64;
    glib::timeout_add_local(Duration::from_millis(300), move || {
        if !grid.is_complete() {
            return glib::ControlFlow::Continue;
        }
        let rows = grid.row_count();
        let n = sent.get();
        if n == jumps {
            let p = pending.borrow();
            let mut ms = p.done_ms.clone();
            ms.sort_by(f64::total_cmp);
            let pct = |q: f64| {
                ms.get(((q / 100.0) * (ms.len().max(1) - 1) as f64).round() as usize)
                    .copied()
                    .unwrap_or(f64::NAN)
            };
            println!(
                r#"{{"rows":{rows},"window":[{},{}],"jumps":{jumps},"landed":{},"first_row":{first},"first_jump_ms":{:.2},"p50":{:.2},"p99":{:.2},"max":{:.2}}}"#,
                grid.width(),
                grid.height(),
                p.done_ms.len(),
                p.done_ms.first().copied().unwrap_or(f64::NAN),
                pct(50.0),
                pct(99.0),
                ms.last().copied().unwrap_or(f64::NAN),
            );
            app.quit();
            return glib::ControlFlow::Break;
        }
        if pending.borrow().since.is_some() {
            return glib::ControlFlow::Continue; // previous jump has not painted yet
        }
        // Keep targets where the target row can sit at the top of the view.
        let last_top = rows.saturating_sub((vadj.page_size() / ROW_H).ceil() as u64);
        let target = if n == 0 {
            first.min(last_top)
        } else {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) % last_top.max(1)
        };
        sent.set(n + 1);
        let mut p = pending.borrow_mut();
        p.target = target;
        p.since = Some(Instant::now());
        drop(p);
        vadj.set_value(target as f64 * ROW_H);
        glib::ControlFlow::Continue
    });
}

fn print_report(
    ends: &[Instant],
    cpu_ms: &[f64],
    peak_cache: usize,
    grid: &GridView,
    vadj: &gtk::Adjustment,
) {
    let mut intervals: Vec<f64> = ends
        .windows(2)
        .map(|w| (w[1] - w[0]).as_secs_f64() * 1e3)
        .collect();
    let mut cpu = cpu_ms.to_vec();
    let refresh = grid
        .native()
        .and_then(|n| n.surface())
        .and_then(|s| s.display().monitor_at_surface(&s))
        .map_or(60.0, |m| m.refresh_rate() as f64 / 1000.0);
    let budget = 1000.0 / refresh;
    let missed = intervals.iter().filter(|&&i| i > budget * 1.5).count();
    let mean_fps = if intervals.is_empty() {
        0.0
    } else {
        1000.0 * intervals.len() as f64 / intervals.iter().sum::<f64>()
    };
    let summary = |xs: &mut Vec<f64>| {
        xs.sort_by(f64::total_cmp);
        let p = |q: f64| {
            xs.get(((q / 100.0) * (xs.len().max(1) - 1) as f64).round() as usize)
                .copied()
                .unwrap_or(0.0)
        };
        format!(
            r#"{{"p50":{:.2},"p99":{:.2},"max":{:.2}}}"#,
            p(50.0),
            p(99.0),
            xs.last().copied().unwrap_or(0.0)
        )
    };
    println!(
        r#"{{"rows":{},"window":[{},{}],"refresh_hz":{refresh:.1},"frames":{},"mean_fps":{mean_fps:.1},"missed_frames":{missed},"interval_ms":{},"frame_cpu_ms":{},"grid_cache_peak_bytes":{peak_cache},"rss_anon_kib":{},"end_row":{}}}"#,
        grid.row_count(),
        grid.width(),
        grid.height(),
        intervals.len(),
        summary(&mut intervals),
        summary(&mut cpu),
        rss_anon_kib(),
        (vadj.value() / ROW_H) as u64,
    );
}

fn rss_anon_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("RssAnon:"))?
                .split_whitespace()
                .nth(1)?
                .parse()
                .ok()
        })
        .unwrap_or(0)
}
