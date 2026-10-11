//! Values: what a cell's raw text reads as, what a formula produces, and how a result
//! shows.

use crate::ast::ErrorKind;
use crate::scalar;
use std::fmt::Write;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Empty,
    Number(f64),
    /// A number shown as a date: serial days since 1899-12-30, the time as the fraction.
    Date(f64),
    Text(Box<str>),
    Bool(bool),
    Error(ErrorKind),
    Array(Arc<Array>),
}

/// A rectangular block of values, row-major.
#[derive(Debug, Clone, PartialEq)]
pub struct Array {
    pub rows: usize,
    pub cols: usize,
    pub values: Vec<Value>,
}

impl Array {
    pub fn get(&self, row: usize, col: usize) -> &Value {
        &self.values[row * self.cols + col]
    }
}

impl Value {
    pub fn text(s: impl Into<Box<str>>) -> Self {
        Value::Text(s.into())
    }

    /// The value itself, or an array's top-left value (an empty array reads as Empty).
    pub fn scalar(&self) -> &Value {
        match self {
            Value::Array(a) => a.values.first().map_or(&Value::Empty, Value::scalar),
            v => v,
        }
    }

    pub fn is_number(&self) -> bool {
        matches!(self, Value::Number(_) | Value::Date(_))
    }
}

/// How a non-formula cell's raw text reads: empty, a plain number (so `00123` stays text,
/// invariant 1), `TRUE`/`FALSE` in any case, an ISO date or date-time, an exact error
/// literal, or else text.
pub fn literal(raw: &str) -> Value {
    if raw.is_empty() {
        Value::Empty
    } else if scalar::number(raw).is_some() {
        Value::Number(raw.parse().expect("number grammar parses as f64"))
    } else if raw.eq_ignore_ascii_case("TRUE") {
        Value::Bool(true)
    } else if raw.eq_ignore_ascii_case("FALSE") {
        Value::Bool(false)
    } else if let Some(serial) = scalar::iso_serial(raw) {
        Value::Date(serial)
    } else if let Some(e) = ErrorKind::parse(raw) {
        Value::Error(e)
    } else {
        Value::text(raw)
    }
}

/// How a formula result shows in a cell.
pub fn result_text(v: &Value) -> String {
    match v {
        Value::Empty => "0".to_string(),
        Value::Number(n) => number_text(*n),
        Value::Date(d) => date_text(*d),
        Value::Text(t) => t.to_string(),
        Value::Bool(b) => (if *b { "TRUE" } else { "FALSE" }).to_string(),
        Value::Error(e) => e.text().to_string(),
        Value::Array(_) => result_text(v.scalar()),
    }
}

/// A number as Excel's General format writes it: 15 significant digits; scientific
/// (`1.2345E+15`, `1E-10`) from 1E+15 up and below 1E-9.
pub fn number_text(x: f64) -> String {
    if !x.is_finite() {
        return ErrorKind::Num.text().to_string();
    }
    let y: f64 = format!("{x:.14e}").parse().expect("{:e} parses as f64");
    if y == 0.0 {
        return "0".to_string();
    }
    let a = y.abs();
    if a >= 1e15 || a < 1e-9 {
        let s = format!("{y:e}");
        let (mantissa, exp) = s.split_once('e').expect("{:e} has an exponent");
        let exp: i32 = exp.parse().expect("an integer exponent");
        let mut out = String::with_capacity(mantissa.len() + 5);
        out.push_str(mantissa);
        let _ = write!(
            out,
            "E{}{:02}",
            if exp < 0 { '-' } else { '+' },
            exp.unsigned_abs()
        );
        out
    } else {
        format!("{y}")
    }
}

/// `YYYY-MM-DD`, or `YYYY-MM-DD HH:MM:SS` when the time (rounded to the second) isn't
/// midnight; a time of day alone (integer part 0, as `TIME` gives) is `HH:MM:SS`.
pub fn date_text(serial: f64) -> String {
    if !serial.is_finite() || serial.abs() > 1e8 {
        return ErrorKind::Num.text().to_string();
    }
    let total = (serial * 86_400.0).round() as i64;
    let (days, secs) = (total.div_euclid(86_400), total.rem_euclid(86_400));
    let mut out = String::with_capacity(19);
    if days != 0 {
        let (y, m, d) = scalar::civil_from_serial(days);
        let _ = write!(out, "{y:04}-{m:02}-{d:02}");
        if secs == 0 {
            return out;
        }
        out.push(' ');
    }
    let _ = write!(
        out,
        "{:02}:{:02}:{:02}",
        secs / 3_600,
        secs / 60 % 60,
        secs % 60
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_show_as_excel_shows_them() {
        let cases: &[(Value, &str)] = &[
            (Value::Empty, "0"),
            (Value::Number(3.0), "3"),
            (Value::Number(-0.0), "0"),
            (Value::Number(0.1 + 0.2), "0.3"),
            (Value::Number(1.0 / 3.0), "0.333333333333333"),
            (Value::Number(123_456_789_012_345.0), "123456789012345"),
            (Value::Number(1e15), "1E+15"),
            (Value::Number(1.2345e15), "1.2345E+15"),
            (Value::Number(-1e20), "-1E+20"),
            (Value::Number(1e-9), "0.000000001"),
            (Value::Number(1e-10), "1E-10"),
            (Value::Number(1.5e-100), "1.5E-100"),
            (Value::Number(f64::INFINITY), "#NUM!"),
            (Value::Date(45_292.0), "2024-01-01"),
            (Value::Date(45_292.5), "2024-01-01 12:00:00"),
            (Value::Date(45_292.0 + 1.0 / 86_400.0 * 0.4), "2024-01-01"),
            (Value::Date(0.5), "12:00:00"),
            (Value::Date(0.0), "00:00:00"),
            (
                Value::Date(13.0 / 24.0 + 5.0 / 1_440.0 + 7.0 / 86_400.0),
                "13:05:07",
            ),
            (Value::Bool(true), "TRUE"),
            (Value::text("x"), "x"),
            (Value::Error(ErrorKind::NA), "#N/A"),
        ];
        for (v, want) in cases {
            assert_eq!(result_text(v), *want, "{v:?}");
        }
    }

    #[test]
    fn raw_cells_read_as_literals() {
        assert_eq!(literal(""), Value::Empty);
        assert_eq!(literal("12"), Value::Number(12.0));
        assert_eq!(literal("00123"), Value::text("00123"));
        assert_eq!(literal("true"), Value::Bool(true));
        assert_eq!(literal("yes"), Value::text("yes"));
        assert_eq!(literal("2024-01-01"), Value::Date(45_292.0));
        assert_eq!(literal("#n/a"), Value::Error(ErrorKind::NA));
        assert_eq!(literal(" 1"), Value::text(" 1"));
    }
}
