//! Saving (SLATE-7): Ctrl+S saves over the document's file, Ctrl+Shift+S saves it as a
//! .md or .txt file through the file dialog. A plain-text file is written back in its
//! encoding, line endings, and final newline; a Markdown file keeps the bytes of every block
//! not edited ([`document_model::write_markdown`]). The bytes are made and written off the
//! UI thread, atomically (`suite_file::save_atomic`): the old file stays whole until the new
//! one is complete and verified. Editing goes on meanwhile; the document saved is the one
//! shown when the save began.
//!
//! The window's title starts with "• " while the document differs from its file, and
//! closing it then asks whether to save. Word documents can't be saved yet (SLATE-8): Save
//! says so and offers Save As.

use crate::view::Page;
use document_model::{
    write_markdown, write_text, Document, Encoding, LineEnding, MarkdownSource, TextInfo,
};
use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::AtomicBool;

/// How a save writes the document.
#[derive(Debug, Clone)]
pub enum Target {
    Text(TextInfo),
    /// With the file's source, when it was read or last saved as Markdown.
    Markdown(TextInfo, Option<MarkdownSource>),
    /// Not saved yet: Save As writes it as Markdown or plain text.
    Docx,
}

impl Target {
    fn info(&self) -> Option<TextInfo> {
        match self {
            Target::Text(info) | Target::Markdown(info, _) => Some(*info),
            Target::Docx => None,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Target::Text(_) => "Plain text",
            Target::Markdown(..) => "Markdown",
            Target::Docx => "Word document",
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

/// A window's document file: where it is, how it is written, and whether it is saved.
pub struct Saver {
    window: glib::WeakRef<gtk::ApplicationWindow>,
    page: glib::WeakRef<Page>,
    status: glib::WeakRef<gtk::Label>,
    path: RefCell<PathBuf>,
    target: RefCell<Target>,
    /// The page's position ([`Page::position`]) when the file was last read or saved.
    saved: Cell<u64>,
    saving: Cell<bool>,
    /// Saves asked for while one runs, in order: each starts when the one before ends.
    waiting: RefCell<VecDeque<Waiting>>,
    /// Closing goes ahead without asking (the user chose).
    closing: Cell<bool>,
}

impl Saver {
    /// The saver of `window`, whose page shows `path` read as `target`. Adds the window's
    /// `save` and `save-as` actions and asks before a window with changes closes.
    pub fn attach(
        window: &gtk::ApplicationWindow,
        page: &Page,
        status: &gtk::Label,
        path: PathBuf,
        target: Target,
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
            move || saver.show_title()
        });
        window.connect_close_request(move |_| {
            if saver.modified() && !saver.closing.get() {
                saver.ask_before_closing();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
    }

    fn name(&self) -> String {
        suite_file::document_name(&self.path.borrow()).into_owned()
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

    /// Ctrl+S: save over the file; a Word document is offered Save As. `then` runs once
    /// the file is saved.
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
        if let Target::Docx = target {
            self.cannot_save_docx(then);
            return;
        }
        let dest = self.path.borrow().clone();
        self.save_to(dest, target, false, then);
    }

    fn cannot_save_docx(self: &Rc<Self>, then: Option<Box<dyn FnOnce()>>) {
        let dialog = gtk::AlertDialog::builder()
            .message("Slate can’t save Word documents yet")
            .detail("Save As saves this document as Markdown (.md) or plain text (.txt).")
            .buttons(["Cancel", "Save As…"])
            .default_button(1)
            .cancel_button(0)
            .modal(true)
            .build();
        let saver = self.clone();
        glib::spawn_future_local(async move {
            let parent = saver.window.upgrade();
            if let Ok(1) = dialog.choose_future(parent.as_ref()).await {
                saver.save_as(then);
            }
        });
    }

    /// Ctrl+Shift+S: pick a .md or .txt file, then save there.
    pub fn save_as(self: &Rc<Self>, then: Option<Box<dyn FnOnce()>>) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let markdown = filter("Markdown", &["*.md", "*.markdown"]);
        let text = filter("Plain text", &["*.txt", "*.text"]);
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&markdown);
        filters.append(&text);
        let current = self.path.borrow().clone();
        let (default, initial) = match &*self.target.borrow() {
            Target::Text(_) => (&text, self.name()),
            Target::Markdown(..) => (&markdown, self.name()),
            Target::Docx => (&markdown, with_extension(&current, "md")),
        };
        let dialog = gtk::FileDialog::builder()
            .title("Save As")
            .modal(true)
            .filters(&filters)
            .default_filter(default)
            .initial_name(initial)
            .build();
        if let Some(dir) = current.parent() {
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
    /// line endings when it was plain text or Markdown already.
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
            _ => Err("Slate saves Markdown (.md) and plain text (.txt) files."),
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
                move || write(&doc, &dest, target, save_as)
            });
            let result = job
                .await
                .unwrap_or_else(|_| Err("Saving failed unexpectedly.".to_owned()));
            saver.saving.set(false);
            let name = suite_file::document_name(&dest).into_owned();
            match result {
                Ok(target) => {
                    tracing::info!("saved {}", dest.display());
                    saver.saved(dest, target, position);
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
        let moved = *self.path.borrow() != dest;
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
        self.path.replace(dest);
        self.target.replace(target);
        self.saved.set(position);
        self.show_title();
    }

    /// Closing with changes: save them, discard them, or keep the window.
    fn ask_before_closing(self: &Rc<Self>) {
        let docx = matches!(*self.target.borrow(), Target::Docx);
        let dialog = gtk::AlertDialog::builder()
            .message(format!("Save changes to “{}” before closing?", self.name()))
            .detail("Changes not saved are lost.")
            .buttons(["Cancel", "Discard", if docx { "Save As…" } else { "Save" }])
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
                Ok(2) if docx => saver.save_as(Some(close)),
                Ok(2) => saver.save(Some(close)),
                _ => {}
            }
        });
    }
}

/// Make the bytes of `doc` as `target` and write them at `dest` atomically; the target to
/// save with next time (a Markdown file's new source).
fn write(doc: &Document, dest: &Path, target: Target, save_as: bool) -> Result<Target, String> {
    let made = |target: &Target| match target {
        Target::Text(info) => write_text(&doc.blocks, info).map(|bytes| (bytes, None)),
        Target::Markdown(info, source) => write_markdown(&doc.blocks, info, source.as_ref())
            .map(|(bytes, source)| (bytes, Some(source))),
        Target::Docx => unreachable!("Word documents aren't saved"),
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

/// `path`'s file name with its extension replaced by `ext`.
fn with_extension(path: &Path, ext: &str) -> String {
    suite_file::document_name(&path.with_extension(ext)).into_owned()
}
