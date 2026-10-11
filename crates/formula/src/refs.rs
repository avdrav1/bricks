//! Column letters, name-box references, and how references follow structure changes
//! (inserted and deleted rows and columns, moved formulas, renamed and deleted sheets).

use crate::ast::{Area, CellAddr, ErrorKind, Expr, Reference};
use crate::render::write_reference;

/// Spreadsheet column name: 0 -> A, 25 -> Z, 26 -> AA.
pub fn column_name(col: u32, out: &mut String) {
    out.clear();
    push_column(col, out);
}

/// Appends the column name of `col` to `out`.
pub(crate) fn push_column(mut col: u32, out: &mut String) {
    let mut letters = [0u8; 7];
    let mut i = letters.len();
    loop {
        i -= 1;
        letters[i] = b'A' + (col % 26) as u8;
        if col < 26 {
            break;
        }
        col = col / 26 - 1;
    }
    out.push_str(std::str::from_utf8(&letters[i..]).expect("ASCII letters"));
}

/// The column index of `letters` (any case): A -> 0, AA -> 26.
pub fn column_index(letters: &str) -> Option<u32> {
    if letters.is_empty() || letters.len() > 7 {
        return None;
    }
    let mut n: u64 = 0;
    for c in letters.bytes() {
        if !c.is_ascii_alphabetic() {
            return None;
        }
        n = n * 26 + u64::from(c.to_ascii_uppercase() - b'A') + 1;
    }
    u32::try_from(n - 1).ok()
}

/// A reference typed in the name box: `C5`, `B2:D9`, `A:C`, `3:5`, `Sheet2!B3`,
/// `'Q1 Sales'!A1`; spaces around it and letter case don't matter.
pub fn parse_reference(text: &str) -> Option<Reference> {
    match crate::parse(text.trim()) {
        Ok(Expr::Ref(r)) => Some(r),
        _ => None,
    }
}

/// `B12`, `B12:D20`, `'Q1 Sales'!A1`.
pub fn format_reference(r: &Reference) -> String {
    let mut out = String::new();
    write_reference(&mut out, r);
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Rows,
    Cols,
}

/// A change that references must follow. Row positions count all file rows and column
/// positions are view columns, both 0-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change<'a> {
    Insert {
        sheet: &'a str,
        axis: Axis,
        at: u64,
        count: u64,
    },
    Delete {
        sheet: &'a str,
        axis: Axis,
        at: u64,
        count: u64,
    },
    /// The formula itself moved (paste, sort): relative parts follow it.
    Shift {
        rows: i64,
        cols: i64,
    },
    RenameSheet {
        from: &'a str,
        to: &'a str,
    },
    DeleteSheet {
        name: &'a str,
    },
}

/// Adjusts every reference in `expr`, which sits on sheet `own_sheet`, for `change`.
/// A reference that no longer points anywhere becomes `#REF!`. Returns whether anything
/// changed.
pub fn adjust(expr: &mut Expr, own_sheet: &str, change: &Change) -> bool {
    let mut changed = false;
    expr.visit_mut(&mut |e| {
        let Expr::Ref(r) = e else { return };
        match adjust_ref(r, own_sheet, change) {
            Outcome::Same => {}
            Outcome::Changed => changed = true,
            Outcome::Gone => {
                *e = Expr::Error(ErrorKind::Ref);
                changed = true;
            }
        }
    });
    changed
}

/// Whether `expr` names sheet `name` explicitly (ignoring case).
pub fn names_sheet(expr: &Expr, name: &str) -> bool {
    let mut found = false;
    expr.visit(&mut |e| {
        if let Expr::Ref(Reference { sheet: Some(s), .. }) = e {
            found |= same_sheet(s, name);
        }
    });
    found
}

fn same_sheet(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b) || a.to_lowercase() == b.to_lowercase()
}

enum Outcome {
    Same,
    Changed,
    Gone,
}

fn adjust_ref(r: &mut Reference, own_sheet: &str, change: &Change) -> Outcome {
    let before = r.area.clone();
    match *change {
        Change::RenameSheet { from, to } => {
            return match &mut r.sheet {
                Some(s) if same_sheet(s, from) => {
                    *s = to.to_string();
                    Outcome::Changed
                }
                _ => Outcome::Same,
            };
        }
        Change::DeleteSheet { name } => {
            return match &r.sheet {
                Some(s) if same_sheet(s, name) => Outcome::Gone,
                _ => Outcome::Same,
            };
        }
        Change::Insert {
            sheet,
            axis,
            at,
            count,
        }
        | Change::Delete {
            sheet,
            axis,
            at,
            count,
        } => {
            let on = match &r.sheet {
                Some(s) => same_sheet(s, sheet),
                None => same_sheet(own_sheet, sheet),
            };
            if !on {
                return Outcome::Same;
            }
            let insert = matches!(change, Change::Insert { .. });
            let point = |p: u64| -> Option<u64> {
                if insert {
                    Some(if p >= at { p + count } else { p })
                } else if p >= at + count {
                    Some(p - count)
                } else if p >= at {
                    None
                } else {
                    Some(p)
                }
            };
            let span = |s: u64, e: u64| -> Option<(u64, u64)> {
                if insert {
                    return Some((point(s)?, point(e)?));
                }
                let ns = if s >= at + count {
                    s - count
                } else if s >= at {
                    at
                } else {
                    s
                };
                let ne = if e >= at + count {
                    e as i128 - count as i128
                } else if e >= at {
                    at as i128 - 1
                } else {
                    e as i128
                };
                (ne >= ns as i128).then_some((ns, ne as u64))
            };
            let ok = match (&mut r.area, axis) {
                (Area::Cell(a), Axis::Rows) => point(a.row).map(|p| a.row = p).is_some(),
                (Area::Cell(a), Axis::Cols) => {
                    point(u64::from(a.col)).map(|p| a.col = p as u32).is_some()
                }
                (Area::Range(a, b), Axis::Rows) => span(a.row, b.row)
                    .map(|(s, e)| (a.row, b.row) = (s, e))
                    .is_some(),
                (Area::Range(a, b), Axis::Cols) => span(u64::from(a.col), u64::from(b.col))
                    .map(|(s, e)| (a.col, b.col) = (s as u32, e as u32))
                    .is_some(),
                (Area::Rows { first, last, .. }, Axis::Rows) => span(*first, *last)
                    .map(|(s, e)| (*first, *last) = (s, e))
                    .is_some(),
                (Area::Cols { first, last, .. }, Axis::Cols) => {
                    span(u64::from(*first), u64::from(*last))
                        .map(|(s, e)| (*first, *last) = (s as u32, e as u32))
                        .is_some()
                }
                (Area::Rows { .. }, Axis::Cols) | (Area::Cols { .. }, Axis::Rows) => true,
            };
            if !ok {
                return Outcome::Gone;
            }
        }
        Change::Shift { rows, cols } => {
            let ok = match &mut r.area {
                Area::Cell(a) => shift_cell(a, rows, cols),
                Area::Range(a, b) => shift_cell(a, rows, cols) && shift_cell(b, rows, cols),
                Area::Cols {
                    first,
                    last,
                    first_abs,
                    last_abs,
                } => shift32(first, *first_abs, cols) && shift32(last, *last_abs, cols),
                Area::Rows {
                    first,
                    last,
                    first_abs,
                    last_abs,
                } => shift64(first, *first_abs, rows) && shift64(last, *last_abs, rows),
            };
            if !ok {
                return Outcome::Gone;
            }
        }
    }
    if r.area == before {
        Outcome::Same
    } else {
        Outcome::Changed
    }
}

fn shift64(p: &mut u64, abs: bool, by: i64) -> bool {
    if abs {
        return true;
    }
    match p.checked_add_signed(by) {
        Some(v) if v < crate::parse::MAX_ROW => {
            *p = v;
            true
        }
        _ => false,
    }
}

fn shift32(p: &mut u32, abs: bool, by: i64) -> bool {
    if abs {
        return true;
    }
    match i64::from(*p)
        .checked_add(by)
        .and_then(|v| u32::try_from(v).ok())
    {
        Some(v) => {
            *p = v;
            true
        }
        None => false,
    }
}

fn shift_cell(a: &mut CellAddr, rows: i64, cols: i64) -> bool {
    shift64(&mut a.row, a.row_abs, rows) && shift32(&mut a.col, a.col_abs, cols)
}
