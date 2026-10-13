//! Find and replace (SLATE-16): where a text occurs in the document's editable blocks
//! (paragraphs, list items, code), at the top level and in top-level tables' cells, in
//! document order; replacing one occurrence (a text edit) or all of them (one step).
//!
//! Matches stay inside a block. Without `match_case`, letters compare by their lowercase
//! forms. Matches don't overlap: after one, the search goes on past its end.

use crate::clipboard::BlocksEdit;
use crate::text::{replace, slice};
use crate::{block_text, blocks, runs_len, runs_of, set_runs, Container, Edit, Pos};
use document_model::{Block, Document, Run, RunStyle};
use std::sync::atomic::{AtomicBool, Ordering};

/// Where the text was found: `from..to` in one block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Match {
    pub from: Pos,
    pub to: Pos,
}

/// Every occurrence of `needle` in `doc`, in document order, up to `limit` and one more
/// (to tell that there are more); `None` once `cancel` is set (checked block by block). An
/// empty needle finds nothing.
pub fn find(
    doc: &Document,
    needle: &str,
    match_case: bool,
    limit: usize,
    cancel: &AtomicBool,
) -> Option<Vec<Match>> {
    let mut out = Vec::new();
    if needle.is_empty() {
        return Some(out);
    }
    let needle = if match_case {
        needle.to_owned()
    } else {
        fold(needle).0
    };
    for (t, block) in doc.blocks.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        if out.len() > limit {
            out.truncate(limit + 1);
            break;
        }
        match block {
            Block::Table(table) => {
                for (r, row) in table.rows.iter().enumerate() {
                    for (c, cell) in row.cells.iter().enumerate() {
                        let at = Container::Cell {
                            table: t,
                            row: r,
                            cell: c,
                        };
                        for (b, block) in cell.blocks.iter().enumerate() {
                            search(block, at, b, &needle, match_case, &mut out);
                        }
                    }
                }
            }
            block => search(block, Container::Top, t, &needle, match_case, &mut out),
        }
    }
    out.truncate(limit.saturating_add(1));
    Some(out)
}

/// The occurrences of `needle` (already folded without `match_case`) in `block`.
fn search(
    block: &Block,
    at: Container,
    index: usize,
    needle: &str,
    match_case: bool,
    out: &mut Vec<Match>,
) {
    let Some(text) = block_text(block) else {
        return;
    };
    let pos = |offset| Pos {
        at,
        block: index,
        offset,
    };
    if match_case {
        out.extend(text.match_indices(needle).map(|(i, m)| Match {
            from: pos(i),
            to: pos(i + m.len()),
        }));
        return;
    }
    let (folded, map) = fold(&text);
    for (i, m) in folded.match_indices(needle) {
        // Both ends where a character of the text starts (or ends), not inside one
        // character's lowercase form.
        if let (Some(&from), Some(&to)) = (map.get(i), map.get(i + m.len())) {
            if from != usize::MAX && to != usize::MAX && from < to {
                out.push(Match {
                    from: pos(from),
                    to: pos(to),
                });
            }
        }
    }
}

/// `text` lowercased, and for each byte of it (and its end) the byte of `text` where its
/// character starts; `usize::MAX` inside a character's lowercase form.
fn fold(text: &str) -> (String, Vec<usize>) {
    let mut folded = String::with_capacity(text.len());
    let mut map = Vec::with_capacity(text.len() + 1);
    for (i, c) in text.char_indices() {
        let start = folded.len();
        folded.extend(c.to_lowercase());
        map.push(i);
        map.extend(std::iter::repeat_n(usize::MAX, folded.len() - start - 1));
    }
    map.push(text.len());
    (folded, map)
}

/// The look of the character at byte `offset` of `runs`.
fn style_at(runs: &[Run], offset: usize) -> RunStyle {
    let mut start = 0;
    for r in runs {
        let end = start + r.text.len();
        if offset < end {
            return r.style.clone();
        }
        start = end;
    }
    runs.last().map(|r| r.style.clone()).unwrap_or_default()
}

/// Replace match `m` with `with`, in the look of its first character: one text edit.
pub fn replace_one(doc: &Document, m: Match, with: &str) -> Option<Edit> {
    let block = blocks(doc, m.from.at)?.get(m.from.block)?;
    let style = style_at(&runs_of(block), m.from.offset);
    replace(doc, m.from, m.to, with, &style)
}

/// Replace every match in `matches` (in document order, as [`find`] gives them) with
/// `with`, each in the look of its first character, as one step: the top-level blocks from
/// the first match's to the last's are replaced. The caret goes after the last
/// replacement; undoing puts it at the first match.
pub fn replace_all(doc: &Document, matches: &[Match], with: &str) -> Option<Edit> {
    let first = matches.first()?;
    let last = matches.last()?;
    let top = |p: &Pos| match p.at {
        Container::Top => p.block,
        Container::Cell { table, .. } => table,
    };
    let (start, end) = (top(&first.from), top(&last.from));
    let mut inserted: Vec<Block> = doc.blocks.get(start..=end)?.to_vec();
    let mut caret = last.from;
    // Each block's matches, replaced from the last so earlier offsets hold.
    let mut i = matches.len();
    while i > 0 {
        let at = (matches[i - 1].from.at, matches[i - 1].from.block);
        let mut j = i;
        while j > 0 && (matches[j - 1].from.at, matches[j - 1].from.block) == at {
            j -= 1;
        }
        let group = &matches[j..i];
        let owner = &mut inserted[top(&group[0].from) - start];
        let block = match at.0 {
            Container::Top => owner,
            Container::Cell { row, cell, .. } => {
                let Block::Table(table) = owner else {
                    return None;
                };
                table
                    .rows
                    .get_mut(row)?
                    .cells
                    .get_mut(cell)?
                    .blocks
                    .get_mut(at.1)?
            }
        };
        replace_in(block, group, with)?;
        if i == matches.len() {
            let shift: isize = group[..group.len() - 1]
                .iter()
                .map(|m| with.len() as isize - (m.to.offset - m.from.offset) as isize)
                .sum();
            caret.offset = (last.from.offset as isize + shift) as usize + with.len();
        }
        i = j;
    }
    Some(
        BlocksEdit::new(
            start,
            doc.blocks[start..=end].to_vec(),
            inserted,
            caret,
            first.from,
        )
        .into(),
    )
}

/// Replace `matches`, all in `block` and in order, with `with`.
fn replace_in(block: &mut Block, matches: &[Match], with: &str) -> Option<()> {
    if let Block::CodeBlock(code) = block {
        for m in matches.iter().rev() {
            code.text.replace_range(m.from.offset..m.to.offset, with);
        }
        return Some(());
    }
    let runs = runs_of(block).into_owned();
    let len = runs_len(&runs);
    let mut out: Vec<Run> = Vec::with_capacity(runs.len() + matches.len());
    let mut at = 0;
    for m in matches {
        if m.from.offset < at || m.to.offset > len {
            return None;
        }
        out.extend(slice(&runs, at, m.from.offset));
        if !with.is_empty() {
            out.push(Run {
                text: with.to_owned(),
                style: style_at(&runs, m.from.offset),
            });
        }
        at = m.to.offset;
    }
    out.extend(slice(&runs, at, len));
    set_runs(block, out);
    if let Some(p) = crate::paragraph_mut(block) {
        p.merge_runs();
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{normalize, History};
    use document_model::{CodeBlock, Format, Paragraph, StyleSheet, Table, TableCell, TableRow};

    fn para(runs: &[(&str, bool)]) -> Block {
        let mut p = Paragraph::default();
        for (text, bold) in runs {
            p.push(
                text,
                &RunStyle {
                    bold: bold.then_some(true),
                    ..RunStyle::default()
                },
            );
        }
        Block::Paragraph(p)
    }

    fn doc(blocks: Vec<Block>) -> Document {
        let mut d = Document {
            blocks,
            format: Format::Docx,
            styles: StyleSheet::builtin(),
        };
        normalize(&mut d);
        d
    }

    fn cell(blocks: Vec<Block>) -> TableCell {
        TableCell { blocks, span: 1 }
    }

    fn sample() -> Document {
        doc(vec![
            para(&[
                ("The cat sat on ", false),
                ("the CAT", true),
                (" mat.", false),
            ]),
            Block::Table(Table {
                rows: vec![TableRow {
                    cells: vec![
                        cell(vec![para(&[("no match", false)])]),
                        cell(vec![para(&[("a cat", false)]), para(&[("cat cat", false)])]),
                    ],
                    header: false,
                }],
            }),
            Block::CodeBlock(CodeBlock {
                text: "let cat = 1;\n".into(),
                ..CodeBlock::default()
            }),
        ])
    }

    fn texts(m: &[Match]) -> Vec<(Container, usize, usize, usize)> {
        m.iter()
            .map(|m| (m.from.at, m.from.block, m.from.offset, m.to.offset))
            .collect()
    }

    fn no() -> AtomicBool {
        AtomicBool::new(false)
    }

    const ALL: usize = usize::MAX;

    #[test]
    fn matches_come_in_document_order_through_cells_and_code() {
        let d = sample();
        let cell1 = Container::Cell {
            table: 1,
            row: 0,
            cell: 1,
        };
        assert_eq!(
            texts(&find(&d, "cat", false, ALL, &no()).unwrap()),
            [
                (Container::Top, 0, 4, 7),
                (Container::Top, 0, 19, 22),
                (cell1, 0, 2, 5),
                (cell1, 1, 0, 3),
                (cell1, 1, 4, 7),
                (Container::Top, 2, 4, 7),
            ]
        );
        assert_eq!(find(&d, "CAT", true, ALL, &no()).unwrap().len(), 1);
        assert!(find(&d, "", false, ALL, &no()).unwrap().is_empty());
        assert!(find(&d, "cat", false, ALL, &AtomicBool::new(true)).is_none());
        // At most the limit and one more, the first ones.
        let first = find(&d, "cat", false, 2, &no()).unwrap();
        assert_eq!(first, find(&d, "cat", false, ALL, &no()).unwrap()[..3]);
    }

    #[test]
    fn case_folding_keeps_byte_offsets_of_the_text() {
        let d = doc(vec![para(&[("ÉCOLE and école, İstanbul", false)])]);
        let m = find(&d, "école", false, ALL, &no()).unwrap();
        assert_eq!(
            texts(&m),
            [(Container::Top, 0, 0, 6), (Container::Top, 0, 11, 17)]
        );
        // "İ" lowercases to two characters; a match on its first half isn't one.
        assert!(find(&d, "i", false, ALL, &no()).unwrap().is_empty());
        assert_eq!(find(&d, "i̇stanbul", false, ALL, &no()).unwrap().len(), 1);
    }

    #[test]
    fn replace_all_is_one_step_and_keeps_looks() {
        let mut d = sample();
        let before = d.clone();
        let m = find(&d, "cat", false, ALL, &no()).unwrap();
        let edit = replace_all(&d, &m, "dog").unwrap();
        let mut h = History::new();
        let change = h.apply(&mut d, edit);
        assert!(find(&d, "cat", false, ALL, &no()).unwrap().is_empty());
        assert_eq!(find(&d, "dog", false, ALL, &no()).unwrap().len(), 6);
        let Block::Paragraph(p) = &d.blocks[0] else {
            panic!()
        };
        assert_eq!(p.text(), "The dog sat on the dog mat.");
        assert!(p
            .runs
            .iter()
            .any(|r| r.text == "the dog" && r.style.bold == Some(true)));
        let Block::CodeBlock(c) = &d.blocks[2] else {
            panic!()
        };
        assert_eq!(c.text, "let dog = 1;\n");
        assert_eq!(
            change.caret,
            Pos {
                at: Container::Top,
                block: 2,
                offset: 7
            }
        );
        h.undo(&mut d).unwrap();
        assert_eq!(d, before);
    }

    #[test]
    fn replace_all_with_longer_and_empty_text() {
        let mut d = sample();
        let m = find(&d, "cat", false, ALL, &no()).unwrap();
        let mut h = History::new();
        let edit = replace_all(&d, &m, "").unwrap();
        let change = h.apply(&mut d, edit);
        assert!(find(&d, "cat", false, ALL, &no()).unwrap().is_empty());
        assert_eq!(change.caret.offset, 4);
        let mut d = sample();
        let m = find(&d, "cat", false, ALL, &no()).unwrap();
        let edit = replace_all(&d, &m, "kitten").unwrap();
        h.apply(&mut d, edit);
        let cell1 = Container::Cell {
            table: 1,
            row: 0,
            cell: 1,
        };
        assert_eq!(crate::text(&d, cell1, 1).unwrap(), "kitten kitten");
    }

    #[test]
    fn replace_one_keeps_the_look_of_its_first_character() {
        let mut d = sample();
        let m = find(&d, "cat", false, ALL, &no()).unwrap()[1];
        let mut h = History::new();
        let edit = replace_one(&d, m, "dog").unwrap();
        h.apply(&mut d, edit);
        let Block::Paragraph(p) = &d.blocks[0] else {
            panic!()
        };
        assert_eq!(p.text(), "The cat sat on the dog mat.");
        assert!(p
            .runs
            .iter()
            .any(|r| r.text == "the dog" && r.style.bold == Some(true)));
    }
}
