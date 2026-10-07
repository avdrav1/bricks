//! Sparse, quote-aware row index over a [`Source`] (ADR 0002).
//!
//! Stores the byte offset of every [`STRIDE`]th row start. A row's span is found by
//! scanning forward from the nearest checkpoint, at most `STRIDE - 1` rows.
//!
//! Row boundaries follow RFC 4180: a quote opens a quoted field only at the start of a
//! field, `""` inside a quoted field is an escaped quote, and newlines inside quoted
//! fields do not end the row. A quote in the middle of an unquoted field (`5" pipe`) is
//! literal. `\r\n` and `\n` both end rows; the span includes the terminator so saves can
//! copy untouched rows byte for byte.

use crate::fields::{split_fields, Field};
use crate::source::Source;
use crate::{Dialect, RowIndex};
use parking_lot::RwLock;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

/// Rows between checkpoints. 64 costs about 4 MiB per 1 GB of narrow CSV (ADR 0002).
pub const STRIDE: u64 = 64;

/// Bytes scanned between cancel checks and progress publication (ADR 0005).
const CHUNK: usize = 4 << 20;

const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexError {
    /// The cancel flag was set; rows indexed so far stay readable.
    Cancelled,
    /// The file was truncated or replaced in place while mapped.
    FileChanged,
}

impl std::fmt::Display for IndexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Cancelled => "indexing was cancelled",
            Self::FileChanged => "the file changed on disk while it was open",
        })
    }
}

impl std::error::Error for IndexError {}

/// Quote-aware row splitter state.
#[derive(Clone, Copy)]
struct Scanner {
    delimiter: u8,
    quote: u8,
    in_quotes: bool,
}

impl Scanner {
    fn new(dialect: &Dialect) -> Self {
        Self {
            delimiter: dialect.delimiter,
            quote: dialect.quote,
            in_quotes: false,
        }
    }

    /// Find the next row-ending `\n` at or after `*pos` and before `limit`.
    ///
    /// On a hit, returns its offset and leaves `*pos` just past it. On a miss, leaves `*pos`
    /// at `limit` (or `limit + 1` when an escaped `""` straddles it) so scanning can resume.
    fn next_row_end(&mut self, bytes: &[u8], pos: &mut usize, limit: usize) -> Option<usize> {
        while *pos < limit {
            let hay = &bytes[*pos..limit];
            let hit = if self.in_quotes {
                memchr::memchr(self.quote, hay)
            } else {
                memchr::memchr2(self.quote, b'\n', hay)
            };
            let Some(off) = hit else {
                *pos = limit;
                return None;
            };
            let p = *pos + off;
            if bytes[p] == b'\n' {
                *pos = p + 1;
                return Some(p);
            }
            if self.in_quotes {
                if bytes.get(p + 1) == Some(&self.quote) {
                    *pos = p + 2; // escaped quote
                } else {
                    self.in_quotes = false;
                    *pos = p + 1;
                }
            } else {
                self.in_quotes = self.at_field_start(bytes, p);
                *pos = p + 1;
            }
        }
        None
    }

    fn at_field_start(&self, bytes: &[u8], p: usize) -> bool {
        p == 0
            || matches!(bytes[p - 1], b'\n')
            || bytes[p - 1] == self.delimiter
            || (p == UTF8_BOM.len() && bytes.starts_with(UTF8_BOM))
    }
}

/// The rows of `bytes`, terminators included, split with the index's quote rules. The last
/// row may lack a terminator. Used where a whole index is not needed (dialect detection).
pub(crate) fn rows<'a>(bytes: &'a [u8], dialect: &Dialect) -> impl Iterator<Item = &'a [u8]> + 'a {
    let mut scanner = Scanner::new(dialect);
    let mut pos = 0;
    std::iter::from_fn(move || {
        if pos >= bytes.len() {
            return None;
        }
        let start = pos;
        let end = scanner
            .next_row_end(bytes, &mut pos, bytes.len())
            .map_or(bytes.len(), |nl| nl + 1);
        pos = end;
        Some(&bytes[start..end])
    })
}

/// Row index that fills in while [`SparseRowIndex::build`] runs on a worker thread.
/// Readers on other threads see every row counted by [`RowIndex::row_count`].
pub struct SparseRowIndex {
    source: Arc<Source>,
    dialect: Dialect,
    checkpoints: RwLock<Vec<u64>>,
    rows: AtomicU64,
    bytes_done: AtomicU64,
    complete: AtomicBool,
}

impl SparseRowIndex {
    /// An empty index over `source`. Call [`Self::build`] (usually on a worker) to fill it.
    pub fn new(source: Arc<Source>, dialect: &Dialect) -> Arc<Self> {
        Arc::new(Self {
            source,
            dialect: *dialect,
            checkpoints: RwLock::new(vec![0]),
            rows: AtomicU64::new(0),
            bytes_done: AtomicU64::new(0),
            complete: AtomicBool::new(false),
        })
    }

    /// Scan the whole source, publishing rows every 4 MiB and checking `cancel` as often.
    /// Call once.
    pub fn build(&self, cancel: &AtomicBool) -> Result<(), IndexError> {
        let bytes = self.source.bytes();
        let len = bytes.len();
        let mut scanner = Scanner::new(&self.dialect);
        let (mut pos, mut rows, mut row_start) = (0usize, 0u64, 0usize);
        let mut pending = Vec::new();
        while pos < len {
            if cancel.load(Ordering::Relaxed) {
                return Err(IndexError::Cancelled);
            }
            let limit = (pos + CHUNK).min(len);
            while let Some(nl) = scanner.next_row_end(bytes, &mut pos, limit) {
                rows += 1;
                row_start = nl + 1;
                if rows % STRIDE == 0 && row_start < len {
                    pending.push(row_start as u64);
                }
            }
            if self.source.changed() {
                return Err(IndexError::FileChanged);
            }
            self.publish(&mut pending, rows, pos.min(len));
        }
        if row_start < len {
            rows += 1; // last row has no terminator
        }
        self.publish(&mut pending, rows, len);
        self.complete.store(true, Ordering::Release);
        Ok(())
    }

    fn publish(&self, pending: &mut Vec<u64>, rows: u64, bytes_done: usize) {
        if !pending.is_empty() {
            self.checkpoints.write().append(pending);
        }
        self.bytes_done.store(bytes_done as u64, Ordering::Relaxed);
        self.rows.store(rows, Ordering::Release);
    }

    /// Bytes scanned so far, for progress bars.
    pub fn bytes_indexed(&self) -> u64 {
        self.bytes_done.load(Ordering::Relaxed)
    }

    /// Heap held by the index itself (not the mapped file).
    pub fn heap_bytes(&self) -> usize {
        self.checkpoints.read().capacity() * std::mem::size_of::<u64>()
    }

    pub fn source(&self) -> &Arc<Source> {
        &self.source
    }

    /// Split a row into fields (ENG-3). Returns the row's bytes, which the field ranges in
    /// `out` index into; a UTF-8 BOM before the first row is not part of its first field.
    /// `None` past `row_count()` or once the file changed on disk.
    pub fn row_fields(&self, row: u64, out: &mut Vec<Field>) -> Option<&[u8]> {
        let span = self.row_span(row)?;
        let bytes = &self.source.bytes()[span.start as usize..span.end as usize];
        let bytes = if span.start == 0 {
            bytes.strip_prefix(UTF8_BOM).unwrap_or(bytes)
        } else {
            bytes
        };
        split_fields(bytes, &self.dialect, out);
        (!self.source.changed()).then_some(bytes)
    }
}

impl RowIndex for SparseRowIndex {
    fn row_count(&self) -> u64 {
        self.rows.load(Ordering::Acquire)
    }

    fn is_complete(&self) -> bool {
        self.complete.load(Ordering::Acquire)
    }

    fn row_span(&self, row: u64) -> Option<Range<u64>> {
        if row >= self.row_count() || self.source.changed() {
            return None;
        }
        let (mut scanner, mut pos) = self.seek(row)?;
        Some(self.next_span(&mut scanner, &mut pos))
    }

    fn row_spans(&self, rows: Range<u64>, out: &mut Vec<Range<u64>>) -> Result<usize, IndexError> {
        out.clear();
        let end = rows.end.min(self.row_count());
        if rows.start < end {
            let (mut scanner, mut pos) = self.seek(rows.start).ok_or(IndexError::FileChanged)?;
            for _ in rows.start..end {
                out.push(self.next_span(&mut scanner, &mut pos));
            }
        }
        if self.source.changed() {
            out.clear();
            return Err(IndexError::FileChanged);
        }
        Ok(out.len())
    }
}

impl SparseRowIndex {
    /// Scanner positioned at the start of `row`, reached from the nearest checkpoint.
    fn seek(&self, row: u64) -> Option<(Scanner, usize)> {
        let mut pos = self.checkpoints.read()[(row / STRIDE) as usize] as usize;
        let bytes = self.source.bytes();
        let mut scanner = Scanner::new(&self.dialect);
        for _ in 0..row % STRIDE {
            scanner.next_row_end(bytes, &mut pos, bytes.len())?;
        }
        Some((scanner, pos))
    }

    /// Span of the row starting at `*pos`; leaves `*pos` at the next row.
    fn next_span(&self, scanner: &mut Scanner, pos: &mut usize) -> Range<u64> {
        let bytes = self.source.bytes();
        let start = *pos;
        let end = scanner
            .next_row_end(bytes, pos, bytes.len())
            .map_or(bytes.len(), |nl| nl + 1);
        *pos = end;
        start as u64..end as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempFile;

    fn index(content: &[u8], dialect: &Dialect) -> (TempFile, Arc<SparseRowIndex>) {
        let file = TempFile::new(content);
        let source = Arc::new(Source::open(file.path()).unwrap());
        let index = SparseRowIndex::new(source, dialect);
        index.build(&AtomicBool::new(false)).unwrap();
        (file, index)
    }

    fn rows_of(content: &[u8]) -> Vec<String> {
        let (_f, idx) = index(content, &Dialect::default());
        assert!(idx.is_complete());
        (0..idx.row_count())
            .map(|r| {
                let s = idx.row_span(r).unwrap();
                String::from_utf8_lossy(&content[s.start as usize..s.end as usize]).into_owned()
            })
            .collect()
    }

    #[test]
    fn row_boundaries_follow_rfc_4180() {
        let cases: &[(&[u8], &[&str])] = &[
            (b"", &[]),
            (b"a", &["a"]),
            (b"a\n", &["a\n"]),
            (b"\n", &["\n"]),
            (b"a,b\nc,d", &["a,b\n", "c,d"]),
            (b"a,b\r\n1,2\r\n", &["a,b\r\n", "1,2\r\n"]),
            (
                b"id,note\n1,\"line one\nline two\"\n2,x\n",
                &["id,note\n", "1,\"line one\nline two\"\n", "2,x\n"],
            ),
            (
                b"1,\"has \"\"quotes\"\"\nstill\"\n2\n",
                &["1,\"has \"\"quotes\"\"\nstill\"\n", "2\n"],
            ),
            (b"1,\"\"\"\n\"\n2\n", &["1,\"\"\"\n\"\n", "2\n"]),
            (b"5\" pipe,x\ny\n", &["5\" pipe,x\n", "y\n"]),
            (b"a\n\"b\nc", &["a\n", "\"b\nc"]),
            (
                b"\xEF\xBB\xBF\"a\nb\",c\nd\n",
                &["\u{feff}\"a\nb\",c\n", "d\n"],
            ),
        ];
        for (input, want) in cases {
            assert_eq!(
                rows_of(input),
                *want,
                "input {:?}",
                String::from_utf8_lossy(input)
            );
        }
    }

    #[test]
    fn delimiter_sets_where_quotes_open() {
        let tab = Dialect {
            delimiter: b'\t',
            ..Dialect::default()
        };
        let (_f, idx) = index(b"a\t\"x\ny\"\n,\"p\nq\n", &tab);
        // After a tab the quote opens a field; after a comma (not the delimiter) it is literal.
        assert_eq!(idx.row_count(), 3);
        assert_eq!(idx.row_span(1), Some(8..12));
    }

    /// Character-level RFC 4180 state machine: the reference the fast scanner must match.
    fn oracle(bytes: &[u8], delim: u8) -> Vec<Range<u64>> {
        #[derive(PartialEq)]
        enum S {
            FieldStart,
            Unquoted,
            Quoted,
            QuoteInQuoted,
        }
        let (mut s, mut start, mut out) = (S::FieldStart, 0usize, Vec::new());
        for (i, &b) in bytes.iter().enumerate() {
            let bom_end = i == 3 && bytes.starts_with(UTF8_BOM);
            s = match (s, b) {
                (S::Quoted, b'"') => S::QuoteInQuoted,
                (S::Quoted, _) => S::Quoted,
                (S::QuoteInQuoted, b'"') => S::Quoted,
                (_, b'\n') => {
                    out.push(start as u64..i as u64 + 1);
                    start = i + 1;
                    S::FieldStart
                }
                (_, d) if d == delim => S::FieldStart,
                (S::FieldStart, b'"') => S::Quoted,
                (S::Unquoted, b'"') if bom_end => S::Quoted,
                _ => S::Unquoted,
            };
        }
        if start < bytes.len() {
            out.push(start as u64..bytes.len() as u64);
        }
        out
    }

    fn splitmix(mut x: u64) -> u64 {
        x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
        x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        x ^ (x >> 31)
    }

    /// Random CSV-ish bytes heavy in quotes, delimiters and line breaks, including
    /// malformed input (stray and unbalanced quotes).
    fn random_csv(seed: u64, delim: u8) -> Vec<u8> {
        let mut x = seed;
        let mut next = |n: u64| {
            x = splitmix(x);
            x % n
        };
        let rows = 1 + next(400);
        let mut out = Vec::new();
        if next(10) == 0 {
            out.extend_from_slice(UTF8_BOM);
        }
        for _ in 0..rows {
            let fields = 1 + next(5);
            for f in 0..fields {
                if f > 0 {
                    out.push(delim);
                }
                let quoted = next(3) == 0;
                if quoted {
                    out.push(b'"');
                }
                for _ in 0..next(8) {
                    out.push(match next(12) {
                        0 => b'"',
                        1 => b'\n',
                        2 => b'\r',
                        3 => delim,
                        4 => b',',
                        _ => b'a' + next(26) as u8,
                    });
                }
                if quoted && next(8) != 0 {
                    out.push(b'"');
                }
            }
            out.extend_from_slice(if next(4) == 0 { b"\r\n" } else { b"\n" });
        }
        if next(3) == 0 {
            out.pop(); // no trailing newline
        }
        out
    }

    #[test]
    fn matches_rfc_4180_oracle_on_random_input() {
        for seed in 0..300 {
            let delim = if seed % 3 == 0 { b'\t' } else { b',' };
            let bytes = random_csv(seed, delim);
            let dialect = Dialect {
                delimiter: delim,
                ..Dialect::default()
            };
            let (_f, idx) = index(&bytes, &dialect);
            let want = oracle(&bytes, delim);
            let got: Vec<_> = (0..idx.row_count())
                .map(|r| idx.row_span(r).unwrap())
                .collect();
            assert_eq!(got, want, "seed {seed}");
            assert_eq!(idx.row_span(idx.row_count()), None);
        }
    }

    #[test]
    fn cancel_stops_before_the_end_and_keeps_rows_readable() {
        let content: Vec<u8> = (0..400_000)
            .flat_map(|i| format!("{i},x\n").into_bytes())
            .collect();
        let file = TempFile::new(&content);
        let idx = SparseRowIndex::new(
            Arc::new(Source::open(file.path()).unwrap()),
            &Dialect::default(),
        );
        assert_eq!(
            idx.build(&AtomicBool::new(true)),
            Err(IndexError::Cancelled)
        );
        assert!(!idx.is_complete());
        assert!(idx.row_count() < 400_000);
    }

    #[test]
    fn rows_are_readable_while_indexing_streams() {
        // ~30 MB so the build publishes several 4 MiB chunks.
        let content: Vec<u8> = (0..2_000_000u32)
            .flat_map(|i| format!("{i},\"q\nq\",z\n").into_bytes())
            .collect();
        let file = TempFile::new(&content);
        let idx = SparseRowIndex::new(
            Arc::new(Source::open(file.path()).unwrap()),
            &Dialect::default(),
        );
        let worker = {
            let idx = idx.clone();
            std::thread::spawn(move || idx.build(&AtomicBool::new(false)))
        };
        let mut seen_partial = false;
        while !idx.is_complete() {
            let n = idx.row_count();
            seen_partial |= n > 0 && n < 2_000_000;
            if n > 0 {
                let r = n - 1;
                let span = idx.row_span(r).unwrap();
                let row = &content[span.start as usize..span.end as usize];
                assert_eq!(row, format!("{r},\"q\nq\",z\n").as_bytes());
            }
        }
        worker.join().unwrap().unwrap();
        assert_eq!(idx.row_count(), 2_000_000);
        assert_eq!(idx.bytes_indexed(), content.len() as u64);
        // The partial view is timing-dependent; only report it.
        eprintln!("observed a partial index while building: {seen_partial}");
    }

    #[test]
    fn row_spans_match_single_row_lookups_for_any_range() {
        let mut out = Vec::new();
        for seed in 0..60 {
            let bytes = random_csv(seed, b',');
            let (_f, idx) = index(&bytes, &Dialect::default());
            let n = idx.row_count();
            for k in 0..20 {
                let a = splitmix(seed * 100 + k) % (n + 1);
                let b = a + splitmix(seed * 100 + k + 50) % 150; // may run past the end
                let got = idx.row_spans(a..b, &mut out).unwrap();
                let want: Vec<_> = (a..b.min(n)).map(|r| idx.row_span(r).unwrap()).collect();
                assert_eq!(got, want.len(), "seed {seed} range {a}..{b}");
                assert_eq!(out, want, "seed {seed} range {a}..{b}");
            }
        }
    }

    #[test]
    fn row_spans_report_a_file_changed_underneath() {
        let content: Vec<u8> = (0..100_000)
            .flat_map(|i| format!("{i},x\n").into_bytes())
            .collect();
        let file = TempFile::new(&content);
        let idx = SparseRowIndex::new(
            Arc::new(Source::open(file.path()).unwrap()),
            &Dialect::default(),
        );
        idx.build(&AtomicBool::new(false)).unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(file.path())
            .unwrap()
            .set_len(0)
            .unwrap();
        let mut out = Vec::new();
        assert_eq!(
            idx.row_spans(99_000..99_100, &mut out),
            Err(IndexError::FileChanged)
        );
        assert!(out.is_empty());
    }

    #[test]
    fn index_is_one_u64_per_stride_rows() {
        let content: Vec<u8> = (0..64_000)
            .flat_map(|i| format!("{i}\n").into_bytes())
            .collect();
        let (_f, idx) = index(&content, &Dialect::default());
        assert_eq!(idx.row_count(), 64_000);
        assert!(idx.heap_bytes() <= 2 * (64_000 / STRIDE as usize + 1) * 8);
    }
}
