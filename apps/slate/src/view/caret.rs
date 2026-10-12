//! The caret in model positions ([`Pos`]: a container — the document or a table's cell —
//! a block in it, and a byte offset into the block's text): where a point or the selection
//! puts it, and moving it by character, word, line, block, and cell. It stays in editable
//! blocks (paragraphs, list items, code) of the document and of top-level tables' cells;
//! movement walks the flow's leaves in document order, skipping pictures, rules, page
//! breaks, raw source, and tables nested in cells.

use super::a11y::log_attrs;
use super::State;
use document_edit::{Container, Pos};
use flow::{LeafId, Position, TextPlace};
use gtk::pango;
use std::ops::Range;

// PangoLogAttr's bit positions (pango-break.h).
const CURSOR_POSITION: u32 = 1 << 4;
const WORD_START: u32 = 1 << 5;
const WORD_END: u32 = 1 << 6;
/// How far each probe of Up and Down steps through the space between lines.
const PROBE: f64 = 2.0;

/// The container and block a leaf's path names, if it is an editable block.
fn block_of(st: &State, leaf: LeafId) -> Option<(Container, usize)> {
    let (at, block) = match *st.flow.path(leaf) {
        [block] => (Container::Top, block as usize),
        [table, row, cell, block] => (
            Container::Cell {
                table: table as usize,
                row: row as usize,
                cell: cell as usize,
            },
            block as usize,
        ),
        _ => return None,
    };
    let blocks = document_edit::blocks(st.flow.document(), at)?;
    blocks
        .get(block)
        .is_some_and(document_edit::is_editable)
        .then_some((at, block))
}

fn path_of(pos: Pos) -> Vec<u32> {
    match pos.at {
        Container::Top => vec![pos.block as u32],
        Container::Cell { table, row, cell } => {
            vec![table as u32, row as u32, cell as u32, pos.block as u32]
        }
    }
}

fn leaf_of(st: &State, pos: Pos) -> Option<LeafId> {
    st.flow.leaf_at(&path_of(pos))
}

fn len(st: &State, at: Container, block: usize) -> usize {
    document_edit::text_len(st.flow.document(), at, block).unwrap_or(0)
}

fn start_of((at, block): (Container, usize)) -> Pos {
    Pos {
        at,
        block,
        offset: 0,
    }
}

fn end_of(st: &State, (at, block): (Container, usize)) -> Pos {
    Pos {
        at,
        block,
        offset: len(st, at, block),
    }
}

/// The editable block of the first leaf from `from` on, going forward, or back.
fn editable_from(st: &State, from: usize, forward: bool) -> Option<(Container, usize)> {
    let n = st.flow.leaf_count();
    let leaf = |i: usize| block_of(st, LeafId::from_index(i));
    if forward {
        (from..n).find_map(leaf)
    } else {
        (0..=from.min(n.checked_sub(1)?)).rev().find_map(leaf)
    }
}

/// The editable block after (`forward`) or before `pos`'s.
fn neighbour(st: &State, pos: Pos, forward: bool) -> Option<(Container, usize)> {
    let i = leaf_of(st, pos)?.index();
    if forward {
        editable_from(st, i + 1, true)
    } else {
        editable_from(st, i.checked_sub(1)?, false)
    }
}

/// The start of the document's first editable block.
pub(super) fn first(st: &State) -> Option<Pos> {
    editable_from(st, 0, true).map(start_of)
}

/// The end of its last editable block.
pub(super) fn last(st: &State) -> Option<Pos> {
    let n = st.flow.leaf_count();
    editable_from(st, n.checked_sub(1)?, false).map(|b| end_of(st, b))
}

/// The middle of the first editable top-level block from `block` on, on a character
/// boundary.
pub(super) fn middle_from(st: &State, block: usize) -> Option<Pos> {
    let leaf = (block..st.flow.block_count()).find_map(|b| st.flow.leaf_at(&[b as u32]))?;
    let (at, block) = editable_from(st, leaf.index(), true)?;
    let text = document_edit::text(st.flow.document(), at, block)?;
    let mut offset = text.len() / 2;
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    Some(Pos { at, block, offset })
}

/// `p` as a caret position: itself in an editable block, else the start of the next
/// editable block (`forward`) or the end of the one before.
pub(super) fn to_model(st: &State, p: Position, forward: bool) -> Option<Pos> {
    if let Some((at, block)) = block_of(st, p.leaf) {
        return Some(Pos {
            at,
            block,
            offset: p.offset.min(len(st, at, block)),
        });
    }
    let i = p.leaf.index();
    let next = editable_from(st, i + 1, true).map(start_of);
    let back = i
        .checked_sub(1)
        .and_then(|i| editable_from(st, i, false))
        .map(|b| end_of(st, b));
    if forward {
        next.or(back)
    } else {
        back.or(next)
    }
}

/// `pos` as a flow position.
pub(super) fn to_flow(st: &State, pos: Pos) -> Option<Position> {
    Some(Position {
        leaf: leaf_of(st, pos)?,
        offset: pos.offset,
    })
}

/// The selection as caret positions, start first; `None` without one, or when it holds no
/// editable text.
pub(super) fn selection(st: &State) -> Option<(Pos, Pos)> {
    let (s, e) = st.flow.selection()?;
    let s = to_model(st, s, true)?;
    let e = to_model(st, e, false)?;
    (s < e).then_some((s, e))
}

/// `pos`'s block's text place and its layout.
pub(super) fn layout(st: &mut State, pos: Pos) -> Option<(TextPlace, pango::Layout)> {
    let leaf = leaf_of(st, pos)?;
    let place = st.flow.text_place(leaf, &mut st.shaper)?;
    let content = st.flow.content(leaf)?;
    let layout = st.shaper.layout(place.key, content, place.width).clone();
    Some((place, layout))
}

/// The caret's rectangle at `pos`: left from the column's edge, top in document
/// coordinates, and height.
pub(super) fn rect(st: &mut State, pos: Pos) -> Option<(f64, f64, f64)> {
    let (place, layout) = layout(st, pos)?;
    let (strong, _) = layout.cursor_pos(pos.offset as i32);
    let s = f64::from(pango::SCALE);
    Some((
        f64::from(place.x) + f64::from(strong.x()) / s,
        place.y + f64::from(strong.y()) / s,
        f64::from(strong.height()) / s,
    ))
}

/// The byte offset of each character of `text`, and its length.
fn char_starts(text: &str) -> Vec<usize> {
    text.char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
        .collect()
}

/// The positions in `pos`'s block whose log attributes have `bit`, as byte offsets.
fn stops(st: &mut State, pos: Pos, bit: u32) -> Option<Vec<usize>> {
    let (_, layout) = layout(st, pos)?;
    let text = document_edit::text(st.flow.document(), pos.at, pos.block)?;
    let attrs = log_attrs(&layout);
    Some(
        char_starts(&text)
            .into_iter()
            .zip(attrs)
            .filter(|(_, a)| a & bit != 0)
            .map(|(b, _)| b)
            .collect(),
    )
}

/// The next stop with `bit` after `pos`, else the start of the next editable block.
fn forward(st: &mut State, pos: Pos, bit: u32) -> Pos {
    if let Some(stops) = stops(st, pos, bit) {
        if let Some(&b) = stops.iter().find(|&&b| b > pos.offset) {
            return Pos { offset: b, ..pos };
        }
    }
    if pos.offset < len(st, pos.at, pos.block) {
        return end_of(st, (pos.at, pos.block));
    }
    neighbour(st, pos, true).map_or(pos, start_of)
}

/// The last stop with `bit` before `pos`, else the end of the editable block before.
fn backward(st: &mut State, pos: Pos, bit: u32) -> Pos {
    if let Some(stops) = stops(st, pos, bit) {
        if let Some(&b) = stops.iter().rev().find(|&&b| b < pos.offset) {
            return Pos { offset: b, ..pos };
        }
    }
    if pos.offset > 0 {
        return Pos { offset: 0, ..pos };
    }
    neighbour(st, pos, false).map_or(pos, |b| end_of(st, b))
}

pub(super) fn right(st: &mut State, pos: Pos) -> Pos {
    forward(st, pos, CURSOR_POSITION)
}

pub(super) fn left(st: &mut State, pos: Pos) -> Pos {
    backward(st, pos, CURSOR_POSITION)
}

pub(super) fn word_right(st: &mut State, pos: Pos) -> Pos {
    forward(st, pos, WORD_END)
}

pub(super) fn word_left(st: &mut State, pos: Pos) -> Pos {
    backward(st, pos, WORD_START)
}

/// The character before `pos` in its block, for Backspace: `None` at the block's start.
pub(super) fn char_before(st: &State, pos: Pos) -> Option<Pos> {
    let text = document_edit::text(st.flow.document(), pos.at, pos.block)?;
    let (i, _) = text[..pos.offset].char_indices().next_back()?;
    Some(Pos { offset: i, ..pos })
}

/// The character after `pos` in its block, for Delete: `None` at the block's end.
pub(super) fn char_after(st: &State, pos: Pos) -> Option<Pos> {
    let text = document_edit::text(st.flow.document(), pos.at, pos.block)?;
    let c = text[pos.offset..].chars().next()?;
    Some(Pos {
        offset: pos.offset + c.len_utf8(),
        ..pos
    })
}

/// The end of wrapped line `line` of `layout`: before the space it wraps at, so the caret
/// shows on that line, unless it is the last line.
fn line_end(layout: &pango::Layout, text: &str, line: i32) -> usize {
    let Some(l) = layout.line_readonly(line) else {
        return text.len();
    };
    let end = (l.start_index() + l.length()) as usize;
    if line + 1 < layout.line_count() {
        if let Some(c) = text[..end]
            .chars()
            .next_back()
            .filter(|c| c.is_whitespace())
        {
            return end - c.len_utf8();
        }
    }
    end
}

/// The start (`false`) or end (`true`) of the wrapped line `pos` is on.
pub(super) fn line_edge(st: &mut State, pos: Pos, end: bool) -> Pos {
    let Some((_, layout)) = layout(st, pos) else {
        return pos;
    };
    let Some(text) = document_edit::text(st.flow.document(), pos.at, pos.block) else {
        return pos;
    };
    let (line, _) = layout.index_to_line_x(pos.offset as i32, false);
    let offset = if end {
        line_end(&layout, &text, line)
    } else {
        layout
            .line_readonly(line)
            .map_or(0, |l| l.start_index() as usize)
    };
    Pos { offset, ..pos }
}

/// One line up or down from `pos`, as near as can be to column `goal` (pixels from the
/// column's left edge; `pos`'s own when `None`): the first caret position whose line
/// starts below the caret's (or ends above it), found by hit-testing through the space
/// between lines, so it crosses into and out of table cells. The new position and goal.
pub(super) fn vertical(
    st: &mut State,
    pos: Pos,
    goal: Option<f64>,
    down: bool,
) -> (Pos, Option<f64>) {
    let Some((x, y, h)) = rect(st, pos) else {
        return (pos, goal);
    };
    let goal = goal.unwrap_or(x);
    let end = st.flow.content_height();
    let mut probe = if down { y + h + 1.0 } else { y - 1.0 };
    while (0.0..end).contains(&probe) {
        let found = st
            .flow
            .hit(&mut st.shaper, goal, probe)
            .and_then(|at| to_model(st, at.position, down));
        if let Some(p) = found {
            if let Some((_, py, ph)) = rect(st, p) {
                if (down && py >= y + h - 0.5) || (!down && py + ph <= y + 0.5) {
                    return (p, Some(goal));
                }
            }
        }
        probe += if down { PROBE } else { -PROBE };
    }
    // Up on the first line goes to the start; down on the last to the end.
    let edge = if down {
        end_of(st, (pos.at, pos.block))
    } else {
        Pos { offset: 0, ..pos }
    };
    (edge, Some(goal))
}

/// The cell after (`forward`) or before `pos`'s, at the start of its first editable block,
/// for Tab and Shift+Tab; past the table's last cell, the start of the editable block
/// after it, and before its first, the end of the one before. `None` outside a table.
pub(super) fn next_cell(st: &State, pos: Pos, forward: bool) -> Option<Pos> {
    let Container::Cell { table, .. } = pos.at else {
        return None;
    };
    let i = leaf_of(st, pos)?.index();
    let n = st.flow.leaf_count();
    let leaves: Box<dyn Iterator<Item = usize>> = if forward {
        Box::new(i + 1..n)
    } else {
        Box::new((0..i).rev())
    };
    for j in leaves {
        let Some((at, block)) = block_of(st, LeafId::from_index(j)) else {
            continue;
        };
        match at {
            Container::Cell { table: t, .. } if t == table && at == pos.at => continue,
            Container::Cell { table: t, .. } if t == table => {
                // The other cell's first editable block.
                let blocks = document_edit::blocks(st.flow.document(), at)?;
                let first = blocks.iter().position(document_edit::is_editable)?;
                return Some(start_of((at, first.min(block))));
            }
            _ if forward => return Some(start_of((at, block))),
            _ => return Some(end_of(st, (at, block))),
        }
    }
    None
}

/// The blocks a paragraph command acts on: from the selection's first block to its last
/// when both are in one container, else the caret's block.
pub(super) fn blocks(st: &State) -> Option<(Container, Range<usize>)> {
    match selection(st) {
        Some((s, e)) if s.at == e.at => Some((s.at, s.block..e.block + 1)),
        _ => st.caret.map(|c| (c.at, c.block..c.block + 1)),
    }
}

/// Whether `pos` is in a list item.
pub(super) fn is_list_item(st: &State, pos: Pos) -> bool {
    document_edit::blocks(st.flow.document(), pos.at)
        .and_then(|b| b.get(pos.block))
        .is_some_and(|b| matches!(b, document_model::Block::ListItem(_)))
}
