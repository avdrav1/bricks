//! Slate's DOCX reader (SLATE-3, docs/adr/0003-slate-document-model.md): the main part of a
//! WordprocessingML package into the document model; and its save (SLATE-8), which
//! patches the package read ([`save`]).
//!
//! `zip` opens the package and `quick-xml` streams `word/document.xml`. Paragraph and run
//! looks resolve through `word/styles.xml` (document defaults, `basedOn` chains, the
//! paragraph style's then the character style's properties, then direct formatting) and
//! the theme's fonts (`word/theme/theme1.xml`): the document's paragraph styles become its
//! [`StyleSheet`](document_model::StyleSheet), each paragraph names its style, and each
//! run holds its character style's and its own formatting as overrides. List kinds,
//! levels, and numbers resolve through `word/numbering.xml`, and hyperlinks and pictures
//! through the part's relationships. Headers, footers, footnotes, comments, text boxes,
//! and field codes are not read (field results are). The last section's page width less
//! its margins (`w:sectPr`) is the document's text width, and `word/settings.xml`'s
//! `w:defaultTabStop` its default tab stop (ADR 0009); paragraphs' and styles' own stops
//! (`w:tabs`) merge as Word layers them. The font table gives each font's class and the
//! faces the package embeds, de-obfuscated (`fonts.rs`).

mod body;
mod fonts;
mod numbering;
mod save;
mod styles;
mod xml;

pub use save::{save, DocxSource};

use document_model::{Document, DocxInfo, Format};
use quick_xml::events::Event;
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
    Ok(read_package(reader)?.0)
}

/// Read the .docx file at `path`, and where its blocks came from in it, for [`save`].
pub fn read_source(path: &Path) -> Result<(Document, DocxSource), DocxError> {
    let (doc, main) = read_package(BufReader::new(File::open(path)?))?;
    let source = DocxSource::new(main, &doc.blocks)?;
    Ok((doc, source))
}

/// Slate's built-in blank Word document (SLATE-18): one empty paragraph, and Normal, Title,
/// Heading 1 to 6, Quote, and Hyperlink styles in Word's default look. Made by
/// `scripts/blank_docx.py`.
pub fn blank() -> &'static [u8] {
    include_bytes!("../data/blank.docx")
}

/// The main part as read: its name, its text, and where its blocks came from.
pub(crate) struct MainPart {
    pub name: String,
    pub xml: String,
    /// The part started with a byte-order mark.
    pub bom: bool,
    pub body: Option<body::Container>,
}

fn read_package<R: Read + Seek>(reader: R) -> Result<(Document, MainPart), DocxError> {
    let mut package = Package {
        zip: ZipArchive::new(reader)?,
        images: HashMap::new(),
    };
    let main = package.main_part()?;
    let rels = package.rels(&main)?;
    let (styles, numbering) = package.styles_and_numbering(&rels)?;
    let default_tab = package.default_tab(&rels)?;
    let fonts = package.font_table(&rels)?;
    let bytes = package.bytes(&main)?.ok_or(DocxError::NotDocx)?;
    let (bom, bytes) = match bytes.strip_prefix(b"\xEF\xBB\xBF") {
        Some(rest) => (true, rest.to_vec()),
        None => (false, bytes),
    };
    let xml =
        String::from_utf8(bytes).map_err(|_| DocxError::Xml(format!("{main} isn't UTF-8")))?;
    let body::Parsed {
        blocks,
        body,
        text_width,
    } = body::read(&xml, &styles, &numbering, &rels, &mut package)?;
    let doc = Document {
        blocks,
        styles: styles.sheet(),
        format: Format::Docx(DocxInfo {
            text_width: text_width.unwrap_or(DocxInfo::LETTER_TEXT_WIDTH),
            default_tab,
            fonts: Arc::new(fonts),
            numbering: Arc::new(numbering.model()),
        }),
    };
    let main = MainPart {
        name: main,
        xml,
        bom,
        body,
    };
    Ok((doc, main))
}

/// The kind of a relationship: its type URI's last segment (`styles`, `hyperlink`).
fn rel_kind(rel: &Rel) -> &str {
    rel.kind.rsplit('/').next().unwrap_or_default()
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
    /// The part the main part's relationships `rels` name of type `kind`, else `default`.
    fn part_of(rels: &HashMap<String, Rel>, kind: &str, default: &str) -> String {
        rels.values()
            .find(|r| !r.external && rel_kind(r) == kind)
            .map_or_else(|| default.to_owned(), |r| r.target.clone())
    }

    /// The styles (with the theme's fonts) and list numbering the main part's
    /// relationships `rels` name (else at their usual names); empty when the package has
    /// none.
    fn styles_and_numbering(
        &mut self,
        rels: &HashMap<String, Rel>,
    ) -> Result<(styles::Styles, numbering::Numbering), DocxError> {
        let part_of = |kind: &str, default: &str| Self::part_of(rels, kind, default);
        let theme = match self.text(&part_of("theme", "word/theme/theme1.xml"))? {
            Some(xml) => styles::Theme::parse(&xml)?,
            None => styles::Theme::default(),
        };
        let styles = match self.text(&part_of("styles", "word/styles.xml"))? {
            Some(xml) => styles::Styles::parse(&xml, theme)?,
            None => styles::Styles::empty(theme),
        };
        let numbering = match self.text(&part_of("numbering", "word/numbering.xml"))? {
            Some(xml) => numbering::Numbering::parse(&xml, &styles)?,
            None => numbering::Numbering::default(),
        };
        Ok((styles, numbering))
    }

    /// The default tab stop in twips: the settings part's `w:defaultTabStop`, Word's 0.5″
    /// when there is no settings part, it sets none, or it sets 0.
    fn default_tab(&mut self, rels: &HashMap<String, Rel>) -> Result<u32, DocxError> {
        let Some(xml) = self.text(&Self::part_of(rels, "settings", "word/settings.xml"))? else {
            return Ok(DocxInfo::DEFAULT_TAB);
        };
        let mut r = xml::reader(&xml);
        loop {
            match r.read_event()? {
                Event::Empty(e) | Event::Start(e) if xml::local(&e) == b"defaultTabStop" => {
                    let twips = xml::num::<u32>(&e, b"val").unwrap_or(0).min(31_680);
                    return Ok(if twips == 0 {
                        DocxInfo::DEFAULT_TAB
                    } else {
                        twips
                    });
                }
                Event::Eof => return Ok(DocxInfo::DEFAULT_TAB),
                _ => {}
            }
        }
    }

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
        self.rels_of(&rels_part(source), dir_of(source))
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

/// The directory of part `name` (`word` for `word/document.xml`).
fn dir_of(name: &str) -> &str {
    name.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// The name of part `source`'s relationships part.
fn rels_part(source: &str) -> String {
    match source.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{source}.rels"),
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
    use super::{read_from, resolve, DocxInfo, Format};
    use std::io::{Cursor, Write};

    /// The text width read from a package whose document's body is `body`.
    fn text_width(body: &str) -> u32 {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file(
            "word/document.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        write!(
            zip,
            "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
             <w:body>{body}</w:body></w:document>"
        )
        .unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        match read_from(Cursor::new(bytes)).unwrap().format {
            Format::Docx(info) => info.text_width,
            other => panic!("{other:?}"),
        }
    }

    const P: &str = "<w:p><w:r><w:t>Text</w:t></w:r></w:p>";

    #[test]
    fn the_text_width_is_the_page_less_its_margins_and_gutter() {
        // Letter with 0.5″ margins: 7.5″.
        let half_inch = "<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
             <w:pgMar w:top=\"720\" w:right=\"720\" w:bottom=\"720\" w:left=\"720\" \
             w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/></w:sectPr>";
        assert_eq!(text_width(&format!("{P}{half_inch}")), 10_800);
        // A4 with 1″ margins and a 0.5″ gutter.
        let gutter = "<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/>\
             <w:pgMar w:left=\"1440\" w:right=\"1440\" w:gutter=\"720\"/></w:sectPr>";
        assert_eq!(text_width(&format!("{P}{gutter}")), 11_906 - 2_880 - 720);
        // What a section doesn't say is Letter's with 1″ margins.
        assert_eq!(
            text_width(&format!(
                "{P}<w:sectPr><w:pgMar w:left=\"720\"/></w:sectPr>"
            )),
            12_240 - 720 - 1_440
        );
        assert_eq!(
            text_width(&format!("{P}<w:sectPr/>")),
            DocxInfo::LETTER_TEXT_WIDTH
        );
        // Margins wider than the page leave no width, not a wrapped-around one.
        assert_eq!(text_width("<w:sectPr><w:pgSz w:w=\"1000\"/></w:sectPr>"), 0);
    }

    #[test]
    fn the_last_section_gives_the_width() {
        let section = |w: u32| {
            format!(
                "<w:sectPr><w:pgSz w:w=\"{w}\"/><w:pgMar w:left=\"0\" w:right=\"0\"/></w:sectPr>"
            )
        };
        // An earlier section ends at a paragraph; the body's own is the last.
        let body = format!(
            "<w:p><w:pPr>{}</w:pPr><w:r><w:t>One</w:t></w:r></w:p>{P}{}",
            section(16_838),
            section(11_906)
        );
        assert_eq!(text_width(&body), 11_906);
        // Without the body's own, the last paragraph's.
        let body = format!(
            "<w:p><w:pPr>{}</w:pPr></w:p><w:p><w:pPr>{}</w:pPr></w:p>{P}",
            section(16_838),
            section(11_906)
        );
        assert_eq!(text_width(&body), 11_906);
    }

    #[test]
    fn a_document_without_a_section_is_letter_with_inch_margins() {
        assert_eq!(text_width(P), 9_360);
        assert_eq!(DocxInfo::LETTER_TEXT_WIDTH, 9_360);
    }

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
