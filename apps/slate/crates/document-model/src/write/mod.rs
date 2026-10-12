//! Writers (SLATE-7): blocks back into the bytes of a plain-text or Markdown file, in the
//! encoding and line ending the file was read with.
//!
//! Plain text ([`write_text`]) writes each paragraph as a line, as the clipboard's plain
//! text does: a paragraph's own line breaks stay line breaks, a list item is its text
//! without a marker, a code block its lines, a table its cells one a line in row order, a
//! picture its alt text, raw source as written, and a rule or page break nothing (no line).
//! Reading a text file and writing it back gives the same bytes, unless the file mixes
//! line endings (all of them become the first one) or held bytes its encoding couldn't
//! decode.

mod inline;
mod markdown;

pub use markdown::write_markdown;

use crate::{Block, Encoding, LineEnding, TextInfo};
use encoding_rs::{EncoderResult, WINDOWS_1252};
use std::fmt;

/// Text a file's encoding can't hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unencodable {
    pub ch: char,
    pub encoding: Encoding,
}

impl fmt::Display for Unencodable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} has no “{}”", self.encoding.name(), self.ch)
    }
}

impl std::error::Error for Unencodable {}

/// Encode `text` (line breaks already as the file has them) in `encoding`, with the
/// byte-order mark where the encoding has one.
pub fn encode(text: &str, encoding: Encoding) -> Result<Vec<u8>, Unencodable> {
    Ok(match encoding {
        Encoding::Utf8 => text.as_bytes().to_vec(),
        Encoding::Utf8Bom => [b"\xEF\xBB\xBF", text.as_bytes()].concat(),
        Encoding::Utf16Le => {
            let mut out = Vec::with_capacity(2 + text.len() * 2);
            out.extend_from_slice(b"\xFF\xFE");
            text.encode_utf16()
                .for_each(|u| out.extend_from_slice(&u.to_le_bytes()));
            out
        }
        Encoding::Utf16Be => {
            let mut out = Vec::with_capacity(2 + text.len() * 2);
            out.extend_from_slice(b"\xFE\xFF");
            text.encode_utf16()
                .for_each(|u| out.extend_from_slice(&u.to_be_bytes()));
            out
        }
        Encoding::Windows1252 => {
            let mut encoder = WINDOWS_1252.new_encoder();
            // One byte a character, and a character is at least one byte of UTF-8.
            let mut out = Vec::with_capacity(text.len());
            let mut rest = text;
            loop {
                let (result, read) =
                    encoder.encode_from_utf8_to_vec_without_replacement(rest, &mut out, true);
                rest = &rest[read..];
                match result {
                    EncoderResult::InputEmpty => break out,
                    EncoderResult::OutputFull => out.reserve(rest.len().max(16)),
                    EncoderResult::Unmappable(ch) => return Err(Unencodable { ch, encoding }),
                }
            }
        }
    })
}

/// A plain-text file of `blocks`: each paragraph one line (its own `\n` line breaks stay
/// line breaks), joined and ended per `info`: its encoding, line ending, and final newline.
/// Blocks that aren't paragraphs give the lines the clipboard's plain text gives (see the
/// module documentation); a rule or page break gives none.
pub fn write_text(blocks: &[Block], info: &TextInfo) -> Result<Vec<u8>, Unencodable> {
    let mut lines = Vec::with_capacity(blocks.len());
    text_lines(blocks, &mut lines);
    let ending = ending(info.line_ending);
    let mut text = String::with_capacity(lines.iter().map(|l| l.len() + 2).sum());
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            text.push_str(ending);
        }
        push_with_ending(&mut text, line, ending);
    }
    if info.final_newline && !lines.is_empty() {
        text.push_str(ending);
    }
    encode(&text, info.encoding)
}

fn text_lines<'b>(blocks: &'b [Block], lines: &mut Vec<std::borrow::Cow<'b, str>>) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => lines.push(text_of(&p.runs)),
            Block::ListItem(item) => lines.push(text_of(&item.paragraph.runs)),
            Block::CodeBlock(code) => {
                lines.push(code.text.strip_suffix('\n').unwrap_or(&code.text).into())
            }
            Block::Table(table) => {
                for cell in table.rows.iter().flat_map(|r| &r.cells) {
                    if cell.blocks.is_empty() {
                        lines.push("".into());
                    } else {
                        text_lines(&cell.blocks, lines);
                    }
                }
            }
            Block::Image(image) => lines.push(image.alt.as_str().into()),
            Block::Raw(raw) => lines.push(raw.source.trim_end_matches(['\n', '\r']).into()),
            Block::PageBreak | Block::Rule => {}
        }
    }
}

fn text_of(runs: &[crate::Run]) -> std::borrow::Cow<'_, str> {
    match runs {
        [] => "".into(),
        [one] => one.text.as_str().into(),
        _ => runs
            .iter()
            .map(|r| r.text.as_str())
            .collect::<String>()
            .into(),
    }
}

/// The characters of a line ending.
pub(crate) fn ending(line_ending: LineEnding) -> &'static str {
    match line_ending {
        LineEnding::Lf => "\n",
        LineEnding::CrLf => "\r\n",
        LineEnding::Cr => "\r",
    }
}

/// Append `s` to `out` with each of its line breaks (CRLF, CR, or LF) written as `ending`.
pub(crate) fn push_with_ending(out: &mut String, s: &str, ending: &str) {
    if ending == "\n" && !s.contains('\r') {
        out.push_str(s);
        return;
    }
    let mut rest = s;
    while let Some(i) = rest.find(['\n', '\r']) {
        out.push_str(&rest[..i]);
        out.push_str(ending);
        let len = if rest[i..].starts_with("\r\n") { 2 } else { 1 };
        rest = &rest[i + len..];
    }
    out.push_str(rest);
}
