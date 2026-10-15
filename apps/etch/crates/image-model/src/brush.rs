//! The brush: a round tip of a size with a hard or a soft edge, drawn in a colour over
//! what's there (source-over, straight alpha).
//!
//! A stroke covers each pixel by how near the pointer's path came to it: fully inside the
//! tip's disc for a hard edge, falling off smoothly to nothing at the rim for a soft one.
//! A pixel keeps the most coverage any part of the stroke gave it, and its colour is the
//! brush's composited at that coverage over the pixel as it was before the stroke. So a
//! slow stroke, which passes over the same pixels many times, looks the same as a quick
//! one: within a stroke coverage never compounds (Paint.NET's and Paint's soft brushes
//! behave so). Strokes do compound: a second stroke blends over the first.

use crate::document::Saved;
use crate::geom::Rect;
use crate::{Color, Image, TILE};
use std::collections::BTreeMap;

/// A brush: its colour, width in pixels, and edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Brush {
    pub color: Color,
    /// At least 1.
    pub width: u32,
    /// A soft edge falls off to transparent at the rim; a hard one is solid to it.
    pub soft: bool,
}

/// How much of the brush each pixel a stroke reached has had: 0 to 255, kept in
/// [`TILE`]-pixel tiles while the stroke is drawn.
#[derive(Default)]
pub(crate) struct Coverage {
    tiles: BTreeMap<(u32, u32), Box<[u8]>>,
}

impl Coverage {
    fn tile(&mut self, tile: (u32, u32)) -> &mut [u8] {
        self.tiles
            .entry(tile)
            .or_insert_with(|| vec![0; (TILE * TILE) as usize].into_boxed_slice())
    }
}

impl Brush {
    /// The disc's radius in pixels.
    fn radius(&self) -> f64 {
        self.width.max(1) as f64 / 2.0
    }

    /// The disc's centre for pixel `p`: the pixel's centre for odd widths, its bottom-right
    /// corner for even ones (they lean right and down, as the pencil's do).
    fn centre(&self, (x, y): (i64, i64)) -> (f64, f64) {
        let s = if self.width.max(1).is_multiple_of(2) {
            1.0
        } else {
            0.5
        };
        (x as f64 + s, y as f64 + s)
    }

    /// The pixels of `image` whose centres lie within the disc swept from `a` to `b`.
    pub(crate) fn bounds(&self, image: &Image, a: (i64, i64), b: (i64, i64)) -> Option<Rect> {
        let (a, b, r) = (self.centre(a), self.centre(b), self.radius());
        // Pixel x's centre is x + 0.5: in reach when it's within r of the centres' span.
        let lo = |v: f64| (v - r - 0.5).ceil() as i64;
        let hi = |v: f64| (v + r - 0.5).floor() as i64;
        Rect::clipped(
            lo(a.0.min(b.0)),
            lo(a.1.min(b.1)),
            hi(a.0.max(b.0)),
            hi(a.1.max(b.1)),
            image.bounds(),
        )
    }

    /// The coverage, 0 to 255, of a pixel whose centre is `d2` squared pixels from the
    /// path: 255 throughout a hard disc; for a soft one 255 at the centre falling off as
    /// a smoothstep to 0 at the rim.
    fn coverage(&self, d2: f64) -> u8 {
        let r = self.radius();
        if d2 > r * r {
            return 0;
        }
        if !self.soft {
            return 255;
        }
        let t = d2.sqrt() / r;
        (255.0 * (1.0 - t * t * (3.0 - 2.0 * t))).round() as u8
    }

    /// Paints the part of a stroke from pixel `a` to pixel `b` (both included). `before`
    /// gets the tiles it changes, unless it has them already, and gives back each pixel as
    /// it was before the stroke; `cover` holds the stroke's coverage so far. Returns the
    /// pixels it may have changed.
    pub(crate) fn segment(
        &self,
        image: &mut Image,
        before: &mut Saved,
        cover: &mut Coverage,
        a: (i64, i64),
        b: (i64, i64),
    ) -> Option<Rect> {
        let rect = self.bounds(image, a, b)?;
        before.cover(image, rect);
        let (a, b) = (self.centre(a), self.centre(b));
        for row in rect.y / TILE..rect.bottom().div_ceil(TILE) {
            for col in rect.x / TILE..rect.right().div_ceil(TILE) {
                let tile = Saved::tile_rect(image, (col, row));
                let part = tile
                    .intersect(rect)
                    .expect("the tile overlaps the rectangle");
                let orig = before.tile((col, row)).expect("the tile is saved");
                let cov = cover.tile((col, row));
                for y in part.y..part.bottom() {
                    for x in part.x..part.right() {
                        let c = self.coverage(dist2((x as f64 + 0.5, y as f64 + 0.5), a, b));
                        let k = ((y - tile.y) * TILE + (x - tile.x)) as usize;
                        if c <= cov[k] {
                            continue;
                        }
                        cov[k] = c;
                        let o = (((y - tile.y) * tile.width + (x - tile.x)) * 4) as usize;
                        let was = Color::from_bytes(orig[o..o + 4].try_into().unwrap());
                        image.set_pixel(x, y, over(was, self.color, c));
                    }
                }
            }
        }
        Some(rect)
    }
}

/// The squared distance from `p` to the segment from `a` to `b`.
fn dist2(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
    };
    let (ex, ey) = (p.0 - (a.0 + t * dx), p.1 - (a.1 + t * dy));
    ex * ex + ey * ey
}

/// `src` at `coverage` (0 to 255) composited over `dst`, straight alpha, source-over.
fn over(dst: Color, src: Color, coverage: u8) -> Color {
    if coverage == 255 && src.a == 255 {
        return src;
    }
    let sa = src.a as f64 / 255.0 * coverage as f64 / 255.0;
    let da = dst.a as f64 / 255.0 * (1.0 - sa);
    let oa = sa + da;
    if oa <= 0.0 {
        return dst;
    }
    let mix = |s: u8, d: u8| ((s as f64 * sa + d as f64 * da) / oa).round() as u8;
    Color::rgba(
        mix(src.r, dst.r),
        mix(src.g, dst.g),
        mix(src.b, dst.b),
        (oa * 255.0).round() as u8,
    )
}
