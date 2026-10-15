//! [`Document`]: the image, the operations that change it, and their undo history.

use crate::geom::join;
use crate::{Image, Pencil, Rect, TILE};
use std::collections::BTreeMap;

/// Pointer positions beyond this many pixels off the image clamp to it, so far-off points
/// can't overflow the line arithmetic.
const FAR: f64 = 1_000_000.0;

/// A pencil stroke: its tip and the pixels the pointer went through, in order. Lines join
/// each to the next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stroke {
    pub pencil: Pencil,
    pub points: Vec<(i64, i64)>,
}

impl Stroke {
    fn apply(&self, image: &mut Image) -> Option<Rect> {
        let mut changed = None;
        let mut prev = *self.points.first()?;
        for &p in &self.points {
            changed = join(changed, self.pencil.line(image, prev, p));
            prev = p;
        }
        changed
    }
}

/// One undoable change to the image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Operation {
    Stroke(Stroke),
}

impl Operation {
    /// Applies the operation; it's deterministic, so applying it again to the same pixels
    /// gives the same result (redo depends on that).
    fn apply(&self, image: &mut Image) -> Option<Rect> {
        match self {
            Operation::Stroke(s) => s.apply(image),
        }
    }
}

/// The image's tiles from before an operation touched them, keyed by tile column and row.
#[derive(Default)]
struct Saved {
    tiles: BTreeMap<(u32, u32), Vec<u8>>,
}

impl Saved {
    /// The pixels of tile `(col, row)`, clipped to the image.
    fn tile_rect(image: &Image, (col, row): (u32, u32)) -> Rect {
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
    fn cover(&mut self, image: &Image, rect: Rect) {
        for row in rect.y / TILE..rect.bottom().div_ceil(TILE) {
            for col in rect.x / TILE..rect.right().div_ceil(TILE) {
                self.tiles
                    .entry((col, row))
                    .or_insert_with(|| image.copy_rect(Saved::tile_rect(image, (col, row))));
            }
        }
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
    pub fn begin_stroke(&mut self, pencil: Pencil, at: (f64, f64)) -> Option<Rect> {
        self.end_stroke();
        let p = pixel(at);
        let mut step = Step {
            op: Operation::Stroke(Stroke {
                pencil,
                points: vec![p],
            }),
            before: Saved::default(),
            id: self.next_id,
        };
        self.next_id += 1;
        let changed = pencil.bounds(&self.image, p, p);
        if let Some(rect) = changed {
            step.before.cover(&self.image, rect);
            pencil.line(&mut self.image, p, p);
        }
        self.live = Some(step);
        changed
    }

    /// Continues the stroke under way to `to`; does nothing without one.
    pub fn extend_stroke(&mut self, to: (f64, f64)) -> Option<Rect> {
        let step = self.live.as_mut()?;
        let Operation::Stroke(stroke) = &mut step.op;
        let p = pixel(to);
        let last = *stroke.points.last().expect("a stroke starts with a point");
        if p == last {
            return None;
        }
        stroke.points.push(p);
        let rect = stroke.pencil.bounds(&self.image, last, p)?;
        step.before.cover(&self.image, rect);
        stroke.pencil.line(&mut self.image, last, p)
    }

    /// Finishes the stroke under way as one undo step. A stroke that touched no pixel (all
    /// of it off the image) leaves no step.
    pub fn end_stroke(&mut self) {
        if let Some(step) = self.live.take() {
            if !step.before.tiles.is_empty() {
                self.done.push(step);
                self.undone.clear();
            }
        }
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

/// The pixel a point in image coordinates falls in.
fn pixel((x, y): (f64, f64)) -> (i64, i64) {
    let clamp = |v: f64| {
        if v.is_nan() {
            0
        } else {
            v.clamp(-FAR, FAR).floor() as i64
        }
    };
    (clamp(x), clamp(y))
}
