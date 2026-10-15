//! [`Transform`]: the Image menu's commands, which make a new image from the whole of the
//! old one, its size changing (crop, resize, resize canvas, rotate) or not (flip). A
//! transform reads the image and builds the new one without touching it, so the app can
//! run it off the UI thread and hand the result to [`crate::Document::install`].

use crate::orient::{self, Flip, Turn};
use crate::resample;
use crate::{Color, Image, Rect, SizeError};

/// How Resize Image computes the new pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Resampling {
    /// Each new pixel is the old pixel its centre falls in: hard edges, no new colours
    /// (pixel art; doubling makes each pixel a 2×2 square).
    Nearest,
    /// Each new pixel averages the old ones around it (a triangle filter, as wide as the
    /// shrink when shrinking), weighting colours by alpha: photos and drawings.
    #[default]
    Smooth,
}

/// Where Resize Canvas keeps the image on the new canvas: one of nine places, a corner,
/// an edge's middle, or the centre.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Anchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    #[default]
    Centre,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl Anchor {
    /// All nine, row by row from the top left, as the dialog's grid shows them.
    pub const ALL: [Anchor; 9] = [
        Anchor::TopLeft,
        Anchor::Top,
        Anchor::TopRight,
        Anchor::Left,
        Anchor::Centre,
        Anchor::Right,
        Anchor::BottomLeft,
        Anchor::Bottom,
        Anchor::BottomRight,
    ];

    /// Where an image of `old` size has its top-left pixel on a canvas of `new` size (off
    /// it, negative, when shrinking): against the anchor's edges, or centred, a pixel left
    /// over going right and down whether the canvas grows or shrinks.
    pub fn offset(self, old: (u32, u32), new: (u32, u32)) -> (i64, i64) {
        let k = Anchor::ALL.iter().position(|&a| a == self).unwrap_or(4);
        let at = |place: usize, old: u32, new: u32| {
            let room = new as i64 - old as i64;
            match place {
                0 => 0,
                1 => room / 2,
                _ => room,
            }
        };
        (at(k % 3, old.0, new.0), at(k / 3, old.1, new.1))
    }
}

/// A change to the whole image that may change its size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transform {
    /// Crop to Selection: the image becomes this rectangle's pixels (clipped to the image).
    Crop(Rect),
    /// Resize Image: the whole image scaled to `width` × `height` pixels.
    Resize {
        width: u32,
        height: u32,
        resampling: Resampling,
    },
    /// Resize Canvas: the image becomes `width` × `height` pixels, its pixels unscaled and
    /// placed by `anchor` (those off the new canvas cropped), the new area `fill`.
    ResizeCanvas {
        width: u32,
        height: u32,
        anchor: Anchor,
        fill: Color,
    },
    /// Rotate: the image turned a quarter either way (width and height swapped) or half.
    Rotate(Turn),
    /// Flip: the image mirrored left to right or top to bottom.
    Flip(Flip),
}

impl Transform {
    /// The image `image` becomes: a new one, `image` untouched. Deterministic: the same
    /// image always gives the same bytes. `Err` when the result would have no pixels (a
    /// crop off the image, a side of 0) or be larger than [`crate::MAX_SIDE`] a side.
    pub fn apply(&self, image: &Image) -> Result<Image, SizeError> {
        match *self {
            Transform::Crop(rect) => {
                let r = rect.intersect(image.bounds()).ok_or(SizeError::Empty)?;
                Image::from_rgba(r.width, r.height, image.copy_rect(r))
            }
            Transform::Resize {
                width,
                height,
                resampling,
            } => {
                // The size first: no pixels are computed for a size that can't be.
                Image::check_size(width, height)?;
                let data = match resampling {
                    Resampling::Nearest => resample::nearest(image, width, height),
                    Resampling::Smooth => resample::smooth(image, width, height),
                };
                Image::from_rgba(width, height, data)
            }
            Transform::ResizeCanvas {
                width,
                height,
                anchor,
                fill,
            } => {
                let mut out = Image::new(width, height, fill)?;
                let (w, h) = (image.width() as i64, image.height() as i64);
                let (x, y) = anchor.offset((image.width(), image.height()), (width, height));
                if let Some(r) = Rect::clipped(x, y, x + w - 1, y + h - 1, out.bounds()) {
                    let from = ((r.x as i64 - x) as u32, (r.y as i64 - y) as u32);
                    out.copy_from(r, image, from);
                }
                Ok(out)
            }
            Transform::Rotate(turn) => {
                let (w, h, data) = orient::rotate(image, turn);
                Image::from_rgba(w, h, data)
            }
            Transform::Flip(flip) => {
                let data = orient::flip(image, flip);
                Image::from_rgba(image.width(), image.height(), data)
            }
        }
    }

    /// Whether it would give `image` back as it is (a crop to the whole image, a resize or
    /// a canvas of the same size), so it makes no step. A rotation or flip always makes one,
    /// even of an image that looks the same turned.
    pub fn unchanged(&self, image: &Image) -> bool {
        match *self {
            Transform::Crop(rect) => rect.intersect(image.bounds()) == Some(image.bounds()),
            Transform::Resize { width, height, .. }
            | Transform::ResizeCanvas { width, height, .. } => {
                (width, height) == (image.width(), image.height())
            }
            Transform::Rotate(_) | Transform::Flip(_) => false,
        }
    }

    /// The transform that exactly undoes this one, for those that move every pixel whole
    /// (a rotation the other way, the same flip): their steps undo by it, keeping no
    /// image. `None` for those that lose pixels (crop, resize, resize canvas).
    pub fn inverse(&self) -> Option<Transform> {
        match *self {
            Transform::Rotate(Turn::Right) => Some(Transform::Rotate(Turn::Left)),
            Transform::Rotate(Turn::Left) => Some(Transform::Rotate(Turn::Right)),
            Transform::Rotate(Turn::Half) | Transform::Flip(_) => Some(*self),
            Transform::Crop(_) | Transform::Resize { .. } | Transform::ResizeCanvas { .. } => None,
        }
    }
}
