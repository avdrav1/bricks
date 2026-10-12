//! Formatting (FRAME-11): the looks the inspector changes, each one undoable step. Text
//! looks (font, size, bold, italic, underline, colour) over a range of a shape's text,
//! paragraph looks (alignment, bullets) over the paragraphs a range touches, a shape's
//! fill and outline, and a slide's background. A look set by an edit is `Source::Edit`,
//! which a save writes on the shape or slide.

use crate::{Change, ShapeRef, TextPos};
use presentation_model::{
    Align, Background, Bullet, BulletSize, Color, Fill, Layer, Outline, Paragraph, Presentation,
    Run, Shape, Source, TextBody,
};

/// A look for text.
#[derive(Debug, Clone, PartialEq)]
pub enum TextLook {
    Font(String),
    /// Points.
    Size(f64),
    Bold(bool),
    Italic(bool),
    Underline(bool),
    Color(Color),
}

impl TextLook {
    fn set(&self, r: &mut Run) {
        let s = &mut r.sources;
        match self {
            TextLook::Font(f) => (r.font, s.font) = (f.clone(), Source::Edit),
            TextLook::Size(v) => (r.size, s.size) = (*v, Source::Edit),
            TextLook::Bold(v) => (r.bold, s.bold) = (*v, Source::Edit),
            TextLook::Italic(v) => (r.italic, s.italic) = (*v, Source::Edit),
            TextLook::Underline(v) => (r.underline, s.underline) = (*v, Source::Edit),
            TextLook::Color(c) => (r.color, s.color) = (*c, Source::Edit),
        }
    }
}

/// A look for paragraphs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParagraphLook {
    Align(Align),
    /// Bullets on (a `•`) or off.
    Bullets(bool),
}

/// The left margin a paragraph gets when bullets go on and it had none: room for the
/// bullet, which hangs in it (PowerPoint's 0.375 inch).
pub const BULLET_MARGIN: i64 = 342_900;

impl ParagraphLook {
    fn set(&self, p: &mut Paragraph) {
        match *self {
            ParagraphLook::Align(a) => (p.align, p.sources.align) = (a, Source::Edit),
            ParagraphLook::Bullets(on) => {
                p.bullet = on.then(|| Bullet {
                    text: "\u{2022}".into(),
                    autonumber: None,
                    font: Some("Arial".into()),
                    size: BulletSize::FollowText,
                    color: None,
                });
                p.sources.bullet = Source::Edit;
                if on && p.margin_left == 0 && p.indent == 0 {
                    p.margin_left = BULLET_MARGIN;
                    p.indent = -BULLET_MARGIN;
                }
            }
        }
    }
}

/// Whether `shape` takes formatting: one of the slide's own shapes a save can write.
pub fn styleable(shape: &Shape) -> bool {
    shape.layer == Layer::Slide && shape.id.is_some()
}

/// A shape's text formatted: its paragraphs before and after.
#[derive(Debug, Clone, PartialEq)]
pub struct LookEdit {
    shape: ShapeRef,
    before: Vec<Paragraph>,
    after: Vec<Paragraph>,
}

fn body(deck: &Presentation, shape: ShapeRef) -> Option<&TextBody> {
    let s = deck.slides.get(shape.slide)?.shapes.get(shape.shape)?;
    styleable(s).then_some(s.text.as_ref()?)
}

/// `p`'s runs split so that byte offsets `from` and `to` of its text fall between runs;
/// gives the indexes of the runs from `from` to `to`.
fn split(p: &mut Paragraph, from: usize, to: usize) -> std::ops::Range<usize> {
    let mut runs = Vec::with_capacity(p.runs.len() + 2);
    let (mut first, mut last) = (None, 0);
    let mut start = 0;
    for r in p.runs.drain(..) {
        let end = start + r.text.len();
        // Cut points inside this run, on its character boundaries; a field stays whole.
        let mut cuts: Vec<usize> = [from, to]
            .into_iter()
            .filter(|&c| c > start && c < end && r.markup.field.is_empty())
            .map(|c| c - start)
            .collect();
        cuts.dedup();
        let mut pieces = Vec::new();
        let mut at = 0;
        for c in cuts {
            pieces.push(r.text[at..c].to_owned());
            at = c;
        }
        pieces.push(r.text[at..].to_owned());
        let whole = pieces.len() == 1;
        let mut piece_start = start;
        for text in pieces {
            let piece_end = piece_start + text.len();
            if piece_start >= from && piece_end <= to && piece_end > piece_start {
                first.get_or_insert(runs.len());
                last = runs.len() + 1;
            }
            if whole {
                runs.push(r.clone());
            } else {
                let mut piece = r.clone();
                piece.text = text;
                runs.push(piece);
            }
            piece_start = piece_end;
        }
        start = end;
    }
    p.runs = runs;
    first.map_or(0..0, |f| f..last)
}

impl LookEdit {
    /// Give text `from..to` (either order) of `shape` the look `look`; a paragraph whose
    /// text is all in range takes it for its mark too, so text typed there has it. `None`
    /// without a range, or when nothing changes.
    pub fn text(
        deck: &Presentation,
        shape: ShapeRef,
        from: TextPos,
        to: TextPos,
        look: &TextLook,
    ) -> Option<LookEdit> {
        let body = body(deck, shape)?;
        let (from, to) = (from.min(to), from.max(to));
        if from == to || to.paragraph >= body.paragraphs.len() {
            return None;
        }
        let mut after = body.paragraphs.clone();
        for (i, p) in after
            .iter_mut()
            .enumerate()
            .take(to.paragraph + 1)
            .skip(from.paragraph)
        {
            let len = p.text().len();
            let a = if i == from.paragraph { from.offset } else { 0 };
            let b = if i == to.paragraph { to.offset } else { len };
            let range = split(p, a, b);
            for r in &mut p.runs[range] {
                look.set(r);
            }
            if a == 0 && b == len {
                look.set(&mut p.end);
            }
            p.merge_runs();
        }
        Self::new(shape, body, after)
    }

    /// Give the paragraphs of `shape` that `from..to` (either order) touches the look
    /// `look`. `None` when nothing changes.
    pub fn paragraphs(
        deck: &Presentation,
        shape: ShapeRef,
        from: TextPos,
        to: TextPos,
        look: ParagraphLook,
    ) -> Option<LookEdit> {
        let body = body(deck, shape)?;
        let (from, to) = (from.min(to), from.max(to));
        let mut after = body.paragraphs.clone();
        for p in after.iter_mut().take(to.paragraph + 1).skip(from.paragraph) {
            look.set(p);
        }
        if look == ParagraphLook::Bullets(false) || look == ParagraphLook::Bullets(true) {
            let mut numbered = TextBody {
                paragraphs: after,
                ..body.clone()
            };
            numbered.number_bullets();
            after = numbered.paragraphs;
        }
        Self::new(shape, body, after)
    }

    fn new(shape: ShapeRef, body: &TextBody, after: Vec<Paragraph>) -> Option<LookEdit> {
        (after != body.paragraphs).then(|| LookEdit {
            shape,
            before: body.paragraphs.clone(),
            after,
        })
    }

    fn set(&self, deck: &mut Presentation, paragraphs: &[Paragraph]) -> Change {
        let s = &mut deck.slides[self.shape.slide].shapes[self.shape.shape];
        if let Some(t) = s.text.as_mut() {
            t.paragraphs = paragraphs.to_vec();
        }
        Change::Look { shape: self.shape }
    }

    pub(crate) fn apply(&self, deck: &mut Presentation) -> Change {
        self.set(deck, &self.after)
    }

    pub(crate) fn revert(&self, deck: &mut Presentation) -> Change {
        self.set(deck, &self.before)
    }
}

/// A look for a shape.
#[derive(Debug, Clone, PartialEq)]
pub enum ShapeLook {
    Fill(Fill),
    /// `None`: no outline.
    Outline(Option<Outline>),
}

/// A shape's fill or outline changed: both before and after, with where they're set.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeLookEdit {
    shape: ShapeRef,
    before: (Fill, Option<Outline>, presentation_model::ShapeSources),
    after: (Fill, Option<Outline>, presentation_model::ShapeSources),
}

impl ShapeLookEdit {
    /// Give `shape` the look `look`; `None` when it can't take it (not the slide's own,
    /// or a table, whose look is its cells') or has it already.
    pub fn new(deck: &Presentation, shape: ShapeRef, look: ShapeLook) -> Option<ShapeLookEdit> {
        let s = deck.slides.get(shape.slide)?.shapes.get(shape.shape)?;
        if !styleable(s) || matches!(s.kind, presentation_model::ShapeKind::Table(_)) {
            return None;
        }
        let before = (s.fill.clone(), s.outline, s.sources);
        let mut after = before.clone();
        match look {
            ShapeLook::Fill(f) if f != before.0 => (after.0, after.2.fill) = (f, Source::Edit),
            ShapeLook::Outline(o) if o != before.1 => {
                (after.1, after.2.outline) = (o, Source::Edit)
            }
            _ => return None,
        }
        Some(ShapeLookEdit {
            shape,
            before,
            after,
        })
    }

    fn set(
        &self,
        deck: &mut Presentation,
        look: &(Fill, Option<Outline>, presentation_model::ShapeSources),
    ) -> Change {
        let s = &mut deck.slides[self.shape.slide].shapes[self.shape.shape];
        (s.fill, s.outline, s.sources) = look.clone();
        Change::Look { shape: self.shape }
    }

    pub(crate) fn apply(&self, deck: &mut Presentation) -> Change {
        self.set(deck, &self.after)
    }

    pub(crate) fn revert(&self, deck: &mut Presentation) -> Change {
        self.set(deck, &self.before)
    }
}

/// A slide's background changed: before and after, with where it's set.
#[derive(Debug, Clone, PartialEq)]
pub struct BackgroundEdit {
    slide: usize,
    before: (Background, Source),
    after: Background,
}

impl BackgroundEdit {
    /// Give slide `slide` the background `to`; `None` when it has it already, set on
    /// itself.
    pub fn new(deck: &Presentation, slide: usize, to: Background) -> Option<BackgroundEdit> {
        let s = deck.slides.get(slide)?;
        let before = (s.background.clone(), s.background_source);
        (to != before.0 || !before.1.is_own()).then_some(BackgroundEdit {
            slide,
            before,
            after: to,
        })
    }

    pub(crate) fn apply(&self, deck: &mut Presentation) -> Change {
        let s = &mut deck.slides[self.slide];
        (s.background, s.background_source) = (self.after.clone(), Source::Edit);
        Change::Slide { slide: self.slide }
    }

    pub(crate) fn revert(&self, deck: &mut Presentation) -> Change {
        let s = &mut deck.slides[self.slide];
        (s.background, s.background_source) = self.before.clone();
        Change::Slide { slide: self.slide }
    }
}
