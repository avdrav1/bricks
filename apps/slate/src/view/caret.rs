//! The caret in model positions ([`Pos`]: a top-level block and a byte offset into its
//! text): where a point or the selection puts it, and moving it by character, word, line,
//! and block. It stays in editable blocks (paragraphs, list items, code); movement skips
//! tables, pictures, rules, page breaks, and raw source.

use super::a11y::log_attrs;
use super::State;
use document_edit::Pos;
use flow::{Position, TextPlace};
use gtk::pango;

// PangoLogAttr's bit positions (pango-break.h).
const CURSOR_POSITION: u32 = 1 << 4;
const WORD_START: u32 = 1 << 5;
const WORD_END: u32 = 1 << 6;

fn editable(st: &State, block: usize) -> bool {
    st.flow
        .document()
        .blocks
        .get(block)
        .is_some_and(document_edit::is_editable)
}

fn len(st: &State, block: usize) -> usize {
    document_edit::text_len(st.flow.document(), block).unwrap_or(0)
}

/// The nearest editable block from `block` on, going forward or back.
fn editable_from(st: &State, block: usize, forward: bool) -> Option<usize> {
    let n = st.flow.document().blocks.len();
    if forward {
        (block..n).find(|&b| editable(st, b))
    } else {
        (0..=block.min(n.checked_sub(1)?))
            .rev()
            .find(|&b| editable(st, b))
    }
}

fn start_of(block: usize) -> Pos {
    Pos { block, offset: 0 }
}

fn end_of(st: &State, block: usize) -> Pos {
    Pos {
        block,
        offset: len(st, block),
    }
}

/// The start of the document's first editable block.
pub(super) fn first(st: &State) -> Option<Pos> {
    editable_from(st, 0, true).map(start_of)
}

/// The end of its last editable block.
pub(super) fn last(st: &State) -> Option<Pos> {
    let n = st.flow.document().blocks.len();
    editable_from(st, n.checked_sub(1)?, false).map(|b| end_of(st, b))
}

/// The middle of the first editable block from `block` on, on a character boundary.
pub(super) fn middle_from(st: &State, block: usize) -> Option<Pos> {
    let block = editable_from(st, block, true)?;
    let text = document_edit::text(st.flow.document(), block)?;
    let mut offset = text.len() / 2;
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    Some(Pos { block, offset })
}

/// `p` as a caret position: itself in an editable top-level block, else the start of the
/// next editable block (`forward`) or the end of the one before.
pub(super) fn to_model(st: &State, p: Position, forward: bool) -> Option<Pos> {
    let block = *st.flow.path(p.leaf).first()? as usize;
    if st.flow.path(p.leaf).len() == 1 && editable(st, block) {
        return Some(Pos {
            block,
            offset: p.offset.min(len(st, block)),
        });
    }
    let (next, back) = (
        editable_from(st, block + 1, true).map(start_of),
        block
            .checked_sub(1)
            .and_then(|b| editable_from(st, b, false))
            .map(|b| end_of(st, b)),
    );
    if forward {
        next.or(back)
    } else {
        back.or(next)
    }
}

/// `pos` as a flow position.
pub(super) fn to_flow(st: &State, pos: Pos) -> Option<Position> {
    Some(Position {
        leaf: st.flow.block_leaf(pos.block)?,
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

/// Block `block`'s text place and its layout.
pub(super) fn layout(st: &mut State, block: usize) -> Option<(TextPlace, pango::Layout)> {
    let place = st.flow.text_place(block, &mut st.shaper)?;
    let content = st.flow.content(place.leaf)?;
    let layout = st.shaper.layout(place.key, content, place.width).clone();
    Some((place, layout))
}

/// The caret's rectangle at `pos`: left from the column's edge, top in document
/// coordinates, and height.
pub(super) fn rect(st: &mut State, pos: Pos) -> Option<(f64, f64, f64)> {
    let (place, layout) = layout(st, pos.block)?;
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
fn stops(st: &mut State, pos: Pos, bit: u32) -> Option<(Vec<usize>, usize)> {
    let (_, layout) = layout(st, pos.block)?;
    let text = document_edit::text(st.flow.document(), pos.block)?;
    let starts = char_starts(&text);
    let attrs = log_attrs(&layout);
    let stops = starts
        .iter()
        .zip(&attrs)
        .filter(|(_, a)| *a & bit != 0)
        .map(|(&b, _)| b)
        .collect();
    Some((stops, text.len()))
}

/// The next stop with `bit` after `pos`, else the start of the next editable block.
fn forward(st: &mut State, pos: Pos, bit: u32) -> Pos {
    if let Some((stops, _)) = stops(st, pos, bit) {
        if let Some(&b) = stops.iter().find(|&&b| b > pos.offset) {
            return Pos { offset: b, ..pos };
        }
    }
    if pos.offset < len(st, pos.block) {
        return end_of(st, pos.block);
    }
    editable_from(st, pos.block + 1, true).map_or(pos, start_of)
}

/// The last stop with `bit` before `pos`, else the end of the editable block before.
fn backward(st: &mut State, pos: Pos, bit: u32) -> Pos {
    if let Some((stops, _)) = stops(st, pos, bit) {
        if let Some(&b) = stops.iter().rev().find(|&&b| b < pos.offset) {
            return Pos { offset: b, ..pos };
        }
    }
    if pos.offset > 0 {
        return start_of(pos.block);
    }
    pos.block
        .checked_sub(1)
        .and_then(|b| editable_from(st, b, false))
        .map_or(pos, |b| end_of(st, b))
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
    let text = document_edit::text(st.flow.document(), pos.block)?;
    let (i, _) = text[..pos.offset].char_indices().next_back()?;
    Some(Pos { offset: i, ..pos })
}

/// The character after `pos` in its block, for Delete: `None` at the block's end.
pub(super) fn char_after(st: &State, pos: Pos) -> Option<Pos> {
    let text = document_edit::text(st.flow.document(), pos.block)?;
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
    let Some((_, layout)) = layout(st, pos.block) else {
        return pos;
    };
    let Some(text) = document_edit::text(st.flow.document(), pos.block) else {
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

/// One wrapped line up or down from `pos`, as near as can be to `goal` (pango units from
/// the column's left edge; `pos`'s own x when `None`). The new position and the goal.
pub(super) fn vertical(
    st: &mut State,
    pos: Pos,
    goal: Option<i32>,
    down: bool,
) -> (Pos, Option<i32>) {
    let Some((place, here)) = layout(st, pos.block) else {
        return (pos, goal);
    };
    let (line, x) = here.index_to_line_x(pos.offset as i32, false);
    let goal = goal.unwrap_or(place.x * pango::SCALE + x);
    let lines = here.line_count();
    let target = if down && line + 1 < lines {
        Some((pos.block, place, here, line + 1))
    } else if !down && line > 0 {
        Some((pos.block, place, here, line - 1))
    } else {
        let next = if down {
            editable_from(st, pos.block + 1, true)
        } else {
            pos.block
                .checked_sub(1)
                .and_then(|b| editable_from(st, b, false))
        };
        next.and_then(|b| {
            let (place, layout) = layout(st, b)?;
            let line = if down { 0 } else { layout.line_count() - 1 };
            Some((b, place, layout, line))
        })
    };
    let Some((block, place, layout, line)) = target else {
        // Up on the first line goes to the start; down on the last to the end.
        let edge = if down {
            end_of(st, pos.block)
        } else {
            start_of(pos.block)
        };
        return (edge, Some(goal));
    };
    let Some(text) = document_edit::text(st.flow.document(), block) else {
        return (pos, Some(goal));
    };
    let Some(l) = layout.line_readonly(line) else {
        return (pos, Some(goal));
    };
    let hit = l.x_to_index(goal - place.x * pango::SCALE);
    let mut offset = hit.index().max(0) as usize;
    for _ in 0..hit.trailing() {
        offset += text[offset..].chars().next().map_or(0, char::len_utf8);
    }
    let offset = offset.min(line_end(&layout, &text, line));
    (Pos { block, offset }, Some(goal))
}

/// The block a caret or selection command acts on: from the selection's first block to its
/// last, else the caret's.
pub(super) fn blocks(st: &State) -> Option<std::ops::Range<usize>> {
    match selection(st) {
        Some((s, e)) => Some(s.block..e.block + 1),
        None => st.caret.map(|c| c.block..c.block + 1),
    }
}

/// Whether `block` is a list item.
pub(super) fn is_list_item(st: &State, block: usize) -> bool {
    matches!(
        st.flow.document().blocks.get(block),
        Some(document_model::Block::ListItem(_))
    )
}
