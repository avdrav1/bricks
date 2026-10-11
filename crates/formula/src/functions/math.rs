//! Math functions (FML-5).

use super::{add, each_value, Outcome, Registry};
use crate::ast::{ErrorKind, Expr};
use crate::coerce::to_number;
use crate::eval::Context;
use crate::value::Value;

pub(super) fn register(m: &mut Registry) {
    add(m, "SUM", 0, None, sum);
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
) -> Result<Result<(), ErrorKind>, crate::eval::Interrupt> {
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

fn sum(args: &[Expr], ctx: &mut dyn Context) -> Outcome {
    let mut total = Sum::default();
    Ok(match each_number(args, ctx, &mut |n| total.add(n))? {
        Ok(()) => Value::Number(total.total()),
        Err(e) => Value::Error(e),
    })
}
