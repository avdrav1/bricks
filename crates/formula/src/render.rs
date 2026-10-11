//! [`Expr`] back to canonical formula text, without the leading `=`: uppercase function
//! names and column letters, `$` kept, numbers in shortest round-trip form, no spaces, the
//! user's parentheses reproduced.

use crate::ast::{Area, Expr, Reference, UnaryOp};
use crate::refs::push_column;
use std::fmt::Write;

/// Functions newer than Excel 2007, which XLSX files must name with an `_xlfn.` prefix;
/// and BAHTTEXT, which LibreOffice reads from XLSX only with the prefix (and writes so).
const XLFN: [&str; 17] = [
    "BAHTTEXT",
    "CONCAT",
    "DAYS",
    "IFNA",
    "IFS",
    "MAXIFS",
    "MINIFS",
    "MODE.SNGL",
    "STDEV.P",
    "STDEV.S",
    "SWITCH",
    "TEXTJOIN",
    "VAR.P",
    "VAR.S",
    "XLOOKUP",
    "XMATCH",
    "XOR",
];

/// Canonical text of `expr`.
pub fn render(expr: &Expr) -> String {
    let mut out = String::new();
    write_expr(&mut out, expr, false);
    out
}

/// Canonical text of `expr` as an XLSX file stores it: newer functions get `_xlfn.`.
pub fn render_xlsx(expr: &Expr) -> String {
    let mut out = String::new();
    write_expr(&mut out, expr, true);
    out
}

fn write_expr(out: &mut String, expr: &Expr, xlsx: bool) {
    match expr {
        Expr::Number(n) => write_number(out, *n),
        Expr::Text(t) => write_text(out, t),
        Expr::Bool(b) => out.push_str(if *b { "TRUE" } else { "FALSE" }),
        Expr::Error(k) => out.push_str(k.text()),
        Expr::Ref(r) => write_reference(out, r),
        Expr::Name(n) => out.push_str(n),
        Expr::Unary(op, x) => {
            out.push(match op {
                UnaryOp::Neg => '-',
                UnaryOp::Plus => '+',
            });
            write_expr(out, x, xlsx);
        }
        Expr::Binary(op, l, r) => {
            write_expr(out, l, xlsx);
            out.push_str(op.symbol());
            write_expr(out, r, xlsx);
        }
        Expr::Percent(x) => {
            write_expr(out, x, xlsx);
            out.push('%');
        }
        Expr::Paren(x) => {
            out.push('(');
            write_expr(out, x, xlsx);
            out.push(')');
        }
        Expr::Call(name, args) => {
            if xlsx && XLFN.contains(&name.as_str()) {
                out.push_str("_xlfn.");
            }
            out.push_str(name);
            out.push('(');
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_expr(out, a, xlsx);
            }
            out.push(')');
        }
        Expr::Array(rows) => {
            out.push('{');
            for (i, row) in rows.iter().enumerate() {
                if i > 0 {
                    out.push(';');
                }
                for (j, item) in row.iter().enumerate() {
                    if j > 0 {
                        out.push(',');
                    }
                    write_expr(out, item, xlsx);
                }
            }
            out.push('}');
        }
        Expr::Missing => {}
    }
}

/// Shortest text that parses back to `n`: plain digits, with an exponent (`1E+16`,
/// `1.5E-7`) only for very large or very small magnitudes.
pub(crate) fn write_number(out: &mut String, n: f64) {
    if !n.is_finite() {
        out.push_str("#NUM!");
    } else if n == 0.0 {
        out.push('0');
    } else if n.abs() >= 1e16 || n.abs() < 1e-6 {
        let e = format!("{n:e}");
        let (mantissa, exp) = e.split_once('e').expect("{:e} has an exponent");
        out.push_str(mantissa);
        out.push('E');
        if !exp.starts_with('-') {
            out.push('+');
        }
        out.push_str(exp);
    } else {
        let _ = write!(out, "{n}");
    }
}

fn write_text(out: &mut String, t: &str) {
    out.push('"');
    for c in t.chars() {
        if c == '"' {
            out.push('"');
        }
        out.push(c);
    }
    out.push('"');
}

/// Whether a sheet name must be quoted in a reference: anything but a plain identifier,
/// and identifiers that read as a cell (`A1`).
fn needs_quotes(name: &str) -> bool {
    let mut chars = name.chars();
    let plain = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
    if !plain {
        return true;
    }
    let letters = name.chars().take_while(char::is_ascii_alphabetic).count();
    let rest = &name[letters..];
    (1..=3).contains(&letters) && !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
}

pub(crate) fn write_sheet(out: &mut String, name: &str) {
    if needs_quotes(name) {
        out.push('\'');
        for c in name.chars() {
            if c == '\'' {
                out.push('\'');
            }
            out.push(c);
        }
        out.push('\'');
    } else {
        out.push_str(name);
    }
}

fn write_col(out: &mut String, col: u32, abs: bool) {
    if abs {
        out.push('$');
    }
    push_column(col, out);
}

fn write_row(out: &mut String, row: u64, abs: bool) {
    if abs {
        out.push('$');
    }
    let _ = write!(out, "{}", row + 1);
}

pub(crate) fn write_reference(out: &mut String, r: &Reference) {
    if let Some(sheet) = &r.sheet {
        write_sheet(out, sheet);
        out.push('!');
    }
    match &r.area {
        Area::Cell(a) => {
            write_col(out, a.col, a.col_abs);
            write_row(out, a.row, a.row_abs);
        }
        Area::Range(a, b) => {
            write_col(out, a.col, a.col_abs);
            write_row(out, a.row, a.row_abs);
            out.push(':');
            write_col(out, b.col, b.col_abs);
            write_row(out, b.row, b.row_abs);
        }
        Area::Cols {
            first,
            last,
            first_abs,
            last_abs,
        } => {
            write_col(out, *first, *first_abs);
            out.push(':');
            write_col(out, *last, *last_abs);
        }
        Area::Rows {
            first,
            last,
            first_abs,
            last_abs,
        } => {
            write_row(out, *first, *first_abs);
            out.push(':');
            write_row(out, *last, *last_abs);
        }
    }
}
