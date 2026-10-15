//! Etch's image files: reading PNG, JPEG, and BMP into the image model's [`Image`] (RGBA8,
//! straight alpha), and saving it as one. Decoding and encoding are the pure-Rust `image`
//! crate's, with only these three codecs. Toolkit-free: the app calls [`read`] and
//! [`save`] off its UI thread.
//!
//! A file's format is told by its content, never its name. PNG keeps its alpha; JPEG has
//! none, and BMP opens opaque whatever its header says (Paint's reading: the fourth byte of
//! a 32-bit BMP is unused in most files, so honouring it makes ordinary BMPs vanish).
//! Images over [`MAX_SIDE`] pixels a side are refused before their pixels are decoded.
//!
//! Saving picks the format by the file name's extension ([`Format::for_path`]). PNG keeps
//! every pixel and its alpha exactly; JPEG (at [`JPEG_QUALITY`]) and BMP (24-bit) are
//! opaque, so their pixels are composited onto white. The file is written atomically
//! through `suite_file::save_atomic`: a failed save leaves the old file as it was.

use image::codecs::{
    bmp::{BmpDecoder, BmpEncoder},
    jpeg::{JpegDecoder, JpegEncoder},
    png::{PngDecoder, PngEncoder},
};
use image::{DynamicImage, ExtendedColorType, ImageDecoder, ImageEncoder, ImageError, ImageFormat};
use image_model::{Image, SizeError, MAX_SIDE};
use std::fmt;
use std::io::{self, Cursor};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use suite_file::{SaveError, WriteSeek};

/// The quality Etch saves JPEG images at (1 to 100): 90, where JPEG's loss is hard to see
/// in photos and screenshots and files stay a fraction of PNG's size; the `image` crate's
/// own default is 75.
pub const JPEG_QUALITY: u8 = 90;

/// The file formats Etch opens and saves.
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

    /// The format's usual extension: "png", "jpg", "bmp".
    pub fn extension(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpeg => "jpg",
            Format::Bmp => "bmp",
        }
    }

    /// The format a file at `path` is saved in, by its name's extension, in any case:
    /// .png; .jpg or .jpeg; .bmp.
    pub fn for_path(path: &Path) -> Result<Format, UnknownExtension> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        match ext.as_deref() {
            Some("png") => Ok(Format::Png),
            Some("jpg" | "jpeg") => Ok(Format::Jpeg),
            Some("bmp") => Ok(Format::Bmp),
            _ => Err(UnknownExtension),
        }
    }

    /// Whether the format keeps transparency: only PNG. JPEG and BMP images are saved
    /// composited onto white.
    pub fn keeps_alpha(self) -> bool {
        self == Format::Png
    }
}

/// A file name whose extension isn't one Etch saves. Its [`Display`](fmt::Display) is the
/// reason, in words for the alert that names the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnknownExtension;

impl fmt::Display for UnknownExtension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "Etch saves PNG (.png), JPEG (.jpg or .jpeg), and BMP (.bmp) images: \
             end the name with one of them.",
        )
    }
}

impl std::error::Error for UnknownExtension {}

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

/// Reads and decodes the image file at `path`; the format it's in, by its content.
pub fn read(path: &Path) -> Result<(Image, Format), OpenError> {
    let bytes = std::fs::read(path).map_err(OpenError::Read)?;
    let format = format_of(&bytes)?;
    Ok((decode(&bytes)?, format))
}

/// Whether any pixel of `image` isn't fully opaque: saving it as JPEG or BMP changes it.
pub fn has_transparency(image: &Image) -> bool {
    image
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .any(|p| p[3] != u8::MAX)
}

/// The bytes of `image` as a `format` file. JPEG and BMP are composited onto white.
pub fn encode(image: &Image, format: Format) -> Result<Vec<u8>, ImageError> {
    let (w, h) = (image.width(), image.height());
    let mut out = Vec::new();
    match format {
        Format::Png => {
            PngEncoder::new(&mut out).write_image(image.data(), w, h, ExtendedColorType::Rgba8)?
        }
        Format::Jpeg => JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY).write_image(
            &on_white(image),
            w,
            h,
            ExtendedColorType::Rgb8,
        )?,
        Format::Bmp => BmpEncoder::new(&mut out).write_image(
            &on_white(image),
            w,
            h,
            ExtendedColorType::Rgb8,
        )?,
    }
    Ok(out)
}

/// `image`'s pixels composited onto white, as RGB8 rows: each channel `c` at alpha `a`
/// becomes `(c·a + 255·(255 − a)) / 255`, rounded.
pub fn on_white(image: &Image) -> Vec<u8> {
    let pixels = image.data().as_chunks::<4>().0;
    let mut rgb = Vec::with_capacity(pixels.len() * 3);
    for &[r, g, b, a] in pixels {
        if a == u8::MAX {
            rgb.extend_from_slice(&[r, g, b]);
        } else {
            let (a, under) = (u32::from(a), 255 * (255 - u32::from(a)));
            let mix = |c: u8| ((u32::from(c) * a + under + 127) / 255) as u8;
            rgb.extend_from_slice(&[mix(r), mix(g), mix(b)]);
        }
    }
    rgb
}

/// Saves `image` at `path` as a `format` file, atomically: the bytes go to a temp file
/// beside it, are synced, read back and compared, and renamed over the old file. On any
/// error the old file stays as it was.
pub fn save(path: &Path, image: &Image, format: Format) -> Result<(), SaveError> {
    let bytes = encode(image, format).map_err(|e| SaveError::Io(io::Error::other(e)))?;
    write_atomic(path, &bytes, |w| w.write_all(&bytes))
}

/// Writes `bytes` at `dest` atomically with `write` (which writes them), verifying the
/// temp file holds exactly `bytes` before it replaces `dest`.
fn write_atomic<W>(dest: &Path, bytes: &[u8], write: W) -> Result<(), SaveError>
where
    W: FnOnce(&mut (dyn WriteSeek + Send)) -> io::Result<()>,
{
    let cancel = AtomicBool::new(false);
    suite_file::save_atomic(dest, &cancel, write, |tmp| match std::fs::read(tmp) {
        Ok(written) if written == bytes => Ok(()),
        Ok(written) => Err(format!(
            "{} bytes written of the image's {}",
            written.len(),
            bytes.len()
        )),
        Err(e) => Err(e.to_string()),
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A directory holding `old.png` (`old`'s bytes), for one test.
    fn with_old_file(test: &str, old: &[u8]) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("etch-io-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("old.png");
        std::fs::write(&path, old).unwrap();
        (dir, path)
    }

    fn left_in(dir: &Path) -> usize {
        std::fs::read_dir(dir).unwrap().count()
    }

    #[test]
    fn a_writer_failing_partway_leaves_the_old_file() {
        let old = b"the old file's bytes".to_vec();
        let (dir, path) = with_old_file("writer", &old);
        let new = vec![7u8; 1 << 21];
        let result = write_atomic(&path, &new, |w| {
            w.write_all(&new[..new.len() / 2])?;
            Err(io::Error::other("the disk filled up"))
        });
        assert!(matches!(result, Err(SaveError::Io(_))), "{result:?}");
        assert_eq!(std::fs::read(&path).unwrap(), old);
        assert_eq!(left_in(&dir), 1, "the temp file is removed");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_short_write_fails_verifying_and_leaves_the_old_file() {
        let old = b"the old file's bytes".to_vec();
        let (dir, path) = with_old_file("short", &old);
        let new = vec![7u8; 4096];
        let result = write_atomic(&path, &new, |w| w.write_all(&new[..1000]));
        match result {
            Err(SaveError::Verify(why)) => {
                assert_eq!(why, "1000 bytes written of the image's 4096")
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(std::fs::read(&path).unwrap(), old);
        assert_eq!(left_in(&dir), 1, "the temp file is removed");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
