//! Slide edits: adding a slide on a layout, duplicating, deleting, and moving slides.
//! Each holds the slide it puts in or takes out, so it applies and undoes alike.

use crate::Change;
use presentation_model::{Presentation, Slide};

/// A change to the slide list.
#[derive(Debug, Clone, PartialEq)]
pub enum SlideEdit {
    /// Put `slide` in at `at`.
    Insert { at: usize, slide: Slide },
    /// Take out `slide`, the slide at `at`.
    Remove { at: usize, slide: Slide },
    /// Move the slide at `from` to `to`, its index once moved.
    Move { from: usize, to: usize },
}

/// The layout Ctrl+M gives a new slide after slide `after`: that slide's, except after a
/// title slide its master's Title and Content (`obj`, or `tx` in older and LibreOffice
/// decks). With no slide to follow, or one on a layout the deck doesn't list, the first
/// Title and Content, else the first layout. `None` when the deck has no layouts.
pub fn next_layout(deck: &Presentation, after: Option<usize>) -> Option<usize> {
    let content = |master: Option<usize>| {
        ["obj", "tx"].iter().find_map(|kind| {
            deck.layouts
                .iter()
                .position(|l| l.kind == *kind && master.is_none_or(|m| l.master == m))
        })
    };
    let current = after
        .and_then(|i| deck.slides.get(i))
        .and_then(|s| s.layout)
        .filter(|&l| l < deck.layouts.len());
    match current {
        Some(l) if deck.layouts[l].kind == "title" => {
            content(Some(deck.layouts[l].master)).or(Some(l))
        }
        Some(l) => Some(l),
        None => content(None).or((!deck.layouts.is_empty()).then_some(0)),
    }
}

impl SlideEdit {
    /// Ctrl+M: a new slide after slide `after` (first when `None`) on the layout
    /// [`next_layout`] picks.
    pub fn new_slide(deck: &Presentation, after: Option<usize>) -> Option<SlideEdit> {
        Self::on_layout(deck, after, next_layout(deck, after)?)
    }

    /// A new slide on layout `layout`, after slide `after` (first when `None`).
    pub fn on_layout(
        deck: &Presentation,
        after: Option<usize>,
        layout: usize,
    ) -> Option<SlideEdit> {
        let slide = deck.layouts.get(layout)?.slide.clone();
        let at = after.map_or(0, |a| a + 1).min(deck.slides.len());
        Some(SlideEdit::Insert { at, slide })
    }

    /// A copy of slide `at` after it.
    pub fn duplicate(deck: &Presentation, at: usize) -> Option<SlideEdit> {
        let slide = deck.slides.get(at)?.clone();
        Some(SlideEdit::Insert { at: at + 1, slide })
    }

    /// Delete slide `at`.
    pub fn delete(deck: &Presentation, at: usize) -> Option<SlideEdit> {
        let slide = deck.slides.get(at)?.clone();
        Some(SlideEdit::Remove { at, slide })
    }

    /// Move slide `from` so it's at `to`; `None` when that's where it is.
    pub fn move_to(deck: &Presentation, from: usize, to: usize) -> Option<SlideEdit> {
        let n = deck.slides.len();
        (from != to && from < n && to < n).then_some(SlideEdit::Move { from, to })
    }

    pub(crate) fn apply(&self, deck: &mut Presentation) -> Change {
        let slides = &mut deck.slides;
        let show = match self {
            SlideEdit::Insert { at, slide } => {
                slides.insert(*at, slide.clone());
                *at
            }
            SlideEdit::Remove { at, .. } => {
                slides.remove(*at);
                (*at).min(slides.len().saturating_sub(1))
            }
            SlideEdit::Move { from, to } => {
                let s = slides.remove(*from);
                slides.insert(*to, s);
                *to
            }
        };
        Change::Slides { show }
    }

    pub(crate) fn revert(&self, deck: &mut Presentation) -> Change {
        let slides = &mut deck.slides;
        let show = match self {
            // Back on the slide the new one followed.
            SlideEdit::Insert { at, .. } => {
                slides.remove(*at);
                at.saturating_sub(1).min(slides.len().saturating_sub(1))
            }
            SlideEdit::Remove { at, slide } => {
                slides.insert(*at, slide.clone());
                *at
            }
            SlideEdit::Move { from, to } => {
                let s = slides.remove(*to);
                slides.insert(*from, s);
                *from
            }
        };
        Change::Slides { show }
    }
}
