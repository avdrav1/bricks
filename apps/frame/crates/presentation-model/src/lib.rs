//! Frame's presentation model (ADR 0004): what a deck shows, with every value resolved.
//!
//! A reader (the `pptx` crate) walks slide → layout → master → theme once and stores final
//! values here, so the renderer and the tests never follow inheritance. Lengths are EMU
//! (English Metric Units: 914,400 per inch, 12,700 per point), as in the file.
//!
//! Toolkit-free: no GTK, no I/O.

use std::sync::Arc;

/// EMU per inch.
pub const EMU_PER_INCH: i64 = 914_400;
/// EMU per typographic point.
pub const EMU_PER_POINT: i64 = 12_700;

/// A whole deck: the slide size and the slides in show order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Presentation {
    pub size: Size,
    pub slides: Vec<Slide>,
}

/// A width and height in EMU.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Size {
    pub cx: i64,
    pub cy: i64,
}

/// One slide.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Slide {
    pub background: Background,
    /// Shapes in drawing order (z-order, bottom first): the master's and the layout's
    /// own shapes (see [`Layer`]) under the slide's.
    pub shapes: Vec<Shape>,
    /// Hidden slides are skipped by the slideshow and shown dimmed in the slide list.
    pub hidden: bool,
    /// The speaker notes' text, paragraphs joined by `\n`; empty without notes.
    pub notes: String,
}

impl Slide {
    /// The slide's own shapes, without those it shows from its layout and master.
    pub fn own_shapes(&self) -> impl Iterator<Item = &Shape> {
        self.shapes.iter().filter(|s| s.layer == Layer::Slide)
    }

    /// All the text the slide shows, shape by shape (table cells in reading order), one
    /// line per paragraph; empty paragraphs left out.
    pub fn text(&self) -> String {
        let mut lines = Vec::new();
        let mut push = |t: &TextBody| {
            lines.extend(
                t.paragraphs
                    .iter()
                    .map(Paragraph::text)
                    .filter(|l| !l.trim().is_empty()),
            )
        };
        for shape in &self.shapes {
            if let Some(t) = &shape.text {
                push(t);
            }
            if let ShapeKind::Table(table) = &shape.kind {
                for cell in table.rows.iter().flat_map(|r| &r.cells) {
                    if !cell.merged {
                        push(&cell.text);
                    }
                }
            }
        }
        lines.join("\n")
    }
}

/// What fills the slide behind its shapes.
#[derive(Debug, Clone, PartialEq)]
pub enum Background {
    Solid(Color),
    /// A picture stretched over the slide.
    Picture(Image),
    /// A fill Frame doesn't draw (gradient, pattern, tiled picture): shown as `approx`,
    /// with `label` (`Gradient`, ...) saying what it stands for.
    Other {
        label: String,
        approx: Color,
    },
}

impl Default for Background {
    /// A slide that sets no background anywhere is white.
    fn default() -> Self {
        Background::Solid(Color::WHITE)
    }
}

/// Where a shape comes from. Masters and layouts carry decoration (lines, logos, pictures)
/// that every slide using them shows; their placeholders are not shown, only inherited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layer {
    Master,
    Layout,
    #[default]
    Slide,
}

/// A shape's position on the slide: the unrotated box, then rotation about its centre
/// and flips. Group transforms are already applied.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Frame {
    pub x: i64,
    pub y: i64,
    pub cx: i64,
    pub cy: i64,
    /// Clockwise, in degrees.
    pub rotation: f64,
    pub flip_h: bool,
    pub flip_v: bool,
}

impl Frame {
    /// The centre of the box.
    pub fn centre(&self) -> (f64, f64) {
        (
            self.x as f64 + self.cx as f64 / 2.0,
            self.y as f64 + self.cy as f64 / 2.0,
        )
    }
}

/// A placeholder's role, from `<p:ph type>`; `Body` covers `obj`, PowerPoint's default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceholderKind {
    Title,
    CentredTitle,
    Subtitle,
    Body,
    Date,
    Footer,
    SlideNumber,
    Header,
    /// Chart, table, picture, media, SmartArt, clip art placeholders.
    Content,
}

/// What a shape is.
#[derive(Debug, Clone, PartialEq)]
pub enum ShapeKind {
    /// A shape drawn from geometry: `preset` is the preset name (`rect`, `ellipse`, ...),
    /// `custom` for freeform geometry; `adjust` its first adjust value as a fraction
    /// (a rounded rectangle's corner radius over its shorter side, ...).
    Geometry { preset: String, adjust: Option<f64> },
    /// A picture (`<p:pic>`).
    Picture(Picture),
    /// A table (`<a:tbl>` in a graphic frame).
    Table(Table),
    /// Content Frame doesn't draw (`Chart`, `SmartArt`, `Video`, `Picture (EMF)`, ...):
    /// shown as a labelled outline so nothing silently disappears, or as `preview`, the
    /// picture the file keeps for readers that can't show it (a video's poster frame, an
    /// embedded object's preview).
    Unsupported {
        label: String,
        preview: Option<Image>,
    },
}

/// One shape on a slide.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    /// `cNvPr name` (a SmartArt part's model id when it has none).
    pub name: String,
    pub frame: Frame,
    pub kind: ShapeKind,
    pub fill: Fill,
    pub outline: Option<Outline>,
    /// The placeholder role, for placeholders.
    pub placeholder: Option<PlaceholderKind>,
    pub layer: Layer,
    /// The shape's text, when it has a text body with any paragraph.
    pub text: Option<TextBody>,
}

impl Shape {
    /// The shape's text, paragraphs joined by `\n` (empty without text).
    pub fn plain_text(&self) -> String {
        self.text
            .as_ref()
            .map(TextBody::plain_text)
            .unwrap_or_default()
    }
}

/// What fills a shape.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Fill {
    #[default]
    None,
    Solid(Color),
    /// A picture stretched over the shape's box.
    Picture(Image),
    /// A fill Frame doesn't draw (`Gradient`, `Pattern`): shown as `approx` with the label.
    Unsupported {
        label: String,
        approx: Color,
    },
}

/// A shape's or a table border's line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Outline {
    /// EMU; 0 is the thinnest line the screen shows.
    pub width: i64,
    pub color: Color,
    pub dash: Dash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Dash {
    #[default]
    Solid,
    Dot,
    Dash,
    DashDot,
    LongDash,
}

/// A picture's bytes as stored in the file.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub format: ImageFormat,
    pub bytes: Arc<[u8]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Gif,
    Bmp,
    Tiff,
    Emf,
    Wmf,
    Svg,
    Other,
}

impl ImageFormat {
    /// The format of `bytes`, from their signature.
    pub fn of(bytes: &[u8]) -> ImageFormat {
        match bytes {
            [0x89, b'P', b'N', b'G', ..] => ImageFormat::Png,
            [0xFF, 0xD8, 0xFF, ..] => ImageFormat::Jpeg,
            [b'G', b'I', b'F', b'8', ..] => ImageFormat::Gif,
            [b'B', b'M', ..] => ImageFormat::Bmp,
            [b'I', b'I', 42, 0, ..] | [b'M', b'M', 0, 42, ..] => ImageFormat::Tiff,
            [1, 0, 0, 0, ..] if bytes.get(40..44) == Some(b" EMF") => ImageFormat::Emf,
            [0xD7, 0xCD, 0xC6, 0x9A, ..] | [1, 0, 9, 0, ..] | [2, 0, 9, 0, ..] => ImageFormat::Wmf,
            _ if is_svg(bytes) => ImageFormat::Svg,
            _ => ImageFormat::Other,
        }
    }

    /// Whether Frame draws this format (GDK's loaders read it everywhere Frame runs).
    pub fn is_drawable(self) -> bool {
        matches!(
            self,
            ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif | ImageFormat::Bmp
        )
    }

    /// `EMF`, `PNG`, ...: how labels name it.
    pub fn name(self) -> &'static str {
        match self {
            ImageFormat::Png => "PNG",
            ImageFormat::Jpeg => "JPEG",
            ImageFormat::Gif => "GIF",
            ImageFormat::Bmp => "BMP",
            ImageFormat::Tiff => "TIFF",
            ImageFormat::Emf => "EMF",
            ImageFormat::Wmf => "WMF",
            ImageFormat::Svg => "SVG",
            ImageFormat::Other => "unknown format",
        }
    }
}

fn is_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(512)];
    let text = String::from_utf8_lossy(head);
    let t = text.trim_start_matches('\u{feff}').trim_start();
    (t.starts_with("<?xml") || t.starts_with("<svg") || t.starts_with("<!--"))
        && text.contains("<svg")
}

/// A picture shape's content: the image and how much of it is cut off at each edge.
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    pub image: Image,
    pub crop: Crop,
}

/// Fractions of the image cut off at each edge (`srcRect`); negative values pad.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Crop {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

/// A table: column widths, and rows of cells with resolved text, fills, and borders
/// (table style applied).
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    /// Column widths, EMU.
    pub columns: Vec<i64>,
    pub rows: Vec<TableRow>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableRow {
    /// The row's height, EMU (rows grow with their text when drawn).
    pub height: i64,
    pub cells: Vec<TableCell>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableCell {
    pub text: TextBody,
    pub fill: Option<Color>,
    pub borders: Borders,
    /// Columns and rows this cell spans (1 each for a plain cell).
    pub col_span: u32,
    pub row_span: u32,
    /// Covered by a spanning cell to its left or above: not drawn on its own.
    pub merged: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Borders {
    pub top: Option<Outline>,
    pub bottom: Option<Outline>,
    pub left: Option<Outline>,
    pub right: Option<Outline>,
}

/// An sRGB colour with straight alpha (255 is opaque).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub alpha: u8,
}

impl Color {
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const WHITE: Color = Color::rgb(255, 255, 255);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color {
            r,
            g,
            b,
            alpha: 255,
        }
    }

    /// `RRGGBB`, as in the file.
    pub fn hex(&self) -> String {
        format!("{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
}

/// Text in a shape or table cell.
#[derive(Debug, Clone, PartialEq)]
pub struct TextBody {
    pub paragraphs: Vec<Paragraph>,
    pub anchor: Anchor,
    /// `anchorCtr`: the text block is centred horizontally in the box.
    pub anchor_centre: bool,
    /// Space between the box and the text, in EMU.
    pub insets: Insets,
    /// Lines wrap at the box's width (`wrap="square"`); otherwise they run on.
    pub wrap: bool,
    /// `normAutofit`'s font scale (1.0 = as written) and line spacing reduction (0.2 =
    /// 20% less), which PowerPoint stores when it shrank text to fit.
    pub font_scale: f64,
    pub line_reduction: f64,
    /// Text turned a quarter: `vert="vert"` (90 degrees clockwise) or `vert270`.
    pub vertical: Vertical,
}

impl TextBody {
    /// Paragraphs joined by `\n`.
    pub fn plain_text(&self) -> String {
        let texts: Vec<String> = self.paragraphs.iter().map(Paragraph::text).collect();
        texts.join("\n")
    }

    /// Whether there's any text to show.
    pub fn is_empty(&self) -> bool {
        self.paragraphs
            .iter()
            .all(|p| p.runs.iter().all(|r| r.text.is_empty()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Anchor {
    #[default]
    Top,
    Middle,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Vertical {
    #[default]
    Horizontal,
    /// Turned 90 degrees clockwise.
    Down,
    /// Turned 270 degrees clockwise.
    Up,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Insets {
    pub left: i64,
    pub top: i64,
    pub right: i64,
    pub bottom: i64,
}

impl Default for Insets {
    /// PowerPoint's defaults: 0.1 in left and right, 0.05 in top and bottom.
    fn default() -> Self {
        Insets {
            left: 91_440,
            top: 45_720,
            right: 91_440,
            bottom: 45_720,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Centre,
    Right,
    /// Both edges, last line left.
    Justify,
    /// Both edges, last line too.
    Distributed,
}

/// Line spacing or space before/after a paragraph.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Spacing {
    /// A multiple of the line height (1.0 is single spacing).
    Lines(f64),
    /// Points.
    Points(f64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Paragraph {
    /// Outline level, 0 to 8.
    pub level: u8,
    pub align: Align,
    /// Left margin of the text, in EMU (`marL`).
    pub margin_left: i64,
    /// First line's offset from the margin, in EMU (`indent`; negative hangs the bullet).
    pub indent: i64,
    pub line_spacing: Spacing,
    pub space_before: Spacing,
    pub space_after: Spacing,
    /// The bullet or number shown before the text, when it has one.
    pub bullet: Option<Bullet>,
    pub runs: Vec<Run>,
    /// The size, in points, of an empty paragraph's line (`endParaRPr`).
    pub end_size: f64,
}

impl Paragraph {
    /// The paragraph's text; line breaks are `\n`.
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }
}

/// A bullet character or number with its resolved look.
#[derive(Debug, Clone, PartialEq)]
pub struct Bullet {
    /// What shows: `•`, `–`, `3.`, `iv)`, ...
    pub text: String,
    pub font: String,
    /// Points.
    pub size: f64,
    pub color: Color,
}

/// A run of text with one look. A line break is a run whose text is `\n`.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub text: String,
    /// The font family, theme fonts resolved (`+mn-lt` is the theme's minor Latin font).
    pub font: String,
    /// Points, before `TextBody::font_scale`.
    pub size: f64,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub color: Color,
    /// Superscript (positive) or subscript (negative) offset, percent of the size.
    pub baseline: f64,
    pub caps: Caps,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Caps {
    #[default]
    None,
    All,
    Small,
}
