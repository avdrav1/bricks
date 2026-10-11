//! `word/numbering.xml`: list definitions (abstract numberings and their levels) and the
//! numbering instances paragraphs name, and the counters that number list items.
//!
//! Counters belong to the abstract numbering, so two instances of one definition count on
//! from each other, as in Word; an instance's `w:startOverride` restarts a level the first
//! time the instance is used. Numbering a level restarts the levels below it.

use crate::styles::Styles;
use crate::xml::{self, XmlReader};
use crate::DocxError;
use document_model::ListKind;
use quick_xml::events::Event;
use std::collections::{HashMap, HashSet};

const LEVELS: usize = 9;

#[derive(Debug, Clone)]
struct Level {
    start: u64,
    /// `w:numFmt`: `decimal`, `bullet`, `lowerLetter`, `none`…
    format: String,
    /// `w:lvlText`: `%1.`, `%1.%2.`, or a bullet character.
    text: String,
    /// `w:isLgl`: the levels this one shows are written as decimals.
    legal: bool,
    /// `w:pStyle`: the paragraph style this level belongs to (outline numbering).
    style: Option<String>,
}

impl Default for Level {
    fn default() -> Self {
        Self {
            start: 1,
            format: "decimal".into(),
            text: String::new(),
            legal: false,
            style: None,
        }
    }
}

#[derive(Debug, Default)]
struct Abstract {
    levels: HashMap<u8, Level>,
    /// `w:numStyleLink`: the definition lives in the numbering style of that id.
    style_link: Option<String>,
}

#[derive(Debug, Default)]
struct Num {
    abstract_id: u32,
    /// `w:lvlOverride/w:startOverride` per level.
    starts: HashMap<u8, u64>,
    /// `w:lvlOverride/w:lvl`: whole levels replaced for this instance.
    levels: HashMap<u8, Level>,
}

#[derive(Debug, Default)]
pub(crate) struct Numbering {
    abstracts: HashMap<u32, Abstract>,
    nums: HashMap<u32, Num>,
}

/// Where list numbering stands while the document is read.
#[derive(Debug, Default)]
pub(crate) struct Counters {
    counts: HashMap<u32, [Option<u64>; LEVELS]>,
    started: HashSet<u32>,
}

impl Numbering {
    pub fn parse(xml: &str, styles: &Styles) -> Result<Self, DocxError> {
        let mut out = Numbering::default();
        let mut r = xml::reader(xml);
        loop {
            match r.read_event()? {
                Event::Start(e) if xml::local(&e) == b"abstractNum" => {
                    let id = xml::num(&e, b"abstractNumId").unwrap_or(u32::MAX);
                    let a = read_abstract(&mut r)?;
                    out.abstracts.entry(id).or_insert(a);
                }
                Event::Start(e) if xml::local(&e) == b"num" => {
                    let id = xml::num(&e, b"numId").unwrap_or(u32::MAX);
                    let n = read_num(&mut r)?;
                    out.nums.entry(id).or_insert(n);
                }
                Event::Eof => break,
                _ => {}
            }
        }
        // An abstract numbering that links to a numbering style uses that style's
        // instance's definition.
        let links: Vec<(u32, u32)> = out
            .nums
            .iter()
            .filter_map(|(&id, n)| {
                let link = out.abstracts.get(&n.abstract_id)?.style_link.as_deref()?;
                let target = out.nums.get(&styles.numbering_style_num(link)?)?;
                Some((id, target.abstract_id))
            })
            .collect();
        for (id, abstract_id) in links {
            if let Some(n) = out.nums.get_mut(&id) {
                n.abstract_id = abstract_id;
            }
        }
        Ok(out)
    }

    fn level<'a>(&'a self, num: &'a Num, level: u8) -> Option<&'a Level> {
        num.levels
            .get(&level)
            .or_else(|| self.abstracts.get(&num.abstract_id)?.levels.get(&level))
    }

    /// The level of instance `num_id` that belongs to paragraph style `style`, for a
    /// paragraph whose style numbers it without naming a level.
    pub fn level_of_style(&self, num_id: u32, style: &str) -> Option<u8> {
        let num = self.nums.get(&num_id)?;
        (0..LEVELS as u8).find(|&l| {
            self.level(num, l)
                .is_some_and(|lv| lv.style.as_deref() == Some(style))
        })
    }

    /// Number the next item of instance `num_id` at `level`: its kind and label. `None`
    /// when the instance or level doesn't exist (the paragraph isn't a list item).
    pub fn next(&self, counters: &mut Counters, num_id: u32, level: u8) -> Option<ListKind> {
        let level = level.min(LEVELS as u8 - 1);
        let num = self.nums.get(&num_id)?;
        let this = self.level(num, level)?;
        let counts = counters.counts.entry(num.abstract_id).or_default();
        if counters.started.insert(num_id) {
            for &l in num.starts.keys() {
                if let Some(c) = counts.get_mut(l as usize) {
                    *c = None;
                }
            }
        }
        let start = |l: u8| {
            num.starts
                .get(&l)
                .copied()
                .or_else(|| self.level(num, l).map(|lv| lv.start))
                .unwrap_or(1)
        };
        let n = counts[level as usize].map_or_else(|| start(level), |n| n + 1);
        counts[level as usize] = Some(n);
        for c in &mut counts[level as usize + 1..] {
            *c = None;
        }
        if this.format == "bullet" {
            return Some(ListKind::Bullet);
        }
        let mut label = String::new();
        let mut chars = this.text.chars().peekable();
        while let Some(c) = chars.next() {
            match (c, chars.peek().and_then(|d| d.to_digit(10))) {
                ('%', Some(d @ 1..=9)) => {
                    chars.next();
                    let l = (d - 1) as u8;
                    let value = counts[l as usize].unwrap_or_else(|| start(l));
                    let format = match self.level(num, l) {
                        Some(lv) if !this.legal => lv.format.as_str(),
                        _ => "decimal",
                    };
                    label.push_str(&format_number(value, format));
                }
                (c, _) => label.push(c),
            }
        }
        Some(ListKind::Ordered { number: n, label })
    }
}

/// `n` in a `w:numFmt` style. Formats Slate doesn't know show as decimals.
pub(crate) fn format_number(n: u64, format: &str) -> String {
    match format {
        "none" => String::new(),
        "decimalZero" => format!("{n:02}"),
        "lowerLetter" => letters(n, b'a'),
        "upperLetter" => letters(n, b'A'),
        "lowerRoman" => roman(n).to_lowercase(),
        "upperRoman" => roman(n),
        "ordinal" => {
            let suffix = match (n % 10, n % 100) {
                (_, 11..=13) => "th",
                (1, _) => "st",
                (2, _) => "nd",
                (3, _) => "rd",
                _ => "th",
            };
            format!("{n}{suffix}")
        }
        _ => n.to_string(),
    }
}

/// Word's letter numbering: a..z, then aa..zz, aaa…
fn letters(n: u64, a: u8) -> String {
    if n == 0 {
        return String::new();
    }
    let letter = (a + ((n - 1) % 26) as u8) as char;
    std::iter::repeat_n(letter, ((n - 1) / 26 + 1) as usize).collect()
}

fn roman(mut n: u64) -> String {
    const TABLE: [(u64, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for (value, digits) in TABLE {
        while n >= value {
            out.push_str(digits);
            n -= value;
        }
    }
    out
}

fn read_abstract(r: &mut XmlReader<'_>) -> Result<Abstract, DocxError> {
    let mut a = Abstract::default();
    loop {
        match r.read_event()? {
            Event::Start(e) if xml::local(&e) == b"lvl" => {
                let ilvl = xml::num(&e, b"ilvl").unwrap_or(0);
                let level = read_level(r)?;
                a.levels.insert(ilvl, level);
            }
            Event::Empty(e) if xml::local(&e) == b"numStyleLink" => {
                a.style_link = xml::val(&e).map(|v| v.into_owned());
            }
            Event::End(e) if e.local_name().as_ref() == b"abstractNum" => return Ok(a),
            Event::Eof => return Err(DocxError::Xml("unclosed w:abstractNum".into())),
            _ => {}
        }
    }
}

fn read_level(r: &mut XmlReader<'_>) -> Result<Level, DocxError> {
    let mut level = Level::default();
    loop {
        match r.read_event()? {
            Event::Empty(e) => match xml::local(&e) {
                b"start" => level.start = xml::num(&e, b"val").unwrap_or(1),
                b"numFmt" => level.format = xml::val(&e).unwrap_or_default().into_owned(),
                b"lvlText" => level.text = xml::val(&e).unwrap_or_default().into_owned(),
                b"isLgl" => level.legal = xml::on(&e),
                b"pStyle" => level.style = xml::val(&e).map(|v| v.into_owned()),
                _ => {}
            },
            Event::Start(e) => xml::skip(r, &e)?,
            Event::End(e) if e.local_name().as_ref() == b"lvl" => return Ok(level),
            Event::Eof => return Err(DocxError::Xml("unclosed w:lvl".into())),
            _ => {}
        }
    }
}

fn read_num(r: &mut XmlReader<'_>) -> Result<Num, DocxError> {
    let mut num = Num::default();
    let mut override_level = None;
    loop {
        match r.read_event()? {
            Event::Empty(e) => match xml::local(&e) {
                b"abstractNumId" => num.abstract_id = xml::num(&e, b"val").unwrap_or(u32::MAX),
                b"startOverride" => {
                    if let (Some(l), Some(v)) = (override_level, xml::num(&e, b"val")) {
                        num.starts.insert(l, v);
                    }
                }
                _ => {}
            },
            Event::Start(e) => match xml::local(&e) {
                b"lvlOverride" => override_level = xml::num(&e, b"ilvl"),
                b"lvl" => {
                    let ilvl = xml::num(&e, b"ilvl").or(override_level).unwrap_or(0);
                    let level = read_level(r)?;
                    num.levels.insert(ilvl, level);
                }
                _ => xml::skip(r, &e)?,
            },
            Event::End(e) if e.local_name().as_ref() == b"num" => return Ok(num),
            Event::Eof => return Err(DocxError::Xml("unclosed w:num".into())),
            _ => {}
        }
    }
}
