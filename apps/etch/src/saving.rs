//! Saving (ETCH-7): File → Save (Ctrl+S) writes the image over its file, in the format it
//! was opened in; Save As… (Ctrl+Shift+S) writes it as another file, PNG, JPEG, or BMP by
//! the name's extension, which becomes the window's file; Export… (Ctrl+Shift+E) writes a
//! copy the same way while the window keeps its own file and its unsaved state. An image
//! never saved asks where on Save (Untitled.png in the Pictures folder, at first).
//!
//! The image is copied on the UI thread when a save begins (a memory copy, quick even at
//! 8K); encoding and writing run off it, atomically (`image_io::save` through
//! `suite_file::save_atomic`), so editing goes on meanwhile and a failed save leaves the
//! old file whole. Writing transparent pixels as JPEG or BMP, which can't hold them, asks
//! first: they become white.
//!
//! The window's title starts with "• " while the image differs from its file (by history
//! position: undoing back to the saved image clears it), and closing it then asks whether
//! to save.

use crate::canvas::Canvas;
use gtk::prelude::*;
use gtk::{gio, glib};
use image_io::Format;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

/// What a write is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Save or Save As: the file written becomes the window's.
    Save,
    /// Export: a copy; the window keeps its file.
    Export,
}

impl Kind {
    fn verb(self) -> &'static str {
        match self {
            Kind::Save => "Save",
            Kind::Export => "Export",
        }
    }
}

/// What runs once a save has written the file.
type Then = Box<dyn FnOnce()>;
/// A save asked for while another runs.
type Waiting = Box<dyn FnOnce(&Rc<Saver>)>;

/// A window's image file: where it is, its format, and whether it is saved.
pub struct Saver {
    window: glib::WeakRef<gtk::ApplicationWindow>,
    canvas: glib::WeakRef<Canvas>,
    /// `None` for an image not saved yet: Save asks where (Save As).
    path: RefCell<Option<PathBuf>>,
    /// What Save writes: the format the file was read or last saved in.
    format: Cell<Format>,
    /// The name of an image not saved yet: its window's first title ("Untitled 2").
    untitled: String,
    /// The canvas's position when the file was read or last saved; `None` never matches
    /// (saved in the middle of a stroke).
    saved: Cell<Option<u64>>,
    saving: Cell<bool>,
    /// Saves asked for while one runs, in order: each starts when the one before ends.
    waiting: RefCell<VecDeque<Waiting>>,
    /// Closing goes ahead without asking (the user chose).
    closing: Cell<bool>,
}

impl Saver {
    /// The saver of `window`, whose canvas shows the image of `file` (its path and format;
    /// `None`: a new image). Adds the window's `save`, `save-as`, and `export` actions,
    /// marks the title while there are changes, and asks before a window with changes
    /// closes.
    pub fn attach(
        window: &gtk::ApplicationWindow,
        canvas: &Canvas,
        file: Option<(PathBuf, Format)>,
    ) {
        let (path, format) = match file {
            Some((path, format)) => (Some(path), format),
            None => (None, Format::Png),
        };
        let saver = Rc::new(Saver {
            window: window.downgrade(),
            canvas: canvas.downgrade(),
            path: RefCell::new(path),
            format: Cell::new(format),
            untitled: window.title().map(String::from).unwrap_or_default(),
            saved: Cell::new(canvas.position()),
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
            action("save-as", |s| s.choose(Kind::Save, None)),
            action("export", |s| s.choose(Kind::Export, None)),
        ]);
        canvas.connect_changed({
            let saver = saver.clone();
            move |_| saver.show_title()
        });
        window.connect_close_request(move |_| {
            // Text being typed and floating pixels go on the image first: closing never
            // drops them unasked.
            if let Some(canvas) = saver.canvas.upgrade() {
                canvas.commit();
            }
            if saver.modified() && !saver.closing.get() {
                saver.ask_before_closing();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
    }

    /// The file's name, or the Untitled name of an image not saved yet.
    fn name(&self) -> String {
        self.path.borrow().as_deref().map_or_else(
            || self.untitled.clone(),
            |p| suite_file::document_name(p).into_owned(),
        )
    }

    /// The image differs from its file.
    fn modified(&self) -> bool {
        let position = self.canvas.upgrade().and_then(|c| c.position());
        match (self.saved.get(), position) {
            (Some(saved), Some(now)) => saved != now,
            _ => true,
        }
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
        if window.title().as_deref() != Some(title.as_str()) {
            window.set_title(Some(&title));
        }
    }

    /// Ctrl+S: save over the file. `then` runs once the file is saved.
    fn save(self: &Rc<Self>, then: Option<Then>) {
        if self.saving.get() {
            // Saved once the running save ends; a save already waiting covers a bare one.
            let mut waiting = self.waiting.borrow_mut();
            if then.is_some() || waiting.is_empty() {
                waiting.push_back(Box::new(move |s| s.save(then)));
            }
            return;
        }
        let Some(dest) = self.path.borrow().clone() else {
            // A new image: where it goes is asked first.
            self.choose(Kind::Save, then);
            return;
        };
        self.write(dest, self.format.get(), Kind::Save, then);
    }

    /// Save As… and Export…: pick a PNG, JPEG, or BMP file, then write it there. The
    /// extension picks the format; a name without one of Etch's is refused.
    fn choose(self: &Rc<Self>, kind: Kind, then: Option<Then>) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        let mut default = None;
        for (format, name, suffixes) in [
            (Format::Png, "PNG image", &["png"][..]),
            (Format::Jpeg, "JPEG image", &["jpg", "jpeg"]),
            (Format::Bmp, "BMP image", &["bmp"]),
        ] {
            let filter = gtk::FileFilter::new();
            filter.set_name(Some(name));
            for s in suffixes {
                filter.add_suffix(s);
            }
            filters.append(&filter);
            if format == self.format.get() {
                default = Some(filter);
            }
        }
        let current = self.path.borrow().clone();
        let stem = match &current {
            Some(path) => path
                .file_stem()
                .map_or_else(|| self.name(), |s| s.to_string_lossy().into_owned()),
            None => self.untitled.clone(),
        };
        let title = match kind {
            Kind::Save => "Save As",
            Kind::Export => "Export",
        };
        let dialog = gtk::FileDialog::builder()
            .title(title)
            .modal(true)
            .filters(&filters)
            .initial_name(format!("{stem}.{}", self.format.get().extension()))
            .build();
        dialog.set_default_filter(default.as_ref());
        let folder = match &current {
            Some(path) => path.parent().map(Path::to_owned),
            None => glib::user_special_dir(glib::UserDirectory::Pictures).filter(|d| d.is_dir()),
        };
        if let Some(dir) = folder {
            dialog.set_initial_folder(Some(&gio::File::for_path(dir)));
        }
        let saver = self.clone();
        glib::spawn_future_local(async move {
            let file = match dialog.save_future(Some(&window)).await {
                Ok(file) => file,
                Err(e) if e.matches(gtk::DialogError::Dismissed) => return,
                Err(e) => {
                    alert(&window, "Cannot show the file dialog", &e.to_string()).await;
                    return;
                }
            };
            let Some(dest) = file.path() else {
                let message = format!("Cannot {} {}", kind.verb().to_lowercase(), file.uri());
                alert(&window, &message, "Only local files can be saved.").await;
                return;
            };
            match Format::for_path(&dest) {
                Ok(format) => saver.write(dest, format, kind, then),
                Err(why) => {
                    let name = suite_file::document_name(&dest);
                    let message = format!("Cannot {} “{name}”", kind.verb().to_lowercase());
                    alert(&window, &message, &why.to_string()).await;
                }
            }
        });
    }

    /// Write the image at `dest` as `format`: copied now, encoded and written in the
    /// background. A JPEG or BMP of an image with transparency asks first.
    fn write(self: &Rc<Self>, dest: PathBuf, format: Format, kind: Kind, then: Option<Then>) {
        let Some(canvas) = self.canvas.upgrade() else {
            return;
        };
        if self.saving.replace(true) {
            self.waiting
                .borrow_mut()
                .push_back(Box::new(move |s| s.write(dest, format, kind, then)));
            return;
        }
        // Text being typed and floating pixels are part of what's saved.
        canvas.commit();
        let started = Instant::now();
        let (image, position) = canvas.copy_image();
        let copy_ms = started.elapsed().as_secs_f64() * 1e3;
        let image = Arc::new(image);
        let saver = self.clone();
        glib::spawn_future_local(async move {
            let name = suite_file::document_name(&dest).into_owned();
            let window = saver.window.upgrade();
            let mut go = true;
            if !format.keeps_alpha() {
                let pixels = image.clone();
                let clear = gio::spawn_blocking(move || image_io::has_transparency(&pixels))
                    .await
                    .unwrap_or(true);
                if clear {
                    go = confirm_flattening(window.as_ref(), &name, format, kind).await;
                }
            }
            let result = if go {
                let (dest, image) = (dest.clone(), image.clone());
                gio::spawn_blocking(move || {
                    let t = Instant::now();
                    image_io::save(&dest, &image, format)
                        .map(|()| t.elapsed())
                        .map_err(|e| reason(&e))
                })
                .await
                .unwrap_or_else(|_| Err("Saving failed unexpectedly.".to_owned()))
                .map(Some)
            } else {
                Ok(None)
            };
            drop(image);
            saver.saving.set(false);
            match result {
                Ok(Some(took)) => {
                    tracing::info!(
                        "saved {} as {}: copied in {copy_ms:.1} ms, written in {:.0} ms",
                        dest.display(),
                        format.name(),
                        took.as_secs_f64() * 1e3
                    );
                    if kind == Kind::Save {
                        saver.saved(dest, format, position);
                    }
                    if let Some(then) = then {
                        then();
                    }
                }
                Ok(None) => {} // the user kept the transparency: nothing written
                Err(why) => {
                    tracing::warn!("cannot save {}: {why}", dest.display());
                    if let Some(window) = &window {
                        let message = format!("Cannot {} “{name}”", kind.verb().to_lowercase());
                        alert(window, &message, &why).await;
                    }
                }
            }
            let next = saver.waiting.borrow_mut().pop_front();
            if let Some(next) = next {
                next(&saver);
            }
        });
    }

    /// The image at `position` is now the file at `dest`, in `format`.
    fn saved(&self, dest: PathBuf, format: Format, position: Option<u64>) {
        if self.path.borrow().as_deref() != Some(dest.as_path()) {
            if let Some(window) = self.window.upgrade() {
                crate::window::moved(&window, &dest);
            }
        }
        self.path.replace(Some(dest));
        self.format.set(format);
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
            let close: Then = Box::new({
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

/// Why a save failed, in words for the alert.
fn reason(e: &suite_file::SaveError) -> String {
    use std::io::ErrorKind;
    match e {
        suite_file::SaveError::Io(e) => match e.kind() {
            ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem => {
                "You don't have permission to write there.".to_owned()
            }
            ErrorKind::NotFound => "The folder doesn't exist.".to_owned(),
            ErrorKind::StorageFull => "There's no room left on the disk.".to_owned(),
            _ => format!("It can't be written: {e}."),
        },
        other => format!("It can't be written: {other}."),
    }
}

/// Writing an image with transparency as `format` (JPEG or BMP), which has none: whether
/// to go on, the transparent pixels becoming white.
async fn confirm_flattening(
    parent: Option<&gtk::ApplicationWindow>,
    name: &str,
    format: Format,
    kind: Kind,
) -> bool {
    let verb = kind.verb();
    gtk::AlertDialog::builder()
        .message(format!("{verb} “{name}” without transparency?"))
        .detail(format!(
            "{} images can't be transparent: the image's transparent and see-through pixels \
             become white in the file.",
            format.name()
        ))
        .buttons(["Cancel", verb])
        .default_button(1)
        .cancel_button(0)
        .modal(true)
        .build()
        .choose_future(parent)
        .await
        .is_ok_and(|choice| choice == 1)
}

async fn alert(parent: &gtk::ApplicationWindow, message: &str, detail: &str) {
    let _ = gtk::AlertDialog::builder()
        .message(message)
        .detail(detail)
        .buttons(["Close"])
        .modal(true)
        .build()
        .choose_future(Some(parent))
        .await;
}
