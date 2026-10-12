//! The accessible text's boundaries (the character, word, sentence, line, or paragraph
//! holding an offset) and run attributes, from the flow's units and the leaf's Pango
//! layout, so screen readers hear what is drawn.

use super::shaper::PangoShaper;
use document_edit::Container;
use flow::{Flow, Unit};
use glib::translate::IntoGlib;
use gtk::AccessibleTextGranularity as G;
use gtk::{glib, pango};
use std::ops::Range;

/// The accessible text an edit is about to replace, kept to tell screen readers what it
/// removed and inserted once it's done.
pub struct Before {
    start: usize,
    end: usize,
    text: String,
    total: usize,
}

/// What an edit did to the accessible text: the characters it removed (where they started
/// and what they were) and the range it inserted, in character offsets.
#[derive(Default)]
pub struct TextChange {
    pub removed: Option<(usize, String)>,
    pub inserted: Option<Range<usize>>,
}

impl Before {
    /// The text of the blocks `span` names (a cell's: its whole table), as it is now.
    pub fn take(flow: &Flow, (at, blocks): (Container, Range<usize>)) -> Before {
        let blocks = match at {
            Container::Top => blocks,
            Container::Cell { table, .. } => table..table + 1,
        };
        let chars = flow.block_chars(blocks);
        Before {
            text: flow.text(chars.start, chars.end),
            start: chars.start,
            end: chars.end,
            total: flow.char_count(),
        }
    }

    /// What changed, now that the edit is done: the stretch it replaced, less the
    /// characters at its ends that are the same before and after.
    pub fn after(self, flow: &Flow) -> TextChange {
        let end = (self.end + flow.char_count()).saturating_sub(self.total);
        let new = flow.text(self.start, end);
        let (old, new): (Vec<char>, Vec<char>) =
            (self.text.chars().collect(), new.chars().collect());
        let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
        let suffix = old[prefix..]
            .iter()
            .rev()
            .zip(new[prefix..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        let at = self.start + prefix;
        let removed: String = old[prefix..old.len() - suffix].iter().collect();
        let inserted = new.len() - suffix - prefix;
        TextChange {
            removed: (!removed.is_empty()).then_some((at, removed)),
            inserted: (inserted > 0).then_some(at..at + inserted),
        }
    }
}

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

/// Looks as AT-SPI names them: attribute names and values.
pub type Looks = Vec<(&'static str, String)>;

/// The page's text as a whole: what every run has unless its attributes say otherwise.
pub fn default_attributes(shaper: &PangoShaper) -> Looks {
    font_attributes(&shaper.font())
        .into_iter()
        .chain([
            ("underline", "none".to_owned()),
            ("strikethrough", "false".to_owned()),
            ("fg-color", color(super::INK)),
        ])
        .collect()
}

/// The run of the same looks holding character `offset`: its start and end (character
/// offsets) and its looks as AT-SPI names them. `None` for a leaf without laid-out text.
pub fn attributes(
    flow: &Flow,
    shaper: &mut PangoShaper,
    offset: usize,
) -> Option<(usize, usize, Looks)> {
    let unit = flow.unit_at(offset)?;
    let content = flow.content(unit.leaf)?;
    let text = content.text();
    let base = shaper.font();
    let width = flow.wrap_width(unit.leaf).max(1);
    let layout = shaper.layout(flow.layout_key(unit.leaf), content, width);
    let byte = text
        .char_indices()
        .nth(offset - unit.start)
        .map_or(text.len(), |(b, _)| b);
    let list = layout.attributes()?;
    let mut runs = list.iterator();
    loop {
        let (s, e) = runs.range();
        if (s.max(0) as usize..e.max(0) as usize).contains(&byte) || !runs.next_style_change() {
            break;
        }
    }
    let (s, e) = runs.range();
    let (desc, _, extra) = runs.font();
    let mut font = base;
    font.merge(Some(&desc), true);
    let mut looks = font_attributes(&font);
    let (mut underline, mut strike, mut fg) = ("none", "false", color(super::INK));
    for attr in extra.iter() {
        match attr.type_() {
            pango::AttrType::Underline => {
                let single = attr
                    .downcast_ref::<pango::AttrInt>()
                    .is_some_and(|a| a.value() != pango::Underline::None.into_glib());
                underline = if single { "single" } else { "none" };
            }
            pango::AttrType::Strikethrough => {
                let on = attr
                    .downcast_ref::<pango::AttrInt>()
                    .is_some_and(|a| a.value() != 0);
                strike = if on { "true" } else { "false" };
            }
            pango::AttrType::Foreground => {
                if let Some(c) = attr.downcast_ref::<pango::AttrColor>().map(|a| a.color()) {
                    fg = format!("{},{},{}", c.red() >> 8, c.green() >> 8, c.blue() >> 8);
                }
            }
            _ => {}
        }
    }
    looks.extend([
        ("underline", underline.to_owned()),
        ("strikethrough", strike.to_owned()),
        ("fg-color", fg),
    ]);
    let chars = |b: i32| text[..(b.max(0) as usize).min(text.len())].chars().count();
    Some((unit.start + chars(s), unit.start + chars(e), looks))
}

/// A font's family, size in points, weight, and style.
fn font_attributes(font: &pango::FontDescription) -> Looks {
    let style = match font.style() {
        pango::Style::Italic => "italic",
        pango::Style::Oblique => "oblique",
        _ => "normal",
    };
    vec![
        (
            "family-name",
            font.family().map(|f| f.to_string()).unwrap_or_default(),
        ),
        (
            "size",
            format!("{}", f64::from(font.size()) / f64::from(pango::SCALE)),
        ),
        ("weight", font.weight().into_glib().to_string()),
        ("style", style.to_owned()),
    ]
}

/// `hex` (0xRRGGBB) as AT-SPI writes a colour: "r,g,b".
fn color(hex: u32) -> String {
    format!("{},{},{}", hex >> 16 & 0xFF, hex >> 8 & 0xFF, hex & 0xFF)
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
