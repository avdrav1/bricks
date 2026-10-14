//! Windows: one per image, with the app's menu bar, the canvas, and a status bar; the
//! New Image dialog; and the stroke benchmark (`--bench-stroke`).

use crate::canvas::Canvas;
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use image_model::{Color, Image, MAX_SIDE};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;

/// `etch`'s first image, and New Image's size until another is used.
pub const DEFAULT_SIZE: (u32, u32) = (1024, 768);
/// The benchmark's image: 8K.
const BENCH_SIZE: (u32, u32) = (7680, 4320);

thread_local! {
    /// The size New Image offers: the last one used.
    static LAST_SIZE: Cell<(u32, u32)> = const { Cell::new(DEFAULT_SIZE) };
}

/// The app's actions, shortcuts, and menu bar (every window shows it).
pub fn install_app_actions(app: &gtk::Application) {
    let new = gio::ActionEntry::builder("new")
        .activate(|app: &gtk::Application, _, _| ask_new_image(app))
        .build();
    let quit = gio::ActionEntry::builder("quit")
        .activate(|app: &gtk::Application, _, _| app.quit())
        .build();
    app.add_action_entries([new, quit]);
    for (action, keys) in [
        ("app.new", &["<Control>n"][..]),
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
    file.append(Some("_Close"), Some("window.close"));
    file.append(Some("_Quit"), Some("app.quit"));
    menu.append_submenu(Some("_File"), &file);
    let edit = gio::Menu::new();
    edit.append(Some("_Undo"), Some("win.undo"));
    edit.append(Some("_Redo"), Some("win.redo"));
    menu.append_submenu(Some("_Edit"), &edit);
    let view = gio::Menu::new();
    view.append(Some("Zoom _In"), Some("win.zoom-in"));
    view.append(Some("Zoom _Out"), Some("win.zoom-out"));
    view.append(Some("_Actual Size"), Some("win.actual-size"));
    view.append(Some("_Fit to Window"), Some("win.fit"));
    menu.append_submenu(Some("_View"), &view);
    app.set_menubar(Some(&menu));
}

/// A window with a new white image of `size`. The pixels are allocated off the UI thread:
/// a large image takes a moment.
pub fn open_blank(app: &gtk::Application, size: (u32, u32)) {
    open_new(app, size, None);
}

/// `--bench-stroke FRAMES`: a window with an 8K image at 100%, then the benchmark.
pub fn open_bench(app: &gtk::Application, frames: u32) {
    open_new(app, BENCH_SIZE, Some(frames));
}

fn open_new(app: &gtk::Application, (w, h): (u32, u32), bench: Option<u32>) {
    let app = app.clone();
    // Running until the window shows, though no window may be open yet.
    let hold = app.hold();
    glib::spawn_future_local(async move {
        let _hold = hold;
        let made = gio::spawn_blocking(move || Image::new(w, h, Color::WHITE)).await;
        match made {
            Ok(Ok(image)) => show(&app, image, bench),
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

/// "Untitled", or "Untitled N" with the first N no other window has.
fn untitled(app: &gtk::Application) -> String {
    let titles: Vec<_> = app.windows().iter().filter_map(|w| w.title()).collect();
    (1..)
        .map(|n| match n {
            1 => "Untitled".to_owned(),
            n => format!("Untitled {n}"),
        })
        .find(|t| !titles.iter().any(|w| w == t))
        .unwrap_or_default()
}

/// The status bar's labels: pointer position, zoom, image size.
struct Status {
    position: gtk::Label,
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
        if let Some(v) = canvas.view() {
            self.zoom.set_text(&v.percent());
        }
        let (w, h) = canvas.image_size();
        self.size.set_text(&format!("{w}×{h}"));
    }
}

fn show(app: &gtk::Application, image: Image, bench: Option<u32>) {
    let canvas = Canvas::new(image, bench.is_some());
    let (bar, status) = Status::new();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&canvas);
    content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    content.append(&bar);
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title(untitled(app))
        .default_width(1100)
        .default_height(800)
        .show_menubar(true)
        .child(&content)
        .build();

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
    ]);

    // Space held: dragging pans. Ctrl+Shift+Z redoes (shifted, z may arrive as Z; Edit →
    // Redo's accelerator is Ctrl+Y).
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed({
        let canvas = canvas.clone();
        move |_, key, _, state| {
            let mods = state & gtk::accelerator_get_default_mod_mask();
            let redo = gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK;
            if key == gdk::Key::space {
                canvas.set_space(true);
                return glib::Propagation::Stop;
            }
            if mods == redo && matches!(key, gdk::Key::z | gdk::Key::Z) {
                canvas.redo();
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
            for (name, on) in [("undo", canvas.can_undo()), ("redo", canvas.can_redo())] {
                if let Some(a) = window
                    .lookup_action(name)
                    .and_downcast::<gio::SimpleAction>()
                {
                    a.set_enabled(on);
                }
            }
        }
    });
    window.present();
    canvas.grab_focus();
    if let Some(frames) = bench {
        bench_stroke(app, &canvas, frames);
    }
}

/// File → New Image: asks for a width and height, then opens a window with a white image
/// that size.
fn ask_new_image(app: &gtk::Application) {
    let dialog = gtk::Window::builder()
        .title("New Image")
        .modal(true)
        .resizable(false)
        .build();
    dialog.set_transient_for(app.active_window().as_ref());
    let grid = gtk::Grid::builder()
        .row_spacing(8)
        .column_spacing(8)
        .margin_start(16)
        .margin_end(16)
        .margin_top(16)
        .margin_bottom(16)
        .build();
    let (w, h) = LAST_SIZE.get();
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
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    buttons.set_margin_top(8);
    let cancel = gtk::Button::with_mnemonic("_Cancel");
    let create = gtk::Button::with_mnemonic("C_reate");
    create.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&create);
    grid.attach(&buttons, 0, 2, 3, 1);
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
            let size = (width.value() as u32, height.value() as u32);
            LAST_SIZE.set(size);
            if let Some(d) = dialog.upgrade() {
                d.close();
            }
            open_blank(&app, size);
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
