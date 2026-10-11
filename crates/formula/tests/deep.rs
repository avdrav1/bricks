//! FML-2: a 1,000,000-row running total evaluates without overflowing the stack, and a
//! cycle gives #CIRC!. The driver here works the way the workbook's does (FML-3): the
//! context evaluates referenced formula cells in place up to a depth of 64, then
//! interrupts with `Deeper` so the driver evaluates the deeper cell first from its own
//! work stack. `evaluate` itself recurses only through expression depth.

use formula::{evaluate, literal, parse, Context, ErrorKind, Expr, Interrupt, Value};
use std::collections::HashSet;
use std::ops::Range;

const MAX_DEPTH: usize = 64;

/// One column of cells, each a formula or a literal.
struct Column {
    cells: Vec<Result<Expr, Value>>,
    values: Vec<Option<Value>>,
}

struct Eval<'a> {
    col: &'a mut Column,
    depth: usize,
    need: Option<u64>,
    in_progress: HashSet<u64>,
    position: u64,
}

impl Eval<'_> {
    fn eval_cell(&mut self, row: u64) -> Result<Value, Interrupt> {
        let expr = match &self.col.cells[row as usize] {
            Ok(e) => e.clone(),
            Err(v) => return Ok(v.clone()),
        };
        self.in_progress.insert(row);
        let saved = std::mem::replace(&mut self.position, row);
        self.depth += 1;
        let result = evaluate(&expr, self);
        self.depth -= 1;
        self.position = saved;
        self.in_progress.remove(&row);
        if let Ok(v) = &result {
            self.col.values[row as usize] = Some(v.clone());
        }
        result
    }
}

impl Context for Eval<'_> {
    fn cell(&mut self, sheet: Option<&str>, row: u64, col: u32) -> Result<Value, Interrupt> {
        assert!(sheet.is_none());
        if col != 0 || row as usize >= self.col.cells.len() {
            return Ok(Value::Empty);
        }
        if let Some(v) = &self.col.values[row as usize] {
            return Ok(v.clone());
        }
        if self.col.cells[row as usize].is_ok() {
            if self.in_progress.contains(&row) {
                return Ok(Value::Error(ErrorKind::Circ));
            }
            if self.depth >= MAX_DEPTH {
                self.need = Some(row);
                return Err(Interrupt::Deeper);
            }
        }
        self.eval_cell(row)
    }

    fn area(
        &mut self,
        _: Option<&str>,
        _: Range<u64>,
        _: Range<u32>,
        _: &mut dyn FnMut(u64, u32, &Value) -> bool,
    ) -> Result<(), Interrupt> {
        unreachable!("no ranges here")
    }

    fn size(&self, _: Option<&str>) -> Result<(u64, u32), ErrorKind> {
        Ok((self.col.cells.len() as u64, 1))
    }

    fn name(&self, _: &str) -> Option<Expr> {
        None
    }

    fn position(&self) -> (u64, u32) {
        (self.position, 0)
    }

    fn today(&self) -> f64 {
        0.0
    }

    fn now(&self) -> f64 {
        0.0
    }

    fn random(&mut self) -> f64 {
        0.5
    }
}

/// Evaluates `target` with an explicit work stack; returns its value and the deepest the
/// stack got.
fn drive(col: &mut Column, target: u64) -> (Value, usize) {
    let mut stack = vec![target];
    let mut on_stack: HashSet<u64> = HashSet::from([target]);
    let mut deepest = 1;
    while let Some(&top) = stack.last() {
        if col.values[top as usize].is_some() {
            stack.pop();
            on_stack.remove(&top);
            continue;
        }
        let mut ev = Eval {
            col,
            depth: 0,
            need: None,
            in_progress: HashSet::new(),
            position: top,
        };
        match ev.eval_cell(top) {
            Ok(_) => {}
            Err(Interrupt::Deeper) => {
                let need = ev.need.expect("Deeper names the cell it needs");
                if on_stack.contains(&need) {
                    col.values[need as usize] = Some(Value::Error(ErrorKind::Circ));
                } else {
                    stack.push(need);
                    on_stack.insert(need);
                    deepest = deepest.max(stack.len());
                }
            }
            Err(e) => panic!("unexpected {e:?}"),
        }
    }
    let v = col.values[target as usize].clone().expect("evaluated");
    (v, deepest)
}

fn column(raw: impl Iterator<Item = String>) -> Column {
    let cells: Vec<_> = raw
        .map(|r| match r.strip_prefix('=') {
            Some(f) => Ok(parse(f).expect("parses")),
            None => Err(literal(&r)),
        })
        .collect();
    let values = vec![None; cells.len()];
    Column { cells, values }
}

#[test]
fn million_row_running_total() {
    const N: u64 = 1_000_000;
    std::thread::Builder::new()
        .stack_size(8 << 20)
        .spawn(|| {
            // Row 0 holds 1; row n holds =A{n}+1 (the row above, 1-based A{n}).
            let raw = std::iter::once("1".to_string()).chain((1..N).map(|n| format!("=A{n}+1")));
            let mut col = column(raw);
            let (v, deepest) = drive(&mut col, N - 1);
            assert_eq!(v, Value::Number(N as f64));
            assert!(
                deepest > 1000,
                "the driver's stack did the deep work ({deepest})"
            );
            assert!(
                col.values[1..].iter().all(Option::is_some),
                "every formula row is cached"
            );
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn cycles_give_circ() {
    let mut col = column(["=A1+1".to_string()].into_iter());
    assert_eq!(drive(&mut col, 0).0, Value::Error(ErrorKind::Circ));

    // A cycle longer than the in-place depth goes through the driver's stack.
    let n = 500;
    let raw = (0..n).map(|r| format!("=A{}", (r + 1) % n + 1));
    let mut col = column(raw);
    assert_eq!(drive(&mut col, 0).0, Value::Error(ErrorKind::Circ));
}
