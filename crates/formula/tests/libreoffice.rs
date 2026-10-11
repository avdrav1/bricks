//! FML-2, FML-5..8: every case in `tests/data/cases.tsv` evaluates as LibreOffice Calc did
//! (`cases-libreoffice.tsv`, recorded by `scripts/lo_functions.py`), or, where a case
//! names an `excel` result, as Excel does and LibreOffice doesn't.
//!
//! Each case's inputs sit in A1:E5 of a sheet of their own, read through `literal` as a
//! raw cell would be; the formula is in G1.

use formula::{evaluate, literal, parse, Context, ErrorKind, Expr, Interrupt, Value};
use std::collections::HashMap;
use std::ops::Range;

const ROWS: u64 = 5;
const COLS: u32 = 5;

struct Inputs {
    cells: HashMap<(u64, u32), Value>,
    random: u64,
}

impl Inputs {
    fn new(spec: &str) -> Self {
        let mut cells = HashMap::new();
        for part in split_inputs(spec) {
            let (at, raw) = part.split_once('=').expect("cell=value");
            let r = formula::parse_reference(at).expect("a cell");
            let formula::Area::Cell(a) = r.area else {
                panic!("{at} is not a cell")
            };
            let v = literal(raw);
            if v != Value::Empty {
                cells.insert((a.row, a.col), v);
            }
        }
        Inputs {
            cells,
            random: 0x2545_f491_4f6c_dd1d,
        }
    }
}

fn split_inputs(spec: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut chars = spec.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&';') => {
                cur.push(';');
                chars.next();
            }
            ';' => parts.push(std::mem::take(&mut cur)),
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    parts
}

impl Context for Inputs {
    fn cell(&mut self, sheet: Option<&str>, row: u64, col: u32) -> Result<Value, Interrupt> {
        if sheet.is_some() {
            return Ok(Value::Error(ErrorKind::Ref));
        }
        Ok(self.cells.get(&(row, col)).cloned().unwrap_or(Value::Empty))
    }

    fn area(
        &mut self,
        sheet: Option<&str>,
        rows: Range<u64>,
        cols: Range<u32>,
        visit: &mut dyn FnMut(u64, u32, &Value) -> bool,
    ) -> Result<(), Interrupt> {
        assert!(sheet.is_none());
        for r in rows.start..rows.end.min(ROWS) {
            for c in cols.start..cols.end.min(COLS) {
                if let Some(v) = self.cells.get(&(r, c)) {
                    if !visit(r, c, v) {
                        return Ok(());
                    }
                }
            }
        }
        Ok(())
    }

    fn size(&self, sheet: Option<&str>) -> Result<(u64, u32), ErrorKind> {
        match sheet {
            None => Ok((ROWS, COLS)),
            Some(_) => Err(ErrorKind::Ref),
        }
    }

    fn name(&self, _: &str) -> Option<Expr> {
        None
    }

    fn position(&self) -> (u64, u32) {
        (0, 6)
    }

    fn today(&self) -> f64 {
        45_292.0
    }

    fn now(&self) -> f64 {
        45_292.5
    }

    fn random(&mut self) -> f64 {
        self.random ^= self.random << 13;
        self.random ^= self.random >> 7;
        self.random ^= self.random << 17;
        (self.random >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// `(kind, result)` as the fixture writes it.
fn kind_of(v: &Value) -> (&'static str, String) {
    match v.scalar() {
        Value::Empty => ("value", "0.0".to_string()),
        Value::Number(n) | Value::Date(n) => ("value", format!("{n:?}")),
        Value::Text(t) => ("text", t.to_string()),
        Value::Bool(b) => ("bool", (if *b { "TRUE" } else { "FALSE" }).to_string()),
        Value::Error(e) => ("error", e.text().to_string()),
        Value::Array(_) => unreachable!("scalar() unwraps arrays"),
    }
}

fn same(kind: &str, want: &str, got: &(&str, String)) -> bool {
    if kind != got.0 {
        return false;
    }
    if kind == "value" {
        let (a, b): (f64, f64) = (want.parse().expect("a number"), got.1.parse().unwrap());
        return a == b || (a - b).abs() <= 1e-9 * a.abs().max(b.abs());
    }
    want == got.1
}

#[test]
fn cases_match_libreoffice() {
    let data = include_str!("data/cases-libreoffice.tsv");
    let mut failures = Vec::new();
    let mut groups: HashMap<&str, usize> = HashMap::new();
    for line in data.lines().skip(1) {
        let cols: Vec<&str> = line.split('\t').collect();
        let [group, src, inputs, kind, result, excel, why] = cols[..] else {
            panic!("bad fixture line {line:?}")
        };
        *groups.entry(group).or_default() += 1;
        let expr = parse(src).unwrap_or_else(|e| panic!("{src}: {e}"));
        let mut ctx = Inputs::new(inputs);
        let got = kind_of(&evaluate(&expr, &mut ctx).expect("no interrupts"));
        let (want_kind, want) = if excel.is_empty() {
            (kind, result)
        } else {
            assert!(!why.is_empty(), "{src}: an Excel override needs a reason");
            assert!(
                !same(kind, result, &kind_of_text(excel)),
                "{src}: LibreOffice now agrees with Excel ({kind} {result}); drop the override"
            );
            excel.split_once(':').expect("kind:result")
        };
        if !same(want_kind, want, &got) {
            failures.push(format!(
                "{group} ={src} [{inputs}]: want {want_kind} {want}, got {} {}",
                got.0, got.1
            ));
        }
    }
    assert!(groups.get("ops").is_some_and(|&n| n >= 150), "{groups:?}");
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

fn kind_of_text(spec: &str) -> (&str, String) {
    let (k, v) = spec.split_once(':').expect("kind:result");
    (k, v.to_string())
}
