//! Column type inference (TYPE-1): which semantic type each column's values have, from a
//! sample of rows. Types steer sort, filter, and display only; they never change a value
//! (invariant 1), so `00123` stays `00123` whatever its column is.
//!
//! The rules (user decisions, 2026-10-09):
//! - A column has a type only if every non-empty sampled value has it. Empty cells and
//!   null markers (`NA`, `N/A`, `null`, `-`) don't count either way; one stray word makes
//!   the column Text.
//! - Integers with a leading zero (`00123`, `000`) are Text: they are codes, not numbers.
//! - Plain formats only: `-12`, `+3`, `0.5`, `1e6` (no thousands separators or decimal
//!   commas); ISO dates `2026-04-05` and date-times `2026-04-05T10:30[:00][.000][Z|+02:00]`
//!   (or with a space). `true`/`false`/`yes`/`no` in any case are Boolean.
//! - Integer and Decimal values together make Decimal; Date and DateTime make DateTime.

use crate::{ColId, CsvTable, InferredType, RowBlock};
use formula::scalar::{date, is_boolean, is_null, number, time};
use std::sync::atomic::{AtomicBool, Ordering};

/// Rows sampled from the top of the table.
pub const SAMPLE_HEAD: u64 = 1_000;
/// Rows sampled at even steps through the rest, the last row included.
pub const SAMPLE_SPREAD: u64 = 1_000;
/// Sampled rows between cancel checks.
const CANCEL_EVERY: u64 = 256;

/// Each column's inferred type, by column identity.
pub type ColumnTypes = Vec<(ColId, InferredType)>;

/// The type of one raw value; `None` for an empty cell or a null marker.
pub fn value_type(raw: &str) -> Option<InferredType> {
    if raw.is_empty() || is_null(raw) {
        return None;
    }
    Some(if let Some(fraction) = number(raw) {
        if fraction {
            InferredType::Decimal
        } else {
            InferredType::Integer
        }
    } else if is_boolean(raw) {
        InferredType::Boolean
    } else if date(raw).is_some_and(|rest| rest.is_empty()) {
        InferredType::Date
    } else if date(raw).is_some_and(time) {
        InferredType::DateTime
    } else {
        InferredType::Text
    })
}

/// The type of a column holding values of types `a` and `b`.
fn combine(a: InferredType, b: InferredType) -> InferredType {
    use InferredType::*;
    match (a, b) {
        _ if a == b => a,
        (Integer, Decimal) | (Decimal, Integer) => Decimal,
        (Date, DateTime) | (DateTime, Date) => DateTime,
        _ => Text,
    }
}

impl CsvTable {
    /// Infer every column's type from a sample: the first [`SAMPLE_HEAD`] rows and
    /// [`SAMPLE_SPREAD`] more at even steps through the rest, the last row included. Reads
    /// cells as shown (edits included). Run it once the table is indexed, off the UI thread
    /// (on a snapshot); `None` once `cancel` is set.
    pub fn infer_types(&mut self, cancel: &AtomicBool) -> Option<ColumnTypes> {
        let rows = self.unfiltered_row_count();
        let head = rows.min(SAMPLE_HEAD);
        let rest = rows - head;
        let spread = rest.min(SAMPLE_SPREAD);
        let sample = (0..head).chain((0..spread).map(|i| {
            // Even steps from the first row after the head to the last row.
            head + if spread > 1 {
                i * (rest - 1) / (spread - 1)
            } else {
                rest - 1
            }
        }));
        let mut found: Vec<Option<InferredType>> = Vec::new();
        let mut block = RowBlock::full_text();
        for (n, r) in sample.enumerate() {
            if (n as u64).is_multiple_of(CANCEL_EVERY) && cancel.load(Ordering::Relaxed) {
                return None;
            }
            self.read_unfiltered(r..r + 1, &mut block);
            let cells = block.cells_in_row(r);
            if found.len() < cells as usize {
                found.resize(cells as usize, None);
            }
            for (c, slot) in found.iter_mut().enumerate().take(cells as usize) {
                if *slot == Some(InferredType::Text) {
                    continue;
                }
                if let Some(t) = block.cell(r, c as u32).and_then(value_type) {
                    *slot = Some(slot.map_or(t, |s| combine(s, t)));
                }
            }
        }
        Some(
            found
                .into_iter()
                .enumerate()
                .map(|(c, t)| (self.col_id(c as u32), t.unwrap_or(InferredType::Text)))
                .collect(),
        )
    }

    /// Keep the inferred types (from [`Self::infer_types`]) for the columns they name.
    /// Overrides ([`Self::set_type_override`]) stay as they are.
    pub fn set_types(&mut self, types: ColumnTypes) {
        self.types = types.into_iter().collect();
    }

    /// Whether column types were inferred ([`Self::set_types`]) and are still current.
    pub fn has_types(&self) -> bool {
        !self.types.is_empty()
    }

    /// The type sort and filter use for the column shown at `col`: the user's override
    /// (TYPE-2), else the inferred type; `None` until inferred, and for columns inserted
    /// since that have no override.
    pub fn column_type(&self, col: crate::Col) -> Option<InferredType> {
        self.type_of(self.col_id(col))
    }

    pub(crate) fn type_of(&self, id: crate::ColId) -> Option<InferredType> {
        self.type_overrides
            .get(&id)
            .or_else(|| self.types.get(&id))
            .copied()
    }

    /// The inferred type of the column shown at `col`, whatever the override.
    pub fn inferred_type(&self, col: crate::Col) -> Option<InferredType> {
        self.types.get(&self.col_id(col)).copied()
    }

    /// The type the user set for the column shown at `col`, if any.
    pub fn type_override(&self, col: crate::Col) -> Option<InferredType> {
        self.type_overrides.get(&self.col_id(col)).copied()
    }

    /// Set (or with `None`, drop) the user's type for the column shown at `col` (TYPE-2).
    /// It changes only how sort and filter read the column: values, display, and saves
    /// stay as they are. It stays with its column, and through header flips and new
    /// inferences; a sort or filter already done is not redone.
    pub fn set_type_override(&mut self, col: crate::Col, ty: Option<InferredType>) {
        let id = self.col_id(col);
        match ty {
            Some(t) => self.type_overrides.insert(id, t),
            None => self.type_overrides.remove(&id),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use InferredType::*;

    #[test]
    fn values_and_their_types() {
        let cases: &[(&str, Option<InferredType>)] = &[
            ("", None),
            ("NA", None),
            ("n/a", None),
            ("NULL", None),
            ("-", None),
            ("0", Some(Integer)),
            ("-12", Some(Integer)),
            ("+3", Some(Integer)),
            ("12345678901234567890", Some(Integer)),
            ("00123", Some(Text)),
            ("-007", Some(Text)),
            ("0.5", Some(Decimal)),
            ("-2.25", Some(Decimal)),
            ("1e6", Some(Decimal)),
            ("1.5E-3", Some(Decimal)),
            ("00.5", Some(Text)),
            (".5", Some(Text)),
            ("1.", Some(Text)),
            ("1e", Some(Text)),
            ("1,234", Some(Text)),
            ("3,5", Some(Text)),
            (" 12", Some(Text)),
            ("--1", Some(Text)),
            ("true", Some(Boolean)),
            ("FALSE", Some(Boolean)),
            ("Yes", Some(Boolean)),
            ("no", Some(Boolean)),
            ("t", Some(Text)),
            ("2026-04-05", Some(Date)),
            ("2024-02-29", Some(Date)),
            ("2026-02-29", Some(Text)),
            ("2026-13-01", Some(Text)),
            ("2026-4-5", Some(Text)),
            ("05/04/2026", Some(Text)),
            ("2026-04-05T10:30", Some(DateTime)),
            ("2026-04-05 10:30:15", Some(DateTime)),
            ("2026-04-05T10:30:15.250Z", Some(DateTime)),
            ("2026-04-05T10:30:15+02:00", Some(DateTime)),
            ("2026-04-05T24:00", Some(Text)),
            ("2026-04-05T10:60", Some(Text)),
            ("2026-04-05T10:30:15.", Some(Text)),
            ("2026-04-05T10:30+0200", Some(Text)),
            ("2026-04-05x", Some(Text)),
            ("hello", Some(Text)),
        ];
        for &(raw, want) in cases {
            assert_eq!(value_type(raw), want, "{raw:?}");
        }
    }

    #[test]
    fn column_types_combine() {
        assert_eq!(combine(Integer, Decimal), Decimal);
        assert_eq!(combine(Decimal, Integer), Decimal);
        assert_eq!(combine(Date, DateTime), DateTime);
        assert_eq!(combine(Integer, Boolean), Text);
        assert_eq!(combine(Date, Integer), Text);
        assert_eq!(combine(Text, Integer), Text);
    }
}
