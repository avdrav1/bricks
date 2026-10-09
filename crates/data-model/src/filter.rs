//! Column filters (FILT-1, ADR 0004): a filter tests one column's cells; one parallel pass
//! over the file evaluates it into a set of row ids (a bitmap over file rows). The table
//! shows the rows every filter passes, in its current order: the header row always, and
//! nothing about the data changes. Filtering is a view: saving writes every row
//! (invariant 7), and undo has nothing to undo.
//!
//! Rules (user decisions, 2026-10-09):
//! - Text conditions ignore ASCII case, like the sort.
//! - Number conditions take cells that are numbers (as TYPE-1 reads them); any other
//!   cell fails them.
//! - A filter is evaluated when applied: a row edited afterwards stays shown until the
//!   filter is applied again.

use crate::infer::value_type;
use crate::rowmap::{unpack, RowOrder, PACKED_INSERTED};
use crate::{CellRef, Col, ColId, CsvTable, InferredType, RowId};
use csv_engine::RowIndex;
use rayon::prelude::*;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

/// A comparison with a number.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Compare {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// What a cell must be to pass.
#[derive(Debug, Clone, PartialEq)]
pub enum Test {
    /// The whole cell, ignoring case.
    Equals(String),
    Contains(String),
    NotContains(String),
    Empty,
    NonEmpty,
    Number(Compare, f64),
}

/// A test on one column, by identity: it stays with its column through column inserts.
#[derive(Debug, Clone, PartialEq)]
pub struct Filter {
    pub col: ColId,
    pub test: Test,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterError {
    /// Indexing has not finished, so not every row is known yet.
    NotIndexed,
    Cancelled,
}

/// The rows a filter passed when it was evaluated. Rows inserted later count as passing.
#[derive(Debug, Clone, Default)]
pub struct RowSet {
    /// One bit per file row: set if it passes.
    file: Vec<u64>,
    /// Inserted rows that failed.
    hidden_inserted: HashSet<RowId>,
}

impl RowSet {
    pub fn passes(&self, id: RowId) -> bool {
        if id.is_inserted() {
            !self.hidden_inserted.contains(&id)
        } else {
            let r = id.0 as usize;
            self.file
                .get(r / 64)
                .is_some_and(|w| w >> (r % 64) & 1 == 1)
        }
    }
}

/// File rows tested per task, and between cancel checks: a multiple of 64.
const CHUNK: u64 = 65_536;

/// A [`Test`] ready to run: text folded once.
struct Matcher {
    test: Test,
    needle: Vec<u8>,
}

impl Matcher {
    fn new(test: &Test) -> Self {
        let needle = match test {
            Test::Equals(s) | Test::Contains(s) | Test::NotContains(s) => s.to_ascii_lowercase(),
            _ => String::new(),
        };
        Self {
            test: test.clone(),
            needle: needle.into_bytes(),
        }
    }

    fn matches(&self, raw: &str) -> bool {
        let contains = || {
            let n = self.needle.len();
            n == 0
                || raw.as_bytes().windows(n).any(|w| {
                    w.iter()
                        .zip(&self.needle)
                        .all(|(a, b)| a.to_ascii_lowercase() == *b)
                })
        };
        match &self.test {
            Test::Equals(_) => raw.as_bytes().eq_ignore_ascii_case(&self.needle),
            Test::Contains(_) => contains(),
            Test::NotContains(_) => !contains(),
            Test::Empty => raw.is_empty(),
            Test::NonEmpty => !raw.is_empty(),
            Test::Number(cmp, x) => {
                matches!(
                    value_type(raw),
                    Some(InferredType::Integer | InferredType::Decimal)
                ) && raw.parse::<f64>().is_ok_and(|v| match cmp {
                    Compare::Eq => v == *x,
                    Compare::Ne => v != *x,
                    Compare::Lt => v < *x,
                    Compare::Le => v <= *x,
                    Compare::Gt => v > *x,
                    Compare::Ge => v >= *x,
                })
            }
        }
    }
}

impl CsvTable {
    /// Evaluate `filter` over every row of the file and every inserted row, shown or not:
    /// one parallel pass in file order. Run it on a snapshot on the job pool, then
    /// [`Self::set_filter`]. `done` counts file rows tested.
    pub fn filter_rows(
        &self,
        filter: &Filter,
        cancel: &AtomicBool,
        done: &AtomicU64,
    ) -> Result<RowSet, FilterError> {
        if !self.index.is_complete() {
            return Err(FilterError::NotIndexed);
        }
        let m = Matcher::new(&filter.test);
        let col = filter.col;
        let file_rows = self.index.row_count();
        let chunks: Vec<Vec<u64>> = (0..file_rows.div_ceil(CHUNK))
            .into_par_iter()
            .map(|c| {
                if cancel.load(Ordering::Relaxed) {
                    return Err(FilterError::Cancelled);
                }
                let rows = c * CHUNK..file_rows.min((c + 1) * CHUNK);
                let (mut spans, mut fields) = (Vec::new(), Vec::new());
                let found = self.index.row_spans(rows.clone(), &mut spans).ok();
                if found != Some((rows.end - rows.start) as usize) {
                    return Err(FilterError::Cancelled); // the file changed underneath
                }
                let mut words = vec![0u64; spans.len().div_ceil(64)];
                for (i, (row, span)) in rows.clone().zip(&spans).enumerate() {
                    if m.matches(&self.file_cell(row, span, col, &mut fields)) {
                        words[i / 64] |= 1 << (i % 64);
                    }
                }
                done.fetch_add(rows.end - rows.start, Ordering::Relaxed);
                Ok(words)
            })
            .collect::<Result<_, _>>()?;
        let mut file = Vec::with_capacity(file_rows.div_ceil(64) as usize);
        for c in chunks {
            file.extend(c);
        }
        let mut hidden_inserted = HashSet::new();
        if let Some(order) = &self.rows {
            order.for_each(|run| {
                if run.first.is_inserted() {
                    for id in (0..run.len).map(|i| RowId(run.first.0 + i)) {
                        let raw = self.overlay.get(CellRef { row: id, col }).unwrap_or("");
                        if !m.matches(raw) {
                            hidden_inserted.insert(id);
                        }
                    }
                }
            });
        }
        Ok(RowSet {
            file,
            hidden_inserted,
        })
    }

    /// Show only the rows `rows` (from [`Self::filter_rows`] of `filter`) passes, with the
    /// other columns' filters; it replaces any filter on the same column.
    pub fn set_filter(&mut self, filter: Filter, rows: RowSet) {
        self.filters.retain(|(f, _)| f.col != filter.col);
        self.filters.push((filter, Arc::new(rows)));
        self.update_view();
    }

    /// Stop filtering column `col` (by position).
    pub fn clear_filter(&mut self, col: Col) {
        let id = self.col_id(col);
        self.filters.retain(|(f, _)| f.col != id);
        self.update_view();
    }

    /// The filter on column `col` (by position), if any.
    pub fn filter_on(&self, col: Col) -> Option<&Filter> {
        let id = self.col_id(col);
        self.filters.iter().map(|(f, _)| f).find(|f| f.col == id)
    }

    /// Some rows are hidden by filters: rows can't be inserted or deleted (FILT-1).
    pub fn is_filtered(&self) -> bool {
        !self.filters.is_empty()
    }

    /// Columns the grid must show however few rows are loaded: the header row's, and
    /// every filtered one, so a filter that hides every row can still be cleared (FILT-2).
    pub fn min_width(&self) -> Col {
        let filtered = self
            .filters
            .iter()
            .filter_map(|(f, _)| self.col_of(f.col))
            .map(|c| c + 1)
            .max()
            .unwrap_or(0);
        let header = if self.has_header() && self.total_rows() > 0 {
            let id = self.id_at(0);
            let mut fields = Vec::new();
            let n = if id.is_inserted() {
                0
            } else {
                self.index
                    .row_fields(id.0, &mut fields)
                    .map_or(0, |_| fields.len() as Col)
            };
            let edits = self.overlay.row(id);
            match &self.cols {
                Some(m) => m.width(n, |c| edits.is_some_and(|e| e.contains_key(&c))),
                None => n.max(
                    edits
                        .and_then(|e| e.keys().next_back())
                        .map_or(0, |c| c.0 + 1),
                ),
            }
        } else {
            0
        };
        filtered.max(header)
    }

    /// Rows with no filter applied: the "y" in "x of y rows".
    pub fn unfiltered_row_count(&self) -> u64 {
        self.total_rows().saturating_sub(self.first_row())
    }

    /// Recompute which positions are shown, after the filters or the row order changed.
    /// Runs on the UI thread, so it touches each row once at most: the filters fold into
    /// one bitmap first, and file-order runs only visit the rows that pass.
    pub(crate) fn update_view(&mut self) {
        let Some(((_, head), rest)) = self.filters.split_first() else {
            self.visible = None;
            return;
        };
        let mut file = head.file.clone();
        let mut hidden = head.hidden_inserted.clone();
        for (_, set) in rest {
            for (w, o) in file.iter_mut().zip(&set.file) {
                *w &= o;
            }
            hidden.extend(set.hidden_inserted.iter().copied());
        }
        let bit = |r: u64| {
            file.get((r / 64) as usize)
                .is_some_and(|w| w >> (r % 64) & 1 == 1)
        };
        let first = self.first_row();
        let mut shown = Vec::new();
        // Positions `at..` showing file rows `rows`: the ones that pass.
        let passing = |rows: std::ops::Range<u64>, at: u64, shown: &mut Vec<u32>| {
            let mut r = rows.start;
            while r < rows.end {
                let word = file.get((r / 64) as usize).copied().unwrap_or(0) >> (r % 64);
                if word == 0 {
                    r = (r / 64 + 1) * 64; // nothing passes in the rest of this word
                    continue;
                }
                r += u64::from(word.trailing_zeros());
                if r < rows.end {
                    shown.push((at + r - rows.start) as u32);
                }
                r += 1;
            }
        };
        match &self.rows {
            None => passing(first..self.index.row_count(), first, &mut shown),
            Some(RowOrder::Sorted(ids)) => {
                for (p, &v) in ids.iter().enumerate().skip(first as usize) {
                    let pass = if v & PACKED_INSERTED == 0 {
                        bit(u64::from(v))
                    } else {
                        !hidden.contains(&unpack(v))
                    };
                    if pass {
                        shown.push(p as u32);
                    }
                }
            }
            Some(order) => {
                let mut at = 0u64;
                order.for_each(|run| {
                    let skip = first.saturating_sub(at).min(run.len);
                    if run.first.is_inserted() {
                        for i in skip..run.len {
                            if !hidden.contains(&RowId(run.first.0 + i)) {
                                shown.push((at + i) as u32);
                            }
                        }
                    } else {
                        let rows = run.first.0 + skip..run.first.0 + run.len;
                        passing(rows, at + skip, &mut shown);
                    }
                    at += run.len;
                });
            }
        }
        self.visible = Some(Arc::new(shown));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passes(test: Test, raw: &str) -> bool {
        Matcher::new(&test).matches(raw)
    }

    #[test]
    fn text_tests_ignore_case() {
        assert!(passes(Test::Equals("denver".into()), "Denver"));
        assert!(!passes(Test::Equals("denver".into()), "Denver "));
        assert!(passes(Test::Contains("ENV".into()), "Denver"));
        assert!(!passes(Test::Contains("x".into()), ""));
        assert!(
            passes(Test::Contains(String::new()), ""),
            "empty text is in everything"
        );
        assert!(passes(Test::NotContains("bos".into()), "Denver"));
        assert!(!passes(Test::NotContains("BOS".into()), "Boston"));
        assert!(passes(Test::Empty, ""));
        assert!(!passes(Test::Empty, " "));
        assert!(passes(Test::NonEmpty, " "));
    }

    #[test]
    fn number_tests_take_numbers_only() {
        let t = |c, x, raw| passes(Test::Number(c, x), raw);
        assert!(t(Compare::Eq, 5.0, "5"));
        assert!(t(Compare::Eq, 5.0, "5.00"));
        assert!(t(Compare::Eq, 1000.0, "1e3"));
        assert!(t(Compare::Ne, 5.0, "4"));
        assert!(
            !t(Compare::Ne, 5.0, "abc"),
            "not a number: fails every comparison"
        );
        assert!(!t(Compare::Ne, 5.0, ""));
        assert!(
            !t(Compare::Lt, 5.0, "00123"),
            "a code, not a number (TYPE-1)"
        );
        assert!(t(Compare::Lt, 5.0, "-7"));
        assert!(t(Compare::Le, 5.0, "5"));
        assert!(t(Compare::Gt, 5.0, "5.5"));
        assert!(!t(Compare::Ge, 5.0, "4.99"));
    }
}
