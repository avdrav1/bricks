//! What a shaper lays out, and the text a leaf holds.

use crate::tree::{px, LEVEL_INDENT, TWIPS_PER_PX};
use document_model::{
    CodeBlock, Image, LineSpacing, ListItem, Paragraph, Raw, StyleSheet, TabAlign, TabLeader,
};
use std::borrow::Cow;

/// A Markdown or text paragraph's tabs advance to multiples of this many spaces' width in
/// its font (ADR 0009).
pub const TAB_SPACES: u32 = 4;

/// A text-bearing leaf as a shaper lays it out.
#[derive(Debug, Clone, Copy)]
pub enum Content<'a> {
    /// A paragraph, or a list item's paragraph (`list`, whose marker hangs in the indent),
    /// with the document's styles. `header`: the paragraph is in a table's header row.
    /// `default_tab`: the document's default tab stop in twips; `None` for Markdown and
    /// text, whose tabs advance by [`TAB_SPACES`] spaces.
    Paragraph {
        paragraph: &'a Paragraph,
        styles: &'a StyleSheet,
        list: Option<&'a ListItem>,
        header: bool,
        default_tab: Option<u32>,
    },
    /// A code block: its text without the last line's `\n`.
    Code(&'a CodeBlock),
    /// Source Slate shows as written.
    Raw(&'a Raw),
    /// A picture that can't be shown, labelled with its alt text.
    PictureLabel(&'a Image),
}

/// Where a paragraph's tabs advance to (ADR 0009).
#[derive(Debug, Clone, PartialEq)]
pub enum Tabs {
    /// Multiples of this many spaces' width in the paragraph's font.
    Spaces(u32),
    /// These stops, in order. The last two are default stops, a default stop apart, and
    /// the stops after them go on at that interval, as Pango repeats a tab array's last
    /// interval.
    Stops(Vec<Tab>),
}

/// A tab stop where a layout places it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tab {
    /// From the layout's left edge, in pixels at 96 dpi.
    pub x: f64,
    /// Never [`TabAlign::Clear`].
    pub align: TabAlign,
    pub leader: TabLeader,
}

impl<'a> Content<'a> {
    /// The text as laid out; positions in it are byte offsets.
    pub fn text(&self) -> Cow<'a, str> {
        match *self {
            Content::Paragraph { paragraph, .. } => match paragraph.runs.as_slice() {
                [] => Cow::Borrowed(""),
                [run] => Cow::Borrowed(&run.text),
                _ => Cow::Owned(paragraph.text()),
            },
            Content::Code(code) => Cow::Borrowed(code_text(code)),
            Content::Raw(raw) => Cow::Borrowed(&raw.source),
            Content::PictureLabel(image) => Cow::Owned(picture_label(image)),
        }
    }

    /// The first line's indent from the other lines', in pixels at 96 dpi: a paragraph's
    /// style's first-line indent, negative for a hanging first line. 0 for a list item,
    /// whose marker sets its indent, and for other content.
    pub fn first_indent(&self) -> i32 {
        match *self {
            Content::Paragraph {
                paragraph,
                styles,
                list: None,
                ..
            } => px(i64::from(
                styles.style_of(&paragraph.style).para.indent_first,
            )),
            _ => 0,
        }
    }

    /// Where the text's layout starts from the left edge of the column or table cell it is
    /// in, in pixels at 96 dpi: a list item's at its marker's indent; a paragraph's at its
    /// left indent (and its quote or list nesting), less a hanging first line's. 0 for
    /// other content.
    pub fn text_left(&self) -> i32 {
        match *self {
            Content::Paragraph {
                list: Some(item), ..
            } => i32::from(item.level) * LEVEL_INDENT + LEVEL_INDENT,
            Content::Paragraph {
                paragraph, styles, ..
            } => {
                let para = &styles.style_of(&paragraph.style).para;
                let left =
                    i32::from(paragraph.indent) * LEVEL_INDENT + px(i64::from(para.indent_left));
                (left + self.first_indent().min(0)).max(0)
            }
            _ => 0,
        }
    }

    /// Where a paragraph's tabs advance to, from its layout's left edge
    /// ([`Content::text_left`]): a Word document's paragraph's stops (its style's and its
    /// own) right of that edge, then the default stop's multiples from the text's left
    /// edge after the last of them; Markdown's and text's every [`TAB_SPACES`] spaces.
    /// `None` for other content, whose tabs are the shaper's default.
    pub fn tabs(&self) -> Option<Tabs> {
        let Content::Paragraph {
            paragraph,
            styles,
            default_tab,
            ..
        } = *self
        else {
            return None;
        };
        let Some(default) = default_tab else {
            return Some(Tabs::Spaces(TAB_SPACES));
        };
        let default = i64::from(default.max(1));
        let left = i64::from(self.text_left()) * TWIPS_PER_PX as i64;
        let stops: Vec<_> = styles
            .tab_stops(paragraph)
            .into_iter()
            .filter(|s| i64::from(s.position) > left)
            .collect();
        let last = stops.last().map_or(left, |s| i64::from(s.position));
        let first = (last.div_euclid(default) + 1) * default;
        let defaults = (0..2).map(|i| (first + i * default, TabAlign::Left, TabLeader::None));
        let tabs = stops
            .iter()
            .map(|s| (i64::from(s.position), s.align, s.leader))
            .chain(defaults)
            .map(|(twips, align, leader)| Tab {
                x: (twips - left) as f64 / TWIPS_PER_PX,
                align,
                leader,
            })
            .collect();
        Some(Tabs::Stops(tabs))
    }

    /// How far apart the lines are: a paragraph's (or list item's) style's line spacing;
    /// single for other content.
    pub fn line_spacing(&self) -> LineSpacing {
        match *self {
            Content::Paragraph {
                paragraph, styles, ..
            } => styles.style_of(&paragraph.style).para.line,
            _ => LineSpacing::default(),
        }
    }
}

/// A code block's text as shown: without the last line's `\n`.
pub(crate) fn code_text(code: &CodeBlock) -> &str {
    code.text.strip_suffix('\n').unwrap_or(&code.text)
}

/// The label of a picture that can't be shown.
pub(crate) fn picture_label(image: &Image) -> String {
    if image.alt.is_empty() {
        "[picture]".to_owned()
    } else {
        format!("[picture: {}]", image.alt)
    }
}

/// `offset` moved back to the nearest character boundary of `text`, at most its length.
pub(crate) fn floor_boundary(text: &str, offset: usize) -> usize {
    let mut i = offset.min(text.len());
    while !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}
