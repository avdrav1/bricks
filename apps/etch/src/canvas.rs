//! `Canvas`, the widget that shows the image and draws on it (ADR 0010).
//!
//! The image is shown as [`TILE_PX`]-pixel tiles, each a texture built the first time it's
//! visible and dropped when an operation changes its pixels, so a stroke rebuilds a tile
//! or two a frame however large the image. Tiles scale by nearest neighbour at 100% and
//! above, so pixels show as squares, and trilinearly below. Behind them, a checkerboard
//! shows through transparency; around them, a one-pixel line marks the image's edge.
//!
//! Input: a primary-button drag draws with the pencil, a middle-button drag or a drag with
//! Space held pans, the wheel pans (Shift+wheel sideways), and Ctrl+wheel zooms around the
//! pointer. The widget emits `changed` whenever what the status bar or the Edit menu shows
//! may have changed: the view, the history, or the pixel under the pointer.

use crate::view::View;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib, graphene, gsk};
use image_model::{Document, Image, Pencil, Rect};
use std::time::Instant;

/// The side of a tile texture, in image pixels.
pub const TILE_PX: u32 = 256;
/// The checkerboard's squares, in widget pixels.
const CHECK: f32 = 8.0;
const CHECK_LIGHT: gdk::RGBA = gdk::RGBA::new(1.0, 1.0, 1.0, 1.0);
const CHECK_DARK: gdk::RGBA = gdk::RGBA::new(0.8, 0.8, 0.8, 1.0);
/// The line around the image.
const BOUNDARY: gdk::RGBA = gdk::RGBA::new(0.0, 0.0, 0.0, 0.35);
/// How far one wheel notch pans, in widget pixels.
const NOTCH_PX: f64 = 48.0;
/// The tip's outline shows once the tip is this many screen pixels across.
const OUTLINE_MIN_PX: f64 = 4.0;
const CHANGED: &str = "changed";

/// What a drag in progress does.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Drag {
    #[default]
    None,
    Draw,
    /// Panning; the drag's offset when the image last moved.
    Pan {
        last: (f64, f64),
    },
}

mod imp {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[derive(Default)]
    pub struct Canvas {
        pub doc: RefCell<Option<Document>>,
        /// `None` until the widget first has a size.
        pub view: Cell<Option<View>>,
        /// One per tile, row after row; `None` until built, and again once changed.
        pub tiles: RefCell<Vec<Option<gdk::Texture>>>,
        pub(super) drag: Cell<Drag>,
        /// Space is held: a primary-button drag pans.
        pub space: Cell<bool>,
        /// The pointer, in widget coordinates, while it's over the canvas.
        pub pointer: Cell<Option<(f64, f64)>>,
        /// Ctrl+wheel's notches not yet turned into a zoom step.
        pub notches: Cell<f64>,
        /// How long the last snapshot took to build, in milliseconds.
        pub last_ms: Cell<f64>,
        /// How many snapshots have drawn the image.
        pub drawn: Cell<u64>,
        /// Whether the next allocation should show the image at 100% rather than fitted.
        pub actual_size: Cell<bool>,
        /// What a primary-button drag draws with.
        pub tip: Cell<Option<Pencil>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Canvas {
        const NAME: &'static str = "EtchCanvas";
        type Type = super::Canvas;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_accessible_role(gtk::AccessibleRole::Img);
        }
    }

    impl ObjectImpl for Canvas {
        fn signals() -> &'static [glib::subclass::Signal] {
            static SIGNALS: std::sync::LazyLock<Vec<glib::subclass::Signal>> =
                std::sync::LazyLock::new(|| vec![glib::subclass::Signal::builder(CHANGED).build()]);
            &SIGNALS
        }

        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.set_hexpand(true);
            obj.set_vexpand(true);
            obj.set_focusable(true);
            obj.set_overflow(gtk::Overflow::Hidden);
            obj.set_cursor_from_name(Some("crosshair"));
            obj.add_inputs();
        }
    }

    impl WidgetImpl for Canvas {
        fn measure(&self, _orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            (64, 640, -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.parent_size_allocate(width, height, baseline);
            let size = (width as f64, height as f64);
            let view = match self.view.get() {
                Some(mut v) => {
                    v.resize(size);
                    v
                }
                None => {
                    let mut v = View::new(self.obj().image_size(), size);
                    if self.actual_size.get() {
                        v.zoom_at(1.0, (size.0 / 2.0, size.1 / 2.0));
                    }
                    v
                }
            };
            self.view.set(Some(view));
            self.obj().emit_changed();
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let t0 = Instant::now();
            let Some(view) = self.view.get() else {
                return;
            };
            let obj = self.obj();
            let (w, h) = (obj.width() as f32, obj.height() as f32);
            let (ox, oy) = (view.origin.0 as f32, view.origin.1 as f32);
            let (sw, sh) = (view.shown().0 as f32, view.shown().1 as f32);
            snapshot.push_clip(&graphene::Rect::new(0.0, 0.0, w, h));
            snapshot.append_color(
                &BOUNDARY,
                &graphene::Rect::new(ox - 1.0, oy - 1.0, sw + 2.0, sh + 2.0),
            );
            let visible = graphene::Rect::new(0.0, 0.0, w, h)
                .intersection(&graphene::Rect::new(ox, oy, sw, sh));
            if let Some(visible) = visible {
                self.checkerboard(snapshot, &visible, (ox, oy));
                self.draw_tiles(snapshot, &view, &visible);
            }
            self.draw_tip_outline(snapshot, &view);
            snapshot.pop();
            self.last_ms.set(t0.elapsed().as_secs_f64() * 1e3);
            self.drawn.set(self.drawn.get() + 1);
        }
    }

    impl Canvas {
        /// The tip's size around the pointer, where a drag would draw, once it's larger
        /// than a few screen pixels: a dark ring on a light one, seen on any colour. Not
        /// while panning.
        fn draw_tip_outline(&self, snapshot: &gtk::Snapshot, view: &View) {
            let (Some(tip), Some(p)) = (self.tip.get(), self.pointer.get()) else {
                return;
            };
            let w = tip.width.max(1) as f64;
            let panning = self.space.get() || matches!(self.drag.get(), Drag::Pan { .. });
            if w * view.zoom < OUTLINE_MIN_PX || panning {
                return;
            }
            // The disc's centre: the pixel's centre for odd widths, its corner for even ones
            // (they lean right and down).
            let (x, y) = view.to_image(p);
            let shift = if tip.width % 2 == 0 { 1.0 } else { 0.5 };
            let centre = view.to_widget((x.floor() + shift, y.floor() + shift));
            let builder = gsk::PathBuilder::new();
            builder.add_circle(
                &graphene::Point::new(centre.0 as f32, centre.1 as f32),
                (w * view.zoom / 2.0) as f32,
            );
            let path = builder.to_path();
            let light = gdk::RGBA::new(1.0, 1.0, 1.0, 0.7);
            let dark = gdk::RGBA::new(0.0, 0.0, 0.0, 0.8);
            snapshot.append_stroke(&path, &gsk::Stroke::new(3.0), &light);
            snapshot.append_stroke(&path, &gsk::Stroke::new(1.0), &dark);
        }

        fn checkerboard(
            &self,
            snapshot: &gtk::Snapshot,
            visible: &graphene::Rect,
            (ox, oy): (f32, f32),
        ) {
            let cell = graphene::Rect::new(ox, oy, 2.0 * CHECK, 2.0 * CHECK);
            snapshot.push_repeat(visible, Some(&cell));
            snapshot.append_color(&CHECK_LIGHT, &cell);
            snapshot.append_color(&CHECK_DARK, &graphene::Rect::new(ox, oy, CHECK, CHECK));
            snapshot.append_color(
                &CHECK_DARK,
                &graphene::Rect::new(ox + CHECK, oy + CHECK, CHECK, CHECK),
            );
            snapshot.pop();
        }

        /// The tiles `visible` (widget coordinates) shows, built where they aren't yet.
        fn draw_tiles(&self, snapshot: &gtk::Snapshot, view: &View, visible: &graphene::Rect) {
            let doc = self.doc.borrow();
            let Some(doc) = doc.as_ref() else { return };
            let image = doc.image();
            let (iw, ih) = (image.width(), image.height());
            let (cols, rows) = (iw.div_ceil(TILE_PX), ih.div_ceil(TILE_PX));
            let a = view.to_image((visible.x() as f64, visible.y() as f64));
            let b = view.to_image((
                (visible.x() + visible.width()) as f64,
                (visible.y() + visible.height()) as f64,
            ));
            let range = |lo: f64, hi: f64, n: u32, len: u32| {
                let first = (lo.floor().max(0.0) as u32).min(len - 1) / TILE_PX;
                let last = ((hi.ceil().max(1.0) as u32).min(len) - 1) / TILE_PX;
                first..(last + 1).min(n)
            };
            // Tile edges come from one formula, rounded to device pixels, so neighbours
            // meet exactly: no seam and no overlap, whatever the renderer. Rounding moves
            // a tile at most half a screen pixel from where the view puts it.
            let scale = self
                .obj()
                .native()
                .and_then(|n| n.surface())
                .map_or(1.0, |s| s.scale());
            let edge = |origin: f64, px: u32, len: u32| {
                (((origin + px.min(len) as f64 * view.zoom) * scale).round() / scale) as f32
            };
            let filter = if view.zoom >= 1.0 {
                gsk::ScalingFilter::Nearest
            } else {
                gsk::ScalingFilter::Trilinear
            };
            let mut tiles = self.tiles.borrow_mut();
            tiles.resize((cols * rows) as usize, None);
            for row in range(a.1, b.1, rows, ih) {
                let (y0, y1) = (
                    edge(view.origin.1, row * TILE_PX, ih),
                    edge(view.origin.1, (row + 1) * TILE_PX, ih),
                );
                for col in range(a.0, b.0, cols, iw) {
                    let (x0, x1) = (
                        edge(view.origin.0, col * TILE_PX, iw),
                        edge(view.origin.0, (col + 1) * TILE_PX, iw),
                    );
                    let texture = tiles[(row * cols + col) as usize]
                        .get_or_insert_with(|| tile_texture(image, col, row));
                    snapshot.append_scaled_texture(
                        texture,
                        filter,
                        &graphene::Rect::new(x0, y0, x1 - x0, y1 - y0),
                    );
                }
            }
        }
    }
}

/// The pixels of tile `(col, row)` as a texture.
fn tile_texture(image: &Image, col: u32, row: u32) -> gdk::Texture {
    let rect = tile_rect(image, col, row);
    let bytes = glib::Bytes::from_owned(image.copy_rect(rect));
    gdk::MemoryTexture::new(
        rect.width as i32,
        rect.height as i32,
        gdk::MemoryFormat::R8g8b8a8,
        &bytes,
        rect.width as usize * 4,
    )
    .upcast()
}

fn tile_rect(image: &Image, col: u32, row: u32) -> Rect {
    let (x, y) = (col * TILE_PX, row * TILE_PX);
    Rect::new(
        x,
        y,
        TILE_PX.min(image.width() - x),
        TILE_PX.min(image.height() - y),
    )
}

glib::wrapper! {
    pub struct Canvas(ObjectSubclass<imp::Canvas>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Canvas {
    /// A canvas showing `image`: at 100% when it fits the widget, else fitted; with
    /// `actual_size`, at 100% whatever its size.
    pub fn new(image: Image, actual_size: bool) -> Canvas {
        let canvas: Canvas = glib::Object::new();
        let (w, h) = (image.width(), image.height());
        canvas.update_property(&[
            gtk::accessible::Property::Label("Canvas"),
            gtk::accessible::Property::Description(&format!("{w}×{h} image")),
        ]);
        let imp = canvas.imp();
        imp.doc.replace(Some(Document::new(image)));
        imp.actual_size.set(actual_size);
        canvas
    }

    pub fn image_size(&self) -> (u32, u32) {
        let doc = self.imp().doc.borrow();
        doc.as_ref()
            .map_or((1, 1), |d| (d.image().width(), d.image().height()))
    }

    pub fn view(&self) -> Option<View> {
        self.imp().view.get()
    }

    /// The image pixel under the pointer, if the pointer is over the image.
    pub fn pointer_pixel(&self) -> Option<(u32, u32)> {
        let (view, p) = (self.view()?, self.imp().pointer.get()?);
        let (x, y) = view.to_image(p);
        let (w, h) = self.image_size();
        (x >= 0.0 && y >= 0.0 && x < w as f64 && y < h as f64).then_some((x as u32, y as u32))
    }

    pub fn can_undo(&self) -> bool {
        self.imp()
            .doc
            .borrow()
            .as_ref()
            .is_some_and(Document::can_undo)
    }

    pub fn can_redo(&self) -> bool {
        self.imp()
            .doc
            .borrow()
            .as_ref()
            .is_some_and(Document::can_redo)
    }

    pub fn undo(&self) {
        self.imp().drag.set(Drag::None);
        let changed = self.with_doc(Document::undo);
        self.invalidate(changed);
    }

    pub fn redo(&self) {
        self.imp().drag.set(Drag::None);
        let changed = self.with_doc(Document::redo);
        self.invalidate(changed);
    }

    /// Zoom In (`dir` > 0) or Out a step, around the pointer when given, else the centre.
    pub fn zoom_step(&self, dir: i32, around: Option<(f64, f64)>) {
        self.change_view(|v| {
            let anchor = around.unwrap_or((v.size.0 / 2.0, v.size.1 / 2.0));
            v.step(dir, anchor)
        });
    }

    pub fn actual_size(&self) {
        self.change_view(|v| v.zoom_at(1.0, (v.size.0 / 2.0, v.size.1 / 2.0)));
    }

    pub fn fit(&self) {
        self.change_view(View::fit);
    }

    /// What a primary-button drag draws with.
    pub fn set_tip(&self, tip: Pencil) {
        self.imp().tip.set(Some(tip));
        self.queue_draw();
    }

    /// Space held or let go: while held, a primary-button drag pans.
    pub fn set_space(&self, held: bool) {
        let imp = self.imp();
        if imp.space.replace(held) != held && imp.drag.get() == Drag::None {
            self.set_cursor_from_name(Some(if held { "grab" } else { "crosshair" }));
        }
    }

    /// How long the last snapshot took to build, in milliseconds.
    pub fn last_draw_ms(&self) -> f64 {
        self.imp().last_ms.get()
    }

    /// How many snapshots have drawn the image: none until it first shows.
    pub fn frames_drawn(&self) -> u64 {
        self.imp().drawn.get()
    }

    pub fn connect_changed<F: Fn(&Canvas) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_local(CHANGED, false, move |args| {
            f(&args[0].get::<Canvas>().expect("the canvas"));
            None
        })
    }

    /// Starts a pencil stroke at widget point `at`.
    pub fn draw_begin(&self, at: (f64, f64)) {
        let Some(view) = self.view() else { return };
        let Some(tip) = self.imp().tip.get() else {
            return;
        };
        let changed = self.with_doc(|d| d.begin_stroke(tip, view.to_image(at)));
        self.imp().drag.set(Drag::Draw);
        self.invalidate(changed);
    }

    /// Continues the stroke to widget point `to`.
    pub fn draw_to(&self, to: (f64, f64)) {
        let Some(view) = self.view() else { return };
        if let Some(changed) = self.with_doc(|d| d.extend_stroke(view.to_image(to))) {
            self.invalidate(Some(changed));
        }
    }

    /// Finishes the stroke as one undo step.
    pub fn draw_end(&self) {
        self.with_doc(Document::end_stroke);
        self.imp().drag.set(Drag::None);
        self.emit_changed();
    }

    fn with_doc<T>(&self, f: impl FnOnce(&mut Document) -> T) -> T {
        let mut doc = self.imp().doc.borrow_mut();
        f(doc.as_mut().expect("a canvas has a document"))
    }

    /// Drops the tiles `changed` overlaps and redraws.
    fn invalidate(&self, changed: Option<Rect>) {
        if let Some(r) = changed {
            let (w, _) = self.image_size();
            let cols = w.div_ceil(TILE_PX);
            let mut tiles = self.imp().tiles.borrow_mut();
            for row in r.y / TILE_PX..r.bottom().div_ceil(TILE_PX) {
                for col in r.x / TILE_PX..r.right().div_ceil(TILE_PX) {
                    if let Some(t) = tiles.get_mut((row * cols + col) as usize) {
                        *t = None;
                    }
                }
            }
            self.queue_draw();
        }
        self.emit_changed();
    }

    fn change_view(&self, f: impl FnOnce(&mut View)) {
        let imp = self.imp();
        if let Some(mut v) = imp.view.get() {
            f(&mut v);
            if Some(v) != imp.view.get() {
                imp.view.set(Some(v));
                self.queue_draw();
            }
            self.emit_changed();
        }
    }

    fn emit_changed(&self) {
        self.emit_by_name::<()>(CHANGED, &[]);
    }

    fn add_inputs(&self) {
        let drag = gtk::GestureDrag::new();
        // Any button: the primary draws (or pans with Space), the middle pans.
        drag.set_button(0);
        drag.connect_drag_begin({
            let canvas = self.downgrade();
            move |g, x, y| {
                let Some(canvas) = canvas.upgrade() else {
                    return;
                };
                canvas.grab_focus();
                let imp = canvas.imp();
                match g.current_button() {
                    1 if !imp.space.get() => canvas.draw_begin((x, y)),
                    1 | 2 => {
                        imp.drag.set(Drag::Pan { last: (0.0, 0.0) });
                        canvas.set_cursor_from_name(Some("grabbing"));
                    }
                    _ => {
                        g.set_state(gtk::EventSequenceState::Denied);
                    }
                }
            }
        });
        drag.connect_drag_update({
            let canvas = self.downgrade();
            move |g, dx, dy| {
                let Some(canvas) = canvas.upgrade() else {
                    return;
                };
                let imp = canvas.imp();
                match imp.drag.get() {
                    Drag::Draw => {
                        if let Some((x, y)) = g.start_point() {
                            canvas.draw_to((x + dx, y + dy));
                        }
                    }
                    Drag::Pan { last } => {
                        imp.drag.set(Drag::Pan { last: (dx, dy) });
                        canvas.change_view(|v| v.pan(dx - last.0, dy - last.1));
                    }
                    Drag::None => {}
                }
            }
        });
        drag.connect_drag_end({
            let canvas = self.downgrade();
            move |_, _, _| {
                let Some(canvas) = canvas.upgrade() else {
                    return;
                };
                match canvas.imp().drag.get() {
                    Drag::Draw => canvas.draw_end(),
                    Drag::Pan { .. } => {
                        canvas.imp().drag.set(Drag::None);
                        let held = canvas.imp().space.get();
                        canvas.set_cursor_from_name(Some(if held { "grab" } else { "crosshair" }));
                    }
                    Drag::None => {}
                }
            }
        });
        self.add_controller(drag);

        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
        scroll.connect_scroll({
            let canvas = self.downgrade();
            move |c, dx, dy| {
                let Some(canvas) = canvas.upgrade() else {
                    return glib::Propagation::Proceed;
                };
                let state = c.current_event_state();
                let wheel = c.unit() == gdk::ScrollUnit::Wheel;
                if state.contains(gdk::ModifierType::CONTROL_MASK) {
                    canvas.wheel_zoom(dy, wheel);
                } else {
                    let k = if wheel { NOTCH_PX } else { 1.0 };
                    let (mut px, mut py) = (dx * k, dy * k);
                    if state.contains(gdk::ModifierType::SHIFT_MASK) && px == 0.0 {
                        (px, py) = (py, 0.0);
                    }
                    canvas.change_view(|v| v.pan(-px, -py));
                }
                glib::Propagation::Stop
            }
        });
        self.add_controller(scroll);

        let motion = gtk::EventControllerMotion::new();
        // Entering counts as moving: the pointer may arrive without moving.
        let track = {
            let canvas = self.downgrade();
            move |_: &gtk::EventControllerMotion, x: f64, y: f64| {
                if let Some(canvas) = canvas.upgrade() {
                    canvas.imp().pointer.set(Some((x, y)));
                    canvas.redraw_outline();
                    canvas.emit_changed();
                }
            }
        };
        motion.connect_enter(track.clone());
        motion.connect_motion(track);
        motion.connect_leave({
            let canvas = self.downgrade();
            move |_| {
                if let Some(canvas) = canvas.upgrade() {
                    canvas.imp().pointer.set(None);
                    canvas.redraw_outline();
                    canvas.emit_changed();
                }
            }
        });
        self.add_controller(motion);
    }

    /// Redraws when the pointer moved and the tip's outline follows it.
    fn redraw_outline(&self) {
        let (tip, view) = (self.imp().tip.get(), self.view());
        if let (Some(tip), Some(view)) = (tip, view) {
            if tip.width as f64 * view.zoom >= OUTLINE_MIN_PX {
                self.queue_draw();
            }
        }
    }

    /// Ctrl+wheel: each notch down a step out, up a step in, around the pointer; a
    /// touchpad zooms smoothly.
    fn wheel_zoom(&self, dy: f64, wheel: bool) {
        let imp = self.imp();
        let at = imp.pointer.get();
        if !wheel {
            self.change_view(|v| {
                let anchor = at.unwrap_or((v.size.0 / 2.0, v.size.1 / 2.0));
                v.zoom_at(v.zoom * 2f64.powf(-dy / 100.0), anchor)
            });
            return;
        }
        let mut n = imp.notches.get() + dy;
        while n >= 1.0 {
            self.zoom_step(-1, at);
            n -= 1.0;
        }
        while n <= -1.0 {
            self.zoom_step(1, at);
            n += 1.0;
        }
        imp.notches.set(n);
    }
}
