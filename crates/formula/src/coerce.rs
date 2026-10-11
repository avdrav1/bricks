//! Coercion between value types, and comparison, following Excel's type rules with
//! LibreOffice's arithmetic (approximate equality, cancellation to 0). The LibreOffice
//! fixture (`tests/data/cases-libreoffice.tsv`) is the oracle.

use crate::ast::ErrorKind;
use crate::scalar;
use crate::value::{number_text, Value};
use std::cmp::Ordering;

/// A value as a number: booleans are 1/0, empty is 0, text goes through
/// [`text_to_number`], errors propagate.
pub fn to_number(v: &Value) -> Result<f64, ErrorKind> {
    match v {
        Value::Number(n) | Value::Date(n) => Ok(*n),
        Value::Bool(b) => Ok(f64::from(u8::from(*b))),
        Value::Empty => Ok(0.0),
        Value::Text(t) => text_to_number(t).ok_or(ErrorKind::Value),
        Value::Error(e) => Err(*e),
        Value::Array(_) => to_number(v.scalar()),
    }
}

/// Text that reads as a number in arithmetic, as LibreOffice reads it: spaces around it, a
/// sign (spaces may follow it), a trailing `-`, or parentheses for negative, `$`, thousands separators in groups of three,
/// leading zeros, `.5`, an exponent, a trailing `%`, a time `H:MM[:SS[.f]]`, or an ISO
/// date or date-time (its serial number).
pub fn text_to_number(text: &str) -> Option<f64> {
    let t = text.trim_matches(' ');
    if t.is_empty() {
        return None;
    }
    if let Some(serial) = scalar::iso_serial(t) {
        return Some(serial);
    }
    if let Some(time) = time_of_day(t) {
        return Some(time);
    }
    let (t, paren) = match t.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
        Some(inner) => (inner, true),
        None => (t, false),
    };
    let (t, percent) = match t.strip_suffix('%') {
        Some(r) => (r.trim_end_matches(' '), true),
        None => (t, false),
    };
    let (negative, t) = match t.as_bytes().first() {
        Some(b'-') => (true, t[1..].trim_start_matches(' ')),
        Some(b'+') => (false, t[1..].trim_start_matches(' ')),
        _ => match t.strip_suffix('-') {
            Some(r) => (true, r),
            None => (false, t),
        },
    };
    if paren && negative {
        return None;
    }
    let t = t.strip_prefix('$').unwrap_or(t);
    let b = t.as_bytes();
    let int_end = b
        .iter()
        .position(|c| !(c.is_ascii_digit() || *c == b','))
        .unwrap_or(b.len());
    let int = &t[..int_end];
    if int.contains(',') {
        // Thousands separators: 1 to 3 digits, then groups of exactly 3.
        let mut groups = int.split(',');
        let first = groups.next().expect("split yields a first part");
        if !(1..=3).contains(&first.len()) || groups.any(|g| g.len() != 3) {
            return None;
        }
    }
    let mut digits: String = int.chars().filter(|c| *c != ',').collect();
    let mut i = int_end;
    let int_digits = digits.len();
    if i < b.len() && b[i] == b'.' {
        digits.push('.');
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            digits.push(b[i] as char);
            i += 1;
        }
    }
    if int_digits == 0 && digits.len() <= 1 {
        return None;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let start = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j == start {
            return None;
        }
        digits.push_str(std::str::from_utf8(&b[i..j]).ok()?);
        i = j;
    }
    if i != b.len() {
        return None;
    }
    let mut n: f64 = digits.parse().ok()?;
    if percent {
        n /= 100.0;
    }
    if negative || paren {
        n = -n;
    }
    n.is_finite().then_some(n)
}

/// `H:MM`, `H:MM:SS`, or `H:MM:SS.f` as a fraction of a day.
fn time_of_day(t: &str) -> Option<f64> {
    let mut parts = t.split(':');
    let h = parts.next()?;
    let m = parts.next()?;
    let s = parts.next();
    if parts.next().is_some()
        || h.is_empty()
        || h.len() > 2
        || m.len() != 2
        || !h.bytes().chain(m.bytes()).all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let (h, m): (f64, f64) = (h.parse().ok()?, m.parse().ok()?);
    let s: f64 = match s {
        None => 0.0,
        Some(s) => {
            let (whole, frac) = s.split_once('.').unwrap_or((s, ""));
            if whole.len() != 2
                || !whole
                    .bytes()
                    .chain(frac.bytes())
                    .all(|c| c.is_ascii_digit())
            {
                return None;
            }
            s.parse().ok()?
        }
    };
    (m < 60.0 && s < 60.0).then(|| (h * 3_600.0 + m * 60.0 + s) / 86_400.0)
}

/// A value as text: numbers as [`number_text`] (a date is its serial number), booleans as
/// `TRUE`/`FALSE`, empty as `""`.
pub fn to_text(v: &Value) -> Result<String, ErrorKind> {
    match v {
        Value::Empty => Ok(String::new()),
        Value::Number(n) | Value::Date(n) => Ok(number_text(*n)),
        Value::Text(t) => Ok(t.to_string()),
        Value::Bool(b) => Ok((if *b { "TRUE" } else { "FALSE" }).to_string()),
        Value::Error(e) => Err(*e),
        Value::Array(_) => to_text(v.scalar()),
    }
}

/// A value as a boolean: numbers are true when non-zero, `TRUE`/`FALSE` text reads as
/// itself, empty is false, other text is `#VALUE!`.
pub fn to_bool(v: &Value) -> Result<bool, ErrorKind> {
    match v {
        Value::Number(n) | Value::Date(n) => Ok(*n != 0.0),
        Value::Bool(b) => Ok(*b),
        Value::Empty => Ok(false),
        Value::Text(t) if t.eq_ignore_ascii_case("TRUE") => Ok(true),
        Value::Text(t) if t.eq_ignore_ascii_case("FALSE") => Ok(false),
        Value::Text(_) => Err(ErrorKind::Value),
        Value::Error(e) => Err(*e),
        Value::Array(_) => to_bool(v.scalar()),
    }
}

/// 2^-48: numbers this close, relative to both, are equal (LibreOffice's `approxEqual`).
const EPSILON: f64 = 1.0 / 281_474_976_710_656.0;

/// Whether two numbers are equal to about 15 significant digits.
pub fn approx_equal(a: f64, b: f64) -> bool {
    if a == b {
        return true;
    }
    if a == 0.0 || b == 0.0 {
        return false;
    }
    let d = (a - b).abs();
    d < a.abs() * EPSILON && d < b.abs() * EPSILON
}

/// `a + b`, exactly 0 when the two cancel to within rounding (`0.1+0.2-0.3` is 0).
pub fn approx_add(a: f64, b: f64) -> f64 {
    if ((a < 0.0 && b > 0.0) || (a > 0.0 && b < 0.0)) && approx_equal(a, -b) {
        0.0
    } else {
        a + b
    }
}

fn rank(v: &Value) -> u8 {
    match v {
        Value::Number(_) | Value::Date(_) => 0,
        Value::Text(_) => 1,
        Value::Bool(_) => 2,
        _ => 0,
    }
}

/// Orders two non-error scalars: numbers (and dates) before text before booleans; text
/// ignores case; empty compares as 0, `""`, or FALSE against the other side's type.
pub fn compare(a: &Value, b: &Value) -> Ordering {
    let zero = |other: &Value| match other {
        Value::Text(_) => Value::text(""),
        Value::Bool(_) => Value::Bool(false),
        _ => Value::Number(0.0),
    };
    match (a.scalar(), b.scalar()) {
        (Value::Empty, Value::Empty) => Ordering::Equal,
        (Value::Empty, b) => compare(&zero(b), b),
        (a, Value::Empty) => compare(a, &zero(a)),
        (Value::Number(x) | Value::Date(x), Value::Number(y) | Value::Date(y)) => {
            if approx_equal(*x, *y) {
                Ordering::Equal
            } else {
                x.partial_cmp(y).unwrap_or(Ordering::Equal)
            }
        }
        (Value::Text(x), Value::Text(y)) => compare_text(x, y),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (a, b) => rank(a).cmp(&rank(b)),
    }
}

/// Case-insensitive text order.
pub fn compare_text(a: &str, b: &str) -> Ordering {
    if a.is_ascii() && b.is_ascii() {
        let (a, b) = (a.as_bytes(), b.as_bytes());
        return a
            .iter()
            .map(u8::to_ascii_lowercase)
            .cmp(b.iter().map(u8::to_ascii_lowercase));
    }
    a.chars()
        .flat_map(char::to_lowercase)
        .cmp(b.chars().flat_map(char::to_lowercase))
}
