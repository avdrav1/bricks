//! The parts of an XLSX package Bricks reads itself (calamine reads the cell values): where
//! the workbook, its styles, its theme, and each worksheet live, from the relationships.
//! A [`Package`] is a private copy of the file a workbook was read from, which saving
//! patches (ADR 0010).

use crate::xml::attr;
use quick_xml::events::Event;
use quick_xml::Reader;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};
use zip::ZipArchive;

pub(crate) type Zip = ZipArchive<BufReader<File>>;

const REL_OFFICE_DOCUMENT: &str = "/officeDocument";
const REL_WORKSHEET: &str = "/worksheet";
const REL_STYLES: &str = "/styles";
const REL_THEME: &str = "/theme";

/// A worksheet's name, part, and relationship id, in workbook order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SheetPart {
    pub name: String,
    pub part: String,
    pub rel_id: String,
}

/// Where a workbook's parts are.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Parts {
    /// The workbook part (`xl/workbook.xml`).
    pub workbook: String,
    pub worksheets: Vec<SheetPart>,
    pub styles: Option<String>,
    pub theme: Option<String>,
}

/// A private copy of the package a workbook was read from, in the cache directory and
/// removed with the last reference to it: saving reads it, so the original may change,
/// move, or be saved over meanwhile.
#[derive(Debug)]
pub struct Package {
    copy: PathBuf,
    pub(crate) parts: Parts,
}

impl Package {
    /// Copy the package at `path` and read where its parts are.
    pub fn copy_of(path: &Path) -> io::Result<Package> {
        let copy = csv_engine::scratch_path()?;
        std::fs::copy(path, &copy)?;
        // Made first so the copy is removed if reading it fails.
        let mut package = Package {
            copy,
            parts: Parts::default(),
        };
        package.parts = parts(&mut open(&package.copy)?)?;
        Ok(package)
    }

    /// Where the copy is.
    pub fn path(&self) -> &Path {
        &self.copy
    }
}

impl Drop for Package {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.copy);
    }
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
        workbook: workbook.clone(),
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
                        rel_id: rel.id.clone(),
                    });
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(parts)
}

#[derive(Debug, Clone)]
pub(crate) struct Rel {
    pub id: String,
    pub kind: String,
    /// The target part's full name in the package.
    pub target: String,
}

/// The name of part `source`'s relationships part ("" for the package's own).
pub(crate) fn rels_name(source: &str) -> String {
    let (dir, file) = source.rsplit_once('/').unwrap_or(("", source));
    if dir.is_empty() {
        format!("_rels/{file}.rels")
    } else {
        format!("{dir}/_rels/{file}.rels")
    }
}

/// The relationships of part `source` ("" for the package itself), targets resolved.
pub(crate) fn rels(zip: &mut Zip, source: &str) -> io::Result<Vec<Rel>> {
    let dir = source.rsplit_once('/').map_or("", |(d, _)| d);
    let rels_name = rels_name(source);
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
                    target: resolve_target(dir, &target),
                });
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

/// A relationship target relative to directory `dir`, as a part name.
pub(crate) fn resolve_target(dir: &str, target: &str) -> String {
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
    use super::resolve_target as resolve;

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
