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

use crate::{ColId, CsvTable, InferredType, RowBlock, TableSource};
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

fn is_null(raw: &str) -> bool {
    raw == "-"
        || ["na", "n/a", "null"]
            .iter()
            .any(|n| raw.eq_ignore_ascii_case(n))
}

fn is_boolean(raw: &str) -> bool {
    ["true", "false", "yes", "no"]
        .iter()
        .any(|b| raw.eq_ignore_ascii_case(b))
}

/// `Some(has fraction or exponent)` if `raw` is a plain number: an optional sign, `0` or
/// digits not starting with `0`, then optionally `.digits` and `e[+-]digits`.
fn number(raw: &str) -> Option<bool> {
    let b = raw.strip_prefix(['+', '-']).unwrap_or(raw).as_bytes();
    let int = b.iter().take_while(|c| c.is_ascii_digit()).count();
    if int == 0 || (int > 1 && b[0] == b'0') {
        return None;
    }
    let mut i = int;
    let mut fraction = false;
    if b.get(i) == Some(&b'.') {
        let digits = b[i + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 {
            return None;
        }
        i += 1 + digits;
        fraction = true;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let digits = b[i..].iter().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 {
            return None;
        }
        i += digits;
        fraction = true;
    }
    (i == b.len()).then_some(fraction)
}

/// `n` ASCII digits at the start of `b`, as a number.
fn digits(b: &[u8], n: usize) -> Option<u32> {
    let d = b.get(..n)?;
    d.iter()
        .all(u8::is_ascii_digit)
        .then(|| d.iter().fold(0, |v, c| v * 10 + u32::from(c - b'0')))
}

/// A valid ISO date `YYYY-MM-DD` at the start of `raw`; returns what follows it.
fn date(raw: &str) -> Option<&str> {
    let b = raw.as_bytes();
    let (year, month, day) = (
        digits(b, 4)?,
        digits(b.get(5..)?, 2)?,
        digits(b.get(8..)?, 2)?,
    );
    if b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    (1..=days).contains(&day).then(|| &raw[10..])
}

/// After a date: `T` or a space, `HH:MM`, optional `:SS` and `.fraction`, optional `Z` or
/// `+HH:MM`/`-HH:MM`.
fn time(rest: &str) -> bool {
    let b = rest.as_bytes();
    if !matches!(b.first(), Some(b'T' | b' ')) {
        return false;
    }
    let b = &b[1..];
    let hm = |b: &[u8]| -> bool {
        matches!((digits(b, 2), b.get(2), digits(b.get(3..).unwrap_or(&[]), 2)),
            (Some(h), Some(b':'), Some(m)) if h < 24 && m < 60)
    };
    if !hm(b) {
        return false;
    }
    let mut i = 5;
    if b.get(i) == Some(&b':') {
        match digits(&b[i + 1..], 2) {
            Some(s) if s < 60 => i += 3,
            _ => return false,
        }
        if b.get(i) == Some(&b'.') {
            let n = b[i + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
            if n == 0 {
                return false;
            }
            i += 1 + n;
        }
    }
    match &b[i..] {
        [] | [b'Z'] => true,
        [b'+' | b'-', zone @ ..] => zone.len() == 5 && hm(zone),
        _ => false,
    }
}

/// Microseconds since 0000-03-01T00:00Z of a value [`value_type`] calls a Date or a
/// DateTime, for sorting (SORT-1): a date is its midnight, a time without a zone is taken
/// as UTC, and fractions finer than a microsecond are dropped.
pub(crate) fn instant(raw: &str) -> Option<i64> {
    let rest = date(raw)?;
    let b = raw.as_bytes();
    let (y, m, d) = (digits(b, 4)?, digits(&b[5..], 2)?, digits(&b[8..], 2)?);
    let (y, m, d) = (i64::from(y), i64::from(m), i64::from(d));
    // Days since 0000-03-01 (H. Hinnant's days_from_civil, years starting in March).
    let (y, m) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let days =
        y * 365 + y.div_euclid(4) - y.div_euclid(100) + y.div_euclid(400) + (153 * m + 2) / 5 + d
            - 1;
    let mut micros = days * 86_400_000_000;
    if rest.is_empty() {
        return Some(micros);
    }
    if !time(rest) {
        return None;
    }
    let t = &rest.as_bytes()[1..];
    let minutes = |b: &[u8]| Some(i64::from(digits(b, 2)? * 60 + digits(&b[3..], 2)?));
    micros += minutes(t)? * 60_000_000;
    let mut i = 5;
    if t.get(i) == Some(&b':') {
        micros += i64::from(digits(&t[i + 1..], 2)?) * 1_000_000;
        i += 3;
        if t.get(i) == Some(&b'.') {
            let n = t[i + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
            let mut frac = 0;
            for k in 0..6 {
                frac = frac * 10
                    + t.get(i + 1 + k)
                        .filter(|_| k < n)
                        .map_or(0, |c| i64::from(c - b'0'));
            }
            micros += frac;
            i += 1 + n;
        }
    }
    match t.get(i) {
        Some(b'+') => micros -= minutes(&t[i + 1..])? * 60_000_000,
        Some(b'-') => micros += minutes(&t[i + 1..])? * 60_000_000,
        _ => {}
    }
    Some(micros)
}

impl CsvTable {
    /// Infer every column's type from a sample: the first [`SAMPLE_HEAD`] rows and
    /// [`SAMPLE_SPREAD`] more at even steps through the rest, the last row included. Reads
    /// cells as shown (edits included). Run it once the table is indexed, off the UI thread
    /// (on a snapshot); `None` once `cancel` is set.
    pub fn infer_types(&mut self, cancel: &AtomicBool) -> Option<ColumnTypes> {
        let rows = self.row_count();
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
            self.read_rows(r..r + 1, &mut block);
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
    pub fn set_types(&mut self, types: ColumnTypes) {
        self.types = types.into_iter().collect();
    }

    /// The inferred type of the column shown at `col`; `None` until inferred, and for
    /// columns inserted since.
    pub fn column_type(&self, col: crate::Col) -> Option<InferredType> {
        self.types.get(&self.col_id(col)).copied()
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
