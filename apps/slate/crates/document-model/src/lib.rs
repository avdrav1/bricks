//! Slate's document model (docs/adr/0003-slate-document-model.md): one model for every
//! format Slate reads, toolkit-free.
//!
//! A [`Document`] is a flat list of [`Block`]s: paragraphs, list items, code blocks,
//! tables (whose cells hold blocks), images, page breaks, rules, and [`Raw`] source Slate
//! can't model; and its paragraph styles ([`StyleSheet`]). A paragraph names its style and
//! holds [`Run`]s of text, each with its direct formatting over the style. Readers convert
//! a file into the model: plain text ([`read_text`]) and Markdown ([`read_markdown`]) here,
//! DOCX in the `docx` crate. Writers convert it back: plain text ([`write_text`]) and
//! Markdown ([`write_markdown`]).

mod fonts;
mod markdown;
mod matching;
mod styles;
mod tabs;
mod text;
mod write;

pub use fonts::{EmbeddedFace, FaceKind, Font, FontFamily, FontPitch, FontTable};
pub use markdown::{load_linked_images, read_markdown, read_markdown_source, MarkdownSource};
pub use matching::{match_blocks, match_blocks_by};
pub use styles::{
    Border, BorderOverrides, BorderStyle, Borders, LineSpacing, Look, ParaLook, ParaOverrides,
    ParaSpacing, Shading, Style, StyleSheet,
};
pub use tabs::{effective_tabs, merge_tabs, TabAlign, TabLeader, TabStop};
pub use text::{decode, read_text, Encoding, LineEnding, TextInfo};
pub use write::{encode, write_markdown, write_text, Unencodable};

use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// A colour as red, green, blue.
pub type Rgb = [u8; 3];

/// A document read from a file: its blocks, its paragraph styles, and what the file was.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub blocks: Vec<Block>,
    pub styles: StyleSheet,
    pub format: Format,
}

/// The format a document was read from, with what a save needs to write it the same way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Format {
    Text(TextInfo),
    Markdown(TextInfo),
    Docx(DocxInfo),
}

impl Format {
    /// The format's name as the status bar shows it.
    pub fn name(&self) -> &'static str {
        match self {
            Format::Text(_) => "Plain text",
            Format::Markdown(_) => "Markdown",
            Format::Docx(_) => "Word document",
        }
    }

    /// The page's text width in twips, for a format that has pages (ADR 0009).
    pub fn page_text_width(&self) -> Option<u32> {
        match self {
            Format::Docx(info) => Some(info.text_width),
            Format::Text(_) | Format::Markdown(_) => None,
        }
    }

    /// The default tab stop's interval in twips, for a format that sets one (ADR 0009);
    /// Markdown and text have four spaces' width.
    pub fn default_tab(&self) -> Option<u32> {
        match self {
            Format::Docx(info) => Some(info.default_tab),
            Format::Text(_) | Format::Markdown(_) => None,
        }
    }

    /// The document's font table, for a format that has one (ADR 0009).
    pub fn fonts(&self) -> Option<&FontTable> {
        match self {
            Format::Docx(info) => Some(&info.fonts),
            Format::Text(_) | Format::Markdown(_) => None,
        }
    }
}

/// A Word document's page layout, as Slate's pageless column borrows it, and its fonts
/// (ADR 0009).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocxInfo {
    /// The last section's page width less its left and right margins and gutter, in
    /// twips (1/1440″).
    pub text_width: u32,
    /// Past a paragraph's last tab stop, tabs advance to multiples of this from the text's
    /// left edge (`w:defaultTabStop` in `word/settings.xml`), in twips.
    pub default_tab: u32,
    /// `word/fontTable.xml`: each font's class and embedded faces; shared, as documents
    /// are cloned for crash recovery.
    pub fonts: Arc<FontTable>,
}

impl DocxInfo {
    /// Letter (8.5″) with 1″ margins: 6.5″, Word's page when a document gives none.
    pub const LETTER_TEXT_WIDTH: u32 = 12_240 - 2 * 1_440;
    /// Word's default tab stop when its settings give none: 0.5″.
    pub const DEFAULT_TAB: u32 = 720;
}

impl Default for DocxInfo {
    fn default() -> Self {
        Self {
            text_width: Self::LETTER_TEXT_WIDTH,
            default_tab: Self::DEFAULT_TAB,
            fonts: Arc::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Block {
    Paragraph(Paragraph),
    ListItem(ListItem),
    CodeBlock(CodeBlock),
    Table(Table),
    Image(Image),
    PageBreak,
    Rule,
    Raw(Raw),
}

/// A paragraph: runs of text in one paragraph style.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Paragraph {
    /// Its style's id in the document's [`StyleSheet`]; empty (or unknown) for the
    /// document's default style.
    pub style: String,
    pub align: Align,
    /// Nesting beyond the paragraph's own style: the list level a list item's further
    /// paragraphs continue (1 = inside a first-level item), or the quote depth beyond one.
    pub indent: u8,
    pub runs: Vec<Run>,
    /// Its own tab stops, on top of its style's ([`StyleSheet::tab_stops`]), clears
    /// included, in position order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tabs: Vec<TabStop>,
    /// Its own spacing, indents, borders, and shading, on top of its style's
    /// ([`StyleSheet::spacing`], [`StyleSheet::borders`], [`StyleSheet::shading`]).
    #[serde(default, skip_serializing_if = "ParaOverrides::is_empty")]
    pub own: ParaOverrides,
}

impl Paragraph {
    pub fn new(style: impl Into<String>) -> Self {
        Self {
            style: style.into(),
            ..Self::default()
        }
    }

    /// The paragraph's text: its runs' text joined.
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }

    /// Append `text` in `style`, joining the last run when it has the same look.
    pub fn push(&mut self, text: &str, style: &RunStyle) {
        if text.is_empty() {
            return;
        }
        match self.runs.last_mut() {
            Some(last) if last.style == *style => last.text.push_str(text),
            _ => self.runs.push(Run {
                text: text.to_owned(),
                style: style.clone(),
            }),
        }
    }

    /// Join neighbouring runs that look the same and drop empty runs, so one text in one
    /// look is always one run.
    pub fn merge_runs(&mut self) {
        let mut merged: Vec<Run> = Vec::with_capacity(self.runs.len());
        for run in self.runs.drain(..) {
            if run.text.is_empty() {
                continue;
            }
            match merged.last_mut() {
                Some(last) if last.style == run.style => last.text.push_str(&run.text),
                _ => merged.push(run),
            }
        }
        self.runs = merged;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
    Justify,
}

/// Text with one look. Tabs are `\t`; a line break inside the paragraph is `\n`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Run {
    pub text: String,
    pub style: RunStyle,
}

/// A run's direct formatting: each look it sets over its paragraph's style (`None` leaves
/// the style's); see [`StyleSheet::look`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RunStyle {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    /// Inline code: shown in a monospace font.
    pub code: bool,
    /// Source Slate shows as written (Markdown's inline HTML).
    pub raw: bool,
    /// The font family's name.
    pub font: Option<String>,
    /// Font size in half-points (DOCX's unit).
    pub size: Option<u16>,
    pub color: Option<Rgb>,
    /// The link's target URL.
    pub link: Option<String>,
}

/// A list item: one paragraph with its marker. Items of a list follow each other as blocks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListItem {
    pub kind: ListKind,
    /// Nesting level, 0 for a top-level item.
    pub level: u8,
    pub paragraph: Paragraph,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ListKind {
    Bullet,
    /// `label` is the marker as shown, for example `3.` or `b)`.
    Ordered {
        number: u64,
        label: String,
    },
    Task {
        checked: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CodeBlock {
    /// The fenced block's info string's first word, if any.
    pub language: Option<String>,
    /// The code, each line ending in `\n`.
    pub text: String,
    /// The list level the block is nested in (0 outside lists).
    pub indent: u8,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Table {
    pub rows: Vec<TableRow>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct TableRow {
    pub cells: Vec<TableCell>,
    /// A header row (Markdown's first row, DOCX's `tblHeader`).
    pub header: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableCell {
    pub blocks: Vec<Block>,
    /// How many grid columns the cell spans.
    pub span: u32,
}

impl Default for TableCell {
    fn default() -> Self {
        Self {
            blocks: Vec::new(),
            span: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Image {
    pub alt: String,
    pub source: ImageSource,
    /// Width and height in pixels at 96 dpi, when the file says.
    pub size: Option<(f32, f32)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ImageSource {
    /// The picture's bytes, read from the file's package or from disk.
    Embedded {
        format: ImageFormat,
        bytes: Arc<[u8]>,
    },
    /// A URL or path not read (yet): Markdown's image destinations.
    Link(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Gif,
    /// Anything else (EMF, WMF, SVG, TIFF…): kept, not shown.
    Other,
}

impl ImageFormat {
    /// The format of an image from its first bytes.
    pub fn sniff(bytes: &[u8]) -> Self {
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            ImageFormat::Png
        } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            ImageFormat::Jpeg
        } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
            ImageFormat::Gif
        } else {
            ImageFormat::Other
        }
    }
}

/// Source Slate can't model, kept as written so a save can write it back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Raw {
    pub kind: RawKind,
    pub source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RawKind {
    /// A Markdown HTML block.
    Html,
    /// A Markdown footnote definition (`[^1]: …`).
    FootnoteDefinition,
    /// A Markdown link reference definition (`[label]: url "title"`).
    ReferenceDefinition,
}

impl Document {
    /// Call `f` with every piece of text the document shows, in order: runs, code, list
    /// markers excluded, table cells, image alt texts excluded, raw source included.
    pub fn visit_text(&self, f: &mut dyn FnMut(&str)) {
        visit_blocks(&self.blocks, f);
    }

    /// The number of words: runs of non-whitespace, counted across every text the
    /// document shows; blocks and runs that touch don't join words across blocks.
    pub fn word_count(&self) -> usize {
        let mut words = 0;
        let mut in_word = false;
        visit_blocks_with_breaks(&self.blocks, &mut |piece| match piece {
            None => in_word = false,
            Some(text) => {
                for c in text.chars() {
                    if c.is_whitespace() {
                        in_word = false;
                    } else if !in_word {
                        words += 1;
                        in_word = true;
                    }
                }
            }
        });
        words
    }
}

fn visit_blocks(blocks: &[Block], f: &mut dyn FnMut(&str)) {
    visit_blocks_with_breaks(blocks, &mut |piece| {
        if let Some(t) = piece {
            f(t);
        }
    });
}

/// Visit text pieces, with `None` between blocks.
fn visit_blocks_with_breaks(blocks: &[Block], f: &mut dyn FnMut(Option<&str>)) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => p.runs.iter().for_each(|r| f(Some(&r.text))),
            Block::ListItem(item) => item.paragraph.runs.iter().for_each(|r| f(Some(&r.text))),
            Block::CodeBlock(code) => f(Some(&code.text)),
            Block::Table(table) => {
                for cell in table.rows.iter().flat_map(|r| &r.cells) {
                    visit_blocks_with_breaks(&cell.blocks, f);
                }
            }
            Block::Raw(raw) => f(Some(&raw.source)),
            Block::Image(_) | Block::PageBreak | Block::Rule => {}
        }
        f(None);
    }
}
