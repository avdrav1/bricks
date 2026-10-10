//! Application shell: windows, menus, dialogs, OS integration. Toolkit: GTK4 (ADR 0001).
//!
//! Usage: `spreadsheet [FILE.csv] [--version] [--bench-scroll FRAMES | --bench-jump ROW [--jumps N] | --bench-save EDITS | --bench-find TEXT]`
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
//! - `--bench-save` (SAVE-3) pastes EDITS cells, saves, scrolls while the save runs, and
//!   reports the save time, progress updates, main-loop lateness, and frame times. It only
//!   saves files under the temp directory.
//! - `--bench-find TEXT` (SRCH-2) types TEXT into the find bar a character at a time and
//!   prints each keystroke, search start, and result (`FIND …`).

mod crash;
mod editor;
mod grid_view;
mod malformed;
mod recent;
mod recovery;
mod status;
mod theme;

use csv_engine::{Charset, Encoding, IndexError};
use data_model::{CsvTable, DelimiterChoice, RereadError, SaveProgress, SaveStats, Step};
use grid_view::{GridView, ROW_H};
use gtk::{glib, prelude::*};
use status::FindNote;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

const APP_ID: &str = "dev.bricks.Spreadsheet";

#[derive(Clone)]
enum Bench {
    Scroll { frames: u32 },
    Jump { row: u64, jumps: u32 },
    Open,
    Status,
    Save { edits: u32 },
    Find { text: String },
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
            // For the installer and release checks (PKG-1, PKG-3).
            "--version" => {
                println!("spreadsheet {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--bench-open" => bench = Some(Bench::Open),
            "--bench-status" => bench = Some(Bench::Status),
            "--bench-find" => {
                let text = it.next().ok_or("--bench-find needs text")?;
                bench = Some(Bench::Find { text })
            }
            "--bench-save" => {
                bench = Some(Bench::Save {
                    edits: number(&a, &mut it)?.max(1) as u32,
                })
            }
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
            eprintln!("spreadsheet: {e}\nusage: spreadsheet [FILE.csv] [--version] [--bench-scroll FRAMES | --bench-jump ROW [--jumps N] | --bench-save EDITS | --bench-find TEXT]");
            return glib::ExitCode::from(2);
        }
    };
    // The save benchmark overwrites its file: only ever a scratch copy.
    if let (Some(Bench::Save { .. }), Some(file)) = (&args.bench, &args.file) {
        let canonical = |p: &Path| p.canonicalize().ok();
        let scratch = canonical(file)
            .zip(canonical(&std::env::temp_dir()))
            .is_some_and(|(f, tmp)| f.starts_with(tmp));
        if !scratch {
            eprintln!("spreadsheet: --bench-save only saves files under the temp directory");
            return glib::ExitCode::from(2);
        }
    }
    // A file named on the command line is mapped and indexing before the toolkit starts,
    // so its first rows show as early as possible (BENCH-1 times this path). One that
    // can't be opened is said so on the start window (a file manager's launch has no
    // terminal to print to); benchmarks just fail.
    let mut failed = None;
    let opened = match &args.file {
        Some(path) => match open_session(path) {
            Ok(opened) => {
                // Benchmarks and tests open corpus files: they stay off the list.
                if args.bench.is_none() {
                    remember(path);
                }
                Some(opened)
            }
            Err(e) => {
                eprintln!("spreadsheet: cannot open {}: {e}", path.display());
                if args.bench.is_some() {
                    return glib::ExitCode::FAILURE;
                }
                failed = Some((format!("Cannot open {}", path.display()), e.to_string()));
                None
            }
        },
        None => None,
    };

    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_startup(|app| {
        theme::follow_color_scheme();
        let open = gtk::gio::ActionEntry::builder("open")
            .activate(|app: &gtk::Application, _, _| choose_and_open(app))
            .build();
        let new = gtk::gio::ActionEntry::builder("new")
            .activate(|app: &gtk::Application, _, _| new_window(app))
            .build();
        let open_recent = gtk::gio::ActionEntry::builder("open-recent")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(|app: &gtk::Application, _, param| {
                if let Some(path) = param.and_then(|p| p.str()) {
                    open_window(app, Path::new(path), app.active_window().as_ref());
                }
            })
            .build();
        let clear_recent = gtk::gio::ActionEntry::builder("clear-recent")
            .activate(|_: &gtk::Application, _, _| {
                if let Err(e) = recent::Recent::load().clear() {
                    eprintln!("spreadsheet: cannot clear recent files: {e}");
                }
            })
            .build();
        app.add_action_entries([open, new, open_recent, clear_recent]);
        app.set_accels_for_action("app.open", &["<Control>o"]);
        app.set_accels_for_action("app.new", &["<Control>n"]);
    });
    let (opened, failed, bench) = (RefCell::new(opened), RefCell::new(failed), args.bench);
    app.connect_activate(move |app| match opened.take() {
        Some((session, table)) => build_window(app, &session, table, bench.clone()),
        None if app.windows().is_empty() => {
            let window = start_window(app);
            if let Some((message, detail)) = failed.take() {
                alert(Some(window.upcast_ref()), &message, &detail);
            }
        }
        None => {}
    });
    app.run_with_args::<&str>(&[])
}

/// Map `path` and start indexing it.
fn open_session(path: &Path) -> std::io::Result<(Rc<Session>, CsvTable)> {
    let session = Rc::new(Session::new(Some(path.to_owned())));
    let table = session.open()?;
    malformed::refuse_binary(&table)?;
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

/// Shown when the app starts without a file: buttons that open a file or start a new one,
/// and the recent files (APP-4).
fn start_window(app: &gtk::Application) -> gtk::ApplicationWindow {
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
    let hint = gtk::Label::new(Some(
        "Ctrl+O, Ctrl+N, drop a file here, or run: spreadsheet FILE.csv",
    ));
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
    // Unsaved changes from a crash (APP-7) come first: putting them back is likely next.
    let restore = crash::start_section(app).map(|(section, restore)| {
        page.append(&section);
        restore
    });
    let files = recent::Recent::load().files().to_vec();
    if !files.is_empty() {
        let title = gtk::Label::new(Some("Recent"));
        title.add_css_class("heading");
        title.set_margin_top(12);
        page.append(&title);
        for file in files {
            page.append(&recent_button(app, &file));
        }
    }
    page.append(&hint);
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Spreadsheet")
        .default_width(900)
        .default_height(600)
        .child(&page)
        .build();
    window.set_widget_name(START_WINDOW);
    accept_file_drops(app, &window);
    window.set_titlebar(Some(&gtk::HeaderBar::new()));
    // Enter acts on it: Restore after a crash, else Open.
    let first = restore.unwrap_or(open);
    window.set_default_widget(Some(&first));
    GtkWindowExt::set_focus(&window, Some(&first));
    window.present();
    window
}

/// A recent file on the start window: its name and folder; it goes away if the file
/// can't be opened any more.
fn recent_button(app: &gtk::Application, file: &Path) -> gtk::Button {
    let (name, dir) = recent::label(file);
    let text = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    text.append(&gtk::Label::new(Some(&name)));
    let folder = gtk::Label::new(Some(&dir));
    folder.add_css_class("dim-label");
    text.append(&folder);
    let button = gtk::Button::builder()
        .child(&text)
        .tooltip_text(file.display().to_string())
        .css_classes(["flat"])
        .build();
    button.connect_clicked({
        let (app, file) = (app.clone(), file.to_owned());
        move |b| {
            let window = b.root().and_downcast::<gtk::Window>();
            if !open_window(&app, &file, window.as_ref()) {
                b.set_visible(false);
            }
        }
    });
    button
}

/// The recent files menu in a window's header bar (APP-4), read again each time it opens.
fn recent_menu() -> gtk::MenuButton {
    let button = gtk::MenuButton::builder()
        .icon_name("pan-down-symbolic")
        .tooltip_text("Recent files")
        .build();
    button.set_create_popup_func(|button| {
        let menu = gtk::gio::Menu::new();
        let files = gtk::gio::Menu::new();
        for file in recent::Recent::load().files() {
            // A path that isn't UTF-8 can't travel in the action's string.
            let Some(path) = file.to_str() else { continue };
            let (name, dir) = recent::label(file);
            let item = gtk::gio::MenuItem::new(Some(&format!("{name}  ({dir})")), None);
            item.set_action_and_target_value(Some("app.open-recent"), Some(&path.to_variant()));
            files.append_item(&item);
        }
        if files.n_items() == 0 {
            files.append(Some("No recent files"), Some("app.none"));
        }
        menu.append_section(None, &files);
        let clear = gtk::gio::Menu::new();
        clear.append(Some("Clear Recent Files"), Some("app.clear-recent"));
        menu.append_section(None, &clear);
        button.set_menu_model(Some(&menu));
    });
    button
}

/// Put `path` at the top of the recent files; a failure only costs the list.
fn remember(path: &Path) {
    if let Err(e) = recent::Recent::load().add(path) {
        eprintln!("spreadsheet: cannot update recent files: {e}");
    }
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
                Some(path) => {
                    open_window(&app, &path, parent.as_ref());
                }
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

/// Drag and drop to open (APP-5): files dropped on a window open as Open does, each in
/// its own window, or brought forward if already open. GTK hands over a file list from
/// both Wayland and X11 drags (it reads `text/uri-list` too). The windows open after the
/// drop is done: opening one closes the start window, which may be the drop's target.
fn accept_file_drops(app: &gtk::Application, window: &gtk::ApplicationWindow) {
    let target = gtk::DropTarget::new(
        gtk::gdk::FileList::static_type(),
        gtk::gdk::DragAction::COPY,
    );
    target.connect_drop({
        let (app, window) = (app.clone(), window.downgrade());
        move |_, value, _, _| {
            let Ok(files) = value.get::<gtk::gdk::FileList>() else {
                return false;
            };
            let files = files.files();
            if files.is_empty() {
                return false;
            }
            let (app, window) = (app.clone(), window.clone());
            glib::idle_add_local_once(move || {
                let parent = window.upgrade().map(|w| w.upcast::<gtk::Window>());
                for file in files {
                    match file.path() {
                        Some(path) => {
                            open_window(&app, &path, parent.as_ref());
                        }
                        None => alert(
                            parent.as_ref(),
                            &format!("Cannot open {}", file.uri()),
                            "Only local files can be opened.",
                        ),
                    }
                }
            });
            true
        }
    });
    window.add_controller(target);
}

/// Show `path` in a window: its existing one, or a new one. The start window closes once
/// a file is open. A file that can't be opened leaves the recent files. Whether it opened.
fn open_window(app: &gtk::Application, path: &Path, parent: Option<&gtk::Window>) -> bool {
    if let Some(window) = window_for(path) {
        window.present();
        remember(path);
        return true;
    }
    match open_session(path) {
        Ok((session, table)) => {
            remember(path);
            build_window(app, &session, table, None);
            close_start_window(app);
            true
        }
        Err(e) => {
            let _ = recent::Recent::load().remove(path);
            alert(
                parent,
                &format!("Cannot open {}", path.display()),
                &e.to_string(),
            );
            false
        }
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
    /// The running index build; cancelled when a newer reading replaces it.
    indexing: RefCell<Option<jobs::Job<Result<(), IndexError>>>>,
    /// The file's size and modification time when last read (APP-7): a crash journal
    /// records them, and its edits only go back onto the same bytes.
    identity: Cell<Option<(u64, u64)>>,
    /// A crashed session's edits to put back once the file is indexed (APP-7).
    restore: RefCell<Option<crash::Pending>>,
    /// The reading (delimiter, encoding) the malformed-file check last ran on (APP-8).
    checked: Cell<Option<(u8, Encoding)>>,
}

impl Session {
    fn new(path: Option<PathBuf>) -> Self {
        Self {
            path: RefCell::new(path),
            choice: Cell::new(DelimiterChoice::Auto),
            encoding: Cell::new(None),
            header: Cell::new(None),
            indexing: RefCell::new(None),
            identity: Cell::new(None),
            restore: RefCell::new(None),
            checked: Cell::new(None),
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
        self.identity.set(recovery::identity(&path));
        self.apply_header(&mut table);
        self.index(&table);
        Ok(table)
    }

    /// The user's header choice wins over detection on a fresh reading of the file.
    fn apply_header(&self, table: &mut CsvTable) {
        if let Some(on) = self.header.get() {
            table.set_header(on);
        }
    }

    /// Index `table` on the job pool (ENG-8), cancelling any earlier build; rows become
    /// readable as it goes.
    fn index(&self, table: &CsvTable) {
        let index = table.index().clone();
        let job = jobs::spawn(move |cancel| index.build(cancel.flag()));
        if let Some(old) = self.indexing.replace(Some(job)) {
            old.cancel();
        }
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
                    session.index(&table);
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
    let find = FindBar::new(&grid);
    layout.attach(&find.0.root, 0, 2, 2, 1);
    let bar = StatusBar::new();
    layout.attach(&bar.root, 0, 3, 2, 1);

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title(session.name())
        .default_width(1400)
        .default_height(900)
        .child(&layout)
        .build();
    OPEN_WINDOWS.with_borrow_mut(|open| open.push((session.clone(), window.downgrade())));
    // Crash recovery (APP-7), except in benchmarks, which measure the app alone: journal
    // the unsaved edits, and offer back a crashed session's for this file.
    let journal = bench.is_none().then(crash::Journal::new);
    if let Some(j) = &journal {
        window.connect_destroy({
            let j = j.clone();
            move |_| crash::closed(&j)
        });
        let path = session.path.borrow().clone();
        if session.restore.borrow().is_none() {
            if let Some(c) = path.as_deref().and_then(crash::crashed_for) {
                session.restore.replace(Some(crash::Pending {
                    crashed: c,
                    ask: true,
                }));
            }
        }
    }
    accept_file_drops(app, &window);
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
                save_as(&window, &grid, &session, &save, false);
            }
        }
    });
    let header = gtk::HeaderBar::new();
    header.pack_start(&open);
    header.pack_start(&recent_menu());
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

    bar.cancel.connect_clicked({
        let save = save.clone();
        move |_| {
            if let SaveState::Saving(r) = &*save.borrow() {
                r.job.cancel();
            }
        }
    });

    // Ctrl+S saves (asking where, for an untitled table) and Ctrl+Shift+S saves as;
    // Ctrl+Z undoes, Ctrl+Shift+Z and Ctrl+Y redo (CMD-1); Esc cancels a running save
    // (SAVE-3). Navigation keys belong to the grid (GRID-3); the cell editor's entry keeps
    // its own text undo, and the grid lets Esc through when no editor is open.
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed({
        let (window, grid, save, session, find) = (
            window.downgrade(),
            grid.downgrade(),
            save.clone(),
            session.clone(),
            find.clone(),
        );
        move |_, key, _, mods| {
            use gtk::gdk::{Key, ModifierType};
            let (Some(window), Some(grid)) = (window.upgrade(), grid.upgrade()) else {
                return glib::Propagation::Proceed;
            };
            if key == Key::Escape {
                if let SaveState::Saving(r) = &*save.borrow() {
                    r.job.cancel();
                    return glib::Propagation::Stop;
                }
                if grid.cancel_job() {
                    return glib::Propagation::Stop;
                }
            }
            if !mods.contains(ModifierType::CONTROL_MASK) {
                return glib::Propagation::Proceed;
            }
            let shift = mods.contains(ModifierType::SHIFT_MASK);
            match key {
                Key::f | Key::F if !shift => {
                    grid.finish_editing();
                    find.open(false);
                }
                Key::h | Key::H if !shift => {
                    grid.finish_editing();
                    find.open(true);
                }
                Key::s | Key::S => {
                    grid.finish_editing(); // save what was typed, as Calc does
                    if shift || session.path.borrow().is_none() {
                        save_as(&window, &grid, &session, &save, false);
                    } else {
                        start_save(&grid, &session, &save, None, false);
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

    // Closing with unsaved edits asks first (APP-9): Save, Don't Save, or Cancel. Closing
    // while a save runs waits for it and closes once it succeeds; a failed or cancelled
    // save keeps the window and its edits.
    let discard = Rc::new(Cell::new(false));
    window.connect_close_request({
        let (grid, session, save) = (grid.downgrade(), session.clone(), save.clone());
        move |window| {
            let Some(grid) = grid.upgrade() else {
                return glib::Propagation::Proceed;
            };
            grid.finish_editing(); // what was typed counts as an edit
            if discard.get() {
                return glib::Propagation::Proceed;
            }
            if let SaveState::Saving(r) = &mut *save.borrow_mut() {
                r.close_after = true;
                return glib::Propagation::Stop;
            }
            let edits = grid.edit_count();
            if edits == 0 {
                return glib::Propagation::Proceed;
            }
            let dialog = gtk::AlertDialog::builder()
                .message(format!("Save changes to “{}”?", session.name()))
                .detail(format!(
                    "{} unsaved edit{} will be lost if you close without saving.",
                    status::group_digits(edits as u64),
                    if edits == 1 { "" } else { "s" }
                ))
                .buttons(["Cancel", "Don't Save", "Save"])
                .cancel_button(0)
                .default_button(2)
                .modal(true)
                .build();
            let (window, session, save, discard) = (
                window.clone(),
                session.clone(),
                save.clone(),
                discard.clone(),
            );
            glib::spawn_future_local(async move {
                match dialog.choose_future(Some(&window)).await {
                    Ok(1) => {
                        discard.set(true);
                        window.close();
                    }
                    Ok(2) if session.path.borrow().is_none() => {
                        save_as(&window, &grid, &session, &save, true)
                    }
                    Ok(2) => start_save(&grid, &session, &save, None, true),
                    _ => {}
                }
            });
            glib::Propagation::Stop
        }
    });

    // Status: follow the indexer (grow the scroll range) and keep the status bar and title
    // current. Runs once now, so the bar is filled from the first frame, then every 100 ms.
    let started = Instant::now();
    let (mut was_complete, mut generation) = (false, 0);
    let status_bench = matches!(bench, Some(Bench::Status));
    let mut tick = {
        let (grid, window, session, app, save, find, journal) = (
            grid.downgrade(),
            window.downgrade(),
            session.clone(),
            app.clone(),
            save.clone(),
            find.clone(),
            journal.clone(),
        );
        move || {
            let (Some(grid), Some(window)) = (grid.upgrade(), window.upgrade()) else {
                return glib::ControlFlow::Break;
            };
            // While indexing, grow the scroll range; also once more for a table swapped in
            // since the last tick (a save or delimiter change), which may have finished
            // indexing in between.
            if grid.generation() != generation {
                generation = grid.generation();
                was_complete = false;
            }
            let complete = grid.is_complete();
            if !complete || !was_complete {
                grid.update_adjustments();
            }
            if complete && !was_complete {
                grid.infer_types();
                grid.apply_pending_filters(); // the filters a save had on (FILT-3)
            }
            if let Some(j) = &journal {
                crash::journal_tick(&grid, &session, j);
                crash::pending_tick(&window, &grid, &session);
            }
            if session.restore.borrow().is_none() {
                malformed::tick(&window, &grid, &session);
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
            find.tick(&grid);
            let title = status::title(&session.name(), grid.edit_count());
            if window.title().as_deref() != Some(title.as_str()) {
                window.set_title(Some(&title));
            }
            if status_bench {
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
        Some(Bench::Save { edits }) => bench_save(app, &grid, session, &save, &vadj, edits),
        Some(Bench::Find { text }) => bench_find(app, &grid, &find, text.into()),
        Some(Bench::Status) | None => {}
    }
}

/// Find benchmark (SRCH-2). Once the file is indexed, opens the find bar and types TEXT
/// one character every 200 ms, as a quick typist would, and prints a line per event with
/// milliseconds since the first keystroke:
/// - `FIND <ms> key <cancelled 0|1> <text>`: the text changed; whether that cancelled a
///   running search;
/// - `FIND <ms> start <text>`: a search started;
/// - `FIND <ms> done <hit:N/TOTAL | nothing | cancelled | notready> <text>`: it came back.
///
/// Quits once the search for the whole TEXT is done.
fn bench_find(app: &gtk::Application, grid: &GridView, find: &FindBar, text: Rc<str>) {
    let t0 = Rc::new(Cell::new(None::<Instant>));
    let ms = {
        let t0 = t0.clone();
        move || t0.get().map_or(0.0, |t| t.elapsed().as_secs_f64() * 1e3)
    };
    let (app, full) = (app.clone(), text.clone());
    find.0
        .trace
        .replace(Some(Box::new(move |event| match event {
            FindEvent::Key { text, cancelled } => {
                println!("FIND {:.1} key {} {text}", ms(), u8::from(cancelled))
            }
            FindEvent::Start(text) => println!("FIND {:.1} start {text}", ms()),
            FindEvent::Done(done, found) => {
                use grid_view::Found;
                let result = match found {
                    Found::Hit { ordinal, total } => format!("hit:{ordinal}/{total}"),
                    Found::Nothing => "nothing".into(),
                    Found::Cancelled => "cancelled".into(),
                    Found::NotReady => "notready".into(),
                };
                println!("FIND {:.1} done {result} {done}", ms());
                if done == &*full && !matches!(found, Found::Cancelled) {
                    app.quit();
                }
            }
        })));
    let (grid, find) = (grid.clone(), find.clone());
    let mut typed = 0;
    glib::timeout_add_local(Duration::from_millis(200), move || {
        if !grid.is_complete() {
            return glib::ControlFlow::Continue;
        }
        if typed == 0 {
            find.open(false);
            t0.set(Some(Instant::now()));
        }
        // One character at the end, as a keystroke does (`set_text` would empty the
        // field first: two changes).
        let ch = text
            .chars()
            .nth(typed)
            .map(String::from)
            .unwrap_or_default();
        typed += 1;
        let mut end = -1;
        find.0.entry.insert_text(&ch, &mut end);
        if typed >= text.chars().count() {
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
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

/// A save running on the job pool (SAVE-3, ENG-8).
struct Running {
    job: jobs::Job<SaveResult>,
    target: Option<Target>,
    progress: Arc<SaveProgress>,
    /// Bytes the save is expected to write, for the progress percentage.
    estimate: u64,
    /// Close the window once this save succeeds (APP-9).
    close_after: bool,
}

enum SaveState {
    Idle,
    Saving(Running),
    Saved(Duration),
    Cancelled,
    Failed(String),
}

/// What a save job returns: its stats and how long it took, or why it failed.
type SaveResult = Result<(SaveStats, Duration), file_format::SaveError>;

/// Ctrl+S: write the table to its own file on the job pool (save is atomic, SAVE-1); with
/// a `target`, to another file, delimiter, or encoding (Save As, SAVE-2). Editing is paused
/// until it finishes, so no edit can be lost between snapshot and reopen. With
/// `close_after`, the window closes once the save succeeds.
fn start_save(
    grid: &GridView,
    session: &Rc<Session>,
    save: &Rc<RefCell<SaveState>>,
    target: Option<Target>,
    close_after: bool,
) {
    if matches!(*save.borrow(), SaveState::Saving(..))
        || grid.busy()
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
    let progress = Arc::new(SaveProgress::default());
    let estimate = job.estimated_bytes();
    let running = jobs::spawn({
        let progress = progress.clone();
        move |cancel| {
            let t = Instant::now();
            file_format::save_csv(&path, &job, &progress, cancel.flag()).map(|s| (s, t.elapsed()))
        }
    });
    *save.borrow_mut() = SaveState::Saving(Running {
        job: running.clone(),
        target,
        progress,
        estimate,
        close_after,
    });
    let (grid, session, save) = (grid.downgrade(), session.clone(), save.clone());
    glib::spawn_future_local(async move {
        let result = running.finished().await;
        let Some(grid) = grid.upgrade() else { return };
        if finish_save(&grid, &session, &save, result) {
            if let Some(window) = grid.root().and_downcast::<gtk::Window>() {
                window.close();
            }
        }
    });
}

/// When a save has finished: reopen the saved file (its edits are now on disk; after a
/// Save As, the new file with its delimiter and encoding) or report the error with the
/// edits still in memory. A cancelled save changes nothing. Returns whether the save
/// succeeded and asked for the window to close.
fn finish_save(
    grid: &GridView,
    session: &Session,
    save: &RefCell<SaveState>,
    result: Result<SaveResult, jobs::Stopped>,
) -> bool {
    let (target, close_after) = match &mut *save.borrow_mut() {
        SaveState::Saving(r) => (r.target.take(), r.close_after),
        _ => return false,
    };
    grid.set_saving(false);
    let result = match result {
        Ok(Err(file_format::SaveError::Cancelled)) | Err(jobs::Stopped::Cancelled) => {
            *save.borrow_mut() = SaveState::Cancelled;
            return false;
        }
        Ok(done) => done.map_err(|e| e.to_string()),
        Err(jobs::Stopped::Panicked) => Err("the save stopped unexpectedly".to_owned()),
    };
    if let (Ok(_), Some(t)) = (&result, target) {
        remember(&t.path);
        session.path.replace(Some(t.path));
        session.encoding.set(Some(t.encoding));
        session.choice.set(DelimiterChoice::Fixed(t.delimiter));
    }
    let next = match result
        .and_then(|done| session.open().map(|t| (done, t)).map_err(|e| e.to_string()))
    {
        Ok(((stats, took), table)) => {
            // The reopened file has the same columns: the types the user set (TYPE-2) and
            // the filters (FILT-3) come back, types first since filters read them.
            let (types, filters) = (grid.type_overrides(), grid.filters());
            grid.replace_table(table);
            grid.restore_type_overrides(types);
            grid.restore_filters(filters);
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
    let close = close_after && matches!(next, SaveState::Saved(_));
    *save.borrow_mut() = next;
    close
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
    close_after: bool,
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
            choose_save_file(
                &window,
                &grid,
                &session,
                &save,
                delimiter,
                encoding,
                close_after,
            );
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
    close_after: bool,
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
        start_save(&grid, &session, &save, Some(target), close_after);
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

/// The find bar (SRCH-1–3), over the status bar. Ctrl+F opens it on the text field;
/// typing searches as you type from the cursor, its own cell first (each keystroke cancels
/// the running search); Enter or Ctrl+G goes to the next match, Shift+Enter or
/// Ctrl+Shift+G to the previous one; Esc closes it. Searches the whole file or the
/// cursor's column, with or without matching case. The note counts the matches as they
/// stream in, then says which one the cursor is on ("12 of 2,725,768").
///
/// Ctrl+H (or the replace toggle) shows the replace row: Replace (or Enter there)
/// replaces the text in the match under the cursor and goes to the next; Replace All
/// changes every match shown as one undo step.
#[derive(Clone)]
struct FindBar(Rc<FindParts>);

struct FindParts {
    root: gtk::Revealer,
    entry: gtk::SearchEntry,
    case: gtk::CheckButton,
    scope: gtk::DropDown,
    note: gtk::Label,
    replacing: gtk::ToggleButton,
    replace_row: gtk::Box,
    with: gtk::Entry,
    grid: glib::WeakRef<GridView>,
    /// Hears what the bar does (`--bench-find`).
    trace: RefCell<Option<FindTrace>>,
}

type FindTrace = Box<dyn Fn(FindEvent)>;

/// What the find bar did, for `--bench-find`.
enum FindEvent<'a> {
    /// The text changed; whether that cancelled a running search.
    Key {
        text: &'a str,
        cancelled: bool,
    },
    Start(&'a str),
    Done(&'a str, grid_view::Found),
}

impl FindBar {
    fn new(grid: &GridView) -> Self {
        let entry = gtk::SearchEntry::builder()
            .placeholder_text("Find")
            .hexpand(true)
            .build();
        let button = |icon: &str, tip: &str| {
            let b = gtk::Button::from_icon_name(icon);
            b.add_css_class("flat");
            b.set_tooltip_text(Some(tip));
            b
        };
        let previous = button("go-up-symbolic", "Previous match (Shift+Enter)");
        let next = button("go-down-symbolic", "Next match (Enter)");
        let case = gtk::CheckButton::with_label("Match case");
        let scope = gtk::DropDown::from_strings(&["Whole file", "Current column"]);
        let note = gtk::Label::new(None);
        note.add_css_class("dim-label");
        // Room for "2,725,768 of 2,725,768", so the field doesn't jump as the count grows.
        note.set_width_chars(22);
        note.set_xalign(1.0);
        let close = button("window-close-symbolic", "Close (Esc)");
        let replacing = gtk::ToggleButton::builder()
            .icon_name("edit-find-replace-symbolic")
            .tooltip_text("Replace (Ctrl+H)")
            .css_classes(["flat"])
            .build();
        let bar_row = || {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row.set_margin_start(6);
            row.set_margin_end(6);
            row.set_margin_top(4);
            row.set_margin_bottom(4);
            row
        };
        let row = bar_row();
        for w in [
            entry.upcast_ref::<gtk::Widget>(),
            previous.upcast_ref(),
            next.upcast_ref(),
            replacing.upcast_ref(),
            case.upcast_ref(),
            scope.upcast_ref(),
            note.upcast_ref(),
            close.upcast_ref(),
        ] {
            row.append(w);
        }
        let with = gtk::Entry::builder()
            .placeholder_text("Replace with")
            .hexpand(true)
            .build();
        let replace = gtk::Button::with_label("Replace");
        let replace_all = gtk::Button::with_label("Replace All");
        let replace_row = bar_row();
        replace_row.set_margin_top(0);
        replace_row.append(&with);
        replace_row.append(&replace);
        replace_row.append(&replace_all);
        replace_row.set_visible(false);
        let rows = gtk::Box::new(gtk::Orientation::Vertical, 0);
        rows.append(&row);
        rows.append(&replace_row);
        let root = gtk::Revealer::builder()
            .child(&rows)
            .transition_type(gtk::RevealerTransitionType::SlideUp)
            .build();
        let bar = Self(Rc::new(FindParts {
            root,
            entry,
            case,
            scope,
            note,
            replacing,
            replace_row,
            with,
            grid: grid.downgrade(),
            trace: RefCell::new(None),
        }));
        let p = &bar.0;
        // A keystroke stops the search for the text before it at once; the search for the
        // new text starts once typing pauses (`search-changed`).
        p.entry.connect_changed({
            let bar = bar.clone();
            move |entry| {
                let cancelled = bar.0.grid.upgrade().is_some_and(|g| g.cancel_search());
                let text = entry.text();
                bar.trace(FindEvent::Key {
                    text: &text,
                    cancelled,
                });
                if text.is_empty() {
                    bar.0.note.set_text("");
                }
            }
        });
        p.entry.connect_search_changed({
            let bar = bar.clone();
            move |_| bar.run(Step::Here)
        });
        p.entry.connect_activate({
            let bar = bar.clone();
            move |_| bar.run(Step::Next)
        });
        p.entry.connect_next_match({
            let bar = bar.clone();
            move |_| bar.run(Step::Next)
        });
        p.entry.connect_previous_match({
            let bar = bar.clone();
            move |_| bar.run(Step::Previous)
        });
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed({
            let bar = bar.clone();
            move |_, key, _, mods| {
                use gtk::gdk::{Key, ModifierType};
                if matches!(key, Key::Return | Key::KP_Enter)
                    && mods.contains(ModifierType::SHIFT_MASK)
                {
                    bar.run(Step::Previous);
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            }
        });
        p.entry.add_controller(keys);
        previous.connect_clicked({
            let bar = bar.clone();
            move |_| bar.run(Step::Previous)
        });
        next.connect_clicked({
            let bar = bar.clone();
            move |_| bar.run(Step::Next)
        });
        p.case.connect_toggled({
            let bar = bar.clone();
            move |_| bar.run(Step::Here)
        });
        p.scope.connect_selected_notify({
            let bar = bar.clone();
            move |_| bar.run(Step::Here)
        });
        p.entry.connect_stop_search({
            let bar = bar.clone();
            move |_| bar.close()
        });
        close.connect_clicked({
            let bar = bar.clone();
            move |_| bar.close()
        });
        p.replacing.connect_toggled({
            let bar = bar.clone();
            move |t| bar.0.replace_row.set_visible(t.is_active())
        });
        p.with.connect_activate({
            let bar = bar.clone();
            move |_| bar.replace_one()
        });
        replace.connect_clicked({
            let bar = bar.clone();
            move |_| bar.replace_one()
        });
        replace_all.connect_clicked({
            let bar = bar.clone();
            move |_| bar.replace_all()
        });
        // Esc anywhere in the bar closes it: the replace field, a button, the toggle.
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed({
            let bar = bar.clone();
            move |_, key, _, _| {
                if key == gtk::gdk::Key::Escape {
                    bar.close();
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            }
        });
        p.root.add_controller(keys);
        bar
    }

    fn trace(&self, event: FindEvent) {
        if let Some(t) = self.0.trace.borrow().as_ref() {
            t(event);
        }
    }

    /// Search for the bar's text: from the cursor's cell (`Here`), or the next or previous
    /// match.
    fn run(&self, step: Step) {
        let p = &self.0;
        let Some(grid) = p.grid.upgrade() else {
            return;
        };
        let text = p.entry.text().to_string();
        if text.is_empty() {
            grid.cancel_search();
            p.note.set_text("");
            return;
        }
        self.trace(FindEvent::Start(&text));
        let bar = self.clone();
        grid.find(
            &text.clone(),
            p.case.is_active(),
            p.scope.selected() == 1,
            step,
            move |found| {
                bar.show(found);
                bar.trace(FindEvent::Done(&text, found));
            },
        );
    }

    /// Say how a find went.
    fn show(&self, found: grid_view::Found) {
        use grid_view::Found;
        let note = match found {
            Found::Hit { ordinal, total } => FindNote::Hit { ordinal, total },
            Found::Nothing => FindNote::Nothing,
            Found::NotReady => FindNote::NotReady,
            Found::Cancelled => return,
        };
        self.0.note.set_text(&status::find_note(note));
    }

    /// Replace in the match under the cursor and go to the next (or first go to a match).
    fn replace_one(&self) {
        let p = &self.0;
        let (Some(grid), text) = (p.grid.upgrade(), p.entry.text()) else {
            return;
        };
        if text.is_empty() {
            return;
        }
        let bar = self.clone();
        grid.replace_one(
            &text,
            p.case.is_active(),
            p.scope.selected() == 1,
            &p.with.text(),
            move |found| bar.show(found),
        );
    }

    /// Replace every match shown, as one undo step.
    fn replace_all(&self) {
        let p = &self.0;
        let (Some(grid), text) = (p.grid.upgrade(), p.entry.text()) else {
            return;
        };
        if text.is_empty() {
            return;
        }
        let bar = self.clone();
        grid.replace_all(
            &text,
            p.case.is_active(),
            p.scope.selected() == 1,
            &p.with.text(),
            move |done| {
                use grid_view::Replaced;
                let note = match done {
                    Replaced::Cells(cells) => FindNote::Replaced { cells },
                    Replaced::TooMany(cells) => FindNote::TooMany { cells },
                    Replaced::Nothing => FindNote::Nothing,
                    Replaced::NotReady => FindNote::NotReady,
                    Replaced::Cancelled => return,
                };
                bar.0.note.set_text(&status::find_note(note));
            },
        );
    }

    /// The status tick: the count so far while matches are counted.
    fn tick(&self, grid: &GridView) {
        if let Some(found) = grid.search_found() {
            self.0
                .note
                .set_text(&status::find_note(FindNote::Counting { found }));
        }
    }

    /// Show the bar on the find field, with the replace row (Ctrl+H) or as it was
    /// (Ctrl+F).
    fn open(&self, replace: bool) {
        let p = &self.0;
        p.root.set_reveal_child(true);
        if replace {
            p.replacing.set_active(true);
        }
        p.entry.grab_focus();
        p.entry.select_region(0, -1);
    }

    fn close(&self) {
        self.0.root.set_reveal_child(false);
        if let Some(g) = self.0.grid.upgrade() {
            g.cancel_job();
            g.grab_focus();
        }
    }
}

/// The bar under the grid (APP-2): rows and indexing progress on the left, save state (with
/// a Cancel button while saving, SAVE-3) and the file's format on the right.
struct StatusBar {
    root: gtk::Box,
    rows: gtk::Label,
    save: gtk::Label,
    cancel: gtk::Button,
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
        let cancel = gtk::Button::with_label("Cancel");
        cancel.add_css_class("flat");
        cancel.set_tooltip_text(Some("Cancel the save (Esc)"));
        cancel.set_focus_on_click(false); // keep the keyboard in the grid
        cancel.set_visible(false);
        root.append(&rows);
        root.append(&save);
        root.append(&cancel);
        root.append(&format);
        Self {
            root,
            rows,
            save,
            cancel,
            format,
        }
    }

    /// Show the grid's current state; returns what is shown.
    fn update(&self, grid: &GridView, choice: DelimiterChoice, save: &SaveState) -> status::Status {
        let shown = status::status(&status::Facts {
            rows: grid.row_count(),
            of: grid.unfiltered_row_count(),
            indexing: grid.index_progress(),
            delimiter: grid.delimiter(),
            detected: choice == DelimiterChoice::Auto,
            encoding: grid.encoding(),
            edits: grid.edit_count(),
            job: grid.job_progress(),
            save: match save {
                SaveState::Idle => status::SaveView::Idle,
                SaveState::Saving(r) if r.progress.is_checking() => status::SaveView::Checking,
                SaveState::Saving(r) => status::SaveView::Saving(
                    (r.progress.written() as f64 / r.estimate.max(1) as f64).min(0.99),
                ),
                SaveState::Saved(took) => status::SaveView::Saved(*took),
                SaveState::Cancelled => status::SaveView::Cancelled,
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
        self.cancel
            .set_visible(matches!(save, SaveState::Saving(_)));
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

/// Save driver for the SAVE-3 acceptance test. Once indexed, pastes `edits` cells spread
/// down the file, saves, and while the save runs scrolls 37 px a frame, records the bytes
/// written each frame, and measures how late a 10 ms main-loop timer fires. Prints
/// `SAVE {json}` once the save is done and quits.
fn bench_save(
    app: &gtk::Application,
    grid: &GridView,
    session: &Rc<Session>,
    save: &Rc<RefCell<SaveState>>,
    vadj: &gtk::Adjustment,
    edits: u32,
) {
    let Some(clock) = grid.frame_clock() else {
        return;
    };
    #[derive(Default)]
    struct Rec {
        saving: bool,
        paint_start: Option<Instant>,
        ends: Vec<Instant>,
        cpu_ms: Vec<f64>,
        written: Vec<u64>,
        last_timer: Option<Instant>,
        late_ms: Vec<f64>,
    }
    let rec = Rc::new(RefCell::new(Rec::default()));
    clock.connect_before_paint({
        let rec = rec.clone();
        move |_| rec.borrow_mut().paint_start = Some(Instant::now())
    });
    clock.connect_after_paint({
        let rec = rec.clone();
        move |_| {
            let mut r = rec.borrow_mut();
            if let (true, Some(t0)) = (r.saving, r.paint_start) {
                r.ends.push(Instant::now());
                r.cpu_ms.push(t0.elapsed().as_secs_f64() * 1e3);
            }
        }
    });
    glib::timeout_add_local(Duration::from_millis(10), {
        let rec = rec.clone();
        move || {
            let mut r = rec.borrow_mut();
            let now = Instant::now();
            if let (true, Some(last)) = (r.saving, r.last_timer) {
                let late = (now - last).as_secs_f64() * 1e3 - 10.0;
                r.late_ms.push(late.max(0.0));
            }
            r.last_timer = Some(now);
            glib::ControlFlow::Continue
        }
    });

    // The edits are pasted in one frame and the save starts in the next, so the paste work
    // is not counted against the save; measuring starts with the save.
    let rows = Cell::new(None);
    let (app, session, save, vadj) = (app.clone(), session.clone(), save.clone(), vadj.clone());
    grid.add_tick_callback(move |grid, _| {
        let Some(total) = rows.get() else {
            if !grid.is_complete() {
                return glib::ControlFlow::Continue;
            }
            rows.set(Some(grid.row_count()));
            let step = grid.row_count() / u64::from(edits);
            for i in 0..edits {
                let cell = grid::Cell {
                    row: u64::from(i) * step,
                    col: i % 8,
                };
                grid.paste_at(cell, "edited");
            }
            return glib::ControlFlow::Continue;
        };
        if !rec.borrow().saving {
            let mut r = rec.borrow_mut();
            r.saving = true;
            r.last_timer = None;
            drop(r);
            start_save(grid, &session, &save, None, false);
        }
        let line = match &*save.borrow() {
            SaveState::Saving(s) => {
                rec.borrow_mut().written.push(s.progress.written());
                vadj.set_value(vadj.value() + 37.0);
                return glib::ControlFlow::Continue;
            }
            SaveState::Saved(took) => {
                let mut r = rec.borrow_mut();
                r.saving = false;
                let mut written = r.written.clone();
                written.retain(|&w| w > 0);
                written.dedup();
                format!(
                    r#"{{"rows":{},"edits":{edits},"save_ms":{},"progress_samples":{},"loop_late_ms":{},{}}}"#,
                    total,
                    took.as_millis(),
                    written.len(),
                    summary(&mut r.late_ms),
                    frame_stats(grid, &r.ends, &r.cpu_ms),
                )
            }
            SaveState::Failed(e) => format!(r#"{{"error":{e:?}}}"#),
            SaveState::Cancelled => r#"{"error":"the save was cancelled"}"#.to_owned(),
            SaveState::Idle => r#"{"error":"the save did not start"}"#.to_owned(),
        };
        println!("SAVE {line}");
        app.quit();
        glib::ControlFlow::Break
    });
}

/// `{"p50":..,"p99":..,"max":..}` of `xs` (sorted in place).
fn summary(xs: &mut [f64]) -> String {
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
}

/// JSON fields for frames finished at `ends` taking `cpu_ms` each: the display's refresh
/// rate, frame count and rate, frames that missed it, and interval and CPU percentiles.
fn frame_stats(grid: &GridView, ends: &[Instant], cpu_ms: &[f64]) -> String {
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
    format!(
        r#""refresh_hz":{refresh:.1},"frames":{},"mean_fps":{mean_fps:.1},"missed_frames":{missed},"interval_ms":{},"frame_cpu_ms":{}"#,
        intervals.len(),
        summary(&mut intervals),
        summary(&mut cpu),
    )
}

fn print_report(
    ends: &[Instant],
    cpu_ms: &[f64],
    peak_cache: usize,
    grid: &GridView,
    vadj: &gtk::Adjustment,
) {
    println!(
        r#"{{"rows":{},"window":[{},{}],{},"grid_cache_peak_bytes":{peak_cache},"rss_anon_kib":{},"end_row":{}}}"#,
        grid.row_count(),
        grid.width(),
        grid.height(),
        frame_stats(grid, ends, cpu_ms),
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
