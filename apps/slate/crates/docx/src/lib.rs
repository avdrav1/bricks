//! Slate's DOCX reader (SLATE-3, docs/adr/0003-slate-document-model.md): the main part of a
//! WordprocessingML package into the document model.
//!
//! `zip` opens the package and `quick-xml` streams `word/document.xml`. Paragraph and run
//! looks resolve through `word/styles.xml` (document defaults, `basedOn` chains, the
//! paragraph style's then the character style's properties, then direct formatting), list
//! kinds, levels, and numbers through `word/numbering.xml`, and hyperlinks and pictures
//! through the part's relationships. Headers, footers, footnotes, comments, text boxes,
//! and field codes are not read (field results are).

mod body;
mod numbering;
mod styles;
mod xml;

use document_model::{Document, Format};
use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek};
use std::path::Path;
use std::sync::Arc;
use zip::ZipArchive;

#[derive(Debug)]
pub enum DocxError {
    Io(io::Error),
    /// The file isn't a zip package, or a part can't be read from it.
    Package(String),
    /// A part isn't well-formed XML.
    Xml(String),
    /// A zip package without a Word document in it.
    NotDocx,
}

impl fmt::Display for DocxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DocxError::Io(e) => write!(f, "{e}"),
            DocxError::Package(e) => write!(f, "it isn't a readable Word document ({e})"),
            DocxError::Xml(e) => write!(f, "the document is damaged ({e})"),
            DocxError::NotDocx => write!(f, "it isn't a Word document (no word/document.xml)"),
        }
    }
}

impl std::error::Error for DocxError {}

impl From<io::Error> for DocxError {
    fn from(e: io::Error) -> Self {
        DocxError::Io(e)
    }
}

impl From<zip::result::ZipError> for DocxError {
    fn from(e: zip::result::ZipError) -> Self {
        match e {
            zip::result::ZipError::Io(e) => DocxError::Io(e),
            e => DocxError::Package(e.to_string()),
        }
    }
}

impl From<quick_xml::Error> for DocxError {
    fn from(e: quick_xml::Error) -> Self {
        DocxError::Xml(e.to_string())
    }
}

impl From<quick_xml::events::attributes::AttrError> for DocxError {
    fn from(e: quick_xml::events::attributes::AttrError) -> Self {
        DocxError::Xml(e.to_string())
    }
}

/// Read the .docx file at `path`.
pub fn read(path: &Path) -> Result<Document, DocxError> {
    read_from(BufReader::new(File::open(path)?))
}

/// Read a .docx package.
pub fn read_from<R: Read + Seek>(reader: R) -> Result<Document, DocxError> {
    let mut package = Package {
        zip: ZipArchive::new(reader)?,
        images: HashMap::new(),
    };
    let main = package.main_part()?;
    let rels = package.rels(&main)?;
    let part_of = |kind: &str, default: &str| {
        rels.values()
            .find(|r| !r.external && r.kind.rsplit('/').next() == Some(kind))
            .map_or_else(|| default.to_owned(), |r| r.target.clone())
    };
    let styles_part = part_of("styles", "word/styles.xml");
    let numbering_part = part_of("numbering", "word/numbering.xml");
    let styles = match package.text(&styles_part)? {
        Some(xml) => styles::Styles::parse(&xml)?,
        None => styles::Styles::default(),
    };
    let numbering = match package.text(&numbering_part)? {
        Some(xml) => numbering::Numbering::parse(&xml, &styles)?,
        None => numbering::Numbering::default(),
    };
    let xml = package.text(&main)?.ok_or(DocxError::NotDocx)?;
    let blocks = body::read(&xml, &styles, &numbering, &rels, &mut package)?;
    Ok(Document {
        blocks,
        format: Format::Docx,
    })
}

/// A relationship of the main part: where its target is and what it is.
#[derive(Debug, Clone)]
pub(crate) struct Rel {
    /// The part's name in the package, or the URL of an external target.
    pub target: String,
    pub external: bool,
    /// The relationship type URI.
    pub kind: String,
}

pub(crate) struct Package<R> {
    zip: ZipArchive<R>,
    /// Pictures read so far, by part name: a picture used twice is read once.
    images: HashMap<String, Option<Arc<[u8]>>>,
}

impl<R: Read + Seek> Package<R> {
    /// A part's bytes, `None` if the package has no such part.
    fn bytes(&mut self, name: &str) -> Result<Option<Vec<u8>>, DocxError> {
        let mut file = match self.zip.by_name(name) {
            Ok(f) => f,
            Err(zip::result::ZipError::FileNotFound) => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let mut out = Vec::with_capacity(file.size() as usize);
        file.read_to_end(&mut out)?;
        Ok(Some(out))
    }

    /// A part as UTF-8 text (an XML part's byte-order mark stripped).
    fn text(&mut self, name: &str) -> Result<Option<String>, DocxError> {
        let Some(bytes) = self.bytes(name)? else {
            return Ok(None);
        };
        let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
        match String::from_utf8(bytes.to_vec()) {
            Ok(s) => Ok(Some(s)),
            Err(_) => Err(DocxError::Xml(format!("{name} isn't UTF-8"))),
        }
    }

    /// The main document part: the package's officeDocument relationship, else the usual
    /// name.
    fn main_part(&mut self) -> Result<String, DocxError> {
        let rels = self.rels_of("_rels/.rels", "")?;
        let main = rels
            .into_values()
            .find(|r| r.kind.ends_with("/officeDocument") && !r.external)
            .map_or_else(|| "word/document.xml".to_owned(), |r| r.target);
        if self.zip.index_for_name(&main).is_none() {
            return Err(DocxError::NotDocx);
        }
        Ok(main)
    }

    /// The relationships of part `source`, targets resolved to part names.
    fn rels(&mut self, source: &str) -> Result<HashMap<String, Rel>, DocxError> {
        let (dir, file) = source.rsplit_once('/').unwrap_or(("", source));
        let rels_name = if dir.is_empty() {
            format!("_rels/{file}.rels")
        } else {
            format!("{dir}/_rels/{file}.rels")
        };
        self.rels_of(&rels_name, dir)
    }

    fn rels_of(&mut self, rels_name: &str, dir: &str) -> Result<HashMap<String, Rel>, DocxError> {
        let mut out = HashMap::new();
        let Some(xml) = self.text(rels_name)? else {
            return Ok(out);
        };
        let mut r = xml::reader(&xml);
        loop {
            match r.read_event()? {
                quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e)
                    if xml::local(&e) == b"Relationship" =>
                {
                    let (Some(id), Some(target)) = (xml::attr(&e, b"Id"), xml::attr(&e, b"Target"))
                    else {
                        continue;
                    };
                    let external = xml::attr(&e, b"TargetMode").as_deref() == Some("External");
                    let target = if external {
                        target.into_owned()
                    } else {
                        resolve(dir, &target)
                    };
                    let kind = xml::attr(&e, b"Type").unwrap_or_default().into_owned();
                    out.insert(
                        id.into_owned(),
                        Rel {
                            target,
                            external,
                            kind,
                        },
                    );
                }
                quick_xml::events::Event::Eof => break,
                _ => {}
            }
        }
        Ok(out)
    }

    /// A picture part's bytes, read once.
    pub(crate) fn image(&mut self, name: &str) -> Option<Arc<[u8]>> {
        if let Some(found) = self.images.get(name) {
            return found.clone();
        }
        let bytes = self.bytes(name).ok().flatten().map(Arc::from);
        self.images.insert(name.to_owned(), bytes.clone());
        bytes
    }
}

/// A relationship target as a part name: relative to the source part's directory, or
/// absolute (`/word/media/x.png`).
fn resolve(dir: &str, target: &str) -> String {
    let joined = match target.strip_prefix('/') {
        Some(absolute) => absolute.to_owned(),
        None if dir.is_empty() => target.to_owned(),
        None => format!("{dir}/{target}"),
    };
    let mut parts: Vec<&str> = Vec::new();
    for seg in joined.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::resolve;

    #[test]
    fn targets_resolve_against_the_source_part() {
        assert_eq!(resolve("word", "media/image1.png"), "word/media/image1.png");
        assert_eq!(
            resolve("word", "../customXml/item1.xml"),
            "customXml/item1.xml"
        );
        assert_eq!(resolve("word", "/word/media/a.gif"), "word/media/a.gif");
        assert_eq!(resolve("", "word/document.xml"), "word/document.xml");
    }
}
