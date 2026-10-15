//! Shapes: a line, a rectangle, or an ellipse dragged from one pixel to another, drawn
//! hard-edged (no anti-aliasing), as Paint draws them.
//!
//! - A **line** is the pencil's: Bresenham's from pixel to pixel, stamped with the pencil's
//!   disc for widths above one.
//! - A **rectangle** fills the box the drag spans, corners included; its outline is a band
//!   the width wide inside the box, with square corners.
//! - An **ellipse** fits the same box. Its one-pixel outline is the midpoint (Bresenham)
//!   ellipse through the box's edges, as Zingl's rasterizer gives it for any box, even
//!   sizes included; a wider outline stamps the pencil's disc along the ellipse inset by
//!   half the width, so it stays in the box too.
//!
//! The outline is in the outline colour; inside it, a rectangle or ellipse may be filled
//! with the fill colour, or the whole shape may be the fill with no outline. A shape is
//! a few spans a row, so it comes as rectangles of one colour each, rows alike merged:
//! the canvas previews a shape being dragged by drawing them, and committing it paints
//! them. They never overlap, so the colours lay one coat, composited over what's there
//! (source-over, as the brush's), which for an opaque colour replaces the pixel.

use crate::brush::over;
use crate::geom::{join, pixel};
use crate::pencil::bresenham;
use crate::{Color, Image, Pencil, Rect};

/// Whether a rectangle or ellipse is outlined, filled, or both.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Style {
    /// The outline alone.
    #[default]
    Outline,
    /// The whole shape in the fill colour, without an outline.
    Filled,
    /// The outline, and inside it the fill colour.
    Both,
}

/// What a shape is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Line,
    Rectangle(Style),
    Ellipse(Style),
}

/// How a shape is drawn: its kind, its outline's width in pixels, and its colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ShapeTool {
    pub kind: Kind,
    /// At least 1.
    pub width: u32,
    pub outline: Color,
    pub fill: Color,
}

/// A shape dragged from pixel `from` to pixel `to`, both included.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Shape {
    pub tool: ShapeTool,
    pub from: (i64, i64),
    pub to: (i64, i64),
}

/// tan 22.5°: a line within this slope of an axis snaps to it.
const TAN_22_5: f64 = std::f64::consts::SQRT_2 - 1.0;

impl ShapeTool {
    /// The shape dragged from image point `from` to `to` (fractions floor to the pixel they
    /// fall in; points off the image are fine, the shape clips). With `constrain` (Shift
    /// held), a line keeps to the nearest of the eight directions 45° apart, and a
    /// rectangle or ellipse to a square box (a circle), the longer side of the drag.
    pub fn at(self, from: (f64, f64), to: (f64, f64), constrain: bool) -> Shape {
        let (from, mut to) = (pixel(from), pixel(to));
        if constrain {
            let (dx, dy) = (to.0 - from.0, to.1 - from.1);
            let (ax, ay) = (dx.abs(), dy.abs());
            let sign = |v: i64| if v < 0 { -1 } else { 1 };
            let d = ax.max(ay);
            let diagonal = (from.0 + sign(dx) * d, from.1 + sign(dy) * d);
            to = match self.kind {
                Kind::Line if (ay as f64) < ax as f64 * TAN_22_5 => (to.0, from.1),
                Kind::Line if (ax as f64) < ay as f64 * TAN_22_5 => (from.0, to.1),
                _ => diagonal,
            };
        }
        Shape {
            tool: self,
            from,
            to,
        }
    }
}

/// One row of a shape: pixels `outer` (inclusive) are the shape's, and of them `hole`
/// lies inside the outline (the fill's, if any).
struct Row {
    y: i64,
    outer: (i64, i64),
    hole: Option<(i64, i64)>,
}

/// Each row's leftmost and rightmost pixel so far, for rows `lo..` of a shape.
struct Extents {
    lo: i64,
    rows: Vec<(i64, i64)>,
}

impl Extents {
    fn new(lo: i64, hi: i64) -> Extents {
        Extents {
            lo,
            rows: vec![(i64::MAX, i64::MIN); (hi - lo + 1).max(0) as usize],
        }
    }

    /// Takes in the disc `disc` stamped on pixel `(x, y)`.
    fn stamp(&mut self, disc: &[(i64, i64, i64)], x: i64, y: i64) {
        for &(dy, dx0, dx1) in disc {
            let Some(e) = usize::try_from(y + dy - self.lo)
                .ok()
                .and_then(|k| self.rows.get_mut(k))
            else {
                continue;
            };
            *e = (e.0.min(x + dx0), e.1.max(x + dx1));
        }
    }

    /// Row `k`'s pixels, if it has any.
    fn get(&self, k: usize) -> Option<(i64, i64)> {
        self.rows.get(k).copied().filter(|e| e.0 <= e.1)
    }
}

impl Shape {
    /// The box the drag spans: left, top, right, bottom pixels, inclusive.
    fn rect(&self) -> (i64, i64, i64, i64) {
        let ((x0, y0), (x1, y1)) = (self.from, self.to);
        (x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1))
    }

    /// The outline's colour and the fill's, each if the shape has it.
    fn colours(&self) -> (Option<Color>, Option<Color>) {
        let t = self.tool;
        match t.kind {
            Kind::Line | Kind::Rectangle(Style::Outline) | Kind::Ellipse(Style::Outline) => {
                (Some(t.outline), None)
            }
            Kind::Rectangle(Style::Filled) | Kind::Ellipse(Style::Filled) => (None, Some(t.fill)),
            Kind::Rectangle(Style::Both) | Kind::Ellipse(Style::Both) => {
                (Some(t.outline), Some(t.fill))
            }
        }
    }

    /// The shape's rows within rows `lo..=hi`, top to bottom.
    fn rows(&self, lo: i64, hi: i64) -> Vec<Row> {
        let filled = matches!(
            self.tool.kind,
            Kind::Rectangle(Style::Filled) | Kind::Ellipse(Style::Filled)
        );
        // A shape without an outline is the one-pixel outline's and all inside it.
        let width = if filled { 1 } else { self.tool.width.max(1) };
        let (x0, y0, x1, y1) = self.rect();
        match self.tool.kind {
            Kind::Line => line_rows(self.from, self.to, width, lo, hi),
            Kind::Rectangle(_) => {
                let w = width as i64;
                (y0.max(lo)..=y1.min(hi))
                    .map(|y| Row {
                        y,
                        outer: (x0, x1),
                        hole: (y >= y0 + w && y <= y1 - w && x0 + w <= x1 - w)
                            .then_some((x0 + w, x1 - w)),
                    })
                    .collect()
            }
            Kind::Ellipse(_) => ellipse_rows((x0, y0, x1, y1), width, lo.max(y0), hi.min(y1)),
        }
    }

    /// The shape's pixels within `clip`, as rectangles of one colour each, top to bottom:
    /// a few a row, rows with the same spans merged. They don't overlap.
    pub fn rects(&self, clip: Rect) -> Vec<(Rect, Color)> {
        let mut out = Vec::new();
        if clip.is_empty() {
            return out;
        }
        let (outline, fill) = self.colours();
        let (left, right) = (clip.x as i64, clip.right() as i64 - 1);
        // The previous row's rectangles, which the next row extends while its spans match.
        let mut open: Vec<(Rect, Color)> = Vec::new();
        // This row's spans, clipped: first pixel, width, colour.
        let mut spans: Vec<(u32, u32, Color)> = Vec::with_capacity(3);
        for row in self.rows(clip.y as i64, clip.bottom() as i64 - 1) {
            spans.clear();
            let mut span = |s0: i64, s1: i64, c: Color| {
                let (s0, s1) = (s0.max(left), s1.min(right));
                if s0 <= s1 {
                    spans.push((s0 as u32, (s1 - s0 + 1) as u32, c));
                }
            };
            let (a, b) = row.outer;
            match (row.hole, outline) {
                (Some((h0, h1)), Some(c)) => {
                    span(a, h0 - 1, c);
                    if let Some(f) = fill {
                        span(h0, h1, f);
                    }
                    span(h1 + 1, b, c);
                }
                (None, Some(c)) => span(a, b, c),
                (_, None) => {
                    if let Some(f) = fill {
                        span(a, b, f);
                    }
                }
            }
            let y = row.y as u32;
            let same = open.first().is_some_and(|(r, _)| r.bottom() == y)
                && open.len() == spans.len()
                && open
                    .iter()
                    .zip(&spans)
                    .all(|((r, c), &(x, w, sc))| r.x == x && r.width == w && *c == sc);
            if same {
                for (r, _) in &mut open {
                    r.height += 1;
                }
                continue;
            }
            out.append(&mut open);
            open.extend(spans.iter().map(|&(x, w, c)| (Rect::new(x, y, w, 1), c)));
        }
        out.append(&mut open);
        out
    }

    /// Paints the shape on `image`, calling `before(image, rect)` before writing each
    /// rectangle so the caller can save what it replaces. Returns the pixels it changed;
    /// nothing when the shape is off the image.
    pub(crate) fn apply(
        &self,
        image: &mut Image,
        mut before: impl FnMut(&Image, Rect),
    ) -> Option<Rect> {
        let mut changed = None;
        for (r, c) in self.rects(image.bounds()) {
            before(image, r);
            for y in r.y..r.bottom() {
                if c.a == 255 {
                    image.fill_span(y, r.x, r.right(), c);
                    continue;
                }
                for x in r.x..r.right() {
                    let was = image.pixel(x, y).expect("the rectangle is in the image");
                    image.set_pixel(x, y, over(was, c, 255));
                }
            }
            changed = join(changed, Some(r));
        }
        changed
    }
}

/// The rows `lo..=hi` of a line from `a` to `b` of `width`: the pencil's discs along
/// Bresenham's line. Each row's pixels are one run, since the discs' centres step a pixel
/// at a time and each disc's rows hold its centre column.
fn line_rows(a: (i64, i64), b: (i64, i64), width: u32, lo: i64, hi: i64) -> Vec<Row> {
    let pencil = Pencil {
        color: Color::TRANSPARENT,
        width,
    };
    let (disc, (before, after)) = (pencil.rows(), pencil.reach());
    let (lo, hi) = (lo.max(a.1.min(b.1) - before), hi.min(a.1.max(b.1) + after));
    if lo > hi {
        return Vec::new();
    }
    let mut ext = Extents::new(lo, hi);
    bresenham(a, b, |x, y| ext.stamp(&disc, x, y));
    (0..ext.rows.len())
        .filter_map(|k| {
            ext.get(k).map(|outer| Row {
                y: lo + k as i64,
                outer,
                hole: None,
            })
        })
        .collect()
}

/// The rows `lo..=hi` of an ellipse in the box `(x0, y0, x1, y1)` with an outline of
/// `width`: the pencil's disc stamped along the midpoint ellipse of the box inset by the
/// disc's reach. The ellipse's left half runs top to bottom without turning back, so the
/// discs on it cover one run a row, and so do the right half's; between the two runs,
/// when they don't meet, is the inside.
fn ellipse_rows((x0, y0, x1, y1): (i64, i64, i64, i64), width: u32, lo: i64, hi: i64) -> Vec<Row> {
    if lo > hi {
        return Vec::new();
    }
    let pencil = Pencil {
        color: Color::TRANSPARENT,
        width,
    };
    let (disc, (before, after)) = (pencil.rows(), pencil.reach());
    // A box narrower than the outline is all outline: the path shrinks to its middle.
    let inset = |a: i64, b: i64| {
        let (p, q) = (a + before, b - after);
        if p <= q {
            (p, q)
        } else {
            let m = (p + q).div_euclid(2);
            (m, m)
        }
    };
    let ((ix0, ix1), (iy0, iy1)) = (inset(x0, x1), inset(y0, y1));
    let (mut left, mut right) = (Extents::new(lo, hi), Extents::new(lo, hi));
    midpoint_ellipse(ix0, iy0, ix1, iy1, |x, y, on_left| {
        let side = if on_left { &mut left } else { &mut right };
        side.stamp(&disc, x, y);
    });
    // Discs keep to the box but where it's narrower than they are.
    let in_box = |(a, b): (i64, i64)| (a.max(x0), b.min(x1));
    (0..left.rows.len())
        .filter_map(|k| {
            let (l, r) = (left.get(k).map(in_box), right.get(k).map(in_box));
            let (outer, hole) = match (l, r) {
                (Some(l), Some(r)) => (
                    (l.0.min(r.0), l.1.max(r.1)),
                    (l.1 + 1 < r.0).then_some((l.1 + 1, r.0 - 1)),
                ),
                (Some(s), None) | (None, Some(s)) => (s, None),
                (None, None) => return None,
            };
            Some(Row {
                y: lo + k as i64,
                outer,
                hole,
            })
        })
        .collect()
}

/// Calls `plot(x, y, on_left)` with each pixel of the midpoint ellipse that touches the
/// four sides of the box from `(x0, y0)` to `(x1, y1)` (`x0 <= x1`, `y0 <= y1`), and
/// whether it's on the left half. A. Zingl's `plotEllipseRect` ("A Rasterizing Algorithm
/// for Drawing Curves", 2012), which handles boxes of even sides; its error terms in
/// `i128`, so far-off boxes can't overflow.
fn midpoint_ellipse(x0: i64, y0: i64, x1: i64, y1: i64, mut plot: impl FnMut(i64, i64, bool)) {
    let (a, b) = ((x1 - x0) as i128, (y1 - y0) as i128);
    let b1 = b & 1;
    let mut dx = 4 * (1 - a) * b * b;
    let mut dy = 4 * (b1 + 1) * a * a;
    let mut err = dx + dy + b1 * a * a;
    let (mut xl, mut xr) = (x0, x1);
    // The lower half's row going down and the upper half's going up, from the middle.
    let mut yd = y0 + ((b + 1) / 2) as i64;
    let mut yu = yd - b1 as i64;
    let (a8, b8) = (8 * a * a, 8 * b * b);
    loop {
        plot(xr, yd, false);
        plot(xl, yd, true);
        plot(xl, yu, true);
        plot(xr, yu, false);
        let e2 = 2 * err;
        if e2 <= dy {
            yd += 1;
            yu -= 1;
            dy += a8;
            err += dy;
        }
        if e2 >= dx || 2 * err > dy {
            xl += 1;
            xr -= 1;
            dx += b8;
            err += dx;
        }
        if xl > xr {
            break;
        }
    }
    // A flat ellipse (a box one or two pixels wide) stops early: finish its tips.
    while ((yd - yu) as i128) <= b {
        plot(xl - 1, yd, true);
        plot(xr + 1, yd, false);
        plot(xl - 1, yu, true);
        plot(xr + 1, yu, false);
        yd += 1;
        yu -= 1;
    }
}
