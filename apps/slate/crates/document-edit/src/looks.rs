//! Looks edits: paragraph style, list kind, list level, and ordered labels of whole
//! blocks, each holding the blocks' looks (without their text) before and after.

use crate::{bare, paragraph, set_runs, shell, take_runs, text_len, Change, Edit, Pos};
use document_model::{Block, Document, ListItem, ListKind, Paragraph, ParagraphStyle};
use std::ops::Range;

/// Changing the looks of whole blocks: each block's index, its looks before, and its
/// looks after, as blocks without text.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LooksEdit {
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
    pub(crate) fn new(blocks: Vec<(usize, Block, Block)>, caret: Option<Pos>) -> LooksEdit {
        LooksEdit { blocks, caret }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    pub(crate) fn set(&self, doc: &mut Document) {
        for (i, _, after) in &self.blocks {
            restyle(&mut doc.blocks[*i], after);
        }
    }

    pub(crate) fn unset(&self, doc: &mut Document) {
        for (i, before, _) in self.blocks.iter().rev() {
            restyle(&mut doc.blocks[*i], before);
        }
    }

    fn change(&self, doc: &Document) -> Change {
        let first = self.blocks.iter().map(|b| b.0).min().unwrap_or(0);
        let last = self.blocks.iter().map(|b| b.0).max().unwrap_or(0);
        let n = last - first + 1;
        let (caret, anchor) = match self.caret {
            Some(caret) => (caret, None),
            None => (
                Pos::new(last, text_len(doc, last).unwrap_or(0)),
                Some(Pos::new(first, 0)),
            ),
        };
        Change {
            start: first,
            removed: n,
            inserted: n,
            caret,
            anchor,
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

fn edit(blocks: Vec<(usize, Block, Block)>) -> Option<Edit> {
    (!blocks.is_empty()).then(|| LooksEdit::new(blocks, None).into())
}

/// `blocks` clamped to the document.
fn clamp(doc: &Document, blocks: Range<usize>) -> Range<usize> {
    blocks.start.min(doc.blocks.len())..blocks.end.min(doc.blocks.len())
}

/// Give the paragraphs in `blocks` `style`; list items there become paragraphs in it.
pub fn set_style(doc: &Document, blocks: Range<usize>, style: ParagraphStyle) -> Option<Edit> {
    let mut out = Vec::new();
    for i in clamp(doc, blocks) {
        let block = &doc.blocks[i];
        let p = match block {
            Block::Paragraph(p) if p.style != style => p,
            Block::ListItem(item) => &item.paragraph,
            _ => continue,
        };
        let after = Block::Paragraph(Paragraph { style, ..bare(p) });
        out.push((i, shell(block), after));
    }
    edit(out)
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
/// in `blocks`: off when they all already are list items of that kind, else on.
pub fn toggle_list(doc: &Document, blocks: Range<usize>, ordered: bool) -> Option<Edit> {
    let blocks = clamp(doc, blocks);
    let texts = || {
        blocks
            .clone()
            .filter(|&i| paragraph(&doc.blocks[i]).is_some())
    };
    texts().next()?;
    let off = texts()
        .all(|i| matches!(&doc.blocks[i], Block::ListItem(item) if same_kind(&item.kind, ordered)));
    let mut out = Vec::new();
    for i in texts() {
        let block = &doc.blocks[i];
        let after = match block {
            Block::ListItem(item) if off => Block::Paragraph(Paragraph {
                style: ParagraphStyle::Normal,
                ..bare(&item.paragraph)
            }),
            Block::ListItem(item) if same_kind(&item.kind, ordered) => continue,
            Block::ListItem(item) => Block::ListItem(ListItem {
                kind: new_kind(ordered),
                level: item.level,
                paragraph: Paragraph {
                    style: ParagraphStyle::Normal,
                    ..bare(&item.paragraph)
                },
            }),
            Block::Paragraph(p) => Block::ListItem(ListItem {
                kind: new_kind(ordered),
                level: 0,
                paragraph: Paragraph {
                    style: ParagraphStyle::Normal,
                    ..bare(p)
                },
            }),
            _ => continue,
        };
        out.push((i, shell(block), after));
    }
    edit(out)
}

/// The deepest list level.
const MAX_LEVEL: i16 = 8;

/// Move the list items in `blocks` `by` levels deeper (shallower when negative), within
/// 0..=8. A numbered item moved starts at 1 unless it continues a list at its new level.
pub fn indent(doc: &Document, blocks: Range<usize>, by: i8) -> Option<Edit> {
    let mut out = Vec::new();
    for i in clamp(doc, blocks) {
        let block = &doc.blocks[i];
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
    edit(out)
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

/// Renumber the ordered items of the lists touching `blocks` (of `doc` as it is now): at
/// each level, consecutive numbered items (deeper items between them don't break the run,
/// a shallower or other item does) count up from the first one's number, labelled like it.
pub(crate) fn relabel(doc: &Document, blocks: Range<usize>) -> LooksEdit {
    let n = doc.blocks.len();
    let is_item = |i: usize| matches!(doc.blocks.get(i), Some(Block::ListItem(_)));
    let mut lo = blocks.start.min(n);
    while lo > 0 && is_item(lo - 1) {
        lo -= 1;
    }
    let mut hi = blocks.end.min(n);
    while hi < n && is_item(hi) {
        hi += 1;
    }
    // Per level: the next number and the first item's label.
    let mut runs: Vec<Option<(u64, &str)>> = Vec::new();
    let mut out = Vec::new();
    for i in lo..hi {
        let block = &doc.blocks[i];
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
                    out.push((i, shell(block), after));
                }
                runs[level] = Some((next + 1, first));
            }
            None => runs[level] = Some((number + 1, label)),
        }
    }
    LooksEdit::new(out, None)
}
