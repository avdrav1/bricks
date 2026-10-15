//! [`Transform`]: the Image menu's commands, which make a new image from the whole of the
//! old one, its size changing (crop). A transform reads the image and builds the new one
//! without touching it, so the app can run it off the UI thread and hand the result to
//! [`crate::Document::install`].

use crate::{Image, Rect, SizeError};

/// A change to the whole image that may change its size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transform {
    /// Crop to Selection: the image becomes this rectangle's pixels (clipped to the image).
    Crop(Rect),
}

impl Transform {
    /// The image `image` becomes: a new one, `image` untouched. Deterministic: the same
    /// image always gives the same bytes. `Err` when the result would have no pixels (a
    /// crop off the image).
    pub fn apply(&self, image: &Image) -> Result<Image, SizeError> {
        match *self {
            Transform::Crop(rect) => {
                let r = rect.intersect(image.bounds()).ok_or(SizeError::Empty)?;
                Image::from_rgba(r.width, r.height, image.copy_rect(r))
            }
        }
    }

    /// Whether it would give `image` back as it is (a crop to the whole image), so it
    /// makes no step.
    pub fn unchanged(&self, image: &Image) -> bool {
        match *self {
            Transform::Crop(rect) => rect.intersect(image.bounds()) == Some(image.bounds()),
        }
    }
}
