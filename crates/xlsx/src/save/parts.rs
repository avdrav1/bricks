//! The package parts that list other parts (ADR 0010): the workbook (sheets, defined
//! names, calculation), its relationships, the content types, and the document
//! properties' sheet titles. Each is rewritten from the file's own, element by element:
//! what a save doesn't change is copied as written.

use super::cells::escape;
use crate::xml::{attr, num};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::io;

pub(crate) const REL_WORKSHEET: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet";
const NS_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
pub(crate) const CT_WORKSHEET: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";
pub(crate) const CT_WORKBOOK: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml";
pub(crate) const CT_STYLES: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml";
pub(crate) const REL_STYLES: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";

type Result<T, E = io::Error> = std::result::Result<T, E>;

fn xml_error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(format!("a package part doesn't read as XML: {e}"))
}

/// Read `xml` event by event; `f` gets each event and its bytes and says what to write.
fn rewrite(
    xml: &str,
    mut f: impl FnMut(&Event<'_>, &str, &mut String) -> Result<()>,
) -> Result<String> {
    let mut out = String::with_capacity(xml.len() + 256);
    let mut reader = Reader::from_str(xml);
    loop {
        let start = reader.buffer_position() as usize;
        let event = reader.read_event().map_err(xml_error)?;
        let end = reader.buffer_position() as usize;
        if matches!(event, Event::Eof) {
            break;
        }
        f(&event, &xml[start..end], &mut out)?;
    }
    Ok(out)
}

fn local(e: &BytesStart<'_>) -> Vec<u8> {
    e.local_name().as_ref().to_vec()
}

/// The element name's prefix (`x:` of `x:sheet`), or empty.
fn prefix(e: &BytesStart<'_>) -> String {
    let name = e.name();
    let name = String::from_utf8_lossy(name.as_ref());
    name.rsplit_once(':')
        .map_or(String::new(), |(p, _)| format!("{p}:"))
}

/// A start tag written again from its attributes, with `set` replacing or adding some.
fn tag(e: &BytesStart<'_>, set: &[(&str, String)], empty: bool) -> String {
    let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
    let mut out = format!("<{name}");
    for a in e.attributes().flatten() {
        let key = String::from_utf8_lossy(a.key.as_ref()).into_owned();
        let local = key.rsplit(':').next().unwrap_or(&key).to_owned();
        if set.iter().any(|(k, _)| *k == key || *k == local) {
            continue;
        }
        let value = crate::xml::value(a.clone())
            .map(|v| v.into_owned())
            .unwrap_or_default();
        out.push(' ');
        out.push_str(&key);
        out.push_str("=\"");
        escape(&value, &mut out);
        out.push('"');
    }
    for (k, v) in set {
        out.push(' ');
        out.push_str(k);
        out.push_str("=\"");
        escape(v, &mut out);
        out.push('"');
    }
    out.push_str(if empty { "/>" } else { ">" });
    out
}

/// One sheet of the saved workbook as the workbook part lists it.
pub(crate) enum Listed {
    /// The file's `index`th `<sheet>` (a worksheet under a maybe new name, or a chart
    /// sheet kept as it is).
    Old { index: usize, name: Option<String> },
    /// A new worksheet.
    New { rel_id: String, name: String },
}

/// The relationship ids of the workbook part's `<sheet>` elements, in order.
pub(crate) fn sheet_rel_ids(xml: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event().map_err(xml_error)? {
            Event::Start(e) | Event::Empty(e) if local(&e) == b"sheet" => {
                out.push(attr(&e, b"id").unwrap_or_default().into_owned());
            }
            Event::Eof => return Ok(out),
            _ => {}
        }
    }
}

/// What the workbook part needs.
pub(crate) struct WorkbookChange<'a> {
    /// Every sheet in its new order.
    pub sheets: &'a [Listed],
    /// The active sheet's index among `sheets`.
    pub active: usize,
    /// Defined names (without `=`) that Bricks has; `_xlnm.` names are the file's.
    pub names: &'a [(String, String)],
    /// Rewrites a name's formula for renamed and deleted sheets.
    pub refs: &'a dyn Fn(&str) -> String,
}

/// The order of a workbook's children (CT_Workbook).
const WORKBOOK_ORDER: [&[u8]; 18] = [
    b"fileVersion",
    b"fileSharing",
    b"workbookPr",
    b"absPath",
    b"workbookProtection",
    b"bookViews",
    b"sheets",
    b"functionGroups",
    b"externalReferences",
    b"definedNames",
    b"calcPr",
    b"oleSize",
    b"customWorkbookViews",
    b"pivotCaches",
    b"smartTagPr",
    b"smartTagTypes",
    b"webPublishing",
    b"fileRecoveryPr",
];

fn rank(order: &[&[u8]], name: &[u8]) -> usize {
    order.iter().position(|n| *n == name).unwrap_or(order.len())
}

pub(crate) fn workbook(xml: &str, change: &WorkbookChange<'_>) -> Result<String> {
    // The file's `<sheet>` elements in order (all kinds: chart sheets stay where they
    // were, after the worksheet before them) and its defined names.
    let mut listed: Vec<(String, String)> = Vec::new(); // (rel id, raw tag)
    let mut defined: Vec<DefinedName> = Vec::new();
    {
        let mut reader = Reader::from_str(xml);
        let mut in_name: Option<DefinedName> = None;
        loop {
            match reader.read_event().map_err(xml_error)? {
                Event::Start(e) | Event::Empty(e) if local(&e) == b"sheet" => {
                    let id = attr(&e, b"id").unwrap_or_default().into_owned();
                    listed.push((id, tag(&e, &[], true)));
                }
                Event::Start(e) if local(&e) == b"definedName" => {
                    in_name = Some(DefinedName::of(&e));
                }
                Event::Empty(e) if local(&e) == b"definedName" => defined.push(DefinedName::of(&e)),
                Event::Text(t) => {
                    if let Some(n) = in_name.as_mut() {
                        n.text.push_str(&t.decode().map_err(xml_error)?);
                    }
                }
                Event::GeneralRef(r) => {
                    if let Some(n) = in_name.as_mut() {
                        let ent = format!("&{};", String::from_utf8_lossy(&r));
                        if let Ok(u) = quick_xml::escape::unescape(&ent) {
                            n.text.push_str(&u);
                        }
                    }
                }
                Event::End(e) if e.local_name().as_ref() == b"definedName" => {
                    defined.extend(in_name.take());
                }
                Event::Eof => break,
                _ => {}
            }
        }
    }
    let mut wrote_names = false;
    let mut wrote_calc = false;
    let mut skipping: Option<Vec<u8>> = None;
    let mut out = rewrite(xml, |event, raw, out| {
        if let Some(name) = &skipping {
            if let Event::End(e) = event {
                if e.local_name().as_ref() == name.as_slice() {
                    skipping = None;
                }
            }
            return Ok(());
        }
        let (e, empty) = match event {
            Event::Start(e) => (e, false),
            Event::Empty(e) => (e, true),
            Event::End(e) if e.local_name().as_ref() == b"workbook" => {
                let p = String::from_utf8_lossy(e.name().as_ref())
                    .rsplit_once(':')
                    .map_or(String::new(), |(p, _)| format!("{p}:"));
                if !wrote_names {
                    names_block(&p, &defined, change, out);
                }
                if !wrote_calc {
                    out.push_str(&format!("<{p}calcPr fullCalcOnLoad=\"1\"/>"));
                }
                out.push_str(raw);
                return Ok(());
            }
            _ => {
                out.push_str(raw);
                return Ok(());
            }
        };
        let name = local(e);
        let p = prefix(e);
        let r = rank(&WORKBOOK_ORDER, &name);
        if r != WORKBOOK_ORDER.len() {
            if !wrote_names && r > rank(&WORKBOOK_ORDER, b"definedNames") {
                names_block(&p, &defined, change, out);
                wrote_names = true;
            }
            if !wrote_calc && r > rank(&WORKBOOK_ORDER, b"calcPr") {
                out.push_str(&format!("<{p}calcPr fullCalcOnLoad=\"1\"/>"));
                wrote_calc = true;
            }
        }
        match name.as_slice() {
            b"sheets" => {
                sheets_block(&p, &listed, change, out);
                if !empty {
                    skipping = Some(name);
                }
            }
            b"definedNames" => {
                names_block(&p, &defined, change, out);
                wrote_names = true;
                if !empty {
                    skipping = Some(name);
                }
            }
            b"calcPr" => {
                out.push_str(&tag(e, &[("fullCalcOnLoad", "1".into())], empty));
                wrote_calc = true;
            }
            b"workbookView" => {
                let active = change.active.to_string();
                out.push_str(&tag(
                    e,
                    &[("activeTab", active), ("firstSheet", "0".into())],
                    empty,
                ));
            }
            _ => out.push_str(raw),
        }
        Ok(())
    })?;
    out.shrink_to_fit();
    Ok(out)
}

fn sheets_block(
    p: &str,
    listed: &[(String, String)],
    change: &WorkbookChange<'_>,
    out: &mut String,
) {
    let mut next_id = listed
        .iter()
        .filter_map(|(_, raw)| sheet_id(raw))
        .max()
        .unwrap_or(0)
        + 1;
    out.push_str(&format!("<{p}sheets>"));
    for s in change.sheets {
        match s {
            Listed::Old { index, name } => {
                let raw = &listed[*index].1;
                match name {
                    Some(name) => out.push_str(&with_name(raw, name)),
                    None => out.push_str(raw),
                }
            }
            Listed::New { rel_id, name } => {
                out.push_str(&format!("<{p}sheet name=\""));
                escape(name, out);
                out.push_str(&format!(
                    "\" sheetId=\"{next_id}\" xmlns:r=\"{NS_REL}\" r:id=\"{rel_id}\"/>"
                ));
                next_id += 1;
            }
        }
    }
    out.push_str(&format!("</{p}sheets>"));
}

fn sheet_id(raw: &str) -> Option<u32> {
    let mut reader = Reader::from_str(raw);
    match reader.read_event().ok()? {
        Event::Empty(e) | Event::Start(e) => num(&e, b"sheetId"),
        _ => None,
    }
}

/// A `<sheet>` tag with its name attribute replaced.
fn with_name(raw: &str, name: &str) -> String {
    let mut reader = Reader::from_str(raw);
    match reader.read_event() {
        Ok(Event::Empty(e)) | Ok(Event::Start(e)) => tag(&e, &[("name", name.to_owned())], true),
        _ => raw.to_owned(),
    }
}

/// A defined name as the file has it.
struct DefinedName {
    name: String,
    local_sheet: Option<usize>,
    /// The other attributes (`hidden`, `comment`, …), as `key="value"` text.
    rest: String,
    text: String,
}

impl DefinedName {
    fn of(e: &BytesStart<'_>) -> Self {
        let mut rest = String::new();
        for a in e.attributes().flatten() {
            let key = String::from_utf8_lossy(a.key.as_ref()).into_owned();
            if key == "name" || key == "localSheetId" {
                continue;
            }
            let value = crate::xml::value(a.clone())
                .map(|v| v.into_owned())
                .unwrap_or_default();
            rest.push(' ');
            rest.push_str(&key);
            rest.push_str("=\"");
            escape(&value, &mut rest);
            rest.push('"');
        }
        Self {
            name: attr(e, b"name").unwrap_or_default().into_owned(),
            local_sheet: num(e, b"localSheetId"),
            rest,
            text: String::new(),
        }
    }
}

fn names_block(p: &str, defined: &[DefinedName], change: &WorkbookChange<'_>, out: &mut String) {
    let mut body = String::new();
    let write = |name: &str, local: Option<usize>, rest: &str, text: &str, body: &mut String| {
        body.push_str(&format!("<{p}definedName name=\""));
        escape(name, body);
        body.push('"');
        if let Some(l) = local {
            body.push_str(&format!(" localSheetId=\"{l}\""));
        }
        body.push_str(rest);
        body.push('>');
        escape(text, body);
        body.push_str(&format!("</{p}definedName>"));
    };
    // The file's own names (print areas and the like) follow their sheets; names Bricks
    // knows keep their scope if they kept their formula.
    for d in defined {
        let local = match d.local_sheet {
            Some(i) => {
                let new = change
                    .sheets
                    .iter()
                    .position(|s| matches!(s, Listed::Old { index, .. } if *index == i));
                match new {
                    Some(new) => Some(new),
                    None => continue, // its sheet was deleted
                }
            }
            None => None,
        };
        if d.name.starts_with("_xlnm.") {
            write(&d.name, local, &d.rest, &(change.refs)(&d.text), &mut body);
        } else if let Some((_, def)) = change.names.iter().find(|(n, _)| *n == d.name) {
            write(&d.name, local, &d.rest, def, &mut body);
        }
    }
    for (name, def) in change.names {
        if !defined.iter().any(|d| d.name == *name) {
            write(name, None, "", def, &mut body);
        }
    }
    if !body.is_empty() {
        out.push_str(&format!("<{p}definedNames>{body}</{p}definedNames>"));
    }
}

/// The workbook's relationships without `drop` (resolved part names) and with `add`
/// (`(id, type, target relative to the workbook's directory)`).
pub(crate) fn rels(
    xml: &str,
    dir: &str,
    drop: &[String],
    add: &[(String, &str, String)],
) -> Result<String> {
    rewrite(xml, |event, raw, out| {
        match event {
            Event::Empty(e) | Event::Start(e) if local(e) == b"Relationship" => {
                let external = attr(e, b"TargetMode").is_some_and(|m| m == "External");
                let target = attr(e, b"Target").unwrap_or_default();
                let part = crate::package::resolve_target(dir, &target);
                if external || !drop.contains(&part) {
                    out.push_str(raw);
                }
            }
            Event::End(e) if e.local_name().as_ref() == b"Relationships" => {
                for (id, kind, target) in add {
                    out.push_str("<Relationship Id=\"");
                    escape(id, out);
                    out.push_str("\" Type=\"");
                    escape(kind, out);
                    out.push_str("\" Target=\"");
                    escape(target, out);
                    out.push_str("\"/>");
                }
                out.push_str(raw);
            }
            _ => out.push_str(raw),
        }
        Ok(())
    })
}

/// The content types without overrides for `drop` (part names) and with `add` (part
/// names and types); `workbook` switches the workbook part's type (macros dropped), and
/// `drop_default` removes a default by extension (`bin`).
pub(crate) fn content_types(
    xml: &str,
    drop: &[String],
    add: &[(String, &str)],
    workbook: Option<(&str, &str)>,
    drop_default: Option<&str>,
) -> Result<String> {
    rewrite(xml, |event, raw, out| {
        match event {
            Event::Empty(e) | Event::Start(e) if local(e) == b"Override" => {
                let part = attr(e, b"PartName").unwrap_or_default();
                let part = part.trim_start_matches('/');
                if drop.iter().any(|d| d == part) {
                    return Ok(());
                }
                match workbook {
                    Some((wb, kind)) if part == wb => {
                        out.push_str(&tag(e, &[("ContentType", kind.to_owned())], true));
                    }
                    _ => out.push_str(raw),
                }
            }
            Event::Empty(e) | Event::Start(e) if local(e) == b"Default" => {
                let ext = attr(e, b"Extension").unwrap_or_default();
                if drop_default.is_some_and(|d| ext.eq_ignore_ascii_case(d)) {
                    return Ok(());
                }
                out.push_str(raw);
            }
            Event::End(e) if e.local_name().as_ref() == b"Types" => {
                for (part, kind) in add {
                    out.push_str("<Override PartName=\"/");
                    escape(part, out);
                    out.push_str("\" ContentType=\"");
                    escape(kind, out);
                    out.push_str("\"/>");
                }
                out.push_str(raw);
            }
            _ => out.push_str(raw),
        }
        Ok(())
    })
}

/// `docProps/app.xml` without its sheet titles, once sheets were added, removed, or
/// renamed (they're optional, and stale ones mislead).
pub(crate) fn app_properties(xml: &str) -> Result<String> {
    let mut skipping: Option<Vec<u8>> = None;
    rewrite(xml, |event, raw, out| {
        if let Some(name) = &skipping {
            if let Event::End(e) = event {
                if e.local_name().as_ref() == name.as_slice() {
                    skipping = None;
                }
            }
            return Ok(());
        }
        match event {
            Event::Start(e)
                if matches!(local(e).as_slice(), b"HeadingPairs" | b"TitlesOfParts") =>
            {
                skipping = Some(local(e));
            }
            Event::Empty(e)
                if matches!(local(e).as_slice(), b"HeadingPairs" | b"TitlesOfParts") => {}
            _ => out.push_str(raw),
        }
        Ok(())
    })
}
