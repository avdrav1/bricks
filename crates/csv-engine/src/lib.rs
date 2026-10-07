//! CSV parsing, dialect detection, encoding, row indexing, and writing.
//!
//! Invariant: the engine never rewrites a field's bytes. What it reads is what
//! `data-model` treats as the authoritative raw value.

mod detect;
mod fields;
mod index;
mod source;
#[cfg(test)]
mod testutil;

pub use detect::{detect_dialect, DETECT_SAMPLE_BYTES};
pub use fields::{encode_field, split_fields, Field};
pub use index::{IndexError, SparseRowIndex, STRIDE};
pub use source::Source;

/// How a CSV file is delimited and quoted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dialect {
    pub delimiter: u8,
    pub quote: u8,
    pub has_header: bool,
    pub line_ending: LineEnding,
}

impl Default for Dialect {
    fn default() -> Self {
        Self {
            delimiter: b',',
            quote: b'"',
            has_header: true,
            line_ending: LineEnding::Lf,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    CrLf,
}

/// Delimiters V0.1 must detect (spec section 6).
pub const CANDIDATE_DELIMITERS: [u8; 4] = *b",\t;|";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    Latin1,
}

/// Maps physical row numbers (file order) to byte ranges in the source file.
///
/// Implemented by [`SparseRowIndex`] (ENG-1, ADR 0002).
pub trait RowIndex: Send + Sync {
    /// Rows indexed so far. Grows while background indexing runs.
    fn row_count(&self) -> u64;
    /// True once the whole file has been indexed.
    fn is_complete(&self) -> bool;
    /// Byte range of a physical row in the source file, line terminator included.
    /// `None` past `row_count()` or once the file changed on disk.
    fn row_span(&self, row: u64) -> Option<std::ops::Range<u64>>;
    /// Spans of the rows in `rows` that are indexed so far, written to `out` (cleared
    /// first, capacity reused). Returns how many were written. One checkpoint lookup and
    /// one forward scan for the whole range: this is how the grid fetches a visible page.
    fn row_spans(
        &self,
        rows: std::ops::Range<u64>,
        out: &mut Vec<std::ops::Range<u64>>,
    ) -> Result<usize, IndexError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_dialect_is_comma_with_header() {
        let d = Dialect::default();
        assert_eq!(d.delimiter, b',');
        assert!(d.has_header);
    }
}
