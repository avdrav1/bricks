/// A colour with straight (not premultiplied) alpha, 8 bits a channel: how the image
/// stores its pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const WHITE: Color = Color::rgb(255, 255, 255);
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);

    /// An opaque colour.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color { r, g, b, a }
    }

    /// The pixel's bytes in the image's order: red, green, blue, alpha.
    pub const fn to_bytes(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }

    pub const fn from_bytes([r, g, b, a]: [u8; 4]) -> Color {
        Color { r, g, b, a }
    }
}
