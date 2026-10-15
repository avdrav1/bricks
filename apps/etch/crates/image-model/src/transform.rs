//! [`Transform`]: the Image menu's commands, which make a new image from the whole of the
//! old one, its size changing (crop, resize). A transform reads the image and builds the
//! new one without touching it, so the app can run it off the UI thread and hand the result
//! to [`crate::Document::install`].

use crate::resample;
use crate::{Image, Rect, SizeError};

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
        }
    }

    /// Whether it would give `image` back as it is (a crop to the whole image, a resize to
    /// the same size), so it makes no step.
    pub fn unchanged(&self, image: &Image) -> bool {
        match *self {
            Transform::Crop(rect) => rect.intersect(image.bounds()) == Some(image.bounds()),
            Transform::Resize { width, height, .. } => {
                (width, height) == (image.width(), image.height())
            }
        }
    }
}
