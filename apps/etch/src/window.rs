//! Windows: one per image, with the app's menu bar, the tool column, the canvas, and the
//! colour row and status bar below; opening files (the command line and File → Open);
//! the New Image dialog; and the benchmarks (`--bench-stroke`, `--bench-open`). Saving is
//! [`crate::saving`]'s.

use crate::canvas::Canvas;
use crate::colours;
use crate::image_menu;
use crate::saving::Saver;
use crate::settings::{self, Background};
use crate::text;
use crate::tools::{self, Settings, Tool};
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use image_io::Format;
use image_model::{Color, Image, Style, MAX_SIDE};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::time::Instant;

/// `etch`'s first image (always white: the remembered size and background are New
/// Image's).
pub const DEFAULT_SIZE: (u32, u32) = (1024, 768);
/// The benchmark's image: 8K.
const BENCH_SIZE: (u32, u32) = (7680, 4320);

thread_local! {
    /// What New Image offers: the size and background last used, read from the settings
    /// file the first time.
    static NEW: Cell<Option<settings::Settings>> = const { Cell::new(None) };
    /// Each window's controls, for New Image's background colour: the active window's.
    static CONTROLS: RefCell<Vec<(glib::WeakRef<gtk::ApplicationWindow>, Weak<Controls>)>> =
        const { RefCell::new(Vec::new()) };
    /// The windows of the files open, by canonical path: opening one again shows its window.
    static OPEN: RefCell<Vec<(PathBuf, glib::WeakRef<gtk::ApplicationWindow>)>> =
        const { RefCell::new(Vec::new()) };
}

/// What a window runs once its image shows.
#[derive(Clone, Copy)]
enum Bench {
    /// `--bench-stroke FRAMES`.
    Stroke(u32),
    /// `--bench-open FILE`: the process started at `start`; reading and decoding the file
    /// took `read_ms`.
    Open { start: Instant, read_ms: f64 },
}

/// The app's actions, shortcuts, menu bar (every window shows it), and style.
pub fn install_app_actions(app: &gtk::Application) {
    colours::install_style();
    let new = gio::ActionEntry::builder("new")
        .activate(|app: &gtk::Application, _, _| ask_new_image(app))
        .build();
    let open = gio::ActionEntry::builder("open")
        .activate(|app: &gtk::Application, _, _| choose_and_open(app))
        .build();
    // Quit closes each window, so a window with changes asks first.
    let quit = gio::ActionEntry::builder("quit")
        .activate(|app: &gtk::Application, _, _| {
            for window in app.windows() {
                window.close();
            }
        })
        .build();
    app.add_action_entries([new, open, quit]);
    for (action, keys) in [
        ("app.new", &["<Control>n"][..]),
        ("app.open", &["<Control>o"]),
        ("win.save", &["<Control>s"]),
        // Ctrl+Shift+S and Ctrl+Shift+E are the window's key handler's too, as Ctrl+Shift+Z
        // is (below); these show them in the menu.
        ("win.save-as", &["<Control><Shift>s"]),
        ("win.export", &["<Control><Shift>e"]),
        ("app.quit", &["<Control>q"]),
        ("window.close", &["<Control>w"]),
        ("win.undo", &["<Control>z"]),
        // Ctrl+Shift+Z is the window's key handler's: as an accelerator it misses a z
        // whose keymap doesn't consume Shift (wtype's, in check_etch.sh).
        ("win.redo", &["<Control>y"]),
        (
            "win.zoom-in",
            &["<Control>plus", "<Control>equal", "<Control>KP_Add"],
        ),
        ("win.zoom-out", &["<Control>minus", "<Control>KP_Subtract"]),
        ("win.actual-size", &["<Control>0", "<Control>KP_0"]),
        ("win.fit", &["<Control>9"]),
    ] {
        app.set_accels_for_action(action, keys);
    }

    let menu = gio::Menu::new();
    let file = gio::Menu::new();
    file.append(Some("_New Image…"), Some("app.new"));
    file.append(Some("_Open…"), Some("app.open"));
    file.append(Some("_Save"), Some("win.save"));
    file.append(Some("Save _As…"), Some("win.save-as"));
    file.append(Some("_Export…"), Some("win.export"));
    file.append(Some("_Close"), Some("window.close"));
    file.append(Some("_Quit"), Some("app.quit"));
    menu.append_submenu(Some("_File"), &file);
    let edit = gio::Menu::new();
    edit.append(Some("_Undo"), Some("win.undo"));
    edit.append(Some("_Redo"), Some("win.redo"));
    // The keys show beside each command; the window's key handler takes them, so a field
    // or the text being typed keeps its own Ctrl+X, Ctrl+C, Ctrl+V, Ctrl+A, Escape, and
    // Delete.
    let section = |items: &[(&str, &str, &str)]| {
        let section = gio::Menu::new();
        for &(label, action, accel) in items {
            let item = gio::MenuItem::new(Some(label), Some(action));
            item.set_attribute_value("accel", Some(&accel.to_variant()));
            section.append_item(&item);
        }
        section
    };
    edit.append_section(
        None,
        &section(&[
            ("Cu_t", "win.cut", "<Control>x"),
            ("_Copy", "win.copy", "<Control>c"),
            ("_Paste", "win.paste", "<Control>v"),
        ]),
    );
    edit.append_section(
        None,
        &section(&[
            ("Select _All", "win.select-all", "<Control>a"),
            ("_Deselect", "win.deselect", "Escape"),
            ("De_lete", "win.delete", "Delete"),
        ]),
    );
    menu.append_submenu(Some("_Edit"), &edit);
    let view = gio::Menu::new();
    view.append(Some("Zoom _In"), Some("win.zoom-in"));
    view.append(Some("Zoom _Out"), Some("win.zoom-out"));
    view.append(Some("_Actual Size"), Some("win.actual-size"));
    view.append(Some("_Fit to Window"), Some("win.fit"));
    menu.append_submenu(Some("_View"), &view);
    menu.append_submenu(Some("_Image"), &image_menu::menu());
    image_menu::set_accels(app);
    app.set_menubar(Some(&menu));
}

/// A window with a new white image of `size`. The pixels are allocated off the UI thread:
/// a large image takes a moment.
pub fn open_blank(app: &gtk::Application, size: (u32, u32)) {
    open_new(app, size, Color::WHITE, None);
}

/// `--bench-stroke FRAMES`: a window with an 8K image at 100%, then the benchmark.
pub fn open_bench(app: &gtk::Application, frames: u32) {
    open_new(app, BENCH_SIZE, Color::WHITE, Some(frames));
}

fn open_new(app: &gtk::Application, (w, h): (u32, u32), color: Color, bench: Option<u32>) {
    let app = app.clone();
    // Running until the window shows, though no window may be open yet.
    let hold = app.hold();
    glib::spawn_future_local(async move {
        let _hold = hold;
        let made = gio::spawn_blocking(move || Image::new(w, h, color)).await;
        match made {
            Ok(Ok(image)) => {
                let window = new_window(&app, &untitled(&app));
                show(&app, &window, image, None, bench.map(Bench::Stroke));
            }
            Ok(Err(e)) => alert(&app, "Cannot make the image", &e.to_string()).await,
            Err(_) => {
                alert(
                    &app,
                    "Cannot make the image",
                    "Making it failed unexpectedly.",
                )
                .await
            }
        }
    });
}

/// "Images" (PNG, JPEG, BMP) and "All files", the first as the default.
fn file_filters() -> (gio::ListStore, gtk::FileFilter) {
    let images = gtk::FileFilter::new();
    images.set_name(Some("Images (PNG, JPEG, BMP)"));
    for mime in ["image/png", "image/jpeg", "image/bmp"] {
        images.add_mime_type(mime);
    }
    for suffix in ["png", "jpg", "jpeg", "bmp"] {
        images.add_suffix(suffix);
    }
    let all = gtk::FileFilter::new();
    all.set_name(Some("All files"));
    all.add_pattern("*");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&images);
    filters.append(&all);
    (filters, images)
}

/// File → Open (Ctrl+O): the desktop's file dialog (xdg-desktop-portal through
/// `gtk::FileDialog`, GTK's own without a portal), filtered to images. The image opens in
/// a window of its own, as in the suite's other apps: an Untitled window stays as it is.
fn choose_and_open(app: &gtk::Application) {
    let (filters, images) = file_filters();
    let dialog = gtk::FileDialog::builder()
        .title("Open")
        .modal(true)
        .filters(&filters)
        .default_filter(&images)
        .build();
    let (app, parent) = (app.clone(), app.active_window());
    glib::spawn_future_local(async move {
        match dialog.open_future(parent.as_ref()).await {
            Ok(file) => match file.path() {
                Some(path) => open(&app, &path, None),
                None => {
                    alert(
                        &app,
                        &format!("Cannot open {}", file.uri()),
                        "Only local files can be opened.",
                    )
                    .await
                }
            },
            Err(e) if e.matches(gtk::DialogError::Dismissed) => {}
            Err(e) => alert(&app, "Cannot show the file dialog", &e.to_string()).await,
        }
    });
}

/// Open the image file at `path` in its own window: its existing one, or a new one titled
/// with the file's name that says "Opening…" while the file is read and decoded off the UI
/// thread. A file that can't be opened gives an alert naming it and the reason, and its
/// window closes. `bench`: `--bench-open`, started at that instant.
pub fn open(app: &gtk::Application, path: &Path, bench: Option<Instant>) {
    if let Some(window) = window_for(path) {
        window.present();
        return;
    }
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let window = new_window(app, &name);
    let opening = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    opening.set_halign(gtk::Align::Center);
    opening.set_valign(gtk::Align::Center);
    let spinner = gtk::Spinner::new();
    spinner.start();
    opening.append(&spinner);
    opening.append(&gtk::Label::new(Some("Opening…")));
    window.set_child(Some(&opening));
    OPEN.with_borrow_mut(|open| open.push((canonical(path), window.downgrade())));
    window.present();

    let (app, path) = (app.clone(), path.to_owned());
    glib::spawn_future_local(async move {
        let p = path.clone();
        let read = gio::spawn_blocking(move || {
            let t = Instant::now();
            (image_io::read(&p), t.elapsed())
        })
        .await;
        let error = match read {
            Ok((Ok((image, format)), took)) => {
                let read_ms = took.as_secs_f64() * 1e3;
                let (w, h) = (image.width(), image.height());
                tracing::info!("opened {}: {w}×{h} in {read_ms:.0} ms", path.display());
                let bench = bench.map(|start| Bench::Open { start, read_ms });
                show(&app, &window, image, Some((path, format)), bench);
                return;
            }
            Ok((Err(e), _)) => e.to_string(),
            Err(_) => "Reading it failed unexpectedly.".to_owned(),
        };
        eprintln!("etch: cannot open {}: {error}", path.display());
        if bench.is_some() {
            std::process::exit(1);
        }
        let _ = gtk::AlertDialog::builder()
            .message(format!("Cannot open “{name}”"))
            .detail(&error)
            .buttons(["Close"])
            .modal(true)
            .build()
            .choose_future(Some(&window))
            .await;
        window.close();
    });
}

/// The window `path` is open in, if any.
fn window_for(path: &Path) -> Option<gtk::ApplicationWindow> {
    let path = canonical(path);
    OPEN.with_borrow_mut(|open| {
        open.retain(|(_, w)| w.upgrade().is_some());
        open.iter()
            .find(|(p, _)| *p == path)
            .and_then(|(_, w)| w.upgrade())
    })
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}

/// `window` now shows the file at `path` (saved as it): opening that file shows it.
pub fn moved(window: &gtk::ApplicationWindow, path: &Path) {
    let path = canonical(path);
    OPEN.with_borrow_mut(|open| {
        open.retain(|(_, w)| w.upgrade().is_some_and(|w| w != *window));
        open.push((path, window.downgrade()));
    });
}

/// An empty window titled `title`, with the menu bar.
fn new_window(app: &gtk::Application, title: &str) -> gtk::ApplicationWindow {
    gtk::ApplicationWindow::builder()
        .application(app)
        .title(title)
        .default_width(1100)
        .default_height(800)
        .show_menubar(true)
        .build()
}

async fn alert(app: &gtk::Application, message: &str, detail: &str) {
    eprintln!("etch: {message}: {detail}");
    let _ = gtk::AlertDialog::builder()
        .message(message)
        .detail(detail)
        .buttons(["Close"])
        .modal(true)
        .build()
        .choose_future(app.active_window().as_ref())
        .await;
}

/// "Untitled", or "Untitled N" with the first N no other window has (its title marked
/// "• " or not).
fn untitled(app: &gtk::Application) -> String {
    let titles: Vec<_> = app
        .windows()
        .iter()
        .filter_map(|w| w.title())
        .map(|t| t.trim_start_matches("• ").to_owned())
        .collect();
    (1..)
        .map(|n| match n {
            1 => "Untitled".to_owned(),
            n => format!("Untitled {n}"),
        })
        .find(|t| !titles.iter().any(|w| w == t))
        .unwrap_or_default()
}

/// The status bar's labels: pointer position, the selection's size, zoom, image size.
struct Status {
    position: gtk::Label,
    selection: gtk::Label,
    zoom: gtk::Label,
    size: gtk::Label,
}

impl Status {
    fn new() -> (gtk::Box, Status) {
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 24);
        bar.set_margin_start(12);
        bar.set_margin_end(12);
        bar.set_margin_top(4);
        bar.set_margin_bottom(4);
        let label = |expand: bool| {
            let l = gtk::Label::new(None);
            l.set_xalign(0.0);
            l.set_hexpand(expand);
            bar.append(&l);
            l
        };
        let status = Status {
            position: label(true),
            selection: label(false),
            zoom: label(false),
            size: label(false),
        };
        (bar, status)
    }

    fn show(&self, canvas: &Canvas) {
        self.position.set_text(
            &canvas
                .pointer_pixel()
                .map(|(x, y)| format!("x: {x}  y: {y}"))
                .unwrap_or_default(),
        );
        self.selection.set_text(
            &canvas
                .selection_size()
                .map(|(w, h)| format!("Selection: {w}×{h}"))
                .unwrap_or_default(),
        );
        if let Some(v) = canvas.view() {
            self.zoom.set_text(&v.percent());
        }
        let (w, h) = canvas.image_size();
        self.size.set_text(&format!("{w}×{h}"));
    }
}

/// What the tool column, the colour row, the size, and the keys set, and the widgets that
/// show it: the canvas acts with the current tool.
struct Controls {
    settings: Cell<Settings>,
    canvas: Canvas,
    indicator: colours::Indicator,
    /// The size's label, field, and unit, shown while the tool has a size.
    sizing: gtk::Box,
    size: gtk::SpinButton,
    /// The fill's tolerance, its label and field, shown while the fill is the tool.
    tolerating: gtk::Box,
    tolerance: gtk::SpinButton,
    /// The brush's Hard and Soft buttons, shown while the brush is the tool.
    edge: gtk::Box,
    hard: gtk::ToggleButton,
    soft: gtk::ToggleButton,
    /// The rectangle's and ellipse's Outline, Filled, and Both buttons ([`STYLES`]), shown
    /// while one of them is the tool.
    style: gtk::Box,
    styles: [gtk::ToggleButton; 3],
    /// The text's font family list, Bold, and Italic, shown while text is the tool.
    font: gtk::Box,
    families: &'static [String],
    family: gtk::DropDown,
    bold: gtk::ToggleButton,
    italic: gtk::ToggleButton,
    /// Set while the controls show the settings: their changes then aren't the user's.
    showing: Cell<bool>,
}

/// The styles of the style buttons, in order.
const STYLES: [Style; 3] = [Style::Outline, Style::Filled, Style::Both];

impl Controls {
    fn update(&self, f: impl FnOnce(&mut Settings)) {
        let mut s = self.settings.get();
        f(&mut s);
        self.settings.set(s);
        self.show();
    }

    /// A change the user made in a control: applied unless the controls are showing the
    /// settings.
    fn changed(&self, f: impl FnOnce(&mut Settings)) {
        if !self.showing.get() {
            self.update(f);
        }
    }

    fn show(&self) {
        let s = self.settings.get();
        self.showing.set(true);
        self.canvas.set_action(s.action());
        self.indicator.set(s.foreground, s.background);
        self.sizing.set_visible(s.width().is_some());
        let sizes = s.sizes();
        self.size
            .set_range(*sizes.start() as f64, *sizes.end() as f64);
        if let Some(w) = s.width().filter(|&w| self.size.value() as u32 != w) {
            self.size.set_value(w as f64);
        }
        self.tolerating.set_visible(s.tool == Tool::Fill);
        if self.tolerance.value() as u8 != s.tolerance {
            self.tolerance.set_value(s.tolerance as f64);
        }
        self.edge.set_visible(s.tool == Tool::Brush);
        let edge = if s.soft { &self.soft } else { &self.hard };
        if !edge.is_active() {
            edge.set_active(true);
        }
        self.style.set_visible(s.has_style());
        let style = STYLES
            .iter()
            .position(|&t| t == s.style)
            .map(|i| &self.styles[i]);
        if let Some(b) = style.filter(|b| !b.is_active()) {
            b.set_active(true);
        }
        self.font.set_visible(s.tool == Tool::Text);
        if let Some(i) = self.families.iter().position(|f| f == s.family) {
            if self.family.selected() != i as u32 {
                self.family.set_selected(i as u32);
            }
        }
        self.bold.set_active(s.bold);
        self.italic.set_active(s.italic);
        self.showing.set(false);
    }

    /// After a control changed the text's look: the keys go back to the text being typed.
    fn back_to_text(&self) {
        if self.canvas.typing() {
            let canvas = self.canvas.clone();
            // Once the event is handled: a list that closes gives the keys to its button.
            glib::idle_add_local_once(move || {
                if canvas.typing() {
                    canvas.grab_focus();
                }
            });
        }
    }
}

/// A choice of a few, labelled `label`: linked toggle buttons, one on, each with its
/// name and tooltip.
fn choice<const N: usize>(
    label: &str,
    options: [(&str, &str); N],
) -> (gtk::Box, [gtk::ToggleButton; N]) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    row.add_css_class("linked");
    row.set_valign(gtk::Align::Center);
    let buttons = options.map(|(name, tip)| {
        let b = gtk::ToggleButton::with_label(name);
        b.set_tooltip_text(Some(tip));
        b.set_focusable(false);
        row.append(&b);
        b
    });
    for b in &buttons[1..] {
        b.set_group(Some(&buttons[0]));
    }
    row.update_property(&[gtk::accessible::Property::Label(label)]);
    (row, buttons)
}

/// A toggle button showing `markup`, named `name` (for screen readers), with `tip`.
fn toggle(markup: &str, name: &str, tip: &str) -> gtk::ToggleButton {
    let label = gtk::Label::new(None);
    label.set_markup(markup);
    let b = gtk::ToggleButton::new();
    b.set_child(Some(&label));
    b.set_tooltip_text(Some(tip));
    b.set_focusable(false);
    b.set_valign(gtk::Align::Center);
    b.update_property(&[gtk::accessible::Property::Label(name)]);
    b
}

/// The text's font: a family list (searched by typing), Bold, and Italic.
fn font_controls(context: &gtk::pango::Context) -> (gtk::Box, &'static [String], gtk::DropDown) {
    let families = text::families(context);
    let names: Vec<&str> = families.iter().map(String::as_str).collect();
    let string = gtk::PropertyExpression::new(
        gtk::StringObject::static_type(),
        None::<gtk::Expression>,
        "string",
    );
    let family = gtk::DropDown::new(Some(gtk::StringList::new(&names)), Some(string));
    family.set_enable_search(true);
    family.set_search_match_mode(gtk::StringFilterMatchMode::Substring);
    family.set_valign(gtk::Align::Center);
    family.set_size_request(150, -1);
    family.set_tooltip_text(Some("Font (type to search)"));
    // A drop-down is named by the item it shows: its description says what it is.
    family.update_property(&[
        gtk::accessible::Property::Label("Font"),
        gtk::accessible::Property::Description("Font"),
    ]);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    row.append(&family);
    (row, families, family)
}

/// The colour row and the size, left of the status: the colours, the palette, the
/// current tool's size, the text's font, the brush's edge, and the shapes' style.
fn colour_row(controls: &Rc<Controls>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.append(&controls.indicator);
    let pick: Rc<dyn Fn(Color, bool)> = {
        let controls = Rc::downgrade(controls);
        Rc::new(move |colour, background| {
            if let Some(c) = controls.upgrade() {
                c.update(|s| match background {
                    true => s.background = colour,
                    false => s.foreground = colour,
                });
            }
        })
    };
    let palette = colours::palette(pick);
    palette.set_valign(gtk::Align::Center);
    row.append(&palette);
    let label = gtk::Label::with_mnemonic("_Size");
    label.set_mnemonic_widget(Some(&controls.size));
    controls.sizing.append(&label);
    controls.size.set_valign(gtk::Align::Center);
    controls
        .size
        .update_property(&[gtk::accessible::Property::Label("Size")]);
    controls.size.connect_value_changed({
        let controls = Rc::downgrade(controls);
        move |spin| {
            if let Some(c) = controls.upgrade() {
                c.changed(|s| s.set_width(spin.value() as u32));
            }
        }
    });
    // Enter in the size gives the keys back to the text being typed.
    controls.size.connect_activate({
        let controls = Rc::downgrade(controls);
        move |_| {
            if let Some(c) = controls.upgrade() {
                c.back_to_text();
            }
        }
    });
    controls.sizing.append(&controls.size);
    controls.sizing.append(&gtk::Label::new(Some("px")));
    row.append(&controls.sizing);
    let label = gtk::Label::with_mnemonic("_Tolerance");
    label.set_mnemonic_widget(Some(&controls.tolerance));
    controls.tolerating.append(&label);
    controls.tolerance.set_valign(gtk::Align::Center);
    controls.tolerance.set_tooltip_text(Some(
        "Tolerance: how far, 0 to 255 in any channel, a colour may be from the clicked one and still fill (0: exactly its colour)",
    ));
    controls
        .tolerance
        .update_property(&[gtk::accessible::Property::Label("Tolerance")]);
    controls.tolerance.connect_value_changed({
        let controls = Rc::downgrade(controls);
        move |spin| {
            if let Some(c) = controls.upgrade() {
                c.changed(|s| s.tolerance = spin.value() as u8);
            }
        }
    });
    controls.tolerating.append(&controls.tolerance);
    row.append(&controls.tolerating);
    controls.family.connect_selected_notify({
        let controls = Rc::downgrade(controls);
        move |d| {
            let Some(c) = controls.upgrade() else { return };
            if let Some(family) = c.families.get(d.selected() as usize) {
                if !c.showing.get() {
                    c.update(|s| s.family = family);
                    c.back_to_text();
                }
            }
        }
    });
    for (button, set) in [
        (
            &controls.bold,
            (|s, on| s.bold = on) as fn(&mut Settings, bool),
        ),
        (&controls.italic, |s, on| s.italic = on),
    ] {
        button.connect_toggled({
            let controls = Rc::downgrade(controls);
            move |b| {
                if let Some(c) = controls.upgrade() {
                    c.changed(|s| set(s, b.is_active()));
                }
            }
        });
        controls.font.append(button);
    }
    row.append(&controls.font);
    controls.soft.connect_toggled({
        let controls = Rc::downgrade(controls);
        move |soft| {
            if let Some(c) = controls.upgrade() {
                if c.settings.get().soft != soft.is_active() {
                    c.changed(|s| s.soft = soft.is_active());
                }
            }
        }
    });
    row.append(&controls.edge);
    for (button, style) in controls.styles.iter().zip(STYLES) {
        button.connect_toggled({
            let controls = Rc::downgrade(controls);
            move |b| {
                if let Some(c) = controls.upgrade() {
                    if b.is_active() && c.settings.get().style != style {
                        c.changed(|s| s.style = style);
                    }
                }
            }
        });
    }
    row.append(&controls.style);
    row
}

/// Fill `window` with the image's view: the tool column, the canvas showing `image` (of
/// `file`, its path and format; `None` for a new image), the colour row, and the status
/// bar; then run `bench`, if given.
fn show(
    app: &gtk::Application,
    window: &gtk::ApplicationWindow,
    image: Image,
    file: Option<(PathBuf, Format)>,
    bench: Option<Bench>,
) {
    let canvas = Canvas::new(image, matches!(bench, Some(Bench::Stroke(_))));
    let (edge, [hard, soft]) = choice(
        "Edge",
        [
            ("Hard", "Hard edge: solid to the rim"),
            ("Soft", "Soft edge: fades to transparent at the rim"),
        ],
    );
    let (style, styles) = choice(
        "Style",
        [
            (
                "Outline",
                "Outline: the shape's outline in the foreground colour",
            ),
            ("Filled", "Filled: the whole shape in the foreground colour"),
            (
                "Both",
                "Both: the outline in the foreground colour, filled with the background colour",
            ),
        ],
    );
    let (font, families, family) = font_controls(&canvas.pango_context());
    let controls = Rc::new(Controls {
        settings: Cell::new(Settings::default()),
        canvas: canvas.clone(),
        indicator: colours::Indicator::new(),
        sizing: gtk::Box::new(gtk::Orientation::Horizontal, 12),
        size: gtk::SpinButton::with_range(
            *tools::SIZES.start() as f64,
            *tools::SIZES.end() as f64,
            1.0,
        ),
        tolerating: gtk::Box::new(gtk::Orientation::Horizontal, 12),
        tolerance: gtk::SpinButton::with_range(0.0, 255.0, 1.0),
        edge,
        hard,
        soft,
        style,
        styles,
        font,
        families,
        family,
        bold: toggle("<b>B</b>", "Bold", "Bold"),
        italic: toggle("<i>I</i>", "Italic", "Italic"),
        showing: Cell::new(false),
    });
    controls.show();
    CONTROLS.with_borrow_mut(|c| {
        c.retain(|(w, _)| w.upgrade().is_some());
        c.push((window.downgrade(), Rc::downgrade(&controls)));
    });
    let (bar, status) = Status::new();
    bar.prepend(&colour_row(&controls));
    let middle = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    middle.append(&tools::column());
    middle.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    middle.append(&canvas);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&middle);
    content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    content.append(&bar);
    window.set_child(Some(&content));

    let action = |name: &str, f: fn(&Canvas)| {
        let canvas = canvas.clone();
        gio::ActionEntry::builder(name)
            .activate(move |_: &gtk::ApplicationWindow, _, _| f(&canvas))
            .build()
    };
    window.add_action_entries([
        action("undo", Canvas::undo),
        action("redo", Canvas::redo),
        action("zoom-in", |c| c.zoom_step(1, None)),
        action("zoom-out", |c| c.zoom_step(-1, None)),
        action("actual-size", Canvas::actual_size),
        action("fit", Canvas::fit),
        action("deselect", |c| {
            c.deselect();
        }),
        action("delete", |c| {
            c.delete_selection();
        }),
        action("cut", |c| {
            c.cut();
        }),
        action("copy", |c| {
            c.copy();
        }),
    ]);
    let with_window = |name: &str, f: fn(&gtk::ApplicationWindow, &Canvas)| {
        let canvas = canvas.clone();
        gio::ActionEntry::builder(name)
            .activate(move |window: &gtk::ApplicationWindow, _, _| f(window, &canvas))
            .build()
    };
    window.add_action_entries([
        with_window("select-all", select_all),
        with_window("paste", paste),
    ]);
    image_menu::add_actions(window, &canvas);
    // The tool: the tool column's buttons and the tool keys set it.
    let tool = gio::ActionEntry::builder("tool")
        .parameter_type(Some(glib::VariantTy::STRING))
        .state(Tool::default().id().to_variant())
        .activate(|_: &gtk::ApplicationWindow, action, target| {
            if let Some(target) = target {
                action.change_state(target);
            }
        })
        .change_state({
            let controls = controls.clone();
            move |_: &gtk::ApplicationWindow, action, state| {
                let picked = state.and_then(|s| s.str()).and_then(Tool::from_id);
                if let (Some(t), Some(state)) = (picked, state) {
                    action.set_state(state);
                    controls.update(|s| s.set_tool(t));
                }
            }
        })
        .build();
    window.add_action_entries([tool]);
    // A pick sets the colour and goes back to the tool used before the picker.
    canvas.connect_picked({
        let (window, controls) = (window.downgrade(), Rc::downgrade(&controls));
        move |colour, background| {
            let (Some(window), Some(controls)) = (window.upgrade(), controls.upgrade()) else {
                return;
            };
            let mut back = Tool::default();
            controls.update(|s| back = s.pick(colour, background));
            ActionGroupExt::activate_action(&window, "tool", Some(&back.id().to_variant()));
        }
    });

    // Space held: dragging pans. Escape drops a shape being dragged, or deselects.
    // Ctrl+Shift+Z redoes, Ctrl+Shift+S is Save As, Ctrl+Shift+E Export, and Ctrl+Shift+X
    // crops (shifted, a letter may arrive upper case, which an accelerator misses).
    // Ctrl+X, Ctrl+C, and Ctrl+V cut, copy, and paste, Ctrl+A selects all, Delete deletes
    // the selection, and the arrows move it a pixel (Shift: ten).
    // Unmodified, a tool's key picks it and X swaps the colours. None of them while a
    // text field (the size) has the keys, and only the Ctrl+Shift ones while text is
    // typed on the canvas: its keys are its letters.
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed({
        let (canvas, window, controls) = (canvas.clone(), window.downgrade(), controls.clone());
        move |_, key, _, state| {
            let Some(window) = window.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let field = GtkWindowExt::focus(&window)
                .is_some_and(|f| f.is::<gtk::Text>() || f.is::<gtk::TextView>());
            if field {
                return glib::Propagation::Proceed;
            }
            // While an Image menu command runs, the image waits for it.
            if canvas.busy() {
                return glib::Propagation::Stop;
            }
            let text = canvas.typing();
            let mods = state & gtk::accelerator_get_default_mod_mask();
            let redo = gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK;
            if key == gdk::Key::space && !text {
                canvas.set_space(true);
                return glib::Propagation::Stop;
            }
            if key == gdk::Key::Escape && canvas.cancel_shape() {
                return glib::Propagation::Stop;
            }
            if mods == redo && matches!(key, gdk::Key::z | gdk::Key::Z) {
                canvas.redo();
                return glib::Propagation::Stop;
            }
            let shifted = match key {
                gdk::Key::s | gdk::Key::S => Some("save-as"),
                gdk::Key::e | gdk::Key::E => Some("export"),
                _ => None,
            };
            if let (true, Some(action)) = (mods == redo, shifted) {
                ActionGroupExt::activate_action(&window, action, None);
                return glib::Propagation::Stop;
            }
            if mods == redo && image_menu::shifted_key(&window, key) {
                return glib::Propagation::Stop;
            }
            if text {
                return glib::Propagation::Proceed;
            }
            if mods == gdk::ModifierType::CONTROL_MASK {
                match key {
                    gdk::Key::a | gdk::Key::A => select_all(&window, &canvas),
                    gdk::Key::x | gdk::Key::X => {
                        canvas.cut();
                    }
                    gdk::Key::c | gdk::Key::C => {
                        canvas.copy();
                    }
                    gdk::Key::v | gdk::Key::V => paste(&window, &canvas),
                    _ => return glib::Propagation::Proceed,
                }
                return glib::Propagation::Stop;
            }
            if mods.is_empty() || mods == gdk::ModifierType::SHIFT_MASK {
                let step = if mods.is_empty() { 1 } else { 10 };
                let by = match key {
                    gdk::Key::Left | gdk::Key::KP_Left => Some((-step, 0)),
                    gdk::Key::Right | gdk::Key::KP_Right => Some((step, 0)),
                    gdk::Key::Up | gdk::Key::KP_Up => Some((0, -step)),
                    gdk::Key::Down | gdk::Key::KP_Down => Some((0, step)),
                    _ => None,
                };
                if by.is_some_and(|(dx, dy)| canvas.nudge(dx, dy)) {
                    return glib::Propagation::Stop;
                }
            }
            if !mods.is_empty() {
                return glib::Propagation::Proceed;
            }
            if key == gdk::Key::Escape && canvas.deselect() {
                return glib::Propagation::Stop;
            }
            if matches!(key, gdk::Key::Delete | gdk::Key::KP_Delete) && canvas.delete_selection() {
                return glib::Propagation::Stop;
            }
            if matches!(key, gdk::Key::x | gdk::Key::X) {
                controls.update(Settings::swap);
                return glib::Propagation::Stop;
            }
            let picked = key.to_unicode().and_then(|c| {
                Tool::ALL
                    .into_iter()
                    .find(|t| t.key() == c.to_ascii_uppercase())
            });
            if let Some(t) = picked {
                ActionGroupExt::activate_action(&window, "tool", Some(&t.id().to_variant()));
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        }
    });
    keys.connect_key_released({
        let canvas = canvas.clone();
        move |_, key, _, _| {
            if key == gdk::Key::space {
                canvas.set_space(false);
            }
        }
    });
    window.add_controller(keys);

    canvas.connect_changed({
        let window = window.downgrade();
        move |canvas| {
            let Some(window) = window.upgrade() else {
                return;
            };
            status.show(canvas);
            let selected = canvas.selection_size().is_some();
            for (name, on) in [
                ("undo", canvas.can_undo()),
                ("redo", canvas.can_redo()),
                ("deselect", selected),
                ("delete", selected),
                ("cut", selected),
                ("copy", selected),
            ] {
                if let Some(a) = window
                    .lookup_action(name)
                    .and_downcast::<gio::SimpleAction>()
                {
                    a.set_enabled(on);
                }
            }
            image_menu::update(&window, canvas);
        }
    });
    Saver::attach(window, &canvas, file);
    window.present();
    canvas.grab_focus();
    match bench {
        Some(Bench::Stroke(frames)) => bench_stroke(app, &canvas, frames),
        Some(Bench::Open { start, read_ms }) => bench_open(app, window, &canvas, start, read_ms),
        None => {}
    }
}

/// Edit → Paste: reads the clipboard's image without waiting for it, converts it to the
/// image's pixels off the UI thread, then picks the selection tool (as Paint does, so the
/// pasted pixels can be moved at once) and floats them over the image. Nothing happens
/// when the clipboard holds no image.
fn paste(window: &gtk::ApplicationWindow, canvas: &Canvas) {
    let clipboard = canvas.clipboard();
    let (window, canvas) = (window.downgrade(), canvas.downgrade());
    glib::spawn_future_local(async move {
        let texture = match clipboard.read_texture_future().await {
            Ok(Some(texture)) => texture,
            Ok(None) => return,
            Err(e) => {
                tracing::info!("nothing to paste: {e}");
                return;
            }
        };
        let converted = gio::spawn_blocking(move || crate::canvas::pasted(&texture)).await;
        let (pixels, shown) = match converted {
            Ok(Ok(pasted)) => pasted,
            Ok(Err(e)) => {
                tracing::warn!("cannot paste the clipboard's image: {e}");
                return;
            }
            Err(_) => {
                tracing::warn!("pasting failed unexpectedly");
                return;
            }
        };
        let (Some(window), Some(canvas)) = (window.upgrade(), canvas.upgrade()) else {
            return;
        };
        let selection = Tool::Selection.id().to_variant();
        ActionGroupExt::activate_action(&window, "tool", Some(&selection));
        canvas.paste(pixels, shown);
    });
}

/// Edit → Select All: the selection becomes the tool (as in Paint, so the selection can be
/// moved at once) and selects the whole image.
fn select_all(window: &gtk::ApplicationWindow, canvas: &Canvas) {
    let selection = Tool::Selection.id().to_variant();
    ActionGroupExt::activate_action(window, "tool", Some(&selection));
    canvas.select_all();
}

/// New Image's size and background: the last used, from the settings file the first time.
fn new_settings() -> settings::Settings {
    NEW.get().unwrap_or_else(|| {
        let s = settings::Settings::load();
        NEW.set(Some(s));
        s
    })
}

/// The background colour of `window` (the colour row's); white without one.
fn background_of(window: Option<&gtk::Window>) -> Color {
    let found = window.and_then(|window| {
        CONTROLS.with_borrow(|c| {
            c.iter()
                .find(|(w, _)| {
                    w.upgrade()
                        .is_some_and(|w| w.upcast_ref::<gtk::Window>() == window)
                })
                .and_then(|(_, c)| c.upgrade())
                .map(|c| c.settings.get().background)
        })
    });
    found.unwrap_or(Settings::default().background)
}

/// File → New Image: asks for a width, a height, and a background (white, transparent, or
/// the active window's background colour), offering the last ones used, then opens a
/// window with an image of them. What was asked is remembered across runs.
fn ask_new_image(app: &gtk::Application) {
    let parent = app.active_window();
    let background = background_of(parent.as_ref());
    let dialog = gtk::Window::builder()
        .title("New Image")
        .modal(true)
        .resizable(false)
        .build();
    dialog.set_transient_for(parent.as_ref());
    let grid = gtk::Grid::builder()
        .row_spacing(8)
        .column_spacing(8)
        .margin_start(16)
        .margin_end(16)
        .margin_top(16)
        .margin_bottom(16)
        .build();
    let last = new_settings();
    let (w, h) = last.new_size;
    let side = |row: i32, label: &str, value: u32| {
        let name = gtk::Label::with_mnemonic(label);
        name.set_xalign(0.0);
        let spin = gtk::SpinButton::with_range(1.0, MAX_SIDE as f64, 1.0);
        spin.set_value(value as f64);
        spin.set_activates_default(true);
        spin.update_property(&[gtk::accessible::Property::Label(&label.replace('_', ""))]);
        name.set_mnemonic_widget(Some(&spin));
        grid.attach(&name, 0, row, 1, 1);
        grid.attach(&spin, 1, row, 1, 1);
        grid.attach(&gtk::Label::new(Some("pixels")), 2, row, 1, 1);
        spin
    };
    let width = side(0, "_Width", w);
    let height = side(1, "_Height", h);
    let name = gtk::Label::new(Some("Background"));
    name.set_xalign(0.0);
    name.set_valign(gtk::Align::Start);
    grid.attach(&name, 0, 2, 1, 1);
    let choices = gtk::Box::new(gtk::Orientation::Vertical, 4);
    choices.update_property(&[gtk::accessible::Property::Label("Background")]);
    let colour = colours::hex(background);
    let backgrounds = Background::ALL.map(|b| {
        let label = match b {
            Background::White => "Wh_ite",
            Background::Transparent => "_Transparent",
            Background::Colour => "Back_ground colour",
        };
        let button = gtk::CheckButton::with_mnemonic(label);
        if b == Background::Colour {
            button.set_tooltip_text(Some(&format!("The background colour, {colour}")));
            button.update_property(&[gtk::accessible::Property::Description(&colour)]);
        }
        button.set_active(b == last.new_background);
        choices.append(&button);
        button
    });
    for b in &backgrounds[1..] {
        b.set_group(Some(&backgrounds[0]));
    }
    grid.attach(&choices, 1, 2, 2, 1);
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    buttons.set_margin_top(8);
    let cancel = gtk::Button::with_mnemonic("_Cancel");
    let create = gtk::Button::with_mnemonic("C_reate");
    create.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&create);
    grid.attach(&buttons, 0, 3, 3, 1);
    dialog.set_child(Some(&grid));
    dialog.set_default_widget(Some(&create));

    cancel.connect_clicked({
        let dialog = dialog.downgrade();
        move |_| {
            if let Some(d) = dialog.upgrade() {
                d.close();
            }
        }
    });
    create.connect_clicked({
        let (dialog, app) = (dialog.downgrade(), app.clone());
        move |_| {
            width.update();
            height.update();
            let chosen = settings::Settings {
                new_size: (width.value() as u32, height.value() as u32),
                new_background: Background::ALL
                    .into_iter()
                    .zip(&backgrounds)
                    .find(|(_, b)| b.is_active())
                    .map_or(Background::White, |(b, _)| b),
            };
            if let Some(d) = dialog.upgrade() {
                d.close();
            }
            remember(&app, chosen);
            open_new(
                &app,
                chosen.new_size,
                chosen.new_background.color(background),
                None,
            );
        }
    });
    let escape = gtk::ShortcutController::new();
    escape.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("Escape"),
        Some(gtk::NamedAction::new("window.close")),
    ));
    dialog.add_controller(escape);
    dialog.present();
}

/// Makes `chosen` what New Image offers, and writes it to the settings file off the UI
/// thread (the app runs until it's written).
fn remember(app: &gtk::Application, chosen: settings::Settings) {
    if NEW.get() == Some(chosen) {
        return;
    }
    NEW.set(Some(chosen));
    let hold = app.hold();
    glib::spawn_future_local(async move {
        let _hold = hold;
        let path = settings::path();
        let shown = path.clone();
        match gio::spawn_blocking(move || settings::write(&path, &chosen)).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::warn!("cannot save {}: {e}", shown.display()),
            Err(_) => tracing::warn!("saving {} failed unexpectedly", shown.display()),
        }
    });
}

/// ETCH-6's benchmark: from the process's start until the end of the first frame that drew
/// the opened image (the frame clock's after-paint once the canvas has snapshotted it, so
/// the frame rendered its tiles); prints a JSON line and quits. `read_ms`: reading and
/// decoding the file, off the UI thread.
fn bench_open(
    app: &gtk::Application,
    window: &gtk::ApplicationWindow,
    canvas: &Canvas,
    start: Instant,
    read_ms: f64,
) {
    let shown_ms = start.elapsed().as_secs_f64() * 1e3;
    let Some(clock) = window.frame_clock() else {
        eprintln!("etch: the window has no frame clock");
        std::process::exit(1);
    };
    let done = Cell::new(false);
    canvas.queue_draw();
    let (app, canvas) = (app.clone(), canvas.downgrade());
    clock.connect_after_paint(move |_| {
        let Some(canvas) = canvas.upgrade() else {
            return;
        };
        if done.get() || canvas.frames_drawn() == 0 {
            return;
        }
        done.set(true);
        let (w, h) = canvas.image_size();
        println!(
            r#"{{"image":[{w},{h}],"canvas":[{},{}],"zoom":"{}","read_ms":{read_ms:.1},"shown_ms":{shown_ms:.1},"snapshot_ms":{:.1},"first_frame_ms":{:.1}}}"#,
            canvas.width(),
            canvas.height(),
            canvas.view().map(|v| v.percent()).unwrap_or_default(),
            canvas.last_draw_ms(),
            start.elapsed().as_secs_f64() * 1e3,
        );
        app.quit();
    });
}

/// ETCH-2's benchmark: draws a stroke one segment a frame, 16 widget pixels apart (a brisk
/// hand at 60 frames a second), bouncing around the canvas on the 8K image at 100%, for
/// `frames` frames after 30 to settle. Measures each frame's CPU time (frame clock before-
/// to after-paint, which holds the segment, the snapshot, and rendering), the segment's
/// own share, and the snapshot's; prints a JSON line and quits.
fn bench_stroke(app: &gtk::Application, canvas: &Canvas, frames: u32) {
    let Some(clock) = canvas.frame_clock() else {
        // Not realized yet: try again once it is.
        let app = app.clone();
        canvas.connect_realize(move |c| bench_stroke(&app, c, frames));
        return;
    };
    #[derive(Default)]
    struct Rec {
        on: bool,
        start: Option<Instant>,
        cpu_ms: Vec<f64>,
        segment_ms: Vec<f64>,
        snapshot_ms: Vec<f64>,
    }
    let rec = Rc::new(RefCell::new(Rec::default()));
    clock.connect_before_paint({
        let rec = rec.clone();
        move |_| rec.borrow_mut().start = Some(Instant::now())
    });
    clock.connect_after_paint({
        let (rec, canvas) = (rec.clone(), canvas.downgrade());
        move |_| {
            let mut r = rec.borrow_mut();
            if let (true, Some(t0), Some(c)) = (r.on, r.start, canvas.upgrade()) {
                r.cpu_ms.push(t0.elapsed().as_secs_f64() * 1e3);
                r.snapshot_ms.push(c.last_draw_ms());
            }
        }
    });
    let ticks = Cell::new(0u32);
    let pos = Cell::new((40.0f64, 40.0f64));
    let dir = Cell::new((16.0f64 * 0.8, 16.0f64 * 0.6));
    let app = app.clone();
    canvas.add_tick_callback(move |canvas, _| {
        let n = ticks.get();
        ticks.set(n + 1);
        if n < 30 {
            return glib::ControlFlow::Continue;
        }
        if n == 30 {
            rec.borrow_mut().on = true;
            canvas.draw_begin(pos.get());
            return glib::ControlFlow::Continue;
        }
        if n > 30 + frames {
            canvas.draw_end();
            let r = rec.borrow();
            let (w, h) = canvas.image_size();
            println!(
                r#"{{"image":[{w},{h}],"canvas":[{},{}],"zoom":"{}","frames":{},"frame_cpu_ms":{},"segment_ms":{},"snapshot_ms":{}}}"#,
                canvas.width(),
                canvas.height(),
                canvas.view().map(|v| v.percent()).unwrap_or_default(),
                r.cpu_ms.len(),
                summary(&r.cpu_ms),
                summary(&r.segment_ms),
                summary(&r.snapshot_ms),
            );
            app.quit();
            return glib::ControlFlow::Break;
        }
        // Bounce off the canvas's edges.
        let ((x, y), (mut dx, mut dy)) = (pos.get(), dir.get());
        let (cw, ch) = (canvas.width() as f64, canvas.height() as f64);
        if !(0.0..cw).contains(&(x + dx)) {
            dx = -dx;
        }
        if !(0.0..ch).contains(&(y + dy)) {
            dy = -dy;
        }
        let next = (x + dx, y + dy);
        pos.set(next);
        dir.set((dx, dy));
        let t = Instant::now();
        canvas.draw_to(next);
        rec.borrow_mut()
            .segment_ms
            .push(t.elapsed().as_secs_f64() * 1e3);
        glib::ControlFlow::Continue
    });
}

/// `{"p50":..,"p99":..,"max":..}` of `xs`.
fn summary(xs: &[f64]) -> String {
    let mut v = xs.to_vec();
    v.sort_by(f64::total_cmp);
    let at = |q: f64| {
        if v.is_empty() {
            0.0
        } else {
            v[((v.len() - 1) as f64 * q).round() as usize]
        }
    };
    format!(
        r#"{{"p50":{:.3},"p99":{:.3},"max":{:.3}}}"#,
        at(0.5),
        at(0.99),
        at(1.0)
    )
}
