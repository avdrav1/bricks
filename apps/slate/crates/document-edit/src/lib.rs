//! Slate's edits: every change to a document is a command that stores the operation and
//! what undoes it, never a snapshot of the document. A text edit replaces a stretch of
//! text with other text and holds both; a looks edit changes the looks of whole blocks
//! (paragraph style, list membership, list level, list labels) and holds each block's
//! looks before and after. [`History`] keeps the steps for undo and redo.
//!
//! Paragraphs stay normalized ([`normalize`]): neighbouring runs that look the same are
//! one run and no run is empty, so undoing an edit gives back the document exactly.
//!
//! Toolkit-free: no GTK, no I/O.

mod looks;
mod text;

pub use looks::{indent, set_style, toggle_list, LooksEdit};
pub use text::{join_backward, join_forward, look, replace, split, toggle, TextEdit};

use document_model::{Block, CodeBlock, Document, ListItem, Paragraph, Run};
use std::borrow::Cow;
use std::collections::VecDeque;

/// Steps kept for undo; older ones are forgotten.
pub const HISTORY_STEPS: usize = 1_000;

/// A place in the document: a top-level block (index into [`Document::blocks`]) and a
/// byte offset into its text, on a char boundary. Text of Paragraph = runs joined;
/// ListItem = its paragraph's runs joined; CodeBlock = `text` without its final "\n".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Pos {
    pub block: usize,
    pub offset: usize,
}

impl Pos {
    pub fn new(block: usize, offset: usize) -> Pos {
        Pos { block, offset }
    }
}

/// Paragraph, ListItem, CodeBlock are editable; tables, images, page breaks, rules, Raw
/// are not.
pub fn is_editable(block: &Block) -> bool {
    matches!(
        block,
        Block::Paragraph(_) | Block::ListItem(_) | Block::CodeBlock(_)
    )
}

/// The block's text length in bytes (see [`Pos`]); `None` if not editable or out of range.
pub fn text_len(doc: &Document, block: usize) -> Option<usize> {
    match doc.blocks.get(block)? {
        Block::Paragraph(p) => Some(runs_len(&p.runs)),
        Block::ListItem(item) => Some(runs_len(&item.paragraph.runs)),
        Block::CodeBlock(code) => Some(code_text(&code.text).len()),
        _ => None,
    }
}

/// The block's text (see [`Pos`]) for editable blocks.
pub fn text(doc: &Document, block: usize) -> Option<Cow<'_, str>> {
    match doc.blocks.get(block)? {
        Block::Paragraph(p) => Some(joined(&p.runs)),
        Block::ListItem(item) => Some(joined(&item.paragraph.runs)),
        Block::CodeBlock(code) => Some(Cow::Borrowed(code_text(&code.text))),
        _ => None,
    }
}

/// Merge neighbouring runs with the same look and drop empty runs in every paragraph,
/// table cells included, so undo restores the body exactly.
pub fn normalize(doc: &mut Document) {
    normalize_blocks(&mut doc.blocks);
}

fn normalize_blocks(blocks: &mut [Block]) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => p.merge_runs(),
            Block::ListItem(item) => item.paragraph.merge_runs(),
            Block::Table(table) => {
                for cell in table.rows.iter_mut().flat_map(|r| &mut r.cells) {
                    normalize_blocks(&mut cell.blocks);
                }
            }
            _ => {}
        }
    }
}

/// Inline looks a toggle flips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flag {
    Bold,
    Italic,
}

/// What applying, undoing, or redoing changed: blocks `[start, start+removed)` of the
/// document before became `[start, start+inserted)` after (every block whose content or
/// looks changed is inside), and where the caret goes; `anchor` is `Some` when the result
/// is a selection (anchor..caret), e.g. after restyling a selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Change {
    pub start: usize,
    pub removed: usize,
    pub inserted: usize,
    pub caret: Pos,
    pub anchor: Option<Pos>,
}

impl Change {
    /// Widen the change to cover block `index`, of the document after the change when
    /// `after`, else of the document before.
    fn widen(mut self, index: usize, after: bool) -> Change {
        let (here, there) = if after {
            (&mut self.inserted, &mut self.removed)
        } else {
            (&mut self.removed, &mut self.inserted)
        };
        if index < self.start {
            let more = self.start - index;
            *here += more;
            *there += more;
            self.start = index;
        } else if index >= self.start + *here {
            let more = index + 1 - (self.start + *here);
            *here += more;
            *there += more;
        }
        self
    }
}

/// One undoable change to a document.
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    /// A stretch of text replaced by other text: typing, deleting, Enter, joining
    /// paragraphs, bold and italic.
    Text(TextEdit),
    /// The looks of whole blocks: paragraph style, list kind and level.
    Looks(LooksEdit),
}

impl Edit {
    fn apply(&self, doc: &mut Document) -> Change {
        match self {
            Edit::Text(e) => e.apply(doc),
            Edit::Looks(e) => e.apply(doc),
        }
    }

    fn revert(&self, doc: &mut Document) -> Change {
        match self {
            Edit::Text(e) => e.revert(doc),
            Edit::Looks(e) => e.revert(doc),
        }
    }

    /// Whether the edit may change which blocks form a list, so ordered labels need
    /// renumbering.
    fn restructures(&self) -> bool {
        match self {
            Edit::Text(e) => e.restructures(),
            Edit::Looks(_) => true,
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

impl From<LooksEdit> for Edit {
    fn from(e: LooksEdit) -> Edit {
        Edit::Looks(e)
    }
}

/// An edit as applied: the edit and the ordered-list labels it renumbered after it.
#[derive(Debug)]
struct Step {
    edit: Edit,
    relabel: LooksEdit,
}

/// Undo and redo for one document: the steps applied, newest last, and the steps undone.
#[derive(Debug, Default)]
pub struct History {
    done: VecDeque<Step>,
    undone: Vec<Step>,
    /// The next edit starts a step of its own, whatever came before.
    sealed: bool,
}

impl History {
    pub fn new() -> History {
        History::default()
    }

    /// Apply `edit` to `doc` and keep it for undo. Typing straight after typing joins its
    /// step, a word at a time, unless [`History::seal`] was called in between.
    pub fn apply(&mut self, doc: &mut Document, edit: Edit) -> Change {
        let mut change = edit.apply(doc);
        let relabel = if edit.restructures() {
            looks::relabel(doc, change.start..change.start + change.inserted)
        } else {
            LooksEdit::default()
        };
        relabel.set(doc);
        change = relabel.widen(change, true);
        self.undone.clear();
        let joined = !self.sealed
            && relabel.is_empty()
            && self
                .done
                .back_mut()
                .is_some_and(|last| last.relabel.is_empty() && last.edit.absorb(&edit));
        if !joined {
            self.done.push_back(Step { edit, relabel });
            if self.done.len() > HISTORY_STEPS {
                self.done.pop_front();
            }
        }
        self.sealed = false;
        change
    }

    /// Undo the last step.
    pub fn undo(&mut self, doc: &mut Document) -> Option<Change> {
        let step = self.done.pop_back()?;
        step.relabel.unset(doc);
        let change = step.relabel.widen(step.edit.revert(doc), false);
        self.undone.push(step);
        self.sealed = true;
        Some(change)
    }

    /// Redo the last step undone.
    pub fn redo(&mut self, doc: &mut Document) -> Option<Change> {
        let step = self.undone.pop()?;
        let change = step.edit.apply(doc);
        step.relabel.set(doc);
        let change = step.relabel.widen(change, true);
        self.done.push_back(step);
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

// Block content helpers shared by text and looks edits.

pub(crate) fn runs_len(runs: &[Run]) -> usize {
    runs.iter().map(|r| r.text.len()).sum()
}

fn joined(runs: &[Run]) -> Cow<'_, str> {
    match runs {
        [] => Cow::Borrowed(""),
        [run] => Cow::Borrowed(&run.text),
        _ => Cow::Owned(runs.iter().map(|r| r.text.as_str()).collect()),
    }
}

/// A code block's text without its final line break.
pub(crate) fn code_text(text: &str) -> &str {
    text.strip_suffix('\n').unwrap_or(text)
}

/// `p`'s looks without its runs.
pub(crate) fn bare(p: &Paragraph) -> Paragraph {
    Paragraph {
        style: p.style,
        align: p.align,
        indent: p.indent,
        runs: Vec::new(),
    }
}

/// `block`'s looks without its text: a paragraph or list item without runs, a code block
/// holding only its final line break, other blocks whole.
pub(crate) fn shell(block: &Block) -> Block {
    match block {
        Block::Paragraph(p) => Block::Paragraph(bare(p)),
        Block::ListItem(item) => Block::ListItem(ListItem {
            kind: item.kind.clone(),
            level: item.level,
            paragraph: bare(&item.paragraph),
        }),
        Block::CodeBlock(code) => Block::CodeBlock(CodeBlock {
            language: code.language.clone(),
            text: if code.text.ends_with('\n') {
                "\n".to_owned()
            } else {
                String::new()
            },
            indent: code.indent,
        }),
        other => other.clone(),
    }
}

/// The paragraph holding a block's runs, for paragraphs and list items.
pub(crate) fn paragraph(block: &Block) -> Option<&Paragraph> {
    match block {
        Block::Paragraph(p) => Some(p),
        Block::ListItem(item) => Some(&item.paragraph),
        _ => None,
    }
}

pub(crate) fn paragraph_mut(block: &mut Block) -> Option<&mut Paragraph> {
    match block {
        Block::Paragraph(p) => Some(p),
        Block::ListItem(item) => Some(&mut item.paragraph),
        _ => None,
    }
}

/// Take a block's text as runs: a paragraph's or list item's runs (left empty), a code
/// block's text as one plain run (the block keeps it, and its final line break).
pub(crate) fn take_runs(block: &mut Block) -> Vec<Run> {
    match block {
        Block::CodeBlock(code) => code_runs(code),
        _ => paragraph_mut(block).map_or_else(Vec::new, |p| std::mem::take(&mut p.runs)),
    }
}

/// A copy of a block's text as runs (see [`take_runs`]).
pub(crate) fn runs_of(block: &Block) -> Cow<'_, [Run]> {
    match block {
        Block::CodeBlock(code) => Cow::Owned(code_runs(code)),
        _ => paragraph(block).map_or(Cow::Borrowed(&[][..]), |p| Cow::Borrowed(&p.runs)),
    }
}

fn code_runs(code: &CodeBlock) -> Vec<Run> {
    let text = code_text(&code.text);
    if text.is_empty() {
        Vec::new()
    } else {
        vec![Run {
            text: text.to_owned(),
            style: Default::default(),
        }]
    }
}

/// Set a block's text from runs, merging runs that look the same; a code block keeps
/// whether its text ends in a line break and takes the runs' text without looks.
pub(crate) fn set_runs(block: &mut Block, runs: Vec<Run>) {
    match block {
        Block::CodeBlock(code) => {
            let newline = code.text.ends_with('\n');
            let mut text: String = runs.iter().map(|r| r.text.as_str()).collect();
            if newline {
                text.push('\n');
            }
            code.text = text;
        }
        _ => {
            if let Some(p) = paragraph_mut(block) {
                p.runs = runs;
                p.merge_runs();
            }
        }
    }
}
