//! The accessible text's boundaries: the character, word, sentence, line, or paragraph
//! holding an offset, from the flow's units and the leaf's Pango layout.

use super::shaper::PangoShaper;
use flow::{Flow, Unit};
use gtk::pango;
use gtk::AccessibleTextGranularity as G;

// PangoLogAttr's bit positions (pango-break.h).
const WORD_START: u32 = 1 << 5;
const SENTENCE_START: u32 = 1 << 8;

/// The start and end (character offsets) of the `granularity` piece holding `offset`.
pub fn boundary(
    flow: &Flow,
    shaper: &mut PangoShaper,
    offset: usize,
    granularity: G,
) -> Option<(usize, usize)> {
    let unit = flow.unit_at(offset)?;
    let total = flow.char_count();
    let o = offset.min(total);
    let end = unit.start + unit.chars + usize::from(!unit.last);
    Some(match granularity {
        G::Character => (o, (o + 1).min(total)),
        G::Paragraph => (unit.start, end),
        _ => {
            let Some(content) = flow.content(unit.leaf) else {
                // A picture's alt text is a paragraph of its own.
                return Some((unit.start, end));
            };
            let text = content.text();
            let width = flow.wrap_width(unit.leaf).max(1);
            let layout = shaper.layout(flow.layout_key(unit.leaf), content, width);
            let k = o - unit.start;
            match granularity {
                G::Line => line(layout, &text, &unit, k, end),
                G::Word => starts(layout, &unit, k, end, WORD_START),
                _ => starts(layout, &unit, k, end, SENTENCE_START),
            }
        }
    })
}

/// The wrapped line holding character `k` of the unit.
fn line(layout: &pango::Layout, text: &str, unit: &Unit, k: usize, end: usize) -> (usize, usize) {
    let byte = text.char_indices().nth(k).map_or(text.len(), |(b, _)| b);
    let lines = layout.lines_readonly();
    let chars = |b: usize| unit.start + text[..b.min(text.len())].chars().count();
    for (n, line) in lines.iter().enumerate() {
        let start = line.start_index() as usize;
        let stop = start + line.length() as usize;
        if byte < stop || n + 1 == lines.len() {
            let e = if n + 1 < lines.len() {
                chars(stop)
            } else {
                end
            };
            return (chars(start), e);
        }
    }
    (unit.start, end)
}

/// From the last `bit` start at or before character `k` to the next after it.
fn starts(layout: &pango::Layout, unit: &Unit, k: usize, end: usize, bit: u32) -> (usize, usize) {
    let attrs = log_attrs(layout);
    let has = |j: usize| attrs.get(j).is_some_and(|a| a & bit != 0);
    let s = (0..=k.min(unit.chars)).rev().find(|&j| has(j)).unwrap_or(0);
    let e = (k + 1..unit.chars)
        .find(|&j| has(j))
        .map_or(end, |j| unit.start + j);
    (unit.start + s, e)
}

/// The layout's log attributes as their bit fields, one per character and one past the end.
#[allow(unsafe_code)]
pub(super) fn log_attrs(layout: &pango::Layout) -> Vec<u32> {
    use gtk::glib::translate::ToGlibPtr;
    let mut n = 0;
    // SAFETY: PangoLogAttr is a struct of 32 one-bit fields, so each is one u32; Pango
    // returns `n` of them owned by the layout, which outlives the copy made here.
    unsafe {
        let attrs =
            pango::ffi::pango_layout_get_log_attrs_readonly(layout.to_glib_none().0, &mut n);
        std::slice::from_raw_parts(attrs.cast::<u32>(), n as usize).to_vec()
    }
}
