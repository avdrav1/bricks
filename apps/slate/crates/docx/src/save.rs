//! Saving a Word document by patching the package it was read from (SLATE-8, ADR 0003):
//! only what was edited changes.
//!
//! The document's blocks are matched to the blocks the package was read with
//! ([`match_blocks_by`]; a numbered item's number doesn't count, Word numbers the list).
//! Every `w:p` and `w:tbl` of the body or of a table cell whose blocks are all still there,
//! in order, is copied byte for byte, and so is everything between them (bookmarks,
//! section properties, content controls' tags). In place of the elements whose blocks
//! changed, the new blocks are written from the model:
//!
//! - A paragraph starts from the properties of the paragraph it replaces (or, when it
//!   replaces none, of the paragraph copied next to it): its spacing, indents, and the
//!   paragraph mark's look stay; its style (by id: a built-in heading, title, or quote
//!   the document lacks is added, any other unknown id is the default), list numbering,
//!   and alignment are set where they differ. A run whose direct formatting is that of
//!   a run in that paragraph takes its run properties as they are (but not an automatic
//!   colour in a coloured style: the model reads it as the style's colour, and writes
//!   that); any other run's are its direct formatting exactly (bold, italic, underline,
//!   strikethrough with `w:val="0"` where turned off, font, size, colour; nothing its
//!   style gives), over the closest run's properties the model doesn't hold (other
//!   scripts' fonts, highlighting). A link is a `w:hyperlink` to the document's
//!   relationship for its URL (or a new one), or to a bookmark.
//! - A section break the replaced paragraph ended stays with the last paragraph written
//!   for it, with the page break after it; a page break is a `w:br w:type="page"` at the
//!   start of the paragraph after it or the end of the one before, else a section break
//!   with the body's page setup.
//! - A picture the document already has is written with its own `w:drawing`, a new one as
//!   a new media part.
//! - A table whose rows and cells are as they were keeps its markup, its cells patched as
//!   the body is; another is written whole, with single borders.
//! - Code blocks, raw source, and rules (pasted from Markdown) become paragraphs: code in a
//!   monospace font, a rule as a bottom border.
//!
//! What a rewritten paragraph held that the model doesn't (bookmarks, comments, fields,
//! footnote references, tracked changes) doesn't survive in it: a field or note mark is
//! its text, tracked changes are accepted. A missing heading, title, or quote style is
//! added to `styles.xml`, missing list numbering to `numbering.xml`, and new links and
//! pictures to the relationships and content types; every other part is copied as
//! compressed bytes.
//!
//! The package is written through `suite_file::save_atomic`; the check reads it back and
//! compares the document's text, line by line.

use crate::body::{Container, Element, TableSpan};
use crate::numbering::Numbering;
use crate::styles::{ParaProps, RunProps, Styles};
use crate::{dir_of, rel_kind, rels_part, DocxError, MainPart, Package, Rel};
use document_model::{
    match_blocks_by, Align, Block, Image, ImageFormat, ImageSource, ListKind, Paragraph, Rgb, Run,
    RunStyle, StyleSheet, Table,
};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, BufReader, Write};
use std::ops::Range;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use suite_file::{save_atomic, SaveError, WriteSeek};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive};

const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const RELS_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const NUMBERING_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml";
const XML_DECLARATION: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";
/// Twips across a new table: 16 cm, inside A4's and Letter's usual margins.
const TABLE_WIDTH: u32 = 9070;
const BREAK_RUN: &str = "<w:r><w:br w:type=\"page\"/></w:r>";
const MONOSPACE: &str = "Liberation Mono";
/// English Metric Units per pixel at 96 dpi.
const EMU_PER_PX: f64 = 9525.0;

/// `w:pPr`'s children in schema order (Word refuses others).
const PPR_ORDER: &[&str] = &[
    "pStyle",
    "keepNext",
    "keepLines",
    "pageBreakBefore",
    "framePr",
    "widowControl",
    "numPr",
    "suppressLineNumbers",
    "pBdr",
    "shd",
    "tabs",
    "suppressAutoHyphens",
    "kinsoku",
    "wordWrap",
    "overflowPunct",
    "topLinePunct",
    "autoSpaceDE",
    "autoSpaceDN",
    "bidi",
    "adjustRightInd",
    "snapToGrid",
    "spacing",
    "ind",
    "contextualSpacing",
    "mirrorIndents",
    "suppressOverlap",
    "jc",
    "textDirection",
    "textAlignment",
    "textboxTightWrap",
    "outlineLvl",
    "divId",
    "cnfStyle",
    "rPr",
    "sectPr",
    "pPrChange",
];

/// `w:rPr`'s children in schema order.
const RPR_ORDER: &[&str] = &[
    "rStyle",
    "rFonts",
    "b",
    "bCs",
    "i",
    "iCs",
    "caps",
    "smallCaps",
    "strike",
    "dstrike",
    "outline",
    "shadow",
    "emboss",
    "imprint",
    "noProof",
    "snapToGrid",
    "vanish",
    "webHidden",
    "color",
    "spacing",
    "w",
    "kern",
    "position",
    "sz",
    "szCs",
    "highlight",
    "u",
    "effect",
    "bdr",
    "shd",
    "fitText",
    "vertAlign",
    "rtl",
    "cs",
    "em",
    "lang",
    "eastAsianLayout",
    "specVanish",
    "oMath",
    "rPrChange",
];

/// Where a Word document's blocks came from in its package's main part, for [`save`].
#[derive(Debug, Clone)]
pub struct DocxSource {
    main: Arc<Main>,
    /// The blocks, as the document shows them.
    blocks: Vec<Block>,
}

#[derive(Debug)]
struct Main {
    name: String,
    xml: String,
    bom: bool,
    body: Container,
    /// The root element binds prefix `w` to WordprocessingML.
    w_bound: bool,
    /// The prefix the root element binds to relationships.
    r_prefix: Option<String>,
}

impl DocxSource {
    pub(crate) fn new(main: MainPart, blocks: &[Block]) -> Result<DocxSource, DocxError> {
        let body = main
            .body
            .ok_or_else(|| DocxError::Xml(format!("{} has no body", main.name)))?;
        let (w_bound, r_prefix) = root_namespaces(&main.xml);
        Ok(DocxSource {
            main: Arc::new(Main {
                name: main.name,
                xml: main.xml,
                bom: main.bom,
                body,
                w_bound,
                r_prefix,
            }),
            blocks: blocks.to_vec(),
        })
    }

    /// Replace the blocks this source describes, one for one: the blocks as the document
    /// shows them (runs normalized).
    ///
    /// # Panics
    /// If `blocks` doesn't have one block for each of the source's.
    pub fn set_blocks(&mut self, blocks: Vec<Block>) {
        assert_eq!(
            blocks.len(),
            self.blocks.len(),
            "a Word document's source blocks are replaced one for one"
        );
        self.blocks = blocks;
    }
}

/// Save `blocks` at `dest`, atomically, by patching `original`, the package `source` was
/// read from. Returns the source of the file written, for the next save (its blocks are
/// the blocks it reads back as, runs merged).
pub fn save(
    dest: &Path,
    original: &Path,
    blocks: &[Block],
    source: &DocxSource,
    cancel: &AtomicBool,
) -> Result<DocxSource, SaveError> {
    let written = RefCell::new(None);
    save_atomic(
        dest,
        cancel,
        |out| write(out, original, blocks, source),
        |tmp| {
            let (doc, mut back) = crate::read_source(tmp).map_err(|e| e.to_string())?;
            let mut shown = doc.blocks;
            normalize(&mut shown);
            let (want, got) = (lines(blocks), lines(&shown));
            if let Some((a, b)) = want.iter().zip(&got).find(|(a, b)| a != b) {
                return Err(format!("“{a}” reads back as “{b}”"));
            }
            if want.len() != got.len() {
                return Err(format!(
                    "{} lines of text read back as {}",
                    want.len(),
                    got.len()
                ));
            }
            // The next save copies what this one wrote for a block, unless it changes: the
            // source's blocks are the document's where they read back as the same block.
            if shown.len() == blocks.len() {
                for (back, saved) in shown.iter_mut().zip(blocks) {
                    if corresponds(back, saved) {
                        back.clone_from(saved);
                    }
                }
            }
            back.blocks = shown;
            *written.borrow_mut() = Some(back);
            Ok(())
        },
    )?;
    Ok(written
        .into_inner()
        .expect("a verified save has its source"))
}

fn other(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

fn zip_error(e: zip::result::ZipError) -> io::Error {
    match e {
        zip::result::ZipError::Io(e) => e,
        e => other(e),
    }
}

fn write(
    out: &mut (dyn WriteSeek + Send),
    original: &Path,
    blocks: &[Block],
    source: &DocxSource,
) -> io::Result<()> {
    let main = &source.main;
    let mut package = Package {
        zip: ZipArchive::new(BufReader::new(File::open(original)?)).map_err(zip_error)?,
        images: HashMap::new(),
    };
    let rels = package.rels(&main.name).map_err(other)?;
    let (styles, numbering) = package.styles_and_numbering(&rels).map_err(other)?;
    let dir = dir_of(&main.name);
    let part_of = |kind: &str, default: &str| {
        rels.values()
            .find(|r| !r.external && rel_kind(r) == kind)
            .map_or_else(|| format!("{dir}/{default}"), |r| r.target.clone())
    };
    let styles_part = part_of("styles", "styles.xml");
    let numbering_part = part_of("numbering", "numbering.xml");
    let rels_name = rels_part(&main.name);
    let styles_xml = package.text(&styles_part).map_err(other)?;
    let numbering_xml = package.text(&numbering_part).map_err(other)?;
    let names: Vec<String> = package.zip.file_names().map(str::to_owned).collect();

    let mut p = Patcher {
        xml: &main.xml,
        dir,
        styles,
        numbering,
        rels,
        w_decl: if main.w_bound {
            String::new()
        } else {
            format!(" xmlns:w=\"{W_NS}\"")
        },
        r: format!("{}:", main.r_prefix.as_deref().unwrap_or("r")),
        r_decl: if main.r_prefix.is_some() {
            String::new()
        } else {
            format!(" xmlns:r=\"{REL_NS}\"")
        },
        body_sect: main.body.sect.clone().map(|s| &main.xml[s]),
        pictures: Vec::new(),
        next_doc_pr: max_doc_pr(&main.xml) + 1,
        styles_xml,
        styles_changed: false,
        numbering_xml,
        numbering_changed: false,
        new_rels: String::new(),
        link_rels: HashMap::new(),
        media: Vec::new(),
        types: Vec::new(),
        names: names.iter().cloned().collect(),
        last_list: None,
    };
    index_pictures(&main.body, &source.blocks, &mut p.pictures);
    let patched = p.container(&main.body, 0..main.xml.len(), &source.blocks, blocks);

    // The parts that change.
    let mut parts: HashMap<String, Vec<u8>> = HashMap::new();
    if patched != main.xml {
        let mut bytes = Vec::with_capacity(patched.len() + 3);
        if main.bom {
            bytes.extend_from_slice(b"\xEF\xBB\xBF");
        }
        bytes.extend_from_slice(patched.as_bytes());
        parts.insert(main.name.clone(), bytes);
    }
    if p.styles_changed {
        if let Some(xml) = p.styles_xml.take() {
            parts.insert(styles_part, xml.into_bytes());
        }
    }
    let mut added: Vec<(String, Vec<u8>)> = Vec::new();
    if p.numbering_changed {
        if let Some(xml) = p.numbering_xml.take() {
            if !names.contains(&numbering_part) {
                let target = relative(dir, &numbering_part);
                let id = p.free_rid();
                p.new_rels.push_str(&format!(
                    r#"<Relationship Id="{id}" Type="{REL_NS}/numbering" Target="{}"/>"#,
                    escape(&target)
                ));
                p.types.push((
                    format!("/{numbering_part}"),
                    NUMBERING_TYPE.to_owned(),
                    false,
                ));
                added.push((numbering_part.clone(), xml.into_bytes()));
            } else {
                parts.insert(numbering_part, xml.into_bytes());
            }
        }
    }
    if !p.new_rels.is_empty() {
        let xml = match package.text(&rels_name).map_err(other)? {
            Some(xml) => insert_at_end(&xml, &p.new_rels)?,
            None => format!(
                "{XML_DECLARATION}<Relationships xmlns=\"{RELS_NS}\">{}</Relationships>",
                p.new_rels
            ),
        };
        if names.contains(&rels_name) {
            parts.insert(rels_name, xml.into_bytes());
        } else {
            added.push((rels_name, xml.into_bytes()));
        }
    }
    if !p.types.is_empty() {
        let xml = package
            .text("[Content_Types].xml")
            .map_err(other)?
            .ok_or_else(|| other("the package has no [Content_Types].xml"))?;
        let has_default = |ext: &str| {
            xml.match_indices("Extension=\"")
                .any(|(i, m)| xml[i + m.len()..].starts_with(&format!("{ext}\"")))
        };
        let new: String = p
            .types
            .iter()
            .filter(|(key, _, default)| !default || !has_default(key))
            .map(|(key, kind, default)| {
                if *default {
                    format!(r#"<Default Extension="{key}" ContentType="{kind}"/>"#)
                } else {
                    format!(
                        r#"<Override PartName="{}" ContentType="{kind}"/>"#,
                        escape(key)
                    )
                }
            })
            .collect();
        parts.insert(
            "[Content_Types].xml".into(),
            insert_at_end(&xml, &new)?.into_bytes(),
        );
    }
    for (name, bytes) in &p.media {
        added.push((name.clone(), bytes.to_vec()));
    }

    // The package: every part as it was but those changed, then the new ones.
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut zip = zip::ZipWriter::new(out);
    for (i, name) in names.iter().enumerate() {
        match parts.get(name) {
            Some(bytes) => {
                zip.start_file(name, options).map_err(zip_error)?;
                zip.write_all(bytes)?;
            }
            None => {
                let file = package.zip.by_index_raw(i).map_err(zip_error)?;
                zip.raw_copy_file(file).map_err(zip_error)?;
            }
        }
    }
    for (name, bytes) in &added {
        zip.start_file(name, options).map_err(zip_error)?;
        zip.write_all(bytes)?;
    }
    zip.finish().map_err(zip_error)?;
    Ok(())
}

/// Whether the root element binds `w` to WordprocessingML, and the prefix it binds to
/// relationships.
fn root_namespaces(xml: &str) -> (bool, Option<String>) {
    let mut r = Reader::from_str(xml);
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let mut w = false;
                let mut rel = None;
                for a in e.attributes().flatten() {
                    let Some(prefix) = a.key.as_ref().strip_prefix(b"xmlns:") else {
                        continue;
                    };
                    let value = a.value.as_ref();
                    if value == W_NS.as_bytes() && prefix == b"w" {
                        w = true;
                    }
                    if value == REL_NS.as_bytes() && rel.is_none() {
                        rel = std::str::from_utf8(prefix).ok().map(str::to_owned);
                    }
                }
                return (w, rel);
            }
            Ok(Event::Eof) | Err(_) => return (false, None),
            _ => {}
        }
    }
}

/// The largest drawing id (`wp:docPr id`) in `xml`.
fn max_doc_pr(xml: &str) -> u32 {
    xml.match_indices("docPr")
        .filter(|(i, _)| xml[..*i].ends_with([':', '<']))
        .filter_map(|(i, _)| {
            let tag = &xml[i..i + xml[i..].find('>')?];
            doc_pr_id(tag).map(|r| tag[r].parse::<u32>().ok())?
        })
        .max()
        .unwrap_or(0)
}

/// The range of the `id` attribute's value in a `docPr` tag.
fn doc_pr_id(tag: &str) -> Option<Range<usize>> {
    let at = tag.find(" id=\"").map(|i| i + 5)?;
    let end = at + tag[at..].find('"')?;
    Some(at..end)
}

/// Every picture of the blocks read, with its `w:drawing` or `w:pict` element.
fn index_pictures<'a>(
    c: &'a Container,
    blocks: &'a [Block],
    out: &mut Vec<(&'a ImageSource, Range<usize>)>,
) {
    for e in &c.elements {
        let mut spans = e.pictures.iter();
        for block in &blocks[e.blocks.clone()] {
            match block {
                Block::Image(image) => {
                    if let Some(span) = spans.next().cloned().flatten() {
                        out.push((&image.source, span));
                    }
                }
                Block::Table(table) => {
                    let Some(ts) = &e.table else { continue };
                    for (cells, row) in ts.rows.iter().zip(&table.rows) {
                        for (cs, cell) in cells.iter().zip(&row.cells) {
                            index_pictures(&cs.content, &cell.blocks, out);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// The text of `blocks` as a save writes it, line by line: what the check compares.
/// Pictures, page breaks, and rules have none; a linked picture is its alt text.
fn lines(blocks: &[Block]) -> Vec<String> {
    fn push(text: &str, out: &mut Vec<String>) {
        for line in text.split('\n') {
            let line: String = line.chars().filter(|&c| xml_char(c) && c != '\r').collect();
            if !line.trim().is_empty() {
                out.push(line);
            }
        }
    }
    fn walk(blocks: &[Block], out: &mut Vec<String>) {
        for block in blocks {
            match block {
                Block::Paragraph(p) => push(&p.text(), out),
                Block::ListItem(item) => push(&item.paragraph.text(), out),
                Block::CodeBlock(code) => push(&code.text, out),
                Block::Raw(raw) => push(&raw.source, out),
                Block::Table(table) => {
                    for cell in table.rows.iter().flat_map(|r| &r.cells) {
                        walk(&cell.blocks, out);
                    }
                }
                Block::Image(image) => {
                    if let ImageSource::Link(url) = &image.source {
                        push(link_text(image, url), out);
                    }
                }
                Block::PageBreak | Block::Rule => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(blocks, &mut out);
    out
}

/// What a picture that isn't in the package shows as: its alt text, else its URL.
fn link_text<'i>(image: &'i Image, url: &'i str) -> &'i str {
    if image.alt.trim().is_empty() {
        url
    } else {
        &image.alt
    }
}

/// Merge each paragraph's runs, as the editor does with a document it opens.
fn normalize(blocks: &mut [Block]) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => p.merge_runs(),
            Block::ListItem(item) => item.paragraph.merge_runs(),
            Block::Table(table) => {
                for cell in table.rows.iter_mut().flat_map(|r| &mut r.cells) {
                    normalize(&mut cell.blocks);
                }
            }
            _ => {}
        }
    }
}

/// A character XML 1.0 can hold.
fn xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..)
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c if xml_char(c) => out.push(c),
            _ => {}
        }
    }
    out
}

/// A part name relative to directory `dir` (`word`): `numbering.xml`, `../customXml/a.xml`.
fn relative(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        return name.to_owned();
    }
    match name.strip_prefix(&format!("{dir}/")) {
        Some(rest) => rest.to_owned(),
        None => format!("/{name}"),
    }
}

/// `xml` with `content` added as the root element's last children.
fn insert_at_end(xml: &str, content: &str) -> io::Result<String> {
    let at = root_end(xml).ok_or_else(|| other("a part to patch isn't well-formed"))?;
    Ok(format!("{}{content}{}", &xml[..at], &xml[at..]))
}

/// Where the root element's end tag starts.
fn root_end(xml: &str) -> Option<usize> {
    let mut r = Reader::from_str(xml);
    let mut depth = 0usize;
    loop {
        let before = r.buffer_position() as usize;
        match r.read_event().ok()? {
            Event::Start(_) => depth += 1,
            Event::End(_) => {
                depth -= 1;
                if depth == 0 {
                    return Some(before);
                }
            }
            Event::Eof => return None,
            _ => {}
        }
    }
}

/// Where the first child of the root element named `name` (local name) starts.
fn child_start(xml: &str, name: &[u8]) -> Option<usize> {
    let mut r = Reader::from_str(xml);
    let mut depth = 0usize;
    loop {
        let before = r.buffer_position() as usize;
        match r.read_event().ok()? {
            Event::Start(e) | Event::Empty(e) if depth == 1 && crate::xml::local(&e) == name => {
                return Some(before)
            }
            Event::Start(_) => depth += 1,
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Eof => return None,
            _ => {}
        }
    }
}

/// A property element (`w:pPr`, `w:rPr`, `w:sectPr`) taken apart: its start tag's name
/// and attributes, and its children.
#[derive(Debug, Clone, Default)]
struct Props {
    /// The qualified name.
    name: String,
    /// The start tag's attributes, as written (with their leading space).
    attrs: String,
    /// Each child's local name and markup.
    children: Vec<(String, String)>,
}

impl Props {
    fn parse(xml: &str) -> Props {
        let mut r = Reader::from_str(xml);
        let mut props = Props::default();
        let mut open = false;
        loop {
            let before = r.buffer_position() as usize;
            let Ok(event) = r.read_event() else {
                return props;
            };
            let at = r.buffer_position() as usize;
            match event {
                Event::Start(e) | Event::Empty(e) if !open => {
                    let empty = xml[before..at].ends_with("/>");
                    props.name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                    let tag = &xml[before..at];
                    let attrs = &tag[1 + props.name.len()..tag.len() - if empty { 2 } else { 1 }];
                    props.attrs = attrs.trim_end().to_owned();
                    if empty {
                        return props;
                    }
                    open = true;
                }
                Event::Start(e) => {
                    let local = String::from_utf8_lossy(crate::xml::local(&e)).into_owned();
                    let name = e.name().as_ref().to_vec();
                    if r.read_to_end(quick_xml::name::QName(&name)).is_err() {
                        return props;
                    }
                    let end = r.buffer_position() as usize;
                    props.children.push((local, xml[before..end].to_owned()));
                }
                Event::Empty(e) => {
                    let local = String::from_utf8_lossy(crate::xml::local(&e)).into_owned();
                    props.children.push((local, xml[before..at].to_owned()));
                }
                Event::End(_) | Event::Eof => return props,
                _ => {}
            }
        }
    }

    fn remove(&mut self, names: &[&str]) {
        self.children.retain(|(n, _)| !names.contains(&n.as_str()));
    }

    fn push(&mut self, name: &str, xml: String) {
        self.children.push((name.to_owned(), xml));
    }

    /// The children in `order`; a child the order doesn't know stays after the one before
    /// it.
    fn sort(&mut self, order: &[&str]) {
        let mut key = 0;
        let mut keyed: Vec<(usize, (String, String))> = self
            .children
            .drain(..)
            .map(|child| {
                if let Some(k) = order.iter().position(|o| *o == child.0) {
                    key = k;
                }
                (key, child)
            })
            .collect();
        keyed.sort_by_key(|(k, _)| *k);
        self.children = keyed.into_iter().map(|(_, c)| c).collect();
    }

    /// The children alone, inside a bare element, for reading their properties.
    fn bare(&self, name: &str) -> String {
        let mut s = format!("<{name}>");
        for (_, xml) in &self.children {
            s.push_str(xml);
        }
        s.push_str(&format!("</{name}>"));
        s
    }

    fn para_props(&self) -> ParaProps {
        let xml = self.bare("pPr");
        let mut r = crate::xml::reader(&xml);
        let _ = r.read_event();
        ParaProps::read(&mut r).unwrap_or_default()
    }

    fn run_props(&self) -> RunProps {
        let xml = self.bare("rPr");
        let mut r = crate::xml::reader(&xml);
        let _ = r.read_event();
        RunProps::read(&mut r).unwrap_or_default()
    }

    /// The element again, `w:pPr` or `w:rPr` when it is new; nothing when it has no
    /// children.
    fn write(&self, name: &str) -> String {
        if self.children.is_empty() {
            return String::new();
        }
        let name = if self.name.is_empty() {
            name
        } else {
            &self.name
        };
        let mut s = format!("<{name}{}>", self.attrs);
        for (_, xml) in &self.children {
            s.push_str(xml);
        }
        s.push_str(&format!("</{name}>"));
        s
    }
}

/// A paragraph the save replaces or writes next to, whose properties new paragraphs start
/// from.
#[derive(Debug)]
struct Template<'a> {
    /// Its `w:pPr`.
    ppr: Option<&'a str>,
    /// Its paragraph style's id (the default's when it names none).
    style: String,
    list: bool,
    num: Option<u32>,
    /// Its runs' `w:rPr`s, each once, with their properties (`""`: a run without one).
    runs: Vec<(&'a str, RunProps)>,
    /// The section it ends (`w:pPr/w:sectPr`) and whether the next section starts a page;
    /// `None` once given to a paragraph written.
    sect: Option<(&'a str, bool)>,
}

impl<'a> Template<'a> {
    /// The template of paragraph element `e`; `None` for a table.
    fn of(xml: &'a str, e: &Element, styles: &Styles, numbering: &Numbering) -> Option<Self> {
        if e.table.is_some() {
            return None;
        }
        let s = &xml[e.span.clone()];
        let mut r = Reader::from_str(s);
        let mut ppr = None;
        let mut sect = None;
        let mut runs: Vec<&'a str> = Vec::new();
        let mut depth = 0usize;
        loop {
            let before = r.buffer_position() as usize;
            let event = r.read_event().ok()?;
            let at = r.buffer_position() as usize;
            match event {
                Event::Start(e) => match crate::xml::local(&e) {
                    b"pPr" if depth == 1 && ppr.is_none() => {
                        loop {
                            let inner = r.buffer_position() as usize;
                            match r.read_event().ok()? {
                                Event::Start(c) if crate::xml::local(&c) == b"sectPr" => {
                                    let name = c.name().as_ref().to_vec();
                                    r.read_to_end(quick_xml::name::QName(&name)).ok()?;
                                    sect = Some(inner..r.buffer_position() as usize);
                                }
                                Event::Start(c) => {
                                    let name = c.name().as_ref().to_vec();
                                    r.read_to_end(quick_xml::name::QName(&name)).ok()?;
                                }
                                Event::End(_) => break,
                                Event::Eof => return None,
                                _ => {}
                            }
                        }
                        ppr = Some(before..r.buffer_position() as usize);
                    }
                    b"r" => {
                        let mut rpr = "";
                        loop {
                            let inner = r.buffer_position() as usize;
                            let event = r.read_event().ok()?;
                            let after = r.buffer_position() as usize;
                            match event {
                                Event::Start(c) => {
                                    let name = c.name().as_ref().to_vec();
                                    r.read_to_end(quick_xml::name::QName(&name)).ok()?;
                                    if crate::xml::local(&c) == b"rPr" {
                                        rpr = &s[inner..r.buffer_position() as usize];
                                    }
                                }
                                Event::Empty(c) if crate::xml::local(&c) == b"rPr" => {
                                    rpr = &s[inner..after];
                                }
                                Event::End(_) => break,
                                Event::Eof => return None,
                                _ => {}
                            }
                        }
                        if !runs.contains(&rpr) {
                            runs.push(rpr);
                        }
                    }
                    b"del" | b"moveFrom" | b"Fallback" => {
                        let name = e.name().as_ref().to_vec();
                        r.read_to_end(quick_xml::name::QName(&name)).ok()?;
                    }
                    _ => depth += 1,
                },
                Event::Empty(e) if depth == 1 && crate::xml::local(&e) == b"pPr" => {
                    ppr = Some(before..at);
                }
                Event::End(_) => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        break;
                    }
                }
                Event::Eof => break,
                _ => {}
            }
        }
        let ppr = ppr.map(|r| &s[r]);
        let props = ppr
            .map(|p| Props::parse(p).para_props())
            .unwrap_or_default();
        let list = numbering
            .list_of(styles, &props)
            .is_some_and(|(n, l)| numbering.bulleted(n, l).is_some());
        let sect = sect.map(|r| {
            let xml = &s[r];
            let continuous = Props::parse(xml)
                .children
                .iter()
                .any(|(n, x)| n == "type" && x.contains("\"continuous\""));
            (xml, !continuous)
        });
        Some(Template {
            ppr,
            style: styles.paragraph_id(props.style.as_deref()).to_owned(),
            list,
            num: e.num,
            runs: runs
                .into_iter()
                .map(|x| {
                    let props = if x.is_empty() {
                        RunProps::default()
                    } else {
                        Props::parse(x).run_props()
                    };
                    (x, props)
                })
                .collect(),
            sect,
        })
    }
}

/// What the blocks written in place of changed elements start from: the paragraphs
/// replaced, in order; the paragraph copied next to them; the tables replaced, in order.
#[derive(Default)]
struct Gap<'a> {
    paras: Vec<Template<'a>>,
    neighbor: Option<Template<'a>>,
    tables: Vec<(&'a Element, &'a TableSpan, &'a Table)>,
}

impl Gap<'_> {
    /// Which replaced paragraph the `n`th paragraph written starts from: one each, in
    /// order, the last one for the rest.
    fn para(&self, n: usize) -> Option<usize> {
        (!self.paras.is_empty()).then(|| n.min(self.paras.len() - 1))
    }
}

/// How many of the looks the model keeps two runs' direct formatting sets the same, of
/// [`LOOKS`] (links and Slate's own looks aside).
fn agreeing(a: &RunStyle, b: &RunStyle) -> usize {
    [
        a.bold == b.bold,
        a.italic == b.italic,
        a.underline == b.underline,
        a.strike == b.strike,
        a.font == b.font,
        a.size == b.size,
        a.color == b.color,
    ]
    .iter()
    .filter(|s| **s)
    .count()
}

/// The looks [`agreeing`] compares.
const LOOKS: usize = 7;

/// `w:rPr` children the model's run looks write, and the character style (resolved into
/// them when read).
const RUN_LOOKS: &[&str] = &[
    "rStyle", "b", "i", "u", "strike", "dstrike", "sz", "szCs", "color",
];
/// `w:rFonts` attributes the model's font writes.
const FONT_SLOTS: &[&str] = &["ascii", "hAnsi", "asciiTheme", "hAnsiTheme"];

/// A `w:rFonts` with its Latin font `font` (`None`: left to the style) over `fonts`, the
/// one written before, whose other scripts' fonts stay; `None` when it names no font.
fn latin_font(fonts: Option<&str>, font: Option<&str>) -> Option<String> {
    let mut prefix = "w:".to_owned();
    let mut attrs = String::new();
    if let Some(fonts) = fonts {
        if let Ok(Event::Empty(e) | Event::Start(e)) = Reader::from_str(fonts).read_event() {
            let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
            prefix = name.strip_suffix("rFonts").unwrap_or("w:").to_owned();
            for a in e.attributes().flatten() {
                let local = a.key.local_name();
                if FONT_SLOTS.iter().any(|s| s.as_bytes() == local.as_ref()) {
                    continue;
                }
                let value = String::from_utf8_lossy(&a.value).replace('"', "&quot;");
                attrs.push_str(&format!(
                    " {}=\"{value}\"",
                    String::from_utf8_lossy(a.key.as_ref())
                ));
            }
        }
    }
    let latin = font.map(|f| {
        let f = escape(f);
        format!(" {prefix}ascii=\"{f}\" {prefix}hAnsi=\"{f}\"")
    });
    if latin.is_none() && attrs.is_empty() {
        return None;
    }
    Some(format!(
        "<{prefix}rFonts{}{attrs}/>",
        latin.unwrap_or_default()
    ))
}

struct Patcher<'a> {
    xml: &'a str,
    /// The main part's directory.
    dir: &'a str,
    styles: Styles,
    numbering: Numbering,
    rels: HashMap<String, Rel>,
    /// Namespace declarations a new top-level element needs (when the root lacks them).
    w_decl: String,
    /// The relationships prefix, with its colon, and its declaration when the root lacks
    /// it.
    r: String,
    r_decl: String,
    body_sect: Option<&'a str>,
    pictures: Vec<(&'a ImageSource, Range<usize>)>,
    next_doc_pr: u32,
    styles_xml: Option<String>,
    styles_changed: bool,
    numbering_xml: Option<String>,
    numbering_changed: bool,
    /// Relationship elements to add.
    new_rels: String,
    link_rels: HashMap<String, String>,
    /// New media parts.
    media: Vec<(String, Arc<[u8]>)>,
    /// Content types to add: an extension's default or a part's override, and its type.
    types: Vec<(String, String, bool)>,
    names: HashSet<String>,
    /// The list instance and kind of the last list item written or copied.
    last_list: Option<(u32, bool)>,
}

impl<'a> Patcher<'a> {
    /// `xml[range]`, the markup of container `c` (the body: the whole part; a cell: its
    /// `w:tc`), with the blocks it was read with (`old`) become `new`.
    fn container(
        &mut self,
        c: &'a Container,
        range: Range<usize>,
        old: &'a [Block],
        new: &[Block],
    ) -> String {
        let mut kept = match_blocks_by(old, new, same_block);
        let mut new_of = vec![None; old.len()];
        for (j, i) in kept.iter().enumerate() {
            if let Some(i) = i {
                new_of[*i] = Some(j);
            }
        }
        // An element is copied when all its blocks are, one after the other.
        let mut copied = vec![false; c.elements.len()];
        for (k, e) in c.elements.iter().enumerate() {
            if e.blocks.is_empty() {
                continue;
            }
            let first = new_of[e.blocks.start];
            copied[k] = first.is_some()
                && e.blocks
                    .clone()
                    .enumerate()
                    .all(|(n, i)| new_of[i] == first.map(|f| f + n));
            if !copied[k] {
                for i in e.blocks.clone() {
                    if let Some(j) = new_of[i].take() {
                        kept[j] = None;
                    }
                }
            }
        }
        let mut out = String::with_capacity(range.len());
        let mut at = range.start;
        let mut j = 0;
        let mut replaced: Vec<usize> = Vec::new();
        let mut before: Option<usize> = None;
        for (k, e) in c.elements.iter().enumerate() {
            if e.blocks.is_empty() {
                continue;
            }
            if !copied[k] {
                replaced.push(k);
                continue;
            }
            let at_new = new_of[e.blocks.start].expect("a copied element's blocks are kept");
            self.gap(
                &mut out,
                &mut at,
                c,
                old,
                &replaced,
                (before, Some(k)),
                &new[j..at_new],
            );
            out.push_str(&self.xml[at..e.span.end]);
            at = e.span.end;
            if old[e.blocks.clone()]
                .iter()
                .any(|b| matches!(b, Block::ListItem(_)))
            {
                if let Some(num) = e.num {
                    let bullet = old[e.blocks.clone()].iter().any(|b| {
                        matches!(
                            b,
                            Block::ListItem(i) if matches!(i.kind, ListKind::Bullet | ListKind::Task { .. })
                        )
                    });
                    self.last_list = Some((num, bullet));
                }
            }
            j = at_new + e.blocks.len();
            before = Some(k);
            replaced.clear();
        }
        self.gap(
            &mut out,
            &mut at,
            c,
            old,
            &replaced,
            (before, None),
            &new[j..],
        );
        out.push_str(&self.xml[at..range.end]);
        out
    }

    /// Write `blocks` in place of the elements `replaced`, between the copied elements
    /// `around`, copying what lies between up to the last element replaced.
    #[allow(clippy::too_many_arguments)]
    fn gap(
        &mut self,
        out: &mut String,
        at: &mut usize,
        c: &'a Container,
        old: &'a [Block],
        replaced: &[usize],
        around: (Option<usize>, Option<usize>),
        blocks: &[Block],
    ) {
        if replaced.is_empty() && blocks.is_empty() {
            return;
        }
        let insert = match (replaced.first(), around) {
            (Some(&k), _) => c.elements[k].span.start,
            (None, (Some(b), _)) => c.elements[b].span.end,
            (None, (None, Some(a))) => c.elements[a].span.start,
            (None, (None, None)) => c.tail,
        };
        out.push_str(&self.xml[*at..insert]);
        *at = insert;
        let mut gap = Gap::default();
        for &k in replaced {
            let e = &c.elements[k];
            match (&e.table, old.get(e.blocks.start)) {
                (Some(ts), Some(Block::Table(t))) => gap.tables.push((e, ts, t)),
                _ => gap
                    .paras
                    .extend(Template::of(self.xml, e, &self.styles, &self.numbering)),
            }
        }
        if gap.paras.is_empty() {
            let neighbor = |k: Option<usize>| {
                k.and_then(|k| {
                    Template::of(self.xml, &c.elements[k], &self.styles, &self.numbering)
                })
            };
            gap.neighbor = neighbor(around.0)
                .or_else(|| neighbor(around.1))
                .map(|mut t| {
                    t.sect = None;
                    t
                });
        }
        let fresh = self.fresh(blocks, &mut gap);
        out.push_str(&fresh);
        for &k in replaced {
            let span = &c.elements[k].span;
            out.push_str(&self.xml[*at..span.start]);
            *at = span.end;
        }
    }

    /// The markup of `blocks`, written from the model, starting from `gap`'s templates.
    fn fresh(&mut self, blocks: &[Block], gap: &mut Gap<'a>) -> String {
        let mut out = String::new();
        // Runs the next paragraph starts with: page breaks and pictures.
        let mut lead = String::new();
        // The last paragraph written, without its end tag, while a page break may end it.
        let mut open: Option<String> = None;
        let mut paras = 0usize;
        let mut tables = 0usize;
        let close = |out: &mut String, open: &mut Option<String>| {
            if let Some(p) = open.take() {
                out.push_str(&p);
                out.push_str("</w:p>");
            }
        };
        let paragraph_like = |b: &Block| {
            matches!(
                b,
                Block::Paragraph(_)
                    | Block::ListItem(_)
                    | Block::CodeBlock(_)
                    | Block::Raw(_)
                    | Block::Rule
            )
        };
        let mut i = 0;
        while i < blocks.len() {
            match &blocks[i] {
                Block::Image(image) => {
                    close(&mut out, &mut open);
                    let run = self.picture(image);
                    lead.push_str(&run);
                }
                Block::PageBreak => {
                    let next = blocks[i + 1..]
                        .iter()
                        .find(|b| !matches!(b, Block::Image(_)));
                    if next.is_some_and(paragraph_like) {
                        close(&mut out, &mut open);
                        lead.push_str(BREAK_RUN);
                    } else if let (Some(p), true) = (open.as_mut(), lead.is_empty()) {
                        p.push_str(BREAK_RUN);
                        close(&mut out, &mut open);
                    } else {
                        close(&mut out, &mut open);
                        self.pictures_paragraph(&mut out, &mut lead);
                        let sect = self.page_section(gap);
                        out.push_str(&format!("<w:p{}><w:pPr>{sect}</w:pPr></w:p>", self.w_decl));
                    }
                }
                Block::Table(table) => {
                    close(&mut out, &mut open);
                    self.pictures_paragraph(&mut out, &mut lead);
                    let template = gap.tables.get(tables).copied();
                    tables += 1;
                    let xml = self.table(table, template);
                    out.push_str(&xml);
                }
                block => {
                    close(&mut out, &mut open);
                    let t = gap.para(paras);
                    paras += 1;
                    // The section the template ended goes with the last paragraph written
                    // from it, and the page break after it with it.
                    let last_from_t = match t {
                        Some(t) if t + 1 < gap.paras.len() => true,
                        Some(_) => !blocks[i + 1..].iter().any(paragraph_like),
                        None => false,
                    };
                    let mut sect = None;
                    if let Some(t) = t.filter(|_| last_from_t) {
                        if let Some((xml, page)) = gap.paras[t].sect {
                            let breaks = matches!(blocks.get(i + 1), Some(Block::PageBreak));
                            if !page || breaks {
                                sect = Some(xml);
                                gap.paras[t].sect = None;
                                if page {
                                    i += 1;
                                }
                            }
                        }
                    }
                    let template = match t {
                        Some(t) => gap.paras.get(t),
                        None => gap.neighbor.as_ref(),
                    };
                    let lead = std::mem::take(&mut lead);
                    let written = self.paragraph_block(block, template, sect, &lead, &mut out);
                    open = Some(written);
                }
            }
            i += 1;
        }
        close(&mut out, &mut open);
        self.pictures_paragraph(&mut out, &mut lead);
        out
    }

    /// Pictures left over with no paragraph to start: a paragraph of their own.
    fn pictures_paragraph(&self, out: &mut String, lead: &mut String) {
        if !lead.is_empty() {
            out.push_str(&format!("<w:p{}>{lead}</w:p>", self.w_decl));
            lead.clear();
        }
    }

    /// Section properties for a page break with no paragraph to go with: a section the
    /// gap's paragraphs ended that no paragraph took, else the body's page setup; always
    /// starting a new page.
    fn page_section(&self, gap: &mut Gap<'a>) -> String {
        let taken = gap
            .paras
            .iter_mut()
            .find_map(|t| t.sect.take().map(|s| s.0));
        let Some(xml) = taken.or(self.body_sect) else {
            return "<w:sectPr/>".into();
        };
        let mut props = Props::parse(xml);
        props.remove(&["type"]);
        if props.children.is_empty() {
            let name = if props.name.is_empty() {
                "w:sectPr"
            } else {
                &props.name
            };
            return format!("<{name}{}/>", props.attrs);
        }
        props.write("w:sectPr")
    }

    /// Write a paragraph-like block; returns the last paragraph, without its end tag
    /// (earlier ones, for a code block's lines, go to `out`).
    fn paragraph_block(
        &mut self,
        block: &Block,
        template: Option<&Template<'a>>,
        sect: Option<&str>,
        lead: &str,
        out: &mut String,
    ) -> String {
        match block {
            Block::Paragraph(p) => self.paragraph(p, None, template, sect, lead, None),
            Block::ListItem(item) => {
                let bullet = matches!(item.kind, ListKind::Bullet | ListKind::Task { .. });
                let list = Some((bullet, item.level.min(8)));
                self.paragraph(&item.paragraph, list, template, sect, lead, None)
            }
            Block::CodeBlock(code) => {
                let text = code.text.strip_suffix('\n').unwrap_or(&code.text);
                let style = RunStyle {
                    code: true,
                    ..RunStyle::default()
                };
                let mut lines = text.split('\n').peekable();
                let mut lead = lead;
                loop {
                    let line = lines.next().unwrap_or_default();
                    let mut p = Paragraph::default();
                    p.push(line, &style);
                    let last = lines.peek().is_none();
                    let written = self.paragraph(
                        &p,
                        None,
                        template,
                        if last { sect } else { None },
                        lead,
                        None,
                    );
                    lead = "";
                    if last {
                        return written;
                    }
                    out.push_str(&written);
                    out.push_str("</w:p>");
                }
            }
            Block::Raw(raw) => {
                let mut p = Paragraph::default();
                p.push(
                    raw.source.trim_end_matches(['\n', '\r']),
                    &RunStyle::default(),
                );
                self.paragraph(&p, None, template, sect, lead, None)
            }
            Block::Rule => {
                let border = "<w:pBdr><w:bottom w:val=\"single\" w:sz=\"6\" w:space=\"1\" \
                              w:color=\"auto\"/></w:pBdr>";
                let p = Paragraph::default();
                self.paragraph(&p, None, template, sect, lead, Some(border))
            }
            _ => unreachable!("not a paragraph"),
        }
    }

    /// A paragraph without its end tag: `list` is its list kind (bulleted) and level,
    /// `lead` runs it starts with, `border` a `w:pBdr`.
    fn paragraph(
        &mut self,
        p: &Paragraph,
        list: Option<(bool, u8)>,
        template: Option<&Template<'a>>,
        sect: Option<&str>,
        lead: &str,
        border: Option<&str>,
    ) -> String {
        let ppr = self.ppr(&p.style, p.align, list, template, sect, border);
        let color = self.style_color(&self.written_style(&p.style));
        let mut s = format!("<w:p{}>{ppr}{lead}", self.w_decl);
        let mut i = 0;
        while i < p.runs.len() {
            let link = p.runs[i].style.link.clone();
            let end = p.runs[i..]
                .iter()
                .position(|r| r.style.link != link)
                .map_or(p.runs.len(), |n| i + n);
            let close = match &link {
                Some(url) => {
                    s.push_str(&self.hyperlink(url));
                    "</w:hyperlink>"
                }
                None => "",
            };
            for run in &p.runs[i..end] {
                self.run(&mut s, run, color, template);
            }
            s.push_str(close);
            i = end;
        }
        s
    }

    /// A paragraph's `w:pPr`.
    fn ppr(
        &mut self,
        style: &str,
        align: Align,
        list: Option<(bool, u8)>,
        template: Option<&Template<'a>>,
        sect: Option<&str>,
        border: Option<&str>,
    ) -> String {
        let mut props = template
            .and_then(|t| t.ppr)
            .map(Props::parse)
            .unwrap_or_default();
        props.remove(&["pageBreakBefore", "sectPr", "pPrChange"]);
        let style = self.written_style(style);
        if template.map(|t| t.style.as_str()) != Some(style.as_str()) {
            props.remove(&["pStyle"]);
            if let Some(id) = self.style_id(&style) {
                props.push("pStyle", format!("<w:pStyle w:val=\"{}\"/>", escape(&id)));
            }
        }
        if template.is_some_and(|t| t.list != list.is_some()) {
            props.remove(&["ind"]);
        }
        let listed = |p: &Patcher<'_>, props: &Props| {
            p.numbering
                .list_of(&p.styles, &props.para_props())
                .filter(|&(n, l)| p.numbering.bulleted(n, l).is_some())
        };
        match list {
            Some((bullet, level)) => {
                let now = listed(self, &props);
                let fits = now.is_some_and(|(n, l)| {
                    l == level && self.numbering.bulleted(n, l) == Some(bullet)
                });
                let num = match now {
                    Some((n, _)) if fits => n,
                    _ => {
                        props.remove(&["numPr"]);
                        let num = self.num_for(bullet, level, template);
                        props.push(
                            "numPr",
                            format!(
                                "<w:numPr><w:ilvl w:val=\"{level}\"/><w:numId w:val=\"{num}\"/></w:numPr>"
                            ),
                        );
                        num
                    }
                };
                self.last_list = Some((num, bullet));
            }
            None => {
                if listed(self, &props).is_some() {
                    props.remove(&["numPr"]);
                    if listed(self, &props).is_some() {
                        props.push("numPr", "<w:numPr><w:numId w:val=\"0\"/></w:numPr>".into());
                    }
                }
            }
        }
        props.remove(&["jc"]);
        let styled = self
            .styles
            .paragraph_props(&props.para_props())
            .align
            .unwrap_or_default();
        if styled != align {
            let jc = match align {
                Align::Start => "left",
                Align::Center => "center",
                Align::End => "right",
                Align::Justify => "both",
            };
            props.push("jc", format!("<w:jc w:val=\"{jc}\"/>"));
        }
        if let Some(border) = border {
            props.remove(&["pBdr"]);
            props.push("pBdr", border.to_owned());
        }
        if let Some(sect) = sect {
            props.push("sectPr", sect.to_owned());
        }
        props.sort(PPR_ORDER);
        props.write("w:pPr")
    }

    /// The style a paragraph in style `id` is written in: a paragraph style the document
    /// has, a built-in heading, title, or quote it lacks (pasted from Markdown, or set by
    /// Ctrl+N), else the document's default.
    fn written_style(&self, id: &str) -> String {
        if self.styles.paragraph_id(Some(id)) == id || StyleSheet::builtin_style(id).is_some() {
            id.to_owned()
        } else {
            self.styles.paragraph_id(None).to_owned()
        }
    }

    /// The colour paragraph style `id` gives its runs (`None`: automatic).
    fn style_color(&self, id: &str) -> Option<Rgb> {
        self.styles
            .paragraph(Some(id))
            .and_then(|s| s.rpr.color)
            .or(self.styles.default_rpr.color)
            .flatten()
    }

    /// Whether run properties `p`, with their character style, set the colour to
    /// automatic.
    fn automatic(&self, p: &RunProps) -> bool {
        p.color
            .or_else(|| {
                self.styles
                    .character(p.style.as_deref())
                    .and_then(|s| s.rpr.color)
            })
            .is_some_and(|c| c.is_none())
    }

    /// The `w:pStyle` of a paragraph written in style `id` (see [`Patcher::written_style`]):
    /// none for the default style; for a built-in style the document lacks, the document's
    /// style of the same name, else the style added to `styles.xml` in Word's look.
    fn style_id(&mut self, id: &str) -> Option<String> {
        if id == self.styles.paragraph_id(None) {
            return None;
        }
        if self.styles.paragraph_id(Some(id)) == id {
            return Some(id.to_owned());
        }
        let builtin = StyleSheet::builtin_style(id)?;
        let name = match builtin.outline {
            Some(n) => format!("heading {n}"),
            None => builtin.name.clone(),
        };
        if let Some(found) = self.styles.paragraph_named(&name) {
            return Some(found.to_owned());
        }
        let styles_xml = self.styles_xml.as_ref()?;
        let mut new_id = id.to_owned();
        let mut n = 1;
        while self.styles.has(&new_id) {
            new_id = format!("{id}Slate{n}");
            n += 1;
        }
        let (ppr, rpr) = match builtin.outline {
            _ if builtin.title => (
                "<w:spacing w:after=\"120\"/>".to_owned(),
                "<w:sz w:val=\"56\"/>".to_owned(),
            ),
            Some(n) => {
                let n = n.clamp(1, 6);
                let size = [36, 32, 28, 26, 24, 22][usize::from(n) - 1];
                (
                    format!(
                        "<w:keepNext/><w:spacing w:before=\"240\" w:after=\"120\"/><w:outlineLvl w:val=\"{}\"/>",
                        n - 1
                    ),
                    format!("<w:b/><w:sz w:val=\"{size}\"/>"),
                )
            }
            None => (
                "<w:ind w:left=\"720\" w:right=\"720\"/>".to_owned(),
                "<w:i/>".to_owned(),
            ),
        };
        let based = self
            .styles
            .default_paragraph()
            .map(|d| {
                let d = escape(d);
                format!("<w:basedOn w:val=\"{d}\"/><w:next w:val=\"{d}\"/>")
            })
            .unwrap_or_default();
        let def = format!(
            "<w:style w:type=\"paragraph\" w:styleId=\"{new_id}\"{}><w:name w:val=\"{name}\"/>{based}<w:qFormat/><w:pPr>{ppr}</w:pPr><w:rPr>{rpr}</w:rPr></w:style>",
            self.w_decl
        );
        let patched = insert_at_end(styles_xml, &def).ok()?;
        self.styles = Styles::parse(&patched, self.styles.theme().clone()).ok()?;
        self.styles_xml = Some(patched);
        self.styles_changed = true;
        Some(new_id)
    }

    /// The list instance a new list item of this kind and level joins: the list before it,
    /// the template's, the document's first of the kind, or a new one.
    fn num_for(&mut self, bullet: bool, level: u8, template: Option<&Template<'_>>) -> u32 {
        let fits = |n: u32| {
            self.numbering.bulleted(n, level) == Some(bullet)
                && self.numbering.bulleted(n, 0) == Some(bullet)
        };
        let found = self
            .last_list
            .filter(|&(_, b)| b == bullet)
            .map(|(n, _)| n)
            .filter(|&n| fits(n))
            .or_else(|| template.and_then(|t| t.num).filter(|&n| fits(n)))
            .or_else(|| self.numbering.ids().into_iter().find(|&n| fits(n)));
        if let Some(n) = found {
            return n;
        }
        self.add_numbering(bullet)
    }

    /// Add a list definition of nine levels and an instance of it to `numbering.xml`
    /// (made when the package has none); returns the instance.
    fn add_numbering(&mut self, bullet: bool) -> u32 {
        let (abs, num) = self.numbering.free_ids();
        let mut levels = String::new();
        for l in 0..9u32 {
            let (format, text) = if bullet {
                (
                    "bullet",
                    ["\u{2022}", "\u{25E6}", "\u{25AA}"][l as usize % 3].to_owned(),
                )
            } else {
                (
                    ["decimal", "lowerLetter", "lowerRoman"][l as usize % 3],
                    format!("%{}.", l + 1),
                )
            };
            levels.push_str(&format!(
                "<w:lvl w:ilvl=\"{l}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"{format}\"/><w:lvlText w:val=\"{text}\"/><w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"{}\" w:hanging=\"360\"/></w:pPr></w:lvl>",
                720 * (l + 1)
            ));
        }
        let decl = format!(" xmlns:w=\"{W_NS}\"");
        let def = format!(
            "<w:abstractNum{decl} w:abstractNumId=\"{abs}\"><w:multiLevelType w:val=\"hybridMultilevel\"/>{levels}</w:abstractNum>"
        );
        let instance =
            format!("<w:num{decl} w:numId=\"{num}\"><w:abstractNumId w:val=\"{abs}\"/></w:num>");
        let xml = match self.numbering_xml.take() {
            Some(xml) => {
                let first_num = child_start(&xml, b"num")
                    .or_else(|| child_start(&xml, b"numIdMacAtCleanup"))
                    .or_else(|| root_end(&xml))
                    .unwrap_or(xml.len());
                let with_def = format!("{}{def}{}", &xml[..first_num], &xml[first_num..]);
                let at = child_start(&with_def, b"numIdMacAtCleanup")
                    .or_else(|| root_end(&with_def))
                    .unwrap_or(with_def.len());
                format!("{}{instance}{}", &with_def[..at], &with_def[at..])
            }
            None => format!(
                "{XML_DECLARATION}<w:numbering xmlns:w=\"{W_NS}\">{def}{instance}</w:numbering>"
            ),
        };
        if let Ok(parsed) = Numbering::parse(&xml, &self.styles) {
            self.numbering = parsed;
        }
        self.numbering_xml = Some(xml);
        self.numbering_changed = true;
        num
    }

    /// A run of `run`'s text in a paragraph whose style's colour is `style_color`. A
    /// template run whose direct formatting is the run's gives its `w:rPr` as it is; else
    /// the closest one's `w:rPr` keeps what the model doesn't hold (other scripts' fonts,
    /// highlighting, language) and the run's overrides are written exactly: what they set,
    /// nothing the style gives.
    fn run(
        &mut self,
        out: &mut String,
        run: &Run,
        style_color: Option<Rgb>,
        template: Option<&Template<'_>>,
    ) {
        let want = &run.style;
        let none = RunProps::default();
        let candidates = template
            .map(|t| t.runs.as_slice())
            .unwrap_or_default()
            .iter()
            .map(|(x, p)| (*x, p))
            .chain(std::iter::once(("", &none)));
        let (xml, agree) = candidates
            .map(|(x, p)| {
                let mut agree = agreeing(&self.styles.overrides(p), want);
                // The model reads an automatic colour as the style's: in a coloured style
                // it isn't the style's colour a run without its own wants.
                if want.color.is_none() && style_color.is_some() && self.automatic(p) {
                    agree -= 1;
                }
                (x, agree)
            })
            .min_by_key(|(_, agree)| LOOKS - agree)
            .expect("one candidate at least");
        out.push_str("<w:r>");
        if agree == LOOKS && !want.code {
            out.push_str(xml);
        } else {
            let mut rpr = if xml.is_empty() {
                Props::default()
            } else {
                Props::parse(xml)
            };
            let fonts = rpr
                .children
                .iter()
                .find(|(n, _)| n == "rFonts")
                .map(|(_, x)| x.clone());
            rpr.remove(RUN_LOOKS);
            rpr.remove(&["rFonts"]);
            if want.code {
                rpr.push(
                    "rFonts",
                    format!(
                        "<w:rFonts w:ascii=\"{MONOSPACE}\" w:hAnsi=\"{MONOSPACE}\" w:cs=\"{MONOSPACE}\"/>"
                    ),
                );
            } else if let Some(fonts) = latin_font(fonts.as_deref(), want.font.as_deref()) {
                rpr.push("rFonts", fonts);
            }
            let on = |v: bool| if v { "" } else { " w:val=\"0\"" };
            if let Some(b) = want.bold {
                rpr.push("b", format!("<w:b{}/>", on(b)));
            }
            if let Some(i) = want.italic {
                rpr.push("i", format!("<w:i{}/>", on(i)));
            }
            if let Some(s) = want.strike {
                rpr.push("strike", format!("<w:strike{}/>", on(s)));
            }
            if let Some(color) = want.color {
                let [r, g, b] = color;
                rpr.push(
                    "color",
                    format!("<w:color w:val=\"{r:02X}{g:02X}{b:02X}\"/>"),
                );
            }
            if let Some(size) = want.size {
                rpr.push("sz", format!("<w:sz w:val=\"{size}\"/>"));
                rpr.push("szCs", format!("<w:szCs w:val=\"{size}\"/>"));
            }
            if let Some(u) = want.underline {
                let v = if u { "single" } else { "none" };
                rpr.push("u", format!("<w:u w:val=\"{v}\"/>"));
            }
            rpr.sort(RPR_ORDER);
            out.push_str(&rpr.write("w:rPr"));
        }
        push_text(out, &run.text);
        out.push_str("</w:r>");
    }

    /// The start tag of a hyperlink to `url`: a bookmark (`#name`) or the document's
    /// relationship for the URL, added when it has none.
    fn hyperlink(&mut self, url: &str) -> String {
        if let Some(anchor) = url.strip_prefix('#') {
            return format!(
                "<w:hyperlink w:anchor=\"{}\" w:history=\"1\">",
                escape(anchor)
            );
        }
        let found = self
            .rels
            .iter()
            .filter(|(_, r)| r.external && rel_kind(r) == "hyperlink" && r.target == url)
            .map(|(id, _)| id.clone())
            .min();
        let id = match found.or_else(|| self.link_rels.get(url).cloned()) {
            Some(id) => id,
            None => {
                let id = self.free_rid();
                self.new_rels.push_str(&format!(
                    r#"<Relationship Id="{id}" Type="{REL_NS}/hyperlink" Target="{}" TargetMode="External"/>"#,
                    escape(url)
                ));
                self.link_rels.insert(url.to_owned(), id.clone());
                id
            }
        };
        format!(
            "<w:hyperlink {}id=\"{}\"{} w:history=\"1\">",
            self.r,
            escape(&id),
            self.r_decl
        )
    }

    /// A relationship id the main part doesn't use.
    fn free_rid(&mut self) -> String {
        let used = |id: &str, p: &Patcher<'_>| {
            p.rels.contains_key(id) || p.new_rels.contains(&format!("Id=\"{id}\""))
        };
        let mut n = self.rels.len() + 1;
        loop {
            let id = format!("rId{n}");
            if !used(&id, self) {
                return id;
            }
            n += 1;
        }
    }

    /// A run holding `image`: its own `w:drawing` when the document has it, else a new
    /// media part; a linked picture is a link to it.
    fn picture(&mut self, image: &Image) -> String {
        let bytes = match &image.source {
            ImageSource::Link(url) => {
                let mut p = Paragraph::default();
                let style = RunStyle {
                    link: Some(url.clone()),
                    ..RunStyle::default()
                };
                p.push(link_text(image, url), &style);
                // Just the runs: a paragraph's start tag is dropped.
                let xml = self.paragraph(&p, None, None, None, "", None);
                let start = xml.find('>').map_or(0, |i| i + 1);
                return xml[start..].to_owned();
            }
            ImageSource::Embedded { bytes, .. } => bytes,
        };
        let found = self
            .pictures
            .iter()
            .find(|(s, _)| match s {
                ImageSource::Embedded { bytes: b, .. } => Arc::ptr_eq(b, bytes) || b == bytes,
                ImageSource::Link(_) => false,
            })
            .map(|(_, span)| span.clone());
        if let Some(span) = found {
            let mut drawing = self.xml[span].to_owned();
            // Each drawing has its own id.
            if let Some(at) = drawing.find("docPr") {
                if let Some(tag_end) = drawing[at..].find('>') {
                    if let Some(id) = doc_pr_id(&drawing[at..at + tag_end]) {
                        let range = at + id.start..at + id.end;
                        drawing.replace_range(range, &self.next_doc_pr.to_string());
                        self.next_doc_pr += 1;
                    }
                }
            }
            return format!("<w:r>{drawing}</w:r>");
        }
        let (ext, kind) = media_type(bytes);
        let existing = self
            .media
            .iter()
            .find(|(_, b)| Arc::ptr_eq(b, bytes) || b == bytes)
            .map(|(name, _)| name.clone());
        let name = match existing {
            Some(name) => name,
            None => {
                let media_dir = if self.dir.is_empty() {
                    "media".to_owned()
                } else {
                    format!("{}/media", self.dir)
                };
                let mut n = 1;
                let name = loop {
                    let name = format!("{media_dir}/slate-image{n}.{ext}");
                    if !self.names.contains(&name) {
                        break name;
                    }
                    n += 1;
                };
                self.names.insert(name.clone());
                self.media.push((name.clone(), bytes.clone()));
                if !self.types.iter().any(|(e, _, d)| *d && e == ext) {
                    self.types.push((ext.to_owned(), kind.to_owned(), true));
                }
                let id = self.free_rid();
                self.new_rels.push_str(&format!(
                    r#"<Relationship Id="{id}" Type="{REL_NS}/image" Target="{}"/>"#,
                    escape(&relative(self.dir, &name))
                ));
                self.link_rels.insert(format!("media:{name}"), id);
                name
            }
        };
        let rid = self.link_rels[&format!("media:{name}")].clone();
        let (w, h) = image
            .size
            .map(|(w, h)| (w as f64, h as f64))
            .or_else(|| pixel_size(bytes))
            .unwrap_or((96.0, 96.0));
        let (cx, cy) = (
            (w * EMU_PER_PX).round() as u64,
            (h * EMU_PER_PX).round() as u64,
        );
        let id = self.next_doc_pr;
        self.next_doc_pr += 1;
        let alt = escape(&image.alt);
        format!(
            "<w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\" \
             xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\">\
             <wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:docPr id=\"{id}\" name=\"Picture {id}\" descr=\"{alt}\"/>\
             <a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\">\
             <a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
             <pic:pic xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
             <pic:nvPicPr><pic:cNvPr id=\"{id}\" name=\"Picture {id}\" descr=\"{alt}\"/><pic:cNvPicPr/></pic:nvPicPr>\
             <pic:blipFill><a:blip {r}embed=\"{rid}\"{r_decl}/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
             <pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm>\
             <a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic>\
             </a:graphicData></a:graphic></wp:inline></w:drawing></w:r>",
            r = self.r,
            r_decl = self.r_decl,
        )
    }

    /// A table: the one it replaces with its cells patched, when its rows and cells are
    /// as they were; else written whole.
    fn table(
        &mut self,
        table: &Table,
        template: Option<(&'a Element, &'a TableSpan, &'a Table)>,
    ) -> String {
        if let Some((e, ts, old)) = template.filter(|(_, _, old)| same_shape(old, table)) {
            let mut out = String::new();
            let mut at = e.span.start;
            for (r, row) in table.rows.iter().enumerate() {
                for (c, cell) in row.cells.iter().enumerate() {
                    let (cs, was) = (&ts.rows[r][c], &old.rows[r].cells[c]);
                    if same_blocks(&was.blocks, &cell.blocks) {
                        continue;
                    }
                    out.push_str(&self.xml[at..cs.span.start]);
                    if cell.blocks.is_empty() {
                        let mut xml =
                            self.container(&cs.content, cs.span.clone(), &was.blocks, &[]);
                        let end = xml.rfind('<').unwrap_or(xml.len());
                        xml.insert_str(end, "<w:p/>");
                        out.push_str(&xml);
                    } else {
                        let xml =
                            self.container(&cs.content, cs.span.clone(), &was.blocks, &cell.blocks);
                        out.push_str(&xml);
                    }
                    at = cs.span.end;
                }
            }
            out.push_str(&self.xml[at..e.span.end]);
            return out;
        }
        let columns = table
            .rows
            .iter()
            .map(|r| r.cells.iter().map(|c| c.span.max(1)).sum::<u32>())
            .max()
            .unwrap_or(1)
            .max(1);
        let width = TABLE_WIDTH / columns;
        let border = |side: &str| {
            format!("<w:{side} w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>")
        };
        let borders: String = ["top", "left", "bottom", "right", "insideH", "insideV"]
            .iter()
            .map(|s| border(s))
            .collect();
        let mut out = format!(
            "<w:tbl{}><w:tblPr><w:tblW w:w=\"0\" w:type=\"auto\"/><w:tblBorders>{borders}</w:tblBorders><w:tblLook w:val=\"04A0\"/></w:tblPr><w:tblGrid>",
            self.w_decl
        );
        for _ in 0..columns {
            out.push_str(&format!("<w:gridCol w:w=\"{width}\"/>"));
        }
        out.push_str("</w:tblGrid>");
        for row in &table.rows {
            out.push_str("<w:tr>");
            if row.header {
                out.push_str("<w:trPr><w:tblHeader/></w:trPr>");
            }
            for cell in &row.cells {
                let span = cell.span.max(1);
                out.push_str(&format!(
                    "<w:tc><w:tcPr><w:tcW w:w=\"{}\" w:type=\"dxa\"/>",
                    width * span
                ));
                if span > 1 {
                    out.push_str(&format!("<w:gridSpan w:val=\"{span}\"/>"));
                }
                out.push_str("</w:tcPr>");
                let content = self.fresh(&cell.blocks, &mut Gap::default());
                out.push_str(&content);
                let ends_in_paragraph = cell
                    .blocks
                    .last()
                    .is_some_and(|b| !matches!(b, Block::Table(_) | Block::PageBreak));
                if !ends_in_paragraph {
                    out.push_str("<w:p/>");
                }
                out.push_str("</w:tc>");
            }
            out.push_str("</w:tr>");
        }
        out.push_str("</w:tbl>");
        out
    }
}

/// Whether two blocks are the same but for list numbers, which Word sets.
fn same_block(a: &Block, b: &Block) -> bool {
    match (a, b) {
        (Block::ListItem(x), Block::ListItem(y)) => {
            x.level == y.level
                && x.paragraph == y.paragraph
                && match (&x.kind, &y.kind) {
                    (ListKind::Ordered { .. }, ListKind::Ordered { .. }) => true,
                    (k, l) => k == l,
                }
        }
        (Block::Table(x), Block::Table(y)) => {
            same_shape(x, y)
                && x.rows
                    .iter()
                    .zip(&y.rows)
                    .flat_map(|(r, s)| r.cells.iter().zip(&s.cells))
                    .all(|(c, d)| same_blocks(&c.blocks, &d.blocks))
        }
        _ => a == b,
    }
}

fn same_blocks(a: &[Block], b: &[Block]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same_block(x, y))
}

/// Whether block `back`, read back from a save, is block `saved` as written: the same
/// kind with the same text; a table of the same shape whose cells' blocks correspond one
/// for one.
fn corresponds(back: &Block, saved: &Block) -> bool {
    match (back, saved) {
        (Block::Image(x), Block::Image(y)) => x.source == y.source,
        (Block::Table(x), Block::Table(y)) => {
            same_shape(x, y)
                && x.rows
                    .iter()
                    .zip(&y.rows)
                    .flat_map(|(r, s)| r.cells.iter().zip(&s.cells))
                    .all(|(c, d)| {
                        c.blocks.len() == d.blocks.len()
                            && c.blocks
                                .iter()
                                .zip(&d.blocks)
                                .all(|(a, b)| corresponds(a, b))
                    })
        }
        _ => {
            std::mem::discriminant(back) == std::mem::discriminant(saved)
                && lines(std::slice::from_ref(back)) == lines(std::slice::from_ref(saved))
        }
    }
}

/// Whether two tables have the same rows, cells, spans, and header rows.
fn same_shape(a: &Table, b: &Table) -> bool {
    a.rows.len() == b.rows.len()
        && a.rows.iter().zip(&b.rows).all(|(r, s)| {
            r.header == s.header
                && r.cells.len() == s.cells.len()
                && r.cells.iter().zip(&s.cells).all(|(c, d)| c.span == d.span)
        })
}

/// A run's text as WordprocessingML: text, tabs, line breaks, and special hyphens.
fn push_text(out: &mut String, text: &str) {
    let mut plain = String::new();
    let flush = |out: &mut String, plain: &mut String| {
        if !plain.is_empty() {
            out.push_str("<w:t xml:space=\"preserve\">");
            out.push_str(&escape(plain));
            out.push_str("</w:t>");
            plain.clear();
        }
    };
    for c in text.chars() {
        let special = match c {
            '\t' => "<w:tab/>",
            '\n' => "<w:br/>",
            '\u{2011}' => "<w:noBreakHyphen/>",
            '\u{AD}' => "<w:softHyphen/>",
            '\r' => continue,
            c => {
                plain.push(c);
                continue;
            }
        };
        flush(out, &mut plain);
        out.push_str(special);
    }
    flush(out, &mut plain);
}

/// A new media part's extension and content type, from its first bytes.
fn media_type(bytes: &[u8]) -> (&'static str, &'static str) {
    match ImageFormat::sniff(bytes) {
        ImageFormat::Png => ("png", "image/png"),
        ImageFormat::Jpeg => ("jpeg", "image/jpeg"),
        ImageFormat::Gif => ("gif", "image/gif"),
        ImageFormat::Other => {
            if bytes.len() > 44 && &bytes[40..44] == b" EMF" {
                ("emf", "image/x-emf")
            } else if bytes.starts_with(&[0xD7, 0xCD, 0xC6, 0x9A]) {
                ("wmf", "image/x-wmf")
            } else if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
                ("tiff", "image/tiff")
            } else if bytes.starts_with(b"BM") {
                ("bmp", "image/bmp")
            } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
                ("webp", "image/webp")
            } else if bytes.starts_with(b"<svg") || bytes.starts_with(b"<?xml") {
                ("svg", "image/svg+xml")
            } else {
                ("bin", "application/octet-stream")
            }
        }
    }
}

/// A PNG's, GIF's, or JPEG's size in pixels, from its header.
fn pixel_size(b: &[u8]) -> Option<(f64, f64)> {
    let be16 = |i: usize| Some(u16::from_be_bytes([*b.get(i)?, *b.get(i + 1)?]) as f64);
    match ImageFormat::sniff(b) {
        ImageFormat::Png => {
            let be32 =
                |i: usize| Some(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?) as f64);
            Some((be32(16)?, be32(20)?))
        }
        ImageFormat::Gif => {
            let le16 = |i: usize| Some(u16::from_le_bytes([*b.get(i)?, *b.get(i + 1)?]) as f64);
            Some((le16(6)?, le16(8)?))
        }
        ImageFormat::Jpeg => {
            let mut i = 2;
            while i + 9 < b.len() {
                if b[i] != 0xFF {
                    return None;
                }
                let marker = b[i + 1];
                let len = be16(i + 2)? as usize;
                // Start of frame markers, not DHT (C4), JPG (C8), or DAC (CC).
                if (0xC0..=0xCF).contains(&marker) && ![0xC4, 0xC8, 0xCC].contains(&marker) {
                    return Some((be16(i + 7)?, be16(i + 5)?));
                }
                i += 2 + len;
            }
            None
        }
        ImageFormat::Other => None,
    }
}
