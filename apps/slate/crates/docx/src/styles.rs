//! `word/styles.xml`: document defaults and styles, each resolved through its `basedOn`
//! chain, with theme fonts from `word/theme/theme1.xml`; the paragraph and run properties
//! both styles and the body use; and the document's [`StyleSheet`], its paragraph styles
//! resolved for the model.

use crate::xml::{self, XmlReader};
use crate::DocxError;
use document_model::{Align, LineSpacing, Look, ParaLook, Rgb, RunStyle, Style, StyleSheet};
use quick_xml::events::{BytesStart, Event};
use std::collections::HashMap;

/// A run's font as one level names it: a family, or the theme's major (headings) or
/// minor (body) font.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Font {
    Name(String),
    Theme { major: bool },
}

/// The theme's Latin fonts (`a:majorFont`, `a:minorFont`).
#[derive(Debug, Clone, Default)]
pub(crate) struct Theme {
    pub major: Option<String>,
    pub minor: Option<String>,
}

impl Theme {
    /// Read `word/theme/theme1.xml`'s font scheme.
    pub fn parse(xml: &str) -> Result<Theme, DocxError> {
        let mut theme = Theme::default();
        let mut r = xml::reader(xml);
        let mut major = None;
        loop {
            match r.read_event()? {
                Event::Start(e) => match xml::local(&e) {
                    b"majorFont" => major = Some(true),
                    b"minorFont" => major = Some(false),
                    _ => {}
                },
                Event::Empty(e) if xml::local(&e) == b"latin" => {
                    let face = xml::attr(&e, b"typeface")
                        .filter(|f| !f.trim().is_empty())
                        .map(|f| f.into_owned());
                    match major.take() {
                        Some(true) => theme.major = face,
                        Some(false) => theme.minor = face,
                        None => {}
                    }
                }
                Event::End(e) if matches!(e.local_name().as_ref(), b"majorFont" | b"minorFont") => {
                    major = None;
                }
                Event::Eof => return Ok(theme),
                _ => {}
            }
        }
    }

    fn font(&self, font: &Font) -> Option<String> {
        match font {
            Font::Name(n) => Some(n.clone()),
            Font::Theme { major: true } => self.major.clone(),
            Font::Theme { major: false } => self.minor.clone(),
        }
    }
}

/// Run properties as one level of the hierarchy sets them; `None` leaves the level below.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct RunProps {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    /// `w:rFonts`' font for Latin text: `asciiTheme`, `ascii`, `hAnsiTheme`, `hAnsi`, the
    /// first written.
    pub font: Option<Font>,
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
        set(&mut self.font, &over.font);
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
            b"rFonts" => {
                let theme = |name: &[u8]| {
                    xml::attr(e, name).map(|t| Font::Theme {
                        major: t.starts_with("major"),
                    })
                };
                let named = |name: &[u8]| {
                    xml::attr(e, name)
                        .filter(|f| !f.trim().is_empty())
                        .map(|f| Font::Name(f.into_owned()))
                };
                let font = theme(b"asciiTheme")
                    .or_else(|| named(b"ascii"))
                    .or_else(|| theme(b"hAnsiTheme"))
                    .or_else(|| named(b"hAnsi"));
                if font.is_some() {
                    self.font = font;
                }
            }
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
    /// `w:spacing`, in twips.
    pub space_before: Option<u32>,
    pub space_after: Option<u32>,
    pub line: Option<LineSpacing>,
    /// `w:ind`, in twips: the left indent and the first line's (negative: hanging).
    pub indent_left: Option<i32>,
    pub indent_first: Option<i32>,
    /// `w:outlineLvl`: a heading's level (1 to 9), `Some(None)` for body text.
    pub outline: Option<Option<u8>>,
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
        fn set<T: Clone>(to: &mut Option<T>, from: &Option<T>) {
            if from.is_some() {
                to.clone_from(from);
            }
        }
        set(&mut self.align, &over.align);
        set(&mut self.space_before, &over.space_before);
        set(&mut self.space_after, &over.space_after);
        set(&mut self.line, &over.line);
        set(&mut self.indent_left, &over.indent_left);
        set(&mut self.indent_first, &over.indent_first);
        set(&mut self.outline, &over.outline);
        if over.num_id.is_some() {
            self.num_id = over.num_id;
            self.level = over.level;
        } else if over.level.is_some() {
            self.level = over.level;
        }
        set(&mut self.page_break_before, &over.page_break_before);
        set(&mut self.section_break, &over.section_break);
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
            b"spacing" => {
                let twips = |name: &[u8]| xml::num::<i64>(e, name).map(|v| v.clamp(0, 31_680));
                if let Some(v) = twips(b"before") {
                    self.space_before = Some(v as u32);
                }
                if let Some(v) = twips(b"after") {
                    self.space_after = Some(v as u32);
                }
                if let Some(v) = twips(b"line") {
                    self.line = Some(match xml::attr(e, b"lineRule").as_deref() {
                        Some("exact") => LineSpacing::Exact(v as u32),
                        Some("atLeast") => LineSpacing::AtLeast(v as u32),
                        _ => LineSpacing::Auto(v as f32 / 240.0),
                    });
                }
            }
            b"ind" => {
                let twips =
                    |name: &[u8]| xml::num::<i64>(e, name).map(|v| v.clamp(-31_680, 31_680) as i32);
                if let Some(v) = twips(b"left").or_else(|| twips(b"start")) {
                    self.indent_left = Some(v);
                }
                if let Some(v) = twips(b"hanging") {
                    self.indent_first = Some(-v);
                } else if let Some(v) = twips(b"firstLine") {
                    self.indent_first = Some(v);
                }
            }
            b"outlineLvl" => {
                if let Some(n) = xml::num::<u8>(e, b"val") {
                    self.outline = Some((n < 9).then_some(n + 1));
                }
            }
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
struct StyleDef {
    kind: Kind,
    name: String,
    based_on: Option<String>,
    next: Option<String>,
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
    styles: HashMap<String, StyleDef>,
    /// The styles' ids in the order the part lists them.
    order: Vec<String>,
    resolved: HashMap<String, Resolved>,
    pub default_rpr: RunProps,
    pub default_ppr: ParaProps,
    /// The style of paragraphs that name none (`w:default="1"`).
    default_paragraph: Option<String>,
    default_character: Option<String>,
    theme: Theme,
}

/// The deepest `basedOn` chain followed; deeper (or circular) chains stop there.
const MAX_CHAIN: usize = 32;
/// Word's font and size where nothing sets them.
const DEFAULT_FONT: &str = "Times New Roman";
const DEFAULT_SIZE: u16 = 20;

impl Styles {
    /// No styles, with the theme's fonts.
    pub fn empty(theme: Theme) -> Self {
        Styles {
            theme,
            ..Styles::default()
        }
    }

    pub fn parse(xml: &str, theme: Theme) -> Result<Self, DocxError> {
        let mut styles = Styles::empty(theme);
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
                    if !styles.styles.contains_key(&id) {
                        styles.order.push(id.clone());
                        styles.styles.insert(id, style);
                    }
                }
                Event::Eof => break,
                _ => {}
            }
        }
        for id in &styles.order {
            let resolved = styles.resolve(id);
            styles.resolved.insert(id.clone(), resolved);
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
            .or(self.default_paragraph())?;
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

    /// A paragraph's properties: document defaults, its style's, then its own (`direct`).
    pub fn paragraph_props(&self, direct: &ParaProps) -> ParaProps {
        let mut ppr = self.default_ppr.clone();
        if let Some(s) = self.paragraph(direct.style.as_deref()) {
            ppr.apply(&s.ppr);
        }
        ppr.apply(direct);
        ppr
    }

    /// The id, in the [`StyleSheet`], of the style of a paragraph naming `id`: its own if
    /// it is a paragraph style, else the default's.
    pub fn paragraph_id<'a>(&'a self, id: Option<&'a str>) -> &'a str {
        id.filter(|i| self.is(i, Kind::Paragraph))
            .unwrap_or_else(|| self.default_id())
    }

    /// The id of the sheet's default style: the paragraph style marked default, else
    /// the one made of the document defaults alone (see [`Styles::sheet`]).
    fn default_id(&self) -> &str {
        match self.default_paragraph() {
            Some(id) => id,
            None if self.styles.contains_key("Normal") => "SlateDefault",
            None => "Normal",
        }
    }

    /// A run's direct formatting in the model: its character style's properties (with
    /// that style's chain), then its own, only what they set.
    pub fn overrides(&self, direct: &RunProps) -> RunStyle {
        let mut p = self
            .character(direct.style.as_deref())
            .map(|s| s.rpr.clone())
            .unwrap_or_default();
        p.apply(direct);
        RunStyle {
            bold: p.bold,
            italic: p.italic,
            underline: p.underline,
            strike: p.strike,
            font: p.font.as_ref().and_then(|f| self.theme.font(f)),
            size: p.size,
            color: p.color.flatten(),
            ..RunStyle::default()
        }
    }

    /// The document's paragraph styles for the model, each resolved through its chain and
    /// the document defaults, in the order the part lists them; the default style is the
    /// one marked default, else one made of the document defaults.
    pub fn sheet(&self) -> StyleSheet {
        let mut out: Vec<Style> = Vec::new();
        let default_id = self.default_id();
        if !self.is(default_id, Kind::Paragraph) {
            out.push(self.model_style(default_id, None, &Resolved::default()));
        }
        for id in &self.order {
            let def = &self.styles[id];
            if def.kind == Kind::Paragraph {
                out.push(self.model_style(id, Some(def), &self.resolved[id]));
            }
        }
        StyleSheet::new(out, default_id)
    }

    fn model_style(&self, id: &str, def: Option<&StyleDef>, resolved: &Resolved) -> Style {
        let mut rpr = self.default_rpr.clone();
        rpr.apply(&resolved.rpr);
        let mut ppr = self.default_ppr.clone();
        ppr.apply(&resolved.ppr);
        let name = def
            .map(|d| display_name(&d.name))
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| id.to_owned());
        let outline = match ppr.outline {
            Some(level) => level,
            None => heading_of_name(&name),
        };
        Style {
            id: id.to_owned(),
            title: name.eq_ignore_ascii_case("title"),
            name,
            based_on: def.and_then(|d| d.based_on.clone()),
            next: def.and_then(|d| d.next.clone()),
            outline,
            run: Look {
                font: rpr
                    .font
                    .as_ref()
                    .and_then(|f| self.theme.font(f))
                    .unwrap_or_else(|| DEFAULT_FONT.to_owned()),
                size: rpr.size.unwrap_or(DEFAULT_SIZE),
                bold: rpr.bold.unwrap_or(false),
                italic: rpr.italic.unwrap_or(false),
                underline: rpr.underline.unwrap_or(false),
                strike: rpr.strike.unwrap_or(false),
                color: rpr.color.flatten(),
            },
            para: ParaLook {
                align: ppr.align.unwrap_or_default(),
                space_before: ppr.space_before.unwrap_or(0),
                space_after: ppr.space_after.unwrap_or(0),
                line: ppr.line.unwrap_or_default(),
                indent_left: ppr.indent_left.unwrap_or(0),
                indent_first: ppr.indent_first.unwrap_or(0),
            },
        }
    }

    /// The paragraph style marked default, if there is one.
    pub fn default_paragraph(&self) -> Option<&str> {
        self.default_paragraph
            .as_deref()
            .filter(|id| self.is(id, Kind::Paragraph))
    }

    /// Whether the styles have one with id `id`, of any kind.
    pub fn has(&self, id: &str) -> bool {
        self.styles.contains_key(id)
    }

    /// The first paragraph style whose name is `name` (ignoring ASCII case).
    pub fn paragraph_named(&self, name: &str) -> Option<&str> {
        self.order
            .iter()
            .find(|id| {
                let s = &self.styles[*id];
                s.kind == Kind::Paragraph && s.name.trim().eq_ignore_ascii_case(name)
            })
            .map(String::as_str)
    }

    /// The theme's fonts.
    pub fn theme(&self) -> &Theme {
        &self.theme
    }
}

/// A style's name as Word shows it: the built-in names Word writes in lowercase
/// (`heading 1`, `footnote text`, `toc 1`) capitalized (`Heading 1`, `Footnote Text`,
/// `TOC 1`).
fn display_name(name: &str) -> String {
    let name = name.trim();
    if name.chars().any(char::is_uppercase) {
        return name.to_owned();
    }
    let words: Vec<String> = name
        .split(' ')
        .map(|w| match w {
            "toc" | "toa" => w.to_uppercase(),
            "of" => w.to_owned(),
            _ => {
                let mut c = w.chars();
                c.next()
                    .map(|f| f.to_uppercase().chain(c).collect())
                    .unwrap_or_default()
            }
        })
        .collect();
    words.join(" ")
}

/// The level of a style named `Heading N` (1 to 9), Word's built-in headings.
fn heading_of_name(name: &str) -> Option<u8> {
    let (word, n) = name.trim().split_once(' ')?;
    if !word.eq_ignore_ascii_case("heading") {
        return None;
    }
    match n.trim().parse::<u8>() {
        Ok(n @ 1..=9) => Some(n),
        _ => None,
    }
}

fn read_style(r: &mut XmlReader<'_>, kind: Kind) -> Result<StyleDef, DocxError> {
    let mut style = StyleDef {
        kind,
        name: String::new(),
        based_on: None,
        next: None,
        ppr: ParaProps::default(),
        rpr: RunProps::default(),
    };
    loop {
        match r.read_event()? {
            Event::Empty(e) => match xml::local(&e) {
                b"name" => style.name = xml::val(&e).unwrap_or_default().into_owned(),
                b"basedOn" => style.based_on = xml::val(&e).map(|v| v.into_owned()),
                b"next" => style.next = xml::val(&e).map(|v| v.into_owned()),
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

#[cfg(test)]
mod tests {
    use super::*;

    const W: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"";

    fn styles() -> Styles {
        let theme = Theme::parse(
            "<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\">\
             <a:themeElements><a:fontScheme name=\"Office\">\
             <a:majorFont><a:latin typeface=\"Calibri Light\"/><a:ea typeface=\"\"/></a:majorFont>\
             <a:minorFont><a:latin typeface=\"Calibri\"/><a:ea typeface=\"\"/></a:minorFont>\
             </a:fontScheme></a:themeElements></a:theme>",
        )
        .unwrap();
        let xml = format!(
            "<w:styles {W}><w:docDefaults>\
             <w:rPrDefault><w:rPr><w:rFonts w:asciiTheme=\"minorHAnsi\" w:hAnsiTheme=\"minorHAnsi\"/><w:sz w:val=\"22\"/></w:rPr></w:rPrDefault>\
             <w:pPrDefault><w:pPr><w:spacing w:after=\"160\" w:line=\"259\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault>\
             </w:docDefaults>\
             <w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/></w:style>\
             <w:style w:type=\"paragraph\" w:styleId=\"Heading1\"><w:name w:val=\"heading 1\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/>\
             <w:pPr><w:keepNext/><w:spacing w:before=\"240\" w:after=\"0\"/><w:outlineLvl w:val=\"0\"/></w:pPr>\
             <w:rPr><w:rFonts w:asciiTheme=\"majorHAnsi\" w:hAnsiTheme=\"majorHAnsi\"/><w:color w:val=\"2F5496\"/><w:sz w:val=\"32\"/></w:rPr></w:style>\
             <w:style w:type=\"paragraph\" w:styleId=\"Mine\"><w:name w:val=\"My Heading\"/><w:basedOn w:val=\"Heading1\"/>\
             <w:pPr><w:jc w:val=\"center\"/><w:ind w:left=\"720\" w:hanging=\"360\"/><w:spacing w:line=\"300\" w:lineRule=\"exact\"/></w:pPr>\
             <w:rPr><w:rFonts w:ascii=\"Arial\" w:hAnsi=\"Arial\"/><w:b/></w:rPr></w:style>\
             <w:style w:type=\"character\" w:styleId=\"Strong\"><w:name w:val=\"Strong\"/><w:rPr><w:b/><w:sz w:val=\"24\"/></w:rPr></w:style>\
             </w:styles>"
        );
        Styles::parse(&xml, theme).unwrap()
    }

    fn rpr(children: &str) -> RunProps {
        let xml = format!("<w:rPr {W}>{children}</w:rPr>");
        let mut r = xml::reader(&xml);
        let _ = r.read_event();
        RunProps::read(&mut r).unwrap()
    }

    #[test]
    fn styles_resolve_through_based_on_the_defaults_and_the_theme() {
        let sheet = styles().sheet();
        assert_eq!(sheet.default_id(), "Normal");
        let normal = sheet.get("Normal").unwrap();
        assert_eq!((normal.run.font.as_str(), normal.run.size), ("Calibri", 22));
        assert_eq!(normal.para.space_after, 160);
        assert_eq!(normal.para.line, LineSpacing::Auto(259.0 / 240.0));
        assert_eq!(normal.outline, None);

        let heading = sheet.get("Heading1").unwrap();
        assert_eq!(heading.name, "Heading 1");
        assert_eq!(heading.outline, Some(1));
        assert_eq!(heading.next.as_deref(), Some("Normal"));
        assert_eq!(heading.run.font, "Calibri Light");
        assert_eq!(heading.run.size, 32);
        assert_eq!(heading.run.color, Some([0x2F, 0x54, 0x96]));
        assert_eq!(
            (heading.para.space_before, heading.para.space_after),
            (240, 0)
        );
        assert_eq!(heading.para.line, LineSpacing::Auto(259.0 / 240.0));

        // A custom style based on the heading is one, in its own font and the rest of
        // the heading's look.
        let mine = sheet.get("Mine").unwrap();
        assert_eq!((mine.name.as_str(), mine.outline), ("My Heading", Some(1)));
        assert_eq!(mine.run.font, "Arial");
        assert!(mine.run.bold);
        assert_eq!(
            (mine.run.size, mine.run.color),
            (32, Some([0x2F, 0x54, 0x96]))
        );
        assert_eq!(mine.para.align, Align::Center);
        assert_eq!((mine.para.indent_left, mine.para.indent_first), (720, -360));
        assert_eq!(mine.para.line, LineSpacing::Exact(300));
        assert_eq!(mine.para.space_before, 240);
    }

    #[test]
    fn a_runs_overrides_are_its_character_style_and_its_own_formatting() {
        let styles = styles();
        let run = styles.overrides(&rpr(
            "<w:rStyle w:val=\"Strong\"/><w:i/><w:sz w:val=\"20\"/>",
        ));
        assert_eq!(
            run,
            RunStyle {
                bold: Some(true),
                italic: Some(true),
                size: Some(20),
                ..RunStyle::default()
            }
        );
        // Not the paragraph style's or the defaults': a plain run sets nothing.
        assert_eq!(styles.overrides(&rpr("")), RunStyle::default());
        let themed = styles.overrides(&rpr(
            "<w:rFonts w:asciiTheme=\"majorHAnsi\" w:ascii=\"Cambria\" w:eastAsia=\"MS Mincho\"/>",
        ));
        assert_eq!(themed.font.as_deref(), Some("Calibri Light"));
        let named = styles.overrides(&rpr("<w:rFonts w:hAnsi=\"Georgia\"/>"));
        assert_eq!(named.font.as_deref(), Some("Georgia"));
        assert_eq!(
            styles
                .overrides(&rpr("<w:rFonts w:eastAsia=\"MS Mincho\"/>"))
                .font,
            None
        );
    }
}
