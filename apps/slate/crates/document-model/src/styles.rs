//! A document's paragraph styles (ADR 0008): each with its id, name, the style it's based
//! on and the one that follows it, its outline level, and its run and paragraph looks,
//! resolved through the styles it's based on and the document defaults.
//!
//! A paragraph names its style by id; a run holds only its direct formatting
//! ([`RunStyle`]'s overrides), so the look drawn is the style's with the run's overrides on
//! top ([`StyleSheet::look`]). A paragraph's own spacing, indents, borders, and shading go
//! over its style's the same way ([`ParaOverrides`], [`StyleSheet::spacing`],
//! [`StyleSheet::borders`], [`StyleSheet::shading`]; ADR 0009). Markdown and plain text
//! get Slate's built-in sheet ([`StyleSheet::builtin`]); a DOCX file's is its `styles.xml`.

use crate::{effective_tabs, Align, Paragraph, Rgb, RunStyle, TabStop};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::LazyLock;

/// A run's resolved look.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Look {
    /// The font family's name.
    pub font: String,
    /// In half-points (DOCX's unit).
    pub size: u16,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    /// `None` is automatic: the page's text colour.
    pub color: Option<Rgb>,
}

/// How far apart a paragraph's lines are.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum LineSpacing {
    /// A multiple of the font's line height: 1.0 is single spacing.
    Auto(f32),
    /// Exactly this far apart, in twips.
    Exact(u32),
    /// At least this far apart, in twips.
    AtLeast(u32),
}

impl Default for LineSpacing {
    fn default() -> Self {
        LineSpacing::Auto(1.0)
    }
}

/// A paragraph's spacing and indents, in twips (1/20 pt, 1/1440 in): its style's, or its
/// style's with its own on top ([`StyleSheet::spacing`]).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ParaSpacing {
    pub space_before: u32,
    pub space_after: u32,
    pub line: LineSpacing,
    pub indent_left: i32,
    pub indent_right: i32,
    /// The first line's indent from `indent_left`; negative is a hanging indent.
    pub indent_first: i32,
    /// Word's contextual spacing: no space before the paragraph when the one before is in
    /// the same style, none after it when the next one is.
    pub contextual: bool,
}

/// A paragraph border's line (ADR 0009). Word's other line styles draw as the nearest of
/// these: dot-dash ones dashed, triple and thin-thick ones double, wavy, 3-D, inset, outset,
/// and art borders single.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BorderStyle {
    /// No line (Word's `nil` and `none`).
    #[default]
    None,
    Single,
    /// One line twice the width (Word's `thick`, as LibreOffice reads it).
    Thick,
    /// Two lines of the width with a gap of the width between them.
    Double,
    Dotted,
    Dashed,
}

/// One side of a paragraph's border (`w:top`, `w:bottom`, `w:left`, `w:right`, or
/// `w:between` in `w:pBdr`; ADR 0009).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Border {
    pub style: BorderStyle,
    /// Each line's width in eighths of a point (`w:sz`).
    pub width: u16,
    /// How far it is from the text, in points (`w:space`).
    pub space: u16,
    /// `None` is automatic: the text's colour.
    pub color: Option<Rgb>,
}

impl Border {
    /// Whether it draws a line.
    pub fn is_drawn(&self) -> bool {
        self.style != BorderStyle::None
    }

    /// Its lines and the gaps between them, in eighths of a point; 0 without a line.
    pub fn line_width(&self) -> u32 {
        let w = u32::from(self.width);
        match self.style {
            BorderStyle::None => 0,
            BorderStyle::Double => 3 * w,
            BorderStyle::Thick => 2 * w,
            BorderStyle::Single | BorderStyle::Dotted | BorderStyle::Dashed => w,
        }
    }

    /// How far it reaches out from the text, its space and its lines, in twips; 0 without
    /// a line.
    pub fn reach(&self) -> u32 {
        if self.is_drawn() {
            u32::from(self.space) * 20 + (self.line_width() * 5).div_ceil(2)
        } else {
            0
        }
    }
}

/// A paragraph's borders, each side without a line when it has none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Borders {
    pub top: Border,
    pub bottom: Border,
    pub left: Border,
    pub right: Border,
    /// Between it and the next paragraph of a group with the same borders.
    pub between: Border,
}

impl Borders {
    /// Whether any side draws a line.
    pub fn any(&self) -> bool {
        [self.top, self.bottom, self.left, self.right, self.between]
            .iter()
            .any(Border::is_drawn)
    }
}

/// A paragraph's own border sides over its style's, side by side: `None` leaves the
/// style's, a side without a line (Word's `nil`) removes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BorderOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top: Option<Border>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bottom: Option<Border>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left: Option<Border>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub right: Option<Border>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub between: Option<Border>,
}

impl BorderOverrides {
    /// Whether it sets no side.
    pub fn is_empty(&self) -> bool {
        *self == BorderOverrides::default()
    }

    /// `borders` with these sides on top.
    pub fn over(&self, mut borders: Borders) -> Borders {
        for (to, from) in [
            (&mut borders.top, self.top),
            (&mut borders.bottom, self.bottom),
            (&mut borders.left, self.left),
            (&mut borders.right, self.right),
            (&mut borders.between, self.between),
        ] {
            if let Some(b) = from {
                *to = b;
            }
        }
        borders
    }
}

/// A paragraph's shading (`w:shd`; ADR 0009): the colour that fills its box, `None` for
/// none (Word's `auto` fill).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Shading {
    pub fill: Option<Rgb>,
}

/// A paragraph's own spacing, indents, borders, and shading over its style's (ADR 0009),
/// as [`RunStyle`] is a run's look over its style's: each `None` leaves the style's.
/// Distances in twips.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ParaOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_before: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_after: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<LineSpacing>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indent_left: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indent_right: Option<i32>,
    /// The first line's indent from the left one; negative is a hanging indent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indent_first: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contextual: Option<bool>,
    #[serde(skip_serializing_if = "BorderOverrides::is_empty")]
    pub borders: BorderOverrides,
    /// Its own shading replaces its style's whole: an automatic fill removes it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shading: Option<Shading>,
}

impl ParaOverrides {
    /// Whether it sets nothing.
    pub fn is_empty(&self) -> bool {
        *self == ParaOverrides::default()
    }

    /// `spacing` with these overrides' spacing and indents on top.
    pub fn over(&self, mut spacing: ParaSpacing) -> ParaSpacing {
        fn set<T: Copy>(to: &mut T, from: Option<T>) {
            if let Some(v) = from {
                *to = v;
            }
        }
        set(&mut spacing.space_before, self.space_before);
        set(&mut spacing.space_after, self.space_after);
        set(&mut spacing.line, self.line);
        set(&mut spacing.indent_left, self.indent_left);
        set(&mut spacing.indent_right, self.indent_right);
        set(&mut spacing.indent_first, self.indent_first);
        set(&mut spacing.contextual, self.contextual);
        spacing
    }

    /// These overrides without the indents: what a paragraph keeps when it joins or leaves
    /// a list, whose levels indent it.
    pub fn without_indents(self) -> ParaOverrides {
        ParaOverrides {
            indent_left: None,
            indent_right: None,
            indent_first: None,
            ..self
        }
    }
}

/// A paragraph's resolved look.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ParaLook {
    pub align: Align,
    pub spacing: ParaSpacing,
    /// Its tab stops, in position order, without clears (ADR 0009).
    pub tabs: Vec<TabStop>,
    pub borders: Borders,
    pub shading: Shading,
}

/// A paragraph style.
#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    pub id: String,
    /// The name the user sees.
    pub name: String,
    pub based_on: Option<String>,
    /// The style of the paragraph Enter starts after one in this style.
    pub next: Option<String>,
    /// The heading level, 1 to 9; `None` for body text.
    pub outline: Option<u8>,
    /// The document's title style.
    pub title: bool,
    pub run: Look,
    pub para: ParaLook,
}

impl Style {
    /// A quotation's style: Slate's and Word's `Quote`, Word's `Intense Quote`, and
    /// LibreOffice's `Quotations` and `Block Quotation`.
    pub fn is_quote(&self) -> bool {
        self.id == "Quote"
            || ["quote", "intense quote", "quotations", "block quotation"]
                .iter()
                .any(|n| self.name.trim().eq_ignore_ascii_case(n))
    }

    /// A heading or the title: a paragraph after it is in another style.
    pub fn is_heading(&self) -> bool {
        self.title || self.outline.is_some()
    }
}

/// A document's paragraph styles, in the order the document lists them, with the one
/// paragraphs that name none have.
#[derive(Debug, Clone, PartialEq)]
pub struct StyleSheet {
    styles: Vec<Style>,
    /// Each style's index by id.
    index: HashMap<String, usize>,
    default: usize,
}

/// The built-in sheet's normal font and size (the page's: Liberation Sans 11 pt).
const BUILTIN_FONT: &str = "Liberation Sans";
const BUILTIN_SIZE: u16 = 22;
/// Heading sizes relative to body text, levels 1 to 6; and the title's (SL1's page looks).
const HEADING_SCALE: [f32; 6] = [2.0, 1.6, 1.35, 1.2, 1.1, 1.0];
const TITLE_SCALE: f32 = 2.4;
/// Spacing, in pixels at 96 dpi (SL1's flow's).
const PARAGRAPH_SPACE: u32 = 8;
const HEADING_ABOVE: [u32; 6] = [18, 14, 12, 10, 8, 8];
const HEADING_BELOW: u32 = 6;
const TITLE_BELOW: u32 = 12;
const LEVEL_INDENT: i32 = 28;
/// A quote's text colour, as the page draws it.
const QUOTE: Rgb = [0x5E, 0x5C, 0x64];
/// Twips per pixel at 96 dpi.
const TWIPS_PER_PX: u32 = 15;

impl StyleSheet {
    /// A sheet of `styles` whose paragraphs that name none are in `default_id` (the first
    /// style when no style has that id). A sheet always has a style: without any, it has
    /// the built-in Normal.
    pub fn new(mut styles: Vec<Style>, default_id: &str) -> StyleSheet {
        if styles.is_empty() {
            styles.push(BUILTIN.default_style().clone());
        }
        let mut index = HashMap::with_capacity(styles.len());
        for (i, s) in styles.iter().enumerate() {
            index.entry(s.id.clone()).or_insert(i);
        }
        let default = index.get(default_id).copied().unwrap_or(0);
        StyleSheet {
            styles,
            index,
            default,
        }
    }

    /// Slate's sheet for Markdown and plain text: Normal, Title, Heading 1 to 6, and
    /// Quote, in the looks the page draws them.
    pub fn builtin() -> StyleSheet {
        BUILTIN.clone()
    }

    fn make_builtin() -> StyleSheet {
        let normal = Look {
            font: BUILTIN_FONT.to_owned(),
            size: BUILTIN_SIZE,
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            color: None,
        };
        let para = ParaLook {
            spacing: ParaSpacing {
                space_after: PARAGRAPH_SPACE * TWIPS_PER_PX,
                ..ParaSpacing::default()
            },
            ..ParaLook::default()
        };
        let scaled = |scale: f32| (f32::from(BUILTIN_SIZE) * scale).round() as u16;
        let derived = |id: &str, name: &str| Style {
            id: id.to_owned(),
            name: name.to_owned(),
            based_on: Some("Normal".to_owned()),
            next: Some("Normal".to_owned()),
            outline: None,
            title: false,
            run: normal.clone(),
            para: para.clone(),
        };
        let mut styles = vec![Style {
            based_on: None,
            next: None,
            ..derived("Normal", "Normal")
        }];
        styles.push(Style {
            title: true,
            run: Look {
                size: scaled(TITLE_SCALE),
                bold: true,
                ..normal.clone()
            },
            para: ParaLook {
                spacing: ParaSpacing {
                    space_after: TITLE_BELOW * TWIPS_PER_PX,
                    ..para.spacing
                },
                ..para.clone()
            },
            ..derived("Title", "Title")
        });
        for (i, scale) in HEADING_SCALE.iter().enumerate() {
            let n = i + 1;
            styles.push(Style {
                outline: Some(n as u8),
                run: Look {
                    size: scaled(*scale),
                    bold: true,
                    ..normal.clone()
                },
                para: ParaLook {
                    spacing: ParaSpacing {
                        space_before: HEADING_ABOVE[i] * TWIPS_PER_PX,
                        space_after: HEADING_BELOW * TWIPS_PER_PX,
                        ..para.spacing
                    },
                    ..para.clone()
                },
                ..derived(&format!("Heading{n}"), &format!("Heading {n}"))
            });
        }
        styles.push(Style {
            run: Look {
                italic: true,
                color: Some(QUOTE),
                ..normal.clone()
            },
            para: ParaLook {
                spacing: ParaSpacing {
                    indent_left: LEVEL_INDENT * TWIPS_PER_PX as i32,
                    ..para.spacing
                },
                ..para.clone()
            },
            ..derived("Quote", "Quote")
        });
        StyleSheet::new(styles, "Normal")
    }

    /// The styles, in order.
    pub fn styles(&self) -> &[Style] {
        &self.styles
    }

    /// The style with id `id`.
    pub fn get(&self, id: &str) -> Option<&Style> {
        self.index.get(id).map(|&i| &self.styles[i])
    }

    /// The style paragraphs that name none are in.
    pub fn default_style(&self) -> &Style {
        &self.styles[self.default]
    }

    pub fn default_id(&self) -> &str {
        &self.default_style().id
    }

    /// The style a paragraph naming `id` is drawn in: its own; for an id of the built-in
    /// sheet's headings, title, or quote that this sheet lacks (a heading set or pasted in
    /// a Word document without one, which its save adds), the built-in style; else the
    /// default.
    pub fn style_of(&self, id: &str) -> &Style {
        self.get(id)
            .or_else(|| Self::builtin_style(id))
            .unwrap_or_else(|| self.default_style())
    }

    /// The built-in sheet's heading, title, or quote with id `id` (not its Normal).
    pub fn builtin_style(id: &str) -> Option<&'static Style> {
        BUILTIN.get(id).filter(|s| s.id != BUILTIN.default_id())
    }

    /// The style named `name` (ignoring ASCII case).
    pub fn by_name(&self, name: &str) -> Option<&Style> {
        self.styles
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
    }

    /// The heading style of level `level`: the one named `Heading N`, else the first with
    /// that outline level.
    pub fn heading(&self, level: u8) -> Option<&Style> {
        let name = format!("Heading {level}");
        let headings = || {
            self.styles
                .iter()
                .filter(move |s| !s.title && s.outline == Some(level))
        };
        headings()
            .find(|s| s.name.eq_ignore_ascii_case(&name))
            .or_else(|| headings().next())
    }

    /// The look of a run with direct formatting `run` in a paragraph of style `style_id`:
    /// the style's run look with the run's overrides on top.
    pub fn look(&self, style_id: &str, run: &RunStyle) -> Look {
        let mut look = self.style_of(style_id).run.clone();
        if let Some(font) = &run.font {
            look.font.clone_from(font);
        }
        if let Some(size) = run.size {
            look.size = size;
        }
        if let Some(b) = run.bold {
            look.bold = b;
        }
        if let Some(i) = run.italic {
            look.italic = i;
        }
        if let Some(u) = run.underline {
            look.underline = u;
        }
        if let Some(s) = run.strike {
            look.strike = s;
        }
        if run.color.is_some() {
            look.color = run.color;
        }
        look
    }

    /// The stops paragraph `p`'s tabs advance to: its style's with its own on top, in
    /// position order (ADR 0009).
    pub fn tab_stops(&self, p: &Paragraph) -> Vec<TabStop> {
        effective_tabs(&self.style_of(&p.style).para.tabs, &p.tabs)
    }

    /// Paragraph `p`'s spacing and indents: its style's with its own on top (ADR 0009).
    pub fn spacing(&self, p: &Paragraph) -> ParaSpacing {
        p.own.over(self.style_of(&p.style).para.spacing)
    }

    /// Paragraph `p`'s borders: its style's with its own sides on top (ADR 0009).
    pub fn borders(&self, p: &Paragraph) -> Borders {
        p.own.borders.over(self.style_of(&p.style).para.borders)
    }

    /// Paragraph `p`'s shading: its own, else its style's (ADR 0009).
    pub fn shading(&self, p: &Paragraph) -> Shading {
        p.own
            .shading
            .unwrap_or(self.style_of(&p.style).para.shading)
    }

    /// Whether paragraphs `a` and `b`, one after the other, are bordered together, as Word
    /// groups them: they have lines and the same borders, and the same left and right
    /// indents. A group draws its top line above its first paragraph, its bottom line
    /// below its last, and its between line between each two (ADR 0009).
    pub fn bordered_together(&self, a: &Paragraph, b: &Paragraph) -> bool {
        let borders = self.borders(a);
        let (sa, sb) = (self.spacing(a), self.spacing(b));
        borders.any()
            && borders == self.borders(b)
            && (sa.indent_left, sa.indent_right) == (sb.indent_left, sb.indent_right)
    }
}

impl Default for StyleSheet {
    fn default() -> Self {
        BUILTIN.clone()
    }
}

/// The built-in sheet, made once.
static BUILTIN: LazyLock<StyleSheet> = LazyLock::new(StyleSheet::make_builtin);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overrides_go_over_the_style() {
        let sheet = StyleSheet::builtin();
        let heading = sheet.look("Heading1", &RunStyle::default());
        assert!(heading.bold && heading.size == 44 && heading.font == "Liberation Sans");
        let run = RunStyle {
            bold: Some(false),
            italic: Some(true),
            font: Some("Caladea".into()),
            color: Some([1, 2, 3]),
            ..RunStyle::default()
        };
        let look = sheet.look("Heading1", &run);
        assert_eq!(
            look,
            Look {
                font: "Caladea".into(),
                size: 44,
                bold: false,
                italic: true,
                underline: false,
                strike: false,
                color: Some([1, 2, 3]),
            }
        );
        // A quote's own colour stays without an override; an unknown id is Normal's.
        assert_eq!(sheet.look("Quote", &RunStyle::default()).color, Some(QUOTE));
        assert_eq!(sheet.look("Nope", &run).size, 22);
    }

    #[test]
    fn a_paragraphs_own_spacing_goes_over_its_styles() {
        let sheet = StyleSheet::builtin();
        let mut p = Paragraph::new("Heading1");
        let heading = sheet.style_of("Heading1").para.spacing;
        assert_eq!(sheet.spacing(&p), heading);
        p.own = ParaOverrides {
            space_after: Some(0),
            indent_right: Some(720),
            indent_first: Some(-360),
            ..ParaOverrides::default()
        };
        assert_eq!(
            sheet.spacing(&p),
            ParaSpacing {
                space_after: 0,
                indent_right: 720,
                indent_first: -360,
                ..heading
            }
        );
        assert_eq!(
            p.own.without_indents(),
            ParaOverrides {
                space_after: Some(0),
                ..ParaOverrides::default()
            }
        );
    }

    #[test]
    fn a_paragraphs_own_border_sides_and_shading_go_over_its_styles() {
        let line = |style, width, space| Border {
            style,
            width,
            space,
            color: Some([0x2F, 0x54, 0x96]),
        };
        let boxed = Style {
            id: "Boxed".into(),
            name: "Boxed".into(),
            para: ParaLook {
                borders: Borders {
                    top: line(BorderStyle::Single, 8, 4),
                    bottom: line(BorderStyle::Single, 8, 4),
                    ..Borders::default()
                },
                shading: Shading {
                    fill: Some([0xDE, 0xEA, 0xF6]),
                },
                ..ParaLook::default()
            },
            ..StyleSheet::builtin().default_style().clone()
        };
        let sheet = StyleSheet::new(vec![boxed.clone()], "Boxed");
        let mut p = Paragraph::new("Boxed");
        assert_eq!(sheet.borders(&p), boxed.para.borders);
        assert_eq!(sheet.shading(&p), boxed.para.shading);
        // Its own nil top removes the style's, its own left adds one, its automatic fill
        // removes the style's.
        p.own.borders = BorderOverrides {
            top: Some(Border::default()),
            left: Some(line(BorderStyle::Double, 4, 1)),
            ..BorderOverrides::default()
        };
        p.own.shading = Some(Shading::default());
        let borders = sheet.borders(&p);
        assert!(!borders.top.is_drawn());
        assert_eq!(borders.bottom, boxed.para.borders.bottom);
        assert_eq!(borders.left.line_width(), 12);
        // 1 pt and three half-point lines: 20 + 30 twips.
        assert_eq!(borders.left.reach(), 50);
        assert_eq!(sheet.shading(&p).fill, None);
        assert_eq!(p.own.without_indents().borders, p.own.borders);

        // Bordered together: the same borders and indents.
        let mut q = Paragraph::new("Boxed");
        q.own = p.own;
        assert!(sheet.bordered_together(&p, &q));
        q.own.indent_left = Some(720);
        assert!(!sheet.bordered_together(&p, &q));
        let plain = StyleSheet::builtin();
        assert!(!plain.bordered_together(&Paragraph::new("Normal"), &Paragraph::new("Normal")));
    }

    #[test]
    fn a_sheet_without_a_heading_draws_the_builtin_one() {
        let custom = Style {
            id: "Body".into(),
            name: "Body".into(),
            ..StyleSheet::builtin().default_style().clone()
        };
        let sheet = StyleSheet::new(vec![custom], "Body");
        assert_eq!(sheet.style_of("Heading2").name, "Heading 2");
        assert_eq!(sheet.style_of("Normal").id, "Body");
        assert_eq!(sheet.style_of("Other").id, "Body");
        assert!(sheet.heading(2).is_none());
        assert_eq!(StyleSheet::builtin().heading(2).unwrap().id, "Heading2");
    }
}
