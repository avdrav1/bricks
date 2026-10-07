//! Dialect detection (ENG-4): which of comma, tab, semicolon, or pipe separates fields;
//! and whether the first row is a header (ENG-7, [`detect_header`]).
//!
//! For each candidate the sample is split into rows and fields with the same quote rules the
//! index uses, and scored by how consistently rows have the same number of fields. A
//! delimiter is a contender when at least [`MIN_SHARE`] of the rows agree on two or more
//! fields; among contenders the one giving the most fields wins. This handles the common
//! trap of another candidate appearing a fixed number of times per row, such as the comma in
//! `48.86, 2.29` coordinates inside a semicolon file. Lines starting with `#` are skipped
//! while scoring (comment headers in TSV-style data files); they stay ordinary rows.

use crate::fields::split_fields;
use crate::index::rows;
use crate::{Dialect, Field, LineEnding, CANDIDATE_DELIMITERS};

/// Bytes from the start of a file that detection looks at.
pub const DETECT_SAMPLE_BYTES: usize = 64 * 1024;
/// Rows scored per candidate.
const MAX_ROWS: usize = 200;
/// Share of rows that must agree on one field count.
const MIN_SHARE: f64 = 0.9;
/// Rows below the first that header detection looks at.
const HEADER_ROWS: usize = 50;

const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// Guess the dialect from the first bytes of a file (up to [`DETECT_SAMPLE_BYTES`]). Falls
/// back to comma when no candidate splits rows consistently (e.g. one-column files).
pub fn detect_dialect(sample: &[u8]) -> Dialect {
    let body = sample.strip_prefix(UTF8_BOM).unwrap_or(sample);
    let line_ending = match memchr::memchr(b'\n', body) {
        Some(i) if i > 0 && body[i - 1] == b'\r' => LineEnding::CrLf,
        _ => LineEnding::Lf,
    };
    let base = Dialect {
        line_ending,
        ..Dialect::default()
    };
    let mut fields = Vec::new();
    let mut best: Option<(u32, f64, u8)> = None;
    for &delimiter in &CANDIDATE_DELIMITERS {
        let dialect = Dialect { delimiter, ..base };
        let (share, width) = consistency(body, &dialect, &mut fields);
        if width < 2 || share < MIN_SHARE {
            continue;
        }
        // Wider wins; ties go to the more consistent, then to candidate order.
        let better = match best {
            None => true,
            Some((w, s, _)) => width > w || (width == w && share > s),
        };
        if better {
            best = Some((width, share, delimiter));
        }
    }
    let dialect = best.map_or(base, |(_, _, delimiter)| Dialect { delimiter, ..base });
    Dialect {
        has_header: detect_header(body, &dialect),
        ..dialect
    }
}

/// Whether the first row names the columns (ENG-7), in the spirit of Python's
/// `csv.Sniffer.has_header`. Each column whose values below the first row are all numbers,
/// or all the same length, votes: a first cell that doesn't fit (text above numbers, a
/// different length) says header, one that fits says data. Columns of free text don't vote,
/// and a tie means header: most CSV files have one, and the user can flip it. A file with
/// no second row, or a `#` comment first, has no header.
pub fn detect_header(sample: &[u8], dialect: &Dialect) -> bool {
    let body = sample.strip_prefix(UTF8_BOM).unwrap_or(sample);
    let mut it = rows(body, dialect).peekable();
    let Some(first) = it.next() else {
        return false;
    };
    if first.starts_with(b"#") {
        return false;
    }
    let mut fields = Vec::new();
    split_fields(first, dialect, &mut fields);
    let header: Vec<Vec<u8>> = fields
        .iter()
        .map(|f| f.value(first, dialect.quote).into_owned())
        .collect();
    let mut columns = vec![ColumnShape::default(); header.len()];
    let mut data_rows = 0;
    while let Some(row) = it.next() {
        // A sample cut mid-row leaves a partial last row; don't count it.
        if it.peek().is_none() && !row.ends_with(b"\n") && data_rows > 0 {
            break;
        }
        if row.iter().all(|b| b.is_ascii_whitespace()) {
            continue;
        }
        split_fields(row, dialect, &mut fields);
        for (column, f) in columns.iter_mut().zip(&fields) {
            column.see(&f.value(row, dialect.quote));
        }
        data_rows += 1;
        if data_rows == HEADER_ROWS {
            break;
        }
    }
    data_rows > 0
        && columns
            .iter()
            .zip(&header)
            .map(|(c, h)| c.vote(h))
            .sum::<i32>()
            >= 0
}

/// What a column's values below the first row have in common.
#[derive(Clone, Copy)]
struct ColumnShape {
    /// Non-empty values seen.
    seen: u32,
    /// Every one of them is a number.
    numbers: bool,
    /// Their common length in characters, while they all have the same one.
    len: Option<usize>,
}

impl Default for ColumnShape {
    fn default() -> Self {
        Self {
            seen: 0,
            numbers: true,
            len: None,
        }
    }
}

impl ColumnShape {
    fn see(&mut self, value: &[u8]) {
        let v = trim(value);
        if v.is_empty() {
            return;
        }
        self.numbers &= is_number(v);
        let n = char_len(v);
        self.len = match (self.seen, self.len) {
            (0, _) => Some(n),
            (_, Some(len)) if len == n => Some(len),
            _ => None,
        };
        self.seen += 1;
    }

    /// +1 if `first` stands out from the column (a header), -1 if it fits (data), 0 if the
    /// column has no shape to judge by.
    fn vote(&self, first: &[u8]) -> i32 {
        let first = trim(first);
        if self.seen == 0 {
            0
        } else if self.numbers {
            if is_number(first) {
                -1
            } else {
                1
            }
        } else {
            match self.len {
                Some(len) if self.seen >= 2 => {
                    if char_len(first) == len {
                        -1
                    } else {
                        1
                    }
                }
                _ => 0,
            }
        }
    }
}

fn trim(v: &[u8]) -> &[u8] {
    let start = v
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(v.len());
    let end = v
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(start, |i| i + 1);
    &v[start..end]
}

fn is_number(v: &[u8]) -> bool {
    std::str::from_utf8(v)
        .is_ok_and(|s| s.bytes().any(|b| b.is_ascii_digit()) && s.parse::<f64>().is_ok())
}

fn char_len(v: &[u8]) -> usize {
    std::str::from_utf8(v).map_or(v.len(), |s| s.chars().count())
}

/// The most common field count among scored rows, and the share of rows that have it.
fn consistency(body: &[u8], dialect: &Dialect, fields: &mut Vec<Field>) -> (f64, u32) {
    let mut counts: Vec<(u32, u32)> = Vec::new();
    let mut scored = 0u32;
    let mut it = rows(body, dialect).peekable();
    while let Some(row) = it.next() {
        // A sample cut mid-row leaves a partial last row; don't score it.
        if it.peek().is_none() && !row.ends_with(b"\n") && scored > 0 {
            break;
        }
        if row.starts_with(b"#") || row.iter().all(|b| b.is_ascii_whitespace()) {
            continue;
        }
        split_fields(row, dialect, fields);
        let n = fields.len() as u32;
        match counts.iter_mut().find(|(w, _)| *w == n) {
            Some((_, c)) => *c += 1,
            None => counts.push((n, 1)),
        }
        scored += 1;
        if scored as usize == MAX_ROWS {
            break;
        }
    }
    let Some(&(width, n)) = counts.iter().max_by_key(|(w, c)| (*c, *w)) else {
        return (0.0, 0);
    };
    (f64::from(n) / f64::from(scored), width)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delim(sample: &str) -> char {
        detect_dialect(sample.as_bytes()).delimiter as char
    }

    #[test]
    fn picks_the_consistent_delimiter() {
        assert_eq!(delim("a,b,c\n1,2,3\n4,5,6\n"), ',');
        assert_eq!(delim("a\tb\n1\t2\n"), '\t');
        assert_eq!(delim("a|b\n1|2\n"), '|');
        assert_eq!(
            delim("a;b;c\n1,5;2;3\n"),
            ';',
            "decimal commas inside a semicolon file"
        );
    }

    #[test]
    fn delimiters_inside_quotes_do_not_count() {
        assert_eq!(delim("name,note\n\"x;y;z\",1\n\"p;q;r\",2\n"), ',');
        assert_eq!(delim("a\tb\n\"x,y\"\t1\n\"p,q\"\t2\n"), '\t');
    }

    #[test]
    fn a_steady_second_candidate_loses_to_the_wider_split() {
        // Every row has exactly one comma (coordinates), but semicolons give more fields.
        let s = "id;name;coords;city\n1;A;48.86, 2.29;Paris\n2;B;45.76, 4.83;Lyon\n3;C;43.30, 5.37;Marseille\n";
        assert_eq!(delim(s), ';');
    }

    #[test]
    fn comment_lines_are_skipped_while_scoring() {
        let s = "# Data file, with notes; see README | v2\n# columns: a, b\na\tb\tc\n1\t2\t3\n4\t5\t6\n";
        assert_eq!(delim(s), '\t');
    }

    #[test]
    fn a_truncated_last_row_is_ignored() {
        assert_eq!(delim("a|b|c\n1|2|3\n4|5|6\n7|8"), '|');
    }

    #[test]
    fn falls_back_to_comma_and_detects_line_endings() {
        assert_eq!(delim(""), ',');
        assert_eq!(delim("just\none\ncolumn\n"), ',');
        assert_eq!(
            detect_dialect(b"a;b\r\n1;2\r\n").line_ending,
            LineEnding::CrLf
        );
        assert_eq!(detect_dialect(b"\xEF\xBB\xBFa;b\n1;2\n").delimiter, b';');
    }

    #[test]
    fn nasty_corpus_files() {
        for (body, want) in [
            ("a;b;c\n1,5;2;3\n", ';'),
            ("a\tb\n1\t2\n", '\t'),
            ("a|b\n1|2\n", '|'),
            ("a,b\r\n1,2\r\n3,4\r\n", ','),
            (
                "id,note\n1,\"line one\nline two\"\n2,\"has \"\"quotes\"\"\"\n3,plain\n",
                ',',
            ),
            ("a,b,c\n1,2\n3,4,5,6\n7,8,9\n", ','),
        ] {
            assert_eq!(delim(body), want, "{body:?}");
        }
    }

    fn header(sample: &str) -> bool {
        detect_header(sample.as_bytes(), &detect_dialect(sample.as_bytes()))
    }

    #[test]
    fn names_above_numbers_dates_and_codes_are_a_header() {
        let s = "id,name,revenue,date,code\n0,Alice,21688,2026-10-07,00161\n1,Lena,25873,2026-12-16,00012\n2,Sam,16869,2026-01-12,00753\n";
        assert!(header(s));
        assert!(detect_dialect(s.as_bytes()).has_header, "set on detection");
        // pandas writes an empty name over its index column.
        assert!(header(",value\n0,1.5\n1,2.5\n"));
        assert!(header("\u{feff}a;b\n1;2\n3;4\n"), "after a BOM");
    }

    #[test]
    fn a_first_row_shaped_like_the_rest_is_data() {
        assert!(!header("1,2,3\n4,5,6\n7,8,9\n"));
        assert!(!header(
            "2026-01-01,AB12\n2026-01-02,CD34\n2026-01-03,EF56\n"
        ));
        assert!(!header("-3.5\t1e3\n2.25\t-7\n"));
    }

    #[test]
    fn free_text_ties_go_to_header() {
        // Nothing tells names from data here; most files have a header.
        assert!(header("name,city\nAlice,Paris\nBob,Lyon\n"));
        assert!(header("Alice,Paris\nBob,Lyon\n"));
    }

    #[test]
    fn no_second_row_or_a_comment_first_means_no_header() {
        assert!(!header(""));
        assert!(!header("a,b,c\n"));
        assert!(!header("# notes\na\tb\n1\t2\n"));
        // A file without a final newline still has its last row.
        assert!(header("a,b\n1,2"));
    }
}
