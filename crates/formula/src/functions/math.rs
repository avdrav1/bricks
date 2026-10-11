//! Math functions (FML-5).

use super::criteria::{evaluate_criteria, pairs};
use super::{add, add_volatile, each_value, Outcome, Registry};
use crate::ast::{ErrorKind, Expr};
use crate::coerce::to_number;
use crate::eval::{
    as_reference, binary, bounds, evaluate, map, power, unary, zip, Context, Interrupt,
};
use crate::value::{Array, Value};
use std::sync::Arc;

pub(super) fn register(m: &mut Registry) {
    add(m, "SUM", 0, None, sum);
    add(m, "SUMPRODUCT", 1, None, sumproduct);
    add(m, "SUMIF", 2, Some(3), sumif);
    add(m, "SUMIFS", 3, None, sumifs);
    add(m, "PRODUCT", 1, None, product);
    add(m, "ABS", 1, Some(1), |a, c| {
        unary_math(a, c, |x| Ok(x.abs()))
    });
    add(m, "ROUND", 1, Some(2), |a, c| {
        binary_math(a, c, 0.0, |x, d| Ok(round_to(x, d, f64::round)))
    });
    add(m, "ROUNDUP", 1, Some(2), |a, c| {
        binary_math(a, c, 0.0, |x, d| Ok(round_to(x, d, away_from_zero)))
    });
    add(m, "ROUNDDOWN", 1, Some(2), |a, c| {
        binary_math(a, c, 0.0, |x, d| Ok(round_to(x, d, f64::trunc)))
    });
    add(m, "TRUNC", 1, Some(2), |a, c| {
        binary_math(a, c, 0.0, |x, d| Ok(round_to(x, d, f64::trunc)))
    });
    add(m, "INT", 1, Some(1), |a, c| {
        unary_math(a, c, |x| Ok(snap(x).floor()))
    });
    add(m, "MOD", 2, Some(2), |a, c| binary_math(a, c, 0.0, modulo));
    add(m, "POWER", 2, Some(2), |a, c| binary_math(a, c, 0.0, power));
    add(m, "SQRT", 1, Some(1), |a, c| {
        unary_math(a, c, |x| positive(x, true).map(f64::sqrt))
    });
    add(m, "EXP", 1, Some(1), |a, c| {
        unary_math(a, c, |x| Ok(x.exp()))
    });
    add(m, "LN", 1, Some(1), |a, c| {
        unary_math(a, c, |x| positive(x, false).map(f64::ln))
    });
    add(m, "LOG", 1, Some(2), |a, c| binary_math(a, c, 10.0, log));
    add(m, "LOG10", 1, Some(1), |a, c| {
        unary_math(a, c, |x| positive(x, false).map(f64::log10))
    });
    add(m, "PI", 0, Some(0), |_, _| {
        Ok(Value::Number(std::f64::consts::PI))
    });
    add_volatile(m, "RAND", 0, Some(0), |_, c| Ok(Value::Number(c.random())));
    add_volatile(m, "RANDBETWEEN", 2, Some(2), randbetween);
    add(m, "CEILING", 2, Some(2), |a, c| {
        binary_math(a, c, 0.0, |x, s| multiple(x, s, f64::ceil, Ok(0.0)))
    });
    add(m, "FLOOR", 2, Some(2), |a, c| {
        binary_math(a, c, 0.0, |x, s| {
            multiple(x, s, f64::floor, Err(ErrorKind::Div0))
        })
    });
    add(m, "SIGN", 1, Some(1), |a, c| {
        unary_math(a, c, |x| Ok(if x == 0.0 { 0.0 } else { x.signum() }))
    });
    add(m, "QUOTIENT", 2, Some(2), |a, c| {
        binary_math(a, c, 0.0, |x, y| {
            if y == 0.0 {
                Err(ErrorKind::Div0)
            } else {
                Ok(snap(x / y).trunc())
            }
        })
    });
    add(m, "GCD", 1, None, |a, c| integers(a, c, gcd_all));
    add(m, "LCM", 1, None, |a, c| integers(a, c, lcm_all));
    add(m, "FACT", 1, Some(1), |a, c| unary_math(a, c, fact));
    add(m, "MROUND", 2, Some(2), |a, c| {
        binary_math(a, c, 0.0, |x, s| {
            Ok(if s == 0.0 {
                0.0
            } else {
                snap(x / s).round() * s
            })
        })
    });
    add(m, "EVEN", 1, Some(1), |a, c| {
        unary_math(
            a,
            c,
            |x| Ok(x.signum() * (snap(x.abs() / 2.0).ceil() * 2.0)),
        )
    });
    add(m, "ODD", 1, Some(1), |a, c| {
        unary_math(a, c, |x| {
            let sign = if x < 0.0 { -1.0 } else { 1.0 };
            Ok(sign * (snap((x.abs() + 1.0) / 2.0).ceil() * 2.0 - 1.0))
        })
    });
}

/// A value an aggregate counts: through a reference or an array only numbers (and dates)
/// count, and text, booleans, and empty cells are skipped; a direct argument is coerced
/// (`TRUE` is 1, `"3"` is 3, other text is `#VALUE!`). Errors propagate.
pub(super) fn aggregate_number(v: &Value, from_ref: bool) -> Result<Option<f64>, ErrorKind> {
    match v {
        Value::Error(e) => Err(*e),
        Value::Number(n) | Value::Date(n) => Ok(Some(*n)),
        _ if from_ref => Ok(None),
        v => to_number(v).map(Some),
    }
}

/// Neumaier's compensated sum, so long sums keep their low digits as LibreOffice's do.
#[derive(Default)]
pub(super) struct Sum {
    sum: f64,
    compensation: f64,
}

impl Sum {
    pub(super) fn add(&mut self, x: f64) {
        let t = self.sum + x;
        if self.sum.abs() >= x.abs() {
            self.compensation += (self.sum - t) + x;
        } else {
            self.compensation += (x - t) + self.sum;
        }
        self.sum = t;
    }

    pub(super) fn total(&self) -> f64 {
        self.sum + self.compensation
    }
}

/// Calls `f` with each number the arguments give an aggregate; the first error ends it.
pub(super) fn each_number(
    args: &[Expr],
    ctx: &mut dyn Context,
    f: &mut dyn FnMut(f64),
) -> Result<Result<(), ErrorKind>, Interrupt> {
    let mut err = None;
    for a in args {
        if matches!(a, Expr::Missing) {
            continue;
        }
        each_value(
            a,
            ctx,
            &mut |v, from_ref| match aggregate_number(v, from_ref) {
                Ok(Some(n)) => {
                    f(n);
                    true
                }
                Ok(None) => true,
                Err(e) => {
                    err = Some(e);
                    false
                }
            },
        )?;
        if let Some(e) = err {
            return Ok(Err(e));
        }
    }
    Ok(Ok(()))
}

/// A finite number as a value; overflow is `#NUM!`.
pub(super) fn number(r: Result<f64, ErrorKind>) -> Value {
    match r {
        Ok(n) if n.is_finite() => Value::Number(n),
        Ok(_) => Value::Error(ErrorKind::Num),
        Err(e) => Value::Error(e),
    }
}

/// `x` rounded to 15 significant digits, so that representation noise such as
/// `2.675 * 100 = 267.49999999999997` doesn't decide a rounding (LibreOffice's
/// `approxValue`). Numbers of 1E14 and more are returned as they are.
pub(super) fn snap(x: f64) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return x;
    }
    let e = x.abs().log10().floor() as i32;
    if e >= 14 {
        return x;
    }
    let scale = 10f64.powi(14 - e);
    let y = x * scale;
    if !scale.is_finite() || !y.is_finite() {
        return x;
    }
    y.round() / scale
}

/// A one-number function, element-wise over arrays.
fn unary_math(
    args: &[Expr],
    ctx: &mut dyn Context,
    f: fn(f64) -> Result<f64, ErrorKind>,
) -> Outcome {
    let v = evaluate(&args[0], ctx)?;
    Ok(map(v, &|x| number(to_number(x).and_then(f))))
}

/// A two-number function, element-wise over arrays; an absent second argument is `default`.
fn binary_math(
    args: &[Expr],
    ctx: &mut dyn Context,
    default: f64,
    f: fn(f64, f64) -> Result<f64, ErrorKind>,
) -> Outcome {
    let a = evaluate(&args[0], ctx)?;
    let b = match args.get(1) {
        Some(e) => evaluate(e, ctx)?,
        None => Value::Number(default),
    };
    Ok(zip(&a, &b, &|x, y| {
        number(to_number(x).and_then(|x| f(x, to_number(y)?)))
    }))
}

fn away_from_zero(x: f64) -> f64 {
    if x < 0.0 {
        x.floor()
    } else {
        x.ceil()
    }
}

/// `x` rounded by `op` to `digits` decimals (negative: to tens, hundreds, …); `digits` is
/// truncated to an integer.
fn round_to(x: f64, digits: f64, op: fn(f64) -> f64) -> f64 {
    let d = digits.trunc().clamp(-308.0, 308.0) as i32;
    let factor = 10f64.powi(d.abs());
    if d >= 0 {
        let y = x * factor;
        if y.is_finite() {
            op(snap(y)) / factor
        } else {
            x
        }
    } else {
        op(snap(x / factor)) * factor
    }
}

/// `MOD`: the remainder with the sign of the divisor.
fn modulo(x: f64, y: f64) -> Result<f64, ErrorKind> {
    if y == 0.0 {
        return Err(ErrorKind::Div0);
    }
    Ok(x - y * snap(x / y).floor())
}

/// `x` for SQRT (`zero_ok`) and LN/LOG10, else `#NUM!`.
fn positive(x: f64, zero_ok: bool) -> Result<f64, ErrorKind> {
    if x > 0.0 || (zero_ok && x == 0.0) {
        Ok(x)
    } else {
        Err(ErrorKind::Num)
    }
}

fn log(x: f64, base: f64) -> Result<f64, ErrorKind> {
    if x <= 0.0 || base <= 0.0 {
        return Err(ErrorKind::Num);
    }
    if base == 1.0 {
        return Err(ErrorKind::Div0);
    }
    Ok(if base == 10.0 {
        x.log10()
    } else {
        x.ln() / base.ln()
    })
}

/// CEILING and FLOOR in Excel's two-argument form (LibreOffice's CEILING.XCL and
/// FLOOR.XCL): `x` rounded by `op` to a multiple of `s`; a positive `x` with a negative `s`
/// is `#NUM!`, and `s` = 0 gives `zero`.
fn multiple(
    x: f64,
    s: f64,
    op: fn(f64) -> f64,
    zero: Result<f64, ErrorKind>,
) -> Result<f64, ErrorKind> {
    if x > 0.0 && s < 0.0 {
        return Err(ErrorKind::Num);
    }
    if s == 0.0 {
        return zero;
    }
    Ok(op(snap(x / s)) * s)
}

fn fact(x: f64) -> Result<f64, ErrorKind> {
    if x < 0.0 {
        return Err(ErrorKind::Num);
    }
    let n = x.trunc();
    if n > 170.0 {
        // LibreOffice's answer to a factorial past the largest double.
        return Err(ErrorKind::Value);
    }
    Ok((2..=n as u32).fold(1.0, |p, k| p * f64::from(k)))
}

/// `RANDBETWEEN(bottom, top)`: an integer from `bottom` to `top`, both rounded up as
/// LibreOffice does; `top < bottom` is `#NUM!`.
fn randbetween(args: &[Expr], ctx: &mut dyn Context) -> Outcome {
    let bounds = |v: Value| to_number(v.scalar()).map(|x| snap(x).ceil());
    let lo = match bounds(evaluate(&args[0], ctx)?) {
        Ok(n) => n,
        Err(e) => return Ok(Value::Error(e)),
    };
    let hi = match bounds(evaluate(&args[1], ctx)?) {
        Ok(n) => n,
        Err(e) => return Ok(Value::Error(e)),
    };
    if hi < lo {
        return Ok(Value::Error(ErrorKind::Num));
    }
    let r = ctx.random();
    Ok(Value::Number((lo + (r * (hi - lo + 1.0)).floor()).min(hi)))
}

/// The largest integer a double holds exactly; GCD and LCM refuse anything larger.
const MAX_EXACT: f64 = 9_007_199_254_740_992.0;

/// GCD and LCM: the arguments' numbers truncated to integers; a negative is `#NUM!`.
fn integers(args: &[Expr], ctx: &mut dyn Context, f: fn(&[f64]) -> f64) -> Outcome {
    let mut ns = Vec::new();
    let mut bad = false;
    if let Err(e) = each_number(args, ctx, &mut |n| {
        bad |= !(0.0..MAX_EXACT).contains(&n);
        ns.push(n.trunc());
    })? {
        return Ok(Value::Error(e));
    }
    if bad {
        return Ok(Value::Error(ErrorKind::Num));
    }
    Ok(number(Ok(f(&ns))))
}

fn gcd(mut a: f64, mut b: f64) -> f64 {
    while b != 0.0 {
        (a, b) = (b, a % b);
    }
    a
}

fn gcd_all(ns: &[f64]) -> f64 {
    ns.iter().fold(0.0, |g, &n| gcd(g, n))
}

fn lcm_all(ns: &[f64]) -> f64 {
    ns.iter().fold(1.0, |l, &n| {
        if l == 0.0 || n == 0.0 {
            0.0
        } else {
            l / gcd(l, n) * n
        }
    })
}

fn sum(args: &[Expr], ctx: &mut dyn Context) -> Outcome {
    let mut total = Sum::default();
    Ok(match each_number(args, ctx, &mut |n| total.add(n))? {
        Ok(()) => Value::Number(total.total()),
        Err(e) => Value::Error(e),
    })
}

fn product(args: &[Expr], ctx: &mut dyn Context) -> Outcome {
    let mut p = 1.0;
    let mut any = false;
    Ok(
        match each_number(args, ctx, &mut |n| {
            p *= n;
            any = true;
        })? {
            Ok(()) => number(Ok(if any { p } else { 0.0 })),
            Err(e) => Value::Error(e),
        },
    )
}

/// An argument evaluated in array context, as SUMPRODUCT evaluates its arguments: a
/// reference is its area's values and operators apply element-wise over them.
fn array_value(arg: &Expr, ctx: &mut dyn Context) -> Outcome {
    if let Some(r) = as_reference(arg, ctx) {
        let (rows, cols) = match bounds(&r, ctx) {
            Ok(b) => b,
            Err(e) => return Ok(Value::Error(e)),
        };
        let (h, w) = (
            (rows.end - rows.start) as usize,
            (cols.end - cols.start) as usize,
        );
        let mut values = vec![Value::Empty; h * w];
        let (r0, c0) = (rows.start, cols.start);
        ctx.area(r.sheet.as_deref(), rows, cols, &mut |r, c, v| {
            values[(r - r0) as usize * w + (c - c0) as usize] = v.clone();
            true
        })?;
        return Ok(Value::Array(Arc::new(Array {
            rows: h,
            cols: w,
            values,
        })));
    }
    Ok(match arg {
        Expr::Paren(x) => array_value(x, ctx)?,
        Expr::Unary(op, x) => map(array_value(x, ctx)?, &|v| unary(*op, v)),
        Expr::Percent(x) => map(array_value(x, ctx)?, &|v| {
            number(to_number(v).map(|n| n / 100.0))
        }),
        Expr::Binary(op, l, r) => {
            let l = array_value(l, ctx)?;
            let r = array_value(r, ctx)?;
            zip(&l, &r, &|a, b| binary(*op, a, b))
        }
        e => evaluate(e, ctx)?,
    })
}

/// `SUMPRODUCT(a, b, …)`: the sum of the element-wise products of equally shaped arrays;
/// non-numbers count as 0 and errors propagate.
fn sumproduct(args: &[Expr], ctx: &mut dyn Context) -> Outcome {
    let mut arrays = Vec::with_capacity(args.len());
    for a in args {
        arrays.push(match array_value(a, ctx)? {
            Value::Array(a) => a,
            v => Arc::new(Array {
                rows: 1,
                cols: 1,
                values: vec![v],
            }),
        });
    }
    let (rows, cols) = (arrays[0].rows, arrays[0].cols);
    if arrays.iter().any(|a| a.rows != rows || a.cols != cols) {
        return Ok(Value::Error(ErrorKind::Value));
    }
    let mut total = Sum::default();
    for i in 0..rows * cols {
        let mut p = 1.0;
        for a in &arrays {
            match &a.values[i] {
                Value::Number(n) | Value::Date(n) => p *= n,
                Value::Error(e) => return Ok(Value::Error(*e)),
                _ => p = 0.0,
            }
        }
        total.add(p);
    }
    Ok(number(Ok(total.total())))
}

/// Sums the numbers a criteria match selects; errors there propagate.
pub(super) fn matched_numbers(
    m: &super::criteria::Matched,
    ctx: &mut dyn Context,
    f: &mut dyn FnMut(f64),
) -> Result<Result<(), ErrorKind>, Interrupt> {
    let mut err = None;
    m.each_value(ctx, &mut |v| match v {
        Value::Number(n) | Value::Date(n) => {
            f(*n);
            true
        }
        Value::Error(e) => {
            err = Some(*e);
            false
        }
        _ => true,
    })?;
    Ok(err.map_or(Ok(()), Err))
}

/// `SUMIF(range, criterion, [sum_range])`.
fn sumif(args: &[Expr], ctx: &mut dyn Context) -> Outcome {
    let values = args
        .get(2)
        .filter(|e| !matches!(e, Expr::Missing))
        .unwrap_or(&args[0]);
    let m = match evaluate_criteria(Some(values), true, &[(&args[0], &args[1])], ctx)? {
        Ok(m) => m,
        Err(e) => return Ok(Value::Error(e)),
    };
    sum_matched(&m, ctx)
}

/// `SUMIFS(sum_range, range1, criterion1, …)`.
fn sumifs(args: &[Expr], ctx: &mut dyn Context) -> Outcome {
    let Some(pairs) = pairs(&args[1..]) else {
        return Ok(Value::Error(ErrorKind::Value));
    };
    let m = match evaluate_criteria(Some(&args[0]), false, &pairs, ctx)? {
        Ok(m) => m,
        Err(e) => return Ok(Value::Error(e)),
    };
    sum_matched(&m, ctx)
}

fn sum_matched(m: &super::criteria::Matched, ctx: &mut dyn Context) -> Outcome {
    let mut total = Sum::default();
    Ok(match matched_numbers(m, ctx, &mut |n| total.add(n))? {
        Ok(()) => Value::Number(total.total()),
        Err(e) => Value::Error(e),
    })
}
