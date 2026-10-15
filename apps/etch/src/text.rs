//! The text tool's text (ETCH-12): typed on the canvas in a font family, size, colour,
//! bold, and italic, shown over the image exactly as it will land, then put on the image
//! as pixels.
//!
//! A [`TextBox`] holds the text being typed, its caret, an input method's preedit, and its
//! look, laid out with Pango. Each change renders it again ([`render`]): Pango draws the
//! layout with cairo into an ARGB32 surface, whose premultiplied pixels become straight
//! RGBA ([`straight`]) cropped to the ink. The canvas shows those pixels over the image,
//! and committing hands them to `Document::blend`, so the preview is what lands and redo
//! lays down the same bytes. The image model never sees fonts.

use gtk::prelude::*;
use gtk::{cairo, gdk, glib, pango};
use image_model::{Color, Image, Rect};
use std::sync::OnceLock;

/// The family text starts in: fontconfig's sans-serif, whatever it is on the machine.
pub const DEFAULT_FAMILY: &str = "Sans";

/// How text looks: its font family, size (the em, in image pixels), bold, italic, and
/// colour (the foreground's, alpha included).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextLook {
    pub family: &'static str,
    pub size: u32,
    pub bold: bool,
    pub italic: bool,
    pub color: Color,
}

impl TextLook {
    fn font(&self) -> pango::FontDescription {
        let mut font = pango::FontDescription::new();
        font.set_family(self.family);
        font.set_absolute_size(self.size as f64 * pango::SCALE as f64);
        font.set_weight(if self.bold {
            pango::Weight::Bold
        } else {
            pango::Weight::Normal
        });
        font.set_style(if self.italic {
            pango::Style::Italic
        } else {
            pango::Style::Normal
        });
        font
    }
}

/// The font families: listed once a process, so each is a `&'static str` and the tool
/// settings stay `Copy`.
static FAMILIES: OnceLock<Vec<String>> = OnceLock::new();

/// The font families installed (Pango's, from `context`'s font map) and
/// [`DEFAULT_FAMILY`], sorted.
pub fn families(context: &pango::Context) -> &'static [String] {
    FAMILIES.get_or_init(|| {
        let mut names: Vec<String> = context
            .font_map()
            .map(|m| {
                m.list_families()
                    .iter()
                    .map(|f| f.name().to_string())
                    .collect()
            })
            .unwrap_or_default();
        names.push(DEFAULT_FAMILY.to_owned());
        names.sort_unstable_by_key(|n| n.to_lowercase());
        names.dedup();
        names
    })
}

/// An input method's text being composed: its text, looks, and cursor (a byte offset in
/// it).
pub type Preedit = (String, pango::AttrList, usize);

/// Text rendered as pixels: where they go on the image, the straight RGBA pixels, and
/// them as a texture for the canvas.
pub struct Raster {
    pub rect: Rect,
    pub pixels: Image,
    pub texture: gdk::Texture,
}

/// What a key did to the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyOutcome {
    /// Enter: put the text on the image.
    Commit,
    /// Escape: drop it.
    Cancel,
    /// The text changed.
    Changed,
    /// The caret moved, or the key was the text's with nothing to do.
    Moved,
    /// Not the text's key.
    Ignored,
}

/// The text being typed: its top-left corner's image pixel, its text, the caret, the
/// preedit, its look, its layout, and its pixels.
pub struct TextBox {
    at: (i64, i64),
    text: String,
    /// A byte offset into `text`.
    caret: usize,
    preedit: Option<Preedit>,
    look: TextLook,
    layout: pango::Layout,
    /// The image's bounds: the pixels are clipped to them.
    bounds: Rect,
    raster: Option<Raster>,
}

impl TextBox {
    /// Empty text with its top-left corner at image pixel `at`, laid out in `context`
    /// (the canvas's), on an image of `bounds`.
    pub fn new(context: &pango::Context, at: (i64, i64), look: TextLook, bounds: Rect) -> TextBox {
        let mut text = TextBox {
            at,
            text: String::new(),
            caret: 0,
            preedit: None,
            look,
            layout: pango::Layout::new(context),
            bounds,
            raster: None,
        };
        text.changed();
        text
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn raster(&self) -> Option<&Raster> {
        self.raster.as_ref()
    }

    /// The pixels to put on the image, if the text has any ink there.
    pub fn into_raster(self) -> Option<Raster> {
        self.raster
    }

    /// The text takes `look`: a new font, size, or colour shows at once.
    pub fn set_look(&mut self, look: TextLook) {
        if look != self.look {
            self.look = look;
            self.changed();
        }
    }

    /// Typing `s` at the caret (an input method's commit).
    pub fn insert(&mut self, s: &str) {
        self.text.insert_str(self.caret, s);
        self.caret += s.len();
        self.changed();
    }

    pub fn set_preedit(&mut self, preedit: Option<Preedit>) {
        if self.preedit.is_none() && preedit.is_none() {
            return;
        }
        self.preedit = preedit;
        self.changed();
    }

    /// A key pressed while typing, with its modifiers. The primary modifier (Ctrl, ⌘ on
    /// macOS) with Home and End goes to the text's ends.
    pub fn key(&mut self, key: gdk::Key, mods: gdk::ModifierType) -> KeyOutcome {
        use gdk::Key as K;
        let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
        let primary = suite_ui::keys::is_primary(mods);
        if mods.contains(gdk::ModifierType::ALT_MASK) {
            return KeyOutcome::Ignored;
        }
        // While an input method composes, its keys are its own.
        if self.preedit.is_some() {
            return KeyOutcome::Ignored;
        }
        match key {
            K::Return | K::KP_Enter | K::ISO_Enter if shift => {
                self.insert("\n");
                KeyOutcome::Changed
            }
            K::Return | K::KP_Enter | K::ISO_Enter => KeyOutcome::Commit,
            K::Escape => KeyOutcome::Cancel,
            K::BackSpace => match self.text[..self.caret].chars().next_back() {
                Some(c) => {
                    self.caret -= c.len_utf8();
                    self.text.remove(self.caret);
                    self.changed();
                    KeyOutcome::Changed
                }
                None => KeyOutcome::Moved,
            },
            K::Delete | K::KP_Delete => match self.caret < self.text.len() {
                true => {
                    self.text.remove(self.caret);
                    self.changed();
                    KeyOutcome::Changed
                }
                false => KeyOutcome::Moved,
            },
            K::Left | K::KP_Left => self.step(-1),
            K::Right | K::KP_Right => self.step(1),
            K::Up | K::KP_Up => self.line_step(-1),
            K::Down | K::KP_Down => self.line_step(1),
            K::Home | K::KP_Home => self.line_end(false, primary),
            K::End | K::KP_End => self.line_end(true, primary),
            // Tab would take the keys off the canvas.
            K::Tab | K::KP_Tab | K::ISO_Left_Tab => KeyOutcome::Moved,
            _ => KeyOutcome::Ignored,
        }
    }

    /// A click at image point `p`, inside the text: the caret goes there.
    pub fn click(&mut self, p: (f64, f64)) {
        if self.preedit.is_some() {
            return;
        }
        let s = pango::SCALE as f64;
        let x = ((p.0 - self.at.0 as f64) * s).round() as i32;
        let y = ((p.1 - self.at.1 as f64) * s).round() as i32;
        let (_, index, trailing) = self.layout.xy_to_index(x, y);
        self.caret = self.forward(index, trailing);
    }

    /// The image area the text takes (its layout's logical extents, at least the caret's
    /// height), as `(x, y, width, height)` in image pixels.
    pub fn frame(&self) -> (f64, f64, f64, f64) {
        let (_, logical) = self.layout.extents();
        let s = pango::SCALE as f64;
        (
            self.at.0 as f64 + logical.x() as f64 / s,
            self.at.1 as f64 + logical.y() as f64 / s,
            logical.width() as f64 / s,
            logical.height() as f64 / s,
        )
    }

    /// The caret's top and height, in image pixels: `(x, y, height)`.
    pub fn caret(&self) -> (f64, f64, f64) {
        let index = self.caret + self.preedit.as_ref().map_or(0, |p| p.2);
        let (strong, _) = self.layout.cursor_pos(index as i32);
        let s = pango::SCALE as f64;
        (
            self.at.0 as f64 + strong.x() as f64 / s,
            self.at.1 as f64 + strong.y() as f64 / s,
            strong.height() as f64 / s,
        )
    }

    /// Lays the text out again and renders it.
    fn changed(&mut self) {
        let layout = &self.layout;
        layout.set_font_description(Some(&self.look.font()));
        match &self.preedit {
            Some((text, attrs, _)) => {
                let mut joined = String::with_capacity(self.text.len() + text.len());
                joined.push_str(&self.text[..self.caret]);
                joined.push_str(text);
                joined.push_str(&self.text[self.caret..]);
                let preedit = attrs.copy().unwrap_or_default();
                let mut underline: pango::Attribute =
                    pango::AttrInt::new_underline(pango::Underline::Single).into();
                underline.set_start_index(0);
                underline.set_end_index(text.len() as u32);
                preedit.insert(underline);
                let all = pango::AttrList::new();
                all.splice(&preedit, self.caret as i32, text.len() as i32);
                layout.set_text(&joined);
                layout.set_attributes(Some(&all));
            }
            None => {
                layout.set_text(&self.text);
                layout.set_attributes(None);
            }
        }
        self.raster = render(&self.layout, self.at, self.look.color, self.bounds);
    }

    /// The byte offset `trailing` characters on from byte `index` of the layout's text.
    fn forward(&self, index: i32, trailing: i32) -> usize {
        let index = (index.max(0) as usize).min(self.text.len());
        self.text[index..]
            .char_indices()
            .nth(trailing.max(0) as usize)
            .map_or(self.text.len(), |(i, _)| index + i)
    }

    /// Left (`dir` −1) or Right a cursor position, as the text runs on screen.
    fn step(&mut self, dir: i32) -> KeyOutcome {
        let (index, trailing) = self
            .layout
            .move_cursor_visually(true, self.caret as i32, 0, dir);
        self.caret = match index {
            i if i < 0 => 0,
            i32::MAX => self.text.len(),
            i => self.forward(i, trailing),
        };
        KeyOutcome::Moved
    }

    /// Up (`dir` −1) or Down a line, keeping as near the caret's x as the line allows.
    fn line_step(&mut self, dir: i32) -> KeyOutcome {
        let (line, x) = self.layout.index_to_line_x(self.caret as i32, false);
        if let Some(l) = self.layout.line_readonly(line + dir) {
            let hit = l.x_to_index(x);
            self.caret = self.forward(hit.index(), hit.trailing());
        }
        KeyOutcome::Moved
    }

    /// Home (`end` false) or End: of the caret's line, or with `all` of the text.
    fn line_end(&mut self, end: bool, all: bool) -> KeyOutcome {
        self.caret = match (all, end) {
            (true, false) => 0,
            (true, true) => self.text.len(),
            (false, _) => {
                let (line, _) = self.layout.index_to_line_x(self.caret as i32, false);
                self.layout.line_readonly(line).map_or(self.caret, |l| {
                    let start = l.start_index().max(0) as usize;
                    match end {
                        true => (start + l.length().max(0) as usize).min(self.text.len()),
                        false => start,
                    }
                })
            }
        };
        KeyOutcome::Moved
    }
}

/// `layout`, with its top-left corner at image pixel `at`, drawn in `color` and clipped to
/// `bounds`: Pango renders it antialiased through cairo, and the pixels are cropped to
/// those with ink. `None` without ink on the image.
pub fn render(
    layout: &pango::Layout,
    at: (i64, i64),
    color: Color,
    bounds: Rect,
) -> Option<Raster> {
    let (ink, _) = layout.pixel_extents();
    if ink.width() <= 0 || ink.height() <= 0 {
        return None;
    }
    // Antialiasing may reach a pixel past the ink's extents.
    let clip = |lo: i64, hi: i64, min: u32, max: u32| (lo.max(min as i64), hi.min(max as i64));
    let (x0, x1) = clip(
        at.0 + ink.x() as i64 - 1,
        at.0 + (ink.x() + ink.width()) as i64 + 1,
        bounds.x,
        bounds.right(),
    );
    let (y0, y1) = clip(
        at.1 + ink.y() as i64 - 1,
        at.1 + (ink.y() + ink.height()) as i64 + 1,
        bounds.y,
        bounds.bottom(),
    );
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let (w, h) = ((x1 - x0) as u32, (y1 - y0) as u32);
    let mut surface =
        cairo::ImageSurface::create(cairo::Format::ARgb32, w as i32, h as i32).ok()?;
    {
        let cr = cairo::Context::new(&surface).ok()?;
        cr.translate((at.0 - x0) as f64, (at.1 - y0) as f64);
        // Opaque: the colour's alpha is applied as the pixels become straight.
        let opaque = crate::colours::rgba(Color { a: 255, ..color });
        let snapshot = gtk::Snapshot::new();
        snapshot.append_layout(layout, &opaque);
        snapshot.to_node()?.draw(&cr);
    }
    surface.flush();
    let stride = surface.stride() as usize;
    let data = surface.data().ok()?;
    let rgba = straight(&data, stride, w, h, color);
    let (cx0, cy0, cx1, cy1) = ink_bounds(&rgba, w, h)?;
    let (cw, ch) = (cx1 - cx0, cy1 - cy0);
    let mut cropped = Vec::with_capacity((cw * ch * 4) as usize);
    for y in cy0..cy1 {
        let row = (y * w * 4) as usize;
        cropped.extend_from_slice(&rgba[row + (cx0 * 4) as usize..row + (cx1 * 4) as usize]);
    }
    let bytes = glib::Bytes::from(&cropped[..]);
    let texture = gdk::MemoryTexture::new(
        cw as i32,
        ch as i32,
        gdk::MemoryFormat::R8g8b8a8,
        &bytes,
        cw as usize * 4,
    )
    .upcast();
    Some(Raster {
        rect: Rect::new(x0 as u32 + cx0, y0 as u32 + cy0, cw, ch),
        pixels: Image::from_rgba(cw, ch, cropped).ok()?,
        texture,
    })
}

/// Cairo's ARGB32 pixels (premultiplied, a native-endian word each, rows `stride` bytes
/// apart) of text drawn in `color` made opaque, as straight RGBA with `color`'s alpha.
/// A pixel of the colour at some coverage (all of plain text) gets the colour exactly,
/// its alpha the coverage times the colour's, so edges never drift in hue; any other (a
/// colour glyph's) is unpremultiplied.
pub fn straight(data: &[u8], stride: usize, w: u32, h: u32, color: Color) -> Vec<u8> {
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    let scale = |c: u8, a: u32| (c as u32 * a + 127) / 255;
    for y in 0..h as usize {
        for x in 0..w as usize {
            let o = y * stride + x * 4;
            let v = u32::from_ne_bytes(data[o..o + 4].try_into().expect("four bytes"));
            let (a, r, g, b) = (v >> 24, (v >> 16) & 255, (v >> 8) & 255, v & 255);
            if a == 0 {
                out.extend_from_slice(&[0; 4]);
                continue;
            }
            let alpha = scale(color.a, a) as u8;
            let near = |c: u8, p: u32| scale(c, a).abs_diff(p) <= 1;
            if near(color.r, r) && near(color.g, g) && near(color.b, b) {
                out.extend_from_slice(&[color.r, color.g, color.b, alpha]);
            } else {
                let un = |p: u32| ((p * 255 + a / 2) / a).min(255) as u8;
                out.extend_from_slice(&[un(r), un(g), un(b), alpha]);
            }
        }
    }
    out
}

/// The smallest box `(x0, y0, x1, y1)`, ends exclusive, holding every pixel of `rgba`
/// (`w` × `h`) with any alpha.
fn ink_bounds(rgba: &[u8], w: u32, h: u32) -> Option<(u32, u32, u32, u32)> {
    let mut found: Option<(u32, u32, u32, u32)> = None;
    for y in 0..h {
        for x in 0..w {
            if rgba[((y * w + x) * 4 + 3) as usize] == 0 {
                continue;
            }
            found = Some(match found {
                None => (x, y, x + 1, y + 1),
                Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1)),
            });
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One ARGB32 pixel's bytes as cairo stores it.
    fn argb(a: u32, r: u32, g: u32, b: u32) -> [u8; 4] {
        (a << 24 | r << 16 | g << 8 | b).to_ne_bytes()
    }

    #[test]
    fn text_pixels_become_straight_in_exactly_its_colour() {
        let red = Color::rgb(237, 28, 36);
        // Red at coverage 0, 3, 128, and 255, premultiplied as cairo rounds it, then a
        // padding pixel in the row's stride.
        let pm = |c: u32, a: u32| (c * a + 127) / 255;
        let mut data = Vec::new();
        for a in [0, 3, 128, 255] {
            data.extend_from_slice(&argb(a, pm(237, a), pm(28, a), pm(36, a)));
        }
        data.extend_from_slice(&[9; 4]);
        let out = straight(&data, 20, 4, 1, red);
        assert_eq!(
            out,
            [
                [0, 0, 0, 0],
                [237, 28, 36, 3],
                [237, 28, 36, 128],
                [237, 28, 36, 255]
            ]
            .concat()
        );
        // A see-through colour: the coverage times its alpha.
        let clear = Color::rgba(237, 28, 36, 128);
        let out = straight(&data, 20, 4, 1, clear);
        assert_eq!(out[4..8], [237, 28, 36, 2]);
        assert_eq!(out[8..12], [237, 28, 36, 64]);
        assert_eq!(out[12..16], [237, 28, 36, 128]);
    }

    #[test]
    fn other_colours_are_unpremultiplied() {
        // A colour glyph's green at half coverage in red text.
        let data = argb(128, 0, 64, 0);
        let out = straight(&data, 4, 1, 1, Color::rgb(237, 28, 36));
        assert_eq!(out, [0, 128, 0, 128]);
    }

    #[test]
    fn the_ink_bounds_hold_every_pixel_with_alpha() {
        let mut rgba = vec![0u8; 5 * 4 * 4];
        assert_eq!(ink_bounds(&rgba, 5, 4), None);
        rgba[(5 + 1) * 4 + 3] = 1;
        rgba[(3 * 5 + 3) * 4 + 3] = 255;
        assert_eq!(ink_bounds(&rgba, 5, 4), Some((1, 1, 4, 4)));
    }
}
