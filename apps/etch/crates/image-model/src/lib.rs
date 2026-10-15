//! Etch's image model (docs/adr/0010-etch-image-model.md): one RGBA8 raster with straight
//! alpha ([`Image`]), the operations that change it ([`Operation`]), and [`Document`],
//! which applies them and keeps their undo history.
//!
//! Nothing here knows about a toolkit. The app's tools call [`Document`]'s operations, and
//! each returns the [`Rect`] of pixels it may have changed, so the canvas redraws only
//! that. History keeps each step's operation and the image's [`TILE`]-pixel tiles from
//! before it touched them: undo writes the tiles back, redo replays the operation. A
//! transform of the whole image ([`Transform`]) that loses pixels (crop, resize, resize
//! canvas) keeps instead the whole image on its step's other side, which undo and redo
//! swap in; one that moves every pixel whole (rotate, flip) undoes by its inverse and
//! redoes by applying it again, keeping nothing.

mod blend;
mod brush;
mod color;
mod document;
mod fill;
mod geom;
mod image;
mod orient;
mod par;
mod pencil;
mod resample;
mod select;
mod shape;
mod transform;

pub use blend::Blend;
pub use brush::Brush;
pub use color::Color;
pub use document::{Document, Operation, Stroke, Tip};
pub use fill::Fill;
pub use geom::Rect;
pub use image::{Image, SizeError, MAX_SIDE};
pub use orient::{Flip, Turn};
pub use pencil::Pencil;
pub use select::{spanning, Floating};
pub use shape::{Kind, Shape, ShapeTool, Style};
pub use transform::{Anchor, Resampling, Transform};

/// The side of the square tiles history saves pixels in.
pub const TILE: u32 = 64;
