//! Text edits: typing, deleting, Enter, and line breaks in one shape's text, each a
//! replacement of a stretch of the text by other text.
//!
//! Runs keep their looks and where the looks are set ([`presentation_model::RunSources`]):
//! typed text takes the look of the text it joins, so text typed into a placeholder keeps
//! inheriting its layout's style. A body stays as the reader leaves it, with neighbouring
//! runs that look the same merged ([`Paragraph::merge_runs`]), so undoing an edit gives
//! back the body exactly.

use crate::{Change, ShapeRef};
use presentation_model::{Paragraph, Presentation, Run, TextBody};

/// A place in a text body: a paragraph and a byte offset into its text
/// ([`Paragraph::text`]), on a character boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct TextPos {
    pub paragraph: usize,
    pub offset: usize,
}

impl TextPos {
    pub fn new(paragraph: usize, offset: usize) -> TextPos {
        TextPos { paragraph, offset }
    }
}

/// The end of the body's text.
pub fn text_end(body: &TextBody) -> TextPos {
    let last = body.paragraphs.len().saturating_sub(1);
    TextPos::new(
        last,
        body.paragraphs.get(last).map_or(0, |p| text_len(&p.runs)),
    )
}

/// The position a character before `pos`: the end of the paragraph before at the start of
/// a paragraph, `None` at the start of the text.
pub fn before(body: &TextBody, pos: TextPos) -> Option<TextPos> {
    if pos.offset == 0 {
        let paragraph = pos.paragraph.checked_sub(1)?;
        let p = body.paragraphs.get(paragraph)?;
        return Some(TextPos::new(paragraph, text_len(&p.runs)));
    }
    let text = body.paragraphs.get(pos.paragraph)?.text();
    let (offset, _) = text.get(..pos.offset)?.char_indices().next_back()?;
    Some(TextPos::new(pos.paragraph, offset))
}

/// The position a character after `pos`: the start of the next paragraph at the end of a
/// paragraph, `None` at the end of the text.
pub fn after(body: &TextBody, pos: TextPos) -> Option<TextPos> {
    let text = body.paragraphs.get(pos.paragraph)?.text();
    match text.get(pos.offset..)?.chars().next() {
        Some(c) => Some(TextPos::new(pos.paragraph, pos.offset + c.len_utf8())),
        None if pos.paragraph + 1 < body.paragraphs.len() => {
            Some(TextPos::new(pos.paragraph + 1, 0))
        }
        None => None,
    }
}

fn text_len(runs: &[Run]) -> usize {
    runs.iter().map(|r| r.text.len()).sum()
}

/// The runs covering bytes `from..to` of their joined text, cut at both ends.
fn slice(runs: &[Run], from: usize, to: usize) -> Vec<Run> {
    let (mut out, mut start) = (Vec::new(), 0);
    for r in runs {
        if start >= to {
            break;
        }
        let end = start + r.text.len();
        let (a, b) = (from.max(start), to.min(end));
        if a < b {
            out.push(r.with_text(&r.text[a - start..b - start]));
        }
        start = end;
    }
    out
}

/// `p`'s looks without its runs.
fn props(p: &Paragraph) -> Paragraph {
    Paragraph {
        level: p.level,
        align: p.align,
        margin_left: p.margin_left,
        indent: p.indent,
        line_spacing: p.line_spacing,
        space_before: p.space_before,
        space_after: p.space_after,
        bullet: p.bullet.clone(),
        runs: Vec::new(),
        end: p.end.clone(),
        markup: p.markup.clone(),
        sources: p.sources,
    }
}

/// Text cut from or pasted into a body. The first paragraph's runs join the paragraph
/// they're pasted into; each later paragraph is a paragraph of its own with its looks,
/// the last taking the runs after the paste point and that paragraph's mark. With more
/// than one paragraph, the first one's mark becomes the mark of the paragraph pasted into.
#[derive(Debug, Clone, PartialEq)]
struct Fragment(Vec<Paragraph>);

impl Fragment {
    fn is_empty(&self) -> bool {
        self.0.len() == 1 && self.0[0].runs.is_empty()
    }

    /// Where the fragment ends once pasted at `at`.
    fn end(&self, at: TextPos) -> TextPos {
        let n = self.0.len();
        let len = text_len(&self.0[n - 1].runs);
        if n == 1 {
            TextPos::new(at.paragraph, at.offset + len)
        } else {
            TextPos::new(at.paragraph + n - 1, len)
        }
    }

    /// A copy of `from..to` of `body`.
    fn copy(body: &TextBody, from: TextPos, to: TextPos) -> Fragment {
        let ps = &body.paragraphs;
        let (a, b) = (&ps[from.paragraph], &ps[to.paragraph]);
        if from.paragraph == to.paragraph {
            return Fragment(vec![Paragraph {
                runs: slice(&a.runs, from.offset, to.offset),
                ..props(a)
            }]);
        }
        let mut out = vec![Paragraph {
            runs: slice(&a.runs, from.offset, usize::MAX),
            ..props(a)
        }];
        out.extend(ps[from.paragraph + 1..to.paragraph].iter().cloned());
        out.push(Paragraph {
            runs: slice(&b.runs, 0, to.offset),
            ..props(b)
        });
        Fragment(out)
    }
}

/// Remove `from..to` from `body`: the paragraph at `from` takes the runs after `to`, and
/// the mark of the paragraph `to` is in.
fn remove(body: &mut TextBody, from: TextPos, to: TextPos) {
    let ps = &mut body.paragraphs;
    let tail = slice(&ps[to.paragraph].runs, to.offset, usize::MAX);
    let mark = (from.paragraph != to.paragraph).then(|| ps[to.paragraph].end.clone());
    let p = &mut ps[from.paragraph];
    p.runs = slice(&p.runs, 0, from.offset);
    p.runs.extend(tail);
    if let Some(mark) = mark {
        p.end = mark;
    }
    p.merge_runs();
    ps.drain(from.paragraph + 1..=to.paragraph);
}

/// Paste `fragment` at `at`; returns where it ends.
fn paste(body: &mut TextBody, at: TextPos, fragment: &Fragment) -> TextPos {
    let ps = &mut body.paragraphs;
    let p = &mut ps[at.paragraph];
    let tail = slice(&p.runs, at.offset, usize::MAX);
    p.runs = slice(&p.runs, 0, at.offset);
    p.runs.extend(fragment.0[0].runs.iter().cloned());
    if fragment.0.len() == 1 {
        let offset = text_len(&p.runs);
        p.runs.extend(tail);
        p.merge_runs();
        return TextPos::new(at.paragraph, offset);
    }
    let mark = std::mem::replace(&mut p.end, fragment.0[0].end.clone());
    p.merge_runs();
    let mut rest = fragment.0[1..].to_vec();
    let n = rest.len();
    let last = &mut rest[n - 1];
    let offset = text_len(&last.runs);
    last.runs.extend(tail);
    last.end = mark;
    last.merge_runs();
    ps.splice(at.paragraph + 1..at.paragraph + 1, rest);
    TextPos::new(at.paragraph + n, offset)
}

fn body(deck: &Presentation, shape: ShapeRef) -> Option<&TextBody> {
    deck.slides
        .get(shape.slide)?
        .shapes
        .get(shape.shape)?
        .text
        .as_ref()
}

fn body_mut(deck: &mut Presentation, shape: ShapeRef) -> &mut TextBody {
    deck.slides[shape.slide].shapes[shape.shape]
        .text
        .as_mut()
        .expect("an edit's shape has text")
}

fn valid(body: &TextBody, pos: TextPos) -> bool {
    body.paragraphs.get(pos.paragraph).is_some_and(|p| {
        let len = text_len(&p.runs);
        pos.offset <= len && p.text().is_char_boundary(pos.offset)
    })
}

/// The look text typed over `from..to` takes: the first character it replaces, else the
/// character before the caret, else the paragraph's first, else the paragraph mark's.
/// `None` when `from` isn't in a paragraph of `body`.
pub fn look(body: &TextBody, from: TextPos, to: TextPos) -> Option<&Run> {
    let p = body.paragraphs.get(from.paragraph)?;
    let len = text_len(&p.runs);
    let byte = if from != to && from.offset < len {
        from.offset
    } else {
        from.offset.saturating_sub(1)
    };
    let mut start = 0;
    for r in &p.runs {
        let end = start + r.text.len();
        if (start..end).contains(&byte) {
            return Some(r);
        }
        start = end;
    }
    Some(&p.end)
}

/// Replacing a stretch of one shape's text with other text: typing, deleting, Enter.
/// It holds the text it removes and the text it puts in, so it applies and undoes alike.
#[derive(Debug, Clone, PartialEq)]
pub struct TextEdit {
    shape: ShapeRef,
    at: TextPos,
    removed: Fragment,
    inserted: Fragment,
    /// Typed text, which typing straight after it may join.
    typed: bool,
}

impl TextEdit {
    /// `from..to` of `shape`'s text in order, when the shape has text and both are in it.
    fn range(
        deck: &Presentation,
        shape: ShapeRef,
        from: TextPos,
        to: TextPos,
    ) -> Option<(&TextBody, TextPos, TextPos)> {
        let body = body(deck, shape)?;
        let (from, to) = (from.min(to), from.max(to));
        (valid(body, from) && valid(body, to)).then_some((body, from, to))
    }

    fn new(
        body: &TextBody,
        shape: ShapeRef,
        from: TextPos,
        to: TextPos,
        inserted: Vec<Paragraph>,
        typed: bool,
    ) -> TextEdit {
        TextEdit {
            shape,
            at: from,
            removed: Fragment::copy(body, from, to),
            inserted: Fragment(inserted),
            typed,
        }
    }

    /// Type `text` (one line) over `from..to` of `shape`'s text (the caret when they're
    /// equal). It takes the look of the text it replaces, or of the character before the
    /// caret, or of the paragraph's mark in an empty paragraph.
    pub fn typing(
        deck: &Presentation,
        shape: ShapeRef,
        from: TextPos,
        to: TextPos,
        text: &str,
    ) -> Option<TextEdit> {
        if text.is_empty() {
            return None;
        }
        let (body, from, to) = Self::range(deck, shape, from, to)?;
        let p = props(&body.paragraphs[from.paragraph]);
        let run = look(body, from, to)?.with_text(text);
        let inserted = vec![Paragraph {
            runs: vec![run],
            ..p
        }];
        Some(Self::new(body, shape, from, to, inserted, true))
    }

    /// Delete `from..to` of `shape`'s text; `None` when that's nothing.
    pub fn delete(
        deck: &Presentation,
        shape: ShapeRef,
        from: TextPos,
        to: TextPos,
    ) -> Option<TextEdit> {
        let (body, from, to) = Self::range(deck, shape, from, to)?;
        if from == to {
            return None;
        }
        let inserted = vec![props(&body.paragraphs[from.paragraph])];
        Some(Self::new(body, shape, from, to, inserted, false))
    }

    /// Enter: replace `from..to` with a paragraph end; the new paragraph after it takes
    /// the looks of the one it splits (level, alignment, bullet, mark).
    pub fn new_paragraph(
        deck: &Presentation,
        shape: ShapeRef,
        from: TextPos,
        to: TextPos,
    ) -> Option<TextEdit> {
        let (body, from, to) = Self::range(deck, shape, from, to)?;
        let p = props(&body.paragraphs[from.paragraph]);
        let inserted = vec![p.clone(), p];
        Some(Self::new(body, shape, from, to, inserted, false))
    }

    /// Shift+Enter: replace `from..to` with a line break within the paragraph.
    pub fn line_break(
        deck: &Presentation,
        shape: ShapeRef,
        from: TextPos,
        to: TextPos,
    ) -> Option<TextEdit> {
        let (body, from, to) = Self::range(deck, shape, from, to)?;
        let run = look(body, from, to)?.with_text("\n");
        let inserted = vec![Paragraph {
            runs: vec![run],
            ..props(&body.paragraphs[from.paragraph])
        }];
        Some(Self::new(body, shape, from, to, inserted, false))
    }

    /// The shape whose text it changes.
    pub fn shape(&self) -> ShapeRef {
        self.shape
    }

    fn change(&self, caret: TextPos) -> Change {
        Change::Text {
            shape: self.shape,
            caret,
        }
    }

    pub(crate) fn apply(&self, deck: &mut Presentation) -> Change {
        let body = body_mut(deck, self.shape);
        remove(body, self.at, self.removed.end(self.at));
        let caret = paste(body, self.at, &self.inserted);
        body.number_bullets();
        self.change(caret)
    }

    pub(crate) fn revert(&self, deck: &mut Presentation) -> Change {
        let body = body_mut(deck, self.shape);
        remove(body, self.at, self.inserted.end(self.at));
        let caret = paste(body, self.at, &self.removed);
        body.number_bullets();
        self.change(caret)
    }

    /// Join `next`, typed straight after this typing, a word at a time: a word typed
    /// after a space starts a step of its own.
    pub(crate) fn absorb(&mut self, next: &TextEdit) -> bool {
        let continues = self.typed
            && next.typed
            && self.shape == next.shape
            && next.removed.is_empty()
            && self.inserted.0.len() == 1
            && next.inserted.0.len() == 1
            && next.at == self.inserted.end(self.at);
        if !continues {
            return false;
        }
        let last = self.inserted.0[0]
            .runs
            .last()
            .and_then(|r| r.text.chars().next_back());
        let first = next.inserted.0[0]
            .runs
            .first()
            .and_then(|r| r.text.chars().next());
        if last.is_some_and(char::is_whitespace) && first.is_some_and(|c| !c.is_whitespace()) {
            return false;
        }
        let p = &mut self.inserted.0[0];
        p.runs.extend(next.inserted.0[0].runs.iter().cloned());
        p.merge_runs();
        true
    }
}
