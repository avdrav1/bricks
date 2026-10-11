//! Windows: the start window, a window per deck (slide list, canvas, status bar), the
//! file dialog, drops, and alerts (spec FR0).

use crate::deck::Deck;
use crate::slide_view::SlideView;
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

const START_WINDOW: &str = "start";
const THUMB_WIDTH: i32 = 176;

thread_local! {
    /// Each open deck's window, by canonical path: opening a file again brings its
    /// window forward.
    static OPEN: RefCell<Vec<(PathBuf, glib::WeakRef<gtk::ApplicationWindow>)>> =
        const { RefCell::new(Vec::new()) };
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_owned())
}

fn window_for(path: &Path) -> Option<gtk::ApplicationWindow> {
    let path = canonical(path);
    OPEN.with_borrow_mut(|open| {
        open.retain(|(_, w)| w.upgrade().is_some());
        open.iter()
            .find(|(p, _)| *p == path)
            .and_then(|(_, w)| w.upgrade())
    })
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// The header bar every window has: an Open button (Ctrl+O).
fn header() -> gtk::HeaderBar {
    let bar = gtk::HeaderBar::new();
    let open = gtk::Button::from_icon_name("document-open-symbolic");
    open.set_action_name(Some("app.open"));
    open.set_tooltip_text(Some("Open… (Ctrl+O)"));
    open.update_property(&[gtk::accessible::Property::Label("Open")]);
    bar.pack_start(&open);
    bar
}

/// The window shown without a file: what to do to open one.
pub fn start_window(app: &gtk::Application) {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Frame")
        .default_width(720)
        .default_height(480)
        .build();
    window.set_widget_name(START_WINDOW);
    window.set_titlebar(Some(&header()));
    let page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    page.set_valign(gtk::Align::Center);
    page.set_halign(gtk::Align::Center);
    let title = gtk::Label::new(Some("Open a presentation"));
    title.add_css_class("title-1");
    let hint = gtk::Label::new(Some("Press Ctrl+O, or drop a .pptx file here."));
    hint.add_css_class("dim-label");
    let open = gtk::Button::with_label("Open…");
    open.set_action_name(Some("app.open"));
    open.set_halign(gtk::Align::Center);
    open.add_css_class("suggested-action");
    page.append(&title);
    page.append(&hint);
    page.append(&open);
    window.set_child(Some(&page));
    accept_file_drops(app, &window);
    window.present();
}

fn close_start_window(app: &gtk::Application) {
    for w in app.windows() {
        if w.widget_name() == START_WINDOW {
            w.close();
        }
    }
}

/// "Presentations" (.pptx, .pptm) and "All files", the first as the default.
fn file_filters() -> (gio::ListStore, gtk::FileFilter) {
    let pptx = gtk::FileFilter::new();
    pptx.set_name(Some("Presentations"));
    pptx.add_mime_type("application/vnd.openxmlformats-officedocument.presentationml.presentation");
    pptx.add_mime_type("application/vnd.ms-powerpoint.presentation.macroEnabled.12");
    pptx.add_suffix("pptx");
    pptx.add_suffix("pptm");
    let all = gtk::FileFilter::new();
    all.set_name(Some("All files"));
    all.add_pattern("*");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&pptx);
    filters.append(&all);
    (filters, pptx)
}

/// File → Open (Ctrl+O): the desktop's file dialog (xdg-desktop-portal through
/// `gtk::FileDialog`, GTK's own without a portal), filtered to presentations.
pub fn choose_and_open(app: &gtk::Application) {
    let (filters, pptx) = file_filters();
    let dialog = gtk::FileDialog::builder()
        .title("Open")
        .modal(true)
        .filters(&filters)
        .default_filter(&pptx)
        .build();
    let (app, parent) = (app.clone(), app.active_window());
    glib::spawn_future_local(async move {
        match dialog.open_future(parent.as_ref()).await {
            Ok(file) => match file.path() {
                Some(path) => open(&app, &path, None),
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

/// Files dropped on a window open as Open does, each in its own window. They open after
/// the drop is done: opening one may close the start window, the drop's target.
fn accept_file_drops(app: &gtk::Application, window: &gtk::ApplicationWindow) {
    let target = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
    target.connect_drop({
        let (app, window) = (app.clone(), window.downgrade());
        move |_, value, _, _| {
            let Ok(files) = value.get::<gdk::FileList>() else {
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
                        Some(path) => open(&app, &path, None),
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

fn alert(parent: Option<&gtk::Window>, message: &str, detail: &str) {
    gtk::AlertDialog::builder()
        .message(message)
        .detail(detail)
        .modal(true)
        .build()
        .show(parent);
}

/// Open `path` in its own window: its existing one, or a new one that says "Opening…"
/// while the deck is read off the UI thread. A file that can't be read gives an alert
/// naming the file and the reason, and its window closes.
pub fn open(app: &gtk::Application, path: &Path, bench: Option<u32>) {
    if let Some(window) = window_for(path) {
        window.present();
        return;
    }
    let name = file_name(path);
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title(&name)
        .default_width(1280)
        .default_height(760)
        .build();
    window.set_titlebar(Some(&header()));
    let opening = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    opening.set_halign(gtk::Align::Center);
    opening.set_valign(gtk::Align::Center);
    let spinner = gtk::Spinner::new();
    spinner.start();
    opening.append(&spinner);
    opening.append(&gtk::Label::new(Some("Opening…")));
    window.set_child(Some(&opening));
    accept_file_drops(app, &window);
    OPEN.with_borrow_mut(|open| open.push((canonical(path), window.downgrade())));
    window.present();

    let (app, path) = (app.clone(), path.to_owned());
    glib::spawn_future_local(async move {
        let p = path.clone();
        let read = gio::spawn_blocking(move || Deck::load(&p)).await;
        let error = match read {
            Ok(Ok(deck)) => {
                show_deck(&app, &window, Rc::new(deck), bench);
                close_start_window(&app);
                return;
            }
            Ok(Err(e)) => e.to_string(),
            Err(_) => "Reading it failed unexpectedly.".to_owned(),
        };
        eprintln!("frame: cannot open {}: {error}", path.display());
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

/// A deck's window once it's read: the slide list, the canvas, and the status bar.
struct DeckWindow {
    app: gtk::Application,
    window: glib::WeakRef<gtk::ApplicationWindow>,
    deck: Rc<Deck>,
    canvas: SlideView,
    list: gtk::ListBox,
    scrolled: gtk::ScrolledWindow,
    status: gtk::Label,
}

impl DeckWindow {
    fn count(&self) -> usize {
        self.deck.presentation.slides.len()
    }

    /// Show slide `index`: canvas, selected thumbnail, status, and the canvas's accessible
    /// description (the slide's text).
    fn go(&self, index: usize) {
        let n = self.count();
        if n == 0 {
            return;
        }
        let index = index.min(n - 1);
        self.canvas.set_index(index);
        self.status.set_text(&format!("Slide {} of {n}", index + 1));
        let slide = &self.deck.presentation.slides[index];
        let label = format!("Slide {}", index + 1);
        self.canvas.update_property(&[
            gtk::accessible::Property::Label(&label),
            gtk::accessible::Property::Description(&slide.text()),
        ]);
        if let Some(row) = self.list.row_at_index(index as i32) {
            if !row.is_selected() {
                self.list.select_row(Some(&row));
            }
            if let Some(b) = row.compute_bounds(&self.list) {
                let adj = self.scrolled.vadjustment();
                let (top, bottom) = (f64::from(b.y()), f64::from(b.y() + b.height()));
                if top < adj.value() {
                    adj.set_value(top);
                } else if bottom > adj.value() + adj.page_size() {
                    adj.set_value(bottom - adj.page_size());
                }
            }
        }
    }

    fn step(&self, by: isize) {
        let i = self.canvas.index() as isize + by;
        self.go(i.clamp(0, self.count().saturating_sub(1) as isize) as usize);
    }
}

/// The full-screen slideshow, from slide `start` (the next shown one when it's hidden):
/// Space, Enter, Right, Down, Page Down, and a click go forward; Left, Up, Page Up, and
/// Backspace back; Home and End to the first and last. Hidden slides are skipped. Escape,
/// or going past the last slide, ends it, back in the deck's window on the slide shown.
fn slideshow(view: &Rc<DeckWindow>, start: usize) {
    let slides = &view.deck.presentation.slides;
    let shown: Rc<Vec<usize>> = Rc::new((0..slides.len()).filter(|&i| !slides[i].hidden).collect());
    if shown.is_empty() {
        return;
    }
    let first = shown
        .iter()
        .position(|&i| i >= start)
        .unwrap_or(shown.len() - 1);
    let deck_window = view.window.upgrade();
    let show = gtk::Window::builder()
        .application(&view.app)
        .title(format!(
            "Slideshow: {}",
            deck_window
                .as_ref()
                .and_then(|w| w.title())
                .unwrap_or_default()
        ))
        .decorated(false)
        .build();
    show.set_transient_for(deck_window.as_ref());
    let canvas = SlideView::new(&view.deck, shown[first], 0);
    // Black around the slide where its shape differs from the screen's.
    canvas.set_backdrop(gdk::RGBA::BLACK);
    show.set_child(Some(&canvas));
    let at = Rc::new(Cell::new(first));
    let set = {
        let (canvas, shown, deck, at) =
            (canvas.clone(), shown.clone(), view.deck.clone(), at.clone());
        move |pos: usize| {
            at.set(pos);
            let index = shown[pos];
            canvas.set_index(index);
            canvas.update_property(&[
                gtk::accessible::Property::Label(&format!("Slideshow slide {}", index + 1)),
                gtk::accessible::Property::Description(&deck.presentation.slides[index].text()),
            ]);
        }
    };
    set(first);
    let set = Rc::new(set);
    let forward = {
        let (set, at, show, n) = (set.clone(), at.clone(), show.downgrade(), shown.len());
        move || {
            if at.get() + 1 < n {
                set(at.get() + 1);
            } else if let Some(show) = show.upgrade() {
                show.close();
            }
        }
    };
    let forward = Rc::new(forward);
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed({
        let (set, at, show, forward, n) = (
            set.clone(),
            at.clone(),
            show.downgrade(),
            forward.clone(),
            shown.len(),
        );
        move |_, key, _, _| {
            use gdk::Key;
            match key {
                Key::space
                | Key::Return
                | Key::KP_Enter
                | Key::Right
                | Key::KP_Right
                | Key::Down
                | Key::KP_Down
                | Key::Page_Down
                | Key::KP_Page_Down => forward(),
                Key::Left
                | Key::KP_Left
                | Key::Up
                | Key::KP_Up
                | Key::Page_Up
                | Key::KP_Page_Up
                | Key::BackSpace => set(at.get().saturating_sub(1)),
                Key::Home | Key::KP_Home => set(0),
                Key::End | Key::KP_End => set(n - 1),
                Key::Escape => {
                    if let Some(show) = show.upgrade() {
                        show.close();
                    }
                }
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        }
    });
    show.add_controller(keys);
    let click = gtk::GestureClick::new();
    click.connect_released(move |_, _, _, _| forward());
    show.add_controller(click);
    // However it ends, the deck's window comes back on the slide the show was on.
    show.connect_close_request({
        let (view, shown, at) = (view.clone(), shown.clone(), at.clone());
        move |_| {
            view.go(shown[at.get()]);
            if let Some(w) = view.window.upgrade() {
                w.present();
            }
            glib::Propagation::Proceed
        }
    });
    show.fullscreen();
    show.present();
}

fn thumbnail_row(deck: &Rc<Deck>, index: usize) -> gtk::ListBoxRow {
    let slide = &deck.presentation.slides[index];
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    content.set_margin_top(6);
    content.set_margin_bottom(6);
    content.set_margin_start(4);
    content.set_margin_end(8);
    let number = gtk::Label::new(Some(&(index + 1).to_string()));
    number.set_valign(gtk::Align::Start);
    number.set_width_chars(3);
    number.set_xalign(1.0);
    let thumb = SlideView::new(deck, index, THUMB_WIDTH);
    let label = if slide.hidden {
        format!("Slide {} (hidden)", index + 1)
    } else {
        format!("Slide {}", index + 1)
    };
    thumb.update_property(&[gtk::accessible::Property::Label(&label)]);
    content.append(&number);
    content.append(&thumb);
    if slide.hidden {
        // Hidden slides show dimmed; the slideshow skips them.
        content.set_opacity(0.4);
        content.set_tooltip_text(Some("Hidden slide"));
    }
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&content));
    row
}

fn show_deck(
    app: &gtk::Application,
    window: &gtk::ApplicationWindow,
    deck: Rc<Deck>,
    bench: Option<u32>,
) {
    let canvas = SlideView::new(&deck, 0, 0);
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Single);
    list.add_css_class("navigation-sidebar");
    list.update_property(&[gtk::accessible::Property::Label("Slides")]);
    for i in 0..deck.presentation.slides.len() {
        list.append(&thumbnail_row(&deck, i));
    }
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&list)
        .width_request(THUMB_WIDTH + 64)
        .build();
    let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
    paned.set_start_child(Some(&scrolled));
    paned.set_end_child(Some(&canvas));
    paned.set_shrink_start_child(false);
    paned.set_resize_start_child(false);
    paned.set_vexpand(true);
    let status = gtk::Label::new(None);
    status.set_xalign(0.0);
    status.set_margin_start(10);
    status.set_margin_end(10);
    status.set_margin_top(4);
    status.set_margin_bottom(4);
    let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    page.append(&paned);
    page.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    page.append(&status);
    window.set_child(Some(&page));

    let view = Rc::new(DeckWindow {
        app: app.clone(),
        window: window.downgrade(),
        deck: deck.clone(),
        canvas: canvas.clone(),
        list: list.clone(),
        scrolled,
        status: status.clone(),
    });
    if view.count() == 0 {
        status.set_text("No slides");
    }
    list.connect_row_selected({
        let view = Rc::downgrade(&view);
        move |_, row| {
            if let (Some(view), Some(row)) = (view.upgrade(), row) {
                let i = row.index() as usize;
                if i != view.canvas.index() {
                    view.go(i);
                }
            }
        }
    });
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed({
        let view = view.clone();
        move |_, key, _, mods| {
            if mods.intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK) {
                return glib::Propagation::Proceed;
            }
            use gdk::Key;
            match key {
                Key::Page_Down
                | Key::KP_Page_Down
                | Key::Down
                | Key::KP_Down
                | Key::Right
                | Key::KP_Right => view.step(1),
                Key::Page_Up
                | Key::KP_Page_Up
                | Key::Up
                | Key::KP_Up
                | Key::Left
                | Key::KP_Left => view.step(-1),
                Key::Home | Key::KP_Home => view.go(0),
                Key::End | Key::KP_End => view.go(view.count().saturating_sub(1)),
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        }
    });
    window.add_controller(keys);
    // F5 shows the slideshow from the first slide, Shift+F5 from the current one.
    let from_start = gio::ActionEntry::builder("slideshow")
        .activate({
            let view = view.clone();
            move |_: &gtk::ApplicationWindow, _, _| slideshow(&view, 0)
        })
        .build();
    let from_here = gio::ActionEntry::builder("slideshow-here")
        .activate({
            let view = view.clone();
            move |_: &gtk::ApplicationWindow, _, _| slideshow(&view, view.canvas.index())
        })
        .build();
    window.add_action_entries([from_start, from_here]);
    if let Some(bar) = window.titlebar().and_downcast::<gtk::HeaderBar>() {
        let play = gtk::Button::from_icon_name("media-playback-start-symbolic");
        play.set_action_name(Some("win.slideshow"));
        play.set_tooltip_text(Some("Slideshow (F5)"));
        play.update_property(&[gtk::accessible::Property::Label("Slideshow")]);
        bar.pack_end(&play);
    }
    view.go(0);
    if let Some(frames) = bench {
        bench_draw(app, &view, frames);
    }
}

/// FRAME-4's drawing benchmark: shows the deck's densest slide and redraws the canvas
/// every frame, measuring each frame's CPU time (frame clock before- to after-paint) and
/// the canvas snapshot's share of it; prints a JSON line and quits.
fn bench_draw(app: &gtk::Application, view: &Rc<DeckWindow>, frames: u32) {
    let index = view.deck.densest_slide();
    view.go(index);
    let canvas = view.canvas.clone();
    let Some(clock) = canvas.frame_clock() else {
        // Not realized yet: try again once it is.
        let (app, again) = (app.clone(), view.clone());
        view.canvas
            .connect_realize(move |_| bench_draw(&app, &again, frames));
        return;
    };
    #[derive(Default)]
    struct Rec {
        on: bool,
        start: Option<Instant>,
        cpu_ms: Vec<f64>,
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
    let (app, view) = (app.clone(), view.clone());
    canvas.add_tick_callback(move |canvas, _| {
        let n = ticks.get();
        ticks.set(n + 1);
        if n == 30 {
            rec.borrow_mut().on = true;
        } else if n > 30 + frames {
            let r = rec.borrow();
            let slide = &view.deck.presentation.slides[index];
            println!(
                r#"{{"slide":{},"shapes":{},"canvas":[{},{}],"frames":{},"frame_cpu_ms":{},"snapshot_ms":{}}}"#,
                index + 1,
                slide.shapes.len(),
                canvas.width(),
                canvas.height(),
                r.cpu_ms.len(),
                summary(&r.cpu_ms),
                summary(&r.snapshot_ms),
            );
            app.quit();
            return glib::ControlFlow::Break;
        }
        canvas.queue_draw();
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
