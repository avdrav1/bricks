//! Etch's image files: reading PNG, JPEG, and BMP into the image model's [`Image`] (RGBA8,
//! straight alpha). Decoding is the pure-Rust `image` crate's, with only these three
//! codecs. Toolkit-free: the app calls [`read`] off its UI thread.
//!
//! A file's format is told by its content, never its name. PNG keeps its alpha; JPEG has
//! none, and BMP opens opaque whatever its header says (Paint's reading: the fourth byte of
//! a 32-bit BMP is unused in most files, so honouring it makes ordinary BMPs vanish).
//! Images over [`MAX_SIDE`] pixels a side are refused before their pixels are decoded.

use image::codecs::{bmp::BmpDecoder, jpeg::JpegDecoder, png::PngDecoder};
use image::{DynamicImage, ImageDecoder, ImageError, ImageFormat};
use image_model::{Image, SizeError, MAX_SIDE};
use std::fmt;
use std::io::{self, Cursor};
use std::path::Path;

/// The file formats Etch opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
    Bmp,
}

impl Format {
    /// The format's usual name: "PNG", "JPEG", "BMP".
    pub fn name(self) -> &'static str {
        match self {
            Format::Png => "PNG",
            Format::Jpeg => "JPEG",
            Format::Bmp => "BMP",
        }
    }
}

/// Why a file can't be opened. Its [`Display`](fmt::Display) is the reason, in words for
/// the alert that names the file.
#[derive(Debug)]
pub enum OpenError {
    /// The file couldn't be read at all.
    Read(io::Error),
    /// The file has no bytes.
    Empty,
    /// The content is an image format Etch doesn't open (`Some`: which one), or no image.
    Unsupported(Option<&'static str>),
    /// The file says it's `format` but can't be decoded as one: truncated or damaged, or
    /// using a part of the format the decoder doesn't read (`detail` says which).
    Damaged { format: Format, detail: String },
    /// Larger than [`MAX_SIDE`] pixels a side.
    TooLarge { width: u32, height: u32 },
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::Read(e) => match e.kind() {
                io::ErrorKind::NotFound => f.write_str("The file doesn't exist."),
                io::ErrorKind::PermissionDenied => {
                    f.write_str("You don't have permission to read it.")
                }
                io::ErrorKind::IsADirectory => f.write_str("It's a folder, not a file."),
                _ => write!(f, "It can't be read: {e}."),
            },
            OpenError::Empty => f.write_str("The file is empty."),
            OpenError::Unsupported(Some(kind)) => write!(
                f,
                "It's a {kind} image. Etch opens PNG, JPEG, and BMP images."
            ),
            OpenError::Unsupported(None) => {
                f.write_str("It isn't an image Etch can open: PNG, JPEG, or BMP.")
            }
            OpenError::Damaged { format, detail } => write!(
                f,
                "It's a damaged or incomplete {} image ({detail}).",
                format.name()
            ),
            OpenError::TooLarge { width, height } => write!(
                f,
                "It's {width}×{height} pixels; Etch opens images up to {MAX_SIDE} pixels a side."
            ),
        }
    }
}

impl std::error::Error for OpenError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            OpenError::Read(e) => Some(e),
            _ => None,
        }
    }
}

/// Reads and decodes the image file at `path`.
pub fn read(path: &Path) -> Result<Image, OpenError> {
    let bytes = std::fs::read(path).map_err(OpenError::Read)?;
    decode(&bytes)
}

/// The format of an image file's `bytes`, from their content.
pub fn format_of(bytes: &[u8]) -> Result<Format, OpenError> {
    if bytes.is_empty() {
        return Err(OpenError::Empty);
    }
    match image::guess_format(bytes) {
        Ok(ImageFormat::Png) => Ok(Format::Png),
        Ok(ImageFormat::Jpeg) => Ok(Format::Jpeg),
        Ok(ImageFormat::Bmp) => Ok(Format::Bmp),
        Ok(other) => Err(OpenError::Unsupported(kind_name(other))),
        Err(_) => Err(OpenError::Unsupported(None)),
    }
}

/// Decodes an image file's `bytes`.
pub fn decode(bytes: &[u8]) -> Result<Image, OpenError> {
    let format = format_of(bytes)?;
    let damaged = |e: ImageError| damaged(format, e);
    if format == Format::Jpeg && !jpeg_ends(bytes) {
        // The decoder fills a truncated JPEG's missing rows with grey without a word.
        return Err(OpenError::Damaged {
            format,
            detail: "the file ends too soon".to_owned(),
        });
    }
    let input = Cursor::new(bytes);
    let decoded = match format {
        Format::Png => checked(format, PngDecoder::new(input).map_err(damaged)?)
            .and_then(|d| DynamicImage::from_decoder(d).map_err(damaged)),
        Format::Jpeg => checked(format, JpegDecoder::new(input).map_err(damaged)?)
            .and_then(|d| DynamicImage::from_decoder(d).map_err(damaged)),
        Format::Bmp => checked(format, BmpDecoder::new(input).map_err(damaged)?)
            .and_then(|d| DynamicImage::from_decoder(d).map_err(damaged)),
    }?;
    let (width, height) = (decoded.width(), decoded.height());
    let force_opaque = format != Format::Png && decoded.color().has_alpha();
    let mut rgba = decoded.into_rgba8().into_raw();
    if force_opaque {
        for px in rgba.as_chunks_mut::<4>().0 {
            px[3] = u8::MAX;
        }
    }
    Image::from_rgba(width, height, rgba).map_err(|e| match e {
        SizeError::Empty => no_pixels(format),
        SizeError::TooLarge { width, height } => OpenError::TooLarge { width, height },
    })
}

/// `decoder`, if its image isn't empty or larger than [`MAX_SIDE`] a side, with no limit
/// on what decoding it may allocate (the `image` crate's default caps it at 512 MiB, short
/// of a 16384×16384 image).
fn checked<D: ImageDecoder>(format: Format, mut decoder: D) -> Result<D, OpenError> {
    let (width, height) = decoder.dimensions();
    if width > MAX_SIDE || height > MAX_SIDE {
        return Err(OpenError::TooLarge { width, height });
    }
    if width == 0 || height == 0 {
        return Err(no_pixels(format));
    }
    // Without limits, setting them can't fail.
    let _ = decoder.set_limits(image::Limits::no_limits());
    Ok(decoder)
}

fn no_pixels(format: Format) -> OpenError {
    OpenError::Damaged {
        format,
        detail: "it has no pixels".to_owned(),
    }
}

/// Whether a JPEG's last scan is followed by its end-of-image marker (FF D9): in scan data
/// a 0xFF byte is always followed by 0x00 or a restart marker, so the first FF D9 after the
/// last start-of-scan (FF DA) is the image's end. An embedded thumbnail's markers come
/// before the main image's scans.
fn jpeg_ends(bytes: &[u8]) -> bool {
    let last_scan = bytes.windows(2).rposition(|w| w == [0xff, 0xda]);
    last_scan.is_some_and(|at| bytes[at..].windows(2).any(|w| w == [0xff, 0xd9]))
}

fn damaged(format: Format, e: ImageError) -> OpenError {
    let detail = match e {
        ImageError::IoError(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
            "the file ends too soon".to_owned()
        }
        ImageError::IoError(e) => e.to_string(),
        ImageError::Decoding(e) => e.to_string(),
        ImageError::Unsupported(e) => e.to_string(),
        ImageError::Limits(e) => e.to_string(),
        ImageError::Parameter(e) => e.to_string(),
        ImageError::Encoding(e) => e.to_string(),
    };
    OpenError::Damaged { format, detail }
}

/// A format's usual name, for one Etch doesn't open.
fn kind_name(format: ImageFormat) -> Option<&'static str> {
    Some(match format {
        ImageFormat::Gif => "GIF",
        ImageFormat::WebP => "WebP",
        ImageFormat::Tiff => "TIFF",
        ImageFormat::Tga => "TGA",
        ImageFormat::Dds => "DDS",
        ImageFormat::Ico => "ICO",
        ImageFormat::Hdr => "HDR",
        ImageFormat::OpenExr => "OpenEXR",
        ImageFormat::Pnm => "PNM",
        ImageFormat::Farbfeld => "farbfeld",
        ImageFormat::Avif => "AVIF",
        ImageFormat::Qoi => "QOI",
        _ => return None,
    })
}
