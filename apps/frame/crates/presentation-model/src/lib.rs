//! Frame's presentation model (ADR 0004): what a deck shows, with every value resolved.
//!
//! A reader (the `pptx` crate) walks slide → layout → master → theme once and stores final
//! values here, so the renderer and the tests never follow inheritance. Lengths are EMU
//! (English Metric Units: 914,400 per inch, 12,700 per point), as in the file.
//!
//! Toolkit-free: no GTK, no I/O.

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
    /// A shape drawn from geometry: `prst` is the preset name (`rect`, `ellipse`, ...),
    /// `custom` for freeform geometry.
    Geometry { preset: String },
    /// A picture (`<p:pic>`).
    Picture,
    /// A table (`<a:tbl>` in a graphic frame).
    Table,
    /// Content Frame doesn't draw (`Chart`, `SmartArt`, `Video`, `Picture (EMF)`, ...):
    /// shown as a labelled outline so nothing silently disappears.
    Unsupported { label: String },
}

/// One shape on a slide.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    /// `cNvPr name`.
    pub name: String,
    pub frame: Frame,
    pub kind: ShapeKind,
    /// The placeholder role, for placeholders.
    pub placeholder: Option<PlaceholderKind>,
    pub layer: Layer,
}
