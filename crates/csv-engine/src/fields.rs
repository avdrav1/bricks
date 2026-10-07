//! RFC 4180 field splitting for one row (ENG-3).
//!
//! Uses the same quote rules as the row index, so a row the index produced never splits
//! differently: a quote opens a quoted field only as the field's first byte, `""` inside a
//! quoted field is one literal quote, and after the closing quote the rest of the field is
//! literal up to the next delimiter (`"ab"c` reads as `abc`).
//!
//! [`Field::raw`] keeps the exact bytes (invariant 1: raw values are authoritative, and
//! untouched rows save byte for byte). [`Field::value`] is the cell text a user sees and
//! edits: outer quotes removed and `""` unescaped, borrowed whenever nothing needs
//! unescaping.

use crate::Dialect;
use std::borrow::Cow;
use std::ops::Range;

/// One field of a row: its byte range inside the row slice it was split from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// Raw bytes, quotes and escapes included, line terminator excluded.
    pub raw: Range<u32>,
    /// The field starts with the dialect's quote character.
    pub quoted: bool,
}

impl Field {
    /// The field's raw bytes inside `row`.
    pub fn raw<'a>(&self, row: &'a [u8]) -> &'a [u8] {
        &row[self.raw.start as usize..self.raw.end as usize]
    }

    /// The cell text: outer quotes removed and `""` unescaped. An unterminated quoted
    /// field (malformed input at end of file) yields everything after the opening quote.
    pub fn value<'a>(&self, row: &'a [u8], quote: u8) -> Cow<'a, [u8]> {
        let raw = self.raw(row);
        if !self.quoted {
            return Cow::Borrowed(raw);
        }
        let body = &raw[1..];
        match memchr::memchr(quote, body) {
            None => return Cow::Borrowed(body),
            Some(k) if k + 1 == body.len() => return Cow::Borrowed(&body[..k]),
            Some(_) => {}
        }
        let mut v = Vec::with_capacity(body.len());
        let (mut i, mut in_quotes) = (0, true);
        while i < body.len() {
            let b = body[i];
            if b == quote && in_quotes {
                if body.get(i + 1) == Some(&quote) {
                    v.push(quote);
                    i += 2;
                    continue;
                }
                in_quotes = false;
            } else {
                v.push(b);
            }
            i += 1;
        }
        Cow::Owned(v)
    }
}

/// Append `value` to `out` as one CSV field: as-is when it is safe bare, otherwise quoted
/// with inner quotes doubled. Quoted when it holds the delimiter, the quote character, a
/// line break, or starts with the quote character (so it reads back as the same text).
pub fn encode_field(value: &[u8], dialect: &Dialect, out: &mut Vec<u8>) {
    let (delim, quote) = (dialect.delimiter, dialect.quote);
    if !value
        .iter()
        .any(|&b| b == delim || b == quote || b == b'\n' || b == b'\r')
    {
        out.extend_from_slice(value);
        return;
    }
    out.push(quote);
    for &b in value {
        if b == quote {
            out.push(quote);
        }
        out.push(b);
    }
    out.push(quote);
}

/// Split one row (as returned by [`crate::RowIndex::row_span`], terminator included) into
/// fields, written to `out` (cleared first, capacity reused). A row always has at least one
/// field; a blank line is one empty field.
pub fn split_fields(row: &[u8], dialect: &Dialect, out: &mut Vec<Field>) {
    out.clear();
    let (delim, quote) = (dialect.delimiter, dialect.quote);
    let mut start = 0usize;
    let mut quoted = row.first() == Some(&quote);
    let (mut in_quotes, mut pos) = (quoted, usize::from(quoted));
    let content_end = loop {
        if in_quotes {
            match memchr::memchr(quote, &row[pos..]) {
                None => break row.len(),
                Some(o) => {
                    let p = pos + o;
                    if row.get(p + 1) == Some(&quote) {
                        pos = p + 2;
                    } else {
                        in_quotes = false;
                        pos = p + 1;
                    }
                }
            }
        } else {
            match memchr::memchr2(delim, b'\n', &row[pos..]) {
                None => break row.len(),
                Some(o) => {
                    let p = pos + o;
                    if row[p] == b'\n' {
                        // An unquoted newline only ever ends the row.
                        break if p > start && row[p - 1] == b'\r' {
                            p - 1
                        } else {
                            p
                        };
                    }
                    out.push(Field {
                        raw: start as u32..p as u32,
                        quoted,
                    });
                    start = p + 1;
                    quoted = row.get(start) == Some(&quote);
                    in_quotes = quoted;
                    pos = start + usize::from(quoted);
                }
            }
        }
    };
    out.push(Field {
        raw: start as u32..content_end as u32,
        quoted,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempFile;
    use crate::{RowIndex, Source, SparseRowIndex};
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    fn values(row: &[u8], dialect: &Dialect) -> Vec<String> {
        let mut out = Vec::new();
        split_fields(row, dialect, &mut out);
        out.iter()
            .map(|f| String::from_utf8_lossy(&f.value(row, dialect.quote)).into_owned())
            .collect()
    }

    #[test]
    fn splits_and_unescapes_rfc_4180_fields() {
        let d = Dialect::default();
        let cases: &[(&[u8], &[&str])] = &[
            (b"\n", &[""]),
            (b"\r\n", &[""]),
            (b"a", &["a"]),
            (b"a,b\n", &["a", "b"]),
            (b"a,b\r\n", &["a", "b"]),
            (b",,\n", &["", "", ""]),
            (b"\"\",x\n", &["", "x"]),
            (b"1,\"line one\nline two\"\n", &["1", "line one\nline two"]),
            (b"\"a,b\",c\r\n", &["a,b", "c"]),
            (b"\"has \"\"quotes\"\"\"\n", &["has \"quotes\""]),
            (b"\"\"\"\"\n", &["\""]),
            (b"\"ab\"c,d\n", &["abc", "d"]),
            (b"\"a\" \"b\",x\n", &["a \"b\"", "x"]),
            (b"5\" pipe,x\n", &["5\" pipe", "x"]),
            (b"\"ends\r\"\n", &["ends\r"]),
            (b"\"unterminated\nat eof", &["unterminated\nat eof"]),
            (b"00123,  spaced  \n", &["00123", "  spaced  "]),
        ];
        for (row, want) in cases {
            assert_eq!(
                values(row, &d),
                *want,
                "row {:?}",
                String::from_utf8_lossy(row)
            );
        }
    }

    #[test]
    fn raw_spans_cover_the_row_exactly() {
        let d = Dialect::default();
        let row = b"\"a,\"\"b\",c,\"d\ne\"\r\n";
        let mut out = Vec::new();
        split_fields(row, &d, &mut out);
        let raws: Vec<&[u8]> = out.iter().map(|f| f.raw(row)).collect();
        assert_eq!(raws, [&b"\"a,\"\"b\""[..], b"c", b"\"d\ne\""]);
        assert!(
            matches!(out[1].value(row, b'"'), Cow::Borrowed(_)),
            "unquoted value borrows"
        );
        assert!(
            matches!(out[2].value(row, b'"'), Cow::Borrowed(_)),
            "quoted without escapes borrows"
        );
    }

    #[test]
    fn encoded_fields_split_back_to_the_same_value() {
        let d = Dialect::default();
        for v in [
            "",
            "plain",
            "00123",
            "a,b",
            "say \"hi\"",
            "\"",
            "two\nlines",
            "cr\r",
            "  pad  ",
            "5\" pipe",
        ] {
            let mut row = Vec::new();
            encode_field(v.as_bytes(), &d, &mut row);
            row.extend_from_slice(b",end\n");
            assert_eq!(
                values(&row, &d),
                [v, "end"],
                "value {v:?} encoded as {:?}",
                String::from_utf8_lossy(&row)
            );
        }
        let mut bare = Vec::new();
        encode_field(b"00123", &d, &mut bare);
        assert_eq!(bare, b"00123", "safe values are written bare");
    }

    #[test]
    fn other_delimiters() {
        let semi = Dialect {
            delimiter: b';',
            ..Dialect::default()
        };
        assert_eq!(values(b"1,5;\"x;y\";3\n", &semi), ["1,5", "x;y", "3"]);
        let tab = Dialect {
            delimiter: b'\t',
            ..Dialect::default()
        };
        assert_eq!(values(b"a\t\"b\tc\"\n", &tab), ["a", "b\tc"]);
    }

    /// Character-level RFC 4180 parser over a whole file: the reference for index + split.
    fn oracle(bytes: &[u8], delim: u8) -> Vec<Vec<Vec<u8>>> {
        #[derive(Clone, Copy, PartialEq)]
        enum S {
            Start,
            Unquoted,
            Quoted,
            AfterQuote,
        }
        let (mut rows, mut row, mut field, mut s) = (Vec::new(), Vec::new(), Vec::new(), S::Start);
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            match (s, b) {
                (S::Quoted, b'"') => s = S::AfterQuote,
                (S::Quoted, _) => field.push(b),
                (S::AfterQuote, b'"') => {
                    field.push(b'"');
                    s = S::Quoted;
                }
                (_, b'\n') => {
                    if s != S::AfterQuote && field.last() == Some(&b'\r') {
                        field.pop();
                    }
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                    s = S::Start;
                }
                (_, d) if d == delim => {
                    row.push(std::mem::take(&mut field));
                    s = S::Start;
                }
                (S::Start, b'"') => s = S::Quoted,
                _ => {
                    field.push(b);
                    s = S::Unquoted;
                }
            }
            i += 1;
        }
        if s != S::Start || !field.is_empty() || !row.is_empty() {
            row.push(field);
            rows.push(row);
        }
        rows
    }

    fn splitmix(mut x: u64) -> u64 {
        x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
        x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        x ^ (x >> 31)
    }

    fn random_csv(seed: u64, delim: u8) -> Vec<u8> {
        let mut x = seed;
        let mut next = |n: u64| {
            x = splitmix(x);
            x % n
        };
        let mut out = Vec::new();
        for _ in 0..1 + next(300) {
            for f in 0..1 + next(6) {
                if f > 0 {
                    out.push(delim);
                }
                let quoted = next(3) == 0;
                if quoted {
                    out.push(b'"');
                }
                for _ in 0..next(9) {
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
            out.pop();
        }
        out
    }

    #[test]
    fn index_plus_split_matches_whole_file_oracle() {
        let mut fields = Vec::new();
        for seed in 0..300 {
            let delim = if seed % 4 == 0 { b';' } else { b',' };
            let dialect = Dialect {
                delimiter: delim,
                ..Dialect::default()
            };
            let bytes = random_csv(seed, delim);
            let file = TempFile::new(&bytes);
            let idx = SparseRowIndex::new(Arc::new(Source::open(file.path()).unwrap()), &dialect);
            idx.build(&AtomicBool::new(false)).unwrap();
            let got: Vec<Vec<Vec<u8>>> = (0..idx.row_count())
                .map(|r| {
                    let row = idx.row_fields(r, &mut fields).unwrap();
                    fields
                        .iter()
                        .map(|f| f.value(row, b'"').into_owned())
                        .collect()
                })
                .collect();
            assert_eq!(got, oracle(&bytes, delim), "seed {seed}");
        }
    }
}
