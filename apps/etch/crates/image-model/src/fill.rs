//! The fill bucket: the 4-connected region of pixels matching the clicked one becomes one
//! colour, found a row span at a time (scanline flood fill).

use crate::geom::Rect;
use crate::{Color, Image};

/// A fill: where it was clicked, the colour it puts down, and how far a pixel's channels may
/// stray from the clicked pixel's and still be filled (0: exactly its colour).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Fill {
    pub at: (u32, u32),
    pub color: Color,
    pub tolerance: u8,
}

/// Whether pixel `p` belongs to the region of `seed`: each channel within `tolerance` of
/// the seed's. Fully transparent pixels all look alike, so they match each other whatever
/// colour they carry.
#[inline]
fn matches(p: [u8; 4], seed: [u8; 4], tolerance: u8) -> bool {
    if p == seed || (p[3] == 0 && seed[3] == 0) {
        return true;
    }
    tolerance > 0 && p.iter().zip(seed).all(|(&a, b)| a.abs_diff(b) <= tolerance)
}

/// Pixels already filled, when the colour put down still matches the seed (possible only
/// with a tolerance); otherwise a filled pixel tells itself apart by no longer matching.
struct Done(Option<Vec<u64>>);

impl Done {
    #[inline]
    fn has(&self, i: usize) -> bool {
        self.0
            .as_ref()
            .is_some_and(|d| d[i / 64] >> (i % 64) & 1 == 1)
    }

    fn mark(&mut self, from: usize, to: usize) {
        if let Some(d) = &mut self.0 {
            for i in from..to {
                d[i / 64] |= 1 << (i % 64);
            }
        }
    }
}

impl Fill {
    /// Fills the region. Before writing each row span, calls `before(image, y, x0, x1)` so
    /// the caller can save what it replaces. Returns the pixels it changed; nothing when the
    /// point is off the image or an exact fill would put down the colour already there.
    pub(crate) fn apply(
        &self,
        image: &mut Image,
        mut before: impl FnMut(&Image, u32, u32, u32),
    ) -> Option<Rect> {
        let (w, h) = (image.width() as usize, image.height() as usize);
        let (sx, sy) = (self.at.0 as usize, self.at.1 as usize);
        if sx >= w || sy >= h {
            return None;
        }
        let seed = image.pixels()[sy * w + sx];
        let (fill, tol) = (self.color.to_bytes(), self.tolerance);
        if tol == 0 && matches(fill, seed, 0) {
            return None;
        }
        let mut done = Done(matches(fill, seed, tol).then(|| vec![0; (w * h).div_ceil(64)]));
        let open =
            |px: &[[u8; 4]], done: &Done, i: usize| matches(px[i], seed, tol) && !done.has(i);
        let (mut left, mut top, mut right, mut bottom) = (sx, sy, sx, sy);
        // Seeds to fill from, each with the row it was found from and that row's span: the
        // span is filled already, so looking back at that row skips it.
        let mut stack = vec![(sx, sy, usize::MAX, 0, 0)];
        while let Some((x, y, from, fx0, fx1)) = stack.pop() {
            let row = y * w;
            let px = image.pixels();
            if !open(px, &done, row + x) {
                continue;
            }
            let (mut x0, mut x1) = (x, x + 1);
            while x0 > 0 && open(px, &done, row + x0 - 1) {
                x0 -= 1;
            }
            while x1 < w && open(px, &done, row + x1) {
                x1 += 1;
            }
            done.mark(row + x0, row + x1);
            before(image, y as u32, x0 as u32, x1 as u32);
            image.fill_span(y as u32, x0 as u32, x1 as u32, self.color);
            (left, right) = (left.min(x0), right.max(x1 - 1));
            (top, bottom) = (top.min(y), bottom.max(y));
            // One seed for each run of open pixels in the rows above and below.
            let px = image.pixels();
            for ny in [y.wrapping_sub(1), y + 1] {
                if ny >= h {
                    continue;
                }
                let (skip0, skip1) = if ny == from { (fx0, fx1) } else { (x1, x1) };
                for (a, b) in [(x0, skip0.clamp(x0, x1)), (skip1.clamp(x0, x1), x1)] {
                    let mut inside = false;
                    for nx in a..b {
                        let ok = open(px, &done, ny * w + nx);
                        if ok && !inside {
                            stack.push((nx, ny, y, x0, x1));
                        }
                        inside = ok;
                    }
                }
            }
        }
        Some(Rect::new(
            left as u32,
            top as u32,
            (right - left + 1) as u32,
            (bottom - top + 1) as u32,
        ))
    }
}
