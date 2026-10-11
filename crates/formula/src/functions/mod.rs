//! The function registry. Functions receive their arguments unevaluated, so `IF` and
//! `IFERROR` evaluate only the branch they need and aggregates stream reference areas
//! through [`Context::area`] without materializing them. A wrong argument count is
//! `#VALUE!` (checked by the evaluator from `min`/`max`); the parser accepts any count.

use crate::ast::{Area, ErrorKind, Expr};
use crate::eval::{as_reference, Context, Interrupt};
use crate::value::Value;
use std::collections::HashMap;
use std::sync::LazyLock;

/// A function's result, or why evaluation stopped.
pub type Outcome = Result<Value, Interrupt>;

pub struct Function {
    pub min: usize,
    pub max: Option<usize>,
    /// Its result can change without any cell changing (`RAND`, `TODAY`).
    pub volatile: bool,
    pub call: fn(&[Expr], &mut dyn Context) -> Outcome,
}

static FUNCTIONS: LazyLock<HashMap<&'static str, Function>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    register(&mut m);
    m
});

/// The function called `name` (uppercase, without `_xlfn.`).
pub fn get(name: &str) -> Option<&'static Function> {
    FUNCTIONS.get(name)
}

pub fn is_known(name: &str) -> bool {
    FUNCTIONS.contains_key(name)
}

pub fn is_volatile(name: &str) -> bool {
    FUNCTIONS.get(name).is_some_and(|f| f.volatile)
}

type Registry = HashMap<&'static str, Function>;

fn add(
    m: &mut Registry,
    name: &'static str,
    min: usize,
    max: Option<usize>,
    call: fn(&[Expr], &mut dyn Context) -> Outcome,
) {
    m.insert(
        name,
        Function {
            min,
            max,
            volatile: false,
            call,
        },
    );
}

fn register(m: &mut Registry) {
    add(m, "ROW", 0, Some(1), row);
    add(m, "COLUMN", 0, Some(1), column);
    add(m, "TRUE", 0, Some(0), |_, _| Ok(Value::Bool(true)));
    add(m, "FALSE", 0, Some(0), |_, _| Ok(Value::Bool(false)));
}

/// `ROW()`: the formula's own row; `ROW(ref)`: the reference's first row (1-based).
fn row(args: &[Expr], ctx: &mut dyn Context) -> Outcome {
    position_of(args, ctx, true)
}

/// `COLUMN()` and `COLUMN(ref)`, like [`row`].
fn column(args: &[Expr], ctx: &mut dyn Context) -> Outcome {
    position_of(args, ctx, false)
}

fn position_of(args: &[Expr], ctx: &mut dyn Context, rows: bool) -> Outcome {
    let (row, col) = match args.first() {
        None | Some(Expr::Missing) => ctx.position(),
        Some(arg) => match as_reference(arg, ctx) {
            None => return Ok(Value::Error(ErrorKind::Value)),
            Some(r) => match r.area {
                Area::Cell(a) | Area::Range(a, _) => (a.row, a.col),
                Area::Cols { first, .. } => (0, first),
                Area::Rows { first, .. } => (first, 0),
            },
        },
    };
    Ok(Value::Number(
        if rows { row + 1 } else { u64::from(col) + 1 } as f64,
    ))
}
