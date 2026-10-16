//! Saving (FRAME-8, ADR 0006): Ctrl+S (the header bar's Save button on Linux, File → Save
//! on macOS) saves over the deck's file, Ctrl+Shift+S saves it as another file through the
//! file dialog; a new deck, never saved, saves as Save As does. Either patches the package
//! the deck was read from (`pptx::save`), atomically and off the UI thread; the window
//! waits meanwhile, so the deck saved is the deck shown. A save that fails leaves the file
//! as it was and says why.

use crate::window::{self, DeckWindow};
use gtk::prelude::*;
use gtk::{gio, glib};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::AtomicBool;

/// Ctrl+S: save over the deck's file; a new deck asks where, as Save As does.
pub fn save(view: &Rc<DeckWindow>) {
    match view.deck.path() {
        Some(path) => save_to(view, path),
        None => save_as(view),
    }
}

/// Ctrl+Shift+S: pick a file, then save there. A new deck suggests its first slide's
/// title, or the window's title ("Untitled"), as the name.
pub fn save_as(view: &Rc<DeckWindow>) {
    let Some(parent) = view.window() else {
        return;
    };
    let (filters, pptx) = window::file_filters();
    let current = view.deck.path();
    let name = match &current {
        Some(path) => file_name(path),
        None => format!("{}.pptx", suggested_name(view, &parent)),
    };
    let dialog = gtk::FileDialog::builder()
        .title("Save As")
        .modal(true)
        .filters(&filters)
        .default_filter(&pptx)
        .initial_name(name)
        .build();
    if let Some(dir) = current.as_deref().and_then(Path::parent) {
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

/// A new deck's name: its first slide's title on one line, without characters a file
/// name can't have, or else the window's title.
fn suggested_name(view: &DeckWindow, window: &gtk::ApplicationWindow) -> String {
    let deck = view.deck.presentation();
    let title = deck.slides.first().and_then(|slide| {
        let shape = &slide.shapes[deck_edit::title_of(slide)?];
        let paragraphs = &shape.text.as_ref()?.paragraphs;
        let text = paragraphs.iter().map(|p| p.text()).collect::<Vec<_>>();
        let name = text
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .replace('/', "-");
        (!name.is_empty()).then_some(name)
    });
    title.unwrap_or_else(|| window.title().map_or_else(|| "Untitled".into(), Into::into))
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
                if Some(&dest) != view.deck.path().as_ref() {
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
