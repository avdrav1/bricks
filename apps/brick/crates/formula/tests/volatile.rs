//! FML-5: RAND and RANDBETWEEN, which the LibreOffice fixture can't record, tested by their
//! properties over many draws.

use formula::functions::is_volatile;
use formula::{evaluate, parse, Context, ErrorKind, Expr, Interrupt, Value};
use std::ops::Range;

/// An empty sheet whose random numbers come from a xorshift generator.
struct Random(u64);

impl Context for Random {
    fn cell(&mut self, _: Option<&str>, _: u64, _: u32) -> Result<Value, Interrupt> {
        Ok(Value::Empty)
    }

    fn area(
        &mut self,
        _: Option<&str>,
        _: Range<u64>,
        _: Range<u32>,
        _: &mut dyn FnMut(u64, u32, &Value) -> bool,
    ) -> Result<(), Interrupt> {
        Ok(())
    }

    fn size(&self, _: Option<&str>) -> Result<(u64, u32), ErrorKind> {
        Ok((5, 5))
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
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn draws(src: &str, n: usize) -> Vec<Value> {
    let expr = parse(src).unwrap();
    let mut ctx = Random(0x9e37_79b9_7f4a_7c15);
    (0..n).map(|_| evaluate(&expr, &mut ctx).unwrap()).collect()
}

fn numbers(src: &str, n: usize) -> Vec<f64> {
    draws(src, n)
        .into_iter()
        .map(|v| match v {
            Value::Number(x) => x,
            v => panic!("{src} gave {v:?}"),
        })
        .collect()
}

#[test]
fn rand_is_volatile_in_unit_interval_and_varies() {
    assert!(is_volatile("RAND") && is_volatile("RANDBETWEEN"));
    let xs = numbers("RAND()", 1000);
    assert!(xs.iter().all(|x| (0.0..1.0).contains(x)), "{xs:?}");
    let mut distinct = xs.clone();
    distinct.sort_by(f64::total_cmp);
    distinct.dedup();
    assert!(distinct.len() > 990, "only {} distinct", distinct.len());
    let mean = xs.iter().sum::<f64>() / xs.len() as f64;
    assert!((0.4..0.6).contains(&mean), "mean {mean}");
}

#[test]
fn randbetween_gives_every_integer_in_its_bounds() {
    let xs = numbers("RANDBETWEEN(-2,3)", 2000);
    assert!(xs
        .iter()
        .all(|x| x.fract() == 0.0 && (-2.0..=3.0).contains(x)));
    for k in -2..=3 {
        assert!(xs.contains(&f64::from(k)), "{k} never drawn");
    }
    assert!(numbers("RANDBETWEEN(7,7)", 20).iter().all(|&x| x == 7.0));
}

#[test]
fn randbetween_rounds_bounds_up_as_libreoffice_does() {
    // LibreOffice: RANDBETWEEN(1.5,1.7) is 2 and RANDBETWEEN(2.5,2.5) is 3.
    assert!(numbers("RANDBETWEEN(1.5,1.7)", 50)
        .iter()
        .all(|&x| x == 2.0));
    assert!(numbers("RANDBETWEEN(2.5,2.5)", 50)
        .iter()
        .all(|&x| x == 3.0));
}

#[test]
fn randbetween_rejects_bad_arguments() {
    assert_eq!(
        draws("RANDBETWEEN(5,1)", 1)[0],
        Value::Error(ErrorKind::Num)
    );
    assert_eq!(
        draws("RANDBETWEEN(\"x\",1)", 1)[0],
        Value::Error(ErrorKind::Value)
    );
    assert_eq!(
        draws("RANDBETWEEN(1,NA())", 1)[0],
        Value::Error(ErrorKind::NA)
    );
}
