//! Inserting (FRAME-12): text boxes, shapes, pictures, and tables added to a slide, a
//! picture filling an empty placeholder, a shape deleted ([`ShapeEdit`]); and text
//! shrunk to fit its box ([`FitEdit`]). Each is one undoable step.

use crate::{Change, ShapeRef};
use presentation_model::{
    Align, Anchor, Autofit, Borders, Color, Crop, Dash, Fill, Frame, Image, Insets, Layer, Outline,
    Paragraph, Picture, PlaceholderKind, Presentation, Run, Shape, ShapeKind, ShapeSources, Slide,
    Source, Spacing, Table, TableCell, TableRow, TextBody, Vertical,
};
use std::sync::Arc;

/// EMU per pixel at 96 pixels per inch: a picture's size as PowerPoint inserts it.
pub const EMU_PER_PIXEL: i64 = 9_525;
/// EMU per point.
const EMU_PER_POINT: f64 = 12_700.0;

/// The fill, outline, and text colour of shapes and table headers Frame inserts:
/// Office's first accent colour, a darker shade of it, and white.
pub const ACCENT: Color = Color::rgb(0x44, 0x72, 0xC4);
pub const ACCENT_DARK: Color = Color::rgb(0x2F, 0x52, 0x8F);
/// Table body rows, banded.
pub const BAND_1: Color = Color::rgb(0xCF, 0xD5, 0xEA);
pub const BAND_2: Color = Color::rgb(0xE9, 0xEB, 0xF5);

/// Shapes added to, taken from, or swapped on a slide.
#[derive(Debug, Clone, PartialEq)]
pub enum ShapeEdit {
    /// `shape` put at index `at` of the slide's shapes (in front of everything when it's
    /// the end).
    Insert {
        slide: usize,
        at: usize,
        shape: Box<Shape>,
    },
    /// The shape at `at` taken away.
    Remove {
        slide: usize,
        at: usize,
        shape: Box<Shape>,
    },
    /// The shape `at` became `after` (a placeholder filled with a picture).
    Replace {
        at: ShapeRef,
        before: Box<Shape>,
        after: Box<Shape>,
    },
}

/// A placeholder a picture can fill: a picture or content placeholder (`pic`, or `obj`
/// which takes any content) of the slide's own, with no text.
pub fn takes_picture(shape: &Shape) -> bool {
    let empty = shape.text.as_ref().is_none_or(TextBody::is_empty);
    let slot = shape.slot.as_ref().map(|s| &*s.kind);
    shape.layer == Layer::Slide
        && shape.id.is_some()
        && empty
        && matches!(shape.kind, ShapeKind::Geometry { .. })
        && matches!(slot, Some("pic" | "obj"))
}

fn centred(deck: &Presentation, cx: i64, cy: i64) -> Frame {
    Frame {
        x: (deck.size.cx - cx) / 2,
        y: (deck.size.cy - cy) / 2,
        cx,
        cy,
        ..Frame::default()
    }
}

/// The text body a text box gets on slide `slide`: its layout's text box look, else
/// PowerPoint's defaults (18 pt Calibri, black).
fn text_box_body(deck: &Presentation, slide: &Slide) -> TextBody {
    slide
        .layout
        .and_then(|l| deck.layouts.get(l))
        .and_then(|l| l.text_box.clone())
        .unwrap_or_else(default_body)
}

fn default_body() -> TextBody {
    let end = Run {
        text: String::new(),
        font: "Calibri".into(),
        size: 18.0,
        bold: false,
        italic: false,
        underline: false,
        strike: false,
        color: Color::BLACK,
        baseline: 0.0,
        caps: Default::default(),
        sources: Default::default(),
        markup: Default::default(),
        slide_number: false,
    };
    let p = Paragraph {
        level: 0,
        align: Align::Left,
        margin_left: 0,
        indent: 0,
        line_spacing: Spacing::Lines(1.0),
        space_before: Spacing::Points(0.0),
        space_after: Spacing::Points(0.0),
        bullet: None,
        runs: Vec::new(),
        end,
        markup: Arc::from(""),
        sources: Default::default(),
    };
    TextBody {
        paragraphs: vec![p.clone()],
        anchor: Anchor::Top,
        anchor_centre: false,
        insets: Insets {
            left: 91_440,
            top: 45_720,
            right: 91_440,
            bottom: 45_720,
        },
        wrap: true,
        font_scale: 1.0,
        line_reduction: 0.0,
        vertical: Vertical::Horizontal,
        autofit: Autofit::Unset,
        levels: Arc::from(vec![p; 9]),
    }
}

/// `body` with one paragraph holding `text` in its mark's look.
fn with_text(mut body: TextBody, text: &str) -> TextBody {
    let mut p = body
        .paragraphs
        .first()
        .cloned()
        .unwrap_or_else(|| default_body().paragraphs[0].clone());
    p.runs = if text.is_empty() {
        Vec::new()
    } else {
        vec![p.end.with_text(text)]
    };
    p.markup = Arc::from("");
    body.paragraphs = vec![p];
    body
}

/// A shape of the slide's own, not a placeholder, with `id`.
fn own_shape(id: u32, name: String, frame: Frame, kind: ShapeKind) -> Shape {
    Shape {
        name,
        frame,
        kind,
        fill: Fill::None,
        outline: None,
        sources: ShapeSources {
            fill: Source::Edit,
            outline: Source::Edit,
        },
        placeholder: None,
        slot: None,
        layer: Layer::Slide,
        text: None,
        text_frame: None,
        id: Some(id),
        grouped: false,
    }
}

/// Every look of `r` set by an edit, so a save writes them.
fn edited(r: &mut Run) {
    let s = &mut r.sources;
    for source in [
        &mut s.font,
        &mut s.size,
        &mut s.bold,
        &mut s.italic,
        &mut s.underline,
        &mut s.strike,
        &mut s.color,
        &mut s.baseline,
        &mut s.caps,
    ] {
        *source = Source::Edit;
    }
}

/// The preset shapes Insert offers, by preset name, with what the menu calls them.
pub const SHAPES: [(&str, &str); 5] = [
    ("rect", "Rectangle"),
    ("roundRect", "Rounded rectangle"),
    ("ellipse", "Oval"),
    ("triangle", "Triangle"),
    ("line", "Line"),
];

impl ShapeEdit {
    fn insert(deck: &Presentation, slide: usize, shape: Shape) -> Option<ShapeEdit> {
        let at = deck.slides.get(slide)?.shapes.len();
        Some(ShapeEdit::Insert {
            slide,
            at,
            shape: Box::new(shape),
        })
    }

    fn new_id(deck: &Presentation, slide: usize) -> Option<u32> {
        let s = deck.slides.get(slide)?;
        let max = s.shapes.iter().filter_map(|s| s.id).max().unwrap_or(1);
        Some(s.next_id.max(max + 1))
    }

    /// A text box holding `text`, its text all one line in the layout's text box look,
    /// centred on slide `slide`, 40% of the slide wide; it grows to fit its text.
    pub fn text_box(deck: &Presentation, slide: usize, text: &str) -> Option<ShapeEdit> {
        let s = deck.slides.get(slide)?;
        let id = Self::new_id(deck, slide)?;
        let mut body = with_text(text_box_body(deck, s), text);
        body.autofit = Autofit::Grow;
        body.wrap = true;
        let size = body.paragraphs[0].end.size;
        let line = (size * 1.2 * EMU_PER_POINT) as i64;
        let cy = line + body.insets.top + body.insets.bottom;
        let frame = centred(deck, deck.size.cx * 2 / 5, cy);
        let mut shape = own_shape(
            id,
            format!("TextBox {}", id - 1),
            frame,
            ShapeKind::Geometry {
                preset: "rect".into(),
                adjust: None,
            },
        );
        shape.text = Some(body);
        Self::insert(deck, slide, shape)
    }

    /// A preset shape (one of [`SHAPES`]) centred on slide `slide`, a quarter of the
    /// slide's height square, filled with [`ACCENT`] and outlined with [`ACCENT_DARK`];
    /// its text, white and centred, starts empty. A line has no fill or text.
    pub fn shape(deck: &Presentation, slide: usize, preset: &str) -> Option<ShapeEdit> {
        let (_, name) = SHAPES.iter().find(|(p, _)| *p == preset)?;
        let s = deck.slides.get(slide)?;
        let id = Self::new_id(deck, slide)?;
        let side = deck.size.cy / 4;
        let line = preset == "line";
        let frame = centred(deck, side, if line { 0 } else { side });
        let mut shape = own_shape(
            id,
            format!("{name} {}", id - 1),
            frame,
            ShapeKind::Geometry {
                preset: preset.into(),
                adjust: None,
            },
        );
        shape.outline = Some(Outline {
            width: 12_700,
            color: if line { ACCENT } else { ACCENT_DARK },
            dash: Dash::Solid,
        });
        if !line {
            shape.fill = Fill::Solid(ACCENT);
            let mut body = with_text(text_box_body(deck, s), "");
            body.anchor = Anchor::Middle;
            body.autofit = Autofit::Off;
            for p in &mut body.paragraphs {
                (p.align, p.sources.align) = (Align::Centre, Source::Edit);
                p.end.color = Color::WHITE;
                edited(&mut p.end);
            }
            shape.text = Some(body);
        }
        Self::insert(deck, slide, shape)
    }

    /// A table of `rows` by `columns` centred on slide `slide`, 80% of the slide wide:
    /// a header row filled with [`ACCENT`] in white bold text, body rows banded, white
    /// borders.
    pub fn table(
        deck: &Presentation,
        slide: usize,
        rows: usize,
        columns: usize,
    ) -> Option<ShapeEdit> {
        if rows == 0 || columns == 0 {
            return None;
        }
        let s = deck.slides.get(slide)?;
        let id = Self::new_id(deck, slide)?;
        let width = deck.size.cx * 4 / 5;
        let column = width / columns as i64;
        let row_height = 370_840;
        let border = Some(Outline {
            width: 12_700,
            color: Color::WHITE,
            dash: Dash::Solid,
        });
        let base = with_text(text_box_body(deck, s), "");
        let rows: Vec<TableRow> = (0..rows)
            .map(|r| TableRow {
                height: row_height,
                cells: (0..columns)
                    .map(|_| {
                        let mut text = base.clone();
                        text.autofit = Autofit::Unset;
                        for p in &mut text.paragraphs {
                            if r == 0 {
                                (p.end.color, p.end.bold) = (Color::WHITE, true);
                            }
                            edited(&mut p.end);
                        }
                        TableCell {
                            text,
                            fill: Some(match r {
                                0 => ACCENT,
                                r if r % 2 == 1 => BAND_1,
                                _ => BAND_2,
                            }),
                            borders: Borders {
                                top: border,
                                bottom: border,
                                left: border,
                                right: border,
                            },
                            col_span: 1,
                            row_span: 1,
                            merged: false,
                        }
                    })
                    .collect(),
            })
            .collect();
        let frame = centred(
            deck,
            column * columns as i64,
            row_height * rows.len() as i64,
        );
        let table = Table {
            columns: vec![column; columns],
            rows,
        };
        let shape = own_shape(
            id,
            format!("Table {}", id - 1),
            frame,
            ShapeKind::Table(table),
        );
        Self::insert(deck, slide, shape)
    }

    /// Picture `image`, `pixels` wide and high, on slide `slide`: it fills the
    /// placeholder `selected` when that takes a picture, else the first that does
    /// ([`takes_picture`]), cropped to the placeholder's shape; with none, it lands
    /// centred at its own size (96 pixels an inch), scaled down to fit the slide.
    pub fn picture(
        deck: &Presentation,
        slide: usize,
        image: Image,
        pixels: (u32, u32),
        selected: Option<usize>,
    ) -> Option<ShapeEdit> {
        let s = deck.slides.get(slide)?;
        let (w, h) = (pixels.0.max(1) as i64, pixels.1.max(1) as i64);
        let target = selected
            .filter(|&i| s.shapes.get(i).is_some_and(takes_picture))
            .or_else(|| s.shapes.iter().position(takes_picture));
        if let Some(at) = target {
            let before = s.shapes[at].clone();
            let f = before.frame;
            // Crop the picture's longer side so it has the placeholder's shape.
            let (pw, ph) = (f.cx.max(1) as f64, f.cy.max(1) as f64);
            let (iw, ih) = (w as f64, h as f64);
            let mut crop = Crop::default();
            if iw / ih > pw / ph {
                let keep = ih * pw / ph / iw;
                crop.left = (1.0 - keep) / 2.0;
                crop.right = crop.left;
            } else {
                let keep = iw * ph / pw / ih;
                crop.top = (1.0 - keep) / 2.0;
                crop.bottom = crop.top;
            }
            let mut after = before.clone();
            after.kind = ShapeKind::Picture(Picture { image, crop });
            after.text = None;
            after.name = format!("Picture {}", before.id.unwrap_or(2) - 1);
            return Some(ShapeEdit::Replace {
                at: ShapeRef { slide, shape: at },
                before: Box::new(before),
                after: Box::new(after),
            });
        }
        let id = Self::new_id(deck, slide)?;
        let (cx, cy) = (w * EMU_PER_PIXEL, h * EMU_PER_PIXEL);
        let scale = 1f64
            .min(deck.size.cx as f64 / cx as f64)
            .min(deck.size.cy as f64 / cy as f64);
        let frame = centred(
            deck,
            (cx as f64 * scale).round() as i64,
            (cy as f64 * scale).round() as i64,
        );
        let mut shape = own_shape(
            id,
            format!("Picture {}", id - 1),
            frame,
            ShapeKind::Picture(Picture {
                image,
                crop: Crop::default(),
            }),
        );
        shape.sources = ShapeSources::default();
        Self::insert(deck, slide, shape)
    }

    /// Delete shape `at`, one of the slide's own; `None` for shapes from the layout or
    /// master, or inside a group.
    pub fn remove(deck: &Presentation, at: ShapeRef) -> Option<ShapeEdit> {
        let shape = deck.slides.get(at.slide)?.shapes.get(at.shape)?;
        (shape.layer == Layer::Slide && shape.id.is_some() && !shape.grouped).then(|| {
            ShapeEdit::Remove {
                slide: at.slide,
                at: at.shape,
                shape: Box::new(shape.clone()),
            }
        })
    }

    /// Where the shape added or swapped is; `None` for a removal.
    pub fn target(&self) -> Option<ShapeRef> {
        match self {
            ShapeEdit::Insert { slide, at, .. } => Some(ShapeRef {
                slide: *slide,
                shape: *at,
            }),
            ShapeEdit::Replace { at, .. } => Some(*at),
            ShapeEdit::Remove { .. } => None,
        }
    }

    pub(crate) fn apply(&self, deck: &mut Presentation) -> Change {
        match self {
            ShapeEdit::Insert { slide, at, shape } => {
                deck.slides[*slide].shapes.insert(*at, (**shape).clone());
                Change::Shape {
                    shape: ShapeRef {
                        slide: *slide,
                        shape: *at,
                    },
                }
            }
            ShapeEdit::Remove { slide, at, .. } => {
                deck.slides[*slide].shapes.remove(*at);
                Change::Slide { slide: *slide }
            }
            ShapeEdit::Replace { at, after, .. } => {
                deck.slides[at.slide].shapes[at.shape] = (**after).clone();
                Change::Shape { shape: *at }
            }
        }
    }

    pub(crate) fn revert(&self, deck: &mut Presentation) -> Change {
        match self {
            ShapeEdit::Insert { slide, at, .. } => {
                deck.slides[*slide].shapes.remove(*at);
                Change::Slide { slide: *slide }
            }
            ShapeEdit::Remove { slide, at, shape } => {
                deck.slides[*slide].shapes.insert(*at, (**shape).clone());
                Change::Shape {
                    shape: ShapeRef {
                        slide: *slide,
                        shape: *at,
                    },
                }
            }
            ShapeEdit::Replace { at, before, .. } => {
                deck.slides[at.slide].shapes[at.shape] = (**before).clone();
                Change::Shape { shape: *at }
            }
        }
    }
}

/// How a text body shrinks to fit its box: its font scale and line spacing reduction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    pub font_scale: f64,
    pub line_reduction: f64,
}

/// The fits to try, largest first, as PowerPoint steps them: the scale down by 7.5%
/// at a time, lines 10% closer from the first step and 20% from 70%; 25% at the least.
pub const FITS: [Fit; 11] = [
    Fit {
        font_scale: 1.0,
        line_reduction: 0.0,
    },
    Fit {
        font_scale: 0.925,
        line_reduction: 0.1,
    },
    Fit {
        font_scale: 0.85,
        line_reduction: 0.1,
    },
    Fit {
        font_scale: 0.775,
        line_reduction: 0.1,
    },
    Fit {
        font_scale: 0.7,
        line_reduction: 0.2,
    },
    Fit {
        font_scale: 0.625,
        line_reduction: 0.2,
    },
    Fit {
        font_scale: 0.55,
        line_reduction: 0.2,
    },
    Fit {
        font_scale: 0.475,
        line_reduction: 0.2,
    },
    Fit {
        font_scale: 0.4,
        line_reduction: 0.2,
    },
    Fit {
        font_scale: 0.325,
        line_reduction: 0.2,
    },
    Fit {
        font_scale: 0.25,
        line_reduction: 0.2,
    },
];

/// Whether `shape`'s text shrinks when it overflows: its body says `normAutofit`, or
/// it's a placeholder whose body says nothing.
pub fn shrinks(shape: &Shape) -> bool {
    match shape.text.as_ref().map(|t| t.autofit) {
        Some(Autofit::Shrink) => true,
        Some(Autofit::Unset) => shape.placeholder.is_some_and(|k| {
            !matches!(
                k,
                PlaceholderKind::Date | PlaceholderKind::Footer | PlaceholderKind::SlideNumber
            )
        }),
        _ => false,
    }
}

/// A shape's text shrunk (or grown back) to fit: its fit before and after.
#[derive(Debug, Clone, PartialEq)]
pub struct FitEdit {
    shape: ShapeRef,
    before: (Fit, Autofit),
    after: Fit,
}

impl FitEdit {
    /// Give `shape`'s text the fit `fit`, and the autofit `Shrink`, which a save writes as
    /// `normAutofit`; `None` when it has it.
    pub fn new(deck: &Presentation, shape: ShapeRef, fit: Fit) -> Option<FitEdit> {
        let s = deck.slides.get(shape.slide)?.shapes.get(shape.shape)?;
        let t = s.text.as_ref()?;
        let before = Fit {
            font_scale: t.font_scale,
            line_reduction: t.line_reduction,
        };
        (crate::styleable(s) && (before != fit || t.autofit != Autofit::Shrink)).then_some(
            FitEdit {
                shape,
                before: (before, t.autofit),
                after: fit,
            },
        )
    }

    fn set(&self, deck: &mut Presentation, fit: Fit, autofit: Autofit) -> Change {
        let s = &mut deck.slides[self.shape.slide].shapes[self.shape.shape];
        if let Some(t) = s.text.as_mut() {
            (t.font_scale, t.line_reduction, t.autofit) =
                (fit.font_scale, fit.line_reduction, autofit);
        }
        Change::Look { shape: self.shape }
    }

    pub(crate) fn apply(&self, deck: &mut Presentation) -> Change {
        self.set(deck, self.after, Autofit::Shrink)
    }

    pub(crate) fn revert(&self, deck: &mut Presentation) -> Change {
        self.set(deck, self.before.0, self.before.1)
    }
}
