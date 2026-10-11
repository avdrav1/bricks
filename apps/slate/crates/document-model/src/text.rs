//! Plain text: decoding and the line-per-paragraph reader (SLATE-1).
//!
//! A byte-order mark decides first: UTF-8 (stripped), UTF-16 LE or BE. Without one, bytes
//! that are valid UTF-8 are UTF-8, and anything else is Windows-1252 (what files called
//! "Latin-1" almost always are; it decodes every byte). CRLF, CR, and LF each end a line.

use crate::{Block, Document, Format, Paragraph, ParagraphStyle, Run, RunStyle};
use encoding_rs::{UTF_16BE, UTF_16LE, WINDOWS_1252};

/// The character encoding a text file was read in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    /// UTF-8 starting with a byte-order mark.
    Utf8Bom,
    /// UTF-16, little-endian, with a byte-order mark.
    Utf16Le,
    /// UTF-16, big-endian, with a byte-order mark.
    Utf16Be,
    Windows1252,
}

impl Encoding {
    pub fn name(self) -> &'static str {
        match self {
            Encoding::Utf8 => "UTF-8",
            Encoding::Utf8Bom => "UTF-8 with BOM",
            Encoding::Utf16Le => "UTF-16 LE",
            Encoding::Utf16Be => "UTF-16 BE",
            Encoding::Windows1252 => "Windows-1252",
        }
    }
}

/// How a text file ends its lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineEnding {
    /// `\n`; also what a file without any line break records.
    #[default]
    Lf,
    CrLf,
    Cr,
}

/// What a text file was, so a save writes it back the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextInfo {
    pub encoding: Encoding,
    /// The file's first line break; a file mixing them records the first.
    pub line_ending: LineEnding,
    /// The last line ends with a line break.
    pub final_newline: bool,
}

/// Decode a text file's bytes: the text, without a byte-order mark, and its encoding.
pub fn decode(bytes: &[u8]) -> (String, Encoding) {
    if let Some(rest) = bytes.strip_prefix(b"\xEF\xBB\xBF") {
        return (
            String::from_utf8_lossy(rest).into_owned(),
            Encoding::Utf8Bom,
        );
    }
    if let Some(rest) = bytes.strip_prefix(b"\xFF\xFE") {
        let (text, _) = UTF_16LE.decode_without_bom_handling(rest);
        return (text.into_owned(), Encoding::Utf16Le);
    }
    if let Some(rest) = bytes.strip_prefix(b"\xFE\xFF") {
        let (text, _) = UTF_16BE.decode_without_bom_handling(rest);
        return (text.into_owned(), Encoding::Utf16Be);
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => (text.to_owned(), Encoding::Utf8),
        Err(_) => {
            let (text, _) = WINDOWS_1252.decode_without_bom_handling(bytes);
            (text.into_owned(), Encoding::Windows1252)
        }
    }
}

/// Split `text` into lines at CRLF, CR, or LF. Returns the lines, the first line break
/// met, and whether the text ends with one (which then ends the last line rather than
/// starting an empty one).
pub(crate) fn split_lines(text: &str) -> (Vec<&str>, LineEnding, bool) {
    let mut lines = Vec::new();
    let mut ending = None;
    let bytes = text.as_bytes();
    let (mut start, mut i) = (0, 0);
    while i < bytes.len() {
        let found = match bytes[i] {
            b'\r' if bytes.get(i + 1) == Some(&b'\n') => Some((LineEnding::CrLf, 2)),
            b'\r' => Some((LineEnding::Cr, 1)),
            b'\n' => Some((LineEnding::Lf, 1)),
            _ => None,
        };
        match found {
            Some((kind, len)) => {
                ending.get_or_insert(kind);
                lines.push(&text[start..i]);
                i += len;
                start = i;
            }
            None => i += 1,
        }
    }
    let final_newline = start == text.len() && !text.is_empty();
    if start < text.len() {
        lines.push(&text[start..]);
    }
    (lines, ending.unwrap_or_default(), final_newline)
}

/// Read a plain-text file's bytes: one normal paragraph per line (an empty line is an
/// empty paragraph), with the encoding and line ending recorded.
pub fn read_text(bytes: &[u8]) -> Document {
    let (text, encoding) = decode(bytes);
    let (lines, line_ending, final_newline) = split_lines(&text);
    let blocks = lines
        .into_iter()
        .map(|line| {
            let mut p = Paragraph::new(ParagraphStyle::Normal);
            if !line.is_empty() {
                p.runs.push(Run {
                    text: line.to_owned(),
                    style: RunStyle::default(),
                });
            }
            Block::Paragraph(p)
        })
        .collect();
    Document {
        blocks,
        format: Format::Text(TextInfo {
            encoding,
            line_ending,
            final_newline,
        }),
    }
}
