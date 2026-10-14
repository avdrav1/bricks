//! The pencil: hard-edged lines of one colour, Bresenham's between the pixels the pointer
//! reported, stamped with a disc for widths above one (ADR 0010).

use crate::geom::Rect;
use crate::{Color, Image};

/// A pencil's tip: its colour and width in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Pencil {
    pub color: Color,
    /// At least 1.
    pub width: u32,
}

impl Pencil {
    /// The rows of the tip's disc relative to the pixel it's stamped on: `(dy, dx0, dx1)`,
    /// the pixels `dx0..=dx1` of row `dy`. Pixel `(i, j)` of the tip's `width`-square is
    /// in when its distance from the square's centre is at most half the width; offsets
    /// run from `-(width - 1) / 2`, so even widths lean right and down.
    fn rows(&self) -> Vec<(i64, i64, i64)> {
        let w = self.width.max(1) as i64;
        let lo = -(w - 1) / 2;
        let c = (w - 1) as f64 / 2.0;
        let r2 = (w as f64 / 2.0).powi(2);
        (0..w)
            .filter_map(|j| {
                let dy = j as f64 - c;
                let mut xs = (0..w).filter(|&i| (i as f64 - c).powi(2) + dy * dy <= r2);
                let first = xs.next()?;
                let last = xs.next_back().unwrap_or(first);
                Some((lo + j, lo + first, lo + last))
            })
            .collect()
    }

    /// How far the tip reaches from the pixel it's stamped on: `(before, after)`.
    fn reach(&self) -> (i64, i64) {
        let w = self.width.max(1) as i64;
        ((w - 1) / 2, w / 2)
    }

    /// The pixels of `image` a line from `a` to `b` would change.
    pub(crate) fn bounds(&self, image: &Image, a: (i64, i64), b: (i64, i64)) -> Option<Rect> {
        let (before, after) = self.reach();
        Rect::clipped(
            a.0.min(b.0) - before,
            a.1.min(b.1) - before,
            a.0.max(b.0) + after,
            a.1.max(b.1) + after,
            image.bounds(),
        )
    }

    /// Draws a line from pixel `a` to pixel `b`, both included, clipped to the image.
    /// Returns the pixels it may have changed.
    pub(crate) fn line(&self, image: &mut Image, a: (i64, i64), b: (i64, i64)) -> Option<Rect> {
        let changed = self.bounds(image, a, b)?;
        let rows = self.rows();
        let (w, h) = (image.width() as i64, image.height() as i64);
        let mut stamp = |x: i64, y: i64| {
            for &(dy, dx0, dx1) in &rows {
                let yy = y + dy;
                let (x0, x1) = ((x + dx0).max(0), (x + dx1 + 1).min(w));
                if yy >= 0 && yy < h && x1 > x0 {
                    image.fill_span(yy as u32, x0 as u32, x1 as u32, self.color);
                }
            }
        };
        let (dx, sx) = ((b.0 - a.0).abs(), (b.0 - a.0).signum());
        let (dy, sy) = (-(b.1 - a.1).abs(), (b.1 - a.1).signum());
        let (mut x, mut y, mut err) = (a.0, a.1, dx + dy);
        loop {
            stamp(x, y);
            if (x, y) == b {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
        Some(changed)
    }
}
