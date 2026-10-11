//! The parts of an XLSX package Bricks reads itself (calamine reads the cell values): where
//! the workbook, its styles, its theme, and each worksheet live, from the relationships.

use crate::xml::attr;
use quick_xml::events::Event;
use quick_xml::Reader;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::Path;
use zip::ZipArchive;

pub(crate) type Zip = ZipArchive<BufReader<File>>;

const REL_OFFICE_DOCUMENT: &str = "/officeDocument";
const REL_WORKSHEET: &str = "/worksheet";
const REL_STYLES: &str = "/styles";
const REL_THEME: &str = "/theme";

/// A worksheet's name and part, in workbook order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SheetPart {
    pub name: String,
    pub part: String,
}

/// Where a workbook's parts are.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Parts {
    pub worksheets: Vec<SheetPart>,
    pub styles: Option<String>,
    pub theme: Option<String>,
}

pub(crate) fn open(path: &Path) -> io::Result<Zip> {
    ZipArchive::new(BufReader::new(File::open(path)?)).map_err(io::Error::other)
}

/// Part `name` as text; `None` if the package has no such part.
pub(crate) fn read_part(zip: &mut Zip, name: &str) -> io::Result<Option<String>> {
    let mut file = match zip.by_name(name) {
        Ok(f) => f,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(e) => return Err(io::Error::other(e)),
    };
    let mut text = String::with_capacity(file.size() as usize);
    file.read_to_string(&mut text)?;
    Ok(Some(text))
}

/// Find the workbook's worksheets (in order), styles, and theme.
pub(crate) fn parts(zip: &mut Zip) -> io::Result<Parts> {
    let root = rels(zip, "")?;
    let workbook = root
        .iter()
        .find(|r| r.kind.ends_with(REL_OFFICE_DOCUMENT))
        .map_or_else(|| "xl/workbook.xml".to_owned(), |r| r.target.clone());
    let book_rels = rels(zip, &workbook)?;
    let by_id: HashMap<&str, &Rel> = book_rels.iter().map(|r| (r.id.as_str(), r)).collect();
    let find = |suffix: &str| {
        book_rels
            .iter()
            .find(|r| r.kind.ends_with(suffix))
            .map(|r| r.target.clone())
    };
    let mut parts = Parts {
        worksheets: Vec::new(),
        styles: find(REL_STYLES),
        theme: find(REL_THEME),
    };
    let xml = read_part(zip, &workbook)?.unwrap_or_default();
    let mut reader = Reader::from_str(&xml);
    loop {
        match reader.read_event().map_err(io::Error::other)? {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"sheet" => {
                let (Some(name), Some(id)) = (attr(&e, b"name"), attr(&e, b"id")) else {
                    continue;
                };
                if let Some(rel) = by_id.get(&*id).filter(|r| r.kind.ends_with(REL_WORKSHEET)) {
                    parts.worksheets.push(SheetPart {
                        name: name.into_owned(),
                        part: rel.target.clone(),
                    });
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(parts)
}

#[derive(Debug)]
struct Rel {
    id: String,
    kind: String,
    /// The target part's full name in the package.
    target: String,
}

/// The relationships of part `source` ("" for the package itself), targets resolved.
fn rels(zip: &mut Zip, source: &str) -> io::Result<Vec<Rel>> {
    let (dir, file) = source.rsplit_once('/').unwrap_or(("", source));
    let rels_name = if dir.is_empty() {
        format!("_rels/{file}.rels")
    } else {
        format!("{dir}/_rels/{file}.rels")
    };
    let Some(xml) = read_part(zip, &rels_name)? else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    let mut reader = Reader::from_str(&xml);
    loop {
        match reader.read_event().map_err(io::Error::other)? {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"Relationship" => {
                if attr(&e, b"TargetMode").is_some_and(|m| m == "External") {
                    continue;
                }
                let (Some(id), Some(kind), Some(target)) =
                    (attr(&e, b"Id"), attr(&e, b"Type"), attr(&e, b"Target"))
                else {
                    continue;
                };
                out.push(Rel {
                    id: id.into_owned(),
                    kind: kind.into_owned(),
                    target: resolve(dir, &target),
                });
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

/// A relationship target relative to directory `dir`, as a part name.
fn resolve(dir: &str, target: &str) -> String {
    let mut segments: Vec<&str> = if let Some(abs) = target.strip_prefix('/') {
        return normalize(abs.split('/').collect());
    } else if dir.is_empty() {
        Vec::new()
    } else {
        dir.split('/').collect()
    };
    segments.extend(target.split('/'));
    normalize(segments)
}

fn normalize(segments: Vec<&str>) -> String {
    let mut out: Vec<&str> = Vec::with_capacity(segments.len());
    for s in segments {
        match s {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    out.join("/")
}

#[cfg(test)]
mod tests {
    use super::resolve;

    #[test]
    fn targets_resolve_against_the_source_part() {
        assert_eq!(
            resolve("xl", "worksheets/sheet1.xml"),
            "xl/worksheets/sheet1.xml"
        );
        assert_eq!(resolve("xl", "/xl/styles.xml"), "xl/styles.xml");
        assert_eq!(
            resolve("xl/worksheets", "../drawings/d1.xml"),
            "xl/drawings/d1.xml"
        );
        assert_eq!(resolve("", "xl/workbook.xml"), "xl/workbook.xml");
    }
}
