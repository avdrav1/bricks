//! Frame's edits (ADR 0006): every change to a deck is a command that stores the operation
//! and what undoes it, never a snapshot of the deck, and names what it changes (a shape's
//! text, its box or looks, a slide, the slide list), so a save can patch only those.
//! [`History`] keeps the steps for undo and redo.
//!
//! Toolkit-free: no GTK, no I/O.

mod canvas;
mod frame;
mod insert;
mod layout;
mod outline;
mod slides;
mod style;
mod text;

pub use canvas::{next_placeholder, relevel, MAX_LEVEL};
pub use frame::{movable, snap, FrameEdit, Grab, Guide, Snapped};
pub use insert::{
    shrinks, takes_picture, Fit, FitEdit, ShapeEdit, ACCENT, ACCENT_DARK, BAND_1, BAND_2,
    EMU_PER_PIXEL, FITS, SHAPES,
};
pub use layout::LayoutEdit;
pub use outline::{
    body_of, erase, key, outline, title_of, typing, BodyEdit, Line, LineKind, Outline, OutlineKey,
    OutlinePos,
};
pub use slides::{next_layout, SlideEdit};
pub use style::{
    styleable, BackgroundEdit, LookEdit, ParagraphLook, ShapeLook, ShapeLookEdit, TextLook,
    BULLET_MARGIN,
};
pub use text::{after, before, look, text_end, TextEdit, TextPos};

use presentation_model::Presentation;
use std::collections::VecDeque;

/// Steps kept for undo; older ones are forgotten.
pub const HISTORY_STEPS: usize = 1_000;

/// A shape on a slide: indexes into `Presentation::slides` and that slide's `shapes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShapeRef {
    pub slide: usize,
    pub shape: usize,
}

/// What applying, undoing, or redoing an edit changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// A shape's text, and where the caret goes in it.
    Text { shape: ShapeRef, caret: TextPos },
    /// The slide list (slides added, removed, or moved), and the slide to show.
    Slides { show: usize },
    /// A shape's box: it moved or changed size.
    Shape { shape: ShapeRef },
    /// A shape's looks (its text's, fill, outline); its text and box are as they were.
    Look { shape: ShapeRef },
    /// A slide as a whole: its background, its layout and so its placeholders.
    Slide { slide: usize },
}

impl Change {
    /// The slide to show.
    pub fn slide(&self) -> usize {
        match self {
            Change::Text { shape, .. } | Change::Shape { shape } | Change::Look { shape } => {
                shape.slide
            }
            Change::Slides { show } => *show,
            Change::Slide { slide } => *slide,
        }
    }

    /// Where the caret goes, for a text change.
    pub fn caret(&self) -> Option<TextPos> {
        match self {
            Change::Text { caret, .. } => Some(*caret),
            _ => None,
        }
    }

    /// What a step of several edits changed, from each edit's change in order: the first
    /// edit's, but the slide list when any edit changed it.
    fn of_steps(changes: &[Change]) -> Change {
        let first = changes
            .first()
            .copied()
            .unwrap_or(Change::Slides { show: 0 });
        if changes.iter().any(|c| matches!(c, Change::Slides { .. })) {
            Change::Slides {
                show: first.slide(),
            }
        } else {
            first
        }
    }
}

/// One undoable change to a deck.
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    Text(TextEdit),
    Slides(SlideEdit),
    Frame(FrameEdit),
    Look(LookEdit),
    ShapeLook(ShapeLookEdit),
    Background(BackgroundEdit),
    Layout(LayoutEdit),
    Shapes(ShapeEdit),
    Fit(FitEdit),
    /// A shape's paragraphs replaced (outline edits).
    Body(BodyEdit),
    /// Edits made as one step, in order (typing and the shrink it caused).
    Steps(Vec<Edit>),
}

impl Edit {
    fn apply(&self, deck: &mut Presentation) -> Change {
        match self {
            Edit::Text(e) => e.apply(deck),
            Edit::Slides(e) => e.apply(deck),
            Edit::Frame(e) => e.apply(deck),
            Edit::Look(e) => e.apply(deck),
            Edit::ShapeLook(e) => e.apply(deck),
            Edit::Background(e) => e.apply(deck),
            Edit::Layout(e) => e.apply(deck),
            Edit::Shapes(e) => e.apply(deck),
            Edit::Fit(e) => e.apply(deck),
            Edit::Body(e) => e.apply(deck),
            Edit::Steps(steps) => {
                let changes: Vec<Change> = steps.iter().map(|e| e.apply(deck)).collect();
                Change::of_steps(&changes)
            }
        }
    }

    fn revert(&self, deck: &mut Presentation) -> Change {
        match self {
            Edit::Text(e) => e.revert(deck),
            Edit::Slides(e) => e.revert(deck),
            Edit::Frame(e) => e.revert(deck),
            Edit::Look(e) => e.revert(deck),
            Edit::ShapeLook(e) => e.revert(deck),
            Edit::Background(e) => e.revert(deck),
            Edit::Layout(e) => e.revert(deck),
            Edit::Shapes(e) => e.revert(deck),
            Edit::Fit(e) => e.revert(deck),
            Edit::Body(e) => e.revert(deck),
            Edit::Steps(steps) => {
                let mut changes: Vec<Change> = steps.iter().rev().map(|e| e.revert(deck)).collect();
                changes.reverse();
                Change::of_steps(&changes)
            }
        }
    }

    /// Join `next`, already applied, into this step when it continues it: typing after
    /// typing, also when the step holds what the typing caused (a shrink to fit).
    fn absorb(&mut self, next: &Edit) -> bool {
        match (self, next) {
            (Edit::Text(a), Edit::Text(b)) => a.absorb(b),
            (Edit::Steps(steps), Edit::Text(b)) => {
                if let Some(Edit::Text(last)) = steps.last_mut() {
                    if last.absorb(b) {
                        return true;
                    }
                }
                let typed = steps.iter().rev().find_map(|e| match e {
                    Edit::Text(t) => Some(t.clone()),
                    _ => None,
                });
                let continues = typed.is_some_and(|mut t| t.absorb(b));
                if continues {
                    steps.push(next.clone());
                }
                continues
            }
            _ => false,
        }
    }
}

macro_rules! edit_from {
    ($($t:ident => $v:ident),*) => {$(
        impl From<$t> for Edit {
            fn from(e: $t) -> Edit {
                Edit::$v(e)
            }
        }
    )*};
}

edit_from!(
    TextEdit => Text,
    SlideEdit => Slides,
    FrameEdit => Frame,
    LookEdit => Look,
    ShapeLookEdit => ShapeLook,
    BackgroundEdit => Background,
    LayoutEdit => Layout,
    ShapeEdit => Shapes,
    FitEdit => Fit,
    BodyEdit => Body
);

/// Undo and redo for one deck: the steps applied, newest last, and the steps undone.
#[derive(Debug, Default)]
pub struct History {
    done: VecDeque<Edit>,
    undone: Vec<Edit>,
    /// The next edit starts a step of its own, whatever came before.
    sealed: bool,
}

impl History {
    pub fn new() -> History {
        History::default()
    }

    /// Apply `edit` to `deck` and keep it for undo. Typing straight after typing joins its
    /// step, a word at a time, unless [`History::seal`] was called in between.
    pub fn apply(&mut self, deck: &mut Presentation, edit: impl Into<Edit>) -> Change {
        let edit = edit.into();
        let change = edit.apply(deck);
        self.undone.clear();
        let joined = !self.sealed && self.done.back_mut().is_some_and(|last| last.absorb(&edit));
        if !joined {
            self.done.push_back(edit);
            if self.done.len() > HISTORY_STEPS {
                self.done.pop_front();
            }
        }
        self.sealed = false;
        change
    }

    /// Apply `edit` as part of the last step, so one undo takes back both: what an edit
    /// caused (its text shrunk to fit, its box grown). A step of its own when there is
    /// none.
    pub fn amend(&mut self, deck: &mut Presentation, edit: impl Into<Edit>) -> Change {
        let edit = edit.into();
        let change = edit.apply(deck);
        self.undone.clear();
        match self.done.pop_back() {
            Some(Edit::Steps(mut steps)) => {
                steps.push(edit);
                self.done.push_back(Edit::Steps(steps));
            }
            Some(last) => self.done.push_back(Edit::Steps(vec![last, edit])),
            None => self.done.push_back(edit),
        }
        change
    }

    /// Undo the last step.
    pub fn undo(&mut self, deck: &mut Presentation) -> Option<Change> {
        let edit = self.done.pop_back()?;
        let change = edit.revert(deck);
        self.undone.push(edit);
        self.sealed = true;
        Some(change)
    }

    /// Redo the last step undone.
    pub fn redo(&mut self, deck: &mut Presentation) -> Option<Change> {
        let edit = self.undone.pop()?;
        let change = edit.apply(deck);
        self.done.push_back(edit);
        self.sealed = true;
        Some(change)
    }

    /// End the current step: the next edit won't join it (the caret moved, ...).
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn can_undo(&self) -> bool {
        !self.done.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }
}
