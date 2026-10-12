//! The canvas's keyboard flow (FRAME-15), as PowerPoint has it: Ctrl+Enter goes from a
//! slide's text placeholder to the next ([`next_placeholder`]; after the last one the
//! window adds a slide, as New Slide does), and Tab and Shift+Tab at the start of a
//! paragraph, or over several, move the paragraphs a level down or up ([`relevel`]).

use crate::layout::rebased;
use crate::outline::{levels, paragraphs};
use crate::{BodyEdit, ShapeRef, TextPos};
use presentation_model::{Layer, PlaceholderKind, Presentation, Shape, Slide};

/// The deepest paragraph level.
pub const MAX_LEVEL: u8 = 8;

/// Whether `shape` is one of its slide's text placeholders: the slide's own (with an id
/// and a text body) title, centred title, subtitle, body, or content placeholder. Not the
/// date, footer, slide number, or header.
fn text_placeholder(shape: &Shape) -> bool {
    shape.layer == Layer::Slide
        && shape.id.is_some()
        && shape.text.is_some()
        && matches!(
            shape.placeholder,
            Some(
                PlaceholderKind::Title
                    | PlaceholderKind::CentredTitle
                    | PlaceholderKind::Subtitle
                    | PlaceholderKind::Body
                    | PlaceholderKind::Content
            )
        )
}

/// Ctrl+Enter's next stop on `slide`: the first of its text placeholders after shape
/// `after` in its shape order (`None`: the first of them), by index. `None` when there's
/// no other: the window adds a slide then.
pub fn next_placeholder(slide: &Slide, after: Option<usize>) -> Option<usize> {
    let start = after.map_or(0, |a| a + 1);
    slide
        .shapes
        .iter()
        .enumerate()
        .skip(start)
        .find(|(_, s)| text_placeholder(s))
        .map(|(i, _)| i)
}

/// Tab (`down`) or Shift+Tab over `from..to` (either order) of `shape`'s text: the
/// paragraphs the range touches a level down (at most [`MAX_LEVEL`]) or up (at least 0),
/// each with the shape's looks for its new level, as one step; the caret stays at `to`.
/// Only with the caret at the start of a paragraph or a range over more than one; a
/// paragraph already at the limit stays. `None` when nothing would change.
pub fn relevel(
    deck: &Presentation,
    shape: ShapeRef,
    from: TextPos,
    to: TextPos,
    down: bool,
) -> Option<BodyEdit> {
    let (first, last) = (from.min(to), from.max(to));
    let caret_at_start = first == last && first.offset == 0;
    if !caret_at_start && first.paragraph == last.paragraph {
        return None;
    }
    let levels = levels(deck, shape);
    let mut after = paragraphs(deck, shape)?.clone();
    let last_paragraph = last.paragraph.min(after.len().checked_sub(1)?);
    if first.paragraph > last_paragraph {
        return None;
    }
    for p in &mut after[first.paragraph..=last_paragraph] {
        let level = if down {
            p.level.saturating_add(1).min(MAX_LEVEL)
        } else {
            p.level.saturating_sub(1)
        };
        if level != p.level {
            *p = rebased(p, level, &levels)?;
        }
    }
    BodyEdit::new(deck, shape, after, to)
}
