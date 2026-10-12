//! Saving (FRAME-8, ADR 0006): Ctrl+S (the header bar's Save button) saves over the deck's
//! file, Ctrl+Shift+S saves it as another file through the file dialog. Either patches
//! the package the deck was read from (`pptx::save`), atomically and off the UI thread;
//! the window waits meanwhile, so the deck saved is the deck shown. A save that fails
//! leaves the file as it was and says why.

use crate::window::{self, DeckWindow};
use gtk::prelude::*;
use gtk::{gio, glib};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::AtomicBool;

/// Ctrl+S: save over the deck's file.
pub fn save(view: &Rc<DeckWindow>) {
    save_to(view, view.deck.path());
}

/// Ctrl+Shift+S: pick a file, then save there.
pub fn save_as(view: &Rc<DeckWindow>) {
    let Some(parent) = view.window() else {
        return;
    };
    let (filters, pptx) = window::file_filters();
    let current = view.deck.path();
    let dialog = gtk::FileDialog::builder()
        .title("Save As")
        .modal(true)
        .filters(&filters)
        .default_filter(&pptx)
        .initial_name(file_name(&current))
        .build();
    if let Some(dir) = current.parent() {
        dialog.set_initial_folder(Some(&gio::File::for_path(dir)));
    }
    let view = view.clone();
    glib::spawn_future_local(async move {
        match dialog.save_future(Some(&parent)).await {
            Ok(file) => match file.path() {
                Some(path) => save_to(&view, path),
                None => window::alert(
                    Some(parent.upcast_ref()),
                    "Cannot save there",
                    "Only local files can be saved.",
                ),
            },
            Err(e) if e.matches(gtk::DialogError::Dismissed) => {}
            Err(e) => window::alert(
                Some(parent.upcast_ref()),
                "Cannot show the file dialog",
                &e.to_string(),
            ),
        }
    });
}

fn file_name(path: &Path) -> String {
    suite_file::document_name(path).into_owned()
}

fn save_to(view: &Rc<DeckWindow>, dest: PathBuf) {
    let Some(window) = view.window() else {
        return;
    };
    let Some(page) = window.child() else {
        return;
    };
    if !page.is_sensitive() {
        return; // a save is running
    }
    view.seal();
    page.set_sensitive(false);
    let name = file_name(&dest);
    view.set_status(&format!("Saving “{name}”…"));
    let deck = view.deck.presentation().clone();
    let (read, original) = view.deck.origin();
    let view = view.clone();
    glib::spawn_future_local(async move {
        let to = dest.clone();
        let saved = gio::spawn_blocking(move || {
            pptx::save(&to, original.path(), &read, &deck, &AtomicBool::new(false))
                .map_err(|e| e.to_string())
        })
        .await
        .unwrap_or_else(|_| Err("saving failed unexpectedly".to_owned()));
        page.set_sensitive(true);
        match saved {
            Ok(()) => {
                if dest != view.deck.path() {
                    view.moved_to(&dest);
                }
                view.set_status(&format!("Saved “{name}”"));
            }
            Err(e) => {
                eprintln!("frame: cannot save {}: {e}", dest.display());
                view.show_slide_status();
                window::alert(
                    Some(window.upcast_ref()),
                    &format!("Cannot save “{name}”"),
                    &format!("The file is as it was. {e}"),
                );
            }
        }
    });
}
