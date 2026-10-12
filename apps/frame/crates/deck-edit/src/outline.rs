//! The outline (FRAME-13, ADR 0006): a view of the slides, not a second document. Each
//! slide shows as its title (the first paragraph of its title placeholder), its body
//! placeholder's paragraphs as bullets with their levels, and a count of what else it
//! holds, which the outline never changes. Outline edits are the canvas's commands
//! (typing, [`TextEdit`], [`SlideEdit`]) and [`BodyEdit`], so both views share one undo
//! history and one save.

use crate::layout::rebased;
use crate::{next_layout, Change, Edit, ShapeRef, SlideEdit, TextEdit, TextPos};
use presentation_model::{Layer, Paragraph, PlaceholderKind, Presentation, Slide, TextBody};

/// What an outline line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// A slide's title; empty when the slide has no title placeholder or no title.
    Title,
    /// A body paragraph at `level` (0 the first level).
    Bullet { level: u8 },
    /// How many other items the slide holds (shapes outside its title and body): not
    /// editable.
    Others { count: usize },
}

/// One line of the outline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub slide: usize,
    pub kind: LineKind,
    pub text: String,
    /// The body paragraph a bullet is (0 for a title or a count).
    pub paragraph: usize,
}

/// The whole deck as an outline, slide after slide.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Outline {
    pub lines: Vec<Line>,
}

/// A place in the outline: a line and a byte offset into its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct OutlinePos {
    pub line: usize,
    pub offset: usize,
}

impl OutlinePos {
    pub fn new(line: usize, offset: usize) -> OutlinePos {
        OutlinePos { line, offset }
    }
}

/// The keys the outline gives a meaning of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutlineKey {
    /// In a title: a new slide (the title's text after the caret its title); in a bullet:
    /// a new paragraph.
    Enter,
    /// A bullet a level down; a title a first-level bullet of the slide before, its
    /// bullets a level down with it.
    Tab,
    /// A bullet a level up; a first-level bullet the title of a new slide after, with the
    /// bullets after it.
    ShiftTab,
    /// At a line's start: a bullet joins the line before; an empty slide goes.
    Backspace,
    /// At a line's end: the next bullet joins this one.
    Delete,
    /// Ctrl+Enter, anywhere: a new, empty slide after the caret's, on Title and Content,
    /// the caret in its title; no text moves.
    NewSlide,
}

/// The slide's title placeholder, by index.
pub fn title_of(slide: &Slide) -> Option<usize> {
    slide.shapes.iter().position(|s| {
        s.layer == Layer::Slide
            && s.id.is_some()
            && s.text.is_some()
            && matches!(
                s.placeholder,
                Some(PlaceholderKind::Title | PlaceholderKind::CentredTitle)
            )
    })
}

/// The slide's body placeholder (body text or a subtitle), by index.
pub fn body_of(slide: &Slide) -> Option<usize> {
    slide.shapes.iter().position(|s| {
        s.layer == Layer::Slide
            && s.id.is_some()
            && s.text.is_some()
            && matches!(
                s.placeholder,
                Some(PlaceholderKind::Body | PlaceholderKind::Subtitle)
            )
    })
}

/// How many of the slide's own shapes are outside its title and body: everything but
/// empty placeholders and the date, footer, and slide number.
fn others(slide: &Slide, title: Option<usize>, body: Option<usize>) -> usize {
    slide
        .shapes
        .iter()
        .enumerate()
        .filter(|(i, s)| {
            let empty_placeholder =
                s.placeholder.is_some() && s.text.as_ref().is_some_and(TextBody::is_empty);
            let footer = matches!(
                s.placeholder,
                Some(
                    PlaceholderKind::Date | PlaceholderKind::Footer | PlaceholderKind::SlideNumber
                )
            );
            s.layer == Layer::Slide
                && Some(*i) != title
                && Some(*i) != body
                && !empty_placeholder
                && !footer
        })
        .count()
}

/// The deck as an outline.
pub fn outline(deck: &Presentation) -> Outline {
    let mut lines = Vec::new();
    for (n, slide) in deck.slides.iter().enumerate() {
        let title = title_of(slide);
        let body = body_of(slide);
        let title_text = title
            .and_then(|t| slide.shapes[t].text.as_ref())
            .and_then(|t| t.paragraphs.first())
            .map(Paragraph::text)
            .unwrap_or_default();
        lines.push(Line {
            slide: n,
            kind: LineKind::Title,
            text: title_text,
            paragraph: 0,
        });
        if let Some(b) = body.and_then(|b| slide.shapes[b].text.as_ref()) {
            if !b.is_empty() {
                for (i, p) in b.paragraphs.iter().enumerate() {
                    lines.push(Line {
                        slide: n,
                        kind: LineKind::Bullet { level: p.level },
                        text: p.text(),
                        paragraph: i,
                    });
                }
            }
        }
        let count = others(slide, title, body);
        if count > 0 {
            lines.push(Line {
                slide: n,
                kind: LineKind::Others { count },
                text: String::new(),
                paragraph: 0,
            });
        }
    }
    Outline { lines }
}

impl Outline {
    /// The line where slide `slide`'s title is.
    pub fn title_line(&self, slide: usize) -> Option<usize> {
        self.lines
            .iter()
            .position(|l| l.slide == slide && l.kind == LineKind::Title)
    }

    /// The line of slide `slide`'s body paragraph `paragraph`.
    pub fn bullet_line(&self, slide: usize, paragraph: usize) -> Option<usize> {
        self.lines.iter().position(|l| {
            l.slide == slide
                && matches!(l.kind, LineKind::Bullet { .. })
                && l.paragraph == paragraph
        })
    }
}

/// A shape's paragraphs replaced, as one step: what they were and what they are, and where
/// the caret goes in them.
#[derive(Debug, Clone, PartialEq)]
pub struct BodyEdit {
    shape: ShapeRef,
    before: Vec<Paragraph>,
    after: Vec<Paragraph>,
    caret: TextPos,
}

impl BodyEdit {
    /// Give `shape` the paragraphs `after`; `None` when it has them or no text.
    pub fn new(
        deck: &Presentation,
        shape: ShapeRef,
        after: Vec<Paragraph>,
        caret: TextPos,
    ) -> Option<BodyEdit> {
        let body = deck
            .slides
            .get(shape.slide)?
            .shapes
            .get(shape.shape)?
            .text
            .as_ref()?;
        let mut numbered = TextBody {
            paragraphs: after,
            ..body.clone()
        };
        numbered.number_bullets();
        (numbered.paragraphs != body.paragraphs).then(|| BodyEdit {
            shape,
            before: body.paragraphs.clone(),
            after: numbered.paragraphs,
            caret,
        })
    }

    fn set(&self, deck: &mut Presentation, paragraphs: &[Paragraph]) -> Change {
        let s = &mut deck.slides[self.shape.slide].shapes[self.shape.shape];
        if let Some(t) = s.text.as_mut() {
            t.paragraphs = paragraphs.to_vec();
        }
        Change::Text {
            shape: self.shape,
            caret: self.caret,
        }
    }

    pub(crate) fn apply(&self, deck: &mut Presentation) -> Change {
        self.set(deck, &self.after)
    }

    pub(crate) fn revert(&self, deck: &mut Presentation) -> Change {
        self.set(deck, &self.before)
    }
}

/// Where an outline line's text is in the deck: its shape and paragraph.
fn place(deck: &Presentation, line: &Line) -> Option<(ShapeRef, usize)> {
    let slide = deck.slides.get(line.slide)?;
    let shape = match line.kind {
        LineKind::Title => title_of(slide)?,
        LineKind::Bullet { .. } => body_of(slide)?,
        LineKind::Others { .. } => return None,
    };
    Some((
        ShapeRef {
            slide: line.slide,
            shape,
        },
        line.paragraph,
    ))
}

/// Type `text` over `from..to` (either order) of the outline: the caret after it. A range
/// spans one line, or lines of one slide's bullets.
pub fn typing(
    deck: &Presentation,
    outline: &Outline,
    from: OutlinePos,
    to: OutlinePos,
    text: &str,
) -> Option<(Edit, OutlinePos)> {
    let (from, to) = (from.min(to), from.max(to));
    let (shape, a, b) = range(deck, outline, from, to)?;
    let edit = TextEdit::typing(deck, shape, a, b, text)?;
    Some((
        edit.into(),
        OutlinePos::new(from.line, from.offset + text.len()),
    ))
}

/// Delete `from..to` (either order) of the outline; the caret at its start.
pub fn erase(
    deck: &Presentation,
    outline: &Outline,
    from: OutlinePos,
    to: OutlinePos,
) -> Option<(Edit, OutlinePos)> {
    let (from, to) = (from.min(to), from.max(to));
    let (shape, a, b) = range(deck, outline, from, to)?;
    let edit = TextEdit::delete(deck, shape, a, b)?;
    Some((edit.into(), from))
}

/// `from..to` as one shape's text positions, when both are in it.
fn range(
    deck: &Presentation,
    outline: &Outline,
    from: OutlinePos,
    to: OutlinePos,
) -> Option<(ShapeRef, TextPos, TextPos)> {
    let (l1, l2) = (outline.lines.get(from.line)?, outline.lines.get(to.line)?);
    let (s1, p1) = place(deck, l1)?;
    let (s2, p2) = place(deck, l2)?;
    (s1 == s2).then_some((
        s1,
        TextPos::new(p1, from.offset),
        TextPos::new(p2, to.offset),
    ))
}

/// Edits applied one after another to a copy of the deck, each built from the deck as the
/// ones before left it: one step, and the copy after it.
struct Builder {
    deck: Presentation,
    steps: Vec<Edit>,
}

impl Builder {
    fn new(deck: &Presentation) -> Builder {
        Builder {
            deck: deck.clone(),
            steps: Vec::new(),
        }
    }

    fn push(&mut self, edit: Option<impl Into<Edit>>) -> Option<()> {
        let edit = edit?.into();
        edit.apply(&mut self.deck);
        self.steps.push(edit);
        Some(())
    }

    /// Like `push`, but nothing to do is fine.
    fn maybe(&mut self, edit: Option<impl Into<Edit>>) {
        if let Some(e) = edit {
            let e = e.into();
            e.apply(&mut self.deck);
            self.steps.push(e);
        }
    }

    fn done(
        self,
        caret: impl FnOnce(&Presentation, &Outline) -> Option<OutlinePos>,
    ) -> Option<(Edit, OutlinePos)> {
        if self.steps.is_empty() {
            return None;
        }
        let at = caret(&self.deck, &outline(&self.deck))?;
        let edit = if self.steps.len() == 1 {
            self.steps.into_iter().next()?
        } else {
            Edit::Steps(self.steps)
        };
        Some((edit, at))
    }
}

/// The layout slides added from the outline take: Title and Content, else what New Slide
/// would take after `after`.
fn content_layout(deck: &Presentation, after: usize) -> Option<usize> {
    deck.layouts
        .iter()
        .position(|l| l.kind == "obj")
        .or_else(|| next_layout(deck, Some(after)))
}

pub(crate) fn paragraphs(deck: &Presentation, s: ShapeRef) -> Option<&Vec<Paragraph>> {
    Some(
        &deck
            .slides
            .get(s.slide)?
            .shapes
            .get(s.shape)?
            .text
            .as_ref()?
            .paragraphs,
    )
}

/// The looks of `s`'s text per level: its own level looks, else its paragraphs'.
pub(crate) fn levels(deck: &Presentation, s: ShapeRef) -> Vec<Paragraph> {
    deck.slides
        .get(s.slide)
        .and_then(|sl| sl.shapes.get(s.shape))
        .and_then(|sh| sh.text.as_ref())
        .map(|t| {
            if t.levels.is_empty() {
                t.paragraphs.clone()
            } else {
                t.levels.to_vec()
            }
        })
        .unwrap_or_default()
}

/// `p` cut at byte `at`: the text before and after, each with the paragraph's looks.
fn cut(p: &Paragraph, at: usize) -> (Paragraph, Paragraph) {
    let mut before = p.clone();
    let mut after = p.clone();
    before.runs.clear();
    after.runs.clear();
    let mut start = 0;
    for r in &p.runs {
        let end = start + r.text.len();
        if end <= at {
            before.runs.push(r.clone());
        } else if start >= at {
            after.runs.push(r.clone());
        } else {
            before.runs.push(r.with_text(&r.text[..at - start]));
            after.runs.push(r.with_text(&r.text[at - start..]));
        }
        start = end;
    }
    (before, after)
}

/// The slide's own shapes and title: whether it holds nothing but a title and a body.
fn plain(deck: &Presentation, slide: usize) -> bool {
    deck.slides
        .get(slide)
        .is_some_and(|s| others(s, title_of(s), body_of(s)) == 0)
}

/// The outline's meaning of `key` with the caret at `at`: the edit and where the caret
/// goes. `None` when the key does nothing there.
pub fn key(
    deck: &Presentation,
    outline: &Outline,
    at: OutlinePos,
    key: OutlineKey,
) -> Option<(Edit, OutlinePos)> {
    let line = outline.lines.get(at.line)?;
    match (key, line.kind) {
        (OutlineKey::Enter, LineKind::Title) => new_slide(deck, line, at.offset),
        (OutlineKey::Enter, LineKind::Bullet { .. }) => {
            let (shape, p) = place(deck, line)?;
            let pos = TextPos::new(p, at.offset);
            let edit = TextEdit::new_paragraph(deck, shape, pos, pos)?;
            Some((edit.into(), OutlinePos::new(at.line + 1, 0)))
        }
        (OutlineKey::Tab, LineKind::Bullet { level }) if level < 8 => {
            relevel(deck, line, at, level + 1)
        }
        (OutlineKey::ShiftTab, LineKind::Bullet { level }) if level > 0 => {
            relevel(deck, line, at, level - 1)
        }
        (OutlineKey::ShiftTab, LineKind::Bullet { .. }) => promote(deck, line, at.offset),
        (OutlineKey::Tab, LineKind::Title) => demote(deck, line, at.offset),
        (OutlineKey::Backspace, LineKind::Bullet { .. }) if at.offset == 0 => {
            if line.paragraph == 0 {
                return None;
            }
            let (shape, p) = place(deck, line)?;
            let prev = paragraphs(deck, shape)?.get(p - 1)?.text().len();
            let edit =
                TextEdit::delete(deck, shape, TextPos::new(p - 1, prev), TextPos::new(p, 0))?;
            Some((edit.into(), OutlinePos::new(at.line - 1, prev)))
        }
        (OutlineKey::Backspace, LineKind::Title) if at.offset == 0 => {
            drop_slide(deck, outline, line)
        }
        (OutlineKey::Delete, LineKind::Bullet { .. }) if at.offset == line.text.len() => {
            let (shape, p) = place(deck, line)?;
            if p + 1 >= paragraphs(deck, shape)?.len() {
                return None;
            }
            let edit = TextEdit::delete(
                deck,
                shape,
                TextPos::new(p, at.offset),
                TextPos::new(p + 1, 0),
            )?;
            Some((edit.into(), at))
        }
        (OutlineKey::NewSlide, _) => next_slide(deck, line.slide),
        _ => None,
    }
}

/// Ctrl+Enter: an empty slide after slide `slide`, on Title and Content.
fn next_slide(deck: &Presentation, slide: usize) -> Option<(Edit, OutlinePos)> {
    let layout = content_layout(deck, slide)?;
    let mut b = Builder::new(deck);
    b.push(SlideEdit::on_layout(deck, Some(slide), layout))?;
    b.done(|_, o| Some(OutlinePos::new(o.title_line(slide + 1)?, 0)))
}

/// Enter in a title at `offset`: a new slide after (before, at the start of a title with
/// text), on Title and Content, its title the text after the caret.
fn new_slide(deck: &Presentation, line: &Line, offset: usize) -> Option<(Edit, OutlinePos)> {
    let slide = line.slide;
    let layout = content_layout(deck, slide)?;
    let mut b = Builder::new(deck);
    if offset == 0 && !line.text.is_empty() {
        // A new slide at this one's place, before it.
        b.push(SlideEdit::on_layout(deck, slide.checked_sub(1), layout))?;
        return b.done(|_, o| Some(OutlinePos::new(o.title_line(slide + 1)?, 0)));
    }
    let title = title_of(&deck.slides[slide]).map(|t| ShapeRef { slide, shape: t });
    let rest = match title {
        Some(t) => {
            let ps = paragraphs(deck, t)?;
            let (keep, rest) = cut(ps.first()?, offset);
            let mut after = ps.clone();
            after[0] = keep;
            b.maybe(BodyEdit::new(deck, t, after, TextPos::new(0, offset)));
            Some(rest)
        }
        None => None,
    };
    b.push(SlideEdit::on_layout(&b.deck.clone(), Some(slide), layout))?;
    let new = slide + 1;
    if let Some(rest) = rest.filter(|r| !r.text().is_empty()) {
        let t = ShapeRef {
            slide: new,
            shape: title_of(&b.deck.slides[new])?,
        };
        let levels = levels(&b.deck, t);
        let p = rebased(&rest, 0, &levels)?;
        let edit = BodyEdit::new(&b.deck.clone(), t, vec![p], TextPos::new(0, 0));
        b.maybe(edit);
    }
    b.done(|_, o| Some(OutlinePos::new(o.title_line(new)?, 0)))
}

/// A bullet at another level, with that level's looks.
fn relevel(
    deck: &Presentation,
    line: &Line,
    at: OutlinePos,
    level: u8,
) -> Option<(Edit, OutlinePos)> {
    let (shape, p) = place(deck, line)?;
    let levels = levels(deck, shape);
    let mut after = paragraphs(deck, shape)?.clone();
    after[p] = rebased(&after[p], level, &levels)?;
    let edit = BodyEdit::new(deck, shape, after, TextPos::new(p, at.offset))?;
    Some((edit.into(), at))
}

/// Tab on a title: the slide joins the one before, its title a first-level bullet there
/// and its bullets a level down. Only for a slide holding nothing else, after a slide
/// with a body.
fn demote(deck: &Presentation, line: &Line, offset: usize) -> Option<(Edit, OutlinePos)> {
    let slide = line.slide;
    let prev = slide.checked_sub(1)?;
    if !plain(deck, slide) {
        return None;
    }
    let target = ShapeRef {
        slide: prev,
        shape: body_of(&deck.slides[prev])?,
    };
    let levels = levels(deck, target);
    let here = &deck.slides[slide];
    let title = title_of(here)
        .and_then(|t| here.shapes[t].text.as_ref())
        .and_then(|t| t.paragraphs.first().cloned());
    let bullets: Vec<Paragraph> = body_of(here)
        .and_then(|b| here.shapes[b].text.as_ref())
        .filter(|t| !t.is_empty())
        .map(|t| t.paragraphs.clone())
        .unwrap_or_default();
    let mut moved = Vec::new();
    if let Some(t) = &title {
        moved.push(rebased(t, 0, &levels)?);
    }
    for p in &bullets {
        moved.push(rebased(p, (p.level + 1).min(8), &levels)?);
    }
    let old = paragraphs(deck, target)?;
    let target_empty = deck.slides[prev].shapes[target.shape]
        .text
        .as_ref()
        .is_some_and(TextBody::is_empty);
    let mut after = if target_empty {
        Vec::new()
    } else {
        old.clone()
    };
    let first = after.len();
    after.extend(moved);
    if after.is_empty() {
        return None;
    }
    let mut b = Builder::new(deck);
    b.push(BodyEdit::new(
        deck,
        target,
        after,
        TextPos::new(first, offset),
    ))?;
    b.push(SlideEdit::delete(&b.deck.clone(), slide))?;
    b.done(|_, o| Some(OutlinePos::new(o.bullet_line(prev, first)?, offset)))
}

/// Shift+Tab on a first-level bullet: a new slide after, its title the bullet and its
/// body the bullets after it.
fn promote(deck: &Presentation, line: &Line, offset: usize) -> Option<(Edit, OutlinePos)> {
    let slide = line.slide;
    let (shape, p) = place(deck, line)?;
    let all = paragraphs(deck, shape)?.clone();
    let layout = content_layout(deck, slide)?;
    let mut b = Builder::new(deck);
    // The bullets from this one on leave the slide.
    let mut keep: Vec<Paragraph> = all[..p].to_vec();
    if keep.is_empty() {
        // A body keeps one empty paragraph, with the first one's looks.
        let mut empty = all[0].clone();
        empty.runs.clear();
        keep.push(empty);
    }
    let caret = TextPos::new(p.saturating_sub(1), 0);
    b.push(BodyEdit::new(deck, shape, keep, caret))?;
    b.push(SlideEdit::on_layout(&b.deck.clone(), Some(slide), layout))?;
    let new = slide + 1;
    let s = &b.deck.slides[new];
    let title = ShapeRef {
        slide: new,
        shape: title_of(s)?,
    };
    let title_levels = levels(&b.deck, title);
    let t = rebased(&all[p], 0, &title_levels)?;
    let edit = BodyEdit::new(&b.deck.clone(), title, vec![t], TextPos::new(0, offset));
    b.maybe(edit);
    if all.len() > p + 1 {
        let body = ShapeRef {
            slide: new,
            shape: body_of(&b.deck.slides[new])?,
        };
        let body_levels = levels(&b.deck, body);
        let rest: Option<Vec<Paragraph>> = all[p + 1..]
            .iter()
            .map(|q| rebased(q, q.level, &body_levels))
            .collect();
        let edit = BodyEdit::new(&b.deck.clone(), body, rest?, TextPos::new(0, 0));
        b.maybe(edit);
    }
    b.done(|_, o| Some(OutlinePos::new(o.title_line(new)?, offset)))
}

/// Backspace at the start of an empty title: the slide goes, when it holds nothing and
/// isn't the only one; the caret at the end of the line before.
fn drop_slide(deck: &Presentation, outline: &Outline, line: &Line) -> Option<(Edit, OutlinePos)> {
    let slide = line.slide;
    let empty = outline
        .lines
        .iter()
        .filter(|l| l.slide == slide)
        .all(|l| l.kind == LineKind::Title && l.text.is_empty());
    if !empty || deck.slides.len() < 2 || slide == 0 {
        return None;
    }
    let mut b = Builder::new(deck);
    b.push(SlideEdit::delete(deck, slide))?;
    b.done(|_, o| {
        let last = o
            .lines
            .iter()
            .rposition(|l| l.slide == slide - 1 && !matches!(l.kind, LineKind::Others { .. }))?;
        Some(OutlinePos::new(last, o.lines[last].text.len()))
    })
}
