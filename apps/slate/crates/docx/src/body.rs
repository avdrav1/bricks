//! `word/document.xml`'s body: paragraphs, runs, hyperlinks, fields, pictures, and tables
//! into blocks.
//!
//! Containers that only wrap content (content controls, smart tags, custom XML, tracked
//! insertions) are read through; deletions, section properties, and anything else Slate
//! doesn't show are skipped. Of an `mc:AlternateContent`, the `mc:Choice` is read and the
//! `mc:Fallback` (the same content for older readers) skipped. A field shows its result,
//! not its code; a HYPERLINK field links its result.
//!
//! A page break (`w:br w:type="page"`) splits its paragraph as LibreOffice does: the text
//! before it, the break, the text after; a part left empty by a break at either end isn't
//! a paragraph. Pictures are blocks: a paragraph's pictures come before its text, and a
//! paragraph holding only pictures stays as an empty paragraph after them.
//!
//! For saving, the reader also maps the body and every table cell to the elements their
//! blocks came from ([`Container`]): each `w:p` and `w:tbl` child's bytes in the part, the
//! blocks it made, a table's cells, and its pictures' `w:drawing` or `w:pict`. What sits
//! between them (section properties, bookmarks, content controls' tags) isn't an element.

use crate::numbering::{format_number, Counters, Numbering};
use crate::styles::{ParaProps, RunProps, Section, Styles};
use crate::xml::{self, XmlReader};
use crate::{DocxError, Package, Rel};
use document_model::{
    Block, Image, ImageFormat, ImageSource, ListItem, ListNumbering, Paragraph, RunStyle, Table,
    TableCell, TableRow,
};
use quick_xml::events::{BytesStart, Event};
use std::collections::HashMap;
use std::io::{Read, Seek};
use std::ops::Range;

/// The elements of the body or of a table cell, in order: where each block came from in
/// `word/document.xml`.
#[derive(Debug, Clone, Default)]
pub(crate) struct Container {
    pub elements: Vec<Element>,
    /// Where content added after the last element goes: before the container's end tag,
    /// or before the body's own section properties.
    pub tail: usize,
    /// The body's own section properties (`w:body/w:sectPr`).
    pub sect: Option<Range<usize>>,
}

/// A `w:p` or `w:tbl` child of the body or of a cell.
#[derive(Debug, Clone)]
pub(crate) struct Element {
    /// Its bytes in the part.
    pub span: Range<usize>,
    /// The blocks it made, as indices into its container's blocks.
    pub blocks: Range<usize>,
    /// A table's cells.
    pub table: Option<TableSpan>,
    /// For each picture block it made, in order: its `w:drawing` or `w:pict` element (`None`
    /// when one element held several pictures).
    pub pictures: Vec<Option<Range<usize>>>,
    /// A paragraph's list numbering instance (`w:numId`, 0 or `None`: not a list).
    pub num: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TableSpan {
    /// Each row's cells.
    pub rows: Vec<Vec<CellSpan>>,
}

#[derive(Debug, Clone)]
pub(crate) struct CellSpan {
    /// The `w:tc` element.
    pub span: Range<usize>,
    pub content: Container,
}

/// What [`read`] read of the body.
pub(crate) struct Parsed {
    pub blocks: Vec<Block>,
    /// Where the blocks came from (`None` without a `w:body`).
    pub body: Option<Container>,
    /// The text width of the last section (its last `w:sectPr`), in twips; `None` without
    /// one.
    pub text_width: Option<u32>,
}

/// Read the body of `word/document.xml` (`xml`).
pub(crate) fn read<R: Read + Seek>(
    xml: &str,
    styles: &Styles,
    numbering: &Numbering,
    rels: &HashMap<String, Rel>,
    package: &mut Package<R>,
) -> Result<Parsed, DocxError> {
    let mut body = Body {
        r: xml::reader(xml),
        styles,
        numbering,
        rels,
        package,
        counters: Counters::default(),
        fields: Vec::new(),
        depth: 0,
        footnotes: 0,
        endnotes: 0,
        pictures: Vec::new(),
        num: None,
        text_width: None,
    };
    let mut out = Vec::new();
    loop {
        match body.r.read_event()? {
            Event::Start(e) if xml::local(&e) == b"body" => {
                let container = body.blocks(b"body", &mut out, None)?;
                return Ok(Parsed {
                    blocks: out,
                    body: Some(container),
                    text_width: body.text_width,
                });
            }
            Event::Eof => {
                return Ok(Parsed {
                    blocks: out,
                    body: None,
                    text_width: None,
                })
            }
            _ => {}
        }
    }
}

/// A complex field (`w:fldChar` begin … separate … end) being read.
struct Field {
    /// Its code so far (`w:instrText`).
    code: String,
    /// Past `separate`: the text now is the field's result.
    result: bool,
    link: Option<String>,
}

/// What a paragraph holds, in order, before it becomes blocks.
enum Item {
    Text(String, RunStyle),
    PageBreak,
    /// A picture, with its `w:drawing` or `w:pict` element when it was that element's only
    /// picture.
    Image(Image, Option<Range<usize>>),
}

/// A table cell's own properties.
#[derive(Default)]
struct CellProps {
    span: u32,
}

struct Body<'a, 'x, R> {
    r: XmlReader<'x>,
    styles: &'a Styles,
    numbering: &'a Numbering,
    rels: &'a HashMap<String, Rel>,
    package: &'a mut Package<R>,
    counters: Counters,
    fields: Vec<Field>,
    /// Tables open around the current position.
    depth: usize,
    /// Footnote and endnote references so far: their marks number on from these.
    footnotes: u64,
    endnotes: u64,
    /// The pictures of the paragraph just read, as [`Element::pictures`].
    pictures: Vec<Option<Range<usize>>>,
    /// The list numbering instance of the paragraph just read.
    num: Option<u32>,
    /// The text width of the last section properties read (`w:sectPr`), in twips.
    text_width: Option<u32>,
}

/// The deepest nesting of tables kept as tables; the content of tables nested deeper
/// joins the cell around them (so a hostile file can't exhaust the stack).
const MAX_TABLE_DEPTH: usize = 32;

/// Elements that only wrap content: their children are read as if they weren't there.
fn wrapper(name: &[u8]) -> bool {
    matches!(
        name,
        b"sdt"
            | b"sdtContent"
            | b"customXml"
            | b"smartTag"
            | b"ins"
            | b"moveTo"
            | b"dir"
            | b"bdo"
            | b"AlternateContent"
            | b"Choice"
    )
}

impl<R: Read + Seek> Body<'_, '_, R> {
    /// Where the reader is in the part.
    fn pos(&self) -> usize {
        self.r.buffer_position() as usize
    }

    /// Block content up to the end of element `end` (`body`, `tc`), and where each block
    /// came from. In a cell, `cell` takes the cell's properties.
    fn blocks(
        &mut self,
        end: &[u8],
        out: &mut Vec<Block>,
        mut cell: Option<&mut CellProps>,
    ) -> Result<Container, DocxError> {
        let base = out.len();
        let mut container = Container::default();
        // Cells of tables too deep to keep, open inside this element.
        let mut flattened = 0usize;
        loop {
            let before = self.pos();
            let first = out.len();
            match self.r.read_event()? {
                Event::Start(e) => match xml::local(&e) {
                    b"p" => {
                        self.paragraph(out)?;
                        container.elements.push(Element {
                            span: before..self.pos(),
                            blocks: first - base..out.len() - base,
                            table: None,
                            pictures: std::mem::take(&mut self.pictures),
                            num: self.num,
                        });
                    }
                    b"tbl" if self.depth < MAX_TABLE_DEPTH => {
                        self.depth += 1;
                        let table = self.table();
                        self.depth -= 1;
                        let (table, span) = table?;
                        out.push(Block::Table(table));
                        container.elements.push(Element {
                            span: before..self.pos(),
                            blocks: first - base..out.len() - base,
                            table: Some(span),
                            pictures: Vec::new(),
                            num: None,
                        });
                    }
                    b"tbl" | b"tr" => {}
                    b"tc" => flattened += 1,
                    b"tcPr" if cell.is_some() => {
                        if let Some(c) = cell.as_deref_mut() {
                            self.cell_props(c)?;
                        }
                    }
                    b"sectPr" => {
                        self.text_width = Some(Section::read(&mut self.r)?.text_width);
                        container.sect = Some(before..self.pos());
                    }
                    name if wrapper(name) => {}
                    _ => xml::skip(&mut self.r, &e)?,
                },
                Event::Empty(e) if xml::local(&e) == b"p" => {
                    let p = self.paragraph_of(&ParaProps::default());
                    out.push(Block::Paragraph(p));
                    container.elements.push(Element {
                        span: before..self.pos(),
                        blocks: first - base..out.len() - base,
                        table: None,
                        pictures: Vec::new(),
                        num: None,
                    });
                }
                Event::End(e) if e.local_name().as_ref() == b"tc" && flattened > 0 => {
                    flattened -= 1;
                }
                Event::End(e) if e.local_name().as_ref() == end => {
                    container.tail = container.sect.as_ref().map_or(before, |s| s.start);
                    return Ok(container);
                }
                Event::Eof => return Err(DocxError::Xml(format!("unclosed w:{}", show(end)))),
                _ => {}
            }
        }
    }

    fn cell_props(&mut self, c: &mut CellProps) -> Result<(), DocxError> {
        loop {
            match self.r.read_event()? {
                Event::Empty(e) if xml::local(&e) == b"gridSpan" => {
                    c.span = xml::num(&e, b"val").unwrap_or(1).max(1);
                }
                Event::Start(e) => xml::skip(&mut self.r, &e)?,
                Event::End(e) if e.local_name().as_ref() == b"tcPr" => return Ok(()),
                Event::Eof => return Err(DocxError::Xml("unclosed w:tcPr".into())),
                _ => {}
            }
        }
    }

    fn table(&mut self) -> Result<(Table, TableSpan), DocxError> {
        let mut table = Table::default();
        let mut span = TableSpan::default();
        loop {
            match self.r.read_event()? {
                Event::Start(e) => match xml::local(&e) {
                    b"tr" => {
                        let (row, cells) = self.row()?;
                        table.rows.push(row);
                        span.rows.push(cells);
                    }
                    name if wrapper(name) => {}
                    _ => xml::skip(&mut self.r, &e)?,
                },
                Event::End(e) if e.local_name().as_ref() == b"tbl" => return Ok((table, span)),
                Event::Eof => return Err(DocxError::Xml("unclosed w:tbl".into())),
                _ => {}
            }
        }
    }

    fn row(&mut self) -> Result<(TableRow, Vec<CellSpan>), DocxError> {
        let mut row = TableRow::default();
        let mut cells = Vec::new();
        loop {
            let before = self.pos();
            match self.r.read_event()? {
                Event::Start(e) => match xml::local(&e) {
                    b"trPr" => row.header = self.header_row()?,
                    b"tc" => {
                        let mut props = CellProps { span: 1 };
                        let mut blocks = Vec::new();
                        let content = self.blocks(b"tc", &mut blocks, Some(&mut props))?;
                        row.cells.push(TableCell {
                            blocks,
                            span: props.span,
                        });
                        cells.push(CellSpan {
                            span: before..self.pos(),
                            content,
                        });
                    }
                    name if wrapper(name) => {}
                    _ => xml::skip(&mut self.r, &e)?,
                },
                Event::End(e) if e.local_name().as_ref() == b"tr" => return Ok((row, cells)),
                Event::Eof => return Err(DocxError::Xml("unclosed w:tr".into())),
                _ => {}
            }
        }
    }

    /// `w:trPr`: whether the row repeats as a header.
    fn header_row(&mut self) -> Result<bool, DocxError> {
        let mut header = false;
        loop {
            match self.r.read_event()? {
                Event::Empty(e) if xml::local(&e) == b"tblHeader" => header = xml::on(&e),
                Event::Start(e) => xml::skip(&mut self.r, &e)?,
                Event::End(e) if e.local_name().as_ref() == b"trPr" => return Ok(header),
                Event::Eof => return Err(DocxError::Xml("unclosed w:trPr".into())),
                _ => {}
            }
        }
    }

    /// An empty paragraph in the style `direct` names, with the alignment it and the
    /// styles give it, and its own tab stops, spacing, and indents.
    fn paragraph_of(&self, direct: &ParaProps) -> Paragraph {
        let ppr = self.styles.paragraph_props(direct);
        let mut p = Paragraph::new(self.styles.paragraph_id(direct.style.as_deref()));
        p.align = ppr.align.unwrap_or_default();
        p.tabs.clone_from(&direct.tabs);
        p.own = direct.overrides();
        p
    }

    fn paragraph(&mut self, out: &mut Vec<Block>) -> Result<(), DocxError> {
        let mut direct = ParaProps::default();
        let mut items = Vec::new();
        let mut links: Vec<Option<String>> = Vec::new();
        loop {
            match self.r.read_event()? {
                Event::Start(e) => match xml::local(&e) {
                    b"pPr" => direct = ParaProps::read(&mut self.r)?,
                    b"r" => {
                        let link = links.iter().rev().find_map(|l| l.clone());
                        self.run(link, &mut items)?;
                    }
                    b"hyperlink" => links.push(self.hyperlink(&e)),
                    b"fldSimple" => {
                        links.push(xml::attr(&e, b"instr").and_then(|code| hyperlink_field(&code)))
                    }
                    b"Fallback" => xml::skip(&mut self.r, &e)?,
                    name if wrapper(name) => {}
                    _ => xml::skip(&mut self.r, &e)?,
                },
                Event::End(e) => match e.local_name().as_ref() {
                    b"hyperlink" | b"fldSimple" => {
                        links.pop();
                    }
                    b"p" => break,
                    _ => {}
                },
                Event::Eof => return Err(DocxError::Xml("unclosed w:p".into())),
                _ => {}
            }
        }
        self.emit_paragraph(&direct, items, out);
        Ok(())
    }

    /// Turn a paragraph's items into blocks (see the module comment).
    fn emit_paragraph(&mut self, direct: &ParaProps, items: Vec<Item>, out: &mut Vec<Block>) {
        let template = self.paragraph_of(direct);
        let numbered = self.numbering.list_of(self.styles, direct);
        self.num = numbered.map(|(num, _)| num);
        let mut list = numbered.and_then(|(num, level)| {
            self.numbering
                .next(&mut self.counters, num, level)
                .map(|kind| (kind, level))
        });
        let ppr = self.styles.paragraph_props(direct);
        if ppr.page_break_before == Some(true) {
            out.push(Block::PageBreak);
        }
        let mut pieces: Vec<Vec<Item>> = vec![Vec::new()];
        for item in items {
            match item {
                Item::PageBreak => pieces.push(Vec::new()),
                other => pieces.last_mut().expect("one piece at least").push(other),
            }
        }
        // A break at the start of a paragraph with text after it only moves the paragraph
        // to the next page; the part before a break stays even when empty; parts after a
        // break that hold nothing aren't paragraphs.
        let skip_first = pieces.len() > 1
            && pieces[0].iter().all(|i| !matches!(i, Item::Text(..)))
            && pieces[1..]
                .iter()
                .any(|p| p.iter().any(|i| matches!(i, Item::Text(..))));
        // An empty paragraph that ends a section is the section's mark, not a paragraph.
        let section_mark = ppr.section.is_some()
            && pieces
                .iter()
                .flatten()
                .all(|i| matches!(i, Item::PageBreak));
        for (i, piece) in pieces.into_iter().enumerate() {
            if i > 0 {
                out.push(Block::PageBreak);
            }
            let mut p = template.clone();
            let mut has_text = false;
            for item in piece {
                match item {
                    Item::Text(text, style) => {
                        has_text = true;
                        p.push(&text, &style);
                    }
                    Item::Image(image, span) => {
                        out.push(Block::Image(image));
                        self.pictures.push(span);
                    }
                    Item::PageBreak => {}
                }
            }
            let keep = if i == 0 {
                !skip_first && !section_mark
            } else {
                has_text
            };
            if !keep {
                continue;
            }
            match list.take() {
                Some((kind, level)) => out.push(Block::ListItem(ListItem {
                    kind,
                    level,
                    paragraph: p,
                    numbering: self.num.map(|num| ListNumbering {
                        num,
                        mark: self.styles.overrides(&direct.mark),
                    }),
                })),
                None => out.push(Block::Paragraph(p)),
            }
        }
        if let Some(section) = ppr.section {
            if section.starts_page {
                out.push(Block::PageBreak);
            }
            self.text_width = Some(section.text_width);
        }
    }

    /// The target of a `w:hyperlink`: its relationship's URL, or `#bookmark`.
    fn hyperlink(&self, e: &BytesStart<'_>) -> Option<String> {
        if let Some(id) = xml::attr(e, b"id") {
            if let Some(rel) = self.rels.get(id.as_ref()) {
                return Some(rel.target.clone());
            }
        }
        xml::attr(e, b"anchor").map(|a| format!("#{a}"))
    }

    /// Whether text read now shows: outside fields, or in the result of every field
    /// around it.
    fn shows(&self) -> bool {
        self.fields.iter().all(|f| f.result)
    }

    fn run(&mut self, link: Option<String>, items: &mut Vec<Item>) -> Result<(), DocxError> {
        let mut direct = RunProps::default();
        let mut style: Option<RunStyle> = None;
        loop {
            let before = self.pos();
            let event = self.r.read_event()?;
            let empty = matches!(event, Event::Empty(_));
            match event {
                Event::Start(e) | Event::Empty(e) => {
                    let name = xml::local(&e);
                    if name == b"rPr" {
                        if !empty {
                            direct = RunProps::read(&mut self.r)?;
                        }
                        style = None;
                        continue;
                    }
                    let look = style
                        .get_or_insert_with(|| self.run_style(&direct, link.clone()))
                        .clone();
                    match name {
                        b"t" if !empty => {
                            let text = self.text(b"t")?;
                            if self.shows() {
                                items.push(Item::Text(text, look));
                            }
                        }
                        b"instrText" if !empty => {
                            let text = self.text(b"instrText")?;
                            if let Some(f) = self.fields.last_mut().filter(|f| !f.result) {
                                f.code.push_str(&text);
                            }
                        }
                        b"tab" | b"ptab" => self.push_text(items, "\t", look),
                        b"cr" => self.push_text(items, "\n", look),
                        b"br" => match xml::attr(&e, b"type").as_deref() {
                            Some("page") => {
                                if self.shows() {
                                    items.push(Item::PageBreak);
                                }
                            }
                            _ => self.push_text(items, "\n", look),
                        },
                        b"noBreakHyphen" => self.push_text(items, "\u{2011}", look),
                        b"softHyphen" => self.push_text(items, "\u{AD}", look),
                        b"sym" => {
                            let c = xml::attr(&e, b"char")
                                .and_then(|h| u32::from_str_radix(&h, 16).ok())
                                .and_then(char::from_u32);
                            if let Some(c) = c {
                                self.push_text(items, &c.to_string(), look);
                            }
                        }
                        b"fldChar" => self.field_char(&e),
                        // The note's mark, as Word numbers them by default (1, 2, 3 and
                        // i, ii, iii), unless a custom mark follows as text.
                        b"footnoteReference" if !custom_mark(&e) => {
                            self.footnotes += 1;
                            let mark = self.footnotes.to_string();
                            self.push_text(items, &mark, look);
                        }
                        b"endnoteReference" if !custom_mark(&e) => {
                            self.endnotes += 1;
                            let mark = format_number(self.endnotes, "lowerRoman");
                            self.push_text(items, &mark, look);
                        }
                        b"drawing" if !empty => self.drawing(items, before)?,
                        b"pict" if !empty => self.pict(items, before)?,
                        b"Fallback" if !empty => xml::skip(&mut self.r, &e)?,
                        name if wrapper(name) => {}
                        _ if !empty => xml::skip(&mut self.r, &e)?,
                        _ => {}
                    }
                }
                Event::End(e) if e.local_name().as_ref() == b"r" => return Ok(()),
                Event::Eof => return Err(DocxError::Xml("unclosed w:r".into())),
                _ => {}
            }
        }
    }

    fn push_text(&self, items: &mut Vec<Item>, text: &str, look: RunStyle) {
        if self.shows() {
            items.push(Item::Text(text.to_owned(), look));
        }
    }

    /// A run's direct formatting over its paragraph's style: its character style's, then
    /// its own; and its link.
    fn run_style(&self, direct: &RunProps, link: Option<String>) -> RunStyle {
        let field_link = self.fields.iter().rev().find_map(|f| f.link.clone());
        RunStyle {
            link: link.or(field_link),
            ..self.styles.overrides(direct)
        }
    }

    fn field_char(&mut self, e: &BytesStart<'_>) {
        match xml::attr(e, b"fldCharType").as_deref() {
            Some("begin") => self.fields.push(Field {
                code: String::new(),
                result: false,
                link: None,
            }),
            Some("separate") => {
                if let Some(f) = self.fields.last_mut() {
                    f.result = true;
                    f.link = hyperlink_field(&f.code);
                }
            }
            Some("end") => {
                self.fields.pop();
            }
            _ => {}
        }
    }

    /// The text content of the element `end` just started.
    fn text(&mut self, end: &[u8]) -> Result<String, DocxError> {
        let mut out = String::new();
        loop {
            match self.r.read_event()? {
                Event::Text(t) => {
                    out.push_str(&t.decode().map_err(|e| DocxError::Xml(e.to_string()))?)
                }
                Event::CData(t) => out.push_str(&String::from_utf8_lossy(&t)),
                Event::GeneralRef(r) => {
                    if let Some(s) = xml::reference(&r) {
                        out.push_str(&s);
                    }
                }
                Event::End(e) if e.local_name().as_ref() == end => return Ok(out),
                Event::Eof => return Err(DocxError::Xml(format!("unclosed w:{}", show(end)))),
                _ => {}
            }
        }
    }

    /// The picture a relationship id names, if the package has it.
    fn picture(&mut self, id: &str, alt: &str, size: Option<(f32, f32)>) -> Option<Image> {
        let rel = self.rels.get(id).filter(|r| !r.external)?;
        let bytes = self.package.image(&rel.target)?;
        Some(Image {
            alt: alt.to_owned(),
            source: ImageSource::Embedded {
                format: ImageFormat::sniff(&bytes),
                bytes,
            },
            size,
        })
    }

    /// A DrawingML picture (`w:drawing`, starting at `start`): every `a:blip` in it, sized
    /// by `wp:extent` and described by `wp:docPr`. Text boxes in drawings aren't read.
    fn drawing(&mut self, items: &mut Vec<Item>, start: usize) -> Result<(), DocxError> {
        let mut size = None;
        let mut alt = String::new();
        let mut blips = Vec::new();
        loop {
            let event = self.r.read_event()?;
            let start = matches!(event, Event::Start(_));
            match event {
                Event::Start(e) | Event::Empty(e) => match xml::local(&e) {
                    // Text boxes aren't shown; nothing in them is read.
                    b"txbxContent" if start => xml::skip(&mut self.r, &e)?,
                    b"extent" if size.is_none() => {
                        let cx: Option<f64> = xml::num(&e, b"cx");
                        let cy: Option<f64> = xml::num(&e, b"cy");
                        if let (Some(cx), Some(cy)) = (cx, cy) {
                            size = Some(((cx / EMU_PER_PX) as f32, (cy / EMU_PER_PX) as f32));
                        }
                    }
                    b"docPr" => {
                        alt = xml::attr(&e, b"descr")
                            .filter(|d| !d.is_empty())
                            .or_else(|| xml::attr(&e, b"title"))
                            .unwrap_or_default()
                            .into_owned();
                    }
                    b"blip" => {
                        if let Some(id) = xml::attr(&e, b"embed") {
                            blips.push(id.into_owned());
                        }
                    }
                    _ => {}
                },
                Event::End(e) if e.local_name().as_ref() == b"drawing" => break,
                Event::Eof => return Err(DocxError::Xml("unclosed w:drawing".into())),
                _ => {}
            }
        }
        let one = blips.len() == 1;
        let span = one.then(|| start..self.pos());
        for id in blips {
            if let Some(image) = self.picture(&id, &alt, if one { size } else { None }) {
                if self.shows() {
                    items.push(Item::Image(image, span.clone()));
                }
            }
        }
        Ok(())
    }

    /// A VML picture (`w:pict`, starting at `start`): `v:imagedata` sized by its shape's
    /// style. Text boxes aren't read.
    fn pict(&mut self, items: &mut Vec<Item>, start: usize) -> Result<(), DocxError> {
        let mut size = None;
        let mut alt = String::new();
        let mut ids = Vec::new();
        loop {
            let event = self.r.read_event()?;
            let start = matches!(event, Event::Start(_));
            match event {
                Event::Start(e) | Event::Empty(e) => match xml::local(&e) {
                    b"txbxContent" if start => xml::skip(&mut self.r, &e)?,
                    b"shape" | b"rect" => {
                        if let Some(style) = xml::attr(&e, b"style") {
                            size = vml_size(&style).or(size);
                        }
                        if let Some(a) = xml::attr(&e, b"alt") {
                            alt = a.into_owned();
                        }
                    }
                    b"imagedata" => {
                        if let Some(id) = xml::attr(&e, b"id") {
                            ids.push(id.into_owned());
                        }
                        if alt.is_empty() {
                            if let Some(t) = xml::attr(&e, b"title") {
                                alt = t.into_owned();
                            }
                        }
                    }
                    _ => {}
                },
                Event::End(e) if e.local_name().as_ref() == b"pict" => break,
                Event::Eof => return Err(DocxError::Xml("unclosed w:pict".into())),
                _ => {}
            }
        }
        let span = (ids.len() == 1).then(|| start..self.pos());
        for id in ids {
            if let Some(image) = self.picture(&id, &alt, size) {
                if self.shows() {
                    items.push(Item::Image(image, span.clone()));
                }
            }
        }
        Ok(())
    }
}

/// English Metric Units per pixel at 96 dpi.
const EMU_PER_PX: f64 = 9525.0;

/// A VML shape's `width:…;height:…` in pixels at 96 dpi.
fn vml_size(style: &str) -> Option<(f32, f32)> {
    let mut w = None;
    let mut h = None;
    for decl in style.split(';') {
        let Some((k, v)) = decl.split_once(':') else {
            continue;
        };
        let px = css_px(v.trim());
        match k.trim() {
            "width" => w = px,
            "height" => h = px,
            _ => {}
        }
    }
    Some((w?, h?))
}

fn css_px(v: &str) -> Option<f32> {
    let (num, unit) = v.split_at(v.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(v.len()));
    let n: f32 = num.trim().parse().ok()?;
    Some(match unit {
        "pt" => n * 96.0 / 72.0,
        "in" => n * 96.0,
        "cm" => n * 96.0 / 2.54,
        "mm" => n * 96.0 / 25.4,
        "px" | "" => n,
        _ => return None,
    })
}

/// The URL of a `HYPERLINK "url"` field code; `\l "bookmark"` adds `#bookmark`.
fn hyperlink_field(code: &str) -> Option<String> {
    let code = code.trim_start();
    let rest = code
        .get(..9)
        .filter(|k| k.eq_ignore_ascii_case("HYPERLINK"))
        .map(|_| &code[9..])?;
    // The code's arguments: quoted, or separated by spaces.
    let mut args = Vec::new();
    let mut chars = rest.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c == '"' {
            chars.next();
            args.push(chars.by_ref().take_while(|&c| c != '"').collect::<String>());
        } else {
            let mut arg = String::new();
            while let Some(&c) = chars.peek().filter(|c| !c.is_whitespace()) {
                arg.push(c);
                chars.next();
            }
            args.push(arg);
        }
    }
    let mut url = None;
    let mut anchor = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "\\l" => anchor = args.next(),
            a if a.starts_with('\\') => {}
            _ if url.is_none() => url = Some(arg),
            _ => {}
        }
    }
    match (url, anchor) {
        (Some(u), Some(a)) => Some(format!("{u}#{a}")),
        (Some(u), None) => Some(u),
        (None, Some(a)) => Some(format!("#{a}")),
        (None, None) => None,
    }
}

/// A note reference whose mark is the text after it (`w:customMarkFollows`).
fn custom_mark(e: &BytesStart<'_>) -> bool {
    xml::attr(e, b"customMarkFollows").is_some_and(|v| matches!(v.as_ref(), "1" | "true" | "on"))
}

fn show(name: &[u8]) -> String {
    String::from_utf8_lossy(name).into_owned()
}
