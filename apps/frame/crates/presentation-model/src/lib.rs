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

/// A whole deck: the slide size, the slides in show order, and the slide layouts new
/// slides are made from.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Presentation {
    pub size: Size,
    pub slides: Vec<Slide>,
    /// The deck's slide layouts, master by master, in the file's order.
    pub layouts: Vec<Layout>,
}

impl Presentation {
    /// Set every slide-number field ([`Run::slide_number`]) to its slide's number, as
    /// slides are added, deleted, and moved.
    pub fn number_slides(&mut self) {
        for (i, slide) in self.slides.iter_mut().enumerate() {
            let number = (i + 1).to_string();
            for shape in &mut slide.shapes {
                let cells = match &mut shape.kind {
                    ShapeKind::Table(t) => t.rows.iter_mut().flat_map(|r| &mut r.cells).collect(),
                    _ => Vec::new(),
                };
                let bodies = shape
                    .text
                    .iter_mut()
                    .chain(cells.into_iter().map(|c| &mut c.text));
                for run in bodies
                    .flat_map(|b| &mut b.paragraphs)
                    .flat_map(|p| &mut p.runs)
                    .filter(|r| r.slide_number)
                {
                    run.text.clone_from(&number);
                }
            }
        }
    }
}

/// A slide layout (Title Slide, Title and Content, ...).
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// Its name, as PowerPoint's New Slide menu shows it (`cSld name`).
    pub name: String,
    /// Its type (`sldLayout type`): `title`, `obj` (Title and Content), `twoObj`,
    /// `titleOnly`, `blank`, `secHead`, ..., `cust` for a custom one.
    pub kind: String,
    /// The layout's part in the package (`ppt/slideLayouts/slideLayout2.xml`).
    pub part: String,
    /// The master it belongs to, by position in the deck's list of masters.
    pub master: usize,
    /// A new slide on this layout, as PowerPoint makes one: the layout's placeholders
    /// (but date, footer, and slide number), empty, and what the layout and its master
    /// show.
    pub slide: Slide,
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
    /// Its layout, by index into [`Presentation::layouts`]; `None` when the deck doesn't
    /// list the layout the slide names.
    pub layout: Option<usize>,
    /// The part it was read from, or copied from for a duplicate; `None` for a slide made
    /// from a layout.
    pub part: Option<String>,
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
    /// Where the text lays out when that isn't the shape's own box: a SmartArt drawing
    /// part's `txXfrm` (the top of a circle, ...), group transforms applied.
    pub text_frame: Option<Frame>,
    /// The shape's id in its slide part (`cNvPr id`), for the slide's own shapes: how a
    /// save finds the shape to write its edits into. `None` for shapes a save can't
    /// write (from layouts, masters, and SmartArt drawings).
    pub id: Option<u32>,
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

    /// Number the numbered paragraphs, as PowerPoint counts: per level, from the
    /// scheme's start; a paragraph at a level without a number, or with another scheme,
    /// restarts it, as does any paragraph above it. Paragraphs without text don't count
    /// and don't break the count.
    pub fn number_bullets(&mut self) {
        let mut counters: [Option<(String, u32)>; 9] = Default::default();
        for p in &mut self.paragraphs {
            if !p.has_text() {
                continue;
            }
            let level = usize::from(p.level.min(8));
            for c in counters.iter_mut().skip(level + 1) {
                *c = None;
            }
            match p.bullet.as_mut() {
                Some(Bullet {
                    autonumber: Some(a),
                    text,
                    ..
                }) => {
                    let n = match &counters[level] {
                        Some((s, n)) if *s == a.scheme => *n,
                        _ => a.start,
                    };
                    counters[level] = Some((a.scheme.clone(), n + 1));
                    *text = a.format(n);
                }
                _ => counters[level] = None,
            }
        }
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
    /// The paragraph's bullet or number; it shows only while the paragraph has text
    /// ([`Paragraph::shown_bullet`]).
    pub bullet: Option<Bullet>,
    pub runs: Vec<Run>,
    /// The look of the paragraph mark (`endParaRPr`): an empty paragraph's line, and text
    /// typed into it. Its text is empty.
    pub end: Run,
    /// The paragraph's properties element (`<a:pPr …>…</a:pPr>`) as read, for a save to
    /// write back; empty when it has none. Opaque to everything but the file format.
    pub markup: Arc<str>,
}

impl Paragraph {
    /// The paragraph's text; line breaks are `\n`.
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }

    /// Whether any run has text other than spaces and line breaks.
    pub fn has_text(&self) -> bool {
        self.runs.iter().any(|r| !r.text.trim().is_empty())
    }

    /// The run whose look the paragraph's line and bullet follow: the first with text,
    /// else the paragraph mark.
    pub fn first_run(&self) -> &Run {
        self.runs
            .iter()
            .find(|r| !r.text.is_empty() && r.text != "\n")
            .unwrap_or(&self.end)
    }

    /// The bullet as it shows: none while the paragraph has no text; font, size, and
    /// colour that follow the text taken from its first run.
    pub fn shown_bullet(&self) -> Option<BulletLook<'_>> {
        let b = self.bullet.as_ref().filter(|_| self.has_text())?;
        let first = self.first_run();
        Some(BulletLook {
            text: &b.text,
            font: b.font.as_deref().unwrap_or(&first.font),
            size: match b.size {
                BulletSize::FollowText => first.size,
                BulletSize::Percent(p) => first.size * p,
                BulletSize::Points(p) => p,
            },
            color: b.color.unwrap_or(first.color),
        })
    }

    /// Merge neighbouring runs that differ only in their text (line breaks stay runs of
    /// their own) and drop empty runs, so one look over a stretch of text is one run. Runs
    /// the file writes differently (`Run::markup`) stay apart.
    pub fn merge_runs(&mut self) {
        let mut merged: Vec<Run> = Vec::with_capacity(self.runs.len());
        for run in self.runs.drain(..) {
            if run.text.is_empty() {
                continue;
            }
            match merged.last_mut() {
                Some(last)
                    if last.text != "\n"
                        && run.text != "\n"
                        && last.same_look(&run)
                        && last.markup == run.markup =>
                {
                    last.text.push_str(&run.text)
                }
                _ => merged.push(run),
            }
        }
        self.runs = merged;
    }
}

/// A paragraph's bullet character or number.
#[derive(Debug, Clone, PartialEq)]
pub struct Bullet {
    /// What shows: `•`, `–`, `3.`, `iv)`, ... A numbered paragraph's is its number
    /// ([`TextBody::number_bullets`]).
    pub text: String,
    /// How a numbered paragraph counts; `None` for a bullet character.
    pub autonumber: Option<AutoNumber>,
    /// The bullet's font; `None` for the text's.
    pub font: Option<String>,
    pub size: BulletSize,
    /// `None` for the text's colour.
    pub color: Option<Color>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BulletSize {
    /// The size of the text's first run.
    FollowText,
    /// A fraction of the text's size.
    Percent(f64),
    Points(f64),
}

/// A bullet as it shows, with what it takes from the text resolved.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BulletLook<'a> {
    pub text: &'a str,
    pub font: &'a str,
    /// Points.
    pub size: f64,
    pub color: Color,
}

/// An automatic number's scheme (`buAutoNum type`: `arabicPeriod`, `romanLcParenR`, ...)
/// and the number it starts at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoNumber {
    pub scheme: String,
    pub start: u32,
}

impl AutoNumber {
    /// Number `n` in this scheme: `arabicPeriod` 3 is `3.`, `romanLcParenR` 4 is `iv)`,
    /// `alphaUcParenBoth` 2 is `(B)`.
    pub fn format(&self, n: u32) -> String {
        let scheme = self.scheme.as_str();
        let (digits, rest) = if let Some(r) = scheme.strip_prefix("arabic") {
            (n.to_string(), r)
        } else if let Some(r) = scheme.strip_prefix("romanUc") {
            (roman(n), r)
        } else if let Some(r) = scheme.strip_prefix("romanLc") {
            (roman(n).to_lowercase(), r)
        } else if let Some(r) = scheme.strip_prefix("alphaUc") {
            (alpha(n), r)
        } else if let Some(r) = scheme.strip_prefix("alphaLc") {
            (alpha(n).to_lowercase(), r)
        } else {
            (n.to_string(), "Period")
        };
        match rest {
            "ParenR" => format!("{digits})"),
            "ParenBoth" => format!("({digits})"),
            "Plain" => digits,
            "Minus" => format!("- {digits} -"),
            _ => format!("{digits}."),
        }
    }
}

fn roman(mut n: u32) -> String {
    const TABLE: [(u32, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for (v, s) in TABLE {
        while n >= v {
            out.push_str(s);
            n -= v;
        }
    }
    out
}

/// A, B, ..., Z, AA, BB, ... as PowerPoint counts letters.
fn alpha(n: u32) -> String {
    let n = n.max(1) - 1;
    let letter = char::from(b'A' + (n % 26) as u8);
    std::iter::repeat_n(letter, (n / 26 + 1) as usize).collect()
}

/// Where a resolved value is set (ADR 0006): on the shape itself, or inherited from its
/// layout's or master's placeholder or text styles, or from the presentation (its
/// default text style, a table style, PowerPoint's defaults).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Source {
    Own,
    Layout,
    Master,
    #[default]
    Presentation,
}

/// Where each of a run's looks is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RunSources {
    pub font: Source,
    pub size: Source,
    pub bold: Source,
    pub italic: Source,
    pub underline: Source,
    pub strike: Source,
    pub color: Source,
    pub baseline: Source,
    pub caps: Source,
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
    /// Where each look is set: an edit keeps inherited looks inherited.
    pub sources: RunSources,
    /// How the file writes the run, beyond its look, for a save to write it back.
    pub markup: RunMarkup,
    /// A slide-number field: its text is its slide's number.
    pub slide_number: bool,
}

/// What the file says of a run beyond the model: opaque to everything but the file
/// format, compared only to tell runs apart.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunMarkup {
    /// A field's start tag (`<a:fld id="…" type="slidenum">`); empty for text and breaks.
    pub field: Arc<str>,
    /// The run's properties element as read (`<a:rPr …>…</a:rPr>`, or a paragraph
    /// mark's `<a:endParaRPr …/>`); empty when it has none.
    pub props: Arc<str>,
}

impl Run {
    /// Whether `other` looks the same and takes its looks from the same places.
    pub fn same_look(&self, other: &Run) -> bool {
        self.font == other.font
            && self.size == other.size
            && self.bold == other.bold
            && self.italic == other.italic
            && self.underline == other.underline
            && self.strike == other.strike
            && self.color == other.color
            && self.baseline == other.baseline
            && self.caps == other.caps
            && self.sources == other.sources
    }

    /// This run's look with `text`: plain text, not a field, whatever this run is.
    pub fn with_text(&self, text: impl Into<String>) -> Run {
        Run {
            text: text.into(),
            font: self.font.clone(),
            markup: RunMarkup {
                field: Arc::from(""),
                props: self.markup.props.clone(),
            },
            slide_number: false,
            ..*self
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Caps {
    #[default]
    None,
    All,
    Small,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autonumbers() {
        let n = |scheme: &str, n| {
            AutoNumber {
                scheme: scheme.into(),
                start: 1,
            }
            .format(n)
        };
        assert_eq!(n("arabicPeriod", 3), "3.");
        assert_eq!(n("romanLcParenR", 4), "iv)");
        assert_eq!(n("alphaUcParenBoth", 2), "(B)");
        assert_eq!(n("alphaLcPeriod", 27), "aa.");
        assert_eq!(n("romanUcPeriod", 1994), "MCMXCIV.");
        assert_eq!(n("arabicPlain", 7), "7");
    }
}
