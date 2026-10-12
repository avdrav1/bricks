//! An open deck: the presentation as `pptx` read it and as edits change it, with its
//! pictures decoded into GDK textures, and what saving it patches: a copy of the package
//! it was read from. Copying, reading, and decoding happen in [`Deck::load`], off the UI
//! thread (ADR 0004).

use gtk::{gdk, glib};
use presentation_model::{Background, Fill, Image, Presentation, ShapeKind};
use std::cell::{Ref, RefCell, RefMut};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
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
    presentation: RefCell<Presentation>,
    /// The presentation as read, and a private copy of the package it was read from:
    /// every save patches that copy with what changed since (ADR 0006).
    read: Arc<Presentation>,
    original: Arc<pptx::Original>,
    /// The file the deck saves to: the one it was opened from, until Save As.
    path: RefCell<PathBuf>,
    /// Decoded pictures by the address of their bytes (slides share an `Arc` per part).
    textures: RefCell<Textures>,
}

/// Decoded pictures by the address of their bytes, each with its bytes, so no other
/// picture's can take that address while it's kept.
type Textures = HashMap<usize, (Arc<[u8]>, gdk::Texture)>;

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
        let original = pptx::Original::copy_of(path).map_err(OpenError::Io)?;
        let presentation = pptx::read(original.path()).map_err(OpenError::Read)?;
        let mut images = Vec::new();
        // Layouts' new slides too: a slide added on a layout no slide uses shows its
        // pictures.
        let layouts = presentation.layouts.iter().map(|l| &l.slide);
        for slide in presentation.slides.iter().chain(layouts) {
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
                    textures.insert(key(image), (image.bytes.clone(), t));
                }
                Err(e) => tracing::warn!("a {} picture doesn't decode: {e}", image.format.name()),
            }
        }
        Ok(Deck {
            read: Arc::new(presentation.clone()),
            presentation: RefCell::new(presentation),
            original: Arc::new(original),
            path: RefCell::new(path.to_owned()),
            textures: RefCell::new(textures),
        })
    }

    /// The presentation as it is now.
    pub fn presentation(&self) -> Ref<'_, Presentation> {
        self.presentation.borrow()
    }

    /// The presentation, to change through `deck-edit`.
    pub fn presentation_mut(&self) -> RefMut<'_, Presentation> {
        self.presentation.borrow_mut()
    }

    /// The file the deck saves to.
    pub fn path(&self) -> PathBuf {
        self.path.borrow().clone()
    }

    pub fn set_path(&self, path: PathBuf) {
        self.path.replace(path);
    }

    /// What a save needs besides the deck as it is now: the deck as read, and the package
    /// it was read from.
    pub fn origin(&self) -> (Arc<Presentation>, Arc<pptx::Original>) {
        (self.read.clone(), self.original.clone())
    }

    /// The decoded picture, when it decodes.
    pub fn texture(&self, image: &Image) -> Option<gdk::Texture> {
        self.textures
            .borrow()
            .get(&key(image))
            .map(|(_, t)| t.clone())
    }

    /// Keep `texture`, decoded from `image`, to draw it (a picture inserted).
    pub fn add_texture(&self, image: &Image, texture: gdk::Texture) {
        self.textures
            .borrow_mut()
            .insert(key(image), (image.bytes.clone(), texture));
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
        let presentation = self.presentation();
        let slides = &presentation.slides;
        (0..slides.len())
            .max_by_key(|&i| (weight(&slides[i]), std::cmp::Reverse(i)))
            .unwrap_or(0)
    }
}
