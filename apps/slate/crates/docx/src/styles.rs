//! `word/styles.xml`: document defaults and styles, each resolved through its `basedOn`
//! chain; and the paragraph and run properties both styles and the body use.

use crate::xml::{self, XmlReader};
use crate::DocxError;
use document_model::{Align, ParagraphStyle, Rgb};
use quick_xml::events::{BytesStart, Event};
use std::collections::HashMap;

/// Run properties as one level of the hierarchy sets them; `None` leaves the level below.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct RunProps {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    /// Half-points.
    pub size: Option<u16>,
    /// `Some(None)` is `auto`: back to the default colour.
    pub color: Option<Option<Rgb>>,
    /// The character style (`w:rStyle`), in direct formatting only.
    pub style: Option<String>,
}

impl RunProps {
    /// Lay `over` on top of these properties.
    pub fn apply(&mut self, over: &RunProps) {
        fn set<T: Clone>(to: &mut Option<T>, from: &Option<T>) {
            if from.is_some() {
                to.clone_from(from);
            }
        }
        set(&mut self.bold, &over.bold);
        set(&mut self.italic, &over.italic);
        set(&mut self.underline, &over.underline);
        set(&mut self.strike, &over.strike);
        set(&mut self.size, &over.size);
        set(&mut self.color, &over.color);
    }

    /// Read the children of a `w:rPr` just started, up to its end.
    pub fn read(r: &mut XmlReader<'_>) -> Result<Self, DocxError> {
        let mut p = RunProps::default();
        loop {
            match r.read_event()? {
                Event::Empty(e) => p.property(&e),
                Event::Start(e) => {
                    p.property(&e);
                    xml::skip(r, &e)?;
                }
                Event::End(e) if e.local_name().as_ref() == b"rPr" => return Ok(p),
                Event::Eof => return Err(DocxError::Xml("unclosed w:rPr".into())),
                _ => {}
            }
        }
    }

    fn property(&mut self, e: &BytesStart<'_>) {
        match xml::local(e) {
            b"b" => self.bold = Some(xml::on(e)),
            b"i" => self.italic = Some(xml::on(e)),
            b"u" => self.underline = Some(xml::on(e)),
            b"strike" => self.strike = Some(xml::on(e)),
            // A double strikethrough is still struck through.
            b"dstrike" if xml::on(e) => self.strike = Some(true),
            b"sz" => {
                if let Some(v) = xml::num(e, b"val") {
                    self.size = Some(v);
                }
            }
            b"color" => {
                if let Some(v) = xml::val(e) {
                    self.color = Some(rgb(&v));
                }
            }
            b"rStyle" => self.style = xml::val(e).map(|v| v.into_owned()),
            _ => {}
        }
    }
}

/// A `w:color` value: `RRGGBB`, or `None` for `auto`.
fn rgb(v: &str) -> Option<Rgb> {
    if v.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(v, 16).ok()?;
    Some([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

/// Paragraph properties as one level sets them.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ParaProps {
    /// The paragraph style (`w:pStyle`), in the body only.
    pub style: Option<String>,
    pub align: Option<Align>,
    /// `w:numPr`: the numbering instance (0 = no list) and level.
    pub num_id: Option<u32>,
    pub level: Option<u8>,
    pub page_break_before: Option<bool>,
    /// The paragraph ends a section (`w:pPr/w:sectPr`): `true` if the next section starts
    /// on a new page (any section type but `continuous`).
    pub section_break: Option<bool>,
}

impl ParaProps {
    pub fn apply(&mut self, over: &ParaProps) {
        if over.align.is_some() {
            self.align = over.align;
        }
        if over.num_id.is_some() {
            self.num_id = over.num_id;
            self.level = over.level;
        } else if over.level.is_some() {
            self.level = over.level;
        }
        if over.page_break_before.is_some() {
            self.page_break_before = over.page_break_before;
        }
        if over.section_break.is_some() {
            self.section_break = over.section_break;
        }
    }

    /// Read the children of a `w:pPr` just started, up to its end. The paragraph mark's
    /// run properties (`w:pPr/w:rPr`) are skipped: they don't apply to the text.
    pub fn read(r: &mut XmlReader<'_>) -> Result<Self, DocxError> {
        let mut p = ParaProps::default();
        loop {
            match r.read_event()? {
                Event::Empty(e) => p.property(&e),
                Event::Start(e) => match xml::local(&e) {
                    b"numPr" => p.numbering(r)?,
                    b"sectPr" => p.section_break = Some(section_starts_page(r)?),
                    _ => {
                        p.property(&e);
                        xml::skip(r, &e)?;
                    }
                },
                Event::End(e) if e.local_name().as_ref() == b"pPr" => return Ok(p),
                Event::Eof => return Err(DocxError::Xml("unclosed w:pPr".into())),
                _ => {}
            }
        }
    }

    fn numbering(&mut self, r: &mut XmlReader<'_>) -> Result<(), DocxError> {
        loop {
            match r.read_event()? {
                Event::Empty(e) | Event::Start(e) => match xml::local(&e) {
                    b"ilvl" => self.level = xml::num(&e, b"val"),
                    b"numId" => self.num_id = xml::num(&e, b"val"),
                    _ => {}
                },
                Event::End(e) if e.local_name().as_ref() == b"numPr" => return Ok(()),
                Event::Eof => return Err(DocxError::Xml("unclosed w:numPr".into())),
                _ => {}
            }
        }
    }

    fn property(&mut self, e: &BytesStart<'_>) {
        match xml::local(e) {
            b"pStyle" => self.style = xml::val(e).map(|v| v.into_owned()),
            b"jc" => {
                self.align = match xml::val(e).as_deref() {
                    Some("center") => Some(Align::Center),
                    Some("right" | "end") => Some(Align::End),
                    Some(
                        "both" | "distribute" | "justify" | "lowKashida" | "mediumKashida"
                        | "highKashida" | "thaiDistribute",
                    ) => Some(Align::Justify),
                    Some("left" | "start") => Some(Align::Start),
                    _ => None,
                }
            }
            b"pageBreakBefore" => self.page_break_before = Some(xml::on(e)),
            _ => {}
        }
    }
}

/// Read a `w:sectPr` just started: whether its section starts on a new page.
fn section_starts_page(r: &mut XmlReader<'_>) -> Result<bool, DocxError> {
    let mut page = true;
    loop {
        match r.read_event()? {
            Event::Empty(e) if xml::local(&e) == b"type" => {
                page = xml::val(&e).as_deref() != Some("continuous");
            }
            Event::Start(e) => xml::skip(r, &e)?,
            Event::End(e) if e.local_name().as_ref() == b"sectPr" => return Ok(page),
            Event::Eof => return Err(DocxError::Xml("unclosed w:sectPr".into())),
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Paragraph,
    Character,
    Table,
    Numbering,
}

#[derive(Debug, Clone)]
struct Style {
    kind: Kind,
    name: String,
    based_on: Option<String>,
    ppr: ParaProps,
    rpr: RunProps,
}

/// A style with its `basedOn` chain applied (document defaults not included).
#[derive(Debug, Clone, Default)]
pub(crate) struct Resolved {
    pub ppr: ParaProps,
    pub rpr: RunProps,
}

#[derive(Debug, Default)]
pub(crate) struct Styles {
    styles: HashMap<String, Style>,
    resolved: HashMap<String, Resolved>,
    pub default_rpr: RunProps,
    pub default_ppr: ParaProps,
    /// The style of paragraphs that name none (`w:default="1"`).
    default_paragraph: Option<String>,
    default_character: Option<String>,
}

/// The deepest `basedOn` chain followed; deeper (or circular) chains stop there.
const MAX_CHAIN: usize = 32;

impl Styles {
    pub fn parse(xml: &str) -> Result<Self, DocxError> {
        let mut styles = Styles::default();
        let mut r = xml::reader(xml);
        loop {
            match r.read_event()? {
                Event::Start(e) if xml::local(&e) == b"rPrDefault" => {
                    styles.default_rpr = read_wrapped_rpr(&mut r, b"rPrDefault")?;
                }
                Event::Start(e) if xml::local(&e) == b"pPrDefault" => {
                    styles.default_ppr = read_wrapped_ppr(&mut r, b"pPrDefault")?;
                }
                Event::Start(e) if xml::local(&e) == b"style" => {
                    let id = xml::attr(&e, b"styleId").unwrap_or_default().into_owned();
                    let kind = match xml::attr(&e, b"type").as_deref() {
                        Some("character") => Kind::Character,
                        Some("table") => Kind::Table,
                        Some("numbering") => Kind::Numbering,
                        _ => Kind::Paragraph,
                    };
                    if xml::attr(&e, b"default").is_some_and(|d| d == "1" || d == "true") {
                        match kind {
                            Kind::Paragraph => styles.default_paragraph = Some(id.clone()),
                            Kind::Character => styles.default_character = Some(id.clone()),
                            _ => {}
                        }
                    }
                    let style = read_style(&mut r, kind)?;
                    styles.styles.entry(id).or_insert(style);
                }
                Event::Eof => break,
                _ => {}
            }
        }
        let ids: Vec<String> = styles.styles.keys().cloned().collect();
        for id in ids {
            let resolved = styles.resolve(&id);
            styles.resolved.insert(id, resolved);
        }
        Ok(styles)
    }

    fn resolve(&self, id: &str) -> Resolved {
        let mut chain = Vec::new();
        let mut at = Some(id);
        while let Some(s) = at.and_then(|i| self.styles.get(i)) {
            if chain.len() == MAX_CHAIN {
                break;
            }
            chain.push(s);
            at = s.based_on.as_deref();
        }
        let mut out = Resolved::default();
        for s in chain.into_iter().rev() {
            out.ppr.apply(&s.ppr);
            out.rpr.apply(&s.rpr);
        }
        out
    }

    /// A paragraph style with its chain applied, the default paragraph style when `id` is
    /// `None` or unknown.
    pub fn paragraph(&self, id: Option<&str>) -> Option<&Resolved> {
        let id = id
            .filter(|i| self.is(i, Kind::Paragraph))
            .or(self.default_paragraph.as_deref())?;
        self.resolved.get(id)
    }

    /// A character style with its chain applied.
    pub fn character(&self, id: Option<&str>) -> Option<&Resolved> {
        let id = id
            .filter(|i| self.is(i, Kind::Character))
            .or(self.default_character.as_deref())?;
        self.resolved.get(id)
    }

    /// The numbering instance a numbering style (`w:numStyleLink`) stands for.
    pub fn numbering_style_num(&self, id: &str) -> Option<u32> {
        if !self.is(id, Kind::Numbering) {
            return None;
        }
        self.resolved.get(id)?.ppr.num_id
    }

    fn is(&self, id: &str, kind: Kind) -> bool {
        self.styles.get(id).is_some_and(|s| s.kind == kind)
    }

    /// What a paragraph style is in the model, by its name: Word's built-in `Title`,
    /// `heading 1`..`heading 9` (7 to 9 show as 6), and quotes (`Quote`, `Intense Quote`,
    /// and LibreOffice's `Quotations`/`Block Quotation`). Anything else is normal: a
    /// style based on a heading looks like one through its properties, but isn't one.
    pub fn paragraph_style(&self, id: Option<&str>) -> ParagraphStyle {
        let id = id
            .filter(|i| self.is(i, Kind::Paragraph))
            .or(self.default_paragraph.as_deref());
        let Some(style) = id.and_then(|i| self.styles.get(i)) else {
            return ParagraphStyle::Normal;
        };
        style_of_name(&style.name)
    }
}

fn style_of_name(name: &str) -> ParagraphStyle {
    let lower = name.trim().to_lowercase();
    if lower == "title" {
        return ParagraphStyle::Title;
    }
    if let Some(n) = lower.strip_prefix("heading ") {
        if let Ok(n @ 1..=9) = n.trim().parse::<u8>() {
            return ParagraphStyle::Heading(n.min(6));
        }
    }
    match lower.as_str() {
        "quote" | "intense quote" | "quotations" | "block quotation" => ParagraphStyle::Quote,
        _ => ParagraphStyle::Normal,
    }
}

fn read_style(r: &mut XmlReader<'_>, kind: Kind) -> Result<Style, DocxError> {
    let mut style = Style {
        kind,
        name: String::new(),
        based_on: None,
        ppr: ParaProps::default(),
        rpr: RunProps::default(),
    };
    loop {
        match r.read_event()? {
            Event::Empty(e) => match xml::local(&e) {
                b"name" => style.name = xml::val(&e).unwrap_or_default().into_owned(),
                b"basedOn" => style.based_on = xml::val(&e).map(|v| v.into_owned()),
                _ => {}
            },
            Event::Start(e) => match xml::local(&e) {
                b"pPr" => style.ppr = ParaProps::read(r)?,
                b"rPr" => style.rpr = RunProps::read(r)?,
                _ => xml::skip(r, &e)?,
            },
            Event::End(e) if e.local_name().as_ref() == b"style" => return Ok(style),
            Event::Eof => return Err(DocxError::Xml("unclosed w:style".into())),
            _ => {}
        }
    }
}

/// `w:rPrDefault/w:rPr`.
fn read_wrapped_rpr(r: &mut XmlReader<'_>, end: &[u8]) -> Result<RunProps, DocxError> {
    let mut p = RunProps::default();
    loop {
        match r.read_event()? {
            Event::Start(e) if xml::local(&e) == b"rPr" => p = RunProps::read(r)?,
            Event::End(e) if e.local_name().as_ref() == end => return Ok(p),
            Event::Eof => return Err(DocxError::Xml("unclosed w:rPrDefault".into())),
            _ => {}
        }
    }
}

/// `w:pPrDefault/w:pPr`.
fn read_wrapped_ppr(r: &mut XmlReader<'_>, end: &[u8]) -> Result<ParaProps, DocxError> {
    let mut p = ParaProps::default();
    loop {
        match r.read_event()? {
            Event::Start(e) if xml::local(&e) == b"pPr" => p = ParaProps::read(r)?,
            Event::End(e) if e.local_name().as_ref() == end => return Ok(p),
            Event::Eof => return Err(DocxError::Xml("unclosed w:pPrDefault".into())),
            _ => {}
        }
    }
}
