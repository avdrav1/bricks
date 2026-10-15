//! [`Document`]: the image, the operations that change it, and their undo history.

use crate::brush::Coverage;
use crate::geom::{join, pixel};
use crate::{Blend, Brush, Color, Fill, Image, Pencil, Rect, Shape, TILE};
use std::collections::BTreeMap;

/// What a stroke draws with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tip {
    /// Hard-edged pixels that replace what's there (the eraser too).
    Pencil(Pencil),
    /// A disc composited over what's there, hard or soft.
    Brush(Brush),
}

impl From<Pencil> for Tip {
    fn from(p: Pencil) -> Tip {
        Tip::Pencil(p)
    }
}

impl From<Brush> for Tip {
    fn from(b: Brush) -> Tip {
        Tip::Brush(b)
    }
}

impl Tip {
    /// The tip's width in pixels.
    pub fn width(&self) -> u32 {
        match self {
            Tip::Pencil(p) => p.width,
            Tip::Brush(b) => b.width,
        }
    }

    /// Draws the stroke's part from pixel `a` to pixel `b`, saving the tiles it changes
    /// in `before` first; `cover` is the stroke's coverage so far (the brush's).
    fn segment(
        &self,
        image: &mut Image,
        before: &mut Saved,
        cover: &mut Coverage,
        a: (i64, i64),
        b: (i64, i64),
    ) -> Option<Rect> {
        match self {
            Tip::Pencil(p) => {
                before.cover(image, p.bounds(image, a, b)?);
                p.line(image, a, b)
            }
            Tip::Brush(brush) => brush.segment(image, before, cover, a, b),
        }
    }
}

/// A stroke: its tip and the pixels the pointer went through, in order. Lines join each
/// to the next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stroke {
    pub tip: Tip,
    pub points: Vec<(i64, i64)>,
}

impl Stroke {
    fn apply(&self, image: &mut Image) -> Option<Rect> {
        let mut changed = None;
        let mut prev = *self.points.first()?;
        match self.tip {
            Tip::Pencil(pencil) => {
                for &p in &self.points {
                    changed = join(changed, pencil.line(image, prev, p));
                    prev = p;
                }
            }
            // The brush blends over the pixels from before the stroke, so it keeps them
            // while it replays, as it did while drawn.
            Tip::Brush(brush) => {
                let (mut before, mut cover) = (Saved::default(), Coverage::default());
                for &p in &self.points {
                    changed = join(
                        changed,
                        brush.segment(image, &mut before, &mut cover, prev, p),
                    );
                    prev = p;
                }
            }
        }
        changed
    }
}

/// One undoable change to the image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Operation {
    Stroke(Stroke),
    Fill(Fill),
    Shape(Shape),
    Blend(Blend),
}

impl Operation {
    /// Applies the operation; it's deterministic, so applying it again to the same pixels
    /// gives the same result (redo depends on that).
    fn apply(&self, image: &mut Image) -> Option<Rect> {
        match self {
            Operation::Stroke(s) => s.apply(image),
            Operation::Fill(f) => f.apply(image, |_, _, _, _| {}),
            Operation::Shape(s) => s.apply(image, |_, _| {}),
            Operation::Blend(b) => b.apply(image),
        }
    }
}

/// The image's tiles from before an operation touched them, keyed by tile column and row.
#[derive(Default)]
pub(crate) struct Saved {
    tiles: BTreeMap<(u32, u32), Vec<u8>>,
}

impl Saved {
    /// The pixels of tile `(col, row)`, clipped to the image.
    pub(crate) fn tile_rect(image: &Image, (col, row): (u32, u32)) -> Rect {
        let (x, y) = (col * TILE, row * TILE);
        Rect::new(
            x,
            y,
            TILE.min(image.width() - x),
            TILE.min(image.height() - y),
        )
    }

    /// Saves every tile of `image` that `rect` overlaps and isn't saved yet; call it before
    /// changing pixels in `rect`.
    pub(crate) fn cover(&mut self, image: &Image, rect: Rect) {
        for row in rect.y / TILE..rect.bottom().div_ceil(TILE) {
            for col in rect.x / TILE..rect.right().div_ceil(TILE) {
                self.tiles
                    .entry((col, row))
                    .or_insert_with(|| image.copy_rect(Saved::tile_rect(image, (col, row))));
            }
        }
    }

    /// The saved pixels of tile `(col, row)`, packed as [`Image::copy_rect`] gives them.
    pub(crate) fn tile(&self, tile: (u32, u32)) -> Option<&[u8]> {
        self.tiles.get(&tile).map(Vec::as_slice)
    }

    /// Writes the saved tiles back; returns the pixels they cover.
    fn restore(&self, image: &mut Image) -> Option<Rect> {
        let mut changed = None;
        for (&tile, pixels) in &self.tiles {
            let rect = Saved::tile_rect(image, tile);
            image.paste_rect(rect, pixels);
            changed = join(changed, Some(rect));
        }
        changed
    }

    fn bytes(&self) -> usize {
        self.tiles.values().map(Vec::len).sum()
    }
}

/// [`Saved`] for operations that write many small pieces (a fill's row spans, a shape's
/// rows): a flag a tile says whether it's saved without looking it up.
struct Covering {
    saved: Saved,
    cols: u32,
    flags: Vec<bool>,
}

impl Covering {
    fn new(image: &Image) -> Covering {
        let cols = image.width().div_ceil(TILE);
        Covering {
            saved: Saved::default(),
            cols,
            flags: vec![false; (cols * image.height().div_ceil(TILE)) as usize],
        }
    }

    /// [`Saved::cover`].
    fn cover(&mut self, image: &Image, rect: Rect) {
        for row in rect.y / TILE..rect.bottom().div_ceil(TILE) {
            for col in rect.x / TILE..rect.right().div_ceil(TILE) {
                let k = (row * self.cols + col) as usize;
                if !self.flags[k] {
                    self.flags[k] = true;
                    self.saved.cover(image, Saved::tile_rect(image, (col, row)));
                }
            }
        }
    }
}

/// A step of history: the operation, the pixels it replaced, and its number (see
/// [`Document::position`]).
struct Step {
    op: Operation,
    before: Saved,
    id: u64,
}

/// The image being edited, and its history. Every change goes through here, and each
/// returns the pixels it may have changed, for the canvas to redraw.
pub struct Document {
    image: Image,
    done: Vec<Step>,
    undone: Vec<Step>,
    /// The stroke being drawn: already on the image, not yet in `done`.
    live: Option<Step>,
    /// The live stroke's coverage (the brush's).
    cover: Coverage,
    /// The number the next step gets: steps are numbered from 1 in the order they're made.
    next_id: u64,
}

impl Document {
    pub fn new(image: Image) -> Document {
        Document {
            image,
            done: Vec::new(),
            undone: Vec::new(),
            live: None,
            cover: Coverage::default(),
            next_id: 1,
        }
    }

    /// Where the image stands in its history: the last step done's number, 0 before any;
    /// `None` while a stroke is under way (the image is between positions). Equal positions
    /// are equal images: undoing back to a position, or redoing to it, gives its pixels
    /// again, and a step made after undoing gets a new number. A window compares the
    /// position with the one its file was saved at.
    pub fn position(&self) -> Option<u64> {
        match self.live {
            Some(_) => None,
            None => Some(self.done.last().map_or(0, |s| s.id)),
        }
    }

    pub fn image(&self) -> &Image {
        &self.image
    }

    /// Starts a stroke at `at`, in image pixels (fractions floor to the pixel they're in;
    /// points off the image are fine, the stroke clips). Ends any stroke under way.
    pub fn begin_stroke(&mut self, tip: impl Into<Tip>, at: (f64, f64)) -> Option<Rect> {
        self.end_stroke();
        let tip = tip.into();
        let p = pixel(at);
        let mut step = Step {
            op: Operation::Stroke(Stroke {
                tip,
                points: vec![p],
            }),
            before: Saved::default(),
            id: self.next_id,
        };
        self.next_id += 1;
        let changed = tip.segment(&mut self.image, &mut step.before, &mut self.cover, p, p);
        self.live = Some(step);
        changed
    }

    /// Continues the stroke under way to `to`; does nothing without one.
    pub fn extend_stroke(&mut self, to: (f64, f64)) -> Option<Rect> {
        let step = self.live.as_mut()?;
        let Operation::Stroke(stroke) = &mut step.op else {
            return None;
        };
        let p = pixel(to);
        let last = *stroke.points.last().expect("a stroke starts with a point");
        if p == last {
            return None;
        }
        stroke.points.push(p);
        let tip = stroke.tip;
        tip.segment(&mut self.image, &mut step.before, &mut self.cover, last, p)
    }

    /// Finishes the stroke under way as one undo step. A stroke that touched no pixel (all
    /// of it off the image) leaves no step.
    pub fn end_stroke(&mut self) {
        self.cover = Coverage::default();
        if let Some(step) = self.live.take() {
            if !step.before.tiles.is_empty() {
                self.done.push(step);
                self.undone.clear();
            }
        }
    }

    /// Fills the 4-connected region of the pixel at `at` (in image pixels; fractions floor)
    /// with `color`: every pixel reached through neighbours whose colour is within
    /// `tolerance` of the clicked pixel's: no channel, alpha included, differs by more
    /// (0: its exact colour). With a fully transparent pixel only alpha counts, so fully
    /// transparent pixels match each other whatever colour they carry. One undo step, keeping only the tiles
    /// the fill wrote to. Nothing happens off the image or when an exact fill would put
    /// down the colour already there. Ends any stroke under way.
    pub fn fill(&mut self, at: (f64, f64), color: Color, tolerance: u8) -> Option<Rect> {
        self.end_stroke();
        let (x, y) = pixel(at);
        let (w, h) = (self.image.width(), self.image.height());
        if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
            return None;
        }
        let fill = Fill {
            at: (x as u32, y as u32),
            color,
            tolerance,
        };
        let mut covering = Covering::new(&self.image);
        let changed = fill.apply(&mut self.image, |image, y, x0, x1| {
            covering.cover(image, Rect::new(x0, y, x1 - x0, 1));
        })?;
        self.record(Operation::Fill(fill), covering.saved);
        Some(changed)
    }

    /// Draws `shape` (a line, rectangle, or ellipse; see [`Shape`]), clipped to the image.
    /// One undo step, keeping only the tiles the shape wrote to. A shape off the image
    /// leaves no step. Ends any stroke under way.
    pub fn shape(&mut self, shape: Shape) -> Option<Rect> {
        self.end_stroke();
        let mut covering = Covering::new(&self.image);
        let changed = shape.apply(&mut self.image, |image, r| covering.cover(image, r))?;
        self.record(Operation::Shape(shape), covering.saved);
        Some(changed)
    }

    /// Lays `pixels` over the image with their top-left pixel at `at` ([`Blend`]:
    /// source-over, clipped to the image), as one undo step keeping the tiles they cover;
    /// redo lays the same pixels again. Pixels wholly off the image leave no step. Ends any
    /// stroke under way.
    pub fn blend(&mut self, at: (i64, i64), pixels: Image) -> Option<Rect> {
        self.end_stroke();
        let blend = Blend { at, pixels };
        let mut before = Saved::default();
        before.cover(&self.image, blend.bounds(&self.image)?);
        let changed = blend.apply(&mut self.image);
        self.record(Operation::Blend(blend), before);
        changed
    }

    /// Makes `op`, done, the last step, with the pixels it replaced; drops the redo.
    fn record(&mut self, op: Operation, before: Saved) {
        self.done.push(Step {
            op,
            before,
            id: self.next_id,
        });
        self.next_id += 1;
        self.undone.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.done.is_empty() || self.live.is_some()
    }

    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }

    /// Takes back the last step (finishing a stroke under way first).
    pub fn undo(&mut self) -> Option<Rect> {
        self.end_stroke();
        let step = self.done.pop()?;
        let changed = step.before.restore(&mut self.image);
        self.undone.push(step);
        changed
    }

    /// Does the last undone step again.
    pub fn redo(&mut self) -> Option<Rect> {
        self.end_stroke();
        let step = self.undone.pop()?;
        let changed = step.op.apply(&mut self.image);
        self.done.push(step);
        changed
    }

    /// The pixel bytes history keeps to undo with.
    pub fn history_bytes(&self) -> usize {
        self.done
            .iter()
            .chain(&self.undone)
            .chain(&self.live)
            .map(|s| s.before.bytes())
            .sum()
    }
}
