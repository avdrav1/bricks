//! Rotate and Flip: the image turned a quarter or half way, or mirrored, every pixel
//! moved whole (nothing resampled), so each has an exact inverse.

use crate::par::bands;
use crate::Image;

/// A quarter or half turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Turn {
    /// 90° clockwise.
    Right,
    /// 90° anticlockwise.
    Left,
    /// 180°.
    Half,
}

/// Which way Flip mirrors the image.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Flip {
    /// Left to right: the left edge becomes the right.
    Horizontal,
    /// Top to bottom: the top edge becomes the bottom.
    Vertical,
}

/// The rows a quarter turn writes at once: going across the new image, they read
/// neighbouring pixels of each old row, so each cache line read serves them all.
const BLOCK: usize = 16;

/// `image` turned by `turn`: its pixels, and its new width and height (swapped for a
/// quarter turn).
pub(crate) fn rotate(image: &Image, turn: Turn) -> (u32, u32, Vec<u8>) {
    let (w, h) = (image.width() as usize, image.height() as usize);
    let src = image.pixels();
    let (nw, nh) = match turn {
        Turn::Half => (w, h),
        Turn::Right | Turn::Left => (h, w),
    };
    let mut out = vec![0; nw * nh * 4];
    bands(&mut out, nw * 4, |first, band| {
        let rows = band.as_chunks_mut::<4>().0;
        let first = first as usize;
        match turn {
            Turn::Half => {
                for (k, row) in rows.chunks_exact_mut(nw).enumerate() {
                    let from = &src[(h - 1 - (first + k)) * w..][..w];
                    for (p, q) in row.iter_mut().zip(from.iter().rev()) {
                        *p = *q;
                    }
                }
            }
            Turn::Right | Turn::Left => {
                // New pixel (x, y): turned right, the old (y, h - 1 - x); left, the old
                // (w - 1 - y, x).
                let count = rows.len() / nw;
                for b in (0..count).step_by(BLOCK) {
                    let end = (b + BLOCK).min(count);
                    for x in 0..nw {
                        for k in b..end {
                            let y = first + k;
                            let (sx, sy) = match turn {
                                Turn::Right => (y, h - 1 - x),
                                _ => (w - 1 - y, x),
                            };
                            rows[k * nw + x] = src[sy * w + sx];
                        }
                    }
                }
            }
        }
    });
    (nw as u32, nh as u32, out)
}

/// `image` mirrored by `flip`.
pub(crate) fn flip(image: &Image, flip: Flip) -> Vec<u8> {
    let (w, h) = (image.width() as usize, image.height() as usize);
    let src = image.pixels();
    let mut out = vec![0; w * h * 4];
    bands(&mut out, w * 4, |first, band| {
        let rows = band.as_chunks_mut::<4>().0;
        for (k, row) in rows.chunks_exact_mut(w).enumerate() {
            let y = first as usize + k;
            match flip {
                Flip::Horizontal => {
                    for (p, q) in row.iter_mut().zip(src[y * w..][..w].iter().rev()) {
                        *p = *q;
                    }
                }
                Flip::Vertical => row.copy_from_slice(&src[(h - 1 - y) * w..][..w]),
            }
        }
    });
    out
}
