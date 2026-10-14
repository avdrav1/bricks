//! Markdown (SLATE-2): CommonMark through `pulldown-cmark` with tables, strikethrough,
//! task lists, and footnotes, read into the document model.
//!
//! Headings, paragraphs, quotes, lists, code blocks, tables, rules, and images become their
//! blocks; emphasis, strong, strikethrough, inline code, and links become run looks. What
//! the model can't show is kept as written in [`Raw`] blocks: HTML blocks, footnote
//! definitions, and link reference definitions (which the parser resolves and doesn't
//! emit, so they are put back among the top-level blocks by their offset in the source).
//!
//! [`read_markdown_source`] also maps each top-level block back to the source lines it was
//! read from ([`MarkdownSource`]), so a save can copy unchanged blocks byte for byte
//! ([`write_markdown`](crate::write_markdown)).

use crate::text::{decode, split_lines};
use crate::{
    Align, Block, CodeBlock, Document, Format, Image, ImageFormat, ImageSource, ListItem, ListKind,
    Paragraph, Raw, RawKind, RunStyle, StyleSheet, Table, TableCell, TableRow, TextInfo,
};
use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::ops::Range;
use std::path::{Path, PathBuf};

/// Read a Markdown file's bytes (decoded as [`read_text`](crate::read_text) decodes).
pub fn read_markdown(bytes: &[u8]) -> Document {
    read(bytes).0
}

/// Where each top-level block of a Markdown file came from: the decoded source text and
/// the line each block starts on, so a save can copy the blocks that didn't change.
#[derive(Debug, Clone)]
pub struct MarkdownSource {
    /// The decoded text, without a byte-order mark.
    pub(crate) text: String,
    /// The offset of the line each top-level block starts on, never decreasing. Block `i`'s
    /// source runs to `starts[i + 1]` (the last block's to the end of the text); the text
    /// before `starts[0]` comes with the first block.
    pub(crate) starts: Vec<usize>,
    /// The exact source of each top-level image block (`None` for other blocks).
    pub(crate) images: Vec<Option<Range<usize>>>,
    /// The blocks, as the document shows them.
    pub(crate) blocks: Vec<Block>,
}

impl MarkdownSource {
    /// Replace the blocks this source describes, one for one: the blocks as the document
    /// shows them (linked pictures loaded, runs normalized).
    ///
    /// # Panics
    /// If `blocks` doesn't have one block for each of the source's.
    pub fn set_blocks(&mut self, blocks: Vec<Block>) {
        assert_eq!(
            blocks.len(),
            self.blocks.len(),
            "a Markdown source's blocks are replaced one for one"
        );
        self.blocks = blocks;
    }
}

/// [`read_markdown`], plus where each top-level block came from (its blocks are the
/// document's blocks as read).
pub fn read_markdown_source(bytes: &[u8]) -> (Document, MarkdownSource) {
    let (doc, text, mut starts, images) = read(bytes);
    // Back to the start of each block's line, never before the block before it.
    let mut floor = 0;
    for start in &mut starts {
        let line = text[..*start]
            .rfind(['\n', '\r'])
            .map_or(0, |i| i + 1)
            .max(floor);
        *start = line;
        floor = line;
    }
    let source = MarkdownSource {
        text,
        starts,
        images,
        blocks: doc.blocks.clone(),
    };
    (doc, source)
}

/// The document, its decoded text, and where each top-level block starts in it and, for
/// images, ends.
type Read = (Document, String, Vec<usize>, Vec<Option<Range<usize>>>);

fn read(bytes: &[u8]) -> Read {
    let (text, encoding) = decode(bytes);
    let (_, line_ending, final_newline) = split_lines(&text);
    let (blocks, starts, images) = parse(&text);
    let doc = Document {
        blocks,
        styles: StyleSheet::builtin(),
        format: Format::Markdown(TextInfo {
            encoding,
            line_ending,
            final_newline,
        }),
    };
    (doc, text, starts, images)
}

fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
}

/// Top-level blocks, where each starts, and each image's source.
type Located = (Vec<Block>, Vec<usize>, Vec<Option<Range<usize>>>);

/// The blocks of Markdown `text`, where each starts, and each image's source.
fn parse(text: &str) -> Located {
    let parser = Parser::new_ext(text, options()).into_offset_iter();
    let mut definitions: Vec<Range<usize>> = parser
        .reference_definitions()
        .iter()
        .map(|(_, def)| def.span.clone())
        .collect();
    definitions.sort_by_key(|r| r.start);
    let mut b = Builder::new(text);
    for (event, range) in parser {
        b.event(event, range);
    }
    b.flush();
    // Put the reference definitions back where they were among the top-level blocks.
    let n = b.blocks.len() + definitions.len();
    let (mut out, mut starts, mut images) = (
        Vec::with_capacity(n),
        Vec::with_capacity(n),
        Vec::with_capacity(n),
    );
    let mut defs = definitions.into_iter().peekable();
    for ((block, start), image) in b.blocks.into_iter().zip(b.starts).zip(b.images) {
        while let Some(def) = defs.next_if(|d| d.start < start) {
            starts.push(def.start);
            images.push(None);
            out.push(reference_definition(text, def));
        }
        out.push(block);
        starts.push(start);
        images.push(image);
    }
    for def in defs {
        starts.push(def.start);
        images.push(None);
        out.push(reference_definition(text, def));
    }
    (out, starts, images)
}

fn reference_definition(text: &str, span: Range<usize>) -> Block {
    Block::Raw(Raw {
        kind: RawKind::ReferenceDefinition,
        source: source(text, span),
    })
}

/// The source text of `range`, without its final line break.
fn source(text: &str, range: Range<usize>) -> String {
    text[range].trim_end_matches(['\n', '\r']).to_owned()
}

/// A container open around the current event.
enum Open {
    Quote,
    List {
        next: Option<u64>,
    },
    Item {
        kind: ListKind,
        level: u8,
        /// The item's marker has been shown (its first block was emitted).
        shown: bool,
        /// Where the item's marker is in the source.
        start: usize,
    },
    Table {
        table: Table,
        aligns: Vec<Alignment>,
    },
    Row(TableRow),
    Cell,
    Code {
        block: CodeBlock,
    },
    Image {
        dest: String,
        alt: String,
    },
    /// HTML blocks and footnote definitions: kept as source, their events skipped.
    Skip,
}

struct Builder<'t> {
    text: &'t str,
    /// The top-level blocks and where each starts in the source.
    blocks: Vec<Block>,
    starts: Vec<usize>,
    /// The source of each top-level image block.
    images: Vec<Option<Range<usize>>>,
    /// Blocks of the table cells open (innermost last).
    cells: Vec<Vec<Block>>,
    open: Vec<Open>,
    /// The paragraph being filled, and where it started.
    para: Option<(Paragraph, usize)>,
    /// The heading level of the next text, or `None` for the context's paragraph style.
    heading: Option<u8>,
    bold: u32,
    italic: u32,
    strike: u32,
    links: Vec<String>,
}

impl<'t> Builder<'t> {
    fn new(text: &'t str) -> Self {
        Self {
            text,
            blocks: Vec::new(),
            starts: Vec::new(),
            images: Vec::new(),
            cells: Vec::new(),
            open: Vec::new(),
            para: None,
            heading: None,
            bold: 0,
            italic: 0,
            strike: 0,
            links: Vec::new(),
        }
    }

    fn skipping(&self) -> bool {
        self.open.iter().any(|o| matches!(o, Open::Skip))
    }

    fn quote_depth(&self) -> u8 {
        self.open
            .iter()
            .filter(|o| matches!(o, Open::Quote))
            .count() as u8
    }

    /// The levels of list items around the current position (0 outside lists).
    fn item_depth(&self) -> u8 {
        self.open
            .iter()
            .filter(|o| matches!(o, Open::Item { .. }))
            .count() as u8
    }

    fn style(&self) -> RunStyle {
        let on = |n: u32| (n > 0).then_some(true);
        RunStyle {
            bold: on(self.bold),
            italic: on(self.italic),
            strike: on(self.strike),
            link: self.links.last().cloned(),
            ..RunStyle::default()
        }
    }

    /// Emit a block where it belongs: into the innermost table cell, or the document.
    /// The first block of a list item carries its marker: a paragraph becomes the
    /// [`ListItem`]; anything else gets an empty item before it.
    fn emit(&mut self, block: Block, start: usize) {
        let item = self.open.iter_mut().rev().find_map(|o| match o {
            Open::Item {
                kind,
                level,
                shown,
                start,
            } if !*shown => {
                *shown = true;
                Some((kind.clone(), *level, *start))
            }
            _ => None,
        });
        let (block, start) = match (item, block) {
            (Some((kind, level, at)), Block::Paragraph(paragraph)) => (
                Block::ListItem(ListItem {
                    kind,
                    level,
                    paragraph,
                    numbering: None,
                }),
                at,
            ),
            (Some((kind, level, at)), other) => {
                let marker = Block::ListItem(ListItem {
                    kind,
                    level,
                    paragraph: Paragraph::new("Normal"),
                    numbering: None,
                });
                self.push(marker, at);
                (other, start)
            }
            (None, block) => (block, start),
        };
        self.push(block, start);
    }

    fn push(&mut self, block: Block, start: usize) {
        match self.cells.last_mut() {
            Some(cell) => cell.push(block),
            None => {
                self.blocks.push(block);
                self.starts.push(start);
                self.images.push(None);
            }
        }
    }

    /// The paragraph inline content goes into, opened if needed (tight list items have
    /// text without a paragraph around it).
    fn paragraph(&mut self, at: usize) -> &mut Paragraph {
        if self.para.is_none() {
            let quotes = self.quote_depth();
            let items = self.item_depth();
            let style = match self.heading {
                Some(n) => format!("Heading{n}"),
                None if quotes > 0 => "Quote".to_owned(),
                None => "Normal".to_owned(),
            };
            let mut p = Paragraph::new(style);
            p.indent = items + quotes.saturating_sub(1);
            if let Some(Open::Cell) = self.open.last() {
                p.align = self.cell_align();
            }
            self.para = Some((p, at));
        }
        &mut self.para.as_mut().expect("just opened").0
    }

    /// The alignment of the table column the open cell is in.
    fn cell_align(&self) -> Align {
        let column = self.open.iter().rev().find_map(|o| match o {
            Open::Row(row) => Some(row.cells.len()),
            _ => None,
        });
        let aligns = self.open.iter().rev().find_map(|o| match o {
            Open::Table { aligns, .. } => Some(aligns),
            _ => None,
        });
        match column.and_then(|c| aligns?.get(c)) {
            Some(Alignment::Center) => Align::Center,
            Some(Alignment::Right) => Align::End,
            _ => Align::Start,
        }
    }

    /// Emit the paragraph being filled, if any.
    fn flush(&mut self) {
        if let Some((p, start)) = self.para.take() {
            self.emit(Block::Paragraph(p), start);
        }
    }

    fn text(&mut self, text: &str, style: RunStyle, at: usize) {
        if let Some(Open::Image { alt, .. }) = self.open.last_mut() {
            alt.push_str(text);
            return;
        }
        if let Some(Open::Code { block }) = self.open.last_mut() {
            block.text.push_str(text);
            return;
        }
        self.paragraph(at).push(text, &style);
    }

    fn event(&mut self, event: Event<'_>, range: Range<usize>) {
        if self.skipping() {
            match event {
                Event::End(TagEnd::HtmlBlock | TagEnd::FootnoteDefinition) => {}
                Event::Start(Tag::HtmlBlock | Tag::FootnoteDefinition(_)) => {
                    self.open.push(Open::Skip);
                    return;
                }
                _ => return,
            }
        }
        match event {
            Event::Start(tag) => self.start(tag, range),
            Event::End(tag) => self.end(tag, range),
            Event::Text(t) => {
                let style = self.style();
                self.text(&t, style, range.start);
            }
            Event::Code(t) => {
                let style = RunStyle {
                    code: true,
                    ..self.style()
                };
                self.text(&t, style, range.start);
            }
            Event::InlineHtml(t) => {
                let style = RunStyle {
                    raw: true,
                    ..self.style()
                };
                self.text(&t, style, range.start);
            }
            Event::FootnoteReference(_) => {
                let style = self.style();
                let reference = self.text[range.clone()].to_owned();
                self.text(&reference, style, range.start);
            }
            Event::SoftBreak => {
                let style = self.style();
                self.text(" ", style, range.start);
            }
            Event::HardBreak => {
                let style = self.style();
                self.text("\n", style, range.start);
            }
            Event::Rule => {
                self.flush();
                self.emit(Block::Rule, range.start);
            }
            Event::TaskListMarker(checked) => {
                if let Some(Open::Item { kind, .. }) = self
                    .open
                    .iter_mut()
                    .rev()
                    .find(|o| matches!(o, Open::Item { .. }))
                {
                    *kind = ListKind::Task { checked };
                }
            }
            // Only inside HTML blocks, which are skipped; math isn't enabled.
            Event::Html(_) | Event::InlineMath(_) | Event::DisplayMath(_) => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>, range: Range<usize>) {
        let at = range.start;
        match tag {
            Tag::Paragraph => self.flush(),
            Tag::Heading { level, .. } => {
                self.flush();
                self.heading = Some(heading_level(level));
            }
            Tag::BlockQuote(_) => {
                self.flush();
                self.open.push(Open::Quote);
            }
            Tag::CodeBlock(kind) => {
                self.flush();
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().map(str::to_owned)
                    }
                    CodeBlockKind::Indented => None,
                };
                let indent = self.item_depth();
                self.open.push(Open::Code {
                    block: CodeBlock {
                        language,
                        text: String::new(),
                        indent,
                    },
                });
            }
            Tag::HtmlBlock => {
                self.flush();
                let source = source(self.text, range);
                self.emit(
                    Block::Raw(Raw {
                        kind: RawKind::Html,
                        source,
                    }),
                    at,
                );
                self.open.push(Open::Skip);
            }
            Tag::FootnoteDefinition(_) => {
                self.flush();
                let source = source(self.text, range);
                self.emit(
                    Block::Raw(Raw {
                        kind: RawKind::FootnoteDefinition,
                        source,
                    }),
                    at,
                );
                self.open.push(Open::Skip);
            }
            Tag::List(first) => {
                self.flush();
                self.open.push(Open::List { next: first });
            }
            Tag::Item => {
                self.flush();
                let level = self
                    .open
                    .iter()
                    .filter(|o| matches!(o, Open::List { .. }))
                    .count()
                    .saturating_sub(1) as u8;
                let kind = match self.open.iter_mut().rev().find_map(|o| match o {
                    Open::List { next } => Some(next),
                    _ => None,
                }) {
                    Some(Some(n)) => {
                        let number = *n;
                        *n += 1;
                        ListKind::Ordered {
                            number,
                            label: format!("{number}."),
                        }
                    }
                    _ => ListKind::Bullet,
                };
                self.open.push(Open::Item {
                    kind,
                    level,
                    shown: false,
                    start: at,
                });
            }
            Tag::Table(aligns) => {
                self.flush();
                self.open.push(Open::Table {
                    table: Table::default(),
                    aligns,
                });
            }
            Tag::TableHead => self.open.push(Open::Row(TableRow {
                cells: Vec::new(),
                header: true,
            })),
            Tag::TableRow => self.open.push(Open::Row(TableRow::default())),
            Tag::TableCell => {
                self.open.push(Open::Cell);
                self.cells.push(Vec::new());
            }
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Strikethrough => self.strike += 1,
            Tag::Link { dest_url, .. } => self.links.push(dest_url.into_string()),
            Tag::Image { dest_url, .. } => {
                // An image is a block: the paragraph so far ends before it.
                self.flush();
                self.open.push(Open::Image {
                    dest: dest_url.into_string(),
                    alt: String::new(),
                });
            }
            // Not enabled: definition lists, superscript, subscript, metadata blocks.
            Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::Superscript
            | Tag::Subscript
            | Tag::MetadataBlock(_) => {}
        }
    }

    fn end(&mut self, tag: TagEnd, range: Range<usize>) {
        match tag {
            TagEnd::Paragraph => self.flush(),
            TagEnd::Heading(level) => {
                // An empty heading is still a heading.
                self.paragraph(range.start);
                self.flush();
                self.heading = None;
                let _ = level;
            }
            TagEnd::BlockQuote(_) | TagEnd::List(_) => {
                self.flush();
                self.open.pop();
            }
            TagEnd::Item => {
                self.flush();
                if let Some(Open::Item {
                    kind,
                    level,
                    shown: false,
                    start,
                }) = self.open.pop()
                {
                    // An empty item still shows its marker.
                    self.push(
                        Block::ListItem(ListItem {
                            kind,
                            level,
                            paragraph: Paragraph::new("Normal"),
                            numbering: None,
                        }),
                        start,
                    );
                }
            }
            TagEnd::CodeBlock => {
                if let Some(Open::Code { block }) = self.open.pop() {
                    self.emit(Block::CodeBlock(block), range.start);
                }
            }
            TagEnd::HtmlBlock | TagEnd::FootnoteDefinition => {
                self.open.pop();
            }
            TagEnd::Table => {
                if let Some(Open::Table { table, .. }) = self.open.pop() {
                    self.emit(Block::Table(table), range.start);
                }
            }
            TagEnd::TableHead | TagEnd::TableRow => {
                if let Some(Open::Row(row)) = self.open.pop() {
                    if let Some(Open::Table { table, .. }) = self.open.last_mut() {
                        table.rows.push(row);
                    }
                }
            }
            TagEnd::TableCell => {
                self.flush();
                self.open.pop();
                let blocks = self.cells.pop().unwrap_or_default();
                if let Some(Open::Row(row)) = self.open.last_mut() {
                    row.cells.push(TableCell { blocks, span: 1 });
                }
            }
            TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::Strikethrough => self.strike = self.strike.saturating_sub(1),
            TagEnd::Link => {
                self.links.pop();
            }
            TagEnd::Image => {
                if let Some(Open::Image { dest, alt }) = self.open.pop() {
                    let image = Block::Image(Image {
                        alt,
                        source: ImageSource::Link(dest),
                        size: None,
                    });
                    self.emit(image, range.start);
                    if self.cells.is_empty() {
                        if let Some(span) = self.images.last_mut() {
                            *span = Some(range);
                        }
                    }
                }
            }
            TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
            | TagEnd::Superscript
            | TagEnd::Subscript
            | TagEnd::MetadataBlock(_) => {}
        }
    }
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// The largest picture read from disk for a Markdown image.
const IMAGE_MAX_BYTES: u64 = 64 << 20;

/// Read the local pictures a Markdown document links to (paths relative to `base`, the
/// file's directory, or `file:` URLs) into the document, so it shows them. Remote URLs,
/// missing files, and files over 64 MiB stay links.
pub fn load_linked_images(blocks: &mut [Block], base: &Path) {
    for block in blocks {
        match block {
            Block::Image(image) => {
                if let ImageSource::Link(dest) = &image.source {
                    if let Some(bytes) = local_path(dest, base).and_then(|p| read_image(&p)) {
                        image.source = ImageSource::Embedded {
                            format: ImageFormat::sniff(&bytes),
                            bytes: bytes.into(),
                        };
                    }
                }
            }
            Block::Table(table) => {
                for cell in table.rows.iter_mut().flat_map(|r| &mut r.cells) {
                    load_linked_images(&mut cell.blocks, base);
                }
            }
            _ => {}
        }
    }
}

fn read_image(path: &Path) -> Option<Vec<u8>> {
    let meta = std::fs::metadata(path).ok()?;
    (meta.is_file() && meta.len() <= IMAGE_MAX_BYTES)
        .then(|| std::fs::read(path).ok())
        .flatten()
}

/// The file a Markdown destination names, if it is local.
fn local_path(dest: &str, base: &Path) -> Option<PathBuf> {
    let path = match dest.strip_prefix("file://") {
        Some(rest) => percent_decode(rest),
        None if dest.contains("://") || dest.starts_with("data:") || dest.is_empty() => {
            return None
        }
        None => percent_decode(dest.split(['?', '#']).next().unwrap_or(dest)),
    };
    let path = Path::new(&path);
    Some(if path.is_absolute() {
        path.to_owned()
    } else {
        base.join(path)
    })
}

/// `%XX` escapes decoded; a malformed escape stays as written.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match (bytes[i], bytes.get(i + 1), bytes.get(i + 2)) {
            (b'%', Some(&h), Some(&l)) if hex(h).is_some() && hex(l).is_some() => {
                out.push((hex(h).unwrap_or(0) * 16 + hex(l).unwrap_or(0)) as u8);
                i += 3;
            }
            (b, _, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
