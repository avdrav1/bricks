//! How a cell style looks on screen (STYLE-3, ADR 0009): the pool's fonts, colours,
//! fills, borders, and alignment resolved once per style for GTK and Pango.

use data_model::{
    Color, Edge, Fill, HAlign, LineStyle, Pattern, StyleId, StylePool, Underline, VAlign,
};
use formula::FormatColor;
use gtk::{gdk, pango};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

/// Explicit text colours below this contrast against their background fall back to the
/// automatic colour, so dark themes stay readable (ADR 0009).
const MIN_CONTRAST: f64 = 3.0;

/// A line of a cell border as drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line {
    pub width: f32,
    pub color: gdk::RGBA,
    pub dash: Dash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dash {
    Solid,
    /// On and off lengths in pixels.
    Dashed(u8, u8),
    Double,
}

/// A style as the grid draws it.
pub struct Look {
    /// The cell's font, or `None` for the grid's own (the workbook's default font).
    pub font: Option<pango::FontDescription>,
    /// Text decorations Pango needs as attributes.
    pub attrs: Option<pango::AttrList>,
    /// Same key, same font and attributes: layouts are cached by it.
    pub font_key: u32,
    /// Text colour, or `None` for the theme's.
    pub color: Option<gdk::RGBA>,
    pub fill: Option<gdk::RGBA>,
    /// Top, bottom, left, right.
    pub borders: [Option<Line>; 4],
    pub horizontal: HAlign,
    pub vertical: VAlign,
    pub wrap: bool,
    /// Indent steps (each three spaces' width, as in Excel).
    pub indent: u8,
    /// The style's number format code; `None` for General.
    pub format: Option<Box<str>>,
}

impl Look {
    pub fn has_fill_or_borders(&self) -> bool {
        self.fill.is_some() || self.borders.iter().any(Option::is_some)
    }
}

/// The looks of a workbook's styles, resolved on first use.
pub struct Looks {
    pool: Arc<StylePool>,
    /// The grid's font and text colour, and whether the theme is dark.
    base: pango::FontDescription,
    fg: gdk::RGBA,
    looks: Vec<Option<Rc<Look>>>,
    fonts: HashMap<FontKey, u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FontKey {
    family: Option<Box<str>>,
    /// Pango units.
    size: i32,
    bold: bool,
    italic: bool,
    underline: u8,
    strike: bool,
}

impl Looks {
    pub fn new(pool: Arc<StylePool>, base: pango::FontDescription, fg: gdk::RGBA) -> Self {
        Self {
            pool,
            base,
            fg,
            looks: Vec::new(),
            fonts: HashMap::new(),
        }
    }

    /// Start over when the pool, the grid's font, or the theme's colours changed.
    pub fn sync(&mut self, pool: &Arc<StylePool>, base: &pango::FontDescription, fg: gdk::RGBA) {
        if !Arc::ptr_eq(&self.pool, pool) || self.base != *base || self.fg != fg {
            *self = Self::new(pool.clone(), base.clone(), fg);
        }
    }

    pub fn get(&mut self, id: StyleId) -> Rc<Look> {
        let i = id.0 as usize;
        if let Some(Some(look)) = self.looks.get(i) {
            return look.clone();
        }
        let look = Rc::new(self.resolve(id));
        if self.looks.len() <= i {
            self.looks.resize(i + 1, None);
        }
        self.looks[i] = Some(look.clone());
        look
    }

    /// A number format's colour tag (`[Red]`) for a cell of `look`, readable on its
    /// background; `None` keeps the look's own colour.
    pub fn tag_color(&self, look: &Look, tag: FormatColor) -> Option<gdk::RGBA> {
        let rgb = match tag {
            FormatColor::Black => 0x000000,
            FormatColor::Blue => 0x0000FF,
            FormatColor::Cyan => 0x00FFFF,
            FormatColor::Green => 0x00FF00,
            FormatColor::Magenta => 0xFF00FF,
            FormatColor::Red => 0xFF0000,
            FormatColor::White => 0xFFFFFF,
            FormatColor::Yellow => 0xFFFF00,
            // `[Color1]` is palette entry 8: the first eight entries repeat the next eight.
            FormatColor::Indexed(n) => self.pool.rgb(Color::Indexed(n.saturating_add(7)))?,
        };
        let c = rgba(rgb);
        (contrast(c, self.background(look.fill)) >= MIN_CONTRAST).then_some(c)
    }

    /// What a cell shows its text on: its fill, or the theme's background.
    fn background(&self, fill: Option<gdk::RGBA>) -> gdk::RGBA {
        fill.unwrap_or(if self.dark() {
            gdk::RGBA::new(0.14, 0.14, 0.14, 1.0)
        } else {
            gdk::RGBA::WHITE
        })
    }

    fn dark(&self) -> bool {
        luminance(self.fg) > 0.5
    }

    fn rgba(&self, color: Color) -> Option<gdk::RGBA> {
        self.pool.rgb(color).map(rgba)
    }

    fn resolve(&mut self, id: StyleId) -> Look {
        let pool = self.pool.clone();
        let xf = pool.xf(id);
        let font = pool.font(xf);
        let default = pool.default_font();

        let fill = match pool.fill(xf) {
            Some(Fill::Pattern {
                pattern: Pattern::Solid,
                fg,
                ..
            }) => self.rgba(*fg),
            // Other patterns draw as their foreground over their background, half and half.
            Some(Fill::Pattern {
                pattern: Pattern::Other(_),
                fg,
                bg,
            }) => match (self.rgba(*fg), self.rgba(*bg)) {
                (Some(f), Some(b)) => Some(mix(f, b)),
                (f, b) => f
                    .or(b)
                    .map(|c| gdk::RGBA::new(c.red(), c.green(), c.blue(), 0.5)),
            },
            Some(Fill::Gradient { first }) => self.rgba(*first),
            _ => None,
        };
        // The automatic text colour: the theme's, or black or white for a fill.
        let auto = match fill {
            Some(f) if luminance(f) > 0.4 => gdk::RGBA::BLACK,
            Some(_) => gdk::RGBA::WHITE,
            None => self.fg,
        };
        let background = self.background(fill);
        let color = match self.rgba(font.color) {
            Some(c) if contrast(c, background) >= MIN_CONTRAST => Some(c),
            _ => (fill.is_some()).then_some(auto),
        };

        let same_family = font.name.eq_ignore_ascii_case(&default.name);
        let size = if font.size == default.size || default.size == 0 {
            self.base.size()
        } else {
            (f64::from(self.base.size()) * f64::from(font.size) / f64::from(default.size)).round()
                as i32
        };
        let underline = match font.underline {
            Underline::None => 0,
            Underline::Single | Underline::SingleAccounting => 1,
            Underline::Double | Underline::DoubleAccounting => 2,
        };
        let key = FontKey {
            family: (!same_family).then(|| font.name.clone()),
            size,
            bold: font.bold,
            italic: font.italic,
            underline,
            strike: font.strike,
        };
        let plain = key.family.is_none() && size == self.base.size() && !font.bold && !font.italic;
        let font_desc = (!plain).then(|| {
            let mut d = self.base.clone();
            if let Some(family) = &key.family {
                d.set_family(family);
            }
            d.set_size(size);
            if font.bold {
                d.set_weight(pango::Weight::Bold);
            }
            if font.italic {
                d.set_style(pango::Style::Italic);
            }
            d
        });
        let attrs = (underline != 0 || font.strike).then(|| {
            let list = pango::AttrList::new();
            if underline != 0 {
                list.insert(pango::AttrInt::new_underline(if underline == 2 {
                    pango::Underline::Double
                } else {
                    pango::Underline::Single
                }));
            }
            if font.strike {
                list.insert(pango::AttrInt::new_strikethrough(true));
            }
            list
        });
        let font_key = if plain && attrs.is_none() {
            0
        } else {
            let next = self.fonts.len() as u32 + 1;
            *self.fonts.entry(key).or_insert(next)
        };

        let border = pool.border(xf).cloned().unwrap_or_default();
        let line = |e: &Edge| self.line(e);
        let borders = [
            line(&border.top),
            line(&border.bottom),
            line(&border.left),
            line(&border.right),
        ];
        let format = pool
            .custom_format(xf.num_fmt)
            .or_else(|| formula::builtin_format(xf.num_fmt))
            .filter(|c| !c.eq_ignore_ascii_case("general"))
            .map(Box::from);
        Look {
            font: font_desc,
            attrs,
            font_key,
            color,
            fill,
            borders,
            horizontal: xf.align.horizontal,
            vertical: xf.align.vertical,
            wrap: xf.align.wrap,
            indent: xf.align.indent,
            format,
        }
    }

    fn line(&self, e: &Edge) -> Option<Line> {
        let (width, dash) = match e.style {
            LineStyle::None => return None,
            LineStyle::Thin => (1.0, Dash::Solid),
            LineStyle::Hair => (1.0, Dash::Dashed(1, 1)),
            LineStyle::Medium => (2.0, Dash::Solid),
            LineStyle::Thick => (3.0, Dash::Solid),
            LineStyle::Dashed => (1.0, Dash::Dashed(3, 1)),
            LineStyle::Dotted => (1.0, Dash::Dashed(1, 1)),
            LineStyle::Double => (3.0, Dash::Double),
            LineStyle::MediumDashed => (2.0, Dash::Dashed(6, 2)),
            LineStyle::DashDot | LineStyle::DashDotDot => (1.0, Dash::Dashed(4, 2)),
            LineStyle::MediumDashDot | LineStyle::MediumDashDotDot | LineStyle::SlantDashDot => {
                (2.0, Dash::Dashed(6, 2))
            }
        };
        Some(Line {
            width,
            color: self.rgba(e.color).unwrap_or(self.fg),
            dash,
        })
    }
}

pub fn rgba(rgb: u32) -> gdk::RGBA {
    let c = |shift: u32| ((rgb >> shift) & 0xFF) as f32 / 255.0;
    gdk::RGBA::new(c(16), c(8), c(0), 1.0)
}

fn mix(a: gdk::RGBA, b: gdk::RGBA) -> gdk::RGBA {
    gdk::RGBA::new(
        (a.red() + b.red()) / 2.0,
        (a.green() + b.green()) / 2.0,
        (a.blue() + b.blue()) / 2.0,
        1.0,
    )
}

/// WCAG relative luminance.
fn luminance(c: gdk::RGBA) -> f64 {
    let lin = |v: f32| {
        let v = f64::from(v);
        if v <= 0.039_28 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.red()) + 0.7152 * lin(c.green()) + 0.0722 * lin(c.blue())
}

/// WCAG contrast ratio of two colours (1 to 21).
fn contrast(a: gdk::RGBA, b: gdk::RGBA) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;
    use data_model::{Font, Tint, Xf};

    fn pool_with(font: Font, fill: Option<u32>) -> Arc<StylePool> {
        let mut pool = StylePool::default();
        pool.fonts.push(font);
        let fill_id = fill.map_or(0, |rgb| {
            pool.fills.push(Fill::Pattern {
                pattern: Pattern::Solid,
                fg: Color::Rgb(rgb),
                bg: Color::Auto,
            });
            pool.fills.len() as u32 - 1
        });
        pool.xfs.push(Xf {
            font: 1,
            fill: fill_id,
            ..Xf::default()
        });
        Arc::new(pool)
    }

    fn look(pool: Arc<StylePool>, fg: gdk::RGBA) -> Rc<Look> {
        Looks::new(pool, pango::FontDescription::from_string("Sans 10"), fg).get(StyleId(1))
    }

    #[test]
    fn unreadable_text_colours_fall_back_on_dark_themes() {
        let black = Font {
            color: Color::Theme {
                index: 1,
                tint: Tint::default(),
            },
            ..Font::default()
        };
        let light_fg = gdk::RGBA::new(0.9, 0.9, 0.9, 1.0);
        // Black text on a dark theme with no fill: the theme's colour instead.
        assert_eq!(look(pool_with(black.clone(), None), light_fg).color, None);
        // On a light theme it stays.
        assert_eq!(
            look(pool_with(black.clone(), None), gdk::RGBA::BLACK).color,
            Some(gdk::RGBA::BLACK)
        );
        // On a yellow fill, black text reads on any theme.
        let on_yellow = look(pool_with(black, Some(0xFFFF00)), light_fg);
        assert_eq!(on_yellow.color, Some(gdk::RGBA::BLACK));
        assert_eq!(on_yellow.fill, Some(rgba(0xFFFF00)));
        // Automatic text on a dark fill is white.
        let auto = Font {
            color: Color::Auto,
            ..Font::default()
        };
        assert_eq!(
            look(pool_with(auto, Some(0x305496)), light_fg).color,
            Some(gdk::RGBA::WHITE)
        );
    }

    #[test]
    fn the_default_font_draws_in_the_grid_font() {
        let pool = Arc::new(StylePool::default());
        let mut looks = Looks::new(
            pool,
            pango::FontDescription::from_string("Sans 10"),
            gdk::RGBA::BLACK,
        );
        let plain = looks.get(StyleId(0));
        assert!(plain.font.is_none() && plain.attrs.is_none() && plain.font_key == 0);
        let big = Font {
            name: "Arial".into(),
            size: 2200,
            bold: true,
            ..Font::default()
        };
        let styled = look(pool_with(big, None), gdk::RGBA::BLACK);
        let desc = styled.font.clone().unwrap();
        assert_eq!(desc.family().as_deref(), Some("Arial"));
        assert_eq!(
            desc.size(),
            20 * pango::SCALE,
            "22 pt over an 11 pt default doubles 10"
        );
        assert_eq!(desc.weight(), pango::Weight::Bold);
        assert_ne!(styled.font_key, 0);
    }
}
