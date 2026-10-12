//! Frame's edits (ADR 0006): every change to a deck is a command that stores the operation
//! and what undoes it, never a snapshot of the deck, and names what it changes (a shape's
//! text, the slide list), so a save can patch only those. [`History`] keeps the steps for
//! undo and redo.
//!
//! Toolkit-free: no GTK, no I/O.

mod frame;
mod slides;
mod text;

pub use frame::{movable, snap, FrameEdit, Grab, Guide, Snapped};
pub use slides::{next_layout, SlideEdit};
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
}

impl Change {
    /// The slide to show.
    pub fn slide(&self) -> usize {
        match self {
            Change::Text { shape, .. } | Change::Shape { shape } => shape.slide,
            Change::Slides { show } => *show,
        }
    }

    /// Where the caret goes, for a text change.
    pub fn caret(&self) -> Option<TextPos> {
        match self {
            Change::Text { caret, .. } => Some(*caret),
            _ => None,
        }
    }
}

/// One undoable change to a deck.
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    Text(TextEdit),
    Slides(SlideEdit),
    Frame(FrameEdit),
}

impl Edit {
    fn apply(&self, deck: &mut Presentation) -> Change {
        match self {
            Edit::Text(e) => e.apply(deck),
            Edit::Slides(e) => e.apply(deck),
            Edit::Frame(e) => e.apply(deck),
        }
    }

    fn revert(&self, deck: &mut Presentation) -> Change {
        match self {
            Edit::Text(e) => e.revert(deck),
            Edit::Slides(e) => e.revert(deck),
            Edit::Frame(e) => e.revert(deck),
        }
    }

    /// Join `next`, already applied, into this step when it continues it.
    fn absorb(&mut self, next: &Edit) -> bool {
        match (self, next) {
            (Edit::Text(a), Edit::Text(b)) => a.absorb(b),
            _ => false,
        }
    }
}

impl From<TextEdit> for Edit {
    fn from(e: TextEdit) -> Edit {
        Edit::Text(e)
    }
}

impl From<SlideEdit> for Edit {
    fn from(e: SlideEdit) -> Edit {
        Edit::Slides(e)
    }
}

impl From<FrameEdit> for Edit {
    fn from(e: FrameEdit) -> Edit {
        Edit::Frame(e)
    }
}

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
