//! Slate's edits: every change to a document is a command that stores the operation and
//! what undoes it, never a snapshot of the document. A text edit replaces a stretch of
//! text with other text and holds both (typing, deleting, and run looks: bold, italic,
//! underline, strikethrough, font, size, colour); a looks edit changes the looks of whole
//! blocks (paragraph style, alignment, list membership, list level, list labels) and holds
//! each block's looks before and after. [`History`] keeps the steps for undo and redo.
//! [`looks()`] summarises the looks at the caret or across a selection for the format bar.
//!
//! Edits work in a [`Container`]: the document's blocks, or a table cell's. An edit stays
//! inside its container, and undo and redo act on the same one.
//!
//! Paragraphs stay normalized ([`normalize`]): neighbouring runs that look the same are
//! one run and no run is empty, so undoing an edit gives back the document exactly.
//!
//! A selection can be copied as a [`Fragment`], deleted, or pasted over ([`copy`],
//! [`delete`], [`paste`]), each one step; one reaching into or out of a table cell
//! replaces whole top-level blocks ([`BlocksEdit`]).
//!
//! Toolkit-free: no GTK, no I/O.

mod clipboard;
mod find;
mod format;
mod looks;
mod text;

pub use clipboard::{copy, delete, paste, replace_document, BlocksEdit, Fragment};
pub use find::{find, replace_all, replace_one, Match};
pub use format::{format, formatted, looks, LookPatch, Looks, SizeChange, MAX_SIZE, MIN_SIZE};
pub use looks::{indent, set_align, set_style, toggle_list, LooksEdit};
pub use text::{join_backward, join_forward, look, replace, split, toggle, toggled, TextEdit};

use document_model::{Block, CodeBlock, Document, ListItem, Paragraph, Run};
use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::VecDeque;
use std::ops::Range;

/// Steps kept for undo; older ones are forgotten.
pub const HISTORY_STEPS: usize = 1_000;

/// A list of blocks edits work in: the document's, or a cell's of the table at top-level
/// block `table` (tables inside cells aren't containers: their cells aren't editable).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Container {
    #[default]
    Top,
    Cell {
        table: usize,
        row: usize,
        cell: usize,
    },
}

/// A place in the document: a container, a block in it (index into the container's
/// blocks), and a byte offset into the block's text, on a char boundary. Text of
/// Paragraph = runs joined; ListItem = its paragraph's runs joined; CodeBlock = `text`
/// without its final "\n". Places order as they come in the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Pos {
    pub at: Container,
    pub block: usize,
    pub offset: usize,
}

impl Pos {
    /// A place in the document's blocks.
    pub fn new(block: usize, offset: usize) -> Pos {
        Pos {
            at: Container::Top,
            block,
            offset,
        }
    }

    /// A place in a cell of the table at top-level block `table`.
    pub fn in_cell(table: usize, row: usize, cell: usize, block: usize, offset: usize) -> Pos {
        Pos {
            at: Container::Cell { table, row, cell },
            block,
            offset,
        }
    }

    /// The key ordering places in the document: the top-level block, then the place
    /// inside its table (none for a top-level place), then the offset.
    fn key(&self) -> (usize, Option<(usize, usize, usize)>, usize) {
        match self.at {
            Container::Top => (self.block, None, self.offset),
            Container::Cell { table, row, cell } => {
                (table, Some((row, cell, self.block)), self.offset)
            }
        }
    }

    /// The same container and block at another offset.
    pub(crate) fn at_offset(self, offset: usize) -> Pos {
        Pos { offset, ..self }
    }

    /// The same container at another block and offset.
    pub(crate) fn moved(self, block: usize, offset: usize) -> Pos {
        Pos {
            at: self.at,
            block,
            offset,
        }
    }
}

impl Ord for Pos {
    fn cmp(&self, other: &Pos) -> Ordering {
        self.key().cmp(&other.key())
    }
}

impl PartialOrd for Pos {
    fn partial_cmp(&self, other: &Pos) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The blocks of `at`; `None` if `at` isn't a cell of a top-level table.
pub fn blocks(doc: &Document, at: Container) -> Option<&[Block]> {
    match at {
        Container::Top => Some(&doc.blocks),
        Container::Cell { table, row, cell } => match doc.blocks.get(table)? {
            Block::Table(t) => Some(&t.rows.get(row)?.cells.get(cell)?.blocks),
            _ => None,
        },
    }
}

pub(crate) fn blocks_mut(doc: &mut Document, at: Container) -> Option<&mut Vec<Block>> {
    match at {
        Container::Top => Some(&mut doc.blocks),
        Container::Cell { table, row, cell } => match doc.blocks.get_mut(table)? {
            Block::Table(t) => Some(&mut t.rows.get_mut(row)?.cells.get_mut(cell)?.blocks),
            _ => None,
        },
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

/// The text length in bytes (see [`Pos`]) of block `block` of `at`; `None` if not
/// editable or out of range.
pub fn text_len(doc: &Document, at: Container, block: usize) -> Option<usize> {
    block_len(blocks(doc, at)?.get(block)?)
}

/// The text (see [`Pos`]) of block `block` of `at`, for editable blocks.
pub fn text(doc: &Document, at: Container, block: usize) -> Option<Cow<'_, str>> {
    block_text(blocks(doc, at)?.get(block)?)
}

pub(crate) fn block_len(block: &Block) -> Option<usize> {
    match block {
        Block::Paragraph(p) => Some(runs_len(&p.runs)),
        Block::ListItem(item) => Some(runs_len(&item.paragraph.runs)),
        Block::CodeBlock(code) => Some(code_text(&code.text).len()),
        _ => None,
    }
}

pub(crate) fn block_text(block: &Block) -> Option<Cow<'_, str>> {
    match block {
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

pub(crate) fn normalize_blocks(blocks: &mut [Block]) {
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
    Underline,
    Strike,
}

/// What applying, undoing, or redoing changed: blocks `[start, start+removed)` of
/// container `at` before became `[start, start+inserted)` after (every block whose content
/// or looks changed is inside), and where the caret goes; `anchor` is `Some` when the
/// result is a selection (anchor..caret), e.g. after restyling a selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Change {
    pub at: Container,
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
    /// paragraphs, run looks (bold, italic, underline, strikethrough, font, size, colour).
    Text(TextEdit),
    /// The looks of whole blocks: paragraph style, alignment, list kind and level.
    Looks(LooksEdit),
    /// Whole top-level blocks replaced: a selection reaching into or out of a table cell
    /// deleted or pasted over.
    Blocks(BlocksEdit),
}

impl Edit {
    fn apply(&self, doc: &mut Document) -> Change {
        match self {
            Edit::Text(e) => e.apply(doc),
            Edit::Looks(e) => e.apply(doc),
            Edit::Blocks(e) => e.apply(doc),
        }
    }

    fn revert(&self, doc: &mut Document) -> Change {
        match self {
            Edit::Text(e) => e.revert(doc),
            Edit::Looks(e) => e.revert(doc),
            Edit::Blocks(e) => e.revert(doc),
        }
    }

    /// What applying (`forward`) or undoing the edit alone changes, before renumbering.
    fn shape(&self, forward: bool) -> Change {
        match self {
            Edit::Text(e) => e.shape(forward),
            Edit::Looks(e) => e.shape(),
            Edit::Blocks(e) => e.shape(forward),
        }
    }

    /// Block `i` of the edit's container once applied to `old`, its blocks now, as far as
    /// its looks go, for `i` among the blocks it puts in.
    fn looks_after<'a>(&'a self, old: &'a [Block], i: usize) -> Option<&'a Block> {
        match self {
            Edit::Text(e) => e.looks_after(old, i),
            Edit::Looks(e) => e.looks_after(old, i),
            Edit::Blocks(e) => e.looks_after(i),
        }
    }

    /// The blocks applying this edit to `doc` (as it is now) replaces: its container and
    /// the range of block indices in it, exactly the `at` and `start..start + removed` of
    /// the [`Change`] that [`History::apply`] returns for it, ordered-list renumbering
    /// included. The document isn't touched: renumbering runs over the blocks as they
    /// will be.
    pub fn span(&self, doc: &Document) -> (Container, Range<usize>) {
        let mut change = self.shape(true);
        if self.restructures() {
            let old = blocks(doc, change.at).unwrap_or_default();
            let Change {
                start,
                removed,
                inserted,
                ..
            } = change;
            let get = |i: usize| {
                if i < start {
                    old.get(i)
                } else if i < start + inserted {
                    self.looks_after(old, i)
                } else {
                    old.get(i + removed - inserted)
                }
            };
            let n = (old.len() + inserted).saturating_sub(removed);
            looks::renumber(n, get, start..start + inserted, |i, _, _| {
                change = change.widen(i, true);
            });
        }
        (change.at, change.start..change.start + change.removed)
    }

    /// Whether the edit may change which blocks form a list, so ordered labels need
    /// renumbering.
    fn restructures(&self) -> bool {
        match self {
            Edit::Text(e) => e.restructures(),
            Edit::Looks(_) | Edit::Blocks(_) => true,
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

impl From<BlocksEdit> for Edit {
    fn from(e: BlocksEdit) -> Edit {
        Edit::Blocks(e)
    }
}

/// An edit as applied: the edit, the ordered-list labels it renumbered after it, and the
/// id of the document it leaves (see [`History::position`]).
#[derive(Debug)]
struct Step {
    edit: Edit,
    relabel: LooksEdit,
    id: u64,
}

/// Undo and redo for one document: the steps applied, newest last, and the steps undone.
#[derive(Debug, Default)]
pub struct History {
    done: VecDeque<Step>,
    undone: Vec<Step>,
    /// The next edit starts a step of its own, whatever came before.
    sealed: bool,
    /// The last id given out.
    ids: u64,
    /// The position with nothing left to undo: the document as opened (0), or as the
    /// oldest step dropped past [`HISTORY_STEPS`] left it.
    base: u64,
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
            looks::relabel(doc, change.at, change.start..change.start + change.inserted)
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
        if joined {
            // The joined step leaves another document than it did.
            self.ids += 1;
            if let Some(last) = self.done.back_mut() {
                last.id = self.ids;
            }
        } else {
            self.ids += 1;
            self.done.push_back(Step {
                edit,
                relabel,
                id: self.ids,
            });
            if self.done.len() > HISTORY_STEPS {
                if let Some(oldest) = self.done.pop_front() {
                    self.base = oldest.id;
                }
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

    /// The blocks the next undo (`redo` false) or redo replaces in the document as it is
    /// now, as [`Edit::span`] gives them; `None` when there is nothing to undo or redo.
    pub fn next_span(&self, redo: bool) -> Option<(Container, Range<usize>)> {
        let change = if redo {
            let step = self.undone.last()?;
            step.relabel.widen(step.edit.shape(true), true)
        } else {
            let step = self.done.back()?;
            step.relabel.widen(step.edit.shape(false), false)
        };
        Some((change.at, change.start..change.start + change.removed))
    }

    /// End the current step: the next edit won't join it (the caret moved, ...).
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// Which document the history is at: the same id means the same document, so a
    /// window compares it with the position it last saved at. Undoing back to a position
    /// gives its id again; any new edit, typing joined to a step included, gives a new one.
    pub fn position(&self) -> u64 {
        self.done.back().map_or(self.base, |s| s.id)
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
        style: p.style.clone(),
        align: p.align,
        indent: p.indent,
        runs: Vec::new(),
        tabs: p.tabs.clone(),
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
