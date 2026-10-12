//! A cell as worksheet XML: typed values when their text is canonical (so reading the
//! file back gives the same raw text, invariant 1), formulas with their results, strings
//! inline.

use crate::value_text;
use calamine::DataRef;
use data_model::{is_formula, StyleId, StylePool};
use formula::{literal, Value};
use std::collections::HashMap;
use std::fmt::Write as _;

/// `text` escaped for XML content or an attribute value; characters XML can't carry are
/// written as Excel's `_xHHHH_`, and a literal `_xHHHH_` is protected the same way.
pub(crate) fn escape(text: &str, out: &mut String) {
    let bytes = text.as_bytes();
    for (i, c) in text.char_indices() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {
                let _ = write!(out, "_x{:04X}_", c as u32);
            }
            '_' if looks_escaped(&bytes[i..]) => out.push_str("_x005F_"),
            c => out.push(c),
        }
    }
}

/// `_xHHHH_` at the start of `b`.
fn looks_escaped(b: &[u8]) -> bool {
    b.len() >= 7 && b[1] == b'x' && b[2..6].iter().all(u8::is_ascii_hexdigit) && b[6] == b'_'
}

/// `B12` for row 11, column 1.
pub(crate) fn cell_name(row: u64, col: u32, out: &mut String) {
    formula::push_column(col, out);
    let _ = write!(out, "{}", row + 1);
}

/// Styles a save adds as it writes: a date cell that has no date format of its own gets
/// one (the one import reads back as the same ISO text).
pub(crate) struct CellStyles<'a> {
    pub pool: &'a mut StylePool,
    dates: HashMap<(StyleId, bool), StyleId>,
}

impl<'a> CellStyles<'a> {
    pub fn new(pool: &'a mut StylePool) -> Self {
        Self {
            pool,
            dates: HashMap::new(),
        }
    }

    fn date(&mut self, style: StyleId, time: bool) -> StyleId {
        if self.pool.xf(style).num_fmt != 0 {
            return style; // the cell's own format shows it
        }
        let pool = &mut *self.pool;
        *self.dates.entry((style, time)).or_insert_with(|| {
            let code = if time {
                "yyyy-mm-dd hh:mm:ss"
            } else {
                "yyyy-mm-dd"
            };
            pool.with_num_fmt(style, code)
        })
    }
}

/// Write the cell at `row`, `col` with raw text `text` (may be empty: a styled empty cell),
/// style `style`, and, for a formula, the result text `result`.
pub(crate) fn write_cell(
    out: &mut String,
    row: u64,
    col: u32,
    text: &str,
    style: StyleId,
    result: Option<&str>,
    styles: &mut CellStyles<'_>,
) {
    out.push_str("<c r=\"");
    cell_name(row, col, out);
    out.push('"');
    let s = |out: &mut String, style: StyleId| {
        if style != StyleId(0) {
            let _ = write!(out, " s=\"{}\"", style.0);
        }
    };
    if text.is_empty() {
        s(out, style);
        out.push_str("/>");
        return;
    }
    if is_formula(text) {
        if let Ok(expr) = formula::parse(&text[1..]) {
            s(out, style);
            let result = result.unwrap_or("");
            let (kind, value) = match literal(result) {
                Value::Empty => ("", String::new()),
                Value::Number(n) | Value::Date(n) => ("", format!("{n}")),
                Value::Bool(b) => (" t=\"b\"", u8::from(b).to_string()),
                Value::Error(e) => (" t=\"e\"", e.text().to_owned()),
                _ => (" t=\"str\"", result.to_owned()),
            };
            out.push_str(kind);
            out.push_str("><f>");
            escape(&formula::render_xlsx(&expr), out);
            out.push_str("</f>");
            if !value.is_empty() || kind == " t=\"str\"" {
                out.push_str("<v>");
                escape(&value, out);
                out.push_str("</v>");
            }
            out.push_str("</c>");
            return;
        }
    }
    match literal(text) {
        Value::Number(n) if value_text(&DataRef::Float(n)) == text => {
            s(out, style);
            out.push_str("><v>");
            out.push_str(text);
            out.push_str("</v></c>");
        }
        Value::Bool(b) if text == "TRUE" || text == "FALSE" => {
            s(out, style);
            let _ = write!(out, " t=\"b\"><v>{}</v></c>", u8::from(b));
        }
        Value::Date(serial) if canonical_date(text).is_some() => {
            let time = text.len() == 19;
            // Excel counts a 29 February 1900 that never was: before March, its serials
            // are one less than Brick's (whose epoch is LibreOffice's).
            let serial = if serial < 61.0 { serial - 1.0 } else { serial };
            s(out, styles.date(style, time));
            let _ = write!(out, "><v>{serial}</v></c>");
        }
        _ => {
            s(out, style);
            out.push_str(" t=\"inlineStr\"><is><t xml:space=\"preserve\">");
            escape(text, out);
            out.push_str("</t></is></c>");
        }
    }
}

/// `YYYY-MM-DD` or `YYYY-MM-DD HH:MM:SS` exactly, years 1900 to 9999, not midnight in the
/// long form: the forms import gives back for a date cell.
fn canonical_date(text: &str) -> Option<()> {
    let b = text.as_bytes();
    let digits = |r: std::ops::Range<usize>| -> Option<u16> {
        b[r].iter().try_fold(0u16, |n, &d| {
            d.is_ascii_digit().then(|| n * 10 + u16::from(d - b'0'))
        })
    };
    if !(b.len() == 10 || b.len() == 19) || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let y = digits(0..4)?;
    let (m, d) = (digits(5..7)?, digits(8..10)?);
    if !(1900..=9999).contains(&y) || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    if b.len() == 10 {
        return Some(());
    }
    if b[10] != b' ' || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let (h, mi, s) = (digits(11..13)?, digits(14..16)?, digits(17..19)?);
    if (h, mi, s) == (0, 0, 0) || h > 23 || mi > 59 || s > 59 {
        return None; // import shows midnight as the date alone
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(text: &str, result: Option<&str>) -> (String, StylePool) {
        let mut pool = StylePool::default();
        let mut out = String::new();
        write_cell(
            &mut out,
            1,
            2,
            text,
            StyleId(0),
            result,
            &mut CellStyles::new(&mut pool),
        );
        (out, pool)
    }

    #[test]
    fn values_are_typed_only_when_their_text_is_canonical() {
        assert_eq!(cell("12.5", None).0, r#"<c r="C2"><v>12.5</v></c>"#);
        assert_eq!(
            cell("00123", None).0,
            r#"<c r="C2" t="inlineStr"><is><t xml:space="preserve">00123</t></is></c>"#
        );
        assert_eq!(cell("TRUE", None).0, r#"<c r="C2" t="b"><v>1</v></c>"#);
        let (date, pool) = cell("2026-03-04", None);
        assert_eq!(date, r#"<c r="C2" s="1"><v>46085</v></c>"#);
        assert_eq!(pool.custom_format(pool.xfs[1].num_fmt), Some("yyyy-mm-dd"));
        assert_eq!(
            cell("=SUM(A1:A2)", Some("3")).0,
            r#"<c r="C2"><f>SUM(A1:A2)</f><v>3</v></c>"#
        );
        assert_eq!(
            cell("=A1&\"x\"", Some("ax")).0,
            r#"<c r="C2" t="str"><f>A1&amp;&quot;x&quot;</f><v>ax</v></c>"#
        );
        assert_eq!(
            cell("=1/0", Some("#DIV/0!")).0,
            r#"<c r="C2" t="e"><f>1/0</f><v>#DIV/0!</v></c>"#
        );
    }

    #[test]
    fn text_xml_cant_carry_is_escaped_as_excel_does() {
        let mut out = String::new();
        escape("a<b>&\"\u{1}_x0041_", &mut out);
        assert_eq!(out, "a&lt;b&gt;&amp;&quot;_x0001__x005F_x0041_");
    }
}
