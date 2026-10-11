//! Evaluation of a parsed formula against a [`Context`] that supplies cells.
//!
//! `evaluate` recurses only through the expression's own depth. A formula that refers to
//! another formula cell gets that cell's value from the context, which may evaluate it in
//! turn up to a depth of its choosing and then return [`Interrupt::Deeper`] so that a
//! driver with an explicit work stack evaluates the deeper cell first (ADR 0007).

use crate::ast::{Area, BinaryOp, ErrorKind, Expr, Reference, UnaryOp};
use crate::coerce::{approx_add, compare, to_number, to_text};
use crate::functions;
use crate::parse::MAX_ROW;
use crate::value::{Array, Value};
use std::cmp::Ordering;
use std::ops::Range;
use std::sync::Arc;

/// Why an evaluation stopped before producing a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interrupt {
    /// A formula cell deeper than the context evaluates in place is needed first.
    Deeper,
    /// The cell-visit budget ran out.
    Budget,
    Cancelled,
}

/// What a formula can see: cells, areas, names, its own position, and the clock.
pub trait Context {
    /// One cell's value (a formula cell's result); `Empty` beyond the sheet.
    fn cell(&mut self, sheet: Option<&str>, row: u64, col: u32) -> Result<Value, Interrupt>;
    /// Calls `visit` with the non-empty cells of an area in row-major order, stopping when
    /// it returns false. Cells not visited are empty.
    fn area(
        &mut self,
        sheet: Option<&str>,
        rows: Range<u64>,
        cols: Range<u32>,
        visit: &mut dyn FnMut(u64, u32, &Value) -> bool,
    ) -> Result<(), Interrupt>;
    /// The sheet's rows and columns; `Err(Ref)` for an unknown sheet.
    fn size(&self, sheet: Option<&str>) -> Result<(u64, u32), ErrorKind>;
    /// A defined name's formula.
    fn name(&self, name: &str) -> Option<Expr>;
    /// The formula's own cell (row among all file rows, view column).
    fn position(&self) -> (u64, u32);
    /// Today as a serial day number, local time.
    fn today(&self) -> f64;
    /// Now as a serial number with the time as the fraction, local time.
    fn now(&self) -> f64;
    /// A random number in [0, 1).
    fn random(&mut self) -> f64;
}

/// Evaluates `expr` (a formula without its `=`).
pub fn evaluate(expr: &Expr, ctx: &mut dyn Context) -> Result<Value, Interrupt> {
    Ok(match expr {
        Expr::Number(n) => Value::Number(*n),
        Expr::Text(t) => Value::text(t.as_str()),
        Expr::Bool(b) => Value::Bool(*b),
        Expr::Error(e) => Value::Error(*e),
        Expr::Missing => Value::Empty,
        Expr::Ref(r) => reference_value(r, ctx)?,
        Expr::Name(n) => match ctx.name(n) {
            Some(e) => evaluate(&e, ctx)?,
            None => Value::Error(ErrorKind::Name),
        },
        Expr::Paren(x) => evaluate(x, ctx)?,
        Expr::Unary(op, x) => {
            let v = evaluate(x, ctx)?;
            map(v, &|v| unary(*op, v))
        }
        Expr::Percent(x) => {
            let v = evaluate(x, ctx)?;
            map(v, &|v| match to_number(v) {
                Ok(n) => Value::Number(n / 100.0),
                Err(e) => Value::Error(e),
            })
        }
        Expr::Binary(op, l, r) => {
            let l = evaluate(l, ctx)?;
            let r = evaluate(r, ctx)?;
            zip(&l, &r, &|a, b| binary(*op, a, b))
        }
        Expr::Call(name, args) => match functions::get(name) {
            None => Value::Error(ErrorKind::Name),
            Some(f) if args.len() < f.min || f.max.is_some_and(|m| args.len() > m) => {
                Value::Error(ErrorKind::Value)
            }
            Some(f) => (f.call)(args, ctx)?,
        },
        Expr::Array(rows) => {
            let cols = rows.first().map_or(0, Vec::len);
            let mut values = Vec::with_capacity(rows.len() * cols);
            for item in rows.iter().flatten() {
                values.push(evaluate(item, ctx)?);
            }
            Value::Array(Arc::new(Array {
                rows: rows.len(),
                cols,
                values,
            }))
        }
    })
}

/// Names of the functions `expr` calls that Bricks doesn't implement, each once.
pub fn unknown_functions(expr: &Expr) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    expr.visit(&mut |e| {
        if let Expr::Call(name, _) = e {
            if !functions::is_known(name) && !out.contains(name) {
                out.push(name.clone());
            }
        }
    });
    out
}

/// The rows and columns an area covers on its sheet; whole columns and rows end at the
/// sheet's size.
pub fn bounds(r: &Reference, ctx: &dyn Context) -> Result<(Range<u64>, Range<u32>), ErrorKind> {
    Ok(match &r.area {
        Area::Cell(a) => (a.row..a.row + 1, a.col..a.col + 1),
        Area::Range(a, b) => (a.row..b.row + 1, a.col..b.col + 1),
        Area::Cols { first, last, .. } => {
            let (rows, _) = ctx.size(r.sheet.as_deref())?;
            (0..rows, *first..*last + 1)
        }
        Area::Rows { first, last, .. } => {
            let (_, cols) = ctx.size(r.sheet.as_deref())?;
            (*first..*last + 1, 0..cols)
        }
    })
}

/// The reference an argument names, through parentheses and defined names.
pub fn as_reference(arg: &Expr, ctx: &dyn Context) -> Option<Reference> {
    match arg {
        Expr::Ref(r) => Some(r.clone()),
        Expr::Paren(x) => as_reference(x, ctx),
        Expr::Name(n) => as_reference(&ctx.name(n)?, ctx),
        _ => None,
    }
}

/// A reference used as a value takes implicit intersection: a single cell gives its
/// value; a one-column area gives the cell in the formula's own row, a one-row area the
/// cell in its own column; anything else is `#VALUE!`.
fn reference_value(r: &Reference, ctx: &mut dyn Context) -> Result<Value, Interrupt> {
    let sheet = r.sheet.as_deref();
    if let Err(e) = ctx.size(sheet) {
        return Ok(Value::Error(e));
    }
    let (rows, cols) = match &r.area {
        Area::Cell(a) => return ctx.cell(sheet, a.row, a.col),
        Area::Range(a, b) => (a.row..b.row + 1, a.col..b.col + 1),
        Area::Cols { first, last, .. } => (0..MAX_ROW, *first..*last + 1),
        Area::Rows { first, last, .. } => (*first..*last + 1, 0..u32::MAX),
    };
    let (row, col) = ctx.position();
    if cols.end - cols.start == 1 && rows.contains(&row) {
        ctx.cell(sheet, row, cols.start)
    } else if rows.end - rows.start == 1 && cols.contains(&col) {
        ctx.cell(sheet, rows.start, col)
    } else {
        Ok(Value::Error(ErrorKind::Value))
    }
}

/// Applies `f` to a value, element-wise over arrays.
pub(crate) fn map(v: Value, f: &dyn Fn(&Value) -> Value) -> Value {
    match v {
        Value::Array(a) => Value::Array(Arc::new(Array {
            rows: a.rows,
            cols: a.cols,
            values: a.values.iter().map(f).collect(),
        })),
        v => f(&v),
    }
}

/// Applies `f` pairwise, broadcasting a scalar or a one-row/one-column array over the other
/// side; positions outside a smaller array are `#N/A`.
pub(crate) fn zip(l: &Value, r: &Value, f: &dyn Fn(&Value, &Value) -> Value) -> Value {
    let dims = |v: &Value| match v {
        Value::Array(a) => (a.rows, a.cols),
        _ => (1, 1),
    };
    if !matches!(l, Value::Array(_)) && !matches!(r, Value::Array(_)) {
        return f(l, r);
    }
    let ((lr, lc), (rr, rc)) = (dims(l), dims(r));
    let (rows, cols) = (lr.max(rr), lc.max(rc));
    let at = |v: &Value, i: usize, j: usize| -> Value {
        match v {
            Value::Array(a) => {
                let ii = if a.rows == 1 { 0 } else { i };
                let jj = if a.cols == 1 { 0 } else { j };
                if ii < a.rows && jj < a.cols {
                    a.get(ii, jj).clone()
                } else {
                    Value::Error(ErrorKind::NA)
                }
            }
            v => v.clone(),
        }
    };
    let mut values = Vec::with_capacity(rows * cols);
    for i in 0..rows {
        for j in 0..cols {
            values.push(f(&at(l, i, j), &at(r, i, j)));
        }
    }
    Value::Array(Arc::new(Array { rows, cols, values }))
}

pub(crate) fn unary(op: UnaryOp, v: &Value) -> Value {
    match op {
        UnaryOp::Plus => v.clone(),
        UnaryOp::Neg => match to_number(v) {
            Ok(n) => Value::Number(-n),
            Err(e) => Value::Error(e),
        },
    }
}

/// `a ^ b` as LibreOffice computes it: `0^0` is 1, a negative base takes an odd root
/// (`(-8)^(1/3)` is -2), and other fractional powers of negatives, `0^-n`, overflow, and
/// underflow to 0 are `#NUM!`.
pub fn power(a: f64, b: f64) -> Result<f64, ErrorKind> {
    if a == 0.0 && b < 0.0 {
        return Err(ErrorKind::Num);
    }
    let n = if a < 0.0 && b.fract() != 0.0 {
        let inv = 1.0 / b;
        let root = inv.round();
        if (inv - root).abs() < 1e-9 && root % 2.0 != 0.0 {
            -(-a).powf(b)
        } else {
            return Err(ErrorKind::Num);
        }
    } else {
        a.powf(b)
    };
    if n.is_finite() && (n != 0.0 || a == 0.0) {
        Ok(n)
    } else {
        Err(ErrorKind::Num)
    }
}

pub(crate) fn binary(op: BinaryOp, l: &Value, r: &Value) -> Value {
    use BinaryOp::*;
    match op {
        Add | Sub | Mul | Div | Pow => {
            let (a, b) = match (to_number(l), to_number(r)) {
                (Err(e), _) | (_, Err(e)) => return Value::Error(e),
                (Ok(a), Ok(b)) => (a, b),
            };
            let n = match op {
                Add => approx_add(a, b),
                Sub => approx_add(a, -b),
                Mul => a * b,
                Div if b == 0.0 => return Value::Error(ErrorKind::Div0),
                Div => a / b,
                _ => match power(a, b) {
                    Ok(n) => n,
                    Err(e) => return Value::Error(e),
                },
            };
            if !n.is_finite() {
                return Value::Error(ErrorKind::Num);
            }
            let (ld, rd) = (matches!(l, Value::Date(_)), matches!(r, Value::Date(_)));
            match op {
                Add if ld != rd => Value::Date(n),
                Sub if ld && !rd => Value::Date(n),
                _ => Value::Number(n),
            }
        }
        Concat => match (to_text(l), to_text(r)) {
            (Err(e), _) | (_, Err(e)) => Value::Error(e),
            (Ok(a), Ok(b)) => Value::text(a + &b),
        },
        Eq | Ne | Lt | Le | Gt | Ge => {
            if let Value::Error(e) = l.scalar() {
                return Value::Error(*e);
            }
            if let Value::Error(e) = r.scalar() {
                return Value::Error(*e);
            }
            let o = compare(l, r);
            Value::Bool(match op {
                Eq => o == Ordering::Equal,
                Ne => o != Ordering::Equal,
                Lt => o == Ordering::Less,
                Le => o != Ordering::Greater,
                Gt => o == Ordering::Greater,
                _ => o != Ordering::Less,
            })
        }
    }
}
