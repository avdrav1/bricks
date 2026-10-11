//! Criteria for COUNTIF(S), SUMIF(S), AVERAGEIF(S), MINIFS, and MAXIFS (FML-5): a value or a
//! text such as `">=2"`, `"<>x"`, or `"b*"`, and the ranges they test.
//!
//! A criterion range and the range whose values are aggregated align by offset. Cells a
//! reference area doesn't visit are empty, and criteria such as `""` or `"<>x"` match empty
//! cells, so matches are computed over every position of the area, not only its non-empty
//! cells.

use super::wildcard;
use crate::ast::{ErrorKind, Expr};
use crate::coerce::{approx_equal, compare_text};
use crate::eval::{as_reference, bounds, evaluate, Context, Interrupt};
use crate::value::{literal, Array, Value};
use std::cmp::Ordering;
use std::ops::Range;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// A parsed criterion: an operator and the value it compares with.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Criterion {
    op: Op,
    operand: Value,
    /// The operand is text with `*`, `?`, or `~`.
    wild: bool,
}

/// Reads a criterion: text takes an optional operator prefix (`<>`, `>=`, `<=`, `<`, `>`,
/// `=`) and the rest reads as a raw cell would ([`literal`]); any other value is matched
/// for equality. The caller handles an error criterion.
pub(crate) fn parse(v: &Value) -> Criterion {
    match v.scalar() {
        Value::Text(t) => {
            const OPS: [(&str, Op); 6] = [
                ("<>", Op::Ne),
                (">=", Op::Ge),
                ("<=", Op::Le),
                ("<", Op::Lt),
                (">", Op::Gt),
                ("=", Op::Eq),
            ];
            let (op, rest) = OPS
                .iter()
                .find_map(|(p, op)| t.strip_prefix(p).map(|rest| (*op, rest)))
                .unwrap_or((Op::Eq, t));
            let operand = literal(rest);
            let wild = matches!(&operand, Value::Text(p) if wildcard::has_wildcards(p));
            Criterion { op, operand, wild }
        }
        v => Criterion {
            op: Op::Eq,
            operand: v.clone(),
            wild: false,
        },
    }
}

impl Criterion {
    /// Whether a cell's value meets the criterion. Numbers (and dates) compare only with
    /// numbers, text only with text (ignoring case, with wildcards for `=` and `<>`),
    /// booleans only with booleans; `<>` matches every value of another type.
    pub(crate) fn matches(&self, v: &Value) -> bool {
        let order = match (&self.operand, v) {
            (Value::Empty, v) => {
                let blank = match v {
                    Value::Empty => true,
                    Value::Text(t) => t.is_empty(),
                    _ => false,
                };
                return match self.op {
                    Op::Eq => blank,
                    Op::Ne => !blank,
                    _ => false,
                };
            }
            (Value::Number(x) | Value::Date(x), Value::Number(y) | Value::Date(y)) => {
                if approx_equal(*x, *y) {
                    Ordering::Equal
                } else {
                    y.partial_cmp(x).unwrap_or(Ordering::Equal)
                }
            }
            (Value::Text(p), Value::Text(s)) => match self.op {
                Op::Eq | Op::Ne => {
                    let equal = if self.wild {
                        wildcard::matches(p, s)
                    } else {
                        compare_text(s, p) == Ordering::Equal
                    };
                    return equal == (self.op == Op::Eq);
                }
                _ => compare_text(s, p),
            },
            (Value::Bool(a), Value::Bool(b)) => b.cmp(a),
            (Value::Error(a), Value::Error(b)) => {
                return match self.op {
                    Op::Eq => a == b,
                    Op::Ne => a != b,
                    _ => false,
                }
            }
            _ => return self.op == Op::Ne,
        };
        match self.op {
            Op::Eq => order == Ordering::Equal,
            Op::Ne => order != Ordering::Equal,
            Op::Lt => order == Ordering::Less,
            Op::Le => order != Ordering::Greater,
            Op::Gt => order == Ordering::Greater,
            Op::Ge => order != Ordering::Less,
        }
    }
}

/// A range argument: a reference area (streamed, never materialized) or an array.
enum Source {
    Area {
        sheet: Option<String>,
        rows: Range<u64>,
        cols: Range<u32>,
    },
    Values(Arc<Array>),
}

impl Source {
    fn read(arg: &Expr, ctx: &mut dyn Context) -> Result<Result<Self, ErrorKind>, Interrupt> {
        if let Some(r) = as_reference(arg, ctx) {
            return Ok(bounds(&r, ctx).map(|(rows, cols)| Source::Area {
                sheet: r.sheet,
                rows,
                cols,
            }));
        }
        Ok(match evaluate(arg, ctx)? {
            Value::Error(e) => Err(e),
            Value::Array(a) => Ok(Source::Values(a)),
            v => Ok(Source::Values(Arc::new(Array {
                rows: 1,
                cols: 1,
                values: vec![v],
            }))),
        })
    }

    /// Rows and columns.
    fn shape(&self) -> (u64, u64) {
        match self {
            Source::Area { rows, cols, .. } => {
                (rows.end - rows.start, u64::from(cols.end - cols.start))
            }
            Source::Values(a) => (a.rows as u64, a.cols as u64),
        }
    }

    /// The part of a `shape`-sized block at this source's origin that can hold values:
    /// beyond it every cell is empty.
    fn filled(&self, ctx: &dyn Context, shape: (u64, u64)) -> Result<(u64, u64), ErrorKind> {
        Ok(match self {
            Source::Area { sheet, rows, cols } => {
                let (h, w) = ctx.size(sheet.as_deref())?;
                (
                    shape.0.min(h.saturating_sub(rows.start)),
                    shape.1.min(u64::from(w.saturating_sub(cols.start))),
                )
            }
            Source::Values(a) => (shape.0.min(a.rows as u64), shape.1.min(a.cols as u64)),
        })
    }

    /// Calls `f` with the offset (row-major in a block `block.1` wide) and value of each
    /// non-empty cell in the block's first `block.0` rows and `block.1` columns.
    fn visit(
        &self,
        ctx: &mut dyn Context,
        block: (u64, u64),
        f: &mut dyn FnMut(usize, &Value) -> bool,
    ) -> Result<(), Interrupt> {
        let (h, w) = block;
        match self {
            Source::Area { sheet, rows, cols } => {
                let (r0, c0) = (rows.start, cols.start);
                let c_end = (u64::from(c0) + w).min(u64::from(u32::MAX)) as u32;
                ctx.area(sheet.as_deref(), r0..r0 + h, c0..c_end, &mut |r, c, v| {
                    let (i, j) = (r - r0, u64::from(c - c0));
                    if i < h && j < w {
                        f((i * w + j) as usize, v)
                    } else {
                        true
                    }
                })
            }
            Source::Values(a) => {
                for i in 0..(h as usize).min(a.rows) {
                    for j in 0..(w as usize).min(a.cols) {
                        let v = a.get(i, j);
                        if !matches!(v, Value::Empty) && !f(i * w as usize + j, v) {
                            return Ok(());
                        }
                    }
                }
                Ok(())
            }
        }
    }
}

/// The positions of a block that meet every criterion, and the range whose values the
/// function aggregates there.
pub(super) struct Matched {
    /// Rows and columns that may hold values in any of the ranges.
    block: (u64, u64),
    /// Per position of `block`, row-major: whether every criterion holds.
    mask: Vec<bool>,
    /// Matching positions outside `block` (all of them empty in every range).
    beyond: u64,
    values: Option<Source>,
}

impl Matched {
    /// How many positions meet every criterion.
    pub(super) fn count(&self) -> u64 {
        self.mask.iter().filter(|&&m| m).count() as u64 + self.beyond
    }

    /// Calls `f` with each non-empty value of the aggregated range at a matching position.
    pub(super) fn each_value(
        &self,
        ctx: &mut dyn Context,
        f: &mut dyn FnMut(&Value) -> bool,
    ) -> Result<(), Interrupt> {
        let Some(values) = &self.values else {
            return Ok(());
        };
        let mask = &self.mask;
        values.visit(ctx, self.block, &mut |off, v| !mask[off] || f(v))
    }
}

/// Evaluates `(range, criterion)` pairs. `values` is the range to aggregate: with `resize`
/// (SUMIF, AVERAGEIF) it is read at the first criterion range's shape from its own top-left
/// cell, as Excel does; otherwise (the …IFS functions) its shape must equal the criterion
/// ranges', as must every criterion range's, else `#VALUE!`.
pub(super) fn evaluate_criteria(
    values: Option<&Expr>,
    resize: bool,
    pairs: &[(&Expr, &Expr)],
    ctx: &mut dyn Context,
) -> Result<Result<Matched, ErrorKind>, Interrupt> {
    let mut ranges = Vec::with_capacity(pairs.len());
    for (range, criterion) in pairs {
        let source = match Source::read(range, ctx)? {
            Ok(s) => s,
            Err(e) => return Ok(Err(e)),
        };
        let criterion = match evaluate(criterion, ctx)?.scalar() {
            Value::Error(e) => return Ok(Err(*e)),
            v => parse(v),
        };
        ranges.push((source, criterion));
    }
    let Some(shape) = ranges.first().map(|(s, _)| s.shape()) else {
        return Ok(Err(ErrorKind::Value));
    };
    if ranges.iter().any(|(s, _)| s.shape() != shape) {
        return Ok(Err(ErrorKind::Value));
    }
    let values = match values {
        None => None,
        Some(arg) => match Source::read(arg, ctx)? {
            Err(e) => return Ok(Err(e)),
            Ok(s) if !resize && s.shape() != shape => return Ok(Err(ErrorKind::Value)),
            Ok(s) => Some(s),
        },
    };
    let mut block = (0, 0);
    for s in ranges.iter().map(|(s, _)| s).chain(values.as_ref()) {
        match s.filled(ctx, shape) {
            Ok((h, w)) => block = (block.0.max(h), block.1.max(w)),
            Err(e) => return Ok(Err(e)),
        }
    }
    let n = (block.0 * block.1) as usize;
    let mut mask = vec![true; n];
    let mut beyond = true;
    let mut this = Vec::new();
    for (source, criterion) in &ranges {
        let empty = criterion.matches(&Value::Empty);
        beyond &= empty;
        this.clear();
        this.resize(n, empty);
        source.visit(ctx, block, &mut |off, v| {
            this[off] = criterion.matches(v);
            true
        })?;
        for (m, t) in mask.iter_mut().zip(&this) {
            *m &= *t;
        }
    }
    let beyond = if beyond {
        shape.0 * shape.1 - block.0 * block.1
    } else {
        0
    };
    Ok(Ok(Matched {
        block,
        mask,
        beyond,
        values,
    }))
}

/// Splits `range, criterion, range, criterion, …` into pairs; an odd count is `None`.
pub(super) fn pairs(args: &[Expr]) -> Option<Vec<(&Expr, &Expr)>> {
    match args.as_chunks::<2>() {
        (pairs, []) if !pairs.is_empty() => Some(pairs.iter().map(|[r, c]| (r, c)).collect()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Booleans are their own type (Excel); LibreOffice stores them as 1 and 0, so the
    /// LibreOffice fixture can't pin this.
    #[test]
    fn booleans_match_only_booleans() {
        let t = parse(&Value::Bool(true));
        assert!(t.matches(&Value::Bool(true)));
        assert!(!t.matches(&Value::Number(1.0)));
        assert!(!parse(&Value::Number(1.0)).matches(&Value::Bool(true)));
        assert!(parse(&Value::text("<>1")).matches(&Value::Bool(true)));
    }
}
