//! Logic functions (FML-5). IF, IFS, IFERROR, IFNA, and SWITCH evaluate only the
//! arguments they need.

use super::{add, each_value, scalar, Outcome, Registry};
use crate::ast::{ErrorKind, Expr};
use crate::coerce::{compare, to_bool};
use crate::eval::{evaluate, map, Context};
use crate::value::Value;
use std::cmp::Ordering;

pub(super) fn register(m: &mut Registry) {
    add(m, "IF", 1, Some(3), if_);
    add(m, "IFS", 2, None, ifs);
    add(m, "AND", 1, None, |a, c| {
        fold(a, c, |acc, b| acc && b, true)
    });
    add(m, "OR", 1, None, |a, c| {
        fold(a, c, |acc, b| acc || b, false)
    });
    add(m, "XOR", 1, None, |a, c| {
        fold(a, c, |acc, b| acc != b, false)
    });
    add(m, "NOT", 1, Some(1), |a, c| {
        let v = evaluate(&a[0], c)?;
        Ok(map(v, &|v| match to_bool(v) {
            Ok(b) => Value::Bool(!b),
            Err(e) => Value::Error(e),
        }))
    });
    add(m, "IFERROR", 2, Some(2), |a, c| if_error(a, c, |_| true));
    add(m, "IFNA", 2, Some(2), |a, c| {
        if_error(a, c, |e| e == ErrorKind::NA)
    });
    add(m, "SWITCH", 3, None, switch);
}

/// A condition's truth, or the error it is.
fn condition(
    arg: &Expr,
    ctx: &mut dyn Context,
) -> Result<Result<bool, ErrorKind>, crate::eval::Interrupt> {
    Ok(to_bool(&scalar(arg, ctx)?))
}

/// `IF(condition, [then], [else])`: an absent `else` is FALSE.
fn if_(args: &[Expr], ctx: &mut dyn Context) -> Outcome {
    match condition(&args[0], ctx)? {
        Err(e) => Ok(Value::Error(e)),
        Ok(true) => match args.get(1) {
            Some(e) => evaluate(e, ctx),
            None => Ok(Value::Bool(true)),
        },
        Ok(false) => match args.get(2) {
            Some(e) => evaluate(e, ctx),
            None => Ok(Value::Bool(false)),
        },
    }
}

/// `IFS(condition1, value1, …)`: the value of the first true condition, else `#N/A`.
fn ifs(args: &[Expr], ctx: &mut dyn Context) -> Outcome {
    let (pairs, []) = args.as_chunks::<2>() else {
        return Ok(Value::Error(ErrorKind::Value));
    };
    for [test, value] in pairs {
        match condition(test, ctx)? {
            Err(e) => return Ok(Value::Error(e)),
            Ok(true) => return evaluate(value, ctx),
            Ok(false) => {}
        }
    }
    Ok(Value::Error(ErrorKind::NA))
}

/// AND, OR, and XOR: booleans and numbers count, text and empty cells in references and
/// arrays are skipped, direct text must read as a boolean; with nothing to count the
/// result is `#VALUE!`.
fn fold(args: &[Expr], ctx: &mut dyn Context, op: fn(bool, bool) -> bool, init: bool) -> Outcome {
    let mut acc = init;
    let mut any = false;
    let mut err = None;
    for a in args.iter().filter(|a| !matches!(a, Expr::Missing)) {
        each_value(a, ctx, &mut |v, from_ref| {
            let b = match v {
                Value::Text(_) | Value::Empty if from_ref => return true,
                v => to_bool(v),
            };
            match b {
                Ok(b) => {
                    acc = op(acc, b);
                    any = true;
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
    Ok(if any {
        Value::Bool(acc)
    } else {
        Value::Error(ErrorKind::Value)
    })
}

/// IFERROR and IFNA: the first argument, or the second where it is an error `catches`
/// accepts (element-wise over arrays).
fn if_error(args: &[Expr], ctx: &mut dyn Context, catches: fn(ErrorKind) -> bool) -> Outcome {
    let caught = |v: &Value| matches!(v, Value::Error(e) if catches(*e));
    let v = evaluate(&args[0], ctx)?;
    match &v {
        Value::Error(e) if catches(*e) => evaluate(&args[1], ctx),
        Value::Array(a) if a.values.iter().any(caught) => {
            let alt = evaluate(&args[1], ctx)?.scalar().clone();
            Ok(map(v, &|x| if caught(x) { alt.clone() } else { x.clone() }))
        }
        _ => Ok(v),
    }
}

/// `SWITCH(expression, value1, result1, …, [default])`: the result for the first value
/// equal to the expression (text ignoring case), else the default, else `#N/A`.
fn switch(args: &[Expr], ctx: &mut dyn Context) -> Outcome {
    let x = scalar(&args[0], ctx)?;
    if let Value::Error(_) = x {
        return Ok(x);
    }
    let (pairs, default) = args[1..].as_chunks::<2>();
    for [value, result] in pairs {
        let candidate = scalar(value, ctx)?;
        if let Value::Error(_) = candidate {
            return Ok(candidate);
        }
        let same_type = std::mem::discriminant(&x) == std::mem::discriminant(&candidate)
            || (x.is_number() && candidate.is_number())
            || matches!(x, Value::Empty)
            || matches!(candidate, Value::Empty);
        if same_type && compare(&x, &candidate) == Ordering::Equal {
            return evaluate(result, ctx);
        }
    }
    match default {
        [d] => evaluate(d, ctx),
        _ => Ok(Value::Error(ErrorKind::NA)),
    }
}
