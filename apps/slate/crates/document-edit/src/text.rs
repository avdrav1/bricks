//! Text edits: typing, deleting, Enter, joining paragraphs, and bold and italic, each a
//! replacement of a stretch of the document's text by other text.

use crate::looks::LooksEdit;
use crate::{
    bare, paragraph, paragraph_mut, runs_len, runs_of, set_runs, shell, take_runs, text, text_len,
    Change, Edit, Flag, Pos,
};
use document_model::{
    Block, Document, ListItem, ListKind, Paragraph, ParagraphStyle, Run, RunStyle,
};

/// Split `runs` at byte `at` of their joined text: `runs` keeps what's before, the
/// returned runs hold what's after.
fn split_off(runs: &mut Vec<Run>, at: usize) -> Vec<Run> {
    let mut start = 0;
    for i in 0..runs.len() {
        let end = start + runs[i].text.len();
        if at < end {
            if at == start {
                return runs.split_off(i);
            }
            let text = runs[i].text.split_off(at - start);
            let style = runs[i].style.clone();
            let mut tail = runs.split_off(i + 1);
            tail.insert(0, Run { text, style });
            return tail;
        }
        start = end;
    }
    Vec::new()
}

/// The runs covering bytes `from..to` of their joined text, cut at both ends.
fn slice(runs: &[Run], from: usize, to: usize) -> Vec<Run> {
    let (mut out, mut start) = (Vec::new(), 0);
    for r in runs {
        if start >= to {
            break;
        }
        let end = start + r.text.len();
        let (a, b) = (from.max(start), to.min(end));
        if a < b {
            out.push(Run {
                text: r.text[a - start..b - start].to_owned(),
                style: r.style.clone(),
            });
        }
        start = end;
    }
    out
}

/// `block`'s looks holding `runs`.
fn with_runs(block: &Block, runs: Vec<Run>) -> Block {
    let mut out = shell(block);
    set_runs(&mut out, runs);
    out
}

/// Text cut from or pasted into a document, as blocks. The first block's runs join the
/// block they're pasted into (its looks don't matter); each later block is a block of its
/// own, the last with its looks and the text from its start, taking the runs after the
/// paste point.
#[derive(Debug, Clone, PartialEq)]
struct Fragment(Vec<Block>);

impl Fragment {
    fn is_empty(&self) -> bool {
        self.0.len() == 1 && runs_len(&runs_of(&self.0[0])) == 0
    }

    /// Where the fragment ends once pasted at `at`.
    fn end(&self, at: Pos) -> Pos {
        let n = self.0.len();
        let len = runs_len(&runs_of(&self.0[n - 1]));
        if n == 1 {
            Pos::new(at.block, at.offset + len)
        } else {
            Pos::new(at.block + n - 1, len)
        }
    }

    /// A copy of `from..to` of `doc`.
    fn copy(doc: &Document, from: Pos, to: Pos) -> Fragment {
        let (a, b) = (&doc.blocks[from.block], &doc.blocks[to.block]);
        if from.block == to.block {
            return Fragment(vec![with_runs(
                a,
                slice(&runs_of(a), from.offset, to.offset),
            )]);
        }
        let mut out = vec![with_runs(a, slice(&runs_of(a), from.offset, usize::MAX))];
        out.extend(doc.blocks[from.block + 1..to.block].iter().cloned());
        out.push(with_runs(b, slice(&runs_of(b), 0, to.offset)));
        Fragment(out)
    }
}

/// Remove `from..to` from `doc`: the block at `from` keeps its looks and takes the runs
/// after `to`; the blocks after it up to `to`'s go.
fn remove(doc: &mut Document, from: Pos, to: Pos) {
    let mut head = take_runs(&mut doc.blocks[from.block]);
    let mut tail = if from.block == to.block {
        split_off(&mut head, to.offset)
    } else {
        let mut last = take_runs(&mut doc.blocks[to.block]);
        split_off(&mut last, to.offset)
    };
    split_off(&mut head, from.offset);
    head.append(&mut tail);
    set_runs(&mut doc.blocks[from.block], head);
    doc.blocks.drain(from.block + 1..=to.block);
}

/// Paste `fragment` at `at`; returns where it ends.
fn paste(doc: &mut Document, at: Pos, fragment: &Fragment) -> Pos {
    let block = &mut doc.blocks[at.block];
    let mut head = take_runs(block);
    let tail = split_off(&mut head, at.offset);
    head.extend(runs_of(&fragment.0[0]).iter().cloned());
    let n = fragment.0.len();
    if n == 1 {
        let offset = runs_len(&head);
        head.extend(tail);
        set_runs(block, head);
        return Pos::new(at.block, offset);
    }
    set_runs(block, head);
    let mut rest = fragment.0[1..].to_vec();
    let last = rest.last_mut().expect("a fragment of more than one block");
    let mut runs = take_runs(last);
    let offset = runs_len(&runs);
    runs.extend(tail);
    set_runs(last, runs);
    doc.blocks.splice(at.block + 1..at.block + 1, rest);
    Pos::new(at.block + n - 1, offset)
}

fn valid(doc: &Document, pos: Pos) -> bool {
    text(doc, pos.block).is_some_and(|t| pos.offset <= t.len() && t.is_char_boundary(pos.offset))
}

fn is_code(doc: &Document, block: usize) -> bool {
    matches!(doc.blocks[block], Block::CodeBlock(_))
}

/// `from..to` in order, when both are in editable blocks and the range doesn't reach out
/// of a code block.
fn range(doc: &Document, from: Pos, to: Pos) -> Option<(Pos, Pos)> {
    let (from, to) = (from.min(to), from.max(to));
    let ok = valid(doc, from)
        && valid(doc, to)
        && (from.block == to.block || !(is_code(doc, from.block) || is_code(doc, to.block)));
    ok.then_some((from, to))
}

/// The look text typed at `at` takes: the character before the caret's, else the
/// paragraph's first character's, else the default look; never a link.
pub fn look(doc: &Document, at: Pos) -> RunStyle {
    let Some(p) = doc.blocks.get(at.block).and_then(paragraph) else {
        return RunStyle::default();
    };
    let mut before = None;
    if at.offset > 0 {
        let mut start = 0;
        for r in &p.runs {
            let end = start + r.text.len();
            if (start..end).contains(&(at.offset - 1)) {
                before = Some(r);
                break;
            }
            start = end;
        }
    }
    let mut style = before
        .or_else(|| p.runs.iter().find(|r| !r.text.is_empty()))
        .map(|r| r.style.clone())
        .unwrap_or_default();
    style.link = None;
    style
}

/// Replacing a stretch of the document's text with other text: typing, deleting, Enter,
/// joining paragraphs, bold and italic. It holds the text it removes and the text it puts
/// in, so it applies and undoes alike.
#[derive(Debug, Clone, PartialEq)]
pub struct TextEdit {
    at: Pos,
    removed: Fragment,
    inserted: Fragment,
    /// Typed text, which typing straight after it may join.
    typed: bool,
    /// The result is the replaced text selected (bold, italic).
    select: bool,
}

impl TextEdit {
    fn new(doc: &Document, from: Pos, to: Pos, inserted: Vec<Block>, typed: bool) -> TextEdit {
        TextEdit {
            at: from,
            removed: Fragment::copy(doc, from, to),
            inserted: Fragment(inserted),
            typed,
            select: false,
        }
    }

    fn change(&self, removed: usize, inserted: usize, caret: Pos) -> Change {
        Change {
            start: self.at.block,
            removed,
            inserted,
            caret,
            anchor: self.select.then_some(self.at),
        }
    }

    pub(crate) fn apply(&self, doc: &mut Document) -> Change {
        remove(doc, self.at, self.removed.end(self.at));
        let caret = paste(doc, self.at, &self.inserted);
        self.change(self.removed.0.len(), self.inserted.0.len(), caret)
    }

    pub(crate) fn revert(&self, doc: &mut Document) -> Change {
        remove(doc, self.at, self.inserted.end(self.at));
        let caret = paste(doc, self.at, &self.removed);
        self.change(self.inserted.0.len(), self.removed.0.len(), caret)
    }

    /// Whether blocks come or go.
    pub(crate) fn restructures(&self) -> bool {
        self.removed.0.len() != self.inserted.0.len()
    }

    /// Join `next`, typed straight after this typing, a word at a time: a word typed
    /// after whitespace starts a step of its own.
    pub(crate) fn absorb(&mut self, next: &TextEdit) -> bool {
        let continues = self.typed
            && next.typed
            && self.removed.is_empty()
            && next.removed.is_empty()
            && self.inserted.0.len() == 1
            && next.inserted.0.len() == 1
            && next.at == self.inserted.end(self.at);
        if !continues {
            return false;
        }
        let ends_in_space = runs_of(&self.inserted.0[0])
            .last()
            .and_then(|r| r.text.chars().next_back())
            .is_some_and(char::is_whitespace);
        let starts_word = runs_of(&next.inserted.0[0])
            .first()
            .and_then(|r| r.text.chars().next())
            .is_some_and(|c| !c.is_whitespace());
        if ends_in_space && starts_word {
            return false;
        }
        let block = &mut self.inserted.0[0];
        let mut runs = take_runs(block);
        runs.extend(runs_of(&next.inserted.0[0]).iter().cloned());
        set_runs(block, runs);
        true
    }
}

/// Typing `text` over `from..to` (the caret when they're equal) in `style`; empty `text`
/// deletes. In a code block `style` is ignored.
pub fn replace(doc: &Document, from: Pos, to: Pos, text: &str, style: &RunStyle) -> Option<Edit> {
    let (from, to) = range(doc, from, to)?;
    if from == to && text.is_empty() {
        return None;
    }
    let block = &doc.blocks[from.block];
    let mut first = shell(block);
    if !text.is_empty() {
        let style = if is_code(doc, from.block) {
            RunStyle::default()
        } else {
            style.clone()
        };
        set_runs(
            &mut first,
            vec![Run {
                text: text.to_owned(),
                style,
            }],
        );
    }
    let typed = !text.is_empty();
    Some(TextEdit::new(doc, from, to, vec![first], typed).into())
}

/// Enter: remove `from..to`, then split the block at the caret. In a code block it's a
/// line break; an empty list item leaves the list instead.
pub fn split(doc: &Document, from: Pos, to: Pos) -> Option<Edit> {
    let (from, to) = range(doc, from, to)?;
    let block = &doc.blocks[from.block];
    let new = match block {
        Block::CodeBlock(_) => {
            let first = with_runs(
                block,
                vec![Run {
                    text: "\n".to_owned(),
                    style: RunStyle::default(),
                }],
            );
            return Some(TextEdit::new(doc, from, to, vec![first], false).into());
        }
        Block::ListItem(item) if from == to && runs_len(&item.paragraph.runs) == 0 => {
            let after = Block::Paragraph(Paragraph {
                style: ParagraphStyle::Normal,
                ..bare(&item.paragraph)
            });
            return Some(
                LooksEdit::new(vec![(from.block, shell(block), after)], Some(from)).into(),
            );
        }
        Block::ListItem(item) => Block::ListItem(ListItem {
            kind: match &item.kind {
                ListKind::Task { .. } => ListKind::Task { checked: false },
                kind => kind.clone(),
            },
            level: item.level,
            paragraph: bare(&item.paragraph),
        }),
        Block::Paragraph(p) => {
            let at_end = Some(to.offset) == text_len(doc, to.block);
            let style = match p.style {
                ParagraphStyle::Heading(_) | ParagraphStyle::Title if at_end => {
                    ParagraphStyle::Normal
                }
                style => style,
            };
            Block::Paragraph(Paragraph { style, ..bare(p) })
        }
        _ => return None,
    };
    Some(TextEdit::new(doc, from, to, vec![shell(block), new], false).into())
}

fn joinable(block: &Block) -> bool {
    matches!(block, Block::Paragraph(_) | Block::ListItem(_))
}

/// Backspace at the start of a block: a list item leaves its list; a paragraph joins the
/// paragraph or list item before it.
pub fn join_backward(doc: &Document, at: Pos) -> Option<Edit> {
    if at.offset != 0 || !valid(doc, at) {
        return None;
    }
    match &doc.blocks[at.block] {
        block @ Block::ListItem(item) => {
            let after = Block::Paragraph(bare(&item.paragraph));
            Some(LooksEdit::new(vec![(at.block, shell(block), after)], Some(at)).into())
        }
        Block::Paragraph(_) => {
            let prev = at.block.checked_sub(1)?;
            if !joinable(&doc.blocks[prev]) {
                return None;
            }
            let end = Pos::new(prev, text_len(doc, prev)?);
            let first = shell(&doc.blocks[prev]);
            Some(TextEdit::new(doc, end, at, vec![first], false).into())
        }
        _ => None,
    }
}

/// Delete at the end of a paragraph or list item: the paragraph or list item after it
/// joins it.
pub fn join_forward(doc: &Document, at: Pos) -> Option<Edit> {
    if !valid(doc, at) || Some(at.offset) != text_len(doc, at.block) {
        return None;
    }
    let block = &doc.blocks[at.block];
    let next = doc.blocks.get(at.block + 1)?;
    if !joinable(block) || !joinable(next) {
        return None;
    }
    let first = shell(block);
    Some(TextEdit::new(doc, at, Pos::new(at.block + 1, 0), vec![first], false).into())
}

fn flag(style: &mut RunStyle, flag: Flag) -> &mut bool {
    match flag {
        Flag::Bold => &mut style.bold,
        Flag::Italic => &mut style.italic,
    }
}

/// Bold or italic over `from..to` of paragraphs' and list items' text: cleared when every
/// character already has it, else set everywhere. The range stays selected.
pub fn toggle(doc: &Document, from: Pos, to: Pos, which: Flag) -> Option<Edit> {
    let (from, to) = range(doc, from, to)?;
    if from == to {
        return None;
    }
    let removed = Fragment::copy(doc, from, to);
    let mut inserted = removed.clone();
    let (mut any, mut all) = (false, true);
    for block in &mut inserted.0 {
        for run in paragraph_mut(block).into_iter().flat_map(|p| &mut p.runs) {
            any = true;
            all &= *flag(&mut run.style, which);
        }
    }
    if !any {
        return None;
    }
    for block in &mut inserted.0 {
        if let Some(p) = paragraph_mut(block) {
            for run in &mut p.runs {
                *flag(&mut run.style, which) = !all;
            }
            p.merge_runs();
        }
    }
    Some(
        TextEdit {
            at: from,
            removed,
            inserted,
            typed: false,
            select: true,
        }
        .into(),
    )
}
