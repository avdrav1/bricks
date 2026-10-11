//! Statistics functions (FML-5).

use super::criteria::{evaluate_criteria, pairs, Matched};
use super::math::{each_number, matched_numbers, number, snap, Sum};
use super::{add, each_value, scalar, Outcome, Registry};
use crate::ast::{ErrorKind, Expr};
use crate::coerce::{text_to_number, to_number};
use crate::eval::{as_reference, bounds, Context, Interrupt};
use crate::value::Value;
use std::collections::HashMap;

pub(super) fn register(m: &mut Registry) {
    add(m, "AVERAGE", 1, None, |a, c| with_numbers(a, c, average));
    add(m, "AVERAGEA", 1, None, averagea);
    add(m, "AVERAGEIF", 2, Some(3), averageif);
    add(m, "AVERAGEIFS", 3, None, |a, c| ifs(a, c, average_matched));
    add(m, "COUNT", 1, None, count);
    add(m, "COUNTA", 1, None, counta);
    add(m, "COUNTBLANK", 1, Some(1), countblank);
    add(m, "COUNTIF", 2, Some(2), countif);
    add(m, "COUNTIFS", 2, None, countifs);
    add(m, "MIN", 1, None, |a, c| {
        with_numbers(a, c, |ns| {
            Ok(ns.iter().copied().reduce(f64::min).unwrap_or(0.0))
        })
    });
    add(m, "MAX", 1, None, |a, c| {
        with_numbers(a, c, |ns| {
            Ok(ns.iter().copied().reduce(f64::max).unwrap_or(0.0))
        })
    });
    add(m, "MINIFS", 3, None, |a, c| {
        ifs(a, c, |m, c| extreme_matched(m, c, f64::min))
    });
    add(m, "MAXIFS", 3, None, |a, c| {
        ifs(a, c, |m, c| extreme_matched(m, c, f64::max))
    });
    add(m, "MEDIAN", 1, None, |a, c| with_numbers(a, c, median));
    add(m, "MODE", 1, None, |a, c| with_numbers(a, c, mode));
    add(m, "MODE.SNGL", 1, None, |a, c| with_numbers(a, c, mode));
    add(m, "STDEV", 1, None, |a, c| {
        with_numbers(a, c, |ns| variance(ns, 1).map(f64::sqrt))
    });
    add(m, "STDEV.S", 1, None, |a, c| {
        with_numbers(a, c, |ns| variance(ns, 1).map(f64::sqrt))
    });
    add(m, "STDEV.P", 1, None, |a, c| {
        with_numbers(a, c, |ns| variance(ns, 0).map(f64::sqrt))
    });
    add(m, "VAR", 1, None, |a, c| {
        with_numbers(a, c, |ns| variance(ns, 1))
    });
    add(m, "VAR.S", 1, None, |a, c| {
        with_numbers(a, c, |ns| variance(ns, 1))
    });
    add(m, "VAR.P", 1, None, |a, c| {
        with_numbers(a, c, |ns| variance(ns, 0))
    });
    add(m, "LARGE", 2, Some(2), |a, c| nth(a, c, true));
    add(m, "SMALL", 2, Some(2), |a, c| nth(a, c, false));
    add(m, "RANK", 2, Some(3), rank);
}

/// The numbers the arguments give an aggregate, or the first error among them.
fn numbers(args: &[Expr], ctx: &mut dyn Context) -> Result<Result<Vec<f64>, ErrorKind>, Interrupt> {
    let mut ns = Vec::new();
    Ok(each_number(args, ctx, &mut |n| ns.push(n))?.map(|()| ns))
}

/// Applies `f` to the arguments' numbers.
fn with_numbers(
    args: &[Expr],
    ctx: &mut dyn Context,
    f: fn(&[f64]) -> Result<f64, ErrorKind>,
) -> Outcome {
    Ok(number(numbers(args, ctx)?.and_then(|ns| f(&ns))))
}

fn average(ns: &[f64]) -> Result<f64, ErrorKind> {
    if ns.is_empty() {
        return Err(ErrorKind::Div0);
    }
    let mut s = Sum::default();
    ns.iter().for_each(|&n| s.add(n));
    Ok(s.total() / ns.len() as f64)
}

/// The middle number; LibreOffice gives `#VALUE!` for none.
fn median(ns: &[f64]) -> Result<f64, ErrorKind> {
    if ns.is_empty() {
        return Err(ErrorKind::Value);
    }
    let mut ns = ns.to_vec();
    ns.sort_by(f64::total_cmp);
    let k = ns.len() / 2;
    Ok(if ns.len() % 2 == 1 {
        ns[k]
    } else {
        (ns[k - 1] + ns[k]) / 2.0
    })
}

/// The most frequent number, the first met among equally frequent ones; LibreOffice gives
/// `#VALUE!` when no number repeats.
fn mode(ns: &[f64]) -> Result<f64, ErrorKind> {
    let mut counts: HashMap<u64, usize> = HashMap::new();
    for n in ns {
        *counts.entry((n + 0.0).to_bits()).or_default() += 1;
    }
    let mut best = None;
    let mut most = 1;
    for n in ns {
        let c = counts[&(n + 0.0).to_bits()];
        if c > most {
            most = c;
            best = Some(*n);
        }
    }
    best.ok_or(ErrorKind::Value)
}

/// The variance, divided by `n - ddof` (1: a sample, 0: the population), computed in two
/// passes so that large values with small spreads keep their digits.
fn variance(ns: &[f64], ddof: usize) -> Result<f64, ErrorKind> {
    if ns.len() <= ddof {
        return Err(ErrorKind::Div0);
    }
    let mean = average(ns)?;
    let mut s = Sum::default();
    ns.iter().for_each(|&n| s.add((n - mean) * (n - mean)));
    Ok(s.total() / (ns.len() - ddof) as f64)
}

/// `LARGE(array, k)` and `SMALL(array, k)`; `k` is rounded up, and outside 1..=n it is
/// `#VALUE!` as in LibreOffice.
fn nth(args: &[Expr], ctx: &mut dyn Context, largest: bool) -> Outcome {
    let mut ns = match numbers(&args[..1], ctx)? {
        Ok(ns) => ns,
        Err(e) => return Ok(Value::Error(e)),
    };
    let k = match to_number(&scalar(&args[1], ctx)?) {
        Ok(k) => snap(k).ceil(),
        Err(e) => return Ok(Value::Error(e)),
    };
    if k < 1.0 || k > ns.len() as f64 {
        return Ok(Value::Error(ErrorKind::Value));
    }
    let k = k as usize - 1;
    let (_, v, _) = if largest {
        ns.select_nth_unstable_by(k, |a, b| b.total_cmp(a))
    } else {
        ns.select_nth_unstable_by(k, f64::total_cmp)
    };
    Ok(Value::Number(*v))
}

/// `RANK(number, ref, [order])`: 1 plus how many numbers in `ref` are larger (order 0, the
/// default) or smaller (any other order); `#N/A` when `number` isn't among them.
fn rank(args: &[Expr], ctx: &mut dyn Context) -> Outcome {
    let x = match to_number(&scalar(&args[0], ctx)?) {
        Ok(x) => x,
        Err(e) => return Ok(Value::Error(e)),
    };
    let ascending = match args.get(2) {
        None | Some(Expr::Missing) => false,
        Some(e) => match to_number(&scalar(e, ctx)?) {
            Ok(o) => o != 0.0,
            Err(e) => return Ok(Value::Error(e)),
        },
    };
    let (mut before, mut found) = (0u64, false);
    let r = each_number(&args[1..2], ctx, &mut |n| {
        found |= n == x;
        before += u64::from(if ascending { n < x } else { n > x });
    })?;
    Ok(match r {
        Err(e) => Value::Error(e),
        Ok(()) if found => Value::Number((before + 1) as f64),
        Ok(()) => Value::Error(ErrorKind::NA),
    })
}

/// `AVERAGEA`: through references and arrays text counts as 0 and booleans as 0 or 1;
/// direct arguments are coerced as in other aggregates.
fn averagea(args: &[Expr], ctx: &mut dyn Context) -> Outcome {
    let mut s = Sum::default();
    let mut n = 0u64;
    let mut err = None;
    for a in args.iter().filter(|a| !matches!(a, Expr::Missing)) {
        each_value(a, ctx, &mut |v, from_ref| {
            let x = match v {
                Value::Empty if from_ref => return true,
                Value::Error(e) => {
                    err = Some(*e);
                    return false;
                }
                Value::Text(_) if from_ref => Ok(0.0),
                v => to_number(v),
            };
            match x {
                Ok(x) => {
                    s.add(x);
                    n += 1;
                    true
                }
                Err(e) => {
                    err = Some(e);
                    false
                }
            }
        })?;
        if let Some(e) = err {
            return Ok(Value::Error(e));
        }
    }
    Ok(if n == 0 {
        Value::Error(ErrorKind::Div0)
    } else {
        Value::Number(s.total() / n as f64)
    })
}

/// `COUNT`: numbers and dates in references and arrays; direct arguments that read as
/// numbers (booleans, number text) count too.
fn count(args: &[Expr], ctx: &mut dyn Context) -> Outcome {
    let mut n = 0u64;
    for a in args.iter().filter(|a| !matches!(a, Expr::Missing)) {
        each_value(a, ctx, &mut |v, from_ref| {
            n += u64::from(match v {
                Value::Number(_) | Value::Date(_) => true,
                _ if from_ref => false,
                Value::Bool(_) => true,
                Value::Text(t) => text_to_number(t).is_some(),
                _ => false,
            });
            true
        })?;
    }
    Ok(Value::Number(n as f64))
}

/// `COUNTA`: every value that isn't empty, errors included.
fn counta(args: &[Expr], ctx: &mut dyn Context) -> Outcome {
    let mut n = 0u64;
    for a in args.iter().filter(|a| !matches!(a, Expr::Missing)) {
        each_value(a, ctx, &mut |v, from_ref| {
            n += u64::from(!(from_ref && matches!(v, Value::Empty)));
            true
        })?;
    }
    Ok(Value::Number(n as f64))
}

/// `COUNTBLANK(range)`: empty cells and empty texts; the argument must be a reference.
fn countblank(args: &[Expr], ctx: &mut dyn Context) -> Outcome {
    let Some(r) = as_reference(&args[0], ctx) else {
        return Ok(Value::Error(ErrorKind::Value));
    };
    let (rows, cols) = match bounds(&r, ctx) {
        Ok(b) => b,
        Err(e) => return Ok(Value::Error(e)),
    };
    let total = (rows.end - rows.start) * u64::from(cols.end - cols.start);
    let mut filled = 0u64;
    ctx.area(r.sheet.as_deref(), rows, cols, &mut |_, _, v| {
        filled +=
            u64::from(!matches!(v, Value::Empty) && !matches!(v, Value::Text(t) if t.is_empty()));
        true
    })?;
    Ok(Value::Number((total - filled) as f64))
}

fn countif(args: &[Expr], ctx: &mut dyn Context) -> Outcome {
    Ok(
        match evaluate_criteria(None, false, &[(&args[0], &args[1])], ctx)? {
            Ok(m) => Value::Number(m.count() as f64),
            Err(e) => Value::Error(e),
        },
    )
}

fn countifs(args: &[Expr], ctx: &mut dyn Context) -> Outcome {
    let Some(pairs) = pairs(args) else {
        return Ok(Value::Error(ErrorKind::Value));
    };
    Ok(match evaluate_criteria(None, false, &pairs, ctx)? {
        Ok(m) => Value::Number(m.count() as f64),
        Err(e) => Value::Error(e),
    })
}

/// `AVERAGEIF(range, criterion, [average_range])`.
fn averageif(args: &[Expr], ctx: &mut dyn Context) -> Outcome {
    let values = args
        .get(2)
        .filter(|e| !matches!(e, Expr::Missing))
        .unwrap_or(&args[0]);
    match evaluate_criteria(Some(values), true, &[(&args[0], &args[1])], ctx)? {
        Ok(m) => average_matched(&m, ctx),
        Err(e) => Ok(Value::Error(e)),
    }
}

/// The …IFS functions whose first argument is the range to aggregate.
fn ifs(
    args: &[Expr],
    ctx: &mut dyn Context,
    f: fn(&Matched, &mut dyn Context) -> Outcome,
) -> Outcome {
    let Some(pairs) = pairs(&args[1..]) else {
        return Ok(Value::Error(ErrorKind::Value));
    };
    match evaluate_criteria(Some(&args[0]), false, &pairs, ctx)? {
        Ok(m) => f(&m, ctx),
        Err(e) => Ok(Value::Error(e)),
    }
}

fn average_matched(m: &Matched, ctx: &mut dyn Context) -> Outcome {
    let (mut s, mut n) = (Sum::default(), 0u64);
    let r = matched_numbers(m, ctx, &mut |x| {
        s.add(x);
        n += 1;
    })?;
    Ok(match r {
        Err(e) => Value::Error(e),
        Ok(()) if n == 0 => Value::Error(ErrorKind::Div0),
        Ok(()) => Value::Number(s.total() / n as f64),
    })
}

/// MINIFS and MAXIFS: 0 when nothing matches.
fn extreme_matched(m: &Matched, ctx: &mut dyn Context, pick: fn(f64, f64) -> f64) -> Outcome {
    let mut best: Option<f64> = None;
    let r = matched_numbers(m, ctx, &mut |n| best = Some(best.map_or(n, |b| pick(b, n))))?;
    Ok(number(r.map(|()| best.unwrap_or(0.0))))
}
