use crate::{Color, Rect};
use std::fmt;

/// The most pixels an image may have on a side: twice 8K, 1 GiB of pixels at most.
pub const MAX_SIDE: u32 = 16384;

/// Why an image can't have the size asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SizeError {
    /// A side of zero pixels.
    Empty,
    /// A side longer than [`MAX_SIDE`].
    TooLarge { width: u32, height: u32 },
}

impl fmt::Display for SizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SizeError::Empty => f.write_str("an image needs at least one pixel on each side"),
            SizeError::TooLarge { width, height } => write!(
                f,
                "{width}×{height} is too large: at most {MAX_SIDE} pixels a side"
            ),
        }
    }
}

impl std::error::Error for SizeError {}

/// An RGBA8 raster, straight alpha, rows top to bottom with no padding: `width * 4` bytes
/// a row.
#[derive(Clone, PartialEq, Eq)]
pub struct Image {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl fmt::Debug for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Image({}×{})", self.width, self.height)
    }
}

impl Image {
    /// An image every pixel of which is `fill`.
    pub fn new(width: u32, height: u32, fill: Color) -> Result<Image, SizeError> {
        if width == 0 || height == 0 {
            return Err(SizeError::Empty);
        }
        if width > MAX_SIDE || height > MAX_SIDE {
            return Err(SizeError::TooLarge { width, height });
        }
        let data = fill.to_bytes().repeat(width as usize * height as usize);
        Ok(Image {
            width,
            height,
            data,
        })
    }

    /// An image of `data`'s pixels: RGBA8, straight alpha, rows top to bottom, `width * 4`
    /// bytes a row (a decoded file's).
    ///
    /// # Panics
    ///
    /// If `data` isn't `width * height * 4` bytes.
    pub fn from_rgba(width: u32, height: u32, data: Vec<u8>) -> Result<Image, SizeError> {
        if width == 0 || height == 0 {
            return Err(SizeError::Empty);
        }
        if width > MAX_SIDE || height > MAX_SIDE {
            return Err(SizeError::TooLarge { width, height });
        }
        assert_eq!(
            data.len(),
            width as usize * height as usize * 4,
            "RGBA8 data for {width}×{height}"
        );
        Ok(Image {
            width,
            height,
            data,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// The whole image as a rectangle.
    pub fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.width, self.height)
    }

    /// Bytes per row.
    pub fn stride(&self) -> usize {
        self.width as usize * 4
    }

    /// Every pixel's bytes, row after row.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// The pixel at `(x, y)`, if it's in the image.
    pub fn pixel(&self, x: u32, y: u32) -> Option<Color> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = self.offset(x, y);
        Some(Color::from_bytes(self.data[i..i + 4].try_into().unwrap()))
    }

    /// The pixels of `rect` (inside the image), packed row after row.
    pub fn copy_rect(&self, rect: Rect) -> Vec<u8> {
        debug_assert!(rect.right() <= self.width && rect.bottom() <= self.height);
        let row = rect.width as usize * 4;
        let mut out = Vec::with_capacity(row * rect.height as usize);
        for y in rect.y..rect.bottom() {
            let i = self.offset(rect.x, y);
            out.extend_from_slice(&self.data[i..i + row]);
        }
        out
    }

    /// Writes `pixels`, packed as [`Image::copy_rect`] gives them, into `rect`.
    pub(crate) fn paste_rect(&mut self, rect: Rect, pixels: &[u8]) {
        let row = rect.width as usize * 4;
        debug_assert_eq!(pixels.len(), row * rect.height as usize);
        for (k, y) in (rect.y..rect.bottom()).enumerate() {
            let i = self.offset(rect.x, y);
            self.data[i..i + row].copy_from_slice(&pixels[k * row..(k + 1) * row]);
        }
    }

    /// Sets pixels `x0..x1` of row `y` (inside the image) to `color`.
    pub(crate) fn fill_span(&mut self, y: u32, x0: u32, x1: u32, color: Color) {
        let (i, j) = (self.offset(x0, y), self.offset(x1, y));
        let px = color.to_bytes();
        for p in self.data[i..j].as_chunks_mut::<4>().0 {
            *p = px;
        }
    }

    /// Sets the pixel at `(x, y)` (inside the image) to `color`.
    pub(crate) fn set_pixel(&mut self, x: u32, y: u32, color: Color) {
        let i = self.offset(x, y);
        self.data[i..i + 4].copy_from_slice(&color.to_bytes());
    }

    /// Every pixel's bytes, row after row, a pixel an item.
    pub(crate) fn pixels(&self) -> &[[u8; 4]] {
        self.data.as_chunks::<4>().0
    }

    fn offset(&self, x: u32, y: u32) -> usize {
        (y as usize * self.width as usize + x as usize) * 4
    }
}
