//! Looks edits: paragraph style, alignment, list kind, list level, and ordered labels of
//! whole blocks, each holding the blocks' looks (without their text) before and after.

use crate::{
    bare, block_len, blocks, blocks_mut, paragraph, paragraph_mut, set_runs, shell, take_runs,
    Change, Container, Edit, Pos,
};
use document_model::{Align, Block, Document, ListItem, ListKind, Paragraph};
use std::ops::Range;

/// Changing the looks of whole blocks of a container: each block's index, its looks
/// before, and its looks after, as blocks without text.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LooksEdit {
    at: Container,
    blocks: Vec<(usize, Block, Block)>,
    /// Where the caret goes; `None` selects the changed blocks.
    caret: Option<Pos>,
}

/// Give `block` the looks of `looks`, keeping its text.
fn restyle(block: &mut Block, looks: &Block) {
    let runs = take_runs(block);
    *block = looks.clone();
    set_runs(block, runs);
}

impl LooksEdit {
    pub(crate) fn new(
        at: Container,
        blocks: Vec<(usize, Block, Block)>,
        caret: Option<Pos>,
    ) -> LooksEdit {
        LooksEdit { at, blocks, caret }
    }

    /// The blocks of the edit's container.
    fn container<'a>(&self, doc: &'a mut Document) -> &'a mut Vec<Block> {
        blocks_mut(doc, self.at).expect("the edit's container exists")
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    pub(crate) fn set(&self, doc: &mut Document) {
        if self.blocks.is_empty() {
            return;
        }
        let blocks = self.container(doc);
        for (i, _, after) in &self.blocks {
            restyle(&mut blocks[*i], after);
        }
    }

    pub(crate) fn unset(&self, doc: &mut Document) {
        if self.blocks.is_empty() {
            return;
        }
        let blocks = self.container(doc);
        for (i, before, _) in self.blocks.iter().rev() {
            restyle(&mut blocks[*i], before);
        }
    }

    /// What applying or undoing changes, the caret aside: the blocks from the first one it
    /// restyles to the last.
    pub(crate) fn shape(&self) -> Change {
        let first = self.blocks.iter().map(|b| b.0).min().unwrap_or(0);
        let last = self.blocks.iter().map(|b| b.0).max().unwrap_or(0);
        let n = last - first + 1;
        let start = Pos {
            at: self.at,
            block: first,
            offset: 0,
        };
        Change {
            at: self.at,
            start: first,
            removed: n,
            inserted: n,
            caret: start,
            anchor: None,
        }
    }

    fn change(&self, doc: &Document) -> Change {
        let shape = self.shape();
        let (caret, anchor) = match self.caret {
            Some(caret) => (caret, None),
            None => {
                let last = shape.start + shape.removed - 1;
                let len = blocks(doc, self.at)
                    .and_then(|b| b.get(last))
                    .and_then(block_len);
                (shape.caret.moved(last, len.unwrap_or(0)), Some(shape.caret))
            }
        };
        Change {
            caret,
            anchor,
            ..shape
        }
    }

    /// The looks of block `i` of the edit's container once applied to `old`, its blocks
    /// now.
    pub(crate) fn looks_after<'a>(&'a self, old: &'a [Block], i: usize) -> Option<&'a Block> {
        match self.blocks.iter().rfind(|b| b.0 == i) {
            Some((_, _, after)) => Some(after),
            None => old.get(i),
        }
    }

    pub(crate) fn apply(&self, doc: &mut Document) -> Change {
        self.set(doc);
        self.change(doc)
    }

    pub(crate) fn revert(&self, doc: &mut Document) -> Change {
        self.unset(doc);
        self.change(doc)
    }

    /// Widen `change` to cover the blocks this changes, indexes of the document after
    /// `change` when `after`, else before.
    pub(crate) fn widen(&self, change: Change, after: bool) -> Change {
        self.blocks
            .iter()
            .fold(change, |change, b| change.widen(b.0, after))
    }
}

fn edit(at: Container, blocks: Vec<(usize, Block, Block)>) -> Option<Edit> {
    (!blocks.is_empty()).then(|| LooksEdit::new(at, blocks, None).into())
}

/// The blocks of `at` and `range` clamped to them.
fn clamp(doc: &Document, at: Container, range: Range<usize>) -> Option<(&[Block], Range<usize>)> {
    let blocks = blocks(doc, at)?;
    let n = blocks.len();
    Some((blocks, range.start.min(n)..range.end.min(n)))
}

/// Give the paragraphs in `blocks` of `at` the paragraph style with id `style`; list items
/// there become paragraphs in it.
pub fn set_style(doc: &Document, at: Container, blocks: Range<usize>, style: &str) -> Option<Edit> {
    let (blocks, range) = clamp(doc, at, blocks)?;
    let mut out = Vec::new();
    for i in range {
        let block = &blocks[i];
        let p = match block {
            Block::Paragraph(p) if p.style != style => p,
            Block::ListItem(item) => &item.paragraph,
            _ => continue,
        };
        let after = Block::Paragraph(Paragraph {
            style: style.to_owned(),
            ..bare(p)
        });
        out.push((i, shell(block), after));
    }
    edit(at, out)
}

/// Align the paragraphs and list items in `blocks` of `at` to `align`.
pub fn set_align(
    doc: &Document,
    at: Container,
    blocks: Range<usize>,
    align: Align,
) -> Option<Edit> {
    let (blocks, range) = clamp(doc, at, blocks)?;
    let mut out = Vec::new();
    for i in range {
        let block = &blocks[i];
        if paragraph(block).is_some_and(|p| p.align != align) {
            let mut after = shell(block);
            if let Some(p) = paragraph_mut(&mut after) {
                p.align = align;
            }
            out.push((i, shell(block), after));
        }
    }
    edit(at, out)
}

fn same_kind(kind: &ListKind, ordered: bool) -> bool {
    if ordered {
        matches!(kind, ListKind::Ordered { .. })
    } else {
        matches!(kind, ListKind::Bullet)
    }
}

fn new_kind(ordered: bool) -> ListKind {
    if ordered {
        ListKind::Ordered {
            number: 1,
            label: "1.".to_owned(),
        }
    } else {
        ListKind::Bullet
    }
}

/// Bulleted (or numbered when `ordered`) list on or off for the paragraphs and list items
/// in `blocks` of `at`: off when they all already are list items of that kind, else on.
pub fn toggle_list(
    doc: &Document,
    at: Container,
    blocks: Range<usize>,
    ordered: bool,
) -> Option<Edit> {
    let (blocks, range) = clamp(doc, at, blocks)?;
    let normal = || doc.styles.default_id().to_owned();
    let texts = || range.clone().filter(|&i| paragraph(&blocks[i]).is_some());
    texts().next()?;
    let off = texts()
        .all(|i| matches!(&blocks[i], Block::ListItem(item) if same_kind(&item.kind, ordered)));
    let mut out = Vec::new();
    for i in texts() {
        let block = &blocks[i];
        let after = match block {
            Block::ListItem(item) if off => Block::Paragraph(Paragraph {
                style: normal(),
                ..bare(&item.paragraph)
            }),
            Block::ListItem(item) if same_kind(&item.kind, ordered) => continue,
            Block::ListItem(item) => Block::ListItem(ListItem {
                kind: new_kind(ordered),
                level: item.level,
                paragraph: Paragraph {
                    style: normal(),
                    ..bare(&item.paragraph)
                },
            }),
            Block::Paragraph(p) => Block::ListItem(ListItem {
                kind: new_kind(ordered),
                level: 0,
                paragraph: Paragraph {
                    style: normal(),
                    ..bare(p)
                },
            }),
            _ => continue,
        };
        out.push((i, shell(block), after));
    }
    edit(at, out)
}

/// The deepest list level.
const MAX_LEVEL: i16 = 8;

/// Move the list items in `blocks` of `at` `by` levels deeper (shallower when negative),
/// within 0..=8. A numbered item moved starts at 1 unless it continues a list at its new
/// level.
pub fn indent(doc: &Document, at: Container, blocks: Range<usize>, by: i8) -> Option<Edit> {
    let (blocks, range) = clamp(doc, at, blocks)?;
    let mut out = Vec::new();
    for i in range {
        let block = &blocks[i];
        let Block::ListItem(item) = block else {
            continue;
        };
        let level = (i16::from(item.level) + i16::from(by)).clamp(0, MAX_LEVEL) as u8;
        if level == item.level {
            continue;
        }
        let kind = match &item.kind {
            ListKind::Ordered { label, .. } => ListKind::Ordered {
                number: 1,
                label: numbered(label, 1),
            },
            kind => kind.clone(),
        };
        let after = Block::ListItem(ListItem {
            kind,
            level,
            paragraph: bare(&item.paragraph),
        });
        out.push((i, shell(block), after));
    }
    edit(at, out)
}

/// `label` with its first run of decimal digits replaced by `number`; a label without
/// digits stays as it is.
fn numbered(label: &str, number: u64) -> String {
    let Some(start) = label.find(|c: char| c.is_ascii_digit()) else {
        return label.to_owned();
    };
    let end = label[start..]
        .find(|c: char| !c.is_ascii_digit())
        .map_or(label.len(), |n| start + n);
    format!("{}{number}{}", &label[..start], &label[end..])
}

/// Renumber the ordered items of the lists touching `range` of `at` (of `doc` as it is
/// now): at each level, consecutive numbered items (deeper items between them don't break
/// the run, a shallower or other item does) count up from the first one's number,
/// labelled like it.
pub(crate) fn relabel(doc: &Document, at: Container, range: Range<usize>) -> LooksEdit {
    let blocks = blocks(doc, at).unwrap_or_default();
    let mut out = Vec::new();
    renumber(
        blocks.len(),
        |i| blocks.get(i),
        range,
        |i, block, after| {
            out.push((i, shell(block), after));
        },
    );
    LooksEdit::new(at, out, None)
}

/// [`relabel`] over `n` blocks seen through `get`: calls `each` with the index, the
/// block, and its new looks for every item it renumbers, in order.
pub(crate) fn renumber<'a>(
    n: usize,
    get: impl Fn(usize) -> Option<&'a Block>,
    range: Range<usize>,
    mut each: impl FnMut(usize, &'a Block, Block),
) {
    let is_item = |i: usize| matches!(get(i), Some(Block::ListItem(_)));
    let mut lo = range.start.min(n);
    while lo > 0 && is_item(lo - 1) {
        lo -= 1;
    }
    let mut hi = range.end.min(n);
    while hi < n && is_item(hi) {
        hi += 1;
    }
    // Per level: the next number and the first item's label.
    let mut runs: Vec<Option<(u64, &str)>> = Vec::new();
    for i in lo..hi {
        let Some(block) = get(i) else {
            break;
        };
        let Block::ListItem(item) = block else {
            runs.clear();
            continue;
        };
        let level = usize::from(item.level);
        runs.resize(level + 1, None);
        let ListKind::Ordered { number, label } = &item.kind else {
            runs[level] = None;
            continue;
        };
        match runs[level] {
            Some((next, first)) => {
                let want = numbered(first, next);
                if *number != next || *label != want {
                    let after = Block::ListItem(ListItem {
                        kind: ListKind::Ordered {
                            number: next,
                            label: want,
                        },
                        level: item.level,
                        paragraph: bare(&item.paragraph),
                    });
                    each(i, block, after);
                }
                runs[level] = Some((next + 1, first));
            }
            None => runs[level] = Some((number + 1, label)),
        }
    }
}
