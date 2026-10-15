//! Selected pixels lifted off the image and moved ([`Floating`]), and the rectangle a
//! selection drag spans ([`spanning`]). Floating pixels aren't on the image: they float
//! over it until [`crate::Document::land`] puts them down, clearing the place they were
//! lifted from to transparency and replacing the pixels where they land, as one undo step.

use crate::geom::pixel;
use crate::{Color, Image, Rect};

/// Pixels floating over the image with their top-left pixel at `at` (in image pixels; any
/// part may be off the image), lifted from `source` ([`crate::Document::lift`]) or from
/// elsewhere (a paste, [`Floating::new`]). Landing them is the operation: it keeps the
/// pixels, so redo lands the same bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Floating {
    source: Option<Rect>,
    at: (i64, i64),
    pixels: Image,
}

impl Floating {
    /// Pixels from elsewhere (a paste) floating with their top-left pixel at `at`; landing
    /// them clears nothing.
    pub fn new(pixels: Image, at: (i64, i64)) -> Floating {
        Floating {
            source: None,
            at,
            pixels,
        }
    }

    /// Pixels lifted from `source` (inside the image), still where they were.
    pub(crate) fn lifted(source: Rect, pixels: Image) -> Floating {
        Floating {
            source: Some(source),
            at: (source.x as i64, source.y as i64),
            pixels,
        }
    }

    /// Where the pixels were lifted from: landing leaves it transparent where they don't
    /// land. `None` for pixels from elsewhere.
    pub fn source(&self) -> Option<Rect> {
        self.source
    }

    /// The image pixel their top-left pixel is over.
    pub fn at(&self) -> (i64, i64) {
        self.at
    }

    pub fn pixels(&self) -> &Image {
        &self.pixels
    }

    /// Their width and height.
    pub fn size(&self) -> (u32, u32) {
        (self.pixels.width(), self.pixels.height())
    }

    /// Moves them `dx` pixels right and `dy` down.
    pub fn move_by(&mut self, dx: i64, dy: i64) {
        self.at = (self.at.0.saturating_add(dx), self.at.1.saturating_add(dy));
    }

    /// Moves their top-left pixel to `at`.
    pub fn move_to(&mut self, at: (i64, i64)) {
        self.at = at;
    }

    /// Whether they cover image pixel `(x, y)`, on the image or off it.
    pub fn contains(&self, (x, y): (i64, i64)) -> bool {
        let (w, h) = self.size();
        x >= self.at.0 && y >= self.at.1 && x - self.at.0 < w as i64 && y - self.at.1 < h as i64
    }

    /// Whether they're where they were lifted from: landing them there changes nothing.
    pub fn unmoved(&self) -> bool {
        self.source
            .is_some_and(|r| (r.x as i64, r.y as i64) == self.at)
    }

    /// The pixels of an image of `bounds` they'd land on, if any.
    pub fn bounds(&self, bounds: Rect) -> Option<Rect> {
        let (w, h) = self.size();
        let (x, y) = self.at;
        Rect::clipped(x, y, x + w as i64 - 1, y + h as i64 - 1, bounds)
    }

    /// Lands them on `image`: their source cleared to transparency, then their pixels
    /// replacing the image's where they land (transparent ones too), clipped. Returns the
    /// pixels changed.
    pub(crate) fn apply(&self, image: &mut Image) -> Option<Rect> {
        let cleared = self.source.and_then(|r| r.intersect(image.bounds()));
        if let Some(r) = cleared {
            image.fill_rect(r, Color::TRANSPARENT);
        }
        let landed = self.bounds(image.bounds());
        if let Some(d) = landed {
            let from = (
                (d.x as i64 - self.at.0) as u32,
                (d.y as i64 - self.at.1) as u32,
            );
            image.copy_from(d, &self.pixels, from);
        }
        crate::geom::join(cleared, landed)
    }
}

/// The rectangle a selection drag from `a` to `b` (image coordinates, either way round)
/// spans: its edges at the pixel edges nearest the two points, so a drag across 200
/// pixels selects 200, clipped to `bounds`; `None` when it holds no pixel (a click).
pub fn spanning(a: (f64, f64), b: (f64, f64), bounds: Rect) -> Option<Rect> {
    let (ax, ay) = pixel((a.0 + 0.5, a.1 + 0.5));
    let (bx, by) = pixel((b.0 + 0.5, b.1 + 0.5));
    Rect::clipped(
        ax.min(bx),
        ay.min(by),
        ax.max(bx) - 1,
        ay.max(by) - 1,
        bounds,
    )
}
