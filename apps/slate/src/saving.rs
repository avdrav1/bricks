//! Saving (SLATE-7, SLATE-8): Ctrl+S saves over the document's file, Ctrl+Shift+S saves it
//! as another file through the file dialog: .md or .txt, or .docx for a Word document. A
//! plain-text file is written back in its encoding, line endings, and final newline; a
//! Markdown file keeps the bytes of every block not edited
//! ([`document_model::write_markdown`]); a Word document is saved by patching the package it
//! was read from ([`docx::save`]): only what was edited changes. The bytes are made and
//! written off the UI thread, atomically (`suite_file::save_atomic`): the old file stays
//! whole until the new one is complete and verified. Editing goes on meanwhile; the
//! document saved is the one shown when the save began.
//!
//! The window's title starts with "• " while the document differs from its file, and
//! closing it then asks whether to save. Meanwhile its unsaved edits are journaled for
//! crash recovery ([`crate::recovery`]).

use crate::recovery::{self, Header, WRITE_AFTER};
use crate::view::Page;
use document_model::{
    write_markdown, write_text, Document, Encoding, LineEnding, MarkdownSource, TextInfo,
};
use docx::DocxSource;
use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

/// How a save writes the document.
#[derive(Debug, Clone)]
pub enum Target {
    Text(TextInfo),
    /// With the file's source, when it was read or last saved as Markdown.
    Markdown(TextInfo, Option<MarkdownSource>),
    /// A Word document, saved by patching the package it was read from or last saved as.
    Docx(Docx),
}

/// A Word document's source map and a private copy of its package.
#[derive(Debug, Clone)]
pub struct Docx {
    pub source: DocxSource,
    pub original: Arc<Original>,
}

/// A private copy of the package a Word document was read from or last saved as, in Slate's
/// cache folder (`$XDG_CACHE_HOME/slate`, on macOS `~/Library/Caches/dev.cronkite.Slate`;
/// [`suite_platform::dirs`]), removed with it: the next save patches it, so the file may
/// change, move, or be saved over meanwhile. The process holds its owner lock there
/// ([`suite_file::owner`]) before making its first copy, so another Slate's [`sweep`]
/// leaves its copies alone while it runs.
///
/// [`sweep`]: Original::sweep
#[derive(Debug)]
pub struct Original {
    copy: PathBuf,
}

impl Original {
    /// Copy the package at `path`.
    pub fn copy_of(path: &Path) -> std::io::Result<Original> {
        let copy = Original::fresh()?;
        std::fs::copy(path, &copy)?;
        Ok(Original { copy })
    }

    /// A copy of the package `bytes` (the built-in blank document).
    pub fn of_bytes(bytes: &[u8]) -> std::io::Result<Original> {
        let copy = Original::fresh()?;
        std::fs::write(&copy, bytes)?;
        Ok(Original { copy })
    }

    /// A new copy's path, in a directory that exists.
    fn fresh() -> std::io::Result<PathBuf> {
        Original::fresh_in(&Original::dir())
    }

    /// A new copy's path in `dir`, which is made, with this process's owner lock in it.
    fn fresh_in(dir: &Path) -> std::io::Result<PathBuf> {
        static N: AtomicU64 = AtomicU64::new(0);
        std::fs::create_dir_all(dir)?;
        suite_file::owner::hold(dir)?;
        Ok(dir.join(format!(
            "original-{}-{}.docx",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        )))
    }

    pub fn path(&self) -> &Path {
        &self.copy
    }

    fn dir() -> PathBuf {
        suite_platform::dirs::cache_dir(crate::APP)
    }

    /// Remove the copies left by Slate processes no longer running (killed before they
    /// could remove their own), and their owner locks.
    pub fn sweep() {
        Original::sweep_in(&Original::dir());
    }

    fn sweep_in(dir: &Path) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        // Whether each copy's process still runs, asked once a process.
        let mut owners = std::collections::HashMap::new();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(pid) = name
                .to_str()
                .and_then(|n| n.strip_prefix("original-"))
                .and_then(|n| n.split('-').next())
                .and_then(|pid| pid.parse::<u32>().ok())
            else {
                continue;
            };
            let alive = *owners
                .entry(pid)
                .or_insert_with(|| suite_file::owner::alive(dir, pid));
            if !alive {
                let _ = std::fs::remove_file(entry.path());
            }
        }
        for (pid, alive) in owners {
            if !alive {
                suite_file::owner::release(dir, pid);
            }
        }
    }
}

impl Drop for Original {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.copy);
    }
}

impl Target {
    fn info(&self) -> Option<TextInfo> {
        match self {
            Target::Text(info) | Target::Markdown(info, _) => Some(*info),
            Target::Docx(_) => None,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Target::Text(_) => "Plain text",
            Target::Markdown(..) => "Markdown",
            Target::Docx(_) => "Word document",
        }
    }
}

/// What a new file gets: UTF-8, `\n`, ending with a line break.
const NEW_FILE: TextInfo = TextInfo {
    encoding: Encoding::Utf8,
    line_ending: LineEnding::Lf,
    final_newline: true,
};

/// A save asked for while another runs.
type Waiting = Box<dyn FnOnce(&Rc<Saver>)>;

/// The name of a new document until it is saved.
pub const UNTITLED: &str = "Untitled";

/// A window's document file: where it is, how it is written, and whether it is saved.
pub struct Saver {
    window: glib::WeakRef<gtk::ApplicationWindow>,
    page: glib::WeakRef<Page>,
    status: glib::WeakRef<gtk::Label>,
    /// `None` for a new document not saved yet: Save asks where (Save As).
    path: RefCell<Option<PathBuf>>,
    target: RefCell<Target>,
    /// The page's position ([`Page::position`]) when the file was last read or saved.
    saved: Cell<u64>,
    saving: Cell<bool>,
    /// Saves asked for while one runs, in order: each starts when the one before ends.
    waiting: RefCell<VecDeque<Waiting>>,
    /// Closing goes ahead without asking (the user chose).
    closing: Cell<bool>,
    /// The file's size and modification time when it was last read or saved.
    identity: Cell<Option<(u64, u64)>>,
    /// This window's crash recovery journal.
    journal: PathBuf,
    journaling: RefCell<Journaling>,
}

/// Where the crash recovery journal stands.
#[derive(Default)]
struct Journaling {
    /// The journal file is there.
    exists: bool,
    /// The page's position the journal holds.
    written: Option<u64>,
    /// The write that is due.
    due: Option<glib::SourceId>,
    writing: bool,
    /// The window closed: no more journal.
    closed: bool,
}

impl Saver {
    /// The saver of `window`, whose page shows `path` read as `target` (`None`: a new
    /// document), the file's `identity` as read. Adds the window's `save` and `save-as`
    /// actions, asks before a window with changes closes, and journals unsaved edits.
    pub fn attach(
        window: &gtk::ApplicationWindow,
        page: &Page,
        status: &gtk::Label,
        path: Option<PathBuf>,
        target: Target,
        identity: Option<(u64, u64)>,
    ) {
        let saver = Rc::new(Saver {
            window: window.downgrade(),
            page: page.downgrade(),
            status: status.downgrade(),
            path: RefCell::new(path),
            target: RefCell::new(target),
            saved: Cell::new(page.position()),
            saving: Cell::new(false),
            waiting: RefCell::new(VecDeque::new()),
            closing: Cell::new(false),
            identity: Cell::new(identity),
            journal: recovery::store().new_journal(),
            journaling: RefCell::default(),
        });
        // The window's actions and handlers own the saver; it holds the window weakly.
        let action = |name: &str, run: fn(&Rc<Saver>)| {
            let saver = saver.clone();
            gio::ActionEntry::builder(name)
                .activate(move |_: &gtk::ApplicationWindow, _, _| run(&saver))
                .build()
        };
        window.add_action_entries([
            action("save", |s| s.save(None)),
            action("save-as", |s| s.save_as(None)),
        ]);
        page.connect_edited({
            let saver = saver.clone();
            move || {
                saver.show_title();
                saver.journal_changed();
            }
        });
        window.connect_close_request(move |_| {
            if saver.modified() && !saver.closing.get() {
                saver.ask_before_closing();
                glib::Propagation::Stop
            } else {
                // Saved, or the user chose to discard: nothing to recover.
                saver.close_journal();
                glib::Propagation::Proceed
            }
        });
    }

    /// The document changed or was saved: journal it within [`WRITE_AFTER`] while it has
    /// unsaved edits the journal doesn't hold, else remove the journal.
    fn journal_changed(self: &Rc<Self>) {
        let position = self.page.upgrade().map(|p| p.position());
        let modified = self.modified();
        let mut j = self.journaling.borrow_mut();
        if j.closed || j.writing {
            return; // the write in progress looks again when it ends
        }
        if !modified {
            if let Some(due) = j.due.take() {
                due.remove();
            }
            if j.exists {
                recovery::store().remove(&self.journal);
                (j.exists, j.written) = (false, None);
            }
        } else if j.due.is_none() && j.written != position {
            let saver = self.clone();
            j.due = Some(glib::timeout_add_local_once(WRITE_AFTER, move || {
                saver.journaling.borrow_mut().due = None;
                saver.write_journal();
            }));
        }
    }

    /// Write the document to the journal off the UI thread, then look again.
    fn write_journal(self: &Rc<Self>) {
        let Some(page) = self.page.upgrade() else {
            return;
        };
        if !self.modified() || self.journaling.borrow().closed {
            self.journal_changed();
            return;
        }
        let position = page.position();
        let blocks = page.document().blocks;
        let header = Header {
            path: self.path.borrow().clone(),
            file: self.identity.get(),
            saved_ms: recovery::now_ms(),
        };
        let journal = self.journal.clone();
        self.journaling.borrow_mut().writing = true;
        let saver = self.clone();
        glib::spawn_future_local(async move {
            let written = gio::spawn_blocking(move || {
                recovery::write(&recovery::store(), &journal, &header, &blocks)
            })
            .await;
            {
                let mut j = saver.journaling.borrow_mut();
                j.writing = false;
                match written {
                    Ok(Ok(())) => (j.exists, j.written) = (true, Some(position)),
                    Ok(Err(e)) => tracing::warn!("cannot write the recovery journal: {e}"),
                    Err(_) => tracing::warn!("writing the recovery journal failed"),
                }
                if j.closed {
                    // Closed while it was being written.
                    recovery::store().remove(&saver.journal);
                    j.exists = false;
                    return;
                }
            }
            saver.journal_changed();
        });
    }

    /// The window closes with nothing to recover: no journal.
    fn close_journal(&self) {
        let mut j = self.journaling.borrow_mut();
        j.closed = true;
        if let Some(due) = j.due.take() {
            due.remove();
        }
        if j.exists && !j.writing {
            recovery::store().remove(&self.journal);
            j.exists = false;
        }
    }

    /// The file's name, or "Untitled" for a new document.
    fn name(&self) -> String {
        self.path.borrow().as_deref().map_or_else(
            || UNTITLED.to_owned(),
            |p| suite_file::document_name(p).into_owned(),
        )
    }

    /// The document differs from its file.
    pub fn modified(&self) -> bool {
        self.page
            .upgrade()
            .is_some_and(|page| page.position() != self.saved.get())
    }

    /// The window's title: the file's name, after "• " while there are changes to save.
    fn show_title(&self) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let name = self.name();
        let title = if self.modified() {
            format!("• {name}")
        } else {
            name
        };
        window.set_title(Some(&title));
        if let Some(label) = window
            .titlebar()
            .and_downcast::<gtk::HeaderBar>()
            .and_then(|h| h.title_widget())
            .and_downcast::<gtk::Label>()
        {
            label.set_text(&title);
        }
    }

    /// Ctrl+S: save over the file. `then` runs once the file is saved.
    pub fn save(self: &Rc<Self>, then: Option<Box<dyn FnOnce()>>) {
        if self.saving.get() {
            // Saved once the running save ends; a save already waiting covers a bare one.
            let mut waiting = self.waiting.borrow_mut();
            if then.is_some() || waiting.is_empty() {
                waiting.push_back(Box::new(move |s| s.save(then)));
            }
            return;
        }
        let target = self.target.borrow().clone();
        let Some(dest) = self.path.borrow().clone() else {
            // A new document: where it goes is asked first.
            self.save_as(then);
            return;
        };
        self.save_to(dest, target, false, then);
    }

    /// Ctrl+Shift+S: pick a .md or .txt file (or .docx for a Word document), then save there.
    pub fn save_as(self: &Rc<Self>, then: Option<Box<dyn FnOnce()>>) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let markdown = filter("Markdown", &["*.md", "*.markdown"]);
        let text = filter("Plain text", &["*.txt", "*.text"]);
        let word = filter("Word document", &["*.docx"]);
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        let docx = matches!(*self.target.borrow(), Target::Docx(_));
        if docx {
            filters.append(&word);
        }
        filters.append(&markdown);
        filters.append(&text);
        let current = self.path.borrow().clone();
        let default = match &*self.target.borrow() {
            Target::Text(_) => &text,
            Target::Markdown(..) => &markdown,
            Target::Docx(_) => &word,
        };
        let initial = match &*self.path.borrow() {
            Some(_) => self.name(),
            None => format!("{UNTITLED}.docx"),
        };
        let dialog = gtk::FileDialog::builder()
            .title("Save As")
            .modal(true)
            .filters(&filters)
            .default_filter(default)
            .initial_name(initial)
            .build();
        let folder = match &current {
            Some(path) => path.parent().map(Path::to_owned),
            None => glib::user_special_dir(glib::UserDirectory::Documents).filter(|d| d.is_dir()),
        };
        if let Some(dir) = folder {
            dialog.set_initial_folder(Some(&gio::File::for_path(dir)));
        }
        let saver = self.clone();
        glib::spawn_future_local(async move {
            let Ok(file) = dialog.save_future(Some(&window)).await else {
                return; // dismissed
            };
            let Some(dest) = file.path() else {
                return;
            };
            match saver.target_for(&dest) {
                Ok(target) => saver.save_to(dest, target, true, then),
                Err(why) => crate::alert(
                    Some(window.upcast_ref()),
                    &format!("Cannot save “{}”", suite_file::document_name(&dest)),
                    why,
                ),
            }
        });
    }

    /// How a file at `dest` is written, by its extension: the document's own encoding and
    /// line endings when it was plain text or Markdown already; a .docx only for a Word
    /// document, by patching its package.
    fn target_for(&self, dest: &Path) -> Result<Target, &'static str> {
        let ext = dest
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase());
        let current = self.target.borrow();
        let info = current.info().unwrap_or(NEW_FILE);
        match ext.as_deref() {
            Some("md" | "markdown") => {
                let source = match &*current {
                    Target::Markdown(_, source) => source.clone(),
                    _ => None,
                };
                Ok(Target::Markdown(info, source))
            }
            Some("txt" | "text") => Ok(Target::Text(info)),
            Some("docx") => match &*current {
                Target::Docx(_) => Ok(current.clone()),
                _ => Err("Slate saves a Word document (.docx) only when it opened one."),
            },
            _ => Err("Slate saves Markdown (.md), plain text (.txt), and Word (.docx) files."),
        }
    }

    /// Write the document at `dest`, in the background. `save_as`: text its encoding can't
    /// hold is written as UTF-8 instead of refused.
    fn save_to(
        self: &Rc<Self>,
        dest: PathBuf,
        target: Target,
        save_as: bool,
        then: Option<Box<dyn FnOnce()>>,
    ) {
        let Some(page) = self.page.upgrade() else {
            return;
        };
        if self.saving.replace(true) {
            self.waiting
                .borrow_mut()
                .push_back(Box::new(move |s| s.save_to(dest, target, save_as, then)));
            return;
        }
        let doc = page.document();
        let position = page.position();
        let saver = self.clone();
        glib::spawn_future_local(async move {
            let job = gio::spawn_blocking({
                let dest = dest.clone();
                move || {
                    let target = write(&doc, &dest, target, save_as)?;
                    Ok((target, recovery::identity(&dest)))
                }
            });
            let result = job
                .await
                .unwrap_or_else(|_| Err("Saving failed unexpectedly.".to_owned()));
            saver.saving.set(false);
            let name = suite_file::document_name(&dest).into_owned();
            match result {
                Ok((target, identity)) => {
                    tracing::info!("saved {}", dest.display());
                    saver.identity.set(identity);
                    saver.saved(dest, target, position);
                    saver.journal_changed();
                    if let Some(then) = then {
                        then();
                    }
                }
                Err(why) => {
                    tracing::warn!("cannot save {}: {why}", dest.display());
                    let parent = saver.window.upgrade();
                    crate::alert(
                        parent.as_ref().map(|w| w.upcast_ref()),
                        &format!("Cannot save “{name}”"),
                        &why,
                    );
                }
            }
            let next = saver.waiting.borrow_mut().pop_front();
            if let Some(next) = next {
                next(&saver);
            }
        });
    }

    /// The document at `position` is now the file at `dest`, written as `target`.
    fn saved(&self, dest: PathBuf, target: Target, position: u64) {
        let moved = self.path.borrow().as_deref() != Some(dest.as_path());
        if moved {
            if let Some(window) = self.window.upgrade() {
                crate::moved(&window, &crate::canonical(&dest));
            }
            if let Some(page) = self.page.upgrade() {
                let name = suite_file::document_name(&dest).into_owned();
                page.update_property(&[gtk::accessible::Property::Label(&name)]);
            }
        }
        if let Some(status) = self.status.upgrade() {
            let text = status.text();
            let rest = text.split_once(" · ").map_or("", |(_, rest)| rest);
            status.set_text(&format!("{} · {rest}", target.name()));
        }
        self.path.replace(Some(dest));
        self.target.replace(target);
        self.saved.set(position);
        self.show_title();
    }

    /// Closing with changes: save them, discard them, or keep the window.
    fn ask_before_closing(self: &Rc<Self>) {
        let dialog = gtk::AlertDialog::builder()
            .message(format!("Save changes to “{}” before closing?", self.name()))
            .detail("Changes not saved are lost.")
            .buttons(["Cancel", "Discard", "Save"])
            .default_button(2)
            .cancel_button(0)
            .modal(true)
            .build();
        let saver = self.clone();
        glib::spawn_future_local(async move {
            let parent = saver.window.upgrade();
            let choice = dialog.choose_future(parent.as_ref()).await;
            let close: Box<dyn FnOnce()> = Box::new({
                let saver = saver.clone();
                move || {
                    saver.closing.set(true);
                    if let Some(window) = saver.window.upgrade() {
                        window.close();
                    }
                }
            });
            match choice {
                Ok(1) => close(),
                Ok(2) => saver.save(Some(close)),
                _ => {}
            }
        });
    }
}

/// Make the bytes of `doc` as `target` and write them at `dest` atomically; the target to
/// save with next time (a Markdown file's new source; a Word document's new source and a
/// copy of the package written).
fn write(doc: &Document, dest: &Path, target: Target, save_as: bool) -> Result<Target, String> {
    if let Target::Docx(word) = target {
        let cancel = AtomicBool::new(false);
        let source = docx::save(
            dest,
            word.original.path(),
            &doc.blocks,
            &word.source,
            &cancel,
        )
        .map_err(|e| e.to_string())?;
        return Ok(match Original::copy_of(dest) {
            Ok(original) => Target::Docx(Docx {
                source,
                original: Arc::new(original),
            }),
            Err(e) => {
                // The previous copy and its source still make the same saves.
                tracing::warn!("cannot keep a copy of {}: {e}", dest.display());
                Target::Docx(word)
            }
        });
    }
    let made = |target: &Target| match target {
        Target::Text(info) => write_text(&doc.blocks, info).map(|bytes| (bytes, None)),
        Target::Markdown(info, source) => {
            write_markdown(&doc.blocks, &doc.styles, info, source.as_ref())
                .map(|(bytes, source)| (bytes, Some(source)))
        }
        Target::Docx(_) => unreachable!("Word documents are saved above"),
    };
    let (bytes, source, target) = match made(&target) {
        Ok((bytes, source)) => (bytes, source, target),
        Err(_) if save_as => {
            // Save As is the way out of an encoding that can't hold the text: UTF-8.
            let utf8 = TextInfo {
                encoding: Encoding::Utf8,
                ..target.info().unwrap_or(NEW_FILE)
            };
            let target = match target {
                Target::Markdown(_, source) => Target::Markdown(utf8, source),
                _ => Target::Text(utf8),
            };
            let (bytes, source) = made(&target).map_err(|e| e.to_string())?;
            (bytes, source, target)
        }
        Err(e) => {
            return Err(format!(
                "{e}, the encoding this file was read in. Save As saves it as UTF-8."
            ))
        }
    };
    let cancel = AtomicBool::new(false);
    suite_file::save_atomic(
        dest,
        &cancel,
        |w| w.write_all(&bytes),
        |tmp| match std::fs::read(tmp) {
            Ok(written) if written == bytes => Ok(()),
            Ok(_) => Err("the file written differs from the document".to_owned()),
            Err(e) => Err(e.to_string()),
        },
    )
    .map_err(|e| e.to_string())?;
    Ok(match target {
        Target::Markdown(info, _) => Target::Markdown(info, source),
        other => other,
    })
}

fn filter(name: &str, patterns: &[&str]) -> gtk::FileFilter {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some(name));
    for p in patterns {
        filter.add_pattern(p);
    }
    filter
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::process::{Command, Stdio};
    use suite_file::owner;

    /// Run in a child process by the test below: a Slate holding a package copy in
    /// `$SLATE_SWEEP_DIR` until its stdin closes.
    #[test]
    fn hold_a_copy_for_parent() {
        let Some(dir) = std::env::var_os("SLATE_SWEEP_DIR") else {
            return;
        };
        let copy = Original::fresh_in(Path::new(&dir)).unwrap();
        std::fs::write(&copy, b"package").unwrap();
        println!("copy\t{}", copy.display());
        let _ = std::io::stdin().read_line(&mut String::new());
    }

    /// A second Slate's sweep keeps a running Slate's copy, whatever its pid, and removes a
    /// gone one's: one that was killed, and one whose lock nobody holds.
    #[test]
    fn a_sweep_keeps_the_copies_of_running_slates() {
        let dir = std::env::temp_dir().join(format!("slate-sweep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut other = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "saving::tests::hold_a_copy_for_parent",
                "--nocapture",
            ])
            .env("SLATE_SWEEP_DIR", &dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let copy = std::io::BufReader::new(other.stdout.take().unwrap())
            .lines()
            .find_map(|l| l.ok()?.strip_prefix("copy\t").map(PathBuf::from))
            .expect("the other Slate's copy");
        // A copy left by a Slate that's gone: its lock file is there, but nobody holds it.
        let stale = dir.join("original-999999-0.docx");
        std::fs::write(&stale, b"old").unwrap();
        std::fs::File::create(owner::lock_path(&dir, 999_999)).unwrap();

        Original::sweep_in(&dir);
        assert!(copy.exists(), "a running Slate's copy stays");
        assert!(!stale.exists(), "a gone Slate's copy goes");
        assert!(!owner::lock_path(&dir, 999_999).exists(), "with its lock");

        other.kill().unwrap();
        other.wait().unwrap();
        assert!(copy.exists(), "a killed Slate leaves its copy behind");
        Original::sweep_in(&dir);
        assert!(!copy.exists(), "which the next sweep removes");
        assert!(!owner::lock_path(&dir, other.id()).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
