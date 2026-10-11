//! An open deck: the presentation as `pptx` read it, with its pictures decoded into GDK
//! textures. Both happen in [`Deck::load`], off the UI thread (ADR 0004).

use gtk::{gdk, glib};
use presentation_model::{Background, Fill, Image, Presentation, ShapeKind};
use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::sync::Arc;

/// Why a file doesn't open.
#[derive(Debug)]
pub enum OpenError {
    Io(std::io::Error),
    /// A format Frame doesn't open (`.ppt`, `.odp`, Keynote, ...): what to do instead.
    Refused(&'static str),
    Read(pptx::Error),
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::Io(e) => write!(f, "{e}"),
            OpenError::Refused(why) => f.write_str(why),
            OpenError::Read(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for OpenError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            OpenError::Io(e) => Some(e),
            OpenError::Read(e) => Some(e),
            OpenError::Refused(_) => None,
        }
    }
}

pub struct Deck {
    pub presentation: Presentation,
    /// Decoded pictures by the address of their bytes (slides share an `Arc` per part).
    textures: HashMap<usize, gdk::Texture>,
}

fn key(image: &Image) -> usize {
    Arc::as_ptr(&image.bytes).cast::<u8>() as usize
}

impl Deck {
    /// Read the deck at `path` and decode its pictures. Blocking: run it off the UI thread.
    pub fn load(path: &Path) -> Result<Deck, OpenError> {
        let kind = pptx::kind(path).map_err(OpenError::Io)?;
        if let Some(why) = pptx::refusal(kind) {
            return Err(OpenError::Refused(why));
        }
        let presentation = pptx::read(path).map_err(OpenError::Read)?;
        let mut deck = Deck {
            presentation,
            textures: HashMap::new(),
        };
        let mut images = Vec::new();
        for slide in &deck.presentation.slides {
            if let Background::Picture(image) = &slide.background {
                images.push(image);
            }
            for shape in &slide.shapes {
                if let Fill::Picture(image) = &shape.fill {
                    images.push(image);
                }
                match &shape.kind {
                    ShapeKind::Picture(p) => images.push(&p.image),
                    ShapeKind::Unsupported {
                        preview: Some(image),
                        ..
                    } => images.push(image),
                    _ => {}
                }
            }
        }
        let mut textures = HashMap::new();
        for image in images {
            if !image.format.is_drawable() || textures.contains_key(&key(image)) {
                continue;
            }
            // GDK's loaders read PNG, JPEG, GIF, and BMP; a damaged picture draws as a
            // labelled outline.
            match gdk::Texture::from_bytes(&glib::Bytes::from(&image.bytes[..])) {
                Ok(t) => {
                    textures.insert(key(image), t);
                }
                Err(e) => tracing::warn!("a {} picture doesn't decode: {e}", image.format.name()),
            }
        }
        deck.textures = textures;
        Ok(deck)
    }

    /// The decoded picture, when it decodes.
    pub fn texture(&self, image: &Image) -> Option<&gdk::Texture> {
        self.textures.get(&key(image))
    }

    /// The slide with the most to draw (shapes, paragraphs, table cells), for the
    /// drawing benchmark.
    pub fn densest_slide(&self) -> usize {
        let weight = |s: &presentation_model::Slide| -> usize {
            s.shapes
                .iter()
                .map(|sh| {
                    1 + sh.text.as_ref().map_or(0, |t| t.paragraphs.len())
                        + match &sh.kind {
                            ShapeKind::Table(t) => t
                                .rows
                                .iter()
                                .flat_map(|r| &r.cells)
                                .map(|c| 1 + c.text.paragraphs.len())
                                .sum(),
                            _ => 0,
                        }
                })
                .sum()
        };
        let slides = &self.presentation.slides;
        (0..slides.len())
            .max_by_key(|&i| (weight(&slides[i]), std::cmp::Reverse(i)))
            .unwrap_or(0)
    }
}
