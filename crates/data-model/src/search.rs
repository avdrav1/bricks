//! Find (SRCH-1): the next cell, from a given one, whose shown value contains some text.
//!
//! Rules (user decisions, 2026-10-09):
//! - A match is the text anywhere in a cell's value as shown: edits included, quotes
//!   unescaped. "Match case" off ignores ASCII case, like sort and filter.
//! - The search goes row by row from the cursor: across its row, then down, and wraps to
//!   the top. It visits the rows shown (a filter's hidden rows don't count), in their order.
//! - Scope: the whole table, or one column.
//!
//! One parallel pass over the file in file order finds every row with a match. A byte
//! search over each chunk of rows (case-folded when case doesn't matter) picks the rows
//! worth splitting into cells; rows with edits are checked from their edits. Then a walk
//! through the rows as shown, from the cursor, finds the next one, and the cell in it.

use crate::{Col, ColId, CsvTable, Row, RowBlock, RowId, TableSource};
use csv_engine::{split_fields, RowIndex};
use memchr::memmem;
use rayon::prelude::*;
use std::borrow::Cow;
use std::collections::HashSet;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// What to find.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub text: String,
    pub match_case: bool,
    /// One column to look in (by identity), or `None` for every column shown.
    pub col: Option<ColId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchError {
    /// Indexing has not finished, so not every row is known yet.
    NotIndexed,
    Cancelled,
}

/// File rows searched per task, and between cancel checks.
const CHUNK: u64 = 65_536;
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// The rows with a match somewhere in scope.
struct Hits {
    /// One bit per file row.
    file: Vec<u64>,
    inserted: HashSet<RowId>,
}

impl Hits {
    fn has(&self, id: RowId) -> bool {
        if id.is_inserted() {
            self.inserted.contains(&id)
        } else {
            let r = id.0 as usize;
            self.file
                .get(r / 64)
                .is_some_and(|w| w >> (r % 64) & 1 == 1)
        }
    }

    /// The first file row in `rows` with a match.
    fn first_in(&self, rows: Range<u64>) -> Option<u64> {
        let mut r = rows.start;
        while r < rows.end {
            let word = self.file.get((r / 64) as usize).copied().unwrap_or(0) >> (r % 64);
            if word == 0 {
                r = (r / 64 + 1) * 64;
                continue;
            }
            r += u64::from(word.trailing_zeros());
            return (r < rows.end).then_some(r);
        }
        None
    }
}

/// The query's text as compared: folded to lower case unless case matters.
struct Needle {
    bytes: Vec<u8>,
    fold: bool,
}

impl Needle {
    fn new(q: &Query) -> Self {
        let mut bytes = q.text.as_bytes().to_vec();
        if !q.match_case {
            bytes.make_ascii_lowercase();
        }
        Self {
            bytes,
            fold: !q.match_case,
        }
    }

    fn in_value(&self, value: &[u8], scratch: &mut Vec<u8>) -> bool {
        let hay = if self.fold {
            scratch.clear();
            scratch.extend(value.iter().map(u8::to_ascii_lowercase));
            &scratch[..]
        } else {
            value
        };
        memmem::find(hay, &self.bytes).is_some()
    }
}

impl CsvTable {
    /// The next cell after `from` (row, column of the rows shown) whose value contains
    /// the query, row by row and wrapping to the top; `from` itself last. `None` if no
    /// cell does. Run it on a snapshot on the job pool; `done` counts file rows searched.
    pub fn find(
        &mut self,
        q: &Query,
        from: (Row, Col),
        cancel: &AtomicBool,
        done: &AtomicU64,
    ) -> Result<Option<(Row, Col)>, SearchError> {
        if !self.index.is_complete() {
            return Err(SearchError::NotIndexed);
        }
        let n = self.row_count();
        if q.text.is_empty() || n == 0 {
            return Ok(None);
        }
        let needle = Needle::new(q);
        let hits = self.hit_rows(q, &needle, cancel, done)?;
        let (row, col) = (from.0.min(n - 1), from.1);
        let mut scratch = Vec::new();
        // Rest of the cursor's row, the rows below, the rows above, then the cursor's row
        // up to the cursor.
        if let Some(c) = self.match_in_row(q, &needle, row, |c| c > col, &mut scratch) {
            return Ok(Some((row, c)));
        }
        for rows in [row + 1..n, 0..row] {
            let mut start = rows.start;
            while let Some(r) = self.next_hit_row(&hits, start..rows.end) {
                if let Some(c) = self.match_in_row(q, &needle, r, |_| true, &mut scratch) {
                    return Ok(Some((r, c)));
                }
                start = r + 1;
            }
        }
        Ok(self
            .match_in_row(q, &needle, row, |c| c <= col, &mut scratch)
            .map(|c| (row, c)))
    }

    /// Every row with a match in scope, shown or not.
    fn hit_rows(
        &self,
        q: &Query,
        needle: &Needle,
        cancel: &AtomicBool,
        done: &AtomicU64,
    ) -> Result<Hits, SearchError> {
        let file_rows = self.index.row_count();
        // Quotes are doubled in the file, so a quote in the text can't be looked for in
        // the raw bytes: then every row is checked cell by cell.
        let prefilter = !needle.bytes.contains(&self.dialect.quote);
        let chunks: Vec<Vec<u64>> = (0..file_rows.div_ceil(CHUNK))
            .into_par_iter()
            .map(|c| {
                if cancel.load(Ordering::Relaxed) {
                    return Err(SearchError::Cancelled);
                }
                let rows = c * CHUNK..file_rows.min((c + 1) * CHUNK);
                let mut spans = Vec::new();
                let found = self.index.row_spans(rows.clone(), &mut spans).ok();
                if found != Some((rows.end - rows.start) as usize) {
                    return Err(SearchError::Cancelled); // the file changed underneath
                }
                let mut words = vec![0u64; spans.len().div_ceil(64)];
                let (mut fields, mut scratch) = (Vec::new(), Vec::new());
                let mut check = |i: usize| {
                    let row = rows.start + i as u64;
                    // Edited rows are checked from their edits (`edited_hits`).
                    if self.overlay.row(RowId::source(row)).is_none()
                        && self.file_row_matches(
                            q,
                            needle,
                            row,
                            &spans[i],
                            &mut fields,
                            &mut scratch,
                        )
                    {
                        words[i / 64] |= 1 << (i % 64);
                    }
                };
                if prefilter {
                    let bytes = self.index.source().bytes();
                    let (start, end) = match (spans.first(), spans.last()) {
                        (Some(a), Some(b)) => (a.start as usize, b.end as usize),
                        _ => (0, 0),
                    };
                    let chunk = &bytes[start..end];
                    let mut folded = Vec::new();
                    let hay = if needle.fold {
                        folded.extend(chunk.iter().map(u8::to_ascii_lowercase));
                        &folded[..]
                    } else {
                        chunk
                    };
                    let mut next_row = 0usize;
                    for at in memmem::find_iter(hay, &needle.bytes) {
                        let at = (start + at) as u64;
                        let i = spans.partition_point(|s| s.end <= at);
                        if i >= next_row && i < spans.len() {
                            check(i);
                            next_row = i + 1;
                        }
                    }
                } else {
                    (0..spans.len()).for_each(&mut check);
                }
                done.fetch_add(rows.end - rows.start, Ordering::Relaxed);
                Ok(words)
            })
            .collect::<Result<_, _>>()?;
        let mut hits = Hits {
            file: Vec::with_capacity(file_rows.div_ceil(64) as usize),
            inserted: HashSet::new(),
        };
        for c in chunks {
            hits.file.extend(c);
        }
        self.edited_hits(q, needle, &mut hits);
        Ok(hits)
    }

    /// Whether file row `row` (at `span`, without edits) has a match in scope.
    fn file_row_matches(
        &self,
        q: &Query,
        needle: &Needle,
        row: u64,
        span: &Range<u64>,
        fields: &mut Vec<csv_engine::Field>,
        scratch: &mut Vec<u8>,
    ) -> bool {
        let bytes = self.index.source().bytes();
        let mut line = &bytes[span.start as usize..span.end as usize];
        if span.start == 0 {
            line = line.strip_prefix(UTF8_BOM).unwrap_or(line);
        }
        split_fields(line, &self.dialect, fields);
        let cleared = self.cleared.at(row);
        fields.iter().enumerate().any(|(i, f)| {
            let id = ColId::source(i as Col);
            q.col.is_none_or(|c| c == id)
                && (self.cols.is_none() || self.col_of(id).is_some())
                && !cleared.is_some_and(|c| c.contains(id))
                && needle.in_value(&f.value(line, self.dialect.quote), scratch)
        })
    }

    /// Check every row with edits (source rows the file pass skipped, and inserted rows)
    /// from its cells as shown.
    fn edited_hits(&self, q: &Query, needle: &Needle, hits: &mut Hits) {
        let mut scratch = Vec::new();
        for (&id, edits) in self.overlay.iter_rows() {
            let cols: Vec<ColId> = match q.col {
                Some(c) => vec![c],
                None => {
                    let fields = if id.is_inserted() {
                        0
                    } else {
                        let mut f = Vec::new();
                        self.index
                            .row_fields(id.0, &mut f)
                            .map_or(0, |_| f.len() as Col)
                    };
                    let width = match &self.cols {
                        Some(m) => m.width_with(fields, edits.keys().copied()),
                        None => fields.max(edits.keys().next_back().map_or(0, |c| c.0 + 1)),
                    };
                    (0..width).map(|p| self.col_id(p)).collect()
                }
            };
            let found = cols.into_iter().any(|c| {
                self.cell_of(id, c)
                    .is_some_and(|v| needle.in_value(v.as_bytes(), &mut scratch))
            });
            if id.is_inserted() {
                if found {
                    hits.inserted.insert(id);
                }
            } else if let Some(w) = hits.file.get_mut(id.0 as usize / 64) {
                let bit = 1 << (id.0 % 64);
                if found {
                    *w |= bit;
                } else {
                    *w &= !bit;
                }
            }
        }
    }

    /// The first row in `rows` (of the rows shown) with a match.
    fn next_hit_row(&self, hits: &Hits, rows: Range<Row>) -> Option<Row> {
        let mut r = rows.start;
        for positions in self.view_ranges(rows) {
            let runs = match &self.rows {
                None => vec![crate::Run {
                    first: RowId::source(positions.start),
                    len: positions.end - positions.start,
                }],
                Some(order) => order.runs_in(positions),
            };
            for run in runs {
                if run.first.is_inserted() {
                    if let Some(i) = (0..run.len).find(|&i| hits.has(RowId(run.first.0 + i))) {
                        return Some(r + i);
                    }
                } else if let Some(k) = hits.first_in(run.first.0..run.first.0 + run.len) {
                    return Some(r + (k - run.first.0));
                }
                r += run.len;
            }
        }
        None
    }

    /// The first column of row `row` (shown) that `want` admits and whose value contains
    /// the query.
    fn match_in_row(
        &mut self,
        q: &Query,
        needle: &Needle,
        row: Row,
        want: impl Fn(Col) -> bool,
        scratch: &mut Vec<u8>,
    ) -> Option<Col> {
        let mut block = RowBlock::full_text();
        self.read_rows(row..row + 1, &mut block);
        let only = match q.col {
            Some(id) => Some(self.col_of(id)?),
            None => None,
        };
        (0..block.cells_in_row(row))
            .filter(|&c| only.is_none_or(|o| o == c) && want(c))
            .find(|&c| {
                let v: Cow<str> = Cow::Borrowed(block.cell(row, c).unwrap_or(""));
                needle.in_value(v.as_bytes(), scratch)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn needle(text: &str, match_case: bool) -> Needle {
        Needle::new(&Query {
            text: text.into(),
            match_case,
            col: None,
        })
    }

    #[test]
    fn case_folds_unless_asked_not_to() {
        let mut s = Vec::new();
        assert!(needle("LAND", false).in_value(b"Portland", &mut s));
        assert!(!needle("LAND", true).in_value(b"Portland", &mut s));
        assert!(needle("land", true).in_value(b"Portland", &mut s));
        assert!(!needle("x", false).in_value(b"", &mut s));
    }
}
