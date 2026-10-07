//! Every mutation is a reversible command (CMD-1). The undo stack stores
//! commands, never dataset snapshots (spec section 10). Commands change the table only
//! through `CsvTable::apply`, the one mutating entry point, which returns the inverse.
//!
//! History is capped by steps and by bytes (CMD-2, spec SP-6): each command reports what
//! it holds, and the oldest steps go first when either limit is passed.

mod clear;
mod edit;
mod reshape;

pub use clear::ClearCells;
pub use edit::SetCell;
pub use reshape::Reshape;

use data_model::CellRef;
use std::collections::VecDeque;

/// Default history size: 1,000 steps (CMD-2).
pub const HISTORY_STEPS: usize = 1_000;
/// Default history budget: 50 MB, whatever the file size (CMD-2).
pub const HISTORY_BYTES: usize = 50 << 20;

/// A reversible change to some target, usually the data model.
pub trait Command<T> {
    fn apply(&mut self, target: &mut T);
    fn revert(&mut self, target: &mut T);
    /// Short label for menus, e.g. "Sort column B".
    fn label(&self) -> &str;
    /// The cell to show after this command runs or is undone, if it changes one.
    fn focus(&self) -> Option<CellRef> {
        None
    }
    /// Memory this command holds, itself included: what keeping it in history costs.
    /// Can change when it runs (an edit holds whichever value it would swap back in).
    fn heap_bytes(&self) -> usize;
}

/// Undo and redo history within a step and a byte budget.
pub struct UndoStack<T> {
    /// Oldest first; the back is the next undo.
    done: VecDeque<Box<dyn Command<T>>>,
    /// Furthest first; the back is the next redo.
    undone: VecDeque<Box<dyn Command<T>>>,
    max_steps: usize,
    max_bytes: usize,
    /// `heap_bytes` of every command held, done and undone.
    bytes: usize,
}

impl<T> Default for UndoStack<T> {
    fn default() -> Self {
        Self::new(HISTORY_STEPS, HISTORY_BYTES)
    }
}

impl<T> UndoStack<T> {
    pub fn new(max_steps: usize, max_bytes: usize) -> Self {
        Self {
            done: VecDeque::new(),
            undone: VecDeque::new(),
            max_steps,
            max_bytes,
            bytes: 0,
        }
    }

    pub fn execute(&mut self, mut cmd: Box<dyn Command<T>>, target: &mut T) {
        cmd.apply(target);
        let dropped: usize = self.undone.drain(..).map(|c| c.heap_bytes()).sum();
        self.bytes = self.bytes - dropped + cmd.heap_bytes();
        self.done.push_back(cmd);
        self.trim(false);
    }

    /// Revert the last command; returns it, or `None` when there is nothing to undo.
    pub fn undo(&mut self, target: &mut T) -> Option<&dyn Command<T>> {
        let mut cmd = self.done.pop_back()?;
        self.bytes -= cmd.heap_bytes();
        cmd.revert(target);
        self.bytes += cmd.heap_bytes();
        self.undone.push_back(cmd);
        self.trim(true);
        self.undone.back().map(|c| &**c)
    }

    /// Apply the last undone command again; returns it, or `None` when there is none.
    pub fn redo(&mut self, target: &mut T) -> Option<&dyn Command<T>> {
        let mut cmd = self.undone.pop_back()?;
        self.bytes -= cmd.heap_bytes();
        cmd.apply(target);
        self.bytes += cmd.heap_bytes();
        self.done.push_back(cmd);
        self.trim(false);
        self.done.back().map(|c| &**c)
    }

    /// Steps that can be undone.
    pub fn len(&self) -> usize {
        self.done.len()
    }

    pub fn is_empty(&self) -> bool {
        self.done.is_empty()
    }

    /// Steps that can be redone.
    pub fn redo_len(&self) -> usize {
        self.undone.len()
    }

    /// Memory the history holds (the commands' own accounting).
    pub fn heap_bytes(&self) -> usize {
        self.bytes
    }

    /// Forget the oldest steps until both limits hold: the far end of the undo history
    /// first, then the far end of the redo history. The step just run (or just undone)
    /// always stays, so the last action can be undone even if it alone is over budget.
    fn trim(&mut self, just_undone: bool) {
        while self.done.len() > self.max_steps {
            if let Some(c) = self.done.pop_front() {
                self.bytes -= c.heap_bytes();
            }
        }
        while self.bytes > self.max_bytes {
            let oldest = if self.done.len() > usize::from(!just_undone) {
                self.done.pop_front()
            } else if self.undone.len() > usize::from(just_undone) {
                self.undone.pop_front()
            } else {
                None
            };
            match oldest {
                Some(c) => self.bytes -= c.heap_bytes(),
                None => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Add(i64);
    impl Command<i64> for Add {
        fn apply(&mut self, t: &mut i64) {
            *t += self.0;
        }
        fn revert(&mut self, t: &mut i64) {
            *t -= self.0;
        }
        fn label(&self) -> &str {
            "add"
        }
        fn heap_bytes(&self) -> usize {
            std::mem::size_of::<Self>()
        }
    }

    /// A command that holds `n` bytes, like a paste or a replace-all would.
    struct Big(Vec<u8>);
    impl Command<i64> for Big {
        fn apply(&mut self, t: &mut i64) {
            *t += 1;
        }
        fn revert(&mut self, t: &mut i64) {
            *t -= 1;
        }
        fn label(&self) -> &str {
            "big"
        }
        fn heap_bytes(&self) -> usize {
            std::mem::size_of::<Self>() + self.0.capacity()
        }
    }

    const MB: usize = 1 << 20;

    #[test]
    fn history_stays_under_its_byte_budget() {
        let mut v = 0;
        let mut s = UndoStack::new(1_000, 50 * MB);
        for _ in 0..1_000 {
            s.execute(Box::new(Big(vec![0; MB])), &mut v);
            assert!(s.heap_bytes() <= 50 * MB, "{} bytes", s.heap_bytes());
        }
        // The oldest steps went; the newest ones still undo.
        assert!(s.len() < 50 && s.len() >= 45, "{} steps kept", s.len());
        while s.undo(&mut v).is_some() {}
        assert_eq!(v, 1_000 - s.redo_len() as i64);
        // Undone steps count too: redo history is history.
        assert!(s.heap_bytes() <= 50 * MB);
    }

    #[test]
    fn the_last_step_stays_undoable_even_when_it_alone_is_over_budget() {
        let mut v = 0;
        let mut s = UndoStack::new(1_000, 50 * MB);
        s.execute(Box::new(Add(1)), &mut v);
        s.execute(Box::new(Big(vec![0; 60 * MB])), &mut v);
        assert_eq!(s.len(), 1, "older steps make room");
        assert!(s.undo(&mut v).is_some());
        assert_eq!(v, 1);
        // Anything new drops it: it can't stay alongside.
        s.execute(Box::new(Add(5)), &mut v);
        assert_eq!((s.len(), s.redo_len()), (1, 0));
    }

    #[test]
    fn step_limit_still_applies() {
        let mut v = 0;
        let mut s = UndoStack::new(3, 50 * MB);
        for i in 1..=5 {
            s.execute(Box::new(Add(i)), &mut v);
        }
        assert_eq!(s.len(), 3);
        while s.undo(&mut v).is_some() {}
        assert_eq!(v, 1 + 2, "steps 3-5 undone, 1-2 forgotten");
    }

    #[test]
    fn undo_redo_round_trip() {
        let mut v = 0;
        let mut s = UndoStack::new(100, 50 * MB);
        s.execute(Box::new(Add(5)), &mut v);
        s.execute(Box::new(Add(2)), &mut v);
        assert!(s.undo(&mut v).is_some());
        assert_eq!(v, 5);
        assert!(s.redo(&mut v).is_some());
        assert_eq!(v, 7);
    }
}
