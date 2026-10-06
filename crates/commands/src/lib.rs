//! Every mutation is a reversible command (CMD-1). The undo stack stores
//! commands, never dataset snapshots (spec section 10).

/// A reversible change to some target, usually the data model.
pub trait Command<T> {
    fn apply(&mut self, target: &mut T);
    fn revert(&mut self, target: &mut T);
    /// Short label for menus, e.g. "Sort column B".
    fn label(&self) -> &str;
}

pub struct UndoStack<T> {
    done: Vec<Box<dyn Command<T>>>,
    undone: Vec<Box<dyn Command<T>>>,
    limit: usize,
}

impl<T> UndoStack<T> {
    pub fn new(limit: usize) -> Self {
        Self {
            done: Vec::new(),
            undone: Vec::new(),
            limit,
        }
    }

    pub fn execute(&mut self, mut cmd: Box<dyn Command<T>>, target: &mut T) {
        cmd.apply(target);
        self.done.push(cmd);
        self.undone.clear();
        if self.done.len() > self.limit {
            self.done.remove(0);
        }
    }

    pub fn undo(&mut self, target: &mut T) -> bool {
        match self.done.pop() {
            Some(mut cmd) => {
                cmd.revert(target);
                self.undone.push(cmd);
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self, target: &mut T) -> bool {
        match self.undone.pop() {
            Some(mut cmd) => {
                cmd.apply(target);
                self.done.push(cmd);
                true
            }
            None => false,
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
    }

    #[test]
    fn undo_redo_round_trip() {
        let mut v = 0;
        let mut s = UndoStack::new(100);
        s.execute(Box::new(Add(5)), &mut v);
        s.execute(Box::new(Add(2)), &mut v);
        assert!(s.undo(&mut v));
        assert_eq!(v, 5);
        assert!(s.redo(&mut v));
        assert_eq!(v, 7);
    }
}
