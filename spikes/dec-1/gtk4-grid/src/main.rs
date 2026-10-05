// DEC-1 spike: custom-drawn virtual grid in GTK4 (GSK snapshot + Pango). Throwaway.
#[path = "../../common.rs"]
mod common;

use common::*;
use gtk::{gdk, glib, graphene, pango, prelude::*, subclass::prelude::*};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Grid {
        pub adj: RefCell<Option<gtk::Adjustment>>,
        pub args: Cell<Option<Args>>,
        /// Shaped-text cache, same policy as egui's galley cache: entries survive one
        /// frame unused, then drop. Without it every visible cell is re-shaped per frame.
        pub cache_cur: RefCell<HashMap<String, pango::Layout>>,
        pub cache_prev: RefCell<HashMap<String, pango::Layout>>,
        pub buf: RefCell<String>,
        pub top_row: Cell<u64>,
        pub visible: Cell<(u64, u32)>,
        /// Time spent building the render tree (Pango shaping + node creation) last frame.
        pub snapshot_us: Cell<u64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Grid {
        const NAME: &'static str = "Dec1Grid";
        type Type = super::Grid;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Grid {}

    impl Grid {
        fn layout(&self, widget: &super::Grid, text: &str) -> pango::Layout {
            let mut cur = self.cache_cur.borrow_mut();
            if let Some(l) = cur.get(text) {
                return l.clone();
            }
            let l = self
                .cache_prev
                .borrow_mut()
                .remove(text)
                .unwrap_or_else(|| widget.create_pango_layout(Some(text)));
            cur.insert(text.to_owned(), l.clone());
            l
        }

        fn end_frame(&self) {
            let cur = std::mem::take(&mut *self.cache_cur.borrow_mut());
            *self.cache_prev.borrow_mut() = cur;
        }
    }

    impl WidgetImpl for Grid {
        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.parent_size_allocate(width, height, baseline);
            let args = self.args.get().unwrap();
            if let Some(adj) = self.adj.borrow().as_ref() {
                let body_h = (height as f64 - HEADER_H).max(ROW_H);
                adj.configure(
                    adj.value(),
                    0.0,
                    args.content_height(),
                    ROW_H,
                    body_h * 0.9,
                    body_h,
                );
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let widget = self.obj();
            let t0 = mono_ns();
            let args = self.args.get().unwrap();
            let adj = self.adj.borrow();
            let adj = adj.as_ref().unwrap();
            let w = widget.width() as f64;
            let h = widget.height() as f64;
            let body_h = h - HEADER_H;

            let mut buf = self.buf.borrow_mut();
            let ink = gdk::RGBA::new(0.1, 0.1, 0.12, 1.0);
            let text_at = |s: &str, x: f64, y: f64| {
                let layout = self.layout(&widget, s);
                snapshot.save();
                snapshot.translate(&graphene::Point::new(x as f32, y as f32));
                snapshot.append_layout(&layout, &ink);
                snapshot.restore();
            };

            let vp = viewport(adj.value(), body_h, args.rows);
            let ncols = (((w - ROW_HEADER_W) / COL_W).ceil().max(0.0) as u32).min(args.cols);
            self.top_row.set(vp.first_row);
            self.visible.set((vp.row_count, ncols));

            let rect = |x: f64, y: f64, w: f64, h: f64| {
                graphene::Rect::new(x as f32, y as f32, w as f32, h as f32)
            };
            let white = gdk::RGBA::WHITE;
            let stripe = gdk::RGBA::new(0.96, 0.97, 0.99, 1.0);
            let line = gdk::RGBA::new(0.85, 0.86, 0.88, 1.0);
            let header_bg = gdk::RGBA::new(0.92, 0.93, 0.95, 1.0);

            snapshot.append_color(&white, &rect(0.0, 0.0, w, h));

            // Row stripes and horizontal lines.
            for i in 0..vp.row_count {
                let y = HEADER_H + vp.y_offset + i as f64 * ROW_H;
                if (vp.first_row + i) % 2 == 1 {
                    snapshot.append_color(&stripe, &rect(0.0, y, w, ROW_H));
                }
                snapshot.append_color(&line, &rect(0.0, y + ROW_H - 1.0, w, 1.0));
            }

            // Cells, one clip per column.
            for c in 0..ncols {
                let x = ROW_HEADER_W + c as f64 * COL_W;
                snapshot.append_color(&line, &rect(x + COL_W - 1.0, HEADER_H, 1.0, body_h));
                snapshot.push_clip(&rect(x, HEADER_H, COL_W - 1.0, body_h));
                for i in 0..vp.row_count {
                    let y = HEADER_H + vp.y_offset + i as f64 * ROW_H;
                    cell_text(vp.first_row + i, c, &mut buf);
                    text_at(&buf, x + 4.0, y + 3.0);
                }
                snapshot.pop();
            }

            // Row header.
            snapshot.append_color(&header_bg, &rect(0.0, HEADER_H, ROW_HEADER_W, body_h));
            snapshot.push_clip(&rect(0.0, HEADER_H, ROW_HEADER_W, body_h));
            for i in 0..vp.row_count {
                let y = HEADER_H + vp.y_offset + i as f64 * ROW_H;
                buf.clear();
                use std::fmt::Write as _;
                let _ = write!(buf, "{}", vp.first_row + i + 1);
                text_at(&buf, 6.0, y + 3.0);
            }
            snapshot.pop();

            // Column header.
            snapshot.append_color(&header_bg, &rect(0.0, 0.0, w, HEADER_H));
            for c in 0..ncols {
                let x = ROW_HEADER_W + c as f64 * COL_W;
                col_name(c, &mut buf);
                text_at(&buf, x + COL_W / 2.0 - 6.0, 4.0);
            }
            self.end_frame();
            self.snapshot_us.set((mono_ns() - t0) / 1000);
        }
    }
}

glib::wrapper! {
    pub struct Grid(ObjectSubclass<imp::Grid>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Grid {
    fn new(args: Args, adj: &gtk::Adjustment) -> Self {
        let g: Self = glib::Object::new();
        g.imp().args.set(Some(args));
        g.imp().adj.replace(Some(adj.clone()));
        g
    }
}

fn build(app: &gtk::Application, args: Args) {
    let adj = gtk::Adjustment::new(0.0, 0.0, args.content_height(), ROW_H, 500.0, 500.0);
    let grid = Grid::new(args, &adj);
    grid.set_hexpand(true);
    grid.set_vexpand(true);
    grid.set_focusable(true);
    grid.set_overflow(gtk::Overflow::Hidden);

    let sb = gtk::Scrollbar::new(gtk::Orientation::Vertical, Some(&adj));
    let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    hbox.append(&grid);
    hbox.append(&sb);

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("dec1-gtk4")
        .default_width(1600)
        .default_height(1000)
        .child(&hbox)
        .build();

    adj.connect_value_changed({
        let grid = grid.clone();
        move |_| grid.queue_draw()
    });

    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed({
        let adj = adj.clone();
        move |_, key, _, _| {
            emit(format_args!(
                "K {} {}",
                mono_ns(),
                key.name().unwrap_or_default()
            ));
            let v = adj.value();
            let page = adj.page_size();
            let target = match key {
                gdk::Key::Page_Down => v + page,
                gdk::Key::Page_Up => v - page,
                gdk::Key::Down => v + ROW_H,
                gdk::Key::Up => v - ROW_H,
                gdk::Key::Home => 0.0,
                gdk::Key::End => adj.upper(),
                _ => return glib::Propagation::Proceed,
            };
            adj.set_value(target);
            glib::Propagation::Stop
        }
    });
    window.add_controller(keys);

    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    scroll.connect_scroll({
        let adj = adj.clone();
        move |_, _dx, dy| {
            adj.set_value(adj.value() + dy * ROW_H * 3.0);
            glib::Propagation::Stop
        }
    });
    grid.add_controller(scroll);

    window.present();
    grid.grab_focus();

    let fc = window
        .frame_clock()
        .expect("realized window has a frame clock");
    let start = Rc::new(Cell::new(0u64));
    let painted = Rc::new(Cell::new(0u32));
    fc.connect_before_paint({
        let start = start.clone();
        move |_| start.set(mono_ns())
    });
    fc.connect_after_paint({
        let grid = grid.clone();
        move |_| {
            let now = mono_ns();
            let n = painted.get() + 1;
            painted.set(n);
            if n == 1 {
                let (vr, vc) = grid.imp().visible.get();
                emit(format_args!(
                    "READY {} {} {} {}",
                    grid.width(),
                    grid.height(),
                    vr,
                    vc
                ));
            }
            let imp = grid.imp();
            emit(format_args!(
                "F {} {} {} {}",
                now,
                imp.top_row.get(),
                (now - start.get()) / 1000,
                imp.snapshot_us.get()
            ));
        }
    });

    if args.mode != Mode::Interactive {
        let frame = Cell::new(0u32);
        let app = app.clone();
        grid.add_tick_callback(move |grid, _| {
            let n = frame.get() + 1;
            frame.set(n);
            if n > args.frames {
                emit(format_args!("DONE"));
                app.quit();
                return glib::ControlFlow::Break;
            }
            let body_h = grid.height() as f64 - HEADER_H;
            adj.set_value(autopilot(args.mode, n, adj.value(), body_h, args.rows));
            glib::ControlFlow::Continue
        });
    }
}

fn main() -> glib::ExitCode {
    let args = Args::parse();
    let app = gtk::Application::builder()
        .application_id("dev.bricks.Dec1Gtk4")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| build(app, args));
    app.run_with_args::<&str>(&[])
}
