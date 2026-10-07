//! Dialect detection (ENG-4): which of comma, tab, semicolon, or pipe separates fields.
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
    best.map_or(base, |(_, _, delimiter)| Dialect { delimiter, ..base })
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
}
