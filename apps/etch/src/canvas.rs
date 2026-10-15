//! `Canvas`, the widget that shows the image and draws on it (ADR 0010).
//!
//! The image is shown as [`TILE_PX`]-pixel tiles, each a texture built the first time it's
//! visible and dropped when an operation changes its pixels, so a stroke rebuilds a tile
//! or two a frame however large the image. Tiles scale by nearest neighbour at 100% and
//! above, so pixels show as squares, and trilinearly below. Behind them, a checkerboard
//! shows through transparency; around them, a one-pixel line marks the image's edge. All
//! three are drawn in device pixels, their edges rounded to them ([`Device`]), so tiles
//! meet without seams and the checkerboard's squares are crisp whatever the renderer.
//!
//! Input: a primary-button drag draws with the tool's tip (with the fill, a click fills
//! with the foreground and a right-click with the background; with the colour picker, a
//! click picks the foreground and a right-click the background; with a shape, the drag
//! previews the shape over the image, which it doesn't touch until the release draws it,
//! Shift constraining it and Escape cancelling it; with text, a click starts typing there
//! and the text shows over the image until Enter or a click elsewhere puts it on, Escape
//! dropping it; with the selection, a drag selects a rectangle, outlined in dashes, and a
//! drag inside it lifts its pixels, which float over the image, leaving transparency where
//! they were, until they land as one undo step; pixels pasted from the clipboard float
//! the same way, and the selection's pixels are copied to it), a middle-button drag or a
//! drag with Space held pans, the wheel pans (Shift+wheel sideways), and Ctrl+wheel (⌘ on
//! macOS) zooms around the pointer. The widget emits `changed` whenever what the status
//! bar or the Edit menu shows may have changed: the view, the history, the selection, or
//! the pixel under the pointer.

use crate::colours;
use crate::text::{KeyOutcome, TextBox, TextLook};
use crate::tools::Action;
use crate::view::View;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, gio, glib, graphene, gsk};
use image_model::{spanning, Color, Document, Floating, Image, Rect, Shape, Tip, Transform};
use std::time::Instant;

/// The side of a tile texture, in image pixels.
pub const TILE_PX: u32 = 256;
/// The checkerboard's squares, in widget pixels (rounded to whole device pixels).
const CHECK: f64 = 8.0;
const CHECK_LIGHT: gdk::RGBA = gdk::RGBA::new(1.0, 1.0, 1.0, 1.0);
const CHECK_DARK: gdk::RGBA = gdk::RGBA::new(0.8, 0.8, 0.8, 1.0);
/// The line around the image.
const BOUNDARY: gdk::RGBA = gdk::RGBA::new(0.0, 0.0, 0.0, 0.35);
/// How far one wheel notch pans, in widget pixels.
const NOTCH_PX: f64 = 48.0;
/// The tip's outline shows once the tip is this many screen pixels across.
const OUTLINE_MIN_PX: f64 = 4.0;
/// The caret: a dark line on a light one.
const CARET_DARK: gdk::RGBA = gdk::RGBA::new(0.0, 0.0, 0.0, 1.0);
const CARET_LIGHT: gdk::RGBA = gdk::RGBA::new(1.0, 1.0, 1.0, 0.7);
/// The text's frame, dashed over the caret's light.
const FRAME_DARK: gdk::RGBA = gdk::RGBA::new(0.0, 0.0, 0.0, 0.5);
/// How far the text's frame stands off the text, in widget pixels.
const TEXT_PAD: f64 = 4.0;
/// The selection's outline: dark dashes on a light line, seen on any colour.
const SELECTION_LIGHT: gdk::RGBA = gdk::RGBA::new(1.0, 1.0, 1.0, 1.0);
const SELECTION_DARK: gdk::RGBA = gdk::RGBA::new(0.0, 0.0, 0.0, 1.0);
const CHANGED: &str = "changed";

/// What a drag in progress does.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Drag {
    #[default]
    None,
    Draw,
    /// A shape from where the drag began, in image coordinates, as it is so far.
    Shape {
        from: (f64, f64),
        shape: Shape,
    },
    /// Panning; the drag's offset when the image last moved.
    Pan {
        last: (f64, f64),
    },
    /// Selecting a rectangle from where the drag began, in image coordinates.
    Select {
        from: (f64, f64),
    },
    /// Moving the floating pixels: the drag began at `from`, in image coordinates, with
    /// their top-left pixel at `at`.
    Move {
        from: (f64, f64),
        at: (i64, i64),
    },
}

mod imp {
    use super::*;
    use std::cell::{Cell, OnceCell, RefCell};

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
        /// Ctrl+wheel's (⌘ on macOS) notches not yet turned into a zoom step.
        pub notches: Cell<f64>,
        /// How long the last snapshot took to build, in milliseconds.
        pub last_ms: Cell<f64>,
        /// How many snapshots have drawn the image.
        pub drawn: Cell<u64>,
        /// Whether the next allocation should show the image at 100% rather than fitted.
        pub actual_size: Cell<bool>,
        /// What the pointer's buttons do: draw with a tip, fill, or pick a colour.
        pub action: Cell<Option<Action>>,
        /// What a pick does with the colour picked.
        #[allow(clippy::type_complexity)]
        pub picked: RefCell<Option<std::rc::Rc<dyn Fn(Color, bool)>>>,
        /// The shape being dragged, not yet on the image, as the rectangles of one colour
        /// each that draw it over the image.
        pub preview: RefCell<Vec<(Rect, Color)>>,
        /// The text being typed, not yet on the image.
        pub text: RefCell<Option<TextBox>>,
        /// The selection, on the image, while its pixels aren't lifted.
        pub selection: Cell<Option<Rect>>,
        /// The selection's pixels, lifted and floating over the image until they land,
        /// and their texture.
        pub floating: RefCell<Option<Floating>>,
        pub floating_texture: RefCell<Option<gdk::Texture>>,
        /// The input method text is typed through, and the key controller it filters
        /// keys for while text is typed.
        pub im: OnceCell<gtk::IMMulticontext>,
        pub text_keys: OnceCell<gtk::EventControllerKey>,
        /// An Image menu command runs off the UI thread: the image mustn't change until
        /// its result is in.
        pub busy: Cell<bool>,
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
            obj.add_text_inputs();
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
            let dev = obj.device();
            let s = dev.scale as f32;
            let (w, h) = (obj.width() as f32, obj.height() as f32);
            snapshot.push_clip(&graphene::Rect::new(0.0, 0.0, w, h));
            // The image, the line around it, and what shows through it are drawn in device
            // pixels, every edge rounded to one: tiles meet exactly, the checkerboard's
            // squares cover whole pixels, and no renderer resamples them. (At any other
            // scale GTK draws a scaled texture through an offscreen at widget resolution,
            // and an edge between widget pixels comes out half covered: a seam.)
            snapshot.save();
            snapshot.scale(1.0 / s, 1.0 / s);
            let shown = shown_rect(&view, dev, (0, 0), obj.image_size(), true);
            let line = dev.scale.round().max(1.0) as f32;
            snapshot.append_color(
                &BOUNDARY,
                &graphene::Rect::new(
                    shown.x() - line,
                    shown.y() - line,
                    shown.width() + 2.0 * line,
                    shown.height() + 2.0 * line,
                ),
            );
            let visible = graphene::Rect::new(0.0, 0.0, w * s, h * s).intersection(&shown);
            if let Some(visible) = visible {
                let check = (shown.x(), shown.y());
                self.checkerboard(snapshot, dev, &visible, check);
                self.draw_tiles(snapshot, &view, dev, &visible);
                self.draw_floating(snapshot, &view, dev, &visible, check);
                self.draw_preview(snapshot, &view, dev);
            }
            snapshot.restore();
            self.draw_selection(snapshot, &view, dev);
            self.draw_text(snapshot, &view, dev);
            self.draw_tip_outline(snapshot, &view);
            snapshot.pop();
            self.last_ms.set(t0.elapsed().as_secs_f64() * 1e3);
            self.drawn.set(self.drawn.get() + 1);
        }
    }

    impl Canvas {
        /// What a primary-button drag draws with, if it draws.
        pub(super) fn tip(&self) -> Option<Tip> {
            match self.action.get()? {
                Action::Draw(tip) => Some(tip),
                Action::Fill { .. }
                | Action::Pick
                | Action::Shape(_)
                | Action::Text(_)
                | Action::Select => None,
            }
        }

        /// The floating pixels, over the image as they'll land: the place they were lifted
        /// from transparent (the checkerboard), and where they are, on the image, their
        /// pixels over the checkerboard, as they'll replace what's there. In device pixels,
        /// as the tiles are.
        fn draw_floating(
            &self,
            snapshot: &gtk::Snapshot,
            view: &View,
            dev: Device,
            visible: &graphene::Rect,
            origin: (f32, f32),
        ) {
            let floating = self.floating.borrow();
            let texture = self.floating_texture.borrow();
            let (Some(f), Some(texture)) = (floating.as_ref(), texture.as_ref()) else {
                return;
            };
            if let Some(r) = f.source() {
                let at = (r.x as i64, r.y as i64);
                snapshot.push_clip(&shown_rect(view, dev, at, (r.width, r.height), true));
                self.checkerboard(snapshot, dev, visible, origin);
                snapshot.pop();
            }
            let (w, h) = self.obj().image_size();
            if let Some(d) = f.bounds(Rect::new(0, 0, w, h)) {
                let at = (d.x as i64, d.y as i64);
                snapshot.push_clip(&shown_rect(view, dev, at, (d.width, d.height), true));
                self.checkerboard(snapshot, dev, visible, origin);
                let filter = match view.zoom >= 1.0 {
                    true => gsk::ScalingFilter::Nearest,
                    false => gsk::ScalingFilter::Trilinear,
                };
                let shown = shown_rect(view, dev, f.at(), f.size(), true);
                snapshot.append_scaled_texture(texture, filter, &shown);
                snapshot.pop();
            }
        }

        /// The selection's outline, just outside its pixels: dark dashes on a light line,
        /// seen on any colour.
        fn draw_selection(&self, snapshot: &gtk::Snapshot, view: &View, dev: Device) {
            let Some((at, size)) = self.obj().selection_box() else {
                return;
            };
            let r = dev.to_widget(&shown_rect(view, dev, at, size, true));
            let builder = gsk::PathBuilder::new();
            builder.add_rect(&graphene::Rect::new(
                r.x() - 0.5,
                r.y() - 0.5,
                r.width() + 1.0,
                r.height() + 1.0,
            ));
            let outline = builder.to_path();
            let dashed = gsk::Stroke::new(1.0);
            dashed.set_dash(&[4.0, 4.0]);
            // The outline's corners sit on pixel centres: half a pixel in, the dashes end on
            // pixel edges, whole pixels black or white.
            dashed.set_dash_offset(0.5);
            snapshot.append_stroke(&outline, &gsk::Stroke::new(1.0), &SELECTION_LIGHT);
            snapshot.append_stroke(&outline, &dashed, &SELECTION_DARK);
        }

        /// The text being typed, over the image as it will land (its pixels, drawn in device
        /// pixels and scaled as the tiles are), in a dashed frame, with the caret: a dark
        /// line on a light one, seen on any colour.
        fn draw_text(&self, snapshot: &gtk::Snapshot, view: &View, dev: Device) {
            let text = self.text.borrow();
            let Some(text) = text.as_ref() else {
                return;
            };
            if let Some(r) = text.raster() {
                let at = (r.rect.x as i64, r.rect.y as i64);
                let shown = shown_rect(
                    view,
                    dev,
                    at,
                    (r.rect.width, r.rect.height),
                    view.zoom >= 1.0,
                );
                let filter = match view.zoom >= 1.0 {
                    true => gsk::ScalingFilter::Nearest,
                    false => gsk::ScalingFilter::Trilinear,
                };
                snapshot.save();
                snapshot.scale(1.0 / dev.scale as f32, 1.0 / dev.scale as f32);
                snapshot.append_scaled_texture(&r.texture, filter, &shown);
                snapshot.restore();
            }
            let (x, y, w, h) = text_area(text, view);
            let builder = gsk::PathBuilder::new();
            builder.add_rect(&graphene::Rect::new(
                x as f32 + 0.5,
                y as f32 + 0.5,
                w as f32 - 1.0,
                h as f32 - 1.0,
            ));
            let frame = builder.to_path();
            let dashed = gsk::Stroke::new(1.0);
            dashed.set_dash(&[4.0, 4.0]);
            snapshot.append_stroke(&frame, &gsk::Stroke::new(1.0), &CARET_LIGHT);
            snapshot.append_stroke(&frame, &dashed, &FRAME_DARK);
            let (cx, cy, ch) = text.caret();
            let (cx, cy) = view.to_widget((cx, cy));
            let cx = (dev.x(cx) as f64 / dev.scale) as f32;
            let ch = (ch * view.zoom) as f32;
            snapshot.append_color(
                &CARET_LIGHT,
                &graphene::Rect::new(cx - 1.0, cy as f32, 3.0, ch),
            );
            snapshot.append_color(&CARET_DARK, &graphene::Rect::new(cx, cy as f32, 1.0, ch));
        }

        /// The shape being dragged, over the image: its rectangles where the image's pixels
        /// they cover are on screen, in device pixels (edges rounded to them from 100% up,
        /// as the tiles' are).
        fn draw_preview(&self, snapshot: &gtk::Snapshot, view: &View, dev: Device) {
            let preview = self.preview.borrow();
            for (r, c) in preview.iter() {
                let at = (r.x as i64, r.y as i64);
                snapshot.append_color(
                    &colours::rgba(*c),
                    &shown_rect(view, dev, at, (r.width, r.height), view.zoom >= 1.0),
                );
            }
        }

        /// The tip's size around the pointer, where a drag would draw, once it's larger
        /// than a few screen pixels: a dark ring on a light one, seen on any colour. Not
        /// while panning.
        fn draw_tip_outline(&self, snapshot: &gtk::Snapshot, view: &View) {
            let (Some(tip), Some(p)) = (self.tip(), self.pointer.get()) else {
                return;
            };
            let w = tip.width().max(1) as f64;
            let panning = self.space.get() || matches!(self.drag.get(), Drag::Pan { .. });
            if w * view.zoom < OUTLINE_MIN_PX || panning {
                return;
            }
            // The disc's centre: the pixel's centre for odd widths, its corner for even ones
            // (they lean right and down).
            let (x, y) = view.to_image(p);
            let shift = if tip.width() % 2 == 0 { 1.0 } else { 0.5 };
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

        /// The checkerboard over `visible` (device pixels), its squares from `(ox, oy)`, each
        /// [`CHECK`] widget pixels rounded to whole device pixels.
        fn checkerboard(
            &self,
            snapshot: &gtk::Snapshot,
            dev: Device,
            visible: &graphene::Rect,
            (ox, oy): (f32, f32),
        ) {
            let check = (CHECK * dev.scale).round().max(1.0) as f32;
            let cell = graphene::Rect::new(ox, oy, 2.0 * check, 2.0 * check);
            snapshot.push_repeat(visible, Some(&cell));
            snapshot.append_color(&CHECK_LIGHT, &cell);
            snapshot.append_color(&CHECK_DARK, &graphene::Rect::new(ox, oy, check, check));
            snapshot.append_color(
                &CHECK_DARK,
                &graphene::Rect::new(ox + check, oy + check, check, check),
            );
            snapshot.pop();
        }

        /// The tiles `visible` (device pixels) shows, built where they aren't yet.
        fn draw_tiles(
            &self,
            snapshot: &gtk::Snapshot,
            view: &View,
            dev: Device,
            visible: &graphene::Rect,
        ) {
            let doc = self.doc.borrow();
            let Some(doc) = doc.as_ref() else { return };
            let image = doc.image();
            let (iw, ih) = (image.width(), image.height());
            let (cols, rows) = (iw.div_ceil(TILE_PX), ih.div_ceil(TILE_PX));
            let widget = |px: f32| px as f64 / dev.scale;
            let a = view.to_image((widget(visible.x()), widget(visible.y())));
            let b = view.to_image((
                widget(visible.x() + visible.width()),
                widget(visible.y() + visible.height()),
            ));
            let range = |lo: f64, hi: f64, n: u32, len: u32| {
                let first = (lo.floor().max(0.0) as u32).min(len - 1) / TILE_PX;
                let last = ((hi.ceil().max(1.0) as u32).min(len) - 1) / TILE_PX;
                first..(last + 1).min(n)
            };
            // Tile edges come from one formula, rounded to device pixels, so neighbours
            // meet exactly: no seam and no overlap, whatever the renderer. Rounding moves
            // a tile at most half a device pixel from where the view puts it.
            let at = |origin: f64, px: u32, len: u32| origin + px.min(len) as f64 * view.zoom;
            let filter = if view.zoom >= 1.0 {
                gsk::ScalingFilter::Nearest
            } else {
                gsk::ScalingFilter::Trilinear
            };
            let mut tiles = self.tiles.borrow_mut();
            tiles.resize((cols * rows) as usize, None);
            for row in range(a.1, b.1, rows, ih) {
                let (y0, y1) = (
                    dev.y(at(view.origin.1, row * TILE_PX, ih)),
                    dev.y(at(view.origin.1, (row + 1) * TILE_PX, ih)),
                );
                for col in range(a.0, b.0, cols, iw) {
                    let (x0, x1) = (
                        dev.x(at(view.origin.0, col * TILE_PX, iw)),
                        dev.x(at(view.origin.0, (col + 1) * TILE_PX, iw)),
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
    texture(image, tile_rect(image, col, row))
}

/// The pixels of `rect` (inside `image`) as a texture.
fn texture(image: &Image, rect: Rect) -> gdk::Texture {
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

/// A clipboard image's pixels, straight-alpha RGBA8 whatever the texture's own format,
/// with the texture that shows them as they'll land ([`Canvas::paste`]). Copies every
/// pixel twice: run it off the UI thread.
pub fn pasted(clipboard: &gdk::Texture) -> Result<(Image, gdk::Texture), image_model::SizeError> {
    let mut downloader = gdk::TextureDownloader::new(clipboard);
    downloader.set_format(gdk::MemoryFormat::R8g8b8a8);
    let (bytes, stride) = downloader.download_bytes();
    let (w, h) = (clipboard.width() as u32, clipboard.height() as u32);
    let pixels = Image::from_rows(w, h, &bytes, stride)?;
    let shown = texture(&pixels, pixels.bounds());
    Ok((pixels, shown))
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

/// Where the canvas lands on the device pixels of the surface it shows on.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Device {
    /// Device pixels a widget pixel.
    scale: f64,
    /// How far into a device pixel the widget's top-left corner falls, across and down:
    /// 0 unless a fractional scale puts it between pixels.
    offset: (f64, f64),
}

impl Device {
    /// Widget x coordinate `at` rounded to the nearest device pixel edge, in device pixels
    /// from the widget's left edge.
    fn x(self, at: f64) -> f32 {
        ((at * self.scale + self.offset.0).round() - self.offset.0) as f32
    }

    /// Widget y coordinate `at` rounded to the nearest device pixel edge, in device pixels
    /// from the widget's top edge.
    fn y(self, at: f64) -> f32 {
        ((at * self.scale + self.offset.1).round() - self.offset.1) as f32
    }

    /// A rectangle in device pixels as widget coordinates.
    fn to_widget(self, r: &graphene::Rect) -> graphene::Rect {
        let s = self.scale as f32;
        graphene::Rect::new(r.x() / s, r.y() / s, r.width() / s, r.height() / s)
    }
}

/// The device rectangle the `w` × `h` image pixels from `(x, y)` show in, from the
/// widget's top-left corner: with `snap`, edges rounded to device pixels, from the same
/// formula as the tiles', so what's drawn there lines up with them pixel for pixel.
fn shown_rect(
    view: &View,
    dev: Device,
    (x, y): (i64, i64),
    (w, h): (u32, u32),
    snap: bool,
) -> graphene::Rect {
    let at = |origin: f64, px: i64| origin + px as f64 * view.zoom;
    let (ax0, ax1) = (at(view.origin.0, x), at(view.origin.0, x + w as i64));
    let (ay0, ay1) = (at(view.origin.1, y), at(view.origin.1, y + h as i64));
    let (x0, x1, y0, y1) = match snap {
        true => (dev.x(ax0), dev.x(ax1), dev.y(ay0), dev.y(ay1)),
        false => {
            let s = |at: f64| (at * dev.scale) as f32;
            (s(ax0), s(ax1), s(ay0), s(ay1))
        }
    };
    graphene::Rect::new(x0, y0, x1 - x0, y1 - y0)
}

/// Where the text's frame goes, in widget coordinates `(x, y, width, height)`: around the
/// text and its caret, [`TEXT_PAD`] off them. A click inside it moves the caret.
fn text_area(text: &TextBox, view: &View) -> (f64, f64, f64, f64) {
    let (fx, fy, fw, fh) = text.frame();
    let (cx, cy, ch) = text.caret();
    let a = view.to_widget((fx.min(cx), fy.min(cy)));
    let b = view.to_widget(((fx + fw).max(cx), (fy + fh).max(cy + ch)));
    (
        a.0 - TEXT_PAD,
        a.1 - TEXT_PAD,
        b.0 - a.0 + 2.0 * TEXT_PAD,
        b.1 - a.1 + 2.0 * TEXT_PAD,
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
        canvas.update_property(&[gtk::accessible::Property::Label("Canvas")]);
        let imp = canvas.imp();
        imp.doc.replace(Some(Document::new(image)));
        imp.actual_size.set(actual_size);
        canvas.describe();
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

    /// Whether there's anything to undo: a step, text being typed, or floating pixels
    /// (undo drops them); nothing while an Image menu command runs.
    pub fn can_undo(&self) -> bool {
        !self.busy()
            && (self.typing()
                || self.floating()
                || self
                    .imp()
                    .doc
                    .borrow()
                    .as_ref()
                    .is_some_and(Document::can_undo))
    }

    /// Whether there's a step to redo; not while text is typed, pixels float, or an Image
    /// menu command runs.
    pub fn can_redo(&self) -> bool {
        !self.busy()
            && !self.typing()
            && !self.floating()
            && self
                .imp()
                .doc
                .borrow()
                .as_ref()
                .is_some_and(Document::can_redo)
    }

    /// Where the image stands in its history ([`Document::position`]).
    pub fn position(&self) -> Option<u64> {
        self.imp()
            .doc
            .borrow()
            .as_ref()
            .and_then(Document::position)
    }

    /// A copy of the image as it is now, and its position, for saving: editing goes on
    /// while the copy is written.
    pub fn copy_image(&self) -> (Image, Option<u64>) {
        let doc = self.imp().doc.borrow();
        let doc = doc.as_ref().expect("a canvas has a document");
        (doc.image().clone(), doc.position())
    }

    /// Takes back the last step; while text is typed, drops the text instead, and while
    /// pixels float, drops the move (they were never on the image).
    pub fn undo(&self) {
        if self.busy() || self.cancel_text() || self.drop_floating() {
            return;
        }
        self.set_drag(Drag::None);
        self.history(Document::undo);
    }

    /// Does the last step undone again; nothing while text is typed or pixels float.
    pub fn redo(&self) {
        if self.busy() || self.typing() || self.floating() {
            return;
        }
        self.set_drag(Drag::None);
        self.history(Document::redo);
    }

    /// Undo or redo: shows the pixels it changed, or the image anew when its size changed
    /// (a transform's step).
    fn history(&self, step: fn(&mut Document) -> Option<Rect>) {
        let size = self.image_size();
        let changed = self.with_doc(step);
        if self.image_size() != size {
            self.reshaped(size);
        } else {
            self.invalidate(changed);
        }
    }

    /// Whether an Image menu command is running ([`Canvas::transform`]): until it's done
    /// nothing changes the image.
    pub fn busy(&self) -> bool {
        self.imp().busy.get()
    }

    /// Image → Crop to Selection: the image becomes the selection's pixels (floating ones
    /// landing first, the selection where they landed), one undo step. Nothing without a
    /// selection.
    pub fn crop(&self) {
        if self.busy() {
            return;
        }
        self.commit();
        if let Some(r) = self.imp().selection.get() {
            self.transform(Transform::Crop(r));
        }
    }

    /// An Image menu command: text being typed and floating pixels go on the image first,
    /// then `transform` runs off the UI thread on a copy of the image, with the canvas
    /// insensitive and the image unchanging meanwhile; its result becomes the image as
    /// one undo step, shown anew ([`Canvas::reshaped`]), the selection gone.
    pub fn transform(&self, transform: Transform) {
        if self.busy() {
            return;
        }
        self.commit();
        let image = self.with_doc(|d| (!transform.unchanged(d.image())).then(|| d.image().clone()));
        let Some(image) = image else {
            return;
        };
        self.imp().busy.set(true);
        self.set_sensitive(false);
        self.emit_changed();
        let canvas = self.downgrade();
        glib::spawn_future_local(async move {
            let t0 = Instant::now();
            let result = gio::spawn_blocking(move || transform.apply(&image)).await;
            let Some(canvas) = canvas.upgrade() else {
                return;
            };
            let ms = t0.elapsed().as_secs_f64() * 1e3;
            canvas.imp().busy.set(false);
            canvas.set_sensitive(true);
            match result {
                Ok(Ok(image)) => {
                    let size = (image.width(), image.height());
                    tracing::info!("{transform:?} to {}×{} in {ms:.1} ms", size.0, size.1);
                    let before = canvas.image_size();
                    canvas.with_doc(|d| d.install(transform, image));
                    canvas.reshaped(before);
                }
                Ok(Err(e)) => {
                    tracing::warn!("{transform:?}: {e}");
                    canvas.emit_changed();
                }
                Err(_) => {
                    tracing::warn!("{transform:?} failed unexpectedly");
                    canvas.emit_changed();
                }
            }
            canvas.grab_focus();
        });
    }

    /// Crash recovery: `image` (a journal's pixels) becomes the image as one undo step
    /// ([`Document::replace`]), shown anew; one undo gives back the image there was.
    pub fn restore(&self, image: Image) {
        self.commit();
        let before = self.image_size();
        self.with_doc(|d| d.replace(image));
        self.reshaped(before);
    }

    /// After a transform, or undoing or redoing one that changed the size (the image was
    /// `before` in size): every tile texture goes and the selection goes; when the size
    /// changed the image shows as an opened one does, at 100% centred or fitted when
    /// larger than the canvas, else the view stays (a flip keeps the zoom and the part in
    /// view).
    fn reshaped(&self, before: (u32, u32)) {
        let imp = self.imp();
        imp.tiles.borrow_mut().clear();
        imp.selection.set(None);
        let size = self.image_size();
        if let (Some(v), true) = (imp.view.get(), size != before) {
            imp.view.set(Some(View::new(size, v.size)));
        }
        self.describe();
        self.queue_draw();
        self.selection_shown();
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

    /// What the pointer's buttons do. Text being typed takes a new look from the text
    /// tool, and goes on the image when another tool is chosen; the selection goes when
    /// another tool is chosen, its floating pixels landing first.
    pub fn set_action(&self, action: Action) {
        let imp = self.imp();
        imp.action.set(Some(action));
        match action {
            Action::Text(look) => {
                self.deselect();
                self.edit_text(|t| t.set_look(look));
            }
            Action::Select => {
                self.commit_text();
            }
            _ => {
                self.commit_text();
                self.deselect();
            }
        }
        if imp.drag.get() == Drag::None && !imp.space.get() {
            self.set_cursor_from_name(Some(self.idle_cursor()));
        }
        self.queue_draw();
    }

    /// The pointer's cursor while nothing is dragged: an I-beam for text, a move cursor
    /// over the selection, else a crosshair.
    fn idle_cursor(&self) -> &'static str {
        match self.imp().action.get() {
            Some(Action::Text(_)) => "text",
            Some(Action::Select) if self.over_selection() => "move",
            _ => "crosshair",
        }
    }

    /// Space held or let go: while held, a primary-button drag pans.
    pub fn set_space(&self, held: bool) {
        let imp = self.imp();
        if imp.space.replace(held) != held && imp.drag.get() == Drag::None {
            self.set_cursor_from_name(Some(if held { "grab" } else { self.idle_cursor() }));
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

    /// Starts a stroke at widget point `at`, when the tool draws.
    pub fn draw_begin(&self, at: (f64, f64)) {
        let Some(view) = self.view() else { return };
        let Some(tip) = self.imp().tip() else {
            return;
        };
        let changed = self.with_doc(|d| d.begin_stroke(tip, view.to_image(at)));
        self.imp().drag.set(Drag::Draw);
        self.invalidate(changed);
    }

    /// Fills the region under widget point `at` with `color`, taking colours within
    /// `tolerance` of the clicked pixel's, one undo step. Synchronous: an 8K fill takes
    /// under 200 ms (ETCH-9).
    pub fn fill(&self, at: (f64, f64), color: Color, tolerance: u8) {
        let Some(view) = self.view() else { return };
        let changed = self.with_doc(|d| d.fill(view.to_image(at), color, tolerance));
        self.invalidate(changed);
    }

    /// The colour picker: hands the colour of the pixel under widget point `at`, alpha
    /// included, to [`Canvas::connect_picked`]'s callback with `background` (a
    /// right-click). Nothing off the image.
    pub fn pick(&self, at: (f64, f64), background: bool) {
        let Some(view) = self.view() else { return };
        let (x, y) = view.to_image(at);
        if x < 0.0 || y < 0.0 {
            return;
        }
        let colour = self.with_doc(|d| d.image().pixel(x as u32, y as u32));
        let picked = self.imp().picked.borrow().clone();
        if let (Some(colour), Some(picked)) = (colour, picked) {
            picked(colour, background);
        }
    }

    /// What a pick does with the colour: `f(colour, background)`.
    pub fn connect_picked(&self, f: impl Fn(Color, bool) + 'static) {
        self.imp().picked.replace(Some(std::rc::Rc::new(f)));
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

    /// Starts dragging a shape at widget point `at`, when the tool draws one. Until
    /// [`Canvas::shape_end`] the shape shows over the image, which stays as it was.
    pub fn shape_begin(&self, at: (f64, f64)) {
        let (Some(view), Some(Action::Shape(tool))) = (self.view(), self.imp().action.get()) else {
            return;
        };
        let from = view.to_image(at);
        self.set_drag(Drag::Shape {
            from,
            shape: tool.at(from, from, false),
        });
    }

    /// Takes the dragged shape's far corner (a line's end) to widget point `to`;
    /// `constrain` (Shift held) keeps a line to 45° steps and a box square.
    pub fn shape_to(&self, to: (f64, f64), constrain: bool) {
        let Some(view) = self.view() else { return };
        let Drag::Shape { from, shape } = self.imp().drag.get() else {
            return;
        };
        let next = shape.tool.at(from, view.to_image(to), constrain);
        if next != shape {
            self.set_drag(Drag::Shape { from, shape: next });
        }
    }

    /// Draws the dragged shape on the image, one undo step.
    pub fn shape_end(&self) {
        let Drag::Shape { shape, .. } = self.imp().drag.get() else {
            return;
        };
        self.set_drag(Drag::None);
        let changed = self.with_doc(|d| d.shape(shape));
        self.invalidate(changed);
    }

    /// Escape: drops the shape being dragged, leaving the image as it was. Whether a shape
    /// was being dragged.
    pub fn cancel_shape(&self) -> bool {
        let dragging = matches!(self.imp().drag.get(), Drag::Shape { .. });
        if dragging {
            self.set_drag(Drag::None);
        }
        dragging
    }

    /// The selection's size, while there's one (its floating pixels' while they float).
    pub fn selection_size(&self) -> Option<(u32, u32)> {
        self.selection_box().map(|(_, size)| size)
    }

    /// Where the selection is, in image pixels: its top-left pixel and its size (floating
    /// pixels may be partly off the image).
    fn selection_box(&self) -> Option<((i64, i64), (u32, u32))> {
        let imp = self.imp();
        if let Some(f) = imp.floating.borrow().as_ref() {
            return Some((f.at(), f.size()));
        }
        imp.selection
            .get()
            .map(|r| ((r.x as i64, r.y as i64), (r.width, r.height)))
    }

    /// Whether the selection's pixels float: lifted, not yet on the image.
    pub fn floating(&self) -> bool {
        self.imp().floating.borrow().is_some()
    }

    /// Whether image pixel `(x, y)` is in the selection.
    fn in_selection(&self, (x, y): (i64, i64)) -> bool {
        self.selection_box().is_some_and(|((sx, sy), (w, h))| {
            x >= sx && y >= sy && x - sx < w as i64 && y - sy < h as i64
        })
    }

    /// Whether the pointer is over the selection.
    fn over_selection(&self) -> bool {
        let (Some(view), Some(p)) = (self.view(), self.imp().pointer.get()) else {
            return false;
        };
        let (x, y) = self.to_shown(&view, p);
        self.in_selection((x.floor() as i64, y.floor() as i64))
    }

    /// After the selection changed: redraws, sets the cursor, and tells the status bar.
    fn selection_shown(&self) {
        let imp = self.imp();
        if imp.drag.get() == Drag::None && !imp.space.get() {
            self.set_cursor_from_name(Some(self.idle_cursor()));
        }
        self.queue_draw();
        self.emit_changed();
    }

    /// Edit → Select All: the whole image, floating pixels landing first.
    pub fn select_all(&self) {
        self.deselect();
        let (w, h) = self.image_size();
        self.imp().selection.set(Some(Rect::new(0, 0, w, h)));
        self.selection_shown();
    }

    /// Edit → Deselect: the selection goes, its floating pixels landing first. Whether
    /// there was one.
    pub fn deselect(&self) -> bool {
        let imp = self.imp();
        if matches!(imp.drag.get(), Drag::Select { .. }) {
            imp.drag.set(Drag::None);
        }
        let landed = self.land();
        let had = imp.selection.take().is_some() || landed;
        if had {
            self.selection_shown();
        }
        had
    }

    /// Lands the floating pixels, one undo step (none when they're back where they were
    /// lifted from); the selection stays where they landed. Whether pixels floated.
    fn land(&self) -> bool {
        let imp = self.imp();
        let Some(f) = self.take_floating() else {
            return false;
        };
        let (w, h) = self.image_size();
        imp.selection.set(f.bounds(Rect::new(0, 0, w, h)));
        let changed = self.with_doc(|d| d.land(f));
        self.queue_draw();
        self.invalidate(changed);
        true
    }

    /// Takes the floating pixels off the canvas, ending a drag moving them.
    fn take_floating(&self) -> Option<Floating> {
        let imp = self.imp();
        let f = imp.floating.take()?;
        imp.floating_texture.take();
        if matches!(imp.drag.get(), Drag::Move { .. }) {
            imp.drag.set(Drag::None);
        }
        Some(f)
    }

    /// Lifts the selection's pixels to float over the image, unless they float already.
    /// Whether pixels float.
    fn lift(&self) -> bool {
        let imp = self.imp();
        if self.floating() {
            return true;
        }
        let Some(r) = imp.selection.get() else {
            return false;
        };
        let Some(f) = self.with_doc(|d| d.lift(r)) else {
            return false;
        };
        let pixels = f.pixels();
        imp.floating_texture
            .replace(Some(texture(pixels, pixels.bounds())));
        imp.floating.replace(Some(f));
        imp.selection.set(None);
        self.queue_draw();
        true
    }

    /// Undo while pixels float: drops the move, the image as it was, the selection back
    /// where they were lifted from. Whether pixels floated.
    fn drop_floating(&self) -> bool {
        let Some(f) = self.take_floating() else {
            return false;
        };
        self.imp().selection.set(f.source());
        self.selection_shown();
        true
    }

    /// Edit → Delete: clears the selection's pixels to transparency, one undo step. Pixels
    /// floating are dropped and the place they were lifted from cleared. The selection
    /// stays (where they floated). Whether there was one; nothing while an Image menu
    /// command runs.
    pub fn delete_selection(&self) -> bool {
        if self.busy() {
            return false;
        }
        let imp = self.imp();
        let (w, h) = self.image_size();
        let cleared = match self.take_floating() {
            Some(f) => {
                imp.selection.set(f.bounds(Rect::new(0, 0, w, h)));
                f.source()
            }
            None => match imp.selection.get() {
                Some(r) => Some(r),
                None => return false,
            },
        };
        let changed = cleared.and_then(|r| self.with_doc(|d| d.clear(r)));
        self.invalidate(changed);
        self.selection_shown();
        true
    }

    /// The arrow keys: moves the selection's pixels `dx` right and `dy` down, lifting them
    /// first. Whether there's a selection.
    pub fn nudge(&self, dx: i64, dy: i64) -> bool {
        if !self.lift() {
            return false;
        }
        if let Some(f) = self.imp().floating.borrow_mut().as_mut() {
            f.move_by(dx, dy);
        }
        self.selection_shown();
        true
    }

    /// Puts what's pending on the image (before saving or closing): text being typed and
    /// floating pixels, each one undo step.
    pub fn commit(&self) {
        self.commit_text();
        self.land();
    }

    /// Edit → Copy: puts the selection's pixels (the floating ones while they float) on
    /// the clipboard as an image, straight alpha, which GTK offers other apps as PNG and
    /// the other image types it writes. Whether there was a selection.
    pub fn copy(&self) -> bool {
        let imp = self.imp();
        let texture = match imp.floating_texture.borrow().clone() {
            Some(t) => t,
            None => {
                let Some(r) = imp.selection.get() else {
                    return false;
                };
                let doc = imp.doc.borrow();
                texture(doc.as_ref().expect("a canvas has a document").image(), r)
            }
        };
        self.clipboard().set_texture(&texture);
        true
    }

    /// Edit → Cut: copies the selection, then deletes it ([`Canvas::delete_selection`]),
    /// one undo step. Whether there was a selection.
    pub fn cut(&self) -> bool {
        self.copy() && self.delete_selection()
    }

    /// Edit → Paste, once the clipboard's image is read: the selection goes (floating
    /// pixels landing), and `pixels` (shown as `shown`, from [`pasted`]) float
    /// over the image with their top-left pixel at the top-left of the image's visible
    /// part (the image's own top-left when that shows), until they land as one undo step
    /// as moved pixels do. Pixels larger than the image float whole and land clipped.
    /// Nothing while an Image menu command runs.
    pub fn paste(&self, pixels: Image, shown: gdk::Texture) {
        if self.busy() {
            return;
        }
        self.commit_text();
        self.deselect();
        let (w, h) = self.image_size();
        let at = self.view().map_or((0, 0), |view| {
            let (x, y) = self.to_shown(&view, (0.0, 0.0));
            let first = |v: f64, side: u32| (v.ceil().max(0.0) as i64).min(side as i64 - 1);
            (first(x, w), first(y, h))
        });
        let imp = self.imp();
        imp.floating_texture.replace(Some(shown));
        imp.floating.replace(Some(Floating::new(pixels, at)));
        self.selection_shown();
    }

    /// A primary-button press at widget point `at` with the selection: inside the
    /// selection it starts moving its pixels, lifting them; elsewhere floating pixels land
    /// and a new selection starts there.
    pub fn select_begin(&self, at: (f64, f64)) {
        let Some(view) = self.view() else { return };
        let p = self.to_shown(&view, at);
        let imp = self.imp();
        if self.in_selection((p.0.floor() as i64, p.1.floor() as i64)) && self.lift() {
            let at = imp.floating.borrow().as_ref().map(Floating::at);
            if let Some(at) = at {
                imp.drag.set(Drag::Move { from: p, at });
                self.set_cursor_from_name(Some("move"));
            }
        } else {
            self.deselect();
            imp.drag.set(Drag::Select { from: p });
        }
        self.selection_shown();
    }

    /// Takes the selection drag to widget point `to`: the rectangle from where it began
    /// to there, or the floating pixels as far as the pointer moved.
    pub fn select_to(&self, to: (f64, f64)) {
        let Some(view) = self.view() else { return };
        let p = self.to_shown(&view, to);
        let imp = self.imp();
        match imp.drag.get() {
            Drag::Select { from } => {
                let (w, h) = self.image_size();
                let r = spanning(from, p, Rect::new(0, 0, w, h));
                if imp.selection.replace(r) != r {
                    self.selection_shown();
                }
            }
            Drag::Move { from, at } => {
                let to = (
                    at.0 + (p.0 - from.0).round() as i64,
                    at.1 + (p.1 - from.1).round() as i64,
                );
                let moved = imp.floating.borrow_mut().as_mut().is_some_and(|f| {
                    let moved = f.at() != to;
                    f.move_to(to);
                    moved
                });
                if moved {
                    self.queue_draw();
                }
            }
            Drag::None | Drag::Draw | Drag::Shape { .. } | Drag::Pan { .. } => {}
        }
    }

    /// Widget point `at` in image coordinates as the image shows on screen: from its
    /// origin rounded to device pixels, as the tiles are drawn, so a pointer on a pixel's
    /// edge on screen is on its edge in the image, and a drag across 200 screen pixels at
    /// 100% selects 200.
    fn to_shown(&self, view: &View, (x, y): (f64, f64)) -> (f64, f64) {
        let dev = self.device();
        (
            (x - dev.x(view.origin.0) as f64 / dev.scale) / view.zoom,
            (y - dev.y(view.origin.1) as f64 / dev.scale) / view.zoom,
        )
    }

    /// Ends the selection drag: a click leaves no selection; moved pixels stay floating.
    pub fn select_end(&self) {
        self.imp().drag.set(Drag::None);
        self.selection_shown();
    }

    /// Whether text is being typed (not yet on the image).
    pub fn typing(&self) -> bool {
        self.imp().text.borrow().is_some()
    }

    /// A primary-button click at widget point `at` with the text tool: in the text being
    /// typed, the caret goes there; elsewhere that text goes on the image; with none, text
    /// in `look` starts there, when it's on the image.
    pub fn text_click(&self, at: (f64, f64), look: TextLook) {
        let Some(view) = self.view() else { return };
        let inside = self.imp().text.borrow().as_ref().map(|t| {
            let (x, y, w, h) = text_area(t, &view);
            at.0 >= x && at.0 < x + w && at.1 >= y && at.1 < y + h
        });
        let p = view.to_image(at);
        match inside {
            Some(true) => {
                if let Some(im) = self.imp().im.get() {
                    im.reset();
                }
                self.edit_text(|t| t.click(p));
            }
            Some(false) => {
                self.commit_text();
            }
            None => {
                let bounds = self.with_doc(|d| d.image().bounds());
                let (x, y) = (p.0.floor(), p.1.floor());
                if x < 0.0 || y < 0.0 || x >= bounds.width as f64 || y >= bounds.height as f64 {
                    return;
                }
                let text = TextBox::new(&self.pango_context(), (x as i64, y as i64), look, bounds);
                self.imp().text.replace(Some(text));
                let imp = self.imp();
                if let (Some(keys), Some(im)) = (imp.text_keys.get(), imp.im.get()) {
                    keys.set_im_context(Some(im));
                    im.focus_in();
                }
                self.text_shown();
                self.emit_changed();
            }
        }
    }

    /// Puts the text being typed on the image, one undo step (none when it has no ink
    /// there). Whether text was being typed.
    pub fn commit_text(&self) -> bool {
        let Some(text) = self.end_typing() else {
            return false;
        };
        let changed = text
            .into_raster()
            .and_then(|r| self.with_doc(|d| d.blend((r.rect.x as i64, r.rect.y as i64), r.pixels)));
        self.queue_draw();
        self.invalidate(changed);
        true
    }

    /// Escape: drops the text being typed, leaving the image as it was. Whether text was
    /// being typed.
    pub fn cancel_text(&self) -> bool {
        let typed = self.end_typing().is_some();
        if typed {
            self.queue_draw();
            self.emit_changed();
        }
        typed
    }

    /// Stops typing: the input method lets go (a composition it ends goes into the text
    /// first) and the text is taken off the canvas.
    fn end_typing(&self) -> Option<TextBox> {
        let imp = self.imp();
        if !self.typing() {
            return None;
        }
        if let (Some(keys), Some(im)) = (imp.text_keys.get(), imp.im.get()) {
            im.reset();
            im.focus_out();
            keys.set_im_context(None::<&gtk::IMContext>);
        }
        let text = imp.text.take();
        self.describe();
        text
    }

    /// Runs `f` on the text being typed, if any, and shows the result.
    fn edit_text(&self, f: impl FnOnce(&mut TextBox)) {
        let edited = self.imp().text.borrow_mut().as_mut().map(f);
        if edited.is_some() {
            self.text_shown();
        }
    }

    /// After the text being typed changed: redraws, tells the input method where the
    /// caret is, and describes the canvas again.
    fn text_shown(&self) {
        self.queue_draw();
        let imp = self.imp();
        if let (Some(view), Some(text), Some(im)) =
            (self.view(), imp.text.borrow().as_ref(), imp.im.get())
        {
            let (x, y, h) = text.caret();
            let (x, y) = view.to_widget((x, y));
            im.set_cursor_location(&gdk::Rectangle::new(
                x as i32,
                y as i32,
                1,
                (h * view.zoom).ceil() as i32,
            ));
        }
        self.describe();
    }

    /// The canvas's accessible description: the image's size, and the text being typed.
    fn describe(&self) {
        let (w, h) = self.image_size();
        let description = match self.imp().text.borrow().as_ref() {
            Some(t) => format!("{w}×{h} image, typing “{}”", t.text()),
            None => format!("{w}×{h} image"),
        };
        self.update_property(&[gtk::accessible::Property::Description(&description)]);
    }

    /// A key pressed while text is typed (after the input method): the text's keys edit
    /// it, Enter puts it on the image, Escape drops it.
    fn text_key(&self, key: gdk::Key, mods: gdk::ModifierType) -> glib::Propagation {
        let outcome = match self.imp().text.borrow_mut().as_mut() {
            Some(t) => t.key(key, mods),
            None => return glib::Propagation::Proceed,
        };
        match outcome {
            KeyOutcome::Commit => {
                self.commit_text();
            }
            KeyOutcome::Cancel => {
                self.cancel_text();
            }
            KeyOutcome::Changed | KeyOutcome::Moved => self.text_shown(),
            KeyOutcome::Ignored => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    }

    /// Typing: an input method (the desktop's, so any script composes) and the keys it
    /// passes on, while text is typed.
    fn add_text_inputs(&self) {
        let im = gtk::IMMulticontext::new();
        im.set_client_widget(Some(self));
        im.connect_commit({
            let canvas = self.downgrade();
            move |_, s| {
                if let Some(canvas) = canvas.upgrade() {
                    canvas.edit_text(|t| {
                        t.set_preedit(None);
                        t.insert(s);
                    });
                }
            }
        });
        im.connect_preedit_changed({
            let canvas = self.downgrade();
            move |im| {
                let Some(canvas) = canvas.upgrade() else {
                    return;
                };
                let (s, attrs, cursor) = im.preedit_string();
                let cursor = s
                    .char_indices()
                    .nth(cursor.max(0) as usize)
                    .map_or(s.len(), |(i, _)| i);
                let preedit = (!s.is_empty()).then(|| (s.to_string(), attrs, cursor));
                canvas.edit_text(|t| t.set_preedit(preedit));
            }
        });
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed({
            let canvas = self.downgrade();
            move |_, key, _, state| match canvas.upgrade() {
                Some(canvas) => {
                    canvas.text_key(key, state & gtk::accelerator_get_default_mod_mask())
                }
                None => glib::Propagation::Proceed,
            }
        });
        self.add_controller(keys.clone());
        let focus = gtk::EventControllerFocus::new();
        focus.connect_enter({
            let canvas = self.downgrade();
            move |_| {
                if let Some(canvas) = canvas.upgrade().filter(Canvas::typing) {
                    if let Some(im) = canvas.imp().im.get() {
                        im.focus_in();
                    }
                }
            }
        });
        focus.connect_leave({
            let im = im.clone();
            move |_| im.focus_out()
        });
        self.add_controller(focus);
        let imp = self.imp();
        imp.im.set(im).expect("text inputs are added once");
        imp.text_keys.set(keys).expect("text inputs are added once");
    }

    /// What a drag does now; a shape's preview follows.
    fn set_drag(&self, drag: Drag) {
        let imp = self.imp();
        imp.drag.set(drag);
        let rects = match drag {
            Drag::Shape { shape, .. } => shape.rects(self.with_doc(|d| d.image().bounds())),
            Drag::None
            | Drag::Draw
            | Drag::Pan { .. }
            | Drag::Select { .. }
            | Drag::Move { .. } => Vec::new(),
        };
        if !(rects.is_empty() && imp.preview.borrow().is_empty()) {
            imp.preview.replace(rects);
            self.queue_draw();
        }
    }

    /// Where the canvas lands on the device pixels of the surface it shows on: their scale,
    /// and how far into one its top-left corner falls.
    fn device(&self) -> Device {
        let Some(native) = self.native() else {
            return Device {
                scale: 1.0,
                offset: (0.0, 0.0),
            };
        };
        let scale = native.surface().map_or(1.0, |s| s.scale());
        let corner = self
            .compute_point(&native, &graphene::Point::zero())
            .unwrap_or_else(graphene::Point::zero);
        let (sx, sy) = native.surface_transform();
        let into = |at: f64| (at * scale).rem_euclid(1.0);
        Device {
            scale,
            offset: (into(corner.x() as f64 + sx), into(corner.y() as f64 + sy)),
        }
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
                if self.typing() {
                    // The caret moved on screen: the input method's window follows it.
                    self.text_shown();
                }
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
                let (button, space) = (g.current_button(), imp.space.get());
                let secondary = button == 3;
                match (button, imp.action.get()) {
                    (
                        1 | 3,
                        Some(Action::Fill {
                            foreground,
                            background,
                            tolerance,
                        }),
                    ) if !space => canvas.fill(
                        (x, y),
                        if secondary { background } else { foreground },
                        tolerance,
                    ),
                    (1 | 3, Some(Action::Pick)) if !space => canvas.pick((x, y), secondary),
                    (1, Some(Action::Shape(_))) if !space => canvas.shape_begin((x, y)),
                    (1, Some(Action::Text(look))) if !space => canvas.text_click((x, y), look),
                    (1, Some(Action::Select)) if !space => canvas.select_begin((x, y)),
                    (1, _) if !space => canvas.draw_begin((x, y)),
                    (1 | 2, _) => {
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
                    Drag::Shape { .. } => {
                        if let Some((x, y)) = g.start_point() {
                            let state = g.current_event_state();
                            let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
                            canvas.shape_to((x + dx, y + dy), shift);
                        }
                    }
                    Drag::Pan { last } => {
                        imp.drag.set(Drag::Pan { last: (dx, dy) });
                        canvas.change_view(|v| v.pan(dx - last.0, dy - last.1));
                    }
                    Drag::Select { .. } | Drag::Move { .. } => {
                        if let Some((x, y)) = g.start_point() {
                            canvas.select_to((x + dx, y + dy));
                        }
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
                    Drag::Shape { .. } => canvas.shape_end(),
                    Drag::Pan { .. } => {
                        canvas.imp().drag.set(Drag::None);
                        let held = canvas.imp().space.get();
                        let idle = canvas.idle_cursor();
                        canvas.set_cursor_from_name(Some(if held { "grab" } else { idle }));
                    }
                    Drag::Select { .. } | Drag::Move { .. } => canvas.select_end(),
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
                if suite_ui::keys::is_primary(state) {
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
                    canvas.follow_selection();
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

    /// With the selection the tool, the cursor shows whether a drag would move the
    /// selection (over it) or select anew.
    fn follow_selection(&self) {
        let imp = self.imp();
        if imp.action.get() != Some(Action::Select)
            || imp.drag.get() != Drag::None
            || imp.space.get()
        {
            return;
        }
        let want = self.idle_cursor();
        if self.cursor().and_then(|c| c.name()).as_deref() != Some(want) {
            self.set_cursor_from_name(Some(want));
        }
    }

    /// Redraws when the pointer moved and the tip's outline follows it.
    fn redraw_outline(&self) {
        let (tip, view) = (self.imp().tip(), self.view());
        if let (Some(tip), Some(view)) = (tip, view) {
            if tip.width() as f64 * view.zoom >= OUTLINE_MIN_PX {
                self.queue_draw();
            }
        }
    }

    /// Ctrl+wheel (⌘ on macOS): each notch down a step out, up a step in, around the
    /// pointer; a touchpad zooms smoothly.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn view(zoom: f64, origin: (f64, f64)) -> View {
        View {
            zoom,
            origin,
            size: (1280.0, 720.0),
            image: (1024.0, 768.0),
        }
    }

    #[test]
    fn tiles_meet_on_device_pixel_edges() {
        let scales = [(1.0, (0.0, 0.0)), (2.0, (0.0, 0.0)), (1.25, (0.25, 0.75))];
        let views = [
            view(1.0, (128.5, 41.5)),
            view(0.791_341_1, (61.37, 16.0)),
            view(1.037_7, (100.25, 3.6)),
            view(0.5, (200.5, 100.5)),
        ];
        for (scale, offset) in scales {
            let dev = Device { scale, offset };
            for v in &views {
                let mut right = None;
                for col in 0..4 {
                    let r = shown_rect(v, dev, (col * 256, 0), (256, 256), true);
                    // Each edge is a whole device pixel of the surface, and each tile starts
                    // where the one before it ends.
                    for edge in [
                        r.x() as f64 + offset.0,
                        (r.x() + r.width()) as f64 + offset.0,
                    ] {
                        assert!((edge - edge.round()).abs() < 1e-3, "{edge} at {scale}");
                    }
                    if let Some(x) = right {
                        assert_eq!(r.x(), x);
                    }
                    right = Some(r.x() + r.width());
                }
            }
        }
    }

    #[test]
    fn at_100_percent_an_image_spans_its_pixels_times_the_scale() {
        for scale in [1.0, 2.0, 3.0] {
            let dev = Device {
                scale,
                offset: (0.0, 0.0),
            };
            let r = shown_rect(&view(1.0, (128.5, 41.5)), dev, (0, 0), (401, 301), true);
            assert_eq!(
                (r.width(), r.height()),
                (401.0 * scale as f32, 301.0 * scale as f32)
            );
        }
    }
}
