//! Selection and clipboard: copying, deleting, and pasting a stretch of the document that
//! may cross paragraphs, list items, and tables.
//!
//! Within one container a selection is a stretch of text, as typing over it removes. A
//! selection reaching into or out of a table cell works on the document's blocks instead:
//! a table it reaches into goes whole, and the blocks at its ends keep what's outside it
//! without joining.

use crate::text::{paste as paste_into, range, slice, valid, with_runs, TextEdit};
use crate::{
    block_text, blocks, blocks_mut, is_editable, normalize_blocks, replace, runs_of, Change,
    Container, Edit, Pos,
};
use document_model::{Block, Document, Paragraph, RunStyle, StyleSheet};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Text cut or copied from a document, as a paste puts it in: the first block's runs join the
/// paragraph pasted into, each later block is a block of its own, and the last one's runs are
/// followed by the text after the paste point (Frame's and the internal text-edit semantics).
/// A fragment whose content starts with a table starts with an empty paragraph, so blocks[0]
/// is always a Paragraph or ListItem (or a CodeBlock, copied from inside one); same for the
/// end.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fragment {
    pub blocks: Vec<Block>,
    /// The names of the paragraph styles its paragraphs name, by id: a paste maps them to
    /// the styles of those names in the document pasted into. Empty in an edit's own
    /// fragments, whose ids are the document's.
    pub names: BTreeMap<String, String>,
}

impl Fragment {
    /// The clipboard type Slate offers a fragment as, beside plain text.
    pub const MIME: &'static str = "application/x-slate-fragment";

    /// A fragment of `blocks`, whose paragraphs name the document's styles.
    pub fn new(blocks: Vec<Block>) -> Fragment {
        Fragment {
            blocks,
            names: BTreeMap::new(),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a fragment serializes")
    }

    /// The fragment `s` holds; `None` if it isn't one or holds no blocks.
    pub fn from_json(s: &str) -> Option<Fragment> {
        let fragment: Fragment = serde_json::from_str(s).ok()?;
        (!fragment.blocks.is_empty()).then_some(fragment)
    }

    /// The fragment with the names of its paragraphs' styles in `styles`.
    fn named(mut self, styles: &StyleSheet) -> Fragment {
        let mut names = BTreeMap::new();
        visit_paragraphs(&mut self.blocks, &mut |p| {
            names
                .entry(p.style.clone())
                .or_insert_with(|| styles.style_of(&p.style).name.clone());
        });
        self.names = names;
        self
    }

    /// Give `blocks` (this fragment's) the styles of `styles` its paragraphs' style names
    /// name: the style of that name, else for a built-in heading, title, or quote its
    /// built-in id (which the document draws, and a Word document's save adds), else the
    /// default. Without names (plain text), a paragraph naming no style gets the default.
    fn restyle(&self, blocks: &mut [Block], styles: &StyleSheet) {
        visit_paragraphs(blocks, &mut |p| {
            let Some(name) = self.names.get(&p.style) else {
                if self.names.is_empty() && !p.style.is_empty() {
                    return;
                }
                p.style = styles.default_id().to_owned();
                return;
            };
            p.style = match styles.by_name(name) {
                Some(s) => s.id.clone(),
                None => match StyleSheet::builtin_style(&p.style) {
                    Some(b) if b.name.eq_ignore_ascii_case(name) => b.id.clone(),
                    _ => styles.default_id().to_owned(),
                },
            };
        });
    }

    /// Plain text for other apps: each paragraph's text on its own line ("\n" between
    /// blocks); a table's cells each on a line of their own in row order; a picture its
    /// alt text; rules and page breaks nothing (no line). The empty paragraphs around a
    /// table at the fragment's ends give no line.
    pub fn plain_text(&self) -> String {
        let mut blocks = &self.blocks[..];
        if let [first, Block::Table(_), ..] = blocks {
            if is_blank(first) {
                blocks = &blocks[1..];
            }
        }
        if let [.., Block::Table(_), last] = blocks {
            if is_blank(last) {
                blocks = &blocks[..blocks.len() - 1];
            }
        }
        let mut lines = Vec::new();
        lines_of(blocks, &mut lines);
        lines.join("\n")
    }

    /// Plain text pasted from another app: one paragraph per line (split on \n, \r\n),
    /// naming no style (a paste gives it the document's default), every run in `style`.
    pub fn from_plain(text: &str, style: &RunStyle) -> Fragment {
        let blocks = text
            .split('\n')
            .map(|line| {
                let mut p = Paragraph::default();
                p.push(line.strip_suffix('\r').unwrap_or(line), style);
                Block::Paragraph(p)
            })
            .collect();
        Fragment::new(blocks)
    }

    /// The fragment as a paste into `dest`, a block of `at`, puts it in: into a code block
    /// its plain text; into a cell with tables flattened into their cells' blocks; with
    /// text-holding blocks at both ends and runs normalized.
    fn ready(&self, styles: &StyleSheet, at: Container, dest: &Block) -> Fragment {
        if matches!(dest, Block::CodeBlock(_)) {
            let mut p = Paragraph::default();
            p.push(&self.plain_text(), &RunStyle::default());
            return Fragment::new(vec![Block::Paragraph(p)]);
        }
        let mut blocks = match at {
            Container::Top => self.blocks.clone(),
            Container::Cell { .. } => {
                let mut flat = Vec::with_capacity(self.blocks.len());
                flatten(&self.blocks, &mut flat);
                flat
            }
        };
        if !blocks.first().is_some_and(is_editable) {
            blocks.insert(0, Block::Paragraph(Paragraph::default()));
        }
        let last = blocks.last().expect("a first block");
        let holds_tail = matches!(last, Block::Paragraph(_) | Block::ListItem(_));
        if blocks.len() > 1 && !holds_tail {
            blocks.push(Block::Paragraph(Paragraph::default()));
        }
        self.restyle(&mut blocks, styles);
        normalize_blocks(&mut blocks);
        Fragment::new(blocks)
    }
}

/// Call `f` with every paragraph of `blocks`, list items' and table cells' included.
fn visit_paragraphs(blocks: &mut [Block], f: &mut dyn FnMut(&mut Paragraph)) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => f(p),
            Block::ListItem(item) => f(&mut item.paragraph),
            Block::Table(t) => {
                for cell in t.rows.iter_mut().flat_map(|r| &mut r.cells) {
                    visit_paragraphs(&mut cell.blocks, f);
                }
            }
            _ => {}
        }
    }
}

/// An empty paragraph naming no style, as a fragment has around a table at its ends.
fn is_blank(block: &Block) -> bool {
    matches!(block, Block::Paragraph(p) if *p == Paragraph::default())
}

fn lines_of(blocks: &[Block], lines: &mut Vec<String>) {
    for block in blocks {
        match block {
            Block::Table(table) => {
                for cell in table.rows.iter().flat_map(|r| &r.cells) {
                    if cell.blocks.is_empty() {
                        lines.push(String::new());
                    } else {
                        lines_of(&cell.blocks, lines);
                    }
                }
            }
            Block::Image(image) => lines.push(image.alt.clone()),
            Block::Raw(raw) => lines.push(raw.source.trim_end_matches('\n').to_owned()),
            Block::PageBreak | Block::Rule => {}
            _ => lines.extend(block_text(block).map(|t| t.into_owned())),
        }
    }
}

/// `blocks` with every table replaced by its cells' blocks, row by row.
fn flatten(blocks: &[Block], out: &mut Vec<Block>) {
    for block in blocks {
        match block {
            Block::Table(table) => {
                for cell in table.rows.iter().flat_map(|r| &r.cells) {
                    flatten(&cell.blocks, out);
                }
            }
            other => out.push(other.clone()),
        }
    }
}

/// A selection whose ends are in different containers, as top-level blocks
/// `start..=end`: `head` is the start's offset when it's in a top-level block (that
/// block's text from it on is selected), `None` when it's in a cell of table `start`
/// (the whole table is); `tail` likewise for the end.
struct Span {
    start: usize,
    end: usize,
    head: Option<usize>,
    tail: Option<usize>,
    /// The selection's end, where undo puts the caret.
    to: Pos,
}

impl Span {
    fn new(doc: &Document, s: Pos, e: Pos) -> Option<Span> {
        let place = |p: Pos| -> Option<(usize, Option<usize>)> {
            valid(blocks(doc, p.at)?, p).then_some(())?;
            Some(match p.at {
                Container::Top => (p.block, Some(p.offset)),
                Container::Cell { table, .. } => (table, None),
            })
        };
        let ((start, head), (end, tail)) = (place(s)?, place(e)?);
        Some(Span {
            start,
            end,
            head,
            tail,
            to: e,
        })
    }

    fn copy(&self, doc: &Document) -> Fragment {
        let top = &doc.blocks;
        let blank = || Block::Paragraph(Paragraph::default());
        let mut out = Vec::new();
        match self.head {
            Some(offset) => {
                let b = &top[self.start];
                out.push(with_runs(b, slice(&runs_of(b), offset, usize::MAX)));
            }
            None => out.extend([blank(), top[self.start].clone()]),
        }
        if self.end > self.start {
            out.extend(top[self.start + 1..self.end].iter().cloned());
            match self.tail {
                Some(offset) => {
                    let b = &top[self.end];
                    out.push(with_runs(b, slice(&runs_of(b), 0, offset)));
                }
                None => out.extend([top[self.end].clone(), blank()]),
            }
        } else {
            out.push(blank());
        }
        Fragment::new(out)
    }

    /// What's left of blocks `start..=end` once the selection goes, one block, and where
    /// the caret goes in it. Only one end is in a top-level block (the ends' containers
    /// differ); with both in cells an empty paragraph takes the tables' place.
    fn kept(&self, doc: &Document) -> (Block, usize) {
        let top = &doc.blocks;
        match (self.head, self.tail) {
            (Some(offset), _) => {
                let b = &top[self.start];
                (with_runs(b, slice(&runs_of(b), 0, offset)), offset)
            }
            (None, Some(offset)) => {
                let b = &top[self.end];
                (with_runs(b, slice(&runs_of(b), offset, usize::MAX)), 0)
            }
            (None, None) => (Block::Paragraph(Paragraph::default()), 0),
        }
    }

    /// Replacing blocks `start..=end` with `inserted`, the caret going to `caret`.
    fn edit(&self, doc: &Document, inserted: Vec<Block>, caret: Pos) -> Edit {
        BlocksEdit {
            start: self.start,
            removed: doc.blocks[self.start..=self.end].to_vec(),
            inserted,
            caret,
            undo_caret: self.to,
        }
        .into()
    }
}

/// Replacing whole top-level blocks with others: a selection deleted or pasted over when
/// its ends are in different containers. It holds the blocks it touches before and
/// after, never the rest of the document.
#[derive(Debug, Clone, PartialEq)]
pub struct BlocksEdit {
    start: usize,
    removed: Vec<Block>,
    inserted: Vec<Block>,
    /// Where the caret goes after applying, and after undoing.
    caret: Pos,
    undo_caret: Pos,
}

impl BlocksEdit {
    /// Replacing top-level blocks from `start` on (`removed`, as they are) with
    /// `inserted`; the caret goes to `caret`, and to `undo_caret` when undone.
    pub(crate) fn new(
        start: usize,
        removed: Vec<Block>,
        inserted: Vec<Block>,
        caret: Pos,
        undo_caret: Pos,
    ) -> BlocksEdit {
        BlocksEdit {
            start,
            removed,
            inserted,
            caret,
            undo_caret,
        }
    }

    /// What applying (`forward`) or undoing changes.
    pub(crate) fn shape(&self, forward: bool) -> Change {
        let (removed, inserted, caret) = if forward {
            (&self.removed, &self.inserted, self.caret)
        } else {
            (&self.inserted, &self.removed, self.undo_caret)
        };
        Change {
            at: Container::Top,
            start: self.start,
            removed: removed.len(),
            inserted: inserted.len(),
            caret,
            anchor: None,
        }
    }

    /// Block `i` of the document's blocks once applied, for `i` among the blocks it puts
    /// in.
    pub(crate) fn looks_after(&self, i: usize) -> Option<&Block> {
        self.inserted.get(i.checked_sub(self.start)?)
    }

    fn splice(&self, doc: &mut Document, forward: bool) -> Change {
        let (old, new) = if forward {
            (&self.removed, &self.inserted)
        } else {
            (&self.inserted, &self.removed)
        };
        let top = blocks_mut(doc, Container::Top).expect("the document's blocks");
        top.splice(self.start..self.start + old.len(), new.iter().cloned());
        self.shape(forward)
    }

    pub(crate) fn apply(&self, doc: &mut Document) -> Change {
        self.splice(doc, true)
    }

    pub(crate) fn revert(&self, doc: &mut Document) -> Change {
        self.splice(doc, false)
    }
}

/// The whole document replaced by `blocks` as one step, as crash recovery puts unsaved edits
/// back onto the file they were made to: undoing it gives the file's blocks back. The caret
/// goes to the start either way. `None` when `blocks` are the document's already.
pub fn replace_document(doc: &Document, blocks: Vec<Block>) -> Option<Edit> {
    if doc.blocks == blocks {
        return None;
    }
    let start = Pos::default();
    Some(Edit::Blocks(BlocksEdit::new(
        0,
        doc.blocks.clone(),
        blocks,
        start,
        start,
    )))
}

/// `from` and `to` in document order, `None` when they're equal.
fn ordered(from: Pos, to: Pos) -> Option<(Pos, Pos)> {
    (from != to).then(|| (from.min(to), from.max(to)))
}

/// The fragment from `from` to `to` (either order), with its paragraphs' style names;
/// `None` when they're equal or not in text.
pub fn copy(doc: &Document, from: Pos, to: Pos) -> Option<Fragment> {
    let (s, e) = ordered(from, to)?;
    let fragment = if s.at == e.at {
        let (blocks, s, e) = range(doc, s, e)?;
        Fragment::copy(blocks, s, e)
    } else {
        Span::new(doc, s, e)?.copy(doc)
    };
    Some(fragment.named(&doc.styles))
}

/// Delete from `from` to `to` (either order) as one undoable step.
pub fn delete(doc: &Document, from: Pos, to: Pos) -> Option<Edit> {
    let (s, e) = ordered(from, to)?;
    if s.at == e.at {
        return replace(doc, s, e, "", &RunStyle::default());
    }
    let span = Span::new(doc, s, e)?;
    let (kept, offset) = span.kept(doc);
    Some(span.edit(doc, vec![kept], Pos::new(span.start, offset)))
}

/// Replace `from..to` (either order; equal for a caret) with `fragment`, as one undoable
/// step: its paragraphs take the document's styles of their styles' names. The caret goes
/// to the end of the pasted text.
pub fn paste(doc: &Document, from: Pos, to: Pos, fragment: &Fragment) -> Option<Edit> {
    let (s, e) = (from.min(to), from.max(to));
    if s.at == e.at {
        let (blocks, s, e) = range(doc, s, e)?;
        let fragment = fragment.ready(&doc.styles, s.at, &blocks[s.block]);
        if s == e && fragment.is_empty() {
            return None;
        }
        return Some(TextEdit::paste(blocks, s, e, fragment, false).into());
    }
    let span = Span::new(doc, s, e)?;
    let (kept, offset) = span.kept(doc);
    let fragment = fragment.ready(&doc.styles, Container::Top, &kept);
    let mut inserted = vec![kept];
    let end = paste_into(&mut inserted, Pos::new(0, offset), &fragment);
    let caret = Pos::new(span.start + end.block, end.offset);
    Some(span.edit(doc, inserted, caret))
}
