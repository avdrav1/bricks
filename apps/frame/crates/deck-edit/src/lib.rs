//! Frame's edits (ADR 0006): every change to a deck is a command that stores the operation
//! and what undoes it, never a snapshot of the deck, and names the slide, shape, and
//! property it changes, so a save can patch only those. [`History`] keeps the steps for
//! undo and redo.
//!
//! Toolkit-free: no GTK, no I/O.

mod text;

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

/// The part of a shape an edit changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Property {
    /// The text: paragraphs and runs.
    Text,
}

/// What applying, undoing, or redoing an edit changed, and where the caret goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Change {
    pub shape: ShapeRef,
    pub property: Property,
    pub caret: TextPos,
}

/// One undoable change to a deck.
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    Text(TextEdit),
}

impl Edit {
    fn apply(&self, deck: &mut Presentation) -> Change {
        match self {
            Edit::Text(e) => e.apply(deck),
        }
    }

    fn revert(&self, deck: &mut Presentation) -> Change {
        match self {
            Edit::Text(e) => e.revert(deck),
        }
    }

    /// Join `next`, already applied, into this step when it continues it.
    fn absorb(&mut self, next: &Edit) -> bool {
        match (self, next) {
            (Edit::Text(a), Edit::Text(b)) => a.absorb(b),
        }
    }
}

impl From<TextEdit> for Edit {
    fn from(e: TextEdit) -> Edit {
        Edit::Text(e)
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
