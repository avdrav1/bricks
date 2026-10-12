//! What a shaper lays out, and the text a leaf holds.

use document_model::{CodeBlock, Image, ListItem, Paragraph, Raw};
use std::borrow::Cow;

/// A text-bearing leaf as a shaper lays it out.
#[derive(Debug, Clone, Copy)]
pub enum Content<'a> {
    /// A paragraph, or a list item's paragraph (`list`, whose marker hangs in the indent).
    /// `header`: the paragraph is in a table's header row.
    Paragraph {
        paragraph: &'a Paragraph,
        list: Option<&'a ListItem>,
        header: bool,
    },
    /// A code block: its text without the last line's `\n`.
    Code(&'a CodeBlock),
    /// Source Slate shows as written.
    Raw(&'a Raw),
    /// A picture that can't be shown, labelled with its alt text.
    PictureLabel(&'a Image),
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
