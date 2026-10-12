//! Reading a file into a document, off the UI thread (SLATE-4): which reader its name
//! picks, what Slate refuses and why, the pictures decoded for the document, and what a
//! save needs to write it back (`saving::Target`).

use crate::saving::{Docx, Original, Target};
use crate::view::{texture_key, Textures};
use document_model::{Block, Document, Format, ImageFormat, ImageSource, MarkdownSource};
use gtk::{gdk, glib};
use std::fmt;
use std::path::Path;
use std::sync::Arc;

/// A document read and ready to show.
pub struct Opened {
    pub doc: Document,
    pub textures: Textures,
    pub words: usize,
    /// How a save writes it back: a Markdown file's source and a Word document's package,
    /// so a save changes only what was edited.
    pub target: Target,
}

/// Where the blocks came from, for a save.
enum Source {
    None,
    Markdown(MarkdownSource),
    Docx(docx::DocxSource, Original),
}

/// Why a file can't be shown, said to the user.
#[derive(Debug)]
pub struct OpenError(pub String);

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for OpenError {}

impl From<std::io::Error> for OpenError {
    fn from(e: std::io::Error) -> Self {
        OpenError(e.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    Markdown,
    Docx,
}

/// The reader for `path`, by its extension, or why Slate refuses it.
fn kind(path: &Path) -> Result<Kind, OpenError> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let refuse = |what: &str| {
        Err(OpenError(format!(
            "{what} can't be opened. Save it as .docx first."
        )))
    };
    match ext.as_str() {
        "txt" | "text" => Ok(Kind::Text),
        "md" | "markdown" | "mdown" | "mkd" => Ok(Kind::Markdown),
        "docx" => Ok(Kind::Docx),
        "doc" => refuse("A Word 97-2003 document (.doc)"),
        "odt" => refuse("An OpenDocument text (.odt)"),
        "rtf" => refuse("A Rich Text Format document (.rtf)"),
        "" => Err(OpenError(
            "Slate opens .txt, .md, and .docx files; this file has no extension.".into(),
        )),
        other => Err(OpenError(format!(
            "Slate opens .txt, .md, and .docx files, not .{other} files."
        ))),
    }
}

/// Read the file at `path`, merge its runs for editing ([`document_edit::normalize`]), and
/// decode its pictures. A Word document is read from a private copy of its package, which
/// its saves patch.
pub fn read(path: &Path) -> Result<Opened, OpenError> {
    let (mut doc, mut source) = match kind(path)? {
        Kind::Text => (
            document_model::read_text(&std::fs::read(path)?),
            Source::None,
        ),
        Kind::Markdown => {
            let (mut doc, source) = document_model::read_markdown_source(&std::fs::read(path)?);
            if let Some(dir) = path.parent() {
                document_model::load_linked_images(&mut doc.blocks, dir);
            }
            (doc, Source::Markdown(source))
        }
        Kind::Docx => {
            let mut head = [0u8; 8];
            let n = std::io::Read::read(&mut std::fs::File::open(path)?, &mut head)?;
            if head[..n] == [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1] {
                return Err(OpenError(
                    "This is an old Word document or a password-protected one. Save it as \
                     .docx without a password first."
                        .into(),
                ));
            }
            let original = Original::copy_of(path)?;
            let (doc, source) = docx::read_source(original.path())
                .map_err(|e| OpenError(sentence(&e.to_string())))?;
            (doc, Source::Docx(source, original))
        }
    };
    document_edit::normalize(&mut doc);
    // The source describes the blocks as shown: pictures loaded, runs merged.
    match &mut source {
        Source::None => {}
        Source::Markdown(s) => s.set_blocks(doc.blocks.clone()),
        Source::Docx(s, _) => s.set_blocks(doc.blocks.clone()),
    }
    let target = match (doc.format, source) {
        (Format::Markdown(info), Source::Markdown(source)) => Target::Markdown(info, Some(source)),
        (Format::Docx, Source::Docx(source, original)) => Target::Docx(Docx {
            source,
            original: Arc::new(original),
        }),
        (Format::Text(info), _) => Target::Text(info),
        (format, _) => unreachable!("{format:?} read without its source"),
    };
    let mut textures = Textures::new();
    decode_pictures(&doc.blocks, &mut textures);
    let words = doc.word_count();
    Ok(Opened {
        doc,
        textures,
        words,
        target,
    })
}

/// Decode every embedded PNG, JPEG, and GIF once; a picture that doesn't decode shows as
/// its alt text.
fn decode_pictures(blocks: &[Block], textures: &mut Textures) {
    for block in blocks {
        match block {
            Block::Image(image) => {
                let ImageSource::Embedded { format, bytes } = &image.source else {
                    continue;
                };
                if *format == ImageFormat::Other || textures.contains_key(&texture_key(bytes)) {
                    continue;
                }
                match gdk::Texture::from_bytes(&glib::Bytes::from(bytes.as_ref())) {
                    Ok(t) => {
                        textures.insert(texture_key(bytes), t);
                    }
                    Err(e) => tracing::warn!("cannot decode a picture: {e}"),
                }
            }
            Block::Table(table) => {
                for cell in table.rows.iter().flat_map(|r| &r.cells) {
                    decode_pictures(&cell.blocks, textures);
                }
            }
            _ => {}
        }
    }
}

/// `reason` as a sentence: capitalised, with a full stop.
fn sentence(reason: &str) -> String {
    let mut chars = reason.chars();
    let first = chars.next().map(|c| c.to_uppercase().collect::<String>());
    format!("{}{}.", first.unwrap_or_default(), chars.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reason(name: &str, bytes: &[u8]) -> String {
        let dir = std::env::temp_dir().join(format!("slate-open-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        let result = read(&path);
        std::fs::remove_file(&path).unwrap();
        match result {
            Ok(_) => panic!("{name} opened"),
            Err(e) => e.to_string(),
        }
    }

    #[test]
    fn other_word_formats_are_told_to_save_as_docx() {
        for name in ["old.doc", "story.odt", "letter.rtf", "SHOUT.DOC"] {
            let why = reason(name, b"anything");
            assert!(why.ends_with("Save it as .docx first."), "{name}: {why}");
        }
    }

    #[test]
    fn a_docx_that_is_not_a_package_says_so() {
        let why = reason("bad.docx", b"not a zip at all\n");
        assert!(
            why.starts_with("It isn't a readable Word document ("),
            "{why}"
        );
        assert!(why.ends_with(")."), "{why}");
    }

    #[test]
    fn an_old_binary_word_file_named_docx_is_told_to_save_as_docx() {
        let why = reason("old.docx", b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1rest");
        assert!(why.contains("Save it as .docx"), "{why}");
    }

    #[test]
    fn unknown_extensions_name_what_slate_opens() {
        assert_eq!(
            reason("table.csv", b"a,b\n"),
            "Slate opens .txt, .md, and .docx files, not .csv files."
        );
    }
}
