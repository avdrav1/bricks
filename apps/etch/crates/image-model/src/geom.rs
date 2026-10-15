/// A rectangle of whole pixels: `x` and `y` its top-left pixel, `width` and `height` how
/// many pixels it spans.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// One past the rightmost pixel.
    pub const fn right(&self) -> u32 {
        self.x + self.width
    }

    /// One past the bottom pixel.
    pub const fn bottom(&self) -> u32 {
        self.y + self.height
    }

    pub const fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    pub const fn contains(&self, x: u32, y: u32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    /// The smallest rectangle holding both.
    pub fn union(self, other: Rect) -> Rect {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        let (x, y) = (self.x.min(other.x), self.y.min(other.y));
        Rect::new(
            x,
            y,
            self.right().max(other.right()) - x,
            self.bottom().max(other.bottom()) - y,
        )
    }

    /// The pixels in both, if any.
    pub fn intersect(self, other: Rect) -> Option<Rect> {
        let (x, y) = (self.x.max(other.x), self.y.max(other.y));
        let (r, b) = (
            self.right().min(other.right()),
            self.bottom().min(other.bottom()),
        );
        (r > x && b > y).then(|| Rect::new(x, y, r - x, b - y))
    }

    /// The rectangle from `(x0, y0)` to `(x1, y1)` inclusive, in signed pixels, clipped to
    /// `bounds`.
    pub(crate) fn clipped(x0: i64, y0: i64, x1: i64, y1: i64, bounds: Rect) -> Option<Rect> {
        let left = x0.max(bounds.x as i64);
        let top = y0.max(bounds.y as i64);
        let right = (x1 + 1).min(bounds.right() as i64);
        let bottom = (y1 + 1).min(bounds.bottom() as i64);
        (right > left && bottom > top).then(|| {
            Rect::new(
                left as u32,
                top as u32,
                (right - left) as u32,
                (bottom - top) as u32,
            )
        })
    }
}

/// Pointer positions beyond this many pixels off the image clamp to it, so far-off points
/// can't overflow the line arithmetic.
const FAR: f64 = 1_000_000.0;

/// The pixel a point in image coordinates falls in: fractions floor, far-off points clamp.
pub(crate) fn pixel((x, y): (f64, f64)) -> (i64, i64) {
    let clamp = |v: f64| {
        if v.is_nan() {
            0
        } else {
            v.clamp(-FAR, FAR).floor() as i64
        }
    };
    (clamp(x), clamp(y))
}

/// Merges an operation's changed rectangles.
pub(crate) fn join(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.union(b)),
        (a, None) => a,
        (None, b) => b,
    }
}
