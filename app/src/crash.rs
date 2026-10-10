//! Crash recovery in the app (APP-7). Each window journals its unsaved edits 5 seconds
//! after the oldest unwritten change (so at most every 5 s while editing goes on), on the
//! job pool. Its journal goes once nothing is unsaved (saved, or all undone) and when the
//! window closes, so a journal still there after the process ended means a crash.
//!
//! After a crash the start window lists the journals (Restore / Discard), and opening such
//! a file any other way asks "Restore unsaved edits?" once it is indexed. A restore reads
//! the file as it was read then (delimiter, encoding, header row), refuses if the file
//! changed since (size or modification time), and puts the edits back as one undo step.

use crate::grid_view::GridView;
use crate::recovery::{now_ms, Crashed, Store};
use crate::{alert, build_window, canonical, close_start_window, remember, status, Session};
use data_model::{read_journal, CsvTable, DelimiterChoice, JournalHeader};
use gtk::{glib, prelude::*};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// How long the oldest unwritten change waits before the journal is written.
const WRITE_AFTER: Duration = Duration::from_secs(5);

/// A crashed session's edits to put back once the file is indexed: after asking, or
/// straight away when the user chose Restore on the start window.
pub struct Pending {
    pub crashed: Crashed,
    pub ask: bool,
}

/// One window's journal.
pub struct Journal {
    path: PathBuf,
    /// The table revision the journal on disk holds.
    written: Option<u64>,
    /// When the oldest change not in the journal was first seen.
    first: Option<Instant>,
    writing: bool,
    exists: bool,
    closed: bool,
}

impl Journal {
    pub fn new() -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self {
            path: Store::open().new_journal(),
            written: None,
            first: None,
            writing: false,
            exists: false,
            closed: false,
        }))
    }
}

/// The window closed: its edits were saved or deliberately dropped, so its journal goes.
pub fn closed(j: &Rc<RefCell<Journal>>) {
    let mut s = j.borrow_mut();
    s.closed = true;
    if s.exists {
        Store::open().remove(&s.path);
        s.exists = false;
    }
}

/// The status tick: write the journal when due, or remove it when nothing is unsaved.
pub fn journal_tick(grid: &GridView, session: &Session, j: &Rc<RefCell<Journal>>) {
    let mut s = j.borrow_mut();
    if s.writing || s.closed {
        return;
    }
    let changes = grid.edit_count();
    if changes == 0 {
        if s.exists {
            Store::open().remove(&s.path);
            s.exists = false;
        }
        (s.written, s.first) = (None, None);
        return;
    }
    let rev = grid.revision();
    if s.written == Some(rev) {
        s.first = None;
        return;
    }
    if s.first.get_or_insert_with(Instant::now).elapsed() < WRITE_AFTER {
        return;
    }
    let Some(snapshot) = grid.snapshot() else {
        return;
    };
    let (file_size, file_mtime_ns) = session.identity.get().unwrap_or((0, 0));
    let header = JournalHeader {
        path: session.path.borrow().clone(),
        file_size,
        file_mtime_ns,
        delimiter: grid.delimiter(),
        has_header: grid.has_header(),
        encoding: grid.encoding(),
        saved_ms: now_ms(),
        changes: changes as u64,
        rows: grid.row_count(),
        pid: std::process::id(),
    };
    s.writing = true;
    let file = s.path.clone();
    let job =
        jobs::spawn(move |_| Store::open().write(&file, |f| snapshot.write_journal(&header, f)));
    let j = j.clone();
    glib::spawn_future_local(async move {
        let result = job.finished().await;
        let mut s = j.borrow_mut();
        s.writing = false;
        match result {
            Ok(Ok(())) => {
                (s.exists, s.written, s.first) = (true, Some(rev), None);
                if s.closed {
                    Store::open().remove(&s.path); // closed while it was being written
                    s.exists = false;
                }
            }
            Ok(Err(e)) => {
                eprintln!("spreadsheet: cannot write the recovery journal: {e}");
                s.first = Some(Instant::now()); // try again in 5 s
            }
            Err(_) => s.first = Some(Instant::now()),
        }
    });
}

/// A crashed session's journal for `path`, if there is one.
pub fn crashed_for(path: &Path) -> Option<Crashed> {
    let path = canonical(path);
    Store::open()
        .crashed()
        .into_iter()
        .find(|c| c.header.path.as_deref().map(canonical).as_ref() == Some(&path))
}

/// "14:32" today, else "Oct 9, 14:32".
fn when(ms: u64) -> String {
    let at = glib::DateTime::from_unix_local((ms / 1000) as i64);
    let now = glib::DateTime::now_local();
    let format = match (&at, &now) {
        (Ok(a), Ok(n)) if a.ymd() == n.ymd() => "%H:%M",
        _ => "%b %-d, %H:%M",
    };
    at.and_then(|a| a.format(format))
        .map_or_else(|_| "an earlier session".into(), |s| s.to_string())
}

fn changes(n: u64) -> String {
    format!(
        "{} unsaved edit{}",
        status::group_digits(n),
        if n == 1 { "" } else { "s" }
    )
}

fn name(h: &JournalHeader) -> String {
    h.path.as_deref().map_or_else(
        || "an untitled table".to_owned(),
        |p| match p.file_name() {
            Some(n) => format!("“{}”", n.to_string_lossy()),
            None => format!("“{}”", p.display()),
        },
    )
}

/// The status tick, once indexed: deal with a pending restore.
pub fn pending_tick(window: &gtk::ApplicationWindow, grid: &GridView, session: &Rc<Session>) {
    if !grid.is_complete() || grid.busy() || session.restore.borrow().is_none() {
        return;
    }
    let Some(p) = session.restore.take() else {
        return;
    };
    let h = p.crashed.header.clone();
    if h.path.is_some() {
        if session.identity.get() != Some((h.file_size, h.file_mtime_ns)) {
            changed_since(window, p.crashed);
            return;
        }
        // Read the file as it was read when the edits were made.
        if grid.delimiter() != h.delimiter || grid.encoding() != h.encoding {
            session.choice.set(DelimiterChoice::Fixed(h.delimiter));
            session.encoding.set(Some(h.encoding));
            match session.open() {
                Ok(table) => {
                    grid.reset_column_widths();
                    grid.replace_table(table);
                    session.restore.replace(Some(p)); // again once this reading is indexed
                }
                Err(e) => alert(
                    Some(window.upcast_ref()),
                    "Cannot restore unsaved edits",
                    &e.to_string(),
                ),
            }
            return;
        }
    }
    if grid.has_header() != h.has_header {
        session.header.set(Some(h.has_header));
        grid.set_header(h.has_header);
    }
    if !p.ask {
        restore(window, grid, p.crashed);
        return;
    }
    let dialog = gtk::AlertDialog::builder()
        .message("Restore unsaved edits?")
        .detail(format!(
            "Spreadsheet closed unexpectedly at {} while {} had {}.",
            when(h.saved_ms),
            name(&h),
            changes(h.changes)
        ))
        .buttons(["Discard", "Restore"])
        .default_button(1)
        .modal(true)
        .build();
    let (window, grid) = (window.clone(), grid.clone());
    glib::spawn_future_local(async move {
        match dialog.choose_future(Some(&window)).await {
            Ok(1) => restore(&window, &grid, p.crashed),
            Ok(0) => Store::open().remove(&p.crashed.journal),
            _ => {} // dismissed: offered again next time
        }
    });
}

/// The file changed after the crash: its edits can't go onto it.
fn changed_since(window: &gtk::ApplicationWindow, crashed: Crashed) {
    let h = &crashed.header;
    let dialog = gtk::AlertDialog::builder()
        .message(format!("{} changed after the crash", name(h)))
        .detail(format!(
            "Spreadsheet closed unexpectedly at {} with {}, but the file on disk has \
             changed since, so they can't be put back onto it.",
            when(h.saved_ms),
            changes(h.changes)
        ))
        .buttons(["Keep for Later", "Discard"])
        .cancel_button(0)
        .default_button(0)
        .modal(true)
        .build();
    let window = window.clone();
    glib::spawn_future_local(async move {
        if let Ok(1) = dialog.choose_future(Some(&window)).await {
            Store::open().remove(&crashed.journal);
        }
    });
}

/// Read the journal on the job pool, then put its edits back as one undo step.
fn restore(window: &gtk::ApplicationWindow, grid: &GridView, crashed: Crashed) {
    let file = crashed.journal.clone();
    let job = jobs::spawn(move |_| {
        std::fs::File::open(&file)
            .map_err(data_model::JournalError::from)
            .and_then(read_journal)
    });
    let (window, grid) = (window.clone(), grid.clone());
    glib::spawn_future_local(async move {
        let result = match job.finished().await {
            Ok(r) => r.and_then(|(_, state)| grid.restore_edits(state)),
            Err(_) => Err(data_model::JournalError::Corrupt("reading it failed")),
        };
        match result {
            Ok(()) => Store::open().remove(&crashed.journal),
            Err(e) => {
                eprintln!("spreadsheet: cannot restore unsaved edits: {e}");
                alert(
                    Some(window.upcast_ref()),
                    "Cannot restore unsaved edits",
                    &e.to_string(),
                )
            }
        }
    });
}

/// The start window's list of crashed sessions, and its first Restore button (it takes
/// the focus: after a crash, putting the work back is the likely next step).
pub fn start_section(app: &gtk::Application) -> Option<(gtk::Box, gtk::Button)> {
    let crashed = Store::open().crashed();
    if crashed.is_empty() {
        return None;
    }
    let section = gtk::Box::new(gtk::Orientation::Vertical, 6);
    section.set_margin_top(12);
    let title = gtk::Label::new(Some("Unsaved edits from a crash"));
    title.add_css_class("heading");
    section.append(&title);
    let mut first = None;
    for c in crashed {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let h = &c.header;
        let label = match &h.path {
            Some(p) => p
                .file_name()
                .map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into()),
            None => format!("Untitled table ({} rows)", status::group_digits(h.rows)),
        };
        row.append(&gtk::Label::new(Some(&label)));
        let detail = gtk::Label::new(Some(&format!(
            "{} · {}",
            changes(h.changes),
            when(h.saved_ms)
        )));
        detail.add_css_class("dim-label");
        detail.set_hexpand(true);
        detail.set_xalign(0.0);
        row.append(&detail);
        let restore = gtk::Button::with_label("Restore");
        restore.add_css_class("suggested-action");
        let discard = gtk::Button::with_label("Discard");
        discard.add_css_class("flat");
        row.append(&discard);
        row.append(&restore);
        if let Some(p) = &h.path {
            row.set_tooltip_text(Some(&p.display().to_string()));
        }
        restore.connect_clicked({
            let (app, c) = (app.clone(), c.clone());
            move |b| restore_from_start(&app, &c, b.root().and_downcast::<gtk::Window>().as_ref())
        });
        discard.connect_clicked({
            let (journal, row) = (c.journal.clone(), row.downgrade());
            move |_| {
                Store::open().remove(&journal);
                if let Some(r) = row.upgrade() {
                    r.set_visible(false);
                }
            }
        });
        first.get_or_insert(restore);
        section.append(&row);
    }
    Some((section, first?))
}

/// Restore chosen on the start window: open the file as it was read then, or a new
/// untitled table, and put the edits back once it is indexed.
fn restore_from_start(app: &gtk::Application, c: &Crashed, parent: Option<&gtk::Window>) {
    let h = &c.header;
    let session = Rc::new(Session::new(h.path.clone()));
    session.restore.replace(Some(Pending {
        crashed: c.clone(),
        ask: false,
    }));
    let table = match &h.path {
        None => CsvTable::untitled(),
        Some(path) => {
            session.choice.set(DelimiterChoice::Fixed(h.delimiter));
            session.encoding.set(Some(h.encoding));
            session.header.set(Some(h.has_header));
            match session.open() {
                Ok(t) => {
                    remember(path);
                    t
                }
                Err(e) => {
                    let detail = format!("{e}. The unsaved edits are kept for later.");
                    alert(parent, &format!("Cannot open {}", path.display()), &detail);
                    return;
                }
            }
        }
    };
    build_window(app, &session, table, None);
    close_start_window(app);
}
