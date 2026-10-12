//! `SlideView`, the widget that shows one slide through [`crate::render`]: the canvas
//! (scaled to fit, letterboxed), a thumbnail in the slide list (a fixed width), and the
//! slideshow (on black). The canvas also shows the text being edited: its box, the
//! selection, and the caret ([`Editing`]), and outlines empty placeholders.

use crate::deck::Deck;
use crate::render::{self, LocalPoint, PlacedText};
use deck_edit::{Guide, TextPos};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib, graphene, pango};
use presentation_model::Frame;
use std::rc::Rc;
use std::time::Instant;

/// The text being edited on the canvas: a shape of the slide shown, and the selection
/// from `anchor` to `caret` (empty when they're equal).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Editing {
    /// Index into the slide's shapes.
    pub shape: usize,
    pub anchor: TextPos,
    pub caret: TextPos,
    /// Where Up and Down aim, in the text's box: the caret's x when they started.
    pub goal_x: Option<f32>,
}

impl Editing {
    /// A caret at `pos` in `shape`, nothing selected.
    pub fn at(shape: usize, pos: TextPos) -> Editing {
        Editing {
            shape,
            anchor: pos,
            caret: pos,
            goal_x: None,
        }
    }

    /// The selection in order.
    pub fn range(&self) -> (TextPos, TextPos) {
        (self.anchor.min(self.caret), self.anchor.max(self.caret))
    }
}

/// A shape being dragged on the canvas: the box it would land in and the smart guides
/// that box lines up with.
#[derive(Debug, Clone, PartialEq)]
pub struct Preview {
    /// Index into the slide's shapes.
    pub shape: usize,
    pub frame: Frame,
    pub guides: Vec<Guide>,
}

/// The selection and editing colour.
const ACCENT: gdk::RGBA = gdk::RGBA::new(0.21, 0.52, 0.89, 1.0);
/// Smart guides' colour: magenta, apart from the selection's blue.
const GUIDE: gdk::RGBA = gdk::RGBA::new(0.91, 0.19, 0.55, 1.0);
/// The signal a view emits when the slide shown, the selection, or the text selection
/// changes.
const SELECTION_CHANGED: &str = "selection-changed";

/// Where the slide sits in the widget: its top-left corner, the scale correction that
/// makes it fill whole pixels, and the pixels per EMU it draws at.
#[derive(Debug, Clone, Copy)]
struct Placement {
    x: f32,
    y: f32,
    kx: f32,
    ky: f32,
    scale: f32,
}

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
        /// The window's canvas: it outlines empty placeholders and shows editing.
        pub editable: Cell<bool>,
        /// `index` is a layout's: the view shows the new slide it makes, its
        /// placeholders outlined (the New Slide menu).
        pub layout: Cell<bool>,
        pub editing: Cell<Option<Editing>>,
        /// The shape selected (the one being edited when text is), by index.
        pub selected: Cell<Option<usize>>,
        /// A shape being dragged: where it would land, and the guides why.
        pub preview: RefCell<Option<Preview>>,
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

    impl ObjectImpl for SlideView {
        fn signals() -> &'static [glib::subclass::Signal] {
            static SIGNALS: std::sync::LazyLock<Vec<glib::subclass::Signal>> =
                std::sync::LazyLock::new(|| {
                    vec![glib::subclass::Signal::builder(SELECTION_CHANGED).build()]
                });
            &SIGNALS
        }
    }

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
                .map(|d| d.presentation().size)
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
            let presentation = deck.presentation();
            let slide = if self.layout.get() {
                presentation.layouts.get(self.index.get()).map(|l| &l.slide)
            } else {
                presentation.slides.get(self.index.get())
            };
            let Some(slide) = slide else {
                return;
            };
            let Some(at) = obj.placement() else {
                return;
            };
            // A dragged shape draws where it would land.
            let preview = self.preview.borrow();
            let dragged;
            let slide = match preview.as_ref() {
                Some(p) if p.shape < slide.shapes.len() => {
                    let mut s = slide.clone();
                    s.shapes[p.shape].frame = p.frame;
                    dragged = s;
                    &dragged
                }
                _ => slide,
            };
            let ctx = obj.pango();
            snapshot.save();
            snapshot.translate(&graphene::Point::new(at.x, at.y));
            snapshot.scale(at.kx, at.ky);
            render::draw_slide(snapshot, &ctx, deck, slide, at.scale);
            if self.editable.get() || self.layout.get() {
                let muted = gdk::RGBA::new(backdrop.red(), backdrop.green(), backdrop.blue(), 0.6);
                render::draw_empty_placeholders(snapshot, slide, at.scale, &muted);
            }
            if let Some(e) = self.editing.get() {
                if let Some(shape) = slide.shapes.get(e.shape) {
                    render::draw_editing(
                        snapshot, &ctx, shape, at.scale, e.anchor, e.caret, &ACCENT,
                    );
                }
            }
            let selected = self.editing.get().map(|e| e.shape).or(self.selected.get());
            if let Some(shape) = selected.and_then(|i| slide.shapes.get(i)) {
                render::draw_selection(snapshot, &shape.frame, at.scale, at.kx, &ACCENT);
            }
            if let Some(p) = preview.as_ref() {
                render::draw_guides(snapshot, &p.guides, at.scale, at.kx, &GUIDE);
            }
            snapshot.scale(1.0 / at.kx, 1.0 / at.ky);
            if self.backdrop.get().is_none() {
                // A hairline edge, so a white slide shows against a light theme.
                let size = presentation.size;
                let (tw, th) = (
                    size.cx as f32 * at.scale * at.kx,
                    size.cy as f32 * at.scale * at.ky,
                );
                let edge = gdk::RGBA::new(backdrop.red(), backdrop.green(), backdrop.blue(), 0.35);
                let r =
                    gtk::gsk::RoundedRect::from_rect(graphene::Rect::new(0.0, 0.0, tw, th), 0.0);
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

    /// A thumbnail `width` wide of the new slide layout `layout` of `deck` makes.
    pub fn layout(deck: &Rc<Deck>, layout: usize, width: i32) -> SlideView {
        let view = SlideView::new(deck, layout, width);
        view.imp().layout.set(true);
        view
    }

    pub fn index(&self) -> usize {
        self.imp().index.get()
    }

    /// Show slide `index`; editing and the selection end when it's another slide.
    pub fn set_index(&self, index: usize) {
        let imp = self.imp();
        if imp.index.replace(index) != index {
            imp.editing.set(None);
            imp.selected.set(None);
            imp.preview.replace(None);
            self.selection_changed();
        }
        self.queue_draw();
    }

    /// Call `f` whenever the slide shown, the selection, or the text selection changes.
    pub fn connect_selection_changed(&self, f: impl Fn() + 'static) -> glib::SignalHandlerId {
        self.connect_local(SELECTION_CHANGED, false, move |_| {
            f();
            None
        })
    }

    fn selection_changed(&self) {
        self.emit_by_name::<()>(SELECTION_CHANGED, &[]);
    }

    pub fn set_backdrop(&self, color: gdk::RGBA) {
        self.imp().backdrop.set(Some(color));
        self.queue_draw();
    }

    /// Make this the window's canvas, where text is edited.
    pub fn set_editable(&self, editable: bool) {
        self.imp().editable.set(editable);
        self.queue_draw();
    }

    pub fn editing(&self) -> Option<Editing> {
        self.imp().editing.get()
    }

    /// Edit text (`Some`), which selects its shape, or stop (`None`), which leaves the
    /// shape selected.
    pub fn set_editing(&self, editing: Option<Editing>) {
        let imp = self.imp();
        if let Some(e) = imp.editing.replace(editing) {
            imp.selected.set(Some(e.shape));
        }
        if let Some(e) = editing {
            imp.selected.set(Some(e.shape));
        }
        self.queue_draw();
        self.selection_changed();
    }

    /// The selected shape: the one whose text is edited, or one selected by a click.
    pub fn selected(&self) -> Option<usize> {
        let imp = self.imp();
        imp.editing.get().map(|e| e.shape).or(imp.selected.get())
    }

    /// Select shape `shape` (`None`: nothing), editing no text.
    pub fn select(&self, shape: Option<usize>) {
        let imp = self.imp();
        imp.editing.set(None);
        imp.selected.set(shape);
        self.queue_draw();
        self.selection_changed();
    }

    /// Show a shape being dragged where it would land (`None`: no drag).
    pub fn set_preview(&self, preview: Option<Preview>) {
        self.imp().preview.replace(preview);
        self.queue_draw();
    }

    /// Milliseconds the last snapshot took to build.
    pub fn last_draw_ms(&self) -> f64 {
        self.imp().last_ms.get()
    }

    fn pango(&self) -> pango::Context {
        self.imp()
            .pango
            .borrow_mut()
            .get_or_insert_with(|| {
                let c = self.create_pango_context();
                // Fractional glyph positions: text scales smoothly with the slide.
                c.set_round_glyph_positions(false);
                c
            })
            .clone()
    }

    /// Where the slide sits in the widget now: scaled to fit, centred (letterboxed). The
    /// window's canvas keeps a margin around the slide; thumbnails and the slideshow use
    /// all they have.
    fn placement(&self) -> Option<Placement> {
        let imp = self.imp();
        let size = imp.deck.borrow().as_ref()?.presentation().size;
        if size.cx <= 0 || size.cy <= 0 {
            return None;
        }
        let (w, h) = (self.width() as f32, self.height() as f32);
        let margin = if imp.thumb_width.get() == 0 && imp.backdrop.get().is_none() {
            24.0f32.min(w / 8.0).min(h / 8.0)
        } else {
            0.0
        };
        let scale = ((w - 2.0 * margin) / size.cx as f32).min((h - 2.0 * margin) / size.cy as f32);
        if scale <= 0.0 {
            return None;
        }
        let (sw, sh) = (size.cx as f32 * scale, size.cy as f32 * scale);
        // A slide within a pixel of filling an axis fills it: no sliver of backdrop
        // (a 1280x720 screen shows a 10080720x5670720 EMU slide 1279.94 px wide).
        let snap = |avail: f32, len: f32| {
            if avail - len < 1.0 {
                avail
            } else {
                len.round()
            }
        };
        let (tw, th) = (snap(w - 2.0 * margin, sw), snap(h - 2.0 * margin, sh));
        Some(Placement {
            x: ((w - tw) / 2.0).floor(),
            y: ((h - th) / 2.0).floor(),
            kx: tw / sw,
            ky: th / sh,
            scale,
        })
    }

    /// The widget point (`x`, `y`) as a point of the slide in pixels at the scale it's
    /// drawn at.
    fn to_slide(&self, x: f64, y: f64) -> Option<(f32, f32, f32)> {
        let at = self.placement()?;
        Some((
            (x as f32 - at.x) / at.kx,
            (y as f32 - at.y) / at.ky,
            at.scale,
        ))
    }

    /// The shape with text to edit at the widget point (`x`, `y`), and the text position
    /// there.
    pub fn text_at(&self, x: f64, y: f64) -> Option<(usize, TextPos)> {
        let (sx, sy, scale) = self.to_slide(x, y)?;
        let deck = self.imp().deck.borrow().clone()?;
        let presentation = deck.presentation();
        let slide = presentation.slides.get(self.index())?;
        let shape = render::text_shape_at(slide, scale, sx, sy)?;
        let placed = render::place_text(&self.pango(), &slide.shapes[shape], scale)?;
        Some((shape, placed.hit(sx, sy)))
    }

    /// Shape `shape`'s text on this slide, laid out as the canvas draws it.
    pub fn placed_text(&self, shape: usize) -> Option<PlacedText> {
        let scale = self.placement()?.scale;
        let deck = self.imp().deck.borrow().clone()?;
        let presentation = deck.presentation();
        let shape = presentation.slides.get(self.index())?.shapes.get(shape)?;
        render::place_text(&self.pango(), shape, scale)
    }

    /// The caret's rectangle in widget coordinates, for input methods' popups.
    pub fn caret_area(&self) -> Option<gdk::Rectangle> {
        let e = self.editing()?;
        let at = self.placement()?;
        let placed = self.placed_text(e.shape)?;
        let c = placed.caret(e.caret);
        let t = placed.to_slide();
        let top = t.transform_point(&c.top_left());
        let bottom = t.transform_point(&c.bottom_left());
        let (x0, y0) = (top.x().min(bottom.x()), top.y().min(bottom.y()));
        let (x1, y1) = (top.x().max(bottom.x()), top.y().max(bottom.y()));
        let wx = |x: f32| (at.x + x * at.kx).round() as i32;
        let wy = |y: f32| (at.y + y * at.ky).round() as i32;
        Some(gdk::Rectangle::new(
            wx(x0),
            wy(y0),
            (wx(x1) - wx(x0)).max(1),
            (wy(y1) - wy(y0)).max(1),
        ))
    }

    /// How many EMU one screen pixel is on the slide.
    pub fn emu_per_pixel(&self) -> Option<f64> {
        let at = self.placement()?;
        Some(1.0 / (at.scale as f64 * at.kx as f64))
    }

    /// The topmost of the slide's own shapes at the widget point (`x`, `y`), give or take
    /// `slop` pixels.
    pub fn shape_at(&self, x: f64, y: f64, slop: f32) -> Option<usize> {
        let (sx, sy, scale) = self.to_slide(x, y)?;
        let deck = self.imp().deck.borrow().clone()?;
        let presentation = deck.presentation();
        let slide = presentation.slides.get(self.index())?;
        render::shape_at(slide, scale, sx, sy, slop)
    }

    /// The widget point (`x`, `y`) in shape `shape`'s own box, in pixels.
    pub fn local_point(&self, shape: usize, x: f64, y: f64) -> Option<LocalPoint> {
        let (sx, sy, scale) = self.to_slide(x, y)?;
        let deck = self.imp().deck.borrow().clone()?;
        let presentation = deck.presentation();
        let frame = presentation
            .slides
            .get(self.index())?
            .shapes
            .get(shape)?
            .frame;
        render::local_point(&frame, scale, sx, sy)
    }

    /// End a drag's preview, giving where it would have landed.
    pub fn take_preview(&self) -> Option<Preview> {
        let p = self.imp().preview.take();
        self.queue_draw();
        p
    }
}
