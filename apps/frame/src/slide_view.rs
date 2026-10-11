//! `SlideView`, the widget that shows one slide through [`crate::render`]: the canvas
//! (scaled to fit, letterboxed), a thumbnail in the slide list (a fixed width), and the
//! slideshow (on black).

use crate::deck::Deck;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib, graphene, pango};
use std::rc::Rc;
use std::time::Instant;

mod imp {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[derive(Default)]
    pub struct SlideView {
        pub deck: RefCell<Option<Rc<Deck>>>,
        pub index: Cell<usize>,
        /// A thumbnail's width; 0 for a view that fills what it's given.
        pub thumb_width: Cell<i32>,
        /// What fills the space around the slide; `None` for the theme's (a tint of its
        /// foreground colour).
        pub backdrop: Cell<Option<gdk::RGBA>>,
        pub pango: RefCell<Option<pango::Context>>,
        /// How long the last snapshot took to build, in milliseconds (the drawing
        /// benchmark reads it).
        pub last_ms: Cell<f64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SlideView {
        const NAME: &'static str = "FrameSlideView";
        type Type = super::SlideView;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for SlideView {}

    impl WidgetImpl for SlideView {
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            let tw = self.thumb_width.get();
            if tw == 0 {
                return (1, 640, -1, -1);
            }
            let size = self
                .deck
                .borrow()
                .as_ref()
                .map(|d| d.presentation.size)
                .filter(|s| s.cx > 0 && s.cy > 0);
            let n = match orientation {
                gtk::Orientation::Horizontal => tw,
                _ => size.map_or(tw * 9 / 16, |s| {
                    (f64::from(tw) * s.cy as f64 / s.cx as f64).round() as i32
                }),
            };
            (n, n, -1, -1)
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let started = Instant::now();
            let obj = self.obj();
            let (w, h) = (obj.width() as f32, obj.height() as f32);
            let bounds = graphene::Rect::new(0.0, 0.0, w, h);
            let backdrop = self.backdrop.get().unwrap_or_else(|| {
                let fg = obj.color();
                gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), 0.12)
            });
            if self.thumb_width.get() == 0 {
                snapshot.append_color(&backdrop, &bounds);
            }
            let deck = self.deck.borrow();
            let Some(deck) = deck.as_ref() else {
                return;
            };
            let Some(slide) = deck.presentation.slides.get(self.index.get()) else {
                return;
            };
            let size = deck.presentation.size;
            if size.cx <= 0 || size.cy <= 0 {
                return;
            }
            // Scale to fit, centred: letterboxed. The window's canvas keeps a margin
            // around the slide; thumbnails and the slideshow use all they have.
            let margin = if self.thumb_width.get() == 0 && self.backdrop.get().is_none() {
                24.0f32.min(w / 8.0).min(h / 8.0)
            } else {
                0.0
            };
            let scale =
                ((w - 2.0 * margin) / size.cx as f32).min((h - 2.0 * margin) / size.cy as f32);
            if scale <= 0.0 {
                return;
            }
            let (sw, sh) = (size.cx as f32 * scale, size.cy as f32 * scale);
            let (x, y) = (((w - sw) / 2.0).floor(), ((h - sh) / 2.0).floor());
            let ctx = self
                .pango
                .borrow_mut()
                .get_or_insert_with(|| {
                    let c = obj.create_pango_context();
                    // Fractional glyph positions: text scales smoothly with the slide.
                    c.set_round_glyph_positions(false);
                    c
                })
                .clone();
            snapshot.save();
            snapshot.translate(&graphene::Point::new(x, y));
            crate::render::draw_slide(snapshot, &ctx, deck, slide, scale);
            if self.backdrop.get().is_none() {
                // A hairline edge, so a white slide shows against a light theme.
                let edge = gdk::RGBA::new(backdrop.red(), backdrop.green(), backdrop.blue(), 0.35);
                let r =
                    gtk::gsk::RoundedRect::from_rect(graphene::Rect::new(0.0, 0.0, sw, sh), 0.0);
                snapshot.append_border(&r, &[1.0; 4], &[edge; 4]);
            }
            snapshot.restore();
            self.last_ms.set(started.elapsed().as_secs_f64() * 1e3);
        }
    }
}

glib::wrapper! {
    pub struct SlideView(ObjectSubclass<imp::SlideView>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl SlideView {
    /// A view of `deck`'s slide `index`; `thumb_width` 0 fills its space, otherwise it's a
    /// thumbnail that wide.
    pub fn new(deck: &Rc<Deck>, index: usize, thumb_width: i32) -> SlideView {
        let view: SlideView = glib::Object::builder()
            .property("accessible-role", gtk::AccessibleRole::Img)
            .build();
        let imp = view.imp();
        imp.deck.replace(Some(deck.clone()));
        imp.index.set(index);
        imp.thumb_width.set(thumb_width);
        if thumb_width == 0 {
            view.set_hexpand(true);
            view.set_vexpand(true);
        }
        view
    }

    pub fn index(&self) -> usize {
        self.imp().index.get()
    }

    /// Show slide `index`.
    pub fn set_index(&self, index: usize) {
        self.imp().index.set(index);
        self.queue_draw();
    }

    pub fn set_backdrop(&self, color: gdk::RGBA) {
        self.imp().backdrop.set(Some(color));
        self.queue_draw();
    }

    /// Milliseconds the last snapshot took to build.
    pub fn last_draw_ms(&self) -> f64 {
        self.imp().last_ms.get()
    }
}
