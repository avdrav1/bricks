// SLATE-5 spike: the GtkTextView editing engine. Throwaway.
//
// A read-write `gtk::TextView` (wrap WordChar) in a `ScrolledWindow`, the common.rs document
// loaded into its `TextBuffer` with one tag per look, the column centred by the view's
// left/right margins (recomputed on resize and width change; the bullet indent tags follow
// the margin), as `apps/slate/src/view.rs` `Column::fit` does.
//
// The L detector (whole document laid out at the current width). GtkTextView lays out
// ("validates") the visible lines before a frame (first_validate idle, priority
// GTK_PRIORITY_RESIZE - 2, or synchronously in size_allocate) and the rest of the buffer in
// `incremental_validate_callback`, an idle source at GTK_TEXT_VIEW_PRIORITY_VALIDATE (125)
// that validates ~2000 px of lines per dispatch, updates the adjustments synchronously
// (`gtk_text_view_update_adjustments` -> vadjustment upper), and removes itself only once
// `gtk_text_layout_is_valid` (gtktextview.c in GTK 4.24.1, lines 4989-5038). Any invalidation
// (margin change, tag change, edit) re-installs that idle synchronously, from the "invalidated"
// handler. GLib only dispatches the highest-priority ready sources in a main loop iteration,
// and an attached idle is always ready, so an idle of ours at G_PRIORITY_DEFAULT_IDLE (200)
// cannot run while the validate idle (125) is attached: when ours runs, the layout is valid
// and the vadjustment's upper is final. We arm it from a frame's after-paint that was drawn
// at the current column width (so the width change has certainly invalidated the layout);
// if the width changes again before it runs, it is re-armed. With SLATE5_CHECK_L=1 the spike
// logs every vadjustment upper change ("U <ns> <upper>") and, at L, the end of the last line
// from `line_yrange` ("LCHECK ...") to stderr, so measure.py --check-l can verify that no
// upper change follows an L.
//
// Extra knobs (environment, so common.rs's Args stay shared):
//   SLATE5_TABLES=N   embed N 3x3 tables (gtk::Grid of TextViews in child anchors)
//   SLATE5_PROBE=1    print qualitative API facts (undo, signals, a11y, tables) and quit
//   SLATE5_CHECK_L=1  see above
//
// What a frame shows. A width change or an edit only invalidates GtkTextView's layout; the
// view re-validates the visible lines in an idle after that frame or in its own paint
// (`gtk_text_view_paint` flushes the pending first validation before drawing). A frame in
// which the view was not re-snapshotted still shows the old text, so F reports the revision
// and column width as of the view's last `snapshot` (a TextView subclass records them),
// not the current ones.
#[path = "../../common.rs"]
mod common;

use common::*;
use gtk::{gdk, glib, pango, prelude::*, subclass::prelude::*};
use std::cell::Cell;
use std::rc::Rc;

thread_local! {
    /// (revision, column width): current, and as of the view's last snapshot.
    static CURRENT: Cell<(u64, i32)> = const { Cell::new((0, 0)) };
    static PAINTED: Cell<(u64, i32)> = const { Cell::new((0, 0)) };
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct SpikeView;

    #[glib::object_subclass]
    impl ObjectSubclass for SpikeView {
        const NAME: &'static str = "Slate5SpikeView";
        type Type = super::SpikeView;
        type ParentType = gtk::TextView;
    }

    impl ObjectImpl for SpikeView {}

    impl WidgetImpl for SpikeView {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            // The state before drawing: the parent's paint validates against it.
            let state = CURRENT.get();
            self.parent_snapshot(snapshot);
            PAINTED.set(state);
        }
    }

    impl TextViewImpl for SpikeView {}
}

glib::wrapper! {
    pub struct SpikeView(ObjectSubclass<imp::SpikeView>)
        @extends gtk::TextView, gtk::Widget,
        @implements gtk::Accessible, gtk::AccessibleText, gtk::Buildable, gtk::ConstraintTarget, gtk::Scrollable;
}

const BOLD: i32 = 700;
const MIN_MARGIN: i32 = 24;
const TABLE_COLUMNS: i32 = 3;

const CSS: &str = "
window, scrolledwindow, textview, textview > text { background-color: #ffffff; color: #000000; }
grid.table { border: 1px solid #c0c0c8; }
textview.cell { border: 1px solid #c0c0c8; padding: 3px 6px; }
";

/// The column: the view, the tags whose left margin follows the view's, the table cells.
struct Column {
    view: gtk::TextView,
    margin_tags: Vec<(gtk::TextTag, i32)>,
    cells: Vec<gtk::TextView>,
    setting: Cell<TextWidth>,
    view_w: Cell<i32>,
    column_w: Cell<i32>,
    /// Bumped on every column width change; the L detector reports a generation once.
    generation: Cell<u64>,
}

impl Column {
    /// Centre the column in a view `view_w` px wide. Only touches the view and the tags when
    /// the margin changes: setting a tag's margin, even to its value, invalidates its text.
    fn fit(&self, view_w: i32) {
        self.view_w.set(view_w);
        let col = self.setting.get().column(view_w);
        let old = self.column_w.get();
        if old == col {
            return;
        }
        if old != 0 {
            emit(format_args!("W {} {}", mono_ns(), col));
        }
        self.column_w.set(col);
        CURRENT.set((CURRENT.get().0, col));
        self.generation.set(self.generation.get() + 1);
        let margin = ((view_w - col) / 2).max(MIN_MARGIN);
        if self.view.left_margin() != margin {
            self.view.set_left_margin(margin);
            for (tag, extra) in &self.margin_tags {
                tag.set_left_margin(margin + extra);
            }
        }
        if self.view.right_margin() != view_w - col - margin {
            self.view.set_right_margin(view_w - col - margin);
        }
        let cell_w = (col / TABLE_COLUMNS).max(24);
        for c in &self.cells {
            c.set_size_request(cell_w, -1);
        }
    }

    /// Whether fitting a view `view_w` px wide would invalidate the layout (new margins, or
    /// table cells resized), which GtkTextView must not see during its own size_allocate.
    fn invalidates(&self, view_w: i32) -> bool {
        let col = self.setting.get().column(view_w);
        let margin = ((view_w - col) / 2).max(MIN_MARGIN);
        self.view.left_margin() != margin
            || self.view.right_margin() != view_w - col - margin
            || (!self.cells.is_empty() && col != self.column_w.get())
    }

    fn set_text_width(&self, w: TextWidth) {
        self.setting.set(w);
        let vw = self.view_w.get();
        if vw > 0 {
            self.fit(vw);
        }
    }
}

struct Loaded {
    margin_tags: Vec<(gtk::TextTag, i32)>,
    chars: usize,
}

/// Fill `buffer` with `doc`: one insert of the whole text, then the tags applied by range.
fn load(buffer: &gtk::TextBuffer, doc: &[Paragraph]) -> Loaded {
    let table = buffer.tag_table();
    let tag = |name: &str, f: &dyn Fn(&gtk::TextTag)| {
        let t = gtk::TextTag::new(Some(name));
        f(&t);
        table.add(&t);
        t
    };
    let para = tag("para", &|t| t.set_pixels_below_lines(PARAGRAPH_GAP));
    let heads: Vec<gtk::TextTag> = (1..=3u8)
        .map(|l| {
            tag(&format!("h{l}"), &|t| {
                t.set_scale(heading_scale(l));
                t.set_weight(BOLD);
            })
        })
        .collect();
    let bold = tag("bold", &|t| t.set_weight(BOLD));
    let italic = tag("italic", &|t| t.set_style(pango::Style::Italic));
    let bullets: Vec<gtk::TextTag> = (0..2)
        .map(|l| {
            tag(&format!("bullet{l}"), &|t| {
                // GtkTextView (Pango) treats a negative indent as a hanging indent: the
                // first line starts at the margin (the marker at 28 px per level), wrapped
                // lines and the text after the tab 28 px further in.
                t.set_left_margin(MIN_MARGIN + LEVEL_INDENT * l);
                t.set_indent(-LEVEL_INDENT);
                // The text after the marker starts where the wrapped lines do.
                let mut tabs = pango::TabArray::new(1, true);
                tabs.set_tab(0, pango::TabAlign::Left, LEVEL_INDENT);
                t.set_tabs(Some(&tabs));
            })
        })
        .collect();

    let total: usize = doc.iter().map(|p| p.text.len() + 3).sum();
    let mut text = String::with_capacity(total);
    // (start char, end char, tag)
    let mut ranges: Vec<(i32, i32, &gtk::TextTag)> = Vec::new();
    let mut off: i32 = 0;
    let mut chars = 0usize;
    for (i, p) in doc.iter().enumerate() {
        if i > 0 {
            text.push('\n');
            off += 1;
        }
        let start = off;
        if let Kind::Bullet(_) = p.kind {
            text.push_str("•\t");
            off += 2;
        }
        let mut byte = 0;
        for &(s, e, look) in &p.spans {
            off += p.text[byte..s].chars().count() as i32;
            let cs = off;
            off += p.text[s..e].chars().count() as i32;
            byte = e;
            let t = match look {
                Look::Bold => &bold,
                Look::Italic => &italic,
            };
            ranges.push((cs, off, t));
        }
        off += p.text[byte..].chars().count() as i32;
        chars += p.text.chars().count();
        text.push_str(&p.text);
        match p.kind {
            Kind::Heading(l) => ranges.push((start, off, &heads[usize::from(l.clamp(1, 3)) - 1])),
            Kind::Bullet(l) => ranges.push((start, off, &bullets[usize::from(l.min(1))])),
            Kind::Body => {}
        }
    }
    let t0 = mono_ns();
    buffer.insert(&mut buffer.end_iter(), &text);
    let t1 = mono_ns();
    let (s, e) = buffer.bounds();
    buffer.apply_tag(&para, &s, &e);
    for (s, e, t) in &ranges {
        buffer.apply_tag(*t, &buffer.iter_at_offset(*s), &buffer.iter_at_offset(*e));
    }
    eprintln!(
        "load: insert {} ms, {} tag ranges {} ms",
        (t1 - t0) / 1_000_000,
        ranges.len(),
        (mono_ns() - t1) / 1_000_000
    );
    Loaded {
        margin_tags: bullets
            .iter()
            .enumerate()
            .map(|(l, t)| (t.clone(), LEVEL_INDENT * l as i32))
            .collect(),
        chars,
    }
}

/// A 3x3 table of editable text views, like Slate's embedded tables.
fn table(n: usize, cells: &mut Vec<gtk::TextView>) -> gtk::Grid {
    let grid = gtk::Grid::builder().column_homogeneous(true).build();
    grid.add_css_class("table");
    for r in 0..3 {
        for c in 0..TABLE_COLUMNS {
            let v = gtk::TextView::builder()
                .wrap_mode(gtk::WrapMode::WordChar)
                .css_classes(["cell"])
                .build();
            v.buffer()
                .set_text(&format!("Table {n} row {r} column {c}: quartz rhythm"));
            grid.attach(&v, c, r, 1, 1);
            cells.push(v);
        }
    }
    grid
}

fn build(app: &gtk::Application, args: Args) {
    let t_build = mono_ns();
    let doc = document(args.words);
    let t_doc = mono_ns();
    let view: gtk::TextView = glib::Object::builder::<SpikeView>()
        .property("editable", true)
        .property("wrap-mode", gtk::WrapMode::WordChar)
        .build()
        .upcast();
    let buffer = view.buffer();
    // Our document model keeps its own command undo (ADR 0003); the buffer's would be a
    // second, disagreeing history.
    buffer.set_enable_undo(false);
    let loaded = load(&buffer, &doc);

    let target = ((args.at * (doc.len() - 1) as f64).round() as i32).max(0);
    let cursor_iter = buffer.iter_at_line(target).unwrap_or_else(|| buffer.start_iter());
    let cursor_mark = buffer.create_mark(None, &cursor_iter, true);

    let mut cells = Vec::new();
    let tables: usize = std::env::var("SLATE5_TABLES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    // Last to first so earlier line numbers stay valid.
    for k in (0..tables).rev() {
        let line = ((k + 1) * doc.len() / (tables + 1)) as i32;
        let mut it = buffer.iter_at_line(line).unwrap();
        let anchor = buffer.create_child_anchor(&mut it);
        buffer.insert(&mut it, "\n");
        view.add_child_at_anchor(&table(k, &mut cells), &anchor);
    }
    buffer.place_cursor(&buffer.iter_at_mark(&cursor_mark));
    buffer.delete_mark(&cursor_mark);
    eprintln!(
        "build: document {} ms, buffer {} ms, {} tables",
        (t_doc - t_build) / 1_000_000,
        (mono_ns() - t_doc) / 1_000_000,
        tables
    );

    let column = Rc::new(Column {
        view: view.clone(),
        margin_tags: loaded.margin_tags,
        cells,
        setting: Cell::new(args.width),
        view_w: Cell::new(0),
        column_w: Cell::new(0),
        generation: Cell::new(0),
    });
    view.set_left_margin(MIN_MARGIN);
    view.set_right_margin(MIN_MARGIN);

    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&view)
        .build();
    scrolled.hadjustment().connect_page_size_notify({
        let column = column.clone();
        move |adj| {
            // page-size changes inside GtkTextView's size_allocate (validate_onscreen ->
            // update_adjustments); invalidating there trips its `onscreen_validated`
            // assertion, so changes that invalidate wait until the frame is done (a window
            // resize at a fixed text width re-wraps one frame late).
            let w = adj.page_size() as i32;
            if w <= 0 {
                return; // not allocated yet
            }
            if column.invalidates(w) {
                let column = column.clone();
                glib::idle_add_local_full(glib::Priority::HIGH, move || {
                    column.fit(w);
                    glib::ControlFlow::Break
                });
            } else {
                column.fit(w);
            }
        }
    });
    let vadj = scrolled.vadjustment();

    let provider = gtk::CssProvider::new();
    provider.load_from_string(&format!(
        "{CSS}\ntextview {{ font-family: Sans; font-size: {BODY_PT}pt; }}\n"
    ));
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().unwrap(),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    // measure.py opens the window at the measured size (its Hyprland rule floats it there),
    // so READY and the first L are at that size, not at the default's and then a resize.
    let win = std::env::var("SLATE5_WINDOW")
        .ok()
        .and_then(|v| {
            let (w, h) = v.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or((1600, 1000));
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("textview")
        .default_width(win.0)
        .default_height(win.1)
        .child(&scrolled)
        .build();

    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed({
        let column = column.clone();
        move |_, key, _, state| {
            emit(format_args!(
                "K {} {}",
                mono_ns(),
                key.name().unwrap_or_default()
            ));
            if state.contains(gdk::ModifierType::CONTROL_MASK) {
                if let Some(w) = key
                    .to_unicode()
                    .and_then(|c| c.to_digit(10))
                    .and_then(TextWidth::from_key)
                {
                    column.set_text_width(w);
                    return glib::Propagation::Stop;
                }
            }
            glib::Propagation::Proceed
        }
    });
    window.add_controller(keys);

    let revision = Rc::new(Cell::new(0u64));
    buffer.connect_changed({
        let revision = revision.clone();
        move |_| {
            let r = revision.get() + 1;
            revision.set(r);
            CURRENT.set((r, CURRENT.get().1));
            emit(format_args!("E {} {}", mono_ns(), r));
        }
    });

    let check_l = std::env::var_os("SLATE5_CHECK_L").is_some();
    if check_l {
        vadj.connect_upper_notify(|a| eprintln!("U {} {}", mono_ns(), a.upper()));
    }

    view.scroll_to_mark(&buffer.get_insert(), 0.0, true, 0.0, 0.0);
    window.present();
    view.grab_focus();

    let fc = window.frame_clock().expect("realized window has a frame clock");
    let start = Rc::new(Cell::new(0u64));
    let painted = Rc::new(Cell::new(0u64));
    // The generation an L was emitted for, and whether a detector idle is pending.
    let l_done = Rc::new(Cell::new(0u64));
    let l_armed = Rc::new(Cell::new(false));
    fc.connect_before_paint({
        let start = start.clone();
        move |_| start.set(mono_ns())
    });
    let paragraphs = doc.len();
    fc.connect_after_paint({
        let column = column.clone();
        let window = window.clone();
        let vadj = vadj.clone();
        let buffer = buffer.clone();
        move |_| {
            let now = mono_ns();
            let n = painted.get() + 1;
            painted.set(n);
            if n == 1 {
                eprintln!(
                    "renderer: {:?}",
                    window.renderer().map(|r| r.type_().name().to_string())
                );
                emit(format_args!(
                    "READY {} {} {} {} {}",
                    window.width(),
                    window.height(),
                    column.column_w.get(),
                    paragraphs,
                    loaded.chars
                ));
            }
            emit(format_args!(
                "F {} {} {} {} {:.0}",
                now,
                PAINTED.get().0,
                PAINTED.get().1,
                (now - start.get()) / 1000,
                vadj.value()
            ));
            let gen = column.generation.get();
            if gen != 0 && l_done.get() != gen && !l_armed.get() {
                l_armed.set(true);
                let column = column.clone();
                let vadj = vadj.clone();
                let buffer = buffer.clone();
                let l_done = l_done.clone();
                let l_armed = l_armed.clone();
                glib::idle_add_local_full(glib::Priority::DEFAULT_IDLE, move || {
                    l_armed.set(false);
                    if column.generation.get() == gen {
                        l_done.set(gen);
                        let t = mono_ns();
                        emit(format_args!(
                            "L {} {} {:.0}",
                            t,
                            column.column_w.get(),
                            vadj.upper()
                        ));
                        if check_l {
                            let (y, h) = column.view.line_yrange(&buffer.end_iter());
                            eprintln!("LCHECK {} {} {} {}", t, vadj.upper(), y, h);
                        }
                        if gen == 1 {
                            // The cursor was scrolled to on estimated line heights; now
                            // that they are exact, put its paragraph back at the top.
                            column.view.scroll_to_mark(&buffer.get_insert(), 0.0, true, 0.0, 0.0);
                        }
                    }
                    glib::ControlFlow::Break
                });
            }
        }
    });

    if args.mode != Mode::Interactive {
        let frame = Cell::new(0u32);
        let app = app.clone();
        let column = column.clone();
        view.add_tick_callback(move |_, _| {
            let n = frame.get() + 1;
            frame.set(n);
            if n > args.frames {
                emit(format_args!("DONE"));
                app.quit();
                return glib::ControlFlow::Break;
            }
            match args.mode {
                Mode::Scroll => {
                    let v = vadj.value() + 37.0;
                    vadj.set_value(if v > vadj.upper() - vadj.page_size() { 0.0 } else { v });
                }
                Mode::Rewrap if n % 90 == 0 => {
                    let next = if column.setting.get() == TextWidth::Narrow {
                        TextWidth::Full
                    } else {
                        TextWidth::Narrow
                    };
                    column.set_text_width(next);
                }
                _ => {}
            }
            glib::ControlFlow::Continue
        });
    }
}

/// Print API facts the ADR needs, checked against this GTK at run time.
fn probe() {
    let say = |k: &str, v: String| println!("PROBE {k}: {v}");

    // Undo.
    let b = gtk::TextBuffer::new(None);
    say("undo enabled by default", b.enables_undo().to_string());
    b.insert(&mut b.end_iter(), "loaded");
    say("can_undo after programmatic insert", b.can_undo().to_string());
    b.insert_interactive_at_cursor("typed", true);
    say("can_undo after interactive insert", b.can_undo().to_string());
    b.undo();
    say("text after one undo", format!("{:?}", b.text(&b.start_iter(), &b.end_iter(), true)));
    let b2 = gtk::TextBuffer::new(None);
    b2.set_enable_undo(false);
    b2.insert_interactive_at_cursor("typed", true);
    say("can_undo with enable_undo=false", b2.can_undo().to_string());

    // Edit signals: what a bridge into model commands would see.
    let b = gtk::TextBuffer::new(None);
    b.set_text("hello world");
    b.connect_insert_text(|_, it, text| println!("PROBE signal insert-text: offset {} text {:?}", it.offset(), text));
    b.connect_delete_range(|_, s, e| println!("PROBE signal delete-range: {}..{}", s.offset(), e.offset()));
    b.connect_apply_tag(|_, t, s, e| println!("PROBE signal apply-tag: {:?} {}..{}", t.name(), s.offset(), e.offset()));
    b.place_cursor(&b.iter_at_offset(5));
    b.insert_interactive_at_cursor("!", true);
    let mut it = b.iter_at_offset(6);
    b.backspace(&mut it, true, true);
    b.select_range(&b.iter_at_offset(0), &b.iter_at_offset(5));
    b.delete_selection(true, true);
    let bold = gtk::TextTag::new(Some("bold"));
    b.tag_table().add(&bold);
    b.apply_tag(&bold, &b.iter_at_offset(0), &b.iter_at_offset(3));

    // Accessibility and input method.
    let view = gtk::TextView::new();
    say("TextView implements GtkAccessibleText", view.is::<gtk::AccessibleText>().to_string());
    say("TextView accessible role", format!("{:?}", view.accessible_role()));
    say("TextView im-module (None = GtkIMMulticontext default)", format!("{:?}", view.im_module()));
    say("TextView input-purpose", format!("{:?}", view.input_purpose()));

    // Selection across an embedded table.
    let b = view.buffer();
    b.set_text("before\n");
    let anchor = b.create_child_anchor(&mut b.end_iter());
    b.insert(&mut b.end_iter(), "\nafter");
    let mut cells = Vec::new();
    view.add_child_at_anchor(&table(0, &mut cells), &anchor);
    let (s, e) = b.bounds();
    b.select_range(&s, &e);
    let (s, e) = b.selection_bounds().unwrap();
    say("select-all text() over a table", format!("{:?}", b.text(&s, &e, true)));
    say("select-all slice() over a table", format!("{:?}", b.slice(&s, &e, true)));
    say("table cell has its own buffer", (cells[0].buffer() != b).to_string());
}

fn main() -> glib::ExitCode {
    let args = Args::parse();
    let app = gtk::Application::builder()
        .application_id("dev.cronkite.Slate5TextView")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| {
        if std::env::var_os("SLATE5_PROBE").is_some() {
            probe();
            app.quit();
            return;
        }
        build(app, args)
    });
    app.run_with_args::<&str>(&[])
}
