//! Lookup and reference functions (FML-7): VLOOKUP, HLOOKUP, XLOOKUP, MATCH, XMATCH,
//! INDEX, CHOOSE, ROWS, COLUMNS.
//!
//! Lookup areas are references or arrays. Exact matches compare like [`crate::Key`]: numbers
//! never equal text, text ignores case, and VLOOKUP, HLOOKUP, MATCH, and XLOOKUP's match
//! mode 2 read `*`, `?`, and `~` in text as wildcards. Approximate matches binary-search a
//! vector assumed sorted, skipping values of another type than the target. Empty cells
//! and errors in the lookup vector match nothing; an empty target finds nothing.

use super::wildcard::{has_wildcards, matches};
use super::{add, scalar, Outcome, Registry};
use crate::ast::{Area, ErrorKind, Expr};
use crate::coerce::{compare, compare_text, to_bool, to_number};
use crate::eval::{as_reference, bounds, evaluate, Context, Interrupt};
use crate::exact::ExactIndex;
use crate::value::{Array, Value};
use std::cmp::Ordering;
use std::ops::Range;
use std::sync::Arc;

/// Rows and columns of an Excel sheet: what a whole column or row counts at least.
const SHEET_ROWS: u64 = 1_048_576;
const SHEET_COLS: u64 = 16_384;

pub(super) fn register(m: &mut Registry) {
    add(m, "VLOOKUP", 3, Some(4), vlookup);
    add(m, "HLOOKUP", 3, Some(4), hlookup);
    add(m, "XLOOKUP", 3, Some(6), xlookup);
    add(m, "MATCH", 2, Some(3), match_);
    add(m, "XMATCH", 2, Some(4), xmatch);
    add(m, "INDEX", 2, Some(4), index);
    add(m, "CHOOSE", 2, None, choose);
    add(m, "ROWS", 1, Some(1), rows);
    add(m, "COLUMNS", 1, Some(1), columns);
}

/// An area a lookup reads: a reference's cells or an array's elements.
enum Table {
    Ref {
        sheet: Option<String>,
        rows: Range<u64>,
        cols: Range<u32>,
    },
    Array(Arc<Array>),
}

impl Table {
    /// The area an argument names: a reference, an array, or a scalar as a 1×1 array.
    fn of(arg: &Expr, ctx: &mut dyn Context) -> Result<Result<Self, ErrorKind>, Interrupt> {
        if let Some(r) = as_reference(arg, ctx) {
            return Ok(bounds(&r, ctx).map(|(rows, cols)| Table::Ref {
                sheet: r.sheet,
                rows,
                cols,
            }));
        }
        Ok(match evaluate(arg, ctx)? {
            Value::Array(a) => Ok(Table::Array(a)),
            Value::Error(e) => Err(e),
            v => Ok(Table::Array(Arc::new(Array {
                rows: 1,
                cols: 1,
                values: vec![v],
            }))),
        })
    }

    fn rows(&self) -> u64 {
        match self {
            Table::Ref { rows, .. } => rows.end.saturating_sub(rows.start),
            Table::Array(a) => a.rows as u64,
        }
    }

    fn cols(&self) -> u64 {
        match self {
            Table::Ref { cols, .. } => u64::from(cols.end.saturating_sub(cols.start)),
            Table::Array(a) => a.cols as u64,
        }
    }

    /// The value at (`r`, `c`), 0-based within the area.
    fn get(&self, ctx: &mut dyn Context, r: u64, c: u64) -> Result<Value, Interrupt> {
        match self {
            Table::Ref { sheet, rows, cols } => {
                ctx.cell(sheet.as_deref(), rows.start + r, cols.start + c as u32)
            }
            Table::Array(a) => Ok(a.get(r as usize, c as usize).clone()),
        }
    }

    /// Row (`across`) or column `line` of the area.
    fn line(&self, across: bool, line: u64) -> Vector<'_> {
        Vector {
            table: self,
            across,
            line,
            len: if across { self.cols() } else { self.rows() },
        }
    }

    /// The area as a vector: one column, else one row; `None` when it has both several
    /// rows and several columns.
    fn vector(&self) -> Option<Vector<'_>> {
        if self.cols() == 1 {
            Some(self.line(false, 0))
        } else if self.rows() == 1 {
            Some(self.line(true, 0))
        } else {
            None
        }
    }
}

/// One row (`across`) or column of a [`Table`].
struct Vector<'a> {
    table: &'a Table,
    across: bool,
    line: u64,
    len: u64,
}

impl Vector<'_> {
    fn get(&self, ctx: &mut dyn Context, i: u64) -> Result<Value, Interrupt> {
        if self.across {
            self.table.get(ctx, self.line, i)
        } else {
            self.table.get(ctx, i, self.line)
        }
    }

    /// The reference area this vector covers.
    fn area(&self) -> Option<(Option<&str>, Range<u64>, Range<u32>)> {
        let Table::Ref { sheet, rows, cols } = self.table else {
            return None;
        };
        let line = self.line;
        Some(if self.across {
            let r = rows.start + line;
            (sheet.as_deref(), r..r + 1, cols.clone())
        } else {
            let c = cols.start + line as u32;
            (sheet.as_deref(), rows.clone(), c..c + 1)
        })
    }

    /// Calls `f` with each non-empty value and its offset, in order, until it returns false.
    fn each(
        &self,
        ctx: &mut dyn Context,
        f: &mut dyn FnMut(u64, &Value) -> bool,
    ) -> Result<(), Interrupt> {
        match self.area() {
            Some((sheet, rows, cols)) => {
                let (r0, c0, across) = (rows.start, cols.start, self.across);
                ctx.area(sheet, rows, cols, &mut |r, c, v| {
                    f(if across { u64::from(c - c0) } else { r - r0 }, v)
                })
            }
            None => {
                for i in 0..self.len {
                    let v = self.get(ctx, i)?;
                    if !matches!(v, Value::Empty) && !f(i, &v) {
                        break;
                    }
                }
                Ok(())
            }
        }
    }

    /// The context's index of this vector, when it is a reference and the context has one.
    fn index(&self, ctx: &mut dyn Context) -> Result<Option<Arc<ExactIndex>>, Interrupt> {
        match self.area() {
            Some((sheet, rows, cols)) => ctx.exact_index(sheet, rows, cols, self.across),
            None => Ok(None),
        }
    }
}

/// The type class an approximate match compares within; empty and errors have none.
fn class(v: &Value) -> Option<u8> {
    match v {
        Value::Number(_) | Value::Date(_) => Some(0),
        Value::Text(_) => Some(1),
        Value::Bool(_) => Some(2),
        _ => None,
    }
}

/// Exact equality as [`Key`] defines it, without building keys.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x) | Value::Date(x), Value::Number(y) | Value::Date(y)) => x == y,
        (Value::Text(x), Value::Text(y)) => compare_text(x, y) == Ordering::Equal,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        _ => false,
    }
}

/// The first (or `last`) offset holding `target`; text with wildcards matches as a
/// pattern when `wildcards` is set.
fn find_exact(
    ctx: &mut dyn Context,
    v: &Vector,
    target: &Value,
    wildcards: bool,
    last: bool,
) -> Result<Option<u64>, Interrupt> {
    let mut found = None;
    match target {
        Value::Text(p) if wildcards && has_wildcards(p) => {
            v.each(ctx, &mut |i, x| {
                if matches!(x, Value::Text(t) if matches(p, t)) {
                    found = Some(i);
                    return last;
                }
                true
            })?;
        }
        _ => {
            if class(target).is_none() {
                return Ok(None);
            }
            if !last {
                if let Some(index) = v.index(ctx)? {
                    return Ok(index.get(target));
                }
            }
            v.each(ctx, &mut |i, x| {
                if same(x, target) {
                    found = Some(i);
                    return last;
                }
                true
            })?;
        }
    }
    Ok(found)
}

/// Binary search of a vector sorted ascending (descending): the last offset whose value,
/// of the target's type, is ≤ (≥) the target. Values of other types are skipped.
fn find_sorted(
    ctx: &mut dyn Context,
    v: &Vector,
    target: &Value,
    descending: bool,
) -> Result<Option<u64>, Interrupt> {
    let Some(want) = class(target) else {
        return Ok(None);
    };
    let (mut lo, mut hi) = (0, v.len);
    let mut found = None;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        // The first value of the target's type at or after `mid`.
        let mut at = mid;
        let mut hit = None;
        while at < hi {
            let x = v.get(ctx, at)?;
            if class(&x) == Some(want) {
                hit = Some(x);
                break;
            }
            at += 1;
        }
        let Some(x) = hit else {
            hi = mid;
            continue;
        };
        let ord = compare(&x, target);
        if ord == Ordering::Equal || (ord == Ordering::Less) != descending {
            found = Some(at);
            lo = at + 1;
        } else {
            hi = mid;
        }
    }
    Ok(found)
}

/// A linear search for `target`, or else the nearest larger (smaller) value of its type;
/// ties go to the first (or `last`) offset.
fn find_nearest(
    ctx: &mut dyn Context,
    v: &Vector,
    target: &Value,
    larger: bool,
    last: bool,
) -> Result<Option<u64>, Interrupt> {
    let Some(want) = class(target) else {
        return Ok(None);
    };
    let mut exact = None;
    let mut best: Option<(u64, Value)> = None;
    v.each(ctx, &mut |i, x| {
        if class(x) != Some(want) {
            return true;
        }
        match compare(x, target) {
            Ordering::Equal => {
                exact = Some(i);
                return last;
            }
            Ordering::Greater if !larger => return true,
            Ordering::Less if larger => return true,
            _ => {}
        }
        let better = match &best {
            None => true,
            Some((_, b)) => match compare(x, b) {
                Ordering::Equal => last,
                Ordering::Less => larger,
                Ordering::Greater => !larger,
            },
        };
        if better {
            best = Some((i, x.clone()));
        }
        true
    })?;
    Ok(exact.or(best.map(|(i, _)| i)))
}

/// An optional argument that is given and not left empty (`f(a,,b)`).
fn present(arg: Option<&Expr>) -> Option<&Expr> {
    arg.filter(|a| !matches!(a, Expr::Missing))
}

/// A whole number from an argument (truncated), `default` when it is absent.
fn whole(
    arg: Option<&Expr>,
    default: f64,
    ctx: &mut dyn Context,
) -> Result<Result<f64, ErrorKind>, Interrupt> {
    Ok(match arg {
        None => Ok(default),
        Some(a) => to_number(&scalar(a, ctx)?).map(f64::trunc),
    })
}

/// Unwraps an inner `Result`, returning its error as the function's value.
macro_rules! check {
    ($e:expr) => {
        match $e {
            Ok(v) => v,
            Err(e) => return Ok(Value::Error(e)),
        }
    };
}

/// The lookup value; an error is the function's result.
fn target(arg: &Expr, ctx: &mut dyn Context) -> Result<Result<Value, ErrorKind>, Interrupt> {
    Ok(match scalar(arg, ctx)? {
        Value::Error(e) => Err(e),
        v => Ok(v),
    })
}

fn vlookup(args: &[Expr], ctx: &mut dyn Context) -> Outcome {
    lookup_in_table(args, ctx, false)
}

fn hlookup(args: &[Expr], ctx: &mut dyn Context) -> Outcome {
    lookup_in_table(args, ctx, true)
}

/// VLOOKUP (keys down the first column) and HLOOKUP (`across`: keys along the first row).
fn lookup_in_table(args: &[Expr], ctx: &mut dyn Context, across: bool) -> Outcome {
    let target = check!(target(&args[0], ctx)?);
    let table = check!(Table::of(&args[1], ctx)?);
    let n = check!(whole(Some(&args[2]), 0.0, ctx)?);
    // Omitted: approximate; given but empty (`VLOOKUP(x,t,2,)`): FALSE.
    let sorted = match args.get(3) {
        None => true,
        Some(a) => check!(to_bool(&scalar(a, ctx)?)),
    };
    let width = if across { table.rows() } else { table.cols() };
    if n < 1.0 {
        return Ok(Value::Error(ErrorKind::Value));
    }
    if n > width as f64 {
        return Ok(Value::Error(ErrorKind::Ref));
    }
    let keys = table.line(across, 0);
    let found = if sorted {
        find_sorted(ctx, &keys, &target, false)?
    } else {
        find_exact(ctx, &keys, &target, true, false)?
    };
    match found {
        Some(i) => table.line(across, n as u64 - 1).get(ctx, i),
        None => Ok(Value::Error(ErrorKind::NA)),
    }
}

/// MATCH(lookup, vector, [type]): 1 (default) approximate ascending, 0 exact with
/// wildcards, -1 approximate descending.
fn match_(args: &[Expr], ctx: &mut dyn Context) -> Outcome {
    let target = check!(target(&args[0], ctx)?);
    let table = check!(Table::of(&args[1], ctx)?);
    let mode = check!(whole(args.get(2), 1.0, ctx)?);
    let Some(v) = table.vector() else {
        return Ok(Value::Error(ErrorKind::NA));
    };
    let found = if mode == 0.0 {
        find_exact(ctx, &v, &target, true, false)?
    } else {
        find_sorted(ctx, &v, &target, mode < 0.0)?
    };
    Ok(position(found))
}

fn position(found: Option<u64>) -> Value {
    match found {
        Some(i) => Value::Number((i + 1) as f64),
        None => Value::Error(ErrorKind::NA),
    }
}

/// XLOOKUP and XMATCH's search: match mode 0 exact, 2 exact with wildcards, -1 exact or
/// next smaller, 1 exact or next larger; search mode 1 first to last, -1 last to first.
/// Binary search modes (±2) aren't supported.
fn x_search(
    ctx: &mut dyn Context,
    v: &Vector,
    target: &Value,
    match_mode: Option<&Expr>,
    search_mode: Option<&Expr>,
) -> Result<Result<Option<u64>, ErrorKind>, Interrupt> {
    let mode = match whole(present(match_mode), 0.0, ctx)? {
        Ok(m) => m,
        Err(e) => return Ok(Err(e)),
    };
    let last = match whole(present(search_mode), 1.0, ctx)? {
        Ok(1.0) => false,
        Ok(-1.0) => true,
        Ok(_) => return Ok(Err(ErrorKind::Value)),
        Err(e) => return Ok(Err(e)),
    };
    Ok(Ok(match mode {
        0.0 => find_exact(ctx, v, target, false, last)?,
        2.0 => find_exact(ctx, v, target, true, last)?,
        -1.0 => find_nearest(ctx, v, target, false, last)?,
        1.0 => find_nearest(ctx, v, target, true, last)?,
        _ => return Ok(Err(ErrorKind::Value)),
    }))
}

/// XLOOKUP(lookup, lookup_vector, return_vector, [if_not_found], [match_mode],
/// [search_mode]). The return vector runs the same way as the lookup vector, as long.
fn xlookup(args: &[Expr], ctx: &mut dyn Context) -> Outcome {
    let target = check!(target(&args[0], ctx)?);
    let keys = check!(Table::of(&args[1], ctx)?);
    let results = check!(Table::of(&args[2], ctx)?);
    let Some(v) = keys.vector() else {
        return Ok(Value::Error(ErrorKind::Value));
    };
    let fits = if v.across {
        results.rows() == 1 && results.cols() == v.len
    } else {
        results.cols() == 1 && results.rows() == v.len
    };
    if !fits {
        return Ok(Value::Error(ErrorKind::Value));
    }
    match check!(x_search(ctx, &v, &target, args.get(4), args.get(5))?) {
        Some(i) => results.line(v.across, 0).get(ctx, i),
        None => match args.get(3) {
            Some(a) if !matches!(a, Expr::Missing) => evaluate(a, ctx),
            _ => Ok(Value::Error(ErrorKind::NA)),
        },
    }
}

/// XMATCH(lookup, vector, [match_mode], [search_mode]): the position XLOOKUP would use.
fn xmatch(args: &[Expr], ctx: &mut dyn Context) -> Outcome {
    let target = check!(target(&args[0], ctx)?);
    let table = check!(Table::of(&args[1], ctx)?);
    let Some(v) = table.vector() else {
        return Ok(Value::Error(ErrorKind::Value));
    };
    Ok(position(check!(x_search(
        ctx,
        &v,
        &target,
        args.get(2),
        args.get(3)
    )?)))
}

/// INDEX(area, row, [col], [area_num]): one cell's value. A single index into a one-row
/// area picks the column. Row or column 0 (a whole row or column) is `#VALUE!` unless
/// the area is one row or column thick there; only area 1 exists.
fn index(args: &[Expr], ctx: &mut dyn Context) -> Outcome {
    let table = match as_reference(&args[0], ctx) {
        Some(_) => check!(Table::of(&args[0], ctx)?),
        None => match evaluate(&args[0], ctx)? {
            Value::Array(a) => Table::Array(a),
            Value::Error(e) => return Ok(Value::Error(e)),
            _ => return Ok(Value::Error(ErrorKind::Ref)),
        },
    };
    let mut row = check!(whole(Some(&args[1]), 0.0, ctx)?);
    let col_given = present(args.get(2)).is_some();
    let mut col = check!(whole(present(args.get(2)), 0.0, ctx)?);
    let area = check!(whole(present(args.get(3)), 1.0, ctx)?);
    if area != 1.0 {
        return Ok(Value::Error(ErrorKind::Ref));
    }
    if row < 0.0 || col < 0.0 {
        return Ok(Value::Error(ErrorKind::Value));
    }
    let (rows, cols) = (table.rows(), table.cols());
    if !col_given && rows == 1 {
        (row, col) = (0.0, row);
    }
    for (i, n) in [(&mut row, rows), (&mut col, cols)] {
        if *i == 0.0 {
            if n != 1 {
                return Ok(Value::Error(ErrorKind::Value));
            }
            *i = 1.0;
        }
        if *i > n as f64 {
            return Ok(Value::Error(ErrorKind::Ref));
        }
    }
    table.get(ctx, row as u64 - 1, col as u64 - 1)
}

/// CHOOSE(n, v1, v2, …): evaluates only the chosen value.
fn choose(args: &[Expr], ctx: &mut dyn Context) -> Outcome {
    let n = check!(whole(Some(&args[0]), 0.0, ctx)?);
    if n < 1.0 || n >= args.len() as f64 {
        return Ok(Value::Error(ErrorKind::Value));
    }
    evaluate(&args[n as usize], ctx)
}

fn rows(args: &[Expr], ctx: &mut dyn Context) -> Outcome {
    Ok(match size(&args[0], ctx)? {
        Ok((r, _)) => Value::Number(r as f64),
        Err(e) => Value::Error(e),
    })
}

fn columns(args: &[Expr], ctx: &mut dyn Context) -> Outcome {
    Ok(match size(&args[0], ctx)? {
        Ok((_, c)) => Value::Number(c as f64),
        Err(e) => Value::Error(e),
    })
}

/// The rows and columns an argument spans: a reference's area (a whole column has at least
/// an Excel sheet's 1,048,576 rows, a whole row its 16,384 columns), an array's shape, or
/// 1×1 for a scalar.
fn size(arg: &Expr, ctx: &mut dyn Context) -> Result<Result<(u64, u64), ErrorKind>, Interrupt> {
    if let Some(r) = as_reference(arg, ctx) {
        let (sheet_rows, sheet_cols) = match ctx.size(r.sheet.as_deref()) {
            Ok(s) => s,
            Err(e) => return Ok(Err(e)),
        };
        return Ok(Ok(match r.area {
            Area::Cell(_) => (1, 1),
            Area::Range(a, b) => (b.row - a.row + 1, u64::from(b.col - a.col) + 1),
            Area::Cols { first, last, .. } => {
                (sheet_rows.max(SHEET_ROWS), u64::from(last - first) + 1)
            }
            Area::Rows { first, last, .. } => {
                (last - first + 1, u64::from(sheet_cols).max(SHEET_COLS))
            }
        }));
    }
    Ok(match evaluate(arg, ctx)? {
        Value::Array(a) => Ok((a.rows as u64, a.cols as u64)),
        Value::Error(e) => Err(e),
        _ => Ok((1, 1)),
    })
}
