//! Frame's PPTX reader (ADR 0004): reads a `.pptx` package into a
//! [`presentation_model::Presentation`] with every inherited value resolved.
//!
//! Slide order comes from `presentation.xml` (`sldIdLst` through its relationships);
//! placeholders take their frame from the slide's layout, then its master (matched by
//! `idx`, then `type`); group transforms are applied, so each shape's frame is where it
//! shows on the slide.

mod detect;
mod package;
mod read;
mod xml;

pub use detect::{kind, refusal, Kind};

use presentation_model::Presentation;
use std::fmt;
use std::io::{self, Read, Seek};
use std::path::Path;

/// Why a deck can't be read.
#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// Not a ZIP package.
    NotPackage(String),
    /// A part the deck needs is missing.
    Missing(String),
    /// A part can't be parsed.
    Damaged {
        part: String,
        message: String,
    },
}

impl Error {
    pub(crate) fn damaged(part: &str, message: impl Into<String>) -> Self {
        Error::Damaged {
            part: part.to_owned(),
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::NotPackage(e) => write!(f, "This isn't a PowerPoint presentation ({e})."),
            Error::Missing(part) => write!(f, "The presentation has no “{part}”."),
            Error::Damaged { part, message } => {
                write!(f, "The presentation's “{part}” is damaged: {message}")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

/// Read the deck at `path`.
pub fn read(path: &Path) -> Result<Presentation, Error> {
    let file = std::fs::File::open(path)?;
    read_from(io::BufReader::new(file))
}

/// Read a deck from any seekable reader (a file, or bytes in a `Cursor`).
pub fn read_from(reader: impl Read + Seek + 'static) -> Result<Presentation, Error> {
    read::Reader::new(package::Package::open(Box::new(reader))?).presentation()
}
