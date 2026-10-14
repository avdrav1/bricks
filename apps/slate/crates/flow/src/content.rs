//! What a shaper lays out, and the text a leaf holds.

use crate::tree::{px, LEVEL_INDENT, TWIPS_PER_PX};
use document_model::{
    CodeBlock, Image, LineSpacing, ListItem, ListLevel, Look, MarkerAlign, MarkerSuffix,
    ParaSpacing, Paragraph, Raw, RunStyle, StyleSheet, TabAlign, TabLeader, TabStop,
};
use std::borrow::Cow;

/// A Markdown or text paragraph's tabs advance to multiples of this many spaces' width in
/// its font (ADR 0009).
pub const TAB_SPACES: u32 = 4;

/// A text-bearing leaf as a shaper lays it out.
#[derive(Debug, Clone, Copy)]
pub enum Content<'a> {
    /// A paragraph, or a list item's paragraph (`list`, whose marker hangs in the indent),
    /// with the document's styles. `level`: a Word list item's level, which places its
    /// marker and text as Word does ([`Content::place`]); `None` for a Markdown or text
    /// item, which the viewer lays out. `header`: the paragraph is in a table's header row.
    /// `default_tab`: the document's default tab stop in twips; `None` for Markdown and
    /// text, whose tabs advance by [`TAB_SPACES`] spaces.
    Paragraph {
        paragraph: &'a Paragraph,
        styles: &'a StyleSheet,
        list: Option<&'a ListItem>,
        level: Option<&'a ListLevel>,
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

/// A list item's marker as a shaper measures and draws it: not text of the document, so
/// selections, the caret, and the accessible text leave it out.
#[derive(Debug, Clone, PartialEq)]
pub struct Marker<'a> {
    /// Empty for a level without text: it draws nothing, but what follows it does.
    pub text: &'a str,
    pub look: Look,
}

/// What placing a Word list item's marker takes from the shaper, in pixels: the marker's
/// width and a space's in its look.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MarkerWidth {
    pub marker: f64,
    pub space: f64,
}

/// Where a paragraph's text and a list item's marker go, in pixels at 96 dpi from the left
/// edge of the column or table cell the paragraph is in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Place {
    /// The text layout's left edge.
    pub left: i32,
    /// The first line's start from `left` (Pango's indent): when negative, the first line
    /// starts at `left` and the others that far right of it.
    pub first: i32,
    /// A list item's marker's left edge.
    pub marker: Option<f64>,
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

    /// A paragraph's (or list item's) spacing and indents: its style's with its own on top,
    /// and a Word list item's level's between them (ADR 0009); `None` for other content.
    pub fn spacing(&self) -> Option<ParaSpacing> {
        match *self {
            Content::Paragraph {
                paragraph,
                styles,
                level: Some(level),
                ..
            } => Some(styles.list_spacing(paragraph, level)),
            Content::Paragraph {
                paragraph, styles, ..
            } => Some(styles.spacing(paragraph)),
            _ => None,
        }
    }

    /// A list item's marker: a Word item's level's text (a number's label) in its level's
    /// look over its paragraph mark's; a Markdown or text item's bullet, label, or task box
    /// in the document's font. `None` for other content.
    pub fn marker(&self) -> Option<Marker<'a>> {
        let Content::Paragraph {
            paragraph,
            styles,
            list: Some(item),
            level,
            ..
        } = *self
        else {
            return None;
        };
        let look = match level {
            Some(level) => {
                let none = RunStyle::default();
                let mark = item.numbering.as_ref().map_or(&none, |n| &n.mark);
                styles.marker_look(paragraph, mark, level)
            }
            None => {
                let run = &styles.default_style().run;
                Look {
                    font: run.font.clone(),
                    size: run.size,
                    bold: false,
                    italic: false,
                    underline: false,
                    strike: false,
                    color: None,
                }
            }
        };
        Some(Marker {
            text: item.marker(level),
            look,
        })
    }

    /// Whether placing it takes its marker's width ([`Content::place`]): a Word list item.
    pub fn measures_marker(&self) -> bool {
        matches!(
            self,
            Content::Paragraph {
                list: Some(_),
                level: Some(_),
                ..
            }
        )
    }

    /// Where its text and marker go, its marker measuring `widths` (ADR 0009; estimates
    /// before measuring give the default, nothing wide). Never left of the column's edge.
    ///
    /// - A Word list item, as Word places it: its marker at its first line's start (its
    ///   left indent plus its first-line one, negative when hanging), aligned there by its
    ///   level; then a tab to the next of its stops past the marker, the hanging indent's
    ///   implicit stop among them, else the next default stop; or a space; or nothing. Its
    ///   other lines start at its left indent.
    /// - A Markdown or text item: its text at its level's indent (or its own left indent,
    ///   leaving its marker room), its marker [`LEVEL_INDENT`] left of it.
    /// - A paragraph: its left indent (and its quote or list nesting), its first line by its
    ///   first-line indent.
    /// - Other content: at the edge.
    pub fn place(&self, widths: MarkerWidth) -> Place {
        match *self {
            Content::Paragraph {
                paragraph,
                styles,
                list: Some(_),
                level: Some(level),
                default_tab,
                ..
            } => {
                // Negative indents are clamped to the column's edge, first-line start too.
                let s = styles.list_spacing(paragraph, level);
                let nest = i32::from(paragraph.indent) * LEVEL_INDENT;
                let left = (nest + px(i64::from(s.indent_left))).max(0);
                let start = (f64::from(nest)
                    + f64::from(s.indent_left + s.indent_first) / TWIPS_PER_PX)
                    .max(0.0);
                let w = widths.marker;
                let marker = match level.align {
                    MarkerAlign::Left => start,
                    MarkerAlign::Center => start - w / 2.0,
                    MarkerAlign::Right => start - w,
                };
                let end = marker + w;
                let text = match level.suffix {
                    MarkerSuffix::Tab => {
                        let stops = styles.list_tabs(paragraph, level);
                        marker_tab(&stops, f64::from(left), end, default_tab)
                    }
                    MarkerSuffix::Space => end + widths.space,
                    MarkerSuffix::Nothing => end,
                };
                let text = (text.round() as i32).max(0);
                Place {
                    left: left.min(text),
                    first: text - left,
                    marker: Some(marker.max(0.0)),
                }
            }
            Content::Paragraph {
                paragraph,
                list: Some(item),
                ..
            } => {
                let left = match paragraph.own.indent_left {
                    Some(left) => px(i64::from(left)).max(LEVEL_INDENT),
                    None => i32::from(item.level) * LEVEL_INDENT + LEVEL_INDENT,
                };
                Place {
                    left,
                    first: 0,
                    marker: Some(f64::from(left - LEVEL_INDENT)),
                }
            }
            Content::Paragraph {
                paragraph, styles, ..
            } => {
                let s = styles.spacing(paragraph);
                let left =
                    i32::from(paragraph.indent) * LEVEL_INDENT + px(i64::from(s.indent_left));
                let first = px(i64::from(s.indent_first));
                Place {
                    left: (left + first.min(0)).max(0),
                    first,
                    marker: None,
                }
            }
            _ => Place {
                left: 0,
                first: 0,
                marker: None,
            },
        }
    }

    /// How far the text's layout ends left of the right edge of the column or table cell
    /// it is in, in pixels at 96 dpi: a paragraph's or list item's right indent, never past
    /// the edge. 0 for other content.
    pub fn text_right(&self) -> i32 {
        self.spacing()
            .map_or(0, |s| px(i64::from(s.indent_right)).max(0))
    }

    /// Where a paragraph's tabs advance to, from its layout's left edge `left`
    /// ([`Place::left`]): a Word document's paragraph's stops (its style's, a list item's
    /// level's, and its own, and a hanging indent's implicit stop at its left indent) right
    /// of that edge, then the default stop's multiples from the text's left edge after the
    /// last of them; Markdown's and text's every [`TAB_SPACES`] spaces. `None` for other
    /// content, whose tabs are the shaper's default.
    pub fn tabs(&self, left: i32) -> Option<Tabs> {
        let Content::Paragraph {
            paragraph,
            styles,
            level,
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
        let left = i64::from(left) * TWIPS_PER_PX as i64;
        let mut stops = match level {
            Some(level) => styles.list_tabs(paragraph, level),
            None => styles.tab_stops(paragraph),
        };
        if let Some(s) = self.spacing().filter(|s| s.indent_first < 0) {
            let at = stops.partition_point(|t| t.position < s.indent_left);
            if stops.get(at).is_none_or(|t| t.position != s.indent_left) {
                stops.insert(
                    at,
                    TabStop {
                        position: s.indent_left,
                        align: TabAlign::Left,
                        leader: TabLeader::None,
                    },
                );
            }
        }
        stops.retain(|s| i64::from(s.position) > left);
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

    /// How far apart the lines are: a paragraph's (or list item's) line spacing, its
    /// style's or its own; single for other content.
    pub fn line_spacing(&self) -> LineSpacing {
        self.spacing().map_or_else(LineSpacing::default, |s| s.line)
    }
}

/// Where a Word list item's tab after its marker goes, in pixels from the column's edge:
/// to the first of its `stops` (twips) past the marker's end `end`, or to its left indent
/// `left` (a hanging indent's implicit stop) when that is nearer and past it; with neither,
/// to the next multiple of the default stop (twips; Word's 720 when there is none).
fn marker_tab(stops: &[TabStop], left: f64, end: f64, default_tab: Option<u32>) -> f64 {
    let stop = stops
        .iter()
        .map(|s| f64::from(s.position) / TWIPS_PER_PX)
        .find(|&x| x > end);
    let implicit = (left > end).then_some(left);
    match (stop, implicit) {
        (Some(a), Some(b)) => a.min(b),
        (Some(x), None) | (None, Some(x)) => x,
        (None, None) => {
            let d = f64::from(default_tab.unwrap_or(720).max(1)) / TWIPS_PER_PX;
            ((end / d).floor() + 1.0) * d
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
