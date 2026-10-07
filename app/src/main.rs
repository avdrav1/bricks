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

use data_model::{CsvTable, DelimiterChoice, RereadError, SaveStats};
use grid_view::{GridView, ROW_H};
use gtk::{glib, prelude::*};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

const APP_ID: &str = "dev.bricks.Spreadsheet";

enum Bench {
    Scroll { frames: u32 },
    Jump { row: u64, jumps: u32 },
    Open,
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
            "--bench-open" => bench = Some(Bench::Open),
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
    let session = Rc::new(Session {
        path: args.file.clone(),
        choice: Cell::new(DelimiterChoice::Auto),
        indexing: RefCell::new(Arc::new(AtomicBool::new(false))),
    });
    let table = match session.open() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("spreadsheet: cannot open {}: {e}", args.file.display());
            return glib::ExitCode::FAILURE;
        }
    };

    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let args = Rc::new(args);
    let table = RefCell::new(Some(table));
    app.connect_activate(move |app| {
        if let Some(table) = table.take() {
            build_window(app, &args, &session, table);
        }
    });
    app.run_with_args::<&str>(&[])
}

/// The open file: where it is, how its delimiter is chosen, and the index build in flight.
struct Session {
    path: PathBuf,
    choice: Cell<DelimiterChoice>,
    /// Cancel flag of the running index build; set when a newer reading replaces it.
    indexing: RefCell<Arc<AtomicBool>>,
}

impl Session {
    /// Map the file with the current delimiter choice (detected on Auto, ENG-4) and start
    /// indexing it.
    fn open(&self) -> std::io::Result<CsvTable> {
        let table = CsvTable::open(&self.path, self.choice.get())?;
        self.index(&table)?;
        Ok(table)
    }

    /// Index `table` off the UI thread, cancelling any earlier build; rows become readable
    /// as it goes. Moves onto the shared job pool when ENG-8 builds it (ADR 0005).
    fn index(&self, table: &CsvTable) -> std::io::Result<()> {
        let cancel = Arc::new(AtomicBool::new(false));
        self.indexing
            .replace(cancel.clone())
            .store(true, Ordering::Relaxed);
        let index = table.index().clone();
        std::thread::Builder::new()
            .name("index".into())
            .spawn(move || index.build(&cancel))?;
        Ok(())
    }
}

/// Delimiter menu entries, in dropdown order.
const DELIMITERS: [(&str, DelimiterChoice); 5] = [
    ("Auto", DelimiterChoice::Auto),
    ("Comma", DelimiterChoice::Fixed(b',')),
    ("Tab", DelimiterChoice::Fixed(b'\t')),
    ("Semicolon", DelimiterChoice::Fixed(b';')),
    ("Pipe", DelimiterChoice::Fixed(b'|')),
];

/// Delimiter dropdown (ENG-5): re-reads the open file with the chosen delimiter and
/// re-indexes it in the background. Disabled while edits are unsaved, since edits are tied
/// to the columns the old delimiter produced.
fn delimiter_dropdown(session: &Rc<Session>, grid: &GridView) -> gtk::DropDown {
    let names: Vec<&str> = DELIMITERS.iter().map(|(n, _)| *n).collect();
    let dropdown = gtk::DropDown::from_strings(&names);
    dropdown.set_tooltip_text(Some("Delimiter"));
    dropdown.connect_selected_notify({
        let (session, grid) = (session.clone(), grid.downgrade());
        move |dd| {
            let (Some(grid), Some(&(_, choice))) =
                (grid.upgrade(), DELIMITERS.get(dd.selected() as usize))
            else {
                return;
            };
            if choice == session.choice.get() {
                return;
            }
            match grid.reread(choice) {
                Ok(table) => {
                    if let Err(e) = session.index(&table) {
                        eprintln!("spreadsheet: cannot index: {e}");
                        return;
                    }
                    session.choice.set(choice);
                    grid.replace_table(table);
                }
                Err(RereadError::UnsavedEdits(_)) => {
                    let current = DELIMITERS
                        .iter()
                        .position(|(_, c)| *c == session.choice.get())
                        .unwrap_or(0);
                    dd.set_selected(current as u32);
                }
            }
        }
    });
    dropdown
}

fn build_window(app: &gtk::Application, args: &Args, session: &Rc<Session>, table: CsvTable) {
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
    let delimiter = delimiter_dropdown(session, &grid);
    let header = gtk::HeaderBar::new();
    header.pack_end(&delimiter);
    window.set_titlebar(Some(&header));

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

    // Ctrl+S saves. Navigation keys belong to the grid (GRID-3).
    let save = Rc::new(RefCell::new(SaveState::Idle));
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed({
        let (grid, save, session) = (grid.downgrade(), save.clone(), session.clone());
        move |_, key, _, mods| {
            use gtk::gdk::{Key, ModifierType};
            if mods.contains(ModifierType::CONTROL_MASK) && matches!(key, Key::s | Key::S) {
                if let Some(grid) = grid.upgrade() {
                    start_save(&grid, &session.path, &save);
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        }
    });
    window.add_controller(keys);

    // Status: follow the indexer (grow the scroll range), finish saves, keep the title current.
    let started = Instant::now();
    let mut was_complete = false;
    glib::timeout_add_local(Duration::from_millis(100), {
        let (grid, window, session) = (grid.downgrade(), window.downgrade(), session.clone());
        move || {
            let (Some(grid), Some(window)) = (grid.upgrade(), window.upgrade()) else {
                return glib::ControlFlow::Break;
            };
            // While indexing (also after a save reopens the file), grow the scroll range.
            let complete = grid.is_complete();
            if !complete || !was_complete {
                grid.update_adjustments();
            }
            if complete && !was_complete && std::env::var_os("BRICKS_TIMINGS").is_some() {
                eprintln!(
                    "indexed {} rows ({:?} since start)",
                    grid.row_count(),
                    started.elapsed()
                );
            }
            was_complete = complete;
            finish_save(&grid, &session, &save);
            let can_switch =
                grid.edit_count() == 0 && !matches!(*save.borrow(), SaveState::Saving(_));
            delimiter.set_sensitive(can_switch);
            delimiter.set_tooltip_text(Some(if can_switch {
                "Delimiter"
            } else {
                "Delimiter (save your edits before changing it)"
            }));
            window.set_title(Some(&title(
                &name,
                &grid,
                session.choice.get(),
                &save.borrow(),
            )));
            glib::ControlFlow::Continue
        }
    });

    window.present();
    grid.grab_focus();
    match args.bench {
        Some(Bench::Scroll { frames }) => bench_scroll(app, &grid, &vadj, frames),
        Some(Bench::Jump { row, jumps }) => bench_jump(app, &grid, &vadj, row, jumps),
        Some(Bench::Open) => bench_open(app, &grid),
        None => {}
    }
}

/// Open benchmark (BENCH-1). Prints `FIRST_FRAME` at the end of the first frame that shows
/// rows (or, for a file with none, the first frame at all), then, once indexing is done,
/// `OPENED {json}` with memory use, and quits. The caller times from spawn to each line.
fn bench_open(app: &gtk::Application, grid: &GridView) {
    let Some(clock) = grid.frame_clock() else {
        return;
    };
    let (app, first) = (app.clone(), Cell::new(false));
    clock.connect_after_paint({
        let grid = grid.downgrade();
        move |_| {
            let Some(grid) = grid.upgrade() else { return };
            let empty = grid.is_complete() && grid.row_count() == 0;
            if !first.get() && (grid.painted_top_row().is_some() || empty) {
                first.set(true);
                println!("FIRST_FRAME");
            }
            if first.get() && grid.is_complete() {
                println!(
                    r#"OPENED {{"rows":{},"rss_anon_kib":{},"rss_file_kib":{},"vm_hwm_kib":{}}}"#,
                    grid.row_count(),
                    proc_status_kib("RssAnon:"),
                    proc_status_kib("RssFile:"),
                    proc_status_kib("VmHWM:"),
                );
                app.quit();
            }
        }
    });
    // Keep frames coming until indexing finishes.
    grid.add_tick_callback(|_, _| glib::ControlFlow::Continue);
}

enum SaveState {
    Idle,
    Saving(mpsc::Receiver<Result<(SaveStats, Duration), String>>),
    Saved(Duration),
    Failed(String),
}

/// Ctrl+S: write the table to its own file on a worker thread (save is atomic, SAVE-1).
/// Editing is paused until it finishes, so no edit can be lost between snapshot and reopen.
fn start_save(grid: &GridView, path: &Path, save: &RefCell<SaveState>) {
    if matches!(*save.borrow(), SaveState::Saving(_)) || grid.edit_count() == 0 {
        return;
    }
    let job = match grid.save_job() {
        Ok(job) => job,
        Err(_) => {
            *save.borrow_mut() = SaveState::Failed("wait for indexing to finish".into());
            return;
        }
    };
    grid.set_saving(true);
    let (tx, rx) = mpsc::channel();
    let path = path.to_owned();
    let spawned = std::thread::Builder::new()
        .name("save".into())
        .spawn(move || {
            let t = Instant::now();
            let r = file_format::save_csv(&path, &job).map_err(|e| e.to_string());
            let _ = tx.send(r.map(|s| (s, t.elapsed())));
        });
    *save.borrow_mut() = match spawned {
        Ok(_) => SaveState::Saving(rx),
        Err(e) => {
            grid.set_saving(false);
            SaveState::Failed(e.to_string())
        }
    };
}

/// When a save has finished: reopen the saved file (its edits are now on disk) or report
/// the error with the edits still in memory.
fn finish_save(grid: &GridView, session: &Session, save: &RefCell<SaveState>) {
    let result = match &*save.borrow() {
        SaveState::Saving(rx) => match rx.try_recv() {
            Ok(r) => r,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err("the save thread stopped".into()),
        },
        _ => return,
    };
    grid.set_saving(false);
    let next = match result
        .and_then(|done| session.open().map(|t| (done, t)).map_err(|e| e.to_string()))
    {
        Ok(((stats, took), table)) => {
            grid.replace_table(table);
            if std::env::var_os("BRICKS_TIMINGS").is_some() {
                eprintln!(
                    "saved {} rows, {} bytes in {took:?}",
                    stats.rows, stats.bytes
                );
            }
            SaveState::Saved(took)
        }
        Err(e) => {
            eprintln!("spreadsheet: save failed: {e}");
            SaveState::Failed(e)
        }
    };
    *save.borrow_mut() = next;
}

fn delimiter_name(d: u8) -> &'static str {
    match d {
        b',' => "comma",
        b'\t' => "tab",
        b';' => "semicolon",
        b'|' => "pipe",
        _ => "other",
    }
}

fn title(name: &str, grid: &GridView, choice: DelimiterChoice, save: &SaveState) -> String {
    let edits = grid.edit_count();
    let mut t = String::new();
    if edits > 0 {
        t.push_str("• ");
    }
    t.push_str(name);
    t.push_str(" — ");
    t.push_str(&group_digits(grid.row_count()));
    t.push_str(" rows, ");
    t.push_str(delimiter_name(grid.delimiter()));
    if choice == DelimiterChoice::Auto {
        t.push_str(" (detected)");
    }
    let encoding = grid.encoding();
    if encoding != csv_engine::Encoding::UTF8 {
        t.push_str(", ");
        t.push_str(encoding.name());
    }
    if !grid.is_complete() {
        t.push_str(" (indexing…)");
    }
    if edits > 0 {
        t.push_str(&format!(
            " — {edits} unsaved edit{}",
            if edits == 1 { "" } else { "s" }
        ));
    }
    match save {
        SaveState::Saving(_) => t.push_str(" — saving…"),
        SaveState::Saved(took) if edits == 0 => {
            t.push_str(&format!(" — saved in {:.1} s", took.as_secs_f64()))
        }
        SaveState::Failed(e) => t.push_str(&format!(" — save failed: {e}")),
        _ => {}
    }
    if grid.file_changed() {
        t.push_str(" — file changed on disk");
    }
    t
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
        proc_status_kib("RssAnon:"),
        (vadj.value() / ROW_H) as u64,
    );
}

/// A `kB` field of /proc/self/status, e.g. `RssAnon:` or `VmHWM:`.
fn proc_status_kib(key: &str) -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with(key))?
                .split_whitespace()
                .nth(1)?
                .parse()
                .ok()
        })
        .unwrap_or(0)
}
