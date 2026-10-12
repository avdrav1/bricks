//! Malformed-file check (APP-8). The parser is lenient, as RFC 4180 readers are: a quote
//! inside an unquoted field is text, and text after a closing quote runs on. So a broken
//! file still opens, just wrongly. This pass finds what makes it wrong, with the line and
//! the offending text, so the app can say so:
//!
//! - a quote that opens a field and never closes: everything after it reads as one cell
//!   (it can only be the file's last row, since the quote swallowed the rest);
//! - text right after a closing quote (`"ab"c`), which RFC 4180 forbids;
//! - a zero byte: binary data, not text. The first 64 KiB are checked when the file opens
//!   ([`CsvTable::binary_at_start`]), so a binary file is refused at once.
//!
//! One parallel pass over the rows in file order; rows with no quote or zero byte (found by
//! a byte search over each chunk) are not parsed at all.

use crate::CsvTable;
use csv_engine::RowIndex;
use rayon::prelude::*;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

/// Problems kept with their place; the rest are only counted.
pub const KEPT: usize = 5;
/// File rows per task, and between cancel checks.
const CHUNK: u64 = 65_536;
/// Bytes checked for binary data when a file opens.
pub const BINARY_SAMPLE: usize = 64 * 1024;
/// Longest excerpt shown, in characters.
const EXCERPT: usize = 60;

/// One chunk's first problems (kind, byte offset, file row) and how many it has in all.
type ChunkFound = (Vec<(ProblemKind, u64, u64)>, u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProblemKind {
    UnclosedQuote,
    TextAfterQuote,
    Binary,
}

/// One problem: where it is and what is there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub kind: ProblemKind,
    /// Line in the file, from 1, as a text editor counts.
    pub line: u64,
    /// File row (from 0, the header row included): the grid can go to it.
    pub file_row: u64,
    /// The text from the start of its line, up to a little past the problem, with control
    /// characters made visible (`\0`, `\t`, `\x03`).
    pub excerpt: String,
}

/// What the check found: the first [`KEPT`] problems in file order, and how many in all.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Problems {
    pub first: Vec<Problem>,
    pub total: u64,
}

impl Problems {
    pub fn is_empty(&self) -> bool {
        self.total == 0
    }
}

/// The problems in one row: (kind, byte offset in the file) for each.
fn row_problems(
    bytes: &[u8],
    span: Range<u64>,
    delimiter: u8,
    quote: u8,
    out: &mut Vec<(ProblemKind, u64)>,
) {
    let row = &bytes[span.start as usize..span.end as usize];
    if let Some(i) = memchr::memchr(0, row) {
        out.push((ProblemKind::Binary, span.start + i as u64));
        return;
    }
    let end_of_field = |b: u8| b == delimiter || b == b'\n' || b == b'\r';
    let mut i = 0;
    // A BOM before the first field doesn't stop a quote from opening it.
    if span.start == 0 && row.starts_with(b"\xEF\xBB\xBF") {
        i = 3;
    }
    while i < row.len() {
        if row[i] != quote {
            // Unquoted: a quote inside it is text. On to the next field.
            match memchr::memchr(delimiter, &row[i..]) {
                Some(d) => i += d + 1,
                None => return,
            }
            continue;
        }
        let open = i;
        i += 1;
        loop {
            match memchr::memchr(quote, &row[i..]) {
                None => {
                    out.push((ProblemKind::UnclosedQuote, span.start + open as u64));
                    return;
                }
                Some(q) if row.get(i + q + 1) == Some(&quote) => i += q + 2, // ""
                Some(q) => {
                    i += q + 1;
                    break;
                }
            }
        }
        match row.get(i) {
            None => return,
            Some(&b) if end_of_field(b) => {
                if b != delimiter {
                    return;
                }
                i += 1;
            }
            Some(_) => {
                out.push((ProblemKind::TextAfterQuote, span.start + i as u64));
                // The rest of the field is text; on to the next field.
                match memchr::memchr(delimiter, &row[i..]) {
                    Some(d) => i += d + 1,
                    None => return,
                }
            }
        }
    }
}

/// `bytes[at]`'s line: its 1-based number (counting from `from_line` at `from`), and the
/// text from the line's start to a little past `at`.
fn place(bytes: &[u8], from: usize, from_line: u64, at: usize) -> (u64, String) {
    let line = from_line + memchr::memchr_iter(b'\n', &bytes[from..at]).count() as u64;
    let start = memchr::memrchr(b'\n', &bytes[..at]).map_or(0, |p| p + 1);
    let end = memchr::memchr(b'\n', &bytes[at..]).map_or(bytes.len(), |p| at + p);
    let text = String::from_utf8_lossy(&bytes[start..end]);
    let mut excerpt = String::new();
    for (n, c) in text.chars().enumerate() {
        if n == EXCERPT {
            excerpt.push('…');
            break;
        }
        match c {
            '\0' => excerpt.push_str("\\0"),
            '\t' => excerpt.push_str("\\t"),
            '\r' => {}
            c if c.is_control() => excerpt.push_str(&format!("\\x{:02X}", c as u32)),
            c => excerpt.push(c),
        }
    }
    (line, excerpt)
}

impl CsvTable {
    /// Whether the first [`BINARY_SAMPLE`] bytes of the (decoded) file hold a zero byte:
    /// binary data, not CSV. The problem, placed, if so.
    pub fn binary_at_start(&self) -> Option<Problem> {
        let bytes = self.index.source().bytes();
        let sample = &bytes[..bytes.len().min(BINARY_SAMPLE)];
        let at = memchr::memchr(0, sample)?;
        let (line, excerpt) = place(bytes, 0, 1, at);
        Some(Problem {
            kind: ProblemKind::Binary,
            line,
            file_row: 0,
            excerpt,
        })
    }

    /// Check every row (APP-8); `None` once `cancel` is set or if the file isn't indexed.
    /// Run it on a snapshot on the job pool.
    pub fn check_file(&self, cancel: &AtomicBool) -> Option<Problems> {
        if !self.index.is_complete() {
            return None;
        }
        let bytes = self.index.source().bytes();
        let file_rows = self.index.row_count();
        let (delimiter, quote) = (self.dialect.delimiter, self.dialect.quote);
        let chunks: Option<Vec<ChunkFound>> = (0..file_rows.div_ceil(CHUNK))
            .into_par_iter()
            .map(|c| {
                if cancel.load(Ordering::Relaxed) {
                    return None;
                }
                let rows = c * CHUNK..file_rows.min((c + 1) * CHUNK);
                let mut spans = Vec::new();
                let found = self.index.row_spans(rows.clone(), &mut spans).ok()?;
                if found != (rows.end - rows.start) as usize {
                    return None; // the file changed underneath
                }
                let (start, end) = match (spans.first(), spans.last()) {
                    (Some(a), Some(b)) => (a.start as usize, b.end as usize),
                    _ => return Some((Vec::new(), 0)),
                };
                // Only rows with a quote or a zero byte can have a problem.
                let chunk = &bytes[start..end];
                let mut suspects: Vec<usize> = memchr::memchr2_iter(quote, 0, chunk)
                    .map(|at| spans.partition_point(|s| s.end <= (start + at) as u64))
                    .collect();
                suspects.dedup();
                let (mut kept, mut count, mut found) = (Vec::new(), 0u64, Vec::new());
                for i in suspects.into_iter().filter(|&i| i < spans.len()) {
                    found.clear();
                    row_problems(bytes, spans[i].clone(), delimiter, quote, &mut found);
                    for &(kind, offset) in &found {
                        count += 1;
                        if kept.len() < KEPT {
                            kept.push((kind, offset, rows.start + i as u64));
                        }
                    }
                }
                Some((kept, count))
            })
            .collect();
        let mut problems = Problems::default();
        let mut placed = (0usize, 1u64); // where line counting got to
        for (kept, count) in chunks? {
            problems.total += count;
            for (kind, offset, file_row) in kept {
                if problems.first.len() == KEPT {
                    break;
                }
                let at = offset as usize;
                let (line, excerpt) = place(bytes, placed.0, placed.1, at);
                placed = (
                    memchr::memrchr(b'\n', &bytes[..at]).map_or(0, |p| p + 1),
                    line,
                );
                problems.first.push(Problem {
                    kind,
                    line,
                    file_row,
                    excerpt,
                });
            }
        }
        Some(problems)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problems(row: &[u8]) -> Vec<(ProblemKind, u64)> {
        let mut out = Vec::new();
        row_problems(row, 0..row.len() as u64, b',', b'"', &mut out);

        out
    }

    #[test]
    fn rows_and_their_problems() {
        use ProblemKind::*;
        assert_eq!(problems(b"a,\"b,c\",d\n"), []);
        assert_eq!(problems(b"\"say \"\"hi\"\"\",x\r\n"), [], "escaped quotes");
        assert_eq!(
            problems(b"5\" pipe,x\n"),
            [],
            "a quote inside an unquoted field"
        );
        assert_eq!(problems(b"\"\",\"\"\n"), [], "empty quoted fields");
        assert_eq!(problems(b"\"ab\"c,d\n"), [(TextAfterQuote, 4)]);
        assert_eq!(
            problems(b"x,\"ab\" ,\"c\"d\n"),
            [(TextAfterQuote, 6), (TextAfterQuote, 11)]
        );
        assert_eq!(problems(b"1,\"never closes\n2,3\n"), [(UnclosedQuote, 2)]);
        assert_eq!(problems(b"a,b\0c\n"), [(Binary, 3)]);
        assert_eq!(
            problems(b"\xEF\xBB\xBF\"id\",x\n"),
            [],
            "a BOM before a quoted field"
        );
    }

    #[test]
    fn places_count_lines_and_show_the_line() {
        let text = b"id,name\n1,\"a\nb\"\n2,\"oops\"x,\t\x03\n";
        let at = text.iter().position(|&b| b == b'x').unwrap();
        let (line, excerpt) = place(text, 0, 1, at);
        assert_eq!(line, 4, "the quoted newline counts as a line");
        assert_eq!(excerpt, "2,\"oops\"x,\\t\\x03");
        let long = [b'a'; 100];
        assert_eq!(
            place(&long, 0, 1, 50).1.chars().count(),
            EXCERPT + 1,
            "cut with …"
        );
    }
}
