//! The blocks an edit, undo, or redo replaces are known before it runs: `Edit::span` and
//! `History::next_span` give exactly the old range of the `Change` that follows, list
//! renumbering and joined typing included.

use document_edit::{
    copy, delete, indent, join_backward, join_forward, look, normalize, paste, replace, set_style,
    split, text, toggle, toggle_list, Change, Container, Edit, Flag, History, Pos,
};
use document_model::{read_markdown, Block, Document};
use std::ops::Range;
use Container::Top;

fn md(source: &str) -> Document {
    let mut doc = read_markdown(source.as_bytes());
    normalize(&mut doc);
    doc
}

fn pos(block: usize, offset: usize) -> Pos {
    Pos::new(block, offset)
}

fn old_range(change: &Change) -> (Container, Range<usize>) {
    (change.at, change.start..change.start + change.removed)
}

/// The first top-level block whose text starts with `prefix`.
fn find(doc: &Document, prefix: &str) -> usize {
    (0..doc.blocks.len())
        .find(|&i| text(doc, Top, i).is_some_and(|t| t.starts_with(prefix)))
        .unwrap_or_else(|| panic!("no block starting {prefix:?}: {:?}", doc.blocks))
}

fn table(doc: &Document) -> usize {
    doc.blocks
        .iter()
        .position(|b| matches!(b, Block::Table(_)))
        .expect("a table")
}

/// A document and its history, checking every span against the change that follows.
struct Session {
    doc: Document,
    history: History,
    checked: usize,
}

impl Session {
    fn new(source: &str) -> Session {
        Session {
            doc: md(source),
            history: History::new(),
            checked: 0,
        }
    }

    fn apply(&mut self, make: impl FnOnce(&Document) -> Option<Edit>) -> Change {
        let edit = make(&self.doc).expect("an edit");
        let before = self.doc.clone();
        let span = edit.span(&self.doc);
        assert_eq!(self.doc, before, "span leaves the document alone");
        let change = self.history.apply(&mut self.doc, edit);
        assert_eq!(span, old_range(&change), "apply #{}", self.checked);
        self.checked += 1;
        change
    }

    fn undo(&mut self) -> Change {
        let span = self.history.next_span(false).expect("a step to undo");
        let change = self.history.undo(&mut self.doc).expect("a step to undo");
        assert_eq!(span, old_range(&change), "undo #{}", self.checked);
        self.checked += 1;
        change
    }

    fn redo(&mut self) -> Change {
        let span = self.history.next_span(true).expect("a step to redo");
        let change = self.history.redo(&mut self.doc).expect("a step to redo");
        assert_eq!(span, old_range(&change), "redo #{}", self.checked);
        self.checked += 1;
        change
    }

    fn type_at(&mut self, at: Pos, s: &str) -> Change {
        self.apply(|d| replace(d, at, at, s, &look(d, at)))
    }
}

#[test]
fn spans_match_the_changes_of_a_mixed_session() {
    let mut s = Session::new(
        "Intro text\n\n1. one\n2. two\n3. three\n\n| A | B |\n|---|---|\n| c | d |\n\nAfter here\n\nLast\n",
    );
    assert_eq!(s.history.next_span(false), None);
    assert_eq!(s.history.next_span(true), None);

    // Typing, the second keystroke joining the first's step, then a word after a space.
    s.type_at(pos(0, 5), "X");
    s.type_at(pos(0, 6), "Y");
    s.type_at(pos(0, 7), " ");
    s.type_at(pos(0, 8), "w");

    // Enter at the end of "one": the items after it renumber, widening the change.
    let one = find(&s.doc, "one");
    let change = s.apply(|d| split(d, pos(one, 3), pos(one, 3)));
    assert!(change.removed > 1, "renumbering widened {change:?}");
    s.type_at(pos(one + 1, 0), "new");
    // Backspace at the new item's start: it leaves the list (the items after it start a
    // list of their own, keeping their numbers), then joins the item before, and the
    // items after renumber.
    s.apply(|d| join_backward(d, pos(one + 1, 0)));
    let change = s.apply(|d| join_backward(d, pos(one + 1, 0)));
    assert!(change.removed > 1, "renumbering widened {change:?}");

    // Delete at the end of "After here": the paragraph after joins it.
    let after = find(&s.doc, "After");
    s.apply(|d| join_forward(d, pos(after, 10)));

    // Looks: bold, a heading, a numbered list taking in the intro, a deeper item.
    s.apply(|d| toggle(d, pos(0, 0), pos(0, 5), Flag::Bold));
    s.apply(|d| set_style(d, Top, 0..1, "Heading2"));
    s.apply(|d| toggle_list(d, Top, 0..1, true));
    s.apply(|d| indent(d, Top, 2..3, 1));
    s.apply(|d| indent(d, Top, 2..3, -1));

    // A cell edit.
    let t = table(&s.doc);
    let at = Pos::in_cell(t, 0, 0, 0, 1);
    s.type_at(at, "Z");

    // Undo and redo mid-session.
    s.undo();
    s.undo();
    s.redo();
    s.redo();

    // A delete from a top-level block into a cell, through the table.
    let t = table(&s.doc);
    s.apply(|d| delete(d, pos(t - 1, 1), Pos::in_cell(t, 1, 1, 0, 1)));

    // Paste a fragment holding a table and numbered items, from another document.
    let source = md("1. a\n2. b\n\n| P | Q |\n|---|---|\n| r | s |\n\nTail end\n");
    let fragment = copy(&source, pos(0, 0), pos(3, 4)).expect("a fragment");
    assert!(fragment.blocks.iter().any(|b| matches!(b, Block::Table(_))));
    let last = s.doc.blocks.len() - 1;
    s.apply(|d| paste(d, pos(last, 1), pos(last, 1), &fragment));
    // Paste into a numbered list: the items after renumber.
    let items = copy(&source, pos(0, 0), pos(1, 1)).expect("a fragment");
    let first = (0..s.doc.blocks.len())
        .find(|&i| matches!(s.doc.blocks[i], Block::ListItem(_)))
        .expect("a list item");
    s.apply(|d| paste(d, pos(first, 0), pos(first, 0), &items));

    // Undo everything, then redo everything.
    let mut steps = 0;
    while s.history.can_undo() {
        s.undo();
        steps += 1;
    }
    assert_eq!(s.history.next_span(false), None);
    for _ in 0..steps {
        s.redo();
    }
    assert_eq!(s.history.next_span(true), None);
    assert!(s.checked >= 15 + 2 * steps, "{} checks", s.checked);
}
