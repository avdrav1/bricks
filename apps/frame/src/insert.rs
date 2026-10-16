//! Inserting (FRAME-12, spec "Content goes where it belongs"). The toolbar's Insert
//! menu adds a text box (its text selected, so typing replaces it), a shape, a picture
//! (chosen in the file dialog), or a table to the slide shown, and selects it. Pictures
//! also come by drop, image files on the canvas, and by paste (Ctrl+V): a picture fills
//! the selected or first empty picture or content placeholder, else lands centred and
//! scaled to fit. Delete and Backspace delete the selected shape. Each is one step in
//! the deck's undo history.

use crate::editing;
use crate::slide_view::Editing;
use crate::window::{alert, DeckWindow};
use deck_edit::{ShapeEdit, ShapeRef, TextPos, SHAPES};
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use presentation_model::{Image, ImageFormat};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use suite_ui::chrome::Toolbar;

/// The rows and columns of a table Insert adds.
const TABLE: (usize, usize) = (3, 3);
/// What pictures' file names end in: PNG, JPEG, GIF, and BMP, which GDK decodes.
const PICTURES: [&str; 5] = ["png", "jpg", "jpeg", "gif", "bmp"];
const PICTURE_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/gif", "image/bmp"];

/// The slide shown, when there is one.
fn slide(view: &DeckWindow) -> Option<usize> {
    (view.count() > 0).then(|| view.canvas.index())
}

/// Whether `path` names a picture Frame inserts, by its extension.
fn is_picture(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| PICTURES.iter().any(|p| e.eq_ignore_ascii_case(p)))
}

/// Apply `edit` as a step of its own; where the shape it adds is.
fn insert(view: &DeckWindow, edit: Option<ShapeEdit>) -> Option<ShapeRef> {
    let edit = edit?;
    let at = edit.target();
    view.seal();
    view.apply(edit);
    at
}

/// Insert → Text: a text box saying "Text", all of it selected.
fn text(view: &DeckWindow) {
    let Some(slide) = slide(view) else {
        return;
    };
    let edit = ShapeEdit::text_box(&view.deck.presentation(), slide, "Text");
    let Some(at) = insert(view, edit) else {
        return;
    };
    let end = view
        .deck
        .presentation()
        .slides
        .get(at.slide)
        .and_then(|s| s.shapes.get(at.shape))
        .and_then(|s| s.text.as_ref())
        .map(deck_edit::text_end)
        .unwrap_or_default();
    view.im.reset();
    editing::show(
        view,
        Some(Editing {
            shape: at.shape,
            anchor: TextPos::default(),
            caret: end,
            goal_x: None,
        }),
    );
}

/// Insert → Shape: the preset shape `preset`.
fn shape(view: &DeckWindow, preset: &str) {
    if let Some(slide) = slide(view) {
        let edit = ShapeEdit::shape(&view.deck.presentation(), slide, preset);
        insert(view, edit);
    }
}

/// Insert → Table.
fn table(view: &DeckWindow) {
    if let Some(slide) = slide(view) {
        let edit = ShapeEdit::table(&view.deck.presentation(), slide, TABLE.0, TABLE.1);
        insert(view, edit);
    }
}

/// Delete or Backspace: the selected shape goes, when it's one of the slide's own.
pub fn delete(view: &DeckWindow) -> bool {
    let (Some(slide), Some(shape)) = (slide(view), view.canvas.selected()) else {
        return false;
    };
    let edit = ShapeEdit::remove(&view.deck.presentation(), ShapeRef { slide, shape });
    let Some(edit) = edit else {
        return false;
    };
    view.seal();
    view.apply(edit);
    true
}

/// Picture `image`, decoded as `texture`, on the slide shown: into placeholder `into`
/// when that takes a picture, else as [`ShapeEdit::picture`] places it.
fn place(view: &DeckWindow, image: Image, texture: gdk::Texture, into: Option<usize>) {
    let Some(slide) = slide(view) else {
        return;
    };
    let pixels = (
        texture.width().max(1) as u32,
        texture.height().max(1) as u32,
    );
    view.deck.add_texture(&image, texture);
    let edit = ShapeEdit::picture(&view.deck.presentation(), slide, image, pixels, into);
    insert(view, edit);
    to_page(view);
}

/// Read and decode the picture at `path`. Blocking: run it off the UI thread.
fn load(path: &Path) -> Result<(Image, gdk::Texture), String> {
    let bytes: Arc<[u8]> = std::fs::read(path).map_err(|e| e.to_string())?.into();
    let format = ImageFormat::of(&bytes);
    let drawable = matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif | ImageFormat::Bmp
    );
    if !drawable {
        return Err("It isn’t a PNG, JPEG, GIF, or BMP picture.".to_owned());
    }
    let texture = gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes.clone()))
        .map_err(|e| format!("The picture doesn’t decode: {e}"))?;
    Ok((Image { format, bytes }, texture))
}

/// Insert the pictures at `paths` one after another, read off the UI thread; the first
/// into placeholder `into` when it takes one. One that can't be read alerts with its
/// name and the reason.
fn insert_files(view: &Rc<DeckWindow>, paths: Vec<PathBuf>, into: Option<usize>) {
    let view = Rc::downgrade(view);
    glib::spawn_future_local(async move {
        let mut into = into;
        for path in paths {
            let p = path.clone();
            let read = gio::spawn_blocking(move || load(&p)).await;
            let Some(view) = view.upgrade() else {
                return;
            };
            match read {
                Ok(Ok((image, texture))) => place(&view, image, texture, into.take()),
                Ok(Err(why)) => cannot_insert(&view, &path, &why),
                Err(_) => cannot_insert(&view, &path, "Reading it failed unexpectedly."),
            }
        }
    });
}

fn cannot_insert(view: &DeckWindow, path: &Path, why: &str) {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let parent = view.window().map(|w| w.upcast::<gtk::Window>());
    alert(parent.as_ref(), &format!("Cannot insert “{name}”"), why);
}

/// `files`' local paths of pictures, and the other files.
fn split(files: Vec<gio::File>) -> (Vec<PathBuf>, Vec<gio::File>) {
    let (pictures, others): (Vec<_>, Vec<_>) = files
        .into_iter()
        .partition(|f| f.path().is_some_and(|p| is_picture(&p)));
    (pictures.iter().filter_map(|f| f.path()).collect(), others)
}

/// Insert → Picture…: the file dialog, filtered to pictures.
fn choose(view: &Rc<DeckWindow>) {
    let pictures = gtk::FileFilter::new();
    pictures.set_name(Some("Pictures"));
    for t in PICTURE_TYPES {
        pictures.add_mime_type(t);
    }
    for s in PICTURES {
        pictures.add_suffix(s);
    }
    let all = gtk::FileFilter::new();
    all.set_name(Some("All files"));
    all.add_pattern("*");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&pictures);
    filters.append(&all);
    let dialog = gtk::FileDialog::builder()
        .title("Insert Picture")
        .modal(true)
        .filters(&filters)
        .default_filter(&pictures)
        .build();
    let parent = view.window().map(|w| w.upcast::<gtk::Window>());
    let weak = Rc::downgrade(view);
    glib::spawn_future_local(async move {
        let chosen = dialog.open_future(parent.as_ref()).await;
        let Some(view) = weak.upgrade() else {
            return;
        };
        match chosen {
            Ok(file) => match file.path() {
                Some(path) => insert_files(&view, vec![path], view.canvas.selected()),
                None => alert(
                    parent.as_ref(),
                    &format!("Cannot insert {}", file.uri()),
                    "Only local files can be inserted.",
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

/// Ctrl+V: a picture on the clipboard, or picture files, onto the slide shown. While
/// text is edited, the clipboard's text is the text's: `false`, nothing pasted, when
/// there's text or no picture.
pub fn paste(view: &Rc<DeckWindow>) -> bool {
    let clipboard = view.canvas.clipboard();
    let formats = clipboard.formats().union_deserialize_types();
    let texture = formats.contains_type(gdk::Texture::static_type());
    let files = formats.contains_type(gdk::FileList::static_type());
    let text = formats.contains_type(String::static_type());
    if !(texture || files) || (text && view.canvas.editing().is_some()) {
        return false;
    }
    let into = view.canvas.selected();
    let weak = Rc::downgrade(view);
    glib::spawn_future_local(async move {
        if files {
            let read = clipboard
                .read_value_future(gdk::FileList::static_type(), glib::Priority::DEFAULT)
                .await;
            let list = read.ok().and_then(|v| v.get::<gdk::FileList>().ok());
            if let (Some(list), Some(view)) = (list, weak.upgrade()) {
                let (pictures, _) = split(list.files());
                if !pictures.is_empty() {
                    insert_files(&view, pictures, into);
                    return;
                }
            }
            if !texture {
                return;
            }
        }
        if let Ok(Some(t)) = clipboard.read_texture_future().await {
            if let Some(view) = weak.upgrade() {
                let bytes = t.save_to_png_bytes();
                let image = Image {
                    format: ImageFormat::Png,
                    bytes: Arc::from(&bytes[..]),
                };
                place(&view, image, t, into);
            }
        }
    });
    true
}

/// Files dropped on the canvas: pictures go onto the slide shown (into the placeholder
/// under the pointer when it takes one), decks open in their own windows.
fn accept_drops(view: &Rc<DeckWindow>) {
    let target = suite_ui::launch::file_drop_target();
    target.connect_drop({
        let view = Rc::downgrade(view);
        move |_, value, x, y| {
            let (Some(view), Ok(list)) = (view.upgrade(), value.get::<gdk::FileList>()) else {
                return false;
            };
            let (pictures, others) = split(list.files());
            if pictures.is_empty() && others.is_empty() {
                return false;
            }
            if !others.is_empty() {
                view.open_dropped(others);
            }
            if !pictures.is_empty() {
                let into = view.canvas.shape_at(x, y, 0.0);
                insert_files(&view, pictures, into);
            }
            true
        }
    });
    view.canvas.add_controller(target);
}

/// The toolbar's Insert menu, the menu bar's Insert actions (macOS), and the canvas's
/// drops.
pub fn attach(view: &Rc<DeckWindow>, bar: &Toolbar) {
    let button = gtk::MenuButton::new();
    button.set_label("Insert");
    button.set_tooltip_text(Some("Insert a text box, shape, picture, or table"));
    button.update_property(&[gtk::accessible::Property::Label("Insert")]);
    button.set_popover(Some(&menu(view)));
    bar.pack_start(&button);
    if let Some(window) = view.window() {
        let run = |name: &str, run: fn(&Rc<DeckWindow>)| {
            let view = Rc::downgrade(view);
            gio::ActionEntry::builder(name)
                .activate(move |_: &gtk::ApplicationWindow, _, _| {
                    if let Some(view) = view.upgrade() {
                        run(&view);
                        to_page_later(&view);
                    }
                })
                .build()
        };
        // Insert → Shape's items: the preset.
        let shape_entry = {
            let view = Rc::downgrade(view);
            gio::ActionEntry::builder("insert-shape")
                .parameter_type(Some(glib::VariantTy::STRING))
                .activate(move |_: &gtk::ApplicationWindow, _, preset| {
                    let preset = preset.and_then(|p| p.str());
                    let known = SHAPES.iter().find(|(p, _)| Some(*p) == preset);
                    if let (Some(view), Some(&(preset, _))) = (view.upgrade(), known) {
                        shape(&view, preset);
                        to_page_later(&view);
                    }
                })
                .build()
        };
        window.add_action_entries([
            run("insert-text", |v| text(v)),
            shape_entry,
            run("insert-picture", choose),
            run("insert-table", |v| table(v)),
        ]);
    }
    accept_drops(view);
}

/// A flat menu item labelled `label`, its label left-aligned, and `icon` at its end.
fn item(label: &str, icon: Option<&str>) -> gtk::Button {
    let text = gtk::Label::new(Some(label));
    text.set_xalign(0.0);
    text.set_hexpand(true);
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    content.append(&text);
    if let Some(icon) = icon {
        content.append(&gtk::Image::from_icon_name(icon));
    }
    let b = gtk::Button::new();
    b.add_css_class("flat");
    b.set_child(Some(&content));
    b.update_property(&[gtk::accessible::Property::Label(label)]);
    b
}

/// A menu item of `popover` labelled `label` that closes it and runs `run`, then gives
/// the keys back to the canvas.
fn pick(
    view: &Rc<DeckWindow>,
    popover: &gtk::Popover,
    label: &str,
    run: impl Fn(&Rc<DeckWindow>) + 'static,
) -> gtk::Button {
    let b = item(label, None);
    let (view, popover) = (Rc::downgrade(view), popover.downgrade());
    b.connect_clicked(move |_| {
        if let Some(p) = popover.upgrade() {
            p.popdown();
        }
        if let Some(view) = view.upgrade() {
            run(&view);
            to_page_later(&view);
        }
    });
    b
}

/// The Insert menu: Text, Shape (a page of the shapes), Picture…, and Table.
fn menu(view: &Rc<DeckWindow>) -> gtk::Popover {
    let popover = gtk::Popover::new();
    let main = gtk::Box::new(gtk::Orientation::Vertical, 0);
    main.append(&pick(view, &popover, "Text", |v| text(v)));
    let shapes_item = item("Shape", Some("go-next-symbolic"));
    main.append(&shapes_item);
    main.append(&pick(view, &popover, "Picture…", choose));
    main.append(&pick(view, &popover, "Table", |v| table(v)));
    let shapes = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let back = item("Back", Some("go-previous-symbolic"));
    shapes.append(&back);
    shapes.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    for (preset, name) in SHAPES {
        shapes.append(&pick(view, &popover, name, move |v| shape(v, preset)));
    }
    let stack = gtk::Stack::new();
    stack.set_vhomogeneous(false);
    stack.add_named(&main, Some("main"));
    stack.add_named(&shapes, Some("shapes"));
    let show = |page: &'static str| {
        let stack = stack.downgrade();
        move || {
            if let Some(s) = stack.upgrade() {
                s.set_visible_child_name(page);
            }
        }
    };
    let (to_shapes, to_main, closed) = (show("shapes"), show("main"), show("main"));
    shapes_item.connect_clicked(move |_| to_shapes());
    back.connect_clicked(move |_| to_main());
    popover.connect_closed(move |_| closed());
    popover.set_child(Some(&stack));
    popover
}

/// Give the keys back to the page shown, the canvas or the outline, once the menu has
/// closed (a closing popover gives the focus to its button first).
fn to_page_later(view: &Rc<DeckWindow>) {
    let view: Weak<DeckWindow> = Rc::downgrade(view);
    glib::idle_add_local_once(move || {
        if let Some(view) = view.upgrade() {
            to_page(&view);
        }
    });
}

/// The keys to the outline when it's shown, else to the canvas.
fn to_page(view: &DeckWindow) {
    if view.outline.is_shown() {
        view.outline.focus();
    } else {
        view.canvas.grab_focus();
    }
}
