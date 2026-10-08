//! Application shell: windows, menus, dialogs, OS integration. Toolkit: GTK4 (ADR 0001).
//!
//! Usage: `spreadsheet [FILE.csv] [--bench-scroll FRAMES | --bench-jump ROW [--jumps N]]`
//!
//! Without a file it shows a start window; Ctrl+O or an Open button picks one through the
//! desktop's file dialog (xdg-desktop-portal via `gtk::FileDialog`, APP-1), and Ctrl+N or
//! New starts an untitled table (SAVE-2). Each file gets its own window. Ctrl+Shift+S
//! saves as another file, delimiter, or encoding.
//!
//! Benchmark modes print JSON and exit; the acceptance tests run them.
//! - `--bench-scroll` (GRID-1) waits for indexing, jumps to the middle of the file, scrolls
//!   37 px per frame (a fast flick) for FRAMES frames, and reports frame times and memory.
//! - `--bench-jump` (GRID-2) moves the vertical adjustment the way a scrollbar drag does:
//!   first to ROW, then to N-1 random rows, and reports how long each took to paint.
//! - `--bench-status` (APP-2) prints the status bar's row text (`STATUS …`) on every status
//!   update while the file indexes, and quits once indexing is complete.

mod editor;
mod grid_view;
mod status;

use csv_engine::{Charset, Encoding};
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

#[derive(Clone, Copy)]
enum Bench {
    Scroll { frames: u32 },
    Jump { row: u64, jumps: u32 },
    Open,
    Status,
}

struct Args {
    file: Option<PathBuf>,
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
            "--bench-status" => bench = Some(Bench::Status),
            _ if file.is_none() && !a.starts_with("--") => file = Some(PathBuf::from(a)),
            _ => return Err(format!("unexpected argument {a:?}")),
        }
    }
    if let Some(Bench::Jump { jumps: j, .. }) = &mut bench {
        *j = jumps;
    }
    if bench.is_some() && file.is_none() {
        return Err("benchmarks need a file".into());
    }
    Ok(Args { file, bench })
}

fn main() -> glib::ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("spreadsheet: {e}\nusage: spreadsheet [FILE.csv] [--bench-scroll FRAMES | --bench-jump ROW [--jumps N]]");
            return glib::ExitCode::from(2);
        }
    };
    // A file named on the command line is mapped and indexing before the toolkit starts,
    // so its first rows show as early as possible (BENCH-1 times this path).
    let opened = match &args.file {
        Some(path) => match open_session(path) {
            Ok(opened) => Some(opened),
            Err(e) => {
                eprintln!("spreadsheet: cannot open {}: {e}", path.display());
                return glib::ExitCode::FAILURE;
            }
        },
        None => None,
    };

    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_startup(|app| {
        let open = gtk::gio::ActionEntry::builder("open")
            .activate(|app: &gtk::Application, _, _| choose_and_open(app))
            .build();
        let new = gtk::gio::ActionEntry::builder("new")
            .activate(|app: &gtk::Application, _, _| new_window(app))
            .build();
        app.add_action_entries([open, new]);
        app.set_accels_for_action("app.open", &["<Control>o"]);
        app.set_accels_for_action("app.new", &["<Control>n"]);
    });
    let (opened, bench) = (RefCell::new(opened), args.bench);
    app.connect_activate(move |app| match opened.take() {
        Some((session, table)) => build_window(app, &session, table, bench),
        None if app.windows().is_empty() => start_window(app),
        None => {}
    });
    app.run_with_args::<&str>(&[])
}

/// Map `path` and start indexing it.
fn open_session(path: &Path) -> std::io::Result<(Rc<Session>, CsvTable)> {
    let session = Rc::new(Session::new(Some(path.to_owned())));
    let table = session.open()?;
    Ok((session, table))
}

thread_local! {
    /// Every window and the file it shows: opening a file again brings its window forward
    /// instead of a second window that would save over the first one's edits. Looked up by
    /// the session's current path, so it follows Save As.
    static OPEN_WINDOWS: RefCell<Vec<(Rc<Session>, glib::WeakRef<gtk::ApplicationWindow>)>> =
        const { RefCell::new(Vec::new()) };
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}

/// The window already showing `path`, if any.
fn window_for(path: &Path) -> Option<gtk::ApplicationWindow> {
    let path = canonical(path);
    OPEN_WINDOWS.with_borrow_mut(|open| {
        open.retain(|(_, w)| w.upgrade().is_some());
        open.iter()
            .find(|(s, _)| s.path.borrow().as_deref().map(canonical) == Some(path.clone()))
            .and_then(|(_, w)| w.upgrade())
    })
}

/// Ctrl+N (SAVE-2): a new window with an untitled, empty table; type into its edge row
/// and column to fill it, and Save asks where to put it.
fn new_window(app: &gtk::Application) {
    let session = Rc::new(Session::new(None));
    build_window(app, &session, CsvTable::untitled(), None);
    close_start_window(app);
}

fn close_start_window(app: &gtk::Application) {
    for w in app.windows() {
        if w.widget_name() == START_WINDOW {
            w.close();
        }
    }
}

/// Shown when the app starts without a file: buttons that open a file or start a new one.
fn start_window(app: &gtk::Application) {
    let heading = gtk::Label::new(Some("Open a CSV file"));
    heading.add_css_class("title-2");
    let open = gtk::Button::with_label("Open…");
    open.add_css_class("suggested-action");
    open.add_css_class("pill");
    open.set_halign(gtk::Align::Center);
    open.set_action_name(Some("app.open"));
    let new = gtk::Button::with_label("New");
    new.add_css_class("pill");
    new.set_halign(gtk::Align::Center);
    new.set_action_name(Some("app.new"));
    let hint = gtk::Label::new(Some("Ctrl+O, Ctrl+N, or run: spreadsheet FILE.csv"));
    hint.add_css_class("dim-label");
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    page.append(&heading);
    page.append(&open);
    page.append(&new);
    page.append(&hint);
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Spreadsheet")
        .default_width(900)
        .default_height(600)
        .child(&page)
        .build();
    window.set_widget_name(START_WINDOW);
    window.set_titlebar(Some(&gtk::HeaderBar::new()));
    window.present();
    open.grab_focus();
}

const START_WINDOW: &str = "start";

/// "CSV and text files" and "All files", the first as the default.
fn file_filters() -> (gtk::gio::ListStore, gtk::FileFilter) {
    let csv = gtk::FileFilter::new();
    csv.set_name(Some("CSV and text files"));
    for mime in ["text/csv", "text/tab-separated-values", "text/plain"] {
        csv.add_mime_type(mime);
    }
    for suffix in ["csv", "tsv", "tab", "psv", "txt"] {
        csv.add_suffix(suffix);
    }
    let all = gtk::FileFilter::new();
    all.set_name(Some("All files"));
    all.add_pattern("*");
    let filters = gtk::gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&csv);
    filters.append(&all);
    (filters, csv)
}

/// The file dialog (APP-1). `gtk::FileDialog` goes through xdg-desktop-portal's
/// FileChooser when one is running, so each desktop shows its own dialog (GTK falls back
/// to its built-in one without a portal).
fn choose_and_open(app: &gtk::Application) {
    let (filters, csv) = file_filters();
    let dialog = gtk::FileDialog::builder()
        .title("Open")
        .modal(true)
        .filters(&filters)
        .default_filter(&csv)
        .build();
    let (app, parent) = (app.clone(), app.active_window());
    glib::spawn_future_local(async move {
        match dialog.open_future(parent.as_ref()).await {
            Ok(file) => match file.path() {
                Some(path) => open_window(&app, &path, parent.as_ref()),
                None => alert(
                    parent.as_ref(),
                    &format!("Cannot open {}", file.uri()),
                    "Only local files can be opened.",
                ),
            },
            Err(e) if e.matches(gtk::DialogError::Dismissed) => {}
            Err(e) => alert(
                parent.as_ref(),
                "Cannot show the file dialog",
                &e.to_string(),
            ),
        }
    });
}

/// Show `path` in a window: its existing one, or a new one. The start window closes once
/// a file is open.
fn open_window(app: &gtk::Application, path: &Path, parent: Option<&gtk::Window>) {
    if let Some(window) = window_for(path) {
        window.present();
        return;
    }
    match open_session(path) {
        Ok((session, table)) => {
            build_window(app, &session, table, None);
            close_start_window(app);
        }
        Err(e) => alert(
            parent,
            &format!("Cannot open {}", path.display()),
            &e.to_string(),
        ),
    }
}

fn alert(parent: Option<&gtk::Window>, message: &str, detail: &str) {
    gtk::AlertDialog::builder()
        .message(message)
        .detail(detail)
        .modal(true)
        .build()
        .show(parent);
}

/// The open file: where it is (`None` while untitled), how its delimiter and encoding are
/// chosen, and the index build in flight.
struct Session {
    path: RefCell<Option<PathBuf>>,
    choice: Cell<DelimiterChoice>,
    /// The encoding a Save As wrote (SAVE-2); `None` while detection decides.
    encoding: Cell<Option<Encoding>>,
    /// "First row is header" as the user set it (ENG-7); `None` while detection decides.
    /// Re-applied whenever the file is read again (delimiter change, save).
    header: Cell<Option<bool>>,
    /// Cancel flag of the running index build; set when a newer reading replaces it.
    indexing: RefCell<Arc<AtomicBool>>,
}

impl Session {
    fn new(path: Option<PathBuf>) -> Self {
        Self {
            path: RefCell::new(path),
            choice: Cell::new(DelimiterChoice::Auto),
            encoding: Cell::new(None),
            header: Cell::new(None),
            indexing: RefCell::new(Arc::new(AtomicBool::new(false))),
        }
    }

    /// The window title's file name: the file's, or "Untitled".
    fn name(&self) -> String {
        self.path.borrow().as_deref().map_or_else(
            || "Untitled".to_owned(),
            |p| {
                p.file_name()
                    .map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into())
            },
        )
    }

    /// Map the file with the current delimiter choice (detected on Auto, ENG-4) and
    /// encoding (detected unless a Save As chose it), and start indexing it.
    fn open(&self) -> std::io::Result<CsvTable> {
        let path = self
            .path
            .borrow()
            .clone()
            .ok_or_else(|| std::io::Error::other("an untitled table has no file to read"))?;
        let mut table = match self.encoding.get() {
            Some(encoding) => CsvTable::open_as(&path, self.choice.get(), encoding)?,
            None => CsvTable::open(&path, self.choice.get())?,
        };
        self.apply_header(&mut table);
        self.index(&table)?;
        Ok(table)
    }

    /// The user's header choice wins over detection on a fresh reading of the file.
    fn apply_header(&self, table: &mut CsvTable) {
        if let Some(on) = self.header.get() {
            table.set_header(on);
        }
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
                Ok(mut table) => {
                    session.apply_header(&mut table);
                    if let Err(e) = session.index(&table) {
                        eprintln!("spreadsheet: cannot index: {e}");
                        return;
                    }
                    session.choice.set(choice);
                    // Other columns now: the old widths don't belong to them.
                    grid.reset_column_widths();
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

/// "Header row" toggle (ENG-7): whether the first row is column titles or data. Starts as
/// detected; flipping it is the user's choice for this file from then on.
fn header_toggle(session: &Rc<Session>, grid: &GridView) -> gtk::ToggleButton {
    let toggle = gtk::ToggleButton::with_label("Header row");
    toggle.set_tooltip_text(Some("First row is header"));
    toggle.set_active(grid.has_header());
    toggle.connect_toggled({
        let (session, grid) = (session.clone(), grid.downgrade());
        move |t| {
            let Some(grid) = grid.upgrade() else { return };
            // The status tick also sets the toggle to match the grid; only a real flip counts.
            if t.is_active() != grid.has_header() {
                grid.set_header(t.is_active());
                session.header.set(Some(t.is_active()));
            }
        }
    });
    toggle
}

fn build_window(
    app: &gtk::Application,
    session: &Rc<Session>,
    table: CsvTable,
    bench: Option<Bench>,
) {
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
    let bar = StatusBar::new();
    layout.attach(&bar.root, 0, 2, 2, 1);

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title(session.name())
        .default_width(1400)
        .default_height(900)
        .child(&layout)
        .build();
    OPEN_WINDOWS.with_borrow_mut(|open| open.push((session.clone(), window.downgrade())));
    let delimiter = delimiter_dropdown(session, &grid);
    let open = gtk::Button::from_icon_name("document-open-symbolic");
    open.set_tooltip_text(Some("Open… (Ctrl+O)"));
    open.set_action_name(Some("app.open"));
    let new = gtk::Button::from_icon_name("document-new-symbolic");
    new.set_tooltip_text(Some("New (Ctrl+N)"));
    new.set_action_name(Some("app.new"));
    let save = Rc::new(RefCell::new(SaveState::Idle));
    let save_as_button = gtk::Button::from_icon_name("document-save-as-symbolic");
    save_as_button.set_tooltip_text(Some("Save As… (Ctrl+Shift+S)"));
    save_as_button.connect_clicked({
        let (window, grid, session, save) = (
            window.downgrade(),
            grid.downgrade(),
            session.clone(),
            save.clone(),
        );
        move |_| {
            if let (Some(window), Some(grid)) = (window.upgrade(), grid.upgrade()) {
                grid.finish_editing();
                save_as(&window, &grid, &session, &save);
            }
        }
    });
    let header = gtk::HeaderBar::new();
    header.pack_start(&open);
    header.pack_start(&new);
    header.pack_start(&save_as_button);
    header.pack_end(&delimiter);
    let header_row = header_toggle(session, &grid);
    header.pack_end(&header_row);
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

    // Ctrl+S saves (asking where, for an untitled table) and Ctrl+Shift+S saves as;
    // Ctrl+Z undoes, Ctrl+Shift+Z and Ctrl+Y redo (CMD-1). Navigation keys belong to the
    // grid (GRID-3); the cell editor's entry keeps its own text undo.
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed({
        let (window, grid, save, session) = (
            window.downgrade(),
            grid.downgrade(),
            save.clone(),
            session.clone(),
        );
        move |_, key, _, mods| {
            use gtk::gdk::{Key, ModifierType};
            let (Some(window), Some(grid)) = (window.upgrade(), grid.upgrade()) else {
                return glib::Propagation::Proceed;
            };
            if !mods.contains(ModifierType::CONTROL_MASK) {
                return glib::Propagation::Proceed;
            }
            let shift = mods.contains(ModifierType::SHIFT_MASK);
            match key {
                Key::s | Key::S => {
                    grid.finish_editing(); // save what was typed, as Calc does
                    if shift || session.path.borrow().is_none() {
                        save_as(&window, &grid, &session, &save);
                    } else {
                        start_save(&grid, &session, &save, None);
                    }
                }
                Key::z if !shift => {
                    grid.undo();
                }
                Key::Z | Key::z | Key::y | Key::Y => {
                    grid.redo();
                }
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        }
    });
    window.add_controller(keys);

    // Status: follow the indexer (grow the scroll range), finish saves, keep the status bar
    // and title current. Runs once now, so the bar is filled from the first frame, then
    // every 100 ms.
    let started = Instant::now();
    let mut was_complete = false;
    let mut tick = {
        let (grid, window, session, app) = (
            grid.downgrade(),
            window.downgrade(),
            session.clone(),
            app.clone(),
        );
        move || {
            let (Some(grid), Some(window)) = (grid.upgrade(), window.upgrade()) else {
                return glib::ControlFlow::Break;
            };
            // Finish a save first: it can swap in the reopened file, whose indexing the
            // rest of this tick follows.
            finish_save(&grid, &session, &save);
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
            // Detection runs again when the file is re-read (delimiter change, save).
            if header_row.is_active() != grid.has_header() {
                header_row.set_active(grid.has_header());
            }
            let can_switch =
                grid.edit_count() == 0 && !matches!(*save.borrow(), SaveState::Saving(..));
            delimiter.set_sensitive(can_switch);
            delimiter.set_tooltip_text(Some(if can_switch {
                "Delimiter"
            } else {
                "Delimiter (save your edits before changing it)"
            }));
            // A Save As can change the delimiter choice.
            let choice = DELIMITERS
                .iter()
                .position(|(_, c)| *c == session.choice.get())
                .unwrap_or(0) as u32;
            if delimiter.selected() != choice {
                delimiter.set_selected(choice);
            }
            let shown = bar.update(&grid, session.choice.get(), &save.borrow());
            let title = status::title(&session.name(), grid.edit_count());
            if window.title().as_deref() != Some(title.as_str()) {
                window.set_title(Some(&title));
            }
            if matches!(bench, Some(Bench::Status)) {
                // The status acceptance test reads every update of the row count.
                println!("STATUS {}", shown.rows);
                if complete {
                    app.quit();
                }
            }
            was_complete = complete;
            glib::ControlFlow::Continue
        }
    };
    tick();
    glib::timeout_add_local(Duration::from_millis(100), tick);

    window.present();
    grid.grab_focus();
    match bench {
        Some(Bench::Scroll { frames }) => bench_scroll(app, &grid, &vadj, frames),
        Some(Bench::Jump { row, jumps }) => bench_jump(app, &grid, &vadj, row, jumps),
        Some(Bench::Open) => bench_open(app, &grid),
        Some(Bench::Status) | None => {}
    }
}

/// Open benchmark (BENCH-1, APP-3). Prints `FIRST_FRAME {json}` at the end of the first
/// frame that shows rows (or, for a file with none, the first frame at all), with the rows
/// indexed so far and whether indexing was complete; then, once indexing is done,
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
                // How far indexing had got when rows first showed (APP-3).
                println!(
                    r#"FIRST_FRAME {{"rows_indexed":{},"complete":{}}}"#,
                    grid.row_count(),
                    grid.is_complete()
                );
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

/// Where and how a Save As writes (SAVE-2).
struct Target {
    path: PathBuf,
    delimiter: u8,
    encoding: Encoding,
}

enum SaveState {
    Idle,
    Saving(
        mpsc::Receiver<Result<(SaveStats, Duration), String>>,
        Option<Target>,
    ),
    Saved(Duration),
    Failed(String),
}

/// Ctrl+S: write the table to its own file on a worker thread (save is atomic, SAVE-1);
/// with a `target`, to another file, delimiter, or encoding (Save As, SAVE-2). Editing is
/// paused until it finishes, so no edit can be lost between snapshot and reopen.
fn start_save(
    grid: &GridView,
    session: &Session,
    save: &RefCell<SaveState>,
    target: Option<Target>,
) {
    if matches!(*save.borrow(), SaveState::Saving(..))
        || (target.is_none() && grid.edit_count() == 0)
    {
        return;
    }
    let Some(path) = target
        .as_ref()
        .map(|t| t.path.clone())
        .or_else(|| session.path.borrow().clone())
    else {
        return;
    };
    let job = match grid.save_job() {
        Ok(job) => match &target {
            Some(t) => job.with_format(t.delimiter, t.encoding),
            None => job,
        },
        Err(_) => {
            *save.borrow_mut() = SaveState::Failed("wait for indexing to finish".into());
            return;
        }
    };
    grid.set_saving(true);
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("save".into())
        .spawn(move || {
            let t = Instant::now();
            let r = file_format::save_csv(&path, &job).map_err(|e| e.to_string());
            let _ = tx.send(r.map(|s| (s, t.elapsed())));
        });
    *save.borrow_mut() = match spawned {
        Ok(_) => SaveState::Saving(rx, target),
        Err(e) => {
            grid.set_saving(false);
            SaveState::Failed(e.to_string())
        }
    };
}

/// When a save has finished: reopen the saved file (its edits are now on disk; after a
/// Save As, the new file with its delimiter and encoding) or report the error with the
/// edits still in memory.
fn finish_save(grid: &GridView, session: &Session, save: &RefCell<SaveState>) {
    let (result, target) = match &mut *save.borrow_mut() {
        SaveState::Saving(rx, target) => match rx.try_recv() {
            Ok(r) => (r, target.take()),
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => (Err("the save thread stopped".into()), None),
        },
        _ => return,
    };
    grid.set_saving(false);
    if let (Ok(_), Some(t)) = (&result, target) {
        session.path.replace(Some(t.path));
        session.encoding.set(Some(t.encoding));
        session.choice.set(DelimiterChoice::Fixed(t.delimiter));
    }
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

/// Delimiters Save As offers, in dropdown order.
const SAVE_DELIMITERS: [(&str, u8); 4] = [
    ("Comma", b','),
    ("Tab", b'\t'),
    ("Semicolon", b';'),
    ("Pipe", b'|'),
];

/// Encodings Save As offers, in dropdown order. UTF-16 is written with its BOM.
const SAVE_ENCODINGS: [(&str, Encoding); 5] = [
    ("UTF-8", Encoding::UTF8),
    (
        "UTF-8 with BOM",
        Encoding {
            charset: Charset::Utf8,
            bom: true,
        },
    ),
    (
        "UTF-16 LE",
        Encoding {
            charset: Charset::Utf16Le,
            bom: true,
        },
    ),
    (
        "UTF-16 BE",
        Encoding {
            charset: Charset::Utf16Be,
            bom: true,
        },
    ),
    (
        "Windows-1252",
        Encoding {
            charset: Charset::Windows1252,
            bom: false,
        },
    ),
];

/// Ctrl+Shift+S (SAVE-2): pick the delimiter and encoding (the file's own preselected),
/// then the file, in the desktop's save dialog; the table is written there and the window
/// shows that file from then on.
fn save_as(
    window: &gtk::ApplicationWindow,
    grid: &GridView,
    session: &Rc<Session>,
    save: &Rc<RefCell<SaveState>>,
) {
    if matches!(*save.borrow(), SaveState::Saving(..)) {
        return;
    }
    let delimiters = gtk::DropDown::from_strings(&SAVE_DELIMITERS.map(|(n, _)| n));
    delimiters.set_selected(
        SAVE_DELIMITERS
            .iter()
            .position(|(_, d)| *d == grid.delimiter())
            .unwrap_or(0) as u32,
    );
    let encodings = gtk::DropDown::from_strings(&SAVE_ENCODINGS.map(|(n, _)| n));
    let current = grid.encoding();
    encodings.set_selected(
        SAVE_ENCODINGS
            .iter()
            .position(|(_, e)| *e == current)
            .or_else(|| {
                SAVE_ENCODINGS
                    .iter()
                    .position(|(_, e)| e.charset == current.charset)
            })
            .unwrap_or(0) as u32,
    );
    let form = gtk::Grid::builder()
        .row_spacing(10)
        .column_spacing(12)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();
    for (row, (label, dropdown)) in [("Delimiter", &delimiters), ("Encoding", &encodings)]
        .into_iter()
        .enumerate()
    {
        let label = gtk::Label::new(Some(label));
        label.set_xalign(1.0);
        form.attach(&label, 0, row as i32, 1, 1);
        dropdown.set_hexpand(true);
        form.attach(dropdown, 1, row as i32, 1, 1);
    }
    let cancel = gtk::Button::with_label("Cancel");
    let choose = gtk::Button::with_label("Choose File…");
    choose.add_css_class("suggested-action");
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    buttons.append(&cancel);
    buttons.append(&choose);
    form.attach(&buttons, 0, 2, 2, 1);
    let dialog = gtk::Window::builder()
        .title("Save As")
        .transient_for(window)
        .modal(true)
        .resizable(false)
        .child(&form)
        .default_widget(&choose)
        .build();
    let esc = gtk::EventControllerKey::new();
    esc.connect_key_pressed({
        let dialog = dialog.downgrade();
        move |_, key, _, _| {
            if key != gtk::gdk::Key::Escape {
                return glib::Propagation::Proceed;
            }
            if let Some(d) = dialog.upgrade() {
                d.close();
            }
            glib::Propagation::Stop
        }
    });
    dialog.add_controller(esc);
    cancel.connect_clicked({
        let dialog = dialog.downgrade();
        move |_| {
            if let Some(d) = dialog.upgrade() {
                d.close();
            }
        }
    });
    choose.connect_clicked({
        let (dialog, window, grid, session, save) = (
            dialog.downgrade(),
            window.downgrade(),
            grid.downgrade(),
            session.clone(),
            save.clone(),
        );
        move |_| {
            let delimiter = SAVE_DELIMITERS[delimiters.selected() as usize].1;
            let encoding = SAVE_ENCODINGS[encodings.selected() as usize].1;
            if let Some(d) = dialog.upgrade() {
                d.close();
            }
            let (Some(window), Some(grid)) = (window.upgrade(), grid.upgrade()) else {
                return;
            };
            choose_save_file(&window, &grid, &session, &save, delimiter, encoding);
        }
    });
    dialog.present();
}

/// The second step of Save As: the desktop's save dialog, starting next to the current
/// file with its name, the extension following the delimiter (`.tsv` for tabs).
fn choose_save_file(
    window: &gtk::ApplicationWindow,
    grid: &GridView,
    session: &Rc<Session>,
    save: &Rc<RefCell<SaveState>>,
    delimiter: u8,
    encoding: Encoding,
) {
    let (filters, csv) = file_filters();
    let name = suggested_name(&session.name(), session.path.borrow().is_none(), delimiter);
    let builder = gtk::FileDialog::builder()
        .title("Save As")
        .modal(true)
        .filters(&filters)
        .default_filter(&csv)
        .initial_name(name);
    let folder = session
        .path
        .borrow()
        .as_deref()
        .and_then(Path::parent)
        .map(gtk::gio::File::for_path);
    let dialog = match folder {
        Some(folder) => builder.initial_folder(&folder).build(),
        None => builder.build(),
    };
    let (window, grid, session, save) = (
        window.clone(),
        grid.downgrade(),
        session.clone(),
        save.clone(),
    );
    glib::spawn_future_local(async move {
        let file = match dialog.save_future(Some(&window)).await {
            Ok(file) => file,
            Err(e) if e.matches(gtk::DialogError::Dismissed) => return,
            Err(e) => {
                alert(
                    Some(window.upcast_ref()),
                    "Cannot show the file dialog",
                    &e.to_string(),
                );
                return;
            }
        };
        let Some(path) = file.path() else {
            alert(
                Some(window.upcast_ref()),
                &format!("Cannot save to {}", file.uri()),
                "Only local files can be saved.",
            );
            return;
        };
        if window_for(&path).is_some_and(|w| w != window) {
            alert(
                Some(window.upcast_ref()),
                &format!("{} is open in another window", path.display()),
                "Close it there first, or save under another name.",
            );
            return;
        }
        let Some(grid) = grid.upgrade() else { return };
        let target = Target {
            path,
            delimiter,
            encoding,
        };
        start_save(&grid, &session, &save, Some(target));
    });
}

/// The name Save As starts with: the file's own, with `.tsv` for tab-separated output and
/// `.csv` otherwise when it had one of those extensions; `Untitled.csv`/`.tsv` for a new
/// table.
fn suggested_name(name: &str, untitled: bool, delimiter: u8) -> String {
    let ext = if delimiter == b'\t' { "tsv" } else { "csv" };
    if untitled {
        return format!("{name}.{ext}");
    }
    let path = Path::new(name);
    match path.extension().and_then(|e| e.to_str()) {
        Some(e) if e.eq_ignore_ascii_case("csv") || e.eq_ignore_ascii_case("tsv") => {
            path.with_extension(ext).to_string_lossy().into_owned()
        }
        _ => name.to_owned(),
    }
}

/// The bar under the grid (APP-2): rows and indexing progress on the left, save state and
/// the file's format on the right.
struct StatusBar {
    root: gtk::Box,
    rows: gtk::Label,
    save: gtk::Label,
    format: gtk::Label,
}

impl StatusBar {
    fn new() -> Self {
        let label = || {
            let l = gtk::Label::new(None);
            l.add_css_class("caption");
            l
        };
        let (rows, save, format) = (label(), label(), label());
        rows.set_hexpand(true);
        rows.set_xalign(0.0);
        save.set_ellipsize(gtk::pango::EllipsizeMode::End);
        format.add_css_class("dim-label");
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(18)
            .margin_start(10)
            .margin_end(10)
            .margin_top(3)
            .margin_bottom(3)
            .build();
        root.append(&rows);
        root.append(&save);
        root.append(&format);
        Self {
            root,
            rows,
            save,
            format,
        }
    }

    /// Show the grid's current state; returns what is shown.
    fn update(&self, grid: &GridView, choice: DelimiterChoice, save: &SaveState) -> status::Status {
        let shown = status::status(&status::Facts {
            rows: grid.row_count(),
            indexing: grid.index_progress(),
            delimiter: grid.delimiter(),
            detected: choice == DelimiterChoice::Auto,
            encoding: grid.encoding(),
            edits: grid.edit_count(),
            save: match save {
                SaveState::Idle => status::SaveView::Idle,
                SaveState::Saving(..) => status::SaveView::Saving,
                SaveState::Saved(took) => status::SaveView::Saved(*took),
                SaveState::Failed(e) => status::SaveView::Failed(e),
            },
            file_changed: grid.file_changed(),
            clip: grid.clip_view(),
        });
        for (label, text) in [
            (&self.rows, &shown.rows),
            (&self.save, &shown.save),
            (&self.format, &shown.format),
        ] {
            if label.text() != text.as_str() {
                label.set_text(text);
            }
        }
        shown
    }
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
