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

use crate::numbering::{format_number, Counters, Numbering};
use crate::styles::{ParaProps, RunProps, Styles};
use crate::xml::{self, XmlReader};
use crate::{DocxError, Package, Rel};
use document_model::{
    Block, Image, ImageFormat, ImageSource, ListItem, Paragraph, RunStyle, Table, TableCell,
    TableRow,
};
use quick_xml::events::{BytesStart, Event};
use std::collections::HashMap;
use std::io::{Read, Seek};

pub(crate) fn read<R: Read + Seek>(
    xml: &str,
    styles: &Styles,
    numbering: &Numbering,
    rels: &HashMap<String, Rel>,
    package: &mut Package<R>,
) -> Result<Vec<Block>, DocxError> {
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
    };
    let mut out = Vec::new();
    loop {
        match body.r.read_event()? {
            Event::Start(e) if xml::local(&e) == b"body" => {
                body.blocks(b"body", &mut out, None)?;
                return Ok(out);
            }
            Event::Eof => return Ok(out),
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
    Image(Image),
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
    /// Block content up to the end of element `end` (`body`, `tc`). In a cell, `cell`
    /// takes the cell's properties.
    fn blocks(
        &mut self,
        end: &[u8],
        out: &mut Vec<Block>,
        mut cell: Option<&mut CellProps>,
    ) -> Result<(), DocxError> {
        // Cells of tables too deep to keep, open inside this element.
        let mut flattened = 0usize;
        loop {
            match self.r.read_event()? {
                Event::Start(e) => match xml::local(&e) {
                    b"p" => self.paragraph(out)?,
                    b"tbl" if self.depth < MAX_TABLE_DEPTH => {
                        self.depth += 1;
                        let table = self.table();
                        self.depth -= 1;
                        out.push(Block::Table(table?));
                    }
                    b"tbl" | b"tr" => {}
                    b"tc" => flattened += 1,
                    b"tcPr" if cell.is_some() => {
                        if let Some(c) = cell.as_deref_mut() {
                            self.cell_props(c)?;
                        }
                    }
                    name if wrapper(name) => {}
                    _ => xml::skip(&mut self.r, &e)?,
                },
                Event::Empty(e) if xml::local(&e) == b"p" => {
                    let (p, _) = self.paragraph_of(&ParaProps::default());
                    out.push(Block::Paragraph(p));
                }
                Event::End(e) if e.local_name().as_ref() == b"tc" && flattened > 0 => {
                    flattened -= 1;
                }
                Event::End(e) if e.local_name().as_ref() == end => return Ok(()),
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

    fn table(&mut self) -> Result<Table, DocxError> {
        let mut table = Table::default();
        loop {
            match self.r.read_event()? {
                Event::Start(e) => match xml::local(&e) {
                    b"tr" => table.rows.push(self.row()?),
                    name if wrapper(name) => {}
                    _ => xml::skip(&mut self.r, &e)?,
                },
                Event::End(e) if e.local_name().as_ref() == b"tbl" => return Ok(table),
                Event::Eof => return Err(DocxError::Xml("unclosed w:tbl".into())),
                _ => {}
            }
        }
    }

    fn row(&mut self) -> Result<TableRow, DocxError> {
        let mut row = TableRow::default();
        loop {
            match self.r.read_event()? {
                Event::Start(e) => match xml::local(&e) {
                    b"trPr" => row.header = self.header_row()?,
                    b"tc" => {
                        let mut props = CellProps { span: 1 };
                        let mut blocks = Vec::new();
                        self.blocks(b"tc", &mut blocks, Some(&mut props))?;
                        row.cells.push(TableCell {
                            blocks,
                            span: props.span,
                        });
                    }
                    name if wrapper(name) => {}
                    _ => xml::skip(&mut self.r, &e)?,
                },
                Event::End(e) if e.local_name().as_ref() == b"tr" => return Ok(row),
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

    /// An empty paragraph with the look `direct` and the styles give it, and the run
    /// properties its text starts from.
    fn paragraph_of(&self, direct: &ParaProps) -> (Paragraph, RunProps) {
        let style = self.styles.paragraph(direct.style.as_deref());
        let mut ppr = self.styles.default_ppr.clone();
        let mut rpr = self.styles.default_rpr.clone();
        if let Some(s) = style {
            ppr.apply(&s.ppr);
            rpr.apply(&s.rpr);
        }
        ppr.apply(direct);
        let mut p = Paragraph::new(self.styles.paragraph_style(direct.style.as_deref()));
        p.align = ppr.align.unwrap_or_default();
        (p, rpr)
    }

    /// The paragraph's list numbering, from its own `w:numPr` or its style's.
    fn list_props(&self, direct: &ParaProps) -> (Option<u32>, Option<u8>) {
        let mut ppr = self.styles.default_ppr.clone();
        if let Some(s) = self.styles.paragraph(direct.style.as_deref()) {
            ppr.apply(&s.ppr);
        }
        ppr.apply(direct);
        (ppr.num_id, ppr.level)
    }

    fn paragraph(&mut self, out: &mut Vec<Block>) -> Result<(), DocxError> {
        let mut direct = ParaProps::default();
        let mut items = Vec::new();
        let mut links: Vec<Option<String>> = Vec::new();
        let mut base: Option<RunProps> = None;
        loop {
            match self.r.read_event()? {
                Event::Start(e) => match xml::local(&e) {
                    b"pPr" => direct = ParaProps::read(&mut self.r)?,
                    b"r" => {
                        let base = base.get_or_insert_with(|| self.paragraph_of(&direct).1);
                        let link = links.iter().rev().find_map(|l| l.clone());
                        let base = base.clone();
                        self.run(&base, link, &mut items)?;
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
        let (template, _) = self.paragraph_of(direct);
        let mut list = match self.list_props(direct) {
            (Some(num), level) if num != 0 => {
                let style = direct.style.as_deref();
                let level = level
                    .or_else(|| style.and_then(|s| self.numbering.level_of_style(num, s)))
                    .unwrap_or(0);
                self.numbering
                    .next(&mut self.counters, num, level)
                    .map(|kind| (kind, level))
            }
            _ => None,
        };
        let mut ppr = self.styles.default_ppr.clone();
        if let Some(s) = self.styles.paragraph(direct.style.as_deref()) {
            ppr.apply(&s.ppr);
        }
        ppr.apply(direct);
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
        let section_mark = ppr.section_break.is_some()
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
                    Item::Image(image) => out.push(Block::Image(image)),
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
                })),
                None => out.push(Block::Paragraph(p)),
            }
        }
        if ppr.section_break == Some(true) {
            out.push(Block::PageBreak);
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

    fn run(
        &mut self,
        base: &RunProps,
        link: Option<String>,
        items: &mut Vec<Item>,
    ) -> Result<(), DocxError> {
        let mut direct = RunProps::default();
        let mut style: Option<RunStyle> = None;
        loop {
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
                        .get_or_insert_with(|| self.run_style(base, &direct, link.clone()))
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
                        b"drawing" if !empty => self.drawing(items)?,
                        b"pict" if !empty => self.pict(items)?,
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

    /// A run's look: document defaults, paragraph style, character style, direct
    /// formatting, each over the one before.
    fn run_style(&self, base: &RunProps, direct: &RunProps, link: Option<String>) -> RunStyle {
        let mut p = base.clone();
        if let Some(s) = self.styles.character(direct.style.as_deref()) {
            p.apply(&s.rpr);
        }
        p.apply(direct);
        let field_link = self.fields.iter().rev().find_map(|f| f.link.clone());
        RunStyle {
            bold: p.bold.unwrap_or(false),
            italic: p.italic.unwrap_or(false),
            underline: p.underline.unwrap_or(false),
            strike: p.strike.unwrap_or(false),
            size: p.size,
            color: p.color.flatten(),
            link: link.or(field_link),
            ..RunStyle::default()
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

    /// A DrawingML picture (`w:drawing`): every `a:blip` in it, sized by `wp:extent` and
    /// described by `wp:docPr`. Text boxes in drawings aren't read.
    fn drawing(&mut self, items: &mut Vec<Item>) -> Result<(), DocxError> {
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
        for id in blips {
            if let Some(image) = self.picture(&id, &alt, if one { size } else { None }) {
                if self.shows() {
                    items.push(Item::Image(image));
                }
            }
        }
        Ok(())
    }

    /// A VML picture (`w:pict`): `v:imagedata` sized by its shape's style. Text boxes
    /// aren't read.
    fn pict(&mut self, items: &mut Vec<Item>) -> Result<(), DocxError> {
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
        for id in ids {
            if let Some(image) = self.picture(&id, &alt, size) {
                if self.shows() {
                    items.push(Item::Image(image));
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
