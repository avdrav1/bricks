//! Type-aware sort (SORT-1, ADR 0004): one parallel pass reads the column into a key per
//! row, a parallel sort orders the keys, and the result is a new row order (a flat
//! permutation) applied as one undoable [`Edit::Reorder`]. Values never change
//! (invariant 1); only where rows are shown does.
//!
//! The order (user decisions, 2026-10-09):
//! - Values of the column's inferred type (TYPE-1) come first in their own order: numbers
//!   numerically, dates and date-times in time, `false` before `true`, text ignoring ASCII
//!   case with exact bytes breaking ties (`apple`, `Apple`... `banana`).
//! - Values that don't fit the type (`n/a` in a number column) follow as text; descending
//!   reverses both, so they lead. Empty cells are always last.
//! - Ties keep their current order (the sort is stable), in either direction.
//! - The header row stays where it is.
//!
//! The pass reads the file in file order (fast whatever the current order is) and keys
//! each row with its current position. Text keys hold the first 8 bytes; rows whose long
//! values share those are compared whole afterwards.

use crate::infer::{instant, value_type};
use crate::rowmap::{unpack, RowOrder};
use crate::{CellRef, Col, ColId, CsvTable, Edit, InferredType};
use csv_engine::RowIndex;
use rayon::prelude::*;
use std::borrow::Cow;
use std::cmp::Ordering;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;

/// Which way a sort goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Ascending,
    Descending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortError {
    /// Indexing has not finished, so not every row is known yet.
    NotIndexed,
    /// More than 2^31 rows: a sorted order holds 4 bytes a row.
    TooManyRows,
    Cancelled,
}

/// File rows keyed per task, and between cancel checks.
const CHUNK: u64 = 65_536;

/// Classes in ascending order.
const TYPED: u8 = 0;
const MISFIT: u8 = 1;
const BLANK: u8 = 2;

/// Unwinds a sort part-way once it is cancelled. Raised with `resume_unwind`, which
/// skips the panic hook: nothing is printed, and nothing is wrong.
struct Stop;

/// One row's sort key: 24 bytes, so 19M rows take about 460 MB while sorting.
#[derive(Debug, Clone, Copy)]
struct Key {
    /// Order-preserving value: the number, the instant, or the case-folded first 8 bytes.
    k1: u64,
    /// For text, the exact first 8 bytes: the tiebreak between values equal but for case.
    k2: u64,
    /// Position in the current order: the final tiebreak, which keeps the sort stable.
    pos: u32,
    class: u8,
    /// Text longer than 8 bytes: equal `k1`s with this set get compared whole.
    long: bool,
}

fn number_key(f: f64) -> u64 {
    let b = f.to_bits();
    if b >> 63 == 1 {
        !b
    } else {
        b | 1 << 63
    }
}

fn text_key(raw: &str, class: u8, pos: u32) -> Key {
    let (mut folded, mut exact) = ([0u8; 8], [0u8; 8]);
    for (i, &c) in raw.as_bytes().iter().take(8).enumerate() {
        folded[i] = c.to_ascii_lowercase();
        exact[i] = c;
    }
    Key {
        k1: u64::from_be_bytes(folded),
        k2: u64::from_be_bytes(exact),
        pos,
        class,
        long: raw.len() > 8,
    }
}

/// The key of `raw` in a column of type `ty`.
fn key(raw: &str, ty: InferredType, pos: u32) -> Key {
    use InferredType::*;
    if raw.is_empty() {
        return Key {
            k1: 0,
            k2: 0,
            pos,
            class: BLANK,
            long: false,
        };
    }
    let kind = value_type(raw);
    let typed = match ty {
        Text => return text_key(raw, TYPED, pos),
        Integer | Decimal => matches!(kind, Some(Integer | Decimal))
            .then(|| raw.parse::<f64>().ok().map(number_key))
            .flatten(),
        Date | DateTime => matches!(kind, Some(Date | DateTime))
            .then(|| instant(raw).map(|t| t as u64 ^ 1 << 63))
            .flatten(),
        Boolean => (kind == Some(Boolean)).then(|| {
            u64::from(raw.eq_ignore_ascii_case("true") || raw.eq_ignore_ascii_case("yes"))
        }),
    };
    match typed {
        Some(k1) => Key {
            k1,
            k2: 0,
            pos,
            class: TYPED,
            long: false,
        },
        None => text_key(raw, MISFIT, pos),
    }
}

fn compare(a: &Key, b: &Key, desc: bool) -> Ordering {
    // Descending reverses typed values and misfits, and their order; blanks stay last.
    let rank = |k: &Key| match k.class {
        BLANK => 2,
        c if desc => 1 - c,
        c => c,
    };
    rank(a)
        .cmp(&rank(b))
        .then_with(|| {
            if a.class == BLANK {
                return Ordering::Equal;
            }
            let o = (a.k1, a.long, a.k2).cmp(&(b.k1, b.long, b.k2));
            if desc {
                o.reverse()
            } else {
                o
            }
        })
        .then(a.pos.cmp(&b.pos))
}

/// Whole text in sort order: ASCII case ignored, then exact bytes.
fn text_cmp(a: &str, b: &str) -> Ordering {
    let fold = |s: &str| {
        s.bytes()
            .map(|c| c.to_ascii_lowercase())
            .collect::<Vec<_>>()
    };
    fold(a)
        .cmp(&fold(b))
        .then_with(|| a.as_bytes().cmp(b.as_bytes()))
}

impl CsvTable {
    /// The edit that sorts every row but the header by column `col` (its inferred type,
    /// Text until inferred), or `Ok(None)` if they are in that order already. Reads a
    /// snapshot on the job pool (`snapshot()` first); `done` counts file rows read.
    /// Apply the edit to the table the snapshot came from, unchanged since.
    pub fn sort_rows(
        &self,
        col: Col,
        order: SortOrder,
        cancel: &AtomicBool,
        done: &AtomicU64,
    ) -> Result<Option<Edit>, SortError> {
        if !self.index.is_complete() {
            return Err(SortError::NotIndexed);
        }
        let cancelled = || cancel.load(AtomicOrdering::Relaxed);
        let desc = order == SortOrder::Descending;
        let ty = self.column_type(col).unwrap_or(InferredType::Text);
        let cid = self.col_id(col);
        let file_rows = self.index.row_count();
        // Every row's packed id, by position.
        let ids: Vec<u32> = match &self.rows {
            Some(o) => o.packed().ok_or(SortError::TooManyRows)?,
            None if file_rows >= 1 << 31 => return Err(SortError::TooManyRows),
            None => (0..file_rows as u32).collect(),
        };
        let first = (self.first_row() as usize).min(ids.len());
        if ids.len() - first < 2 {
            return Ok(None);
        }
        // Where each file row is shown (`u32::MAX`: deleted, or the header), and the
        // inserted rows shown.
        let mut pos_of = vec![u32::MAX; file_rows as usize];
        let mut inserted = Vec::new();
        for (p, &v) in ids.iter().enumerate().skip(first) {
            match unpack(v) {
                id if id.is_inserted() => inserted.push((id, p as u32)),
                id => pos_of[id.0 as usize] = p as u32,
            }
        }

        let chunks: Vec<Vec<Key>> = (0..file_rows.div_ceil(CHUNK))
            .into_par_iter()
            .map(|c| {
                if cancelled() {
                    return Err(SortError::Cancelled);
                }
                let rows = c * CHUNK..file_rows.min((c + 1) * CHUNK);
                let (mut spans, mut fields) = (Vec::new(), Vec::new());
                let found = self.index.row_spans(rows.clone(), &mut spans).ok();
                if found != Some((rows.end - rows.start) as usize) {
                    // The file changed underneath: nothing sensible to sort.
                    return Err(SortError::Cancelled);
                }
                let mut keys = Vec::with_capacity(spans.len());
                for (row, span) in rows.clone().zip(&spans) {
                    let pos = pos_of[row as usize];
                    if pos != u32::MAX {
                        keys.push(key(&self.file_cell(row, span, cid, &mut fields), ty, pos));
                    }
                }
                done.fetch_add(rows.end - rows.start, AtomicOrdering::Relaxed);
                Ok(keys)
            })
            .collect::<Result<_, _>>()?;
        drop(pos_of);
        let mut keys = Vec::with_capacity(ids.len() - first);
        for c in chunks {
            keys.extend(c);
        }
        for (id, pos) in inserted {
            let raw = self
                .overlay
                .get(CellRef { row: id, col: cid })
                .unwrap_or("");
            keys.push(key(raw, ty, pos));
        }

        // The sorts can't be paused, so a cancel unwinds out of their comparisons (ENG-8:
        // every long job stops within 200 ms; a 19M-row sort alone takes longer).
        let stop = || {
            if cancelled() {
                std::panic::resume_unwind(Box::new(Stop));
            }
        };
        let sorted = catch_unwind(AssertUnwindSafe(|| {
            keys.par_sort_unstable_by(|a, b| {
                stop();
                compare(a, b, desc)
            });
            // Long text sharing its first 8 bytes (case-folded): compare whole values.
            keys.par_chunk_by_mut(|a, b| {
                a.class != BLANK && a.class == b.class && a.long && b.long && a.k1 == b.k1
            })
            .filter(|g| g.len() > 1)
            .for_each(|g| self.refine(g, &ids, cid, desc, &stop));
        }));
        match sorted {
            Ok(()) => {}
            Err(p) if p.is::<Stop>() => return Err(SortError::Cancelled),
            Err(p) => std::panic::resume_unwind(p),
        }

        let mut out = Vec::with_capacity(ids.len());
        out.extend_from_slice(&ids[..first]);
        out.extend(keys.iter().map(|k| ids[k.pos as usize]));
        if out == ids {
            return Ok(None);
        }
        let file_order =
            out.len() as u64 == file_rows && out.iter().enumerate().all(|(i, &v)| v as usize == i);
        Ok(Some(Edit::Reorder(
            (!file_order).then(|| RowOrder::Sorted(Arc::new(out))),
        )))
    }

    /// Re-order `group` (keys with equal long-text prefixes) by whole values. `stop`
    /// unwinds once the sort is cancelled.
    fn refine(&self, group: &mut [Key], ids: &[u32], col: ColId, desc: bool, stop: &impl Fn()) {
        let mut whole: Vec<(String, Key)> = group
            .iter()
            .map(|k| {
                stop();
                let id = unpack(ids[k.pos as usize]);
                let text = self
                    .cell_of(id, col)
                    .map_or_else(String::new, Cow::into_owned);
                (text, *k)
            })
            .collect();
        whole.sort_by(|(a, ka), (b, kb)| {
            stop();
            let o = text_cmp(a, b);
            (if desc { o.reverse() } else { o }).then(ka.pos.cmp(&kb.pos))
        });
        for (slot, (_, k)) in group.iter_mut().zip(whole) {
            *slot = k;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(values: &[&str], ty: InferredType, desc: bool) -> Vec<String> {
        let mut keys: Vec<Key> = values
            .iter()
            .enumerate()
            .map(|(i, v)| key(v, ty, i as u32))
            .collect();
        keys.sort_by(|a, b| compare(a, b, desc));
        keys.iter()
            .map(|k| values[k.pos as usize].to_owned())
            .collect()
    }

    #[test]
    fn numbers_misfits_and_blanks() {
        let v = ["10", "", "n/a", "2", "-1.5", "1e2", "abc", "2"];
        assert_eq!(
            order(&v, InferredType::Integer, false),
            ["-1.5", "2", "2", "10", "1e2", "abc", "n/a", ""]
        );
        assert_eq!(
            order(&v, InferredType::Integer, true),
            ["n/a", "abc", "1e2", "10", "2", "2", "-1.5", ""]
        );
    }

    #[test]
    fn short_text_ignores_case_then_takes_exact_bytes() {
        let v = ["b", "B", "a", "A", "ab", "Ab", "abcdefgh", ""];
        assert_eq!(
            order(&v, InferredType::Text, false),
            ["A", "a", "Ab", "ab", "abcdefgh", "B", "b", ""]
        );
    }

    #[test]
    fn dates_and_times_in_time_order() {
        let v = [
            "2026-04-05T10:30+02:00", // 08:30Z
            "2026-04-05",
            "2026-04-05T09:00Z",
            "2026-04-04T23:59:59.999999",
            "2000-02-29",
        ];
        assert_eq!(
            order(&v, InferredType::DateTime, false),
            [
                "2000-02-29",
                "2026-04-04T23:59:59.999999",
                "2026-04-05",
                "2026-04-05T10:30+02:00",
                "2026-04-05T09:00Z",
            ]
        );
    }

    #[test]
    fn booleans_false_first() {
        let v = ["true", "No", "FALSE", "yes"];
        assert_eq!(
            order(&v, InferredType::Boolean, false),
            ["No", "FALSE", "true", "yes"]
        );
    }

    #[test]
    fn whole_text_order_ignores_case_then_bytes() {
        assert_eq!(text_cmp("alexander", "Alexandra"), Ordering::Less);
        assert_eq!(text_cmp("Alexandra", "alexandra"), Ordering::Less);
        assert_eq!(text_cmp("abcdefgh", "abcdefghi"), Ordering::Less);
    }
}
