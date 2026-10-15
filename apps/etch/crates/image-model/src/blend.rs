//! Pixels from elsewhere laid over the image: rendered text (the text tool) and, later,
//! pasted images. Each pixel is composited source-over (straight alpha) onto the one under
//! it, so a transparent pixel leaves the image as it is and an opaque one replaces it.

use crate::brush::over;
use crate::{Image, Rect};

/// `pixels` laid over the image with their top-left pixel at `at` (in image pixels; any
/// part off the image is clipped). The operation keeps the pixels themselves, so redo
/// gives the same result however they were made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blend {
    pub at: (i64, i64),
    pub pixels: Image,
}

impl Blend {
    /// The image's pixels the blend covers, if any.
    pub(crate) fn bounds(&self, image: &Image) -> Option<Rect> {
        let (x, y) = self.at;
        Rect::clipped(
            x,
            y,
            x + self.pixels.width() as i64 - 1,
            y + self.pixels.height() as i64 - 1,
            image.bounds(),
        )
    }

    /// Composites the pixels over `image`; returns the pixels it covered.
    pub(crate) fn apply(&self, image: &mut Image) -> Option<Rect> {
        let rect = self.bounds(image)?;
        // Where the image's rectangle starts in `pixels`.
        let (sx, sy) = (
            (rect.x as i64 - self.at.0) as u32,
            (rect.y as i64 - self.at.1) as u32,
        );
        for y in 0..rect.height {
            for x in 0..rect.width {
                let src = self
                    .pixels
                    .pixel(sx + x, sy + y)
                    .expect("the rectangle lies within the pixels");
                if src.a == 0 {
                    continue;
                }
                let (ix, iy) = (rect.x + x, rect.y + y);
                let dst = image
                    .pixel(ix, iy)
                    .expect("the rectangle lies within the image");
                image.set_pixel(ix, iy, over(dst, src, 255));
            }
        }
        Some(rect)
    }
}
