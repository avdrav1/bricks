//! Information functions (FML-5).

use super::{add, scalar, Outcome, Registry};
use crate::ast::{ErrorKind, Expr};
use crate::coerce::to_number;
use crate::eval::{evaluate, map, Context};
use crate::value::Value;

pub(super) fn register(m: &mut Registry) {
    add(m, "ISBLANK", 1, Some(1), |a, c| {
        is(a, c, |v| matches!(v, Value::Empty))
    });
    add(m, "ISNUMBER", 1, Some(1), |a, c| is(a, c, Value::is_number));
    add(m, "ISTEXT", 1, Some(1), |a, c| {
        is(a, c, |v| matches!(v, Value::Text(_)))
    });
    add(m, "ISLOGICAL", 1, Some(1), |a, c| {
        is(a, c, |v| matches!(v, Value::Bool(_)))
    });
    add(m, "ISERROR", 1, Some(1), |a, c| {
        is(a, c, |v| matches!(v, Value::Error(_)))
    });
    add(m, "ISERR", 1, Some(1), |a, c| {
        is(
            a,
            c,
            |v| matches!(v, Value::Error(e) if *e != ErrorKind::NA),
        )
    });
    add(m, "ISNA", 1, Some(1), |a, c| {
        is(a, c, |v| matches!(v, Value::Error(ErrorKind::NA)))
    });
    add(m, "ISEVEN", 1, Some(1), |a, c| parity(a, c, 0.0));
    add(m, "ISODD", 1, Some(1), |a, c| parity(a, c, 1.0));
    add(m, "NA", 0, Some(0), |_, _| Ok(Value::Error(ErrorKind::NA)));
    add(m, "N", 1, Some(1), n);
}

/// A type test, element-wise over arrays.
fn is(args: &[Expr], ctx: &mut dyn Context, test: fn(&Value) -> bool) -> Outcome {
    let v = evaluate(&args[0], ctx)?;
    Ok(map(v, &|v| Value::Bool(test(v))))
}

/// ISEVEN and ISODD: whether the number, truncated, has remainder `rem` modulo 2.
fn parity(args: &[Expr], ctx: &mut dyn Context, rem: f64) -> Outcome {
    let v = evaluate(&args[0], ctx)?;
    Ok(map(v, &|v| match to_number(v) {
        Ok(x) => Value::Bool(x.trunc().abs() % 2.0 == rem),
        Err(e) => Value::Error(e),
    }))
}

/// `N(value)`: a number as itself, TRUE as 1, anything else as 0; errors propagate.
fn n(args: &[Expr], ctx: &mut dyn Context) -> Outcome {
    Ok(match scalar(&args[0], ctx)? {
        Value::Number(x) | Value::Date(x) => Value::Number(x),
        Value::Bool(b) => Value::Number(f64::from(u8::from(b))),
        Value::Error(e) => Value::Error(e),
        _ => Value::Number(0.0),
    })
}
