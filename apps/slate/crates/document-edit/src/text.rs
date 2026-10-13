//! Text edits: typing, deleting, Enter, joining paragraphs, and run looks (bold, italic,
//! underline, strikethrough, font, size, colour), each a replacement of a stretch of the
//! document's text by other text.

use crate::looks::LooksEdit;
use crate::{
    bare, block_len, block_text, blocks, blocks_mut, paragraph, paragraph_mut, runs_len, runs_of,
    set_runs, shell, take_runs, Change, Edit, Flag, Fragment, Pos,
};
use document_model::{Block, Document, ListItem, ListKind, Paragraph, Run, RunStyle};
use std::ops::Range;

/// Split `runs` at byte `at` of their joined text: `runs` keeps what's before, the
/// returned runs hold what's after.
pub(crate) fn split_off(runs: &mut Vec<Run>, at: usize) -> Vec<Run> {
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
pub(crate) fn slice(runs: &[Run], from: usize, to: usize) -> Vec<Run> {
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
pub(crate) fn with_runs(block: &Block, runs: Vec<Run>) -> Block {
    let mut out = shell(block);
    set_runs(&mut out, runs);
    out
}

impl Fragment {
    pub(crate) fn is_empty(&self) -> bool {
        self.blocks.len() == 1 && runs_len(&runs_of(&self.blocks[0])) == 0
    }

    /// Where the fragment ends once pasted at `at`.
    fn end(&self, at: Pos) -> Pos {
        let n = self.blocks.len();
        let len = runs_len(&runs_of(&self.blocks[n - 1]));
        if n == 1 {
            at.at_offset(at.offset + len)
        } else {
            at.moved(at.block + n - 1, len)
        }
    }

    /// A copy of `from..to` of `blocks`, the blocks of their container.
    pub(crate) fn copy(blocks: &[Block], from: Pos, to: Pos) -> Fragment {
        let (a, b) = (&blocks[from.block], &blocks[to.block]);
        if from.block == to.block {
            return Fragment::new(vec![with_runs(
                a,
                slice(&runs_of(a), from.offset, to.offset),
            )]);
        }
        let mut out = vec![with_runs(a, slice(&runs_of(a), from.offset, usize::MAX))];
        out.extend(blocks[from.block + 1..to.block].iter().cloned());
        out.push(with_runs(b, slice(&runs_of(b), 0, to.offset)));
        Fragment::new(out)
    }
}

/// Remove `from..to` from `blocks`, the blocks of their container: the block at `from`
/// keeps its looks and takes the runs after `to`; the blocks after it up to `to`'s go.
fn remove(blocks: &mut Vec<Block>, from: Pos, to: Pos) {
    let mut head = take_runs(&mut blocks[from.block]);
    let mut tail = if from.block == to.block {
        split_off(&mut head, to.offset)
    } else {
        let mut last = take_runs(&mut blocks[to.block]);
        split_off(&mut last, to.offset)
    };
    split_off(&mut head, from.offset);
    head.append(&mut tail);
    set_runs(&mut blocks[from.block], head);
    blocks.drain(from.block + 1..=to.block);
}

/// Paste `fragment` at `at` into `blocks`, the blocks of its container; returns where it
/// ends. The fragment's first and last blocks hold text (see [`Fragment`]).
pub(crate) fn paste(blocks: &mut Vec<Block>, at: Pos, fragment: &Fragment) -> Pos {
    let block = &mut blocks[at.block];
    let mut head = take_runs(block);
    let tail = split_off(&mut head, at.offset);
    head.extend(runs_of(&fragment.blocks[0]).iter().cloned());
    let n = fragment.blocks.len();
    if n == 1 {
        let offset = runs_len(&head);
        head.extend(tail);
        set_runs(block, head);
        return at.at_offset(offset);
    }
    set_runs(block, head);
    let mut rest = fragment.blocks[1..].to_vec();
    let last = rest.last_mut().expect("a fragment of more than one block");
    let mut runs = take_runs(last);
    let offset = runs_len(&runs);
    runs.extend(tail);
    set_runs(last, runs);
    blocks.splice(at.block + 1..at.block + 1, rest);
    at.moved(at.block + n - 1, offset)
}

/// Whether `pos` is in an editable block of `blocks`, its container's, on a char boundary.
pub(crate) fn valid(blocks: &[Block], pos: Pos) -> bool {
    blocks
        .get(pos.block)
        .and_then(block_text)
        .is_some_and(|t| pos.offset <= t.len() && t.is_char_boundary(pos.offset))
}

fn is_code(blocks: &[Block], block: usize) -> bool {
    matches!(blocks[block], Block::CodeBlock(_))
}

/// The blocks of `from` and `to`'s container and `from..to` in order, when both are in
/// editable blocks of the same container and the range doesn't reach out of a code block.
pub(crate) fn range(doc: &Document, from: Pos, to: Pos) -> Option<(&[Block], Pos, Pos)> {
    if from.at != to.at {
        return None;
    }
    let blocks = blocks(doc, from.at)?;
    let (from, to) = (from.min(to), from.max(to));
    let ok = valid(blocks, from)
        && valid(blocks, to)
        && (from.block == to.block || !(is_code(blocks, from.block) || is_code(blocks, to.block)));
    ok.then_some((blocks, from, to))
}

/// The look text typed at `at` takes: the character before the caret's, else the
/// paragraph's first character's, else the default look; never a link.
pub fn look(doc: &Document, at: Pos) -> RunStyle {
    let Some(p) = blocks(doc, at.at)
        .and_then(|b| b.get(at.block))
        .and_then(paragraph)
    else {
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
    /// Replacing `from..to` of `blocks`, their container's, with `inserted`.
    fn new(blocks: &[Block], from: Pos, to: Pos, inserted: Vec<Block>, typed: bool) -> TextEdit {
        TextEdit::paste(blocks, from, to, Fragment::new(inserted), typed)
    }

    /// Replacing `from..to` of `blocks`, their container's, with `inserted`, whose first
    /// and last blocks hold text.
    pub(crate) fn paste(
        blocks: &[Block],
        from: Pos,
        to: Pos,
        inserted: Fragment,
        typed: bool,
    ) -> TextEdit {
        TextEdit {
            at: from,
            removed: Fragment::copy(blocks, from, to),
            inserted,
            typed,
            select: false,
        }
    }

    /// What applying (`forward`) or undoing changes, the caret aside: the blocks from
    /// `at` on that the removed text covers become those the inserted text covers.
    pub(crate) fn shape(&self, forward: bool) -> Change {
        let (removed, inserted) = (self.removed.blocks.len(), self.inserted.blocks.len());
        let (removed, inserted) = if forward {
            (removed, inserted)
        } else {
            (inserted, removed)
        };
        Change {
            at: self.at.at,
            start: self.at.block,
            removed,
            inserted,
            caret: self.at,
            anchor: self.select.then_some(self.at),
        }
    }

    /// The looks of block `i` of the edit's container once applied to `old`, its blocks
    /// now, for `i` among the blocks it puts in: the first keeps its looks.
    pub(crate) fn looks_after<'a>(&'a self, old: &'a [Block], i: usize) -> Option<&'a Block> {
        let start = self.at.block;
        if i == start {
            old.get(start)
        } else {
            self.inserted.blocks.get(i.checked_sub(start)?)
        }
    }

    /// The blocks of the edit's container.
    fn container<'a>(&self, doc: &'a mut Document) -> &'a mut Vec<Block> {
        blocks_mut(doc, self.at.at).expect("the edit's container exists")
    }

    pub(crate) fn apply(&self, doc: &mut Document) -> Change {
        let blocks = self.container(doc);
        remove(blocks, self.at, self.removed.end(self.at));
        let caret = paste(blocks, self.at, &self.inserted);
        Change {
            caret,
            ..self.shape(true)
        }
    }

    pub(crate) fn revert(&self, doc: &mut Document) -> Change {
        let blocks = self.container(doc);
        remove(blocks, self.at, self.inserted.end(self.at));
        let caret = paste(blocks, self.at, &self.removed);
        Change {
            caret,
            ..self.shape(false)
        }
    }

    /// Whether blocks come or go.
    pub(crate) fn restructures(&self) -> bool {
        self.removed.blocks.len() != self.inserted.blocks.len()
    }

    /// Join `next`, typed straight after this typing, a word at a time: a word typed
    /// after whitespace starts a step of its own.
    pub(crate) fn absorb(&mut self, next: &TextEdit) -> bool {
        let continues = self.typed
            && next.typed
            && self.removed.is_empty()
            && next.removed.is_empty()
            && self.inserted.blocks.len() == 1
            && next.inserted.blocks.len() == 1
            && next.at == self.inserted.end(self.at);
        if !continues {
            return false;
        }
        let ends_in_space = runs_of(&self.inserted.blocks[0])
            .last()
            .and_then(|r| r.text.chars().next_back())
            .is_some_and(char::is_whitespace);
        let starts_word = runs_of(&next.inserted.blocks[0])
            .first()
            .and_then(|r| r.text.chars().next())
            .is_some_and(|c| !c.is_whitespace());
        if ends_in_space && starts_word {
            return false;
        }
        let block = &mut self.inserted.blocks[0];
        let mut runs = take_runs(block);
        runs.extend(runs_of(&next.inserted.blocks[0]).iter().cloned());
        set_runs(block, runs);
        true
    }
}

/// Typing `text` over `from..to` (the caret when they're equal) in `style`; empty `text`
/// deletes. In a code block `style` is ignored.
pub fn replace(doc: &Document, from: Pos, to: Pos, text: &str, style: &RunStyle) -> Option<Edit> {
    let (blocks, from, to) = range(doc, from, to)?;
    if from == to && text.is_empty() {
        return None;
    }
    let block = &blocks[from.block];
    let mut first = shell(block);
    if !text.is_empty() {
        let style = if is_code(blocks, from.block) {
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
    Some(TextEdit::new(blocks, from, to, vec![first], typed).into())
}

/// Enter: remove `from..to`, then split the block at the caret. In a code block it's a
/// line break; an empty list item leaves the list instead.
pub fn split(doc: &Document, from: Pos, to: Pos) -> Option<Edit> {
    let (blocks, from, to) = range(doc, from, to)?;
    let block = &blocks[from.block];
    let new = match block {
        Block::CodeBlock(_) => {
            let first = with_runs(
                block,
                vec![Run {
                    text: "\n".to_owned(),
                    style: RunStyle::default(),
                }],
            );
            return Some(TextEdit::new(blocks, from, to, vec![first], false).into());
        }
        Block::ListItem(item) if from == to && runs_len(&item.paragraph.runs) == 0 => {
            let after = Block::Paragraph(Paragraph {
                style: doc.styles.default_id().to_owned(),
                ..bare(&item.paragraph)
            });
            let looks = vec![(from.block, shell(block), after)];
            return Some(LooksEdit::new(from.at, looks, Some(from)).into());
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
            let at_end = Some(to.offset) == block_len(&blocks[to.block]);
            let style = doc.styles.style_of(&p.style);
            let style = if at_end && style.is_heading() {
                next_style(doc, &p.style)
            } else {
                p.style.clone()
            };
            Block::Paragraph(Paragraph { style, ..bare(p) })
        }
        _ => return None,
    };
    Some(TextEdit::new(blocks, from, to, vec![shell(block), new], false).into())
}

fn joinable(block: &Block) -> bool {
    matches!(block, Block::Paragraph(_) | Block::ListItem(_))
}

/// Backspace at the start of a block: a list item leaves its list; a paragraph joins the
/// paragraph or list item before it in its container.
pub fn join_backward(doc: &Document, at: Pos) -> Option<Edit> {
    let blocks = blocks(doc, at.at)?;
    if at.offset != 0 || !valid(blocks, at) {
        return None;
    }
    match &blocks[at.block] {
        block @ Block::ListItem(item) => {
            let after = Block::Paragraph(bare(&item.paragraph));
            let looks = vec![(at.block, shell(block), after)];
            Some(LooksEdit::new(at.at, looks, Some(at)).into())
        }
        Block::Paragraph(_) => {
            let prev = at.block.checked_sub(1)?;
            if !joinable(&blocks[prev]) {
                return None;
            }
            let end = at.moved(prev, block_len(&blocks[prev])?);
            let first = shell(&blocks[prev]);
            Some(TextEdit::new(blocks, end, at, vec![first], false).into())
        }
        _ => None,
    }
}

/// Delete at the end of a paragraph or list item: the paragraph or list item after it in
/// its container joins it.
pub fn join_forward(doc: &Document, at: Pos) -> Option<Edit> {
    let blocks = blocks(doc, at.at)?;
    if !valid(blocks, at) || Some(at.offset) != block_len(&blocks[at.block]) {
        return None;
    }
    let block = &blocks[at.block];
    let next = blocks.get(at.block + 1)?;
    if !joinable(block) || !joinable(next) {
        return None;
    }
    let first = shell(block);
    let to = at.moved(at.block + 1, 0);
    Some(TextEdit::new(blocks, at, to, vec![first], false).into())
}

/// The style of the paragraph Enter starts after the end of one in style `id`: the style's
/// next style, else the default.
fn next_style(doc: &Document, id: &str) -> String {
    let styles = &doc.styles;
    styles
        .style_of(id)
        .next
        .as_deref()
        .and_then(|next| styles.get(next))
        .unwrap_or_else(|| styles.default_style())
        .id
        .clone()
}

pub(crate) fn flag(style: &mut RunStyle, flag: Flag) -> &mut Option<bool> {
    match flag {
        Flag::Bold => &mut style.bold,
        Flag::Italic => &mut style.italic,
        Flag::Underline => &mut style.underline,
        Flag::Strike => &mut style.strike,
    }
}

/// Whether text with direct formatting `run` in a paragraph of style `style` shows
/// `which`.
fn shows(doc: &Document, style: &str, run: &RunStyle, which: Flag) -> bool {
    let look = doc.styles.look(style, run);
    match which {
        Flag::Bold => look.bold,
        Flag::Italic => look.italic,
        Flag::Underline => look.underline,
        Flag::Strike => look.strike,
    }
}

/// Bold, italic, underline, or strikethrough over `from..to` of paragraphs' and list
/// items' text: turned off when every character already shows it (its own or its style's),
/// else on everywhere, as the runs' direct formatting. The range stays selected.
pub fn toggle(doc: &Document, from: Pos, to: Pos, which: Flag) -> Option<Edit> {
    let (blocks, start, end) = range(doc, from, to)?;
    let (mut any, mut all) = (false, true);
    for (p, within) in paragraphs_in(blocks, start, end) {
        for run in runs_within(p, within) {
            any = true;
            all &= shows(doc, &p.style, &run.style, which);
        }
    }
    if !any {
        return None;
    }
    restyle(doc, from, to, |_, style| *flag(style, which) = Some(!all))
}

/// The paragraphs and list items `from..to` (in order, in `blocks`) touches, each with the
/// bytes of its text inside the range.
pub(crate) fn paragraphs_in(
    blocks: &[Block],
    from: Pos,
    to: Pos,
) -> impl Iterator<Item = (&Paragraph, Range<usize>)> {
    (from.block..=to.block).filter_map(move |i| {
        let p = paragraph(blocks.get(i)?)?;
        let start = if i == from.block { from.offset } else { 0 };
        let end = if i == to.block { to.offset } else { usize::MAX };
        Some((p, start..end))
    })
}

/// The runs of `p` with text inside `within`, bytes of its text.
pub(crate) fn runs_within(p: &Paragraph, within: Range<usize>) -> impl Iterator<Item = &Run> {
    let mut start = 0;
    p.runs.iter().filter(move |r| {
        let end = start + r.text.len();
        let inside = start.max(within.start) < end.min(within.end);
        start = end;
        inside
    })
}

/// Restyle the text of paragraphs and list items in `from..to`: `each` changes every run's
/// direct formatting, given its paragraph's style id. `None` when the range is empty or
/// nothing changes; the range stays selected.
pub(crate) fn restyle(
    doc: &Document,
    from: Pos,
    to: Pos,
    mut each: impl FnMut(&str, &mut RunStyle),
) -> Option<Edit> {
    let (blocks, from, to) = range(doc, from, to)?;
    if from == to {
        return None;
    }
    let removed = Fragment::copy(blocks, from, to);
    let mut inserted = removed.clone();
    for block in &mut inserted.blocks {
        if let Some(p) = paragraph_mut(block) {
            for run in &mut p.runs {
                each(&p.style, &mut run.style);
            }
            p.merge_runs();
        }
    }
    (inserted != removed).then(|| {
        TextEdit {
            at: from,
            removed,
            inserted,
            typed: false,
            select: true,
        }
        .into()
    })
}

/// The paragraph style id at `at` ("" outside a paragraph or list item).
pub(crate) fn style_at(doc: &Document, at: Pos) -> &str {
    blocks(doc, at.at)
        .and_then(|b| b.get(at.block))
        .and_then(paragraph)
        .map_or("", |p| p.style.as_str())
}

/// The look typed text takes after a toggle (Ctrl+B, Ctrl+I, Ctrl+U) with no selection at
/// `at`: `look` (what typing there would take) with `which` turned off if it shows there,
/// else on.
pub fn toggled(doc: &Document, at: Pos, look: &RunStyle, which: Flag) -> RunStyle {
    let mut look = look.clone();
    let on = shows(doc, style_at(doc, at), &look, which);
    *flag(&mut look, which) = Some(!on);
    look
}
