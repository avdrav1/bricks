//! Reading a file into a document, off the UI thread (SLATE-4): which reader its name
//! picks, what Slate refuses and why, and the pictures decoded for the document.

use crate::view::{texture_key, Textures};
use document_model::{Block, Document, ImageFormat, ImageSource, MarkdownSource};
use gtk::{gdk, glib};
use std::fmt;
use std::path::Path;

/// A document read and ready to show.
pub struct Opened {
    pub doc: Document,
    pub textures: Textures,
    pub words: usize,
    /// A Markdown file's source, so a save rewrites only the blocks edited.
    pub source: Option<MarkdownSource>,
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
/// decode its pictures.
pub fn read(path: &Path) -> Result<Opened, OpenError> {
    let (mut doc, mut source) = match kind(path)? {
        Kind::Text => (document_model::read_text(&std::fs::read(path)?), None),
        Kind::Markdown => {
            let (mut doc, source) = document_model::read_markdown_source(&std::fs::read(path)?);
            if let Some(dir) = path.parent() {
                document_model::load_linked_images(&mut doc.blocks, dir);
            }
            (doc, Some(source))
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
            let doc = docx::read(path).map_err(|e| OpenError(sentence(&e.to_string())))?;
            (doc, None)
        }
    };
    document_edit::normalize(&mut doc);
    // The source describes the blocks as shown: pictures loaded, runs merged.
    if let Some(source) = source.as_mut() {
        source.set_blocks(doc.blocks.clone());
    }
    let mut textures = Textures::new();
    decode_pictures(&doc.blocks, &mut textures);
    let words = doc.word_count();
    Ok(Opened {
        doc,
        textures,
        words,
        source,
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
