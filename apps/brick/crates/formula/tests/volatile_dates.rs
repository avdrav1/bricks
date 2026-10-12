//! FML-8: `TODAY` and `NOW` read the context's clock, so they aren't in the LibreOffice
//! fixture; these properties hold for any clock.

use formula::functions::is_volatile;
use formula::{evaluate, parse, result_text, Context, ErrorKind, Expr, Interrupt, Value};
use std::ops::Range;

/// An empty sheet whose clock reads `now` (a local serial with the time as fraction).
struct Clock {
    now: f64,
}

impl Context for Clock {
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
        Ok((1, 1))
    }

    fn name(&self, _: &str) -> Option<Expr> {
        None
    }

    fn position(&self) -> (u64, u32) {
        (0, 0)
    }

    fn today(&self) -> f64 {
        self.now.floor()
    }

    fn now(&self) -> f64 {
        self.now
    }

    fn random(&mut self) -> f64 {
        0.5
    }
}

fn eval(src: &str, now: f64) -> Value {
    let expr = parse(src).unwrap_or_else(|e| panic!("{src}: {e}"));
    evaluate(&expr, &mut Clock { now }).expect("no interrupts")
}

/// Clocks from 1899 to 2100, at midnight, mid-day, and just before midnight.
fn clocks() -> impl Iterator<Item = f64> {
    (-400..73_000)
        .step_by(997)
        .flat_map(|d| [0.0, 0.25, 0.5, 86_399.0 / 86_400.0].map(|t| f64::from(d) + t))
}

#[test]
fn today_is_the_date_of_now() {
    for now in clocks() {
        assert_eq!(eval("TODAY()", now), Value::Date(now.floor()), "{now}");
        assert_eq!(eval("NOW()", now), Value::Date(now), "{now}");
    }
}

#[test]
fn now_splits_into_today_and_its_time() {
    for now in clocks() {
        let n = |src: &str| match eval(src, now) {
            Value::Number(n) | Value::Date(n) => n,
            v => panic!("{src} at {now}: {v:?}"),
        };
        assert_eq!(
            n("DATE(YEAR(TODAY()),MONTH(TODAY()),DAY(TODAY()))"),
            now.floor(),
            "{now}"
        );
        let seconds = n("HOUR(NOW())*3600+MINUTE(NOW())*60+SECOND(NOW())");
        assert_eq!(
            seconds,
            ((now - now.floor()) * 86_400.0).round() % 86_400.0,
            "{now}"
        );
        assert!(
            n("NOW()-TODAY()") < 1.0 && n("NOW()-TODAY()") >= 0.0,
            "{now}"
        );
    }
}

#[test]
fn date_arithmetic_with_today() {
    let now = 45_292.75; // 2024-01-01 18:00
    assert_eq!(eval("TODAY()-DATE(2023,12,25)", now), Value::Number(7.0));
    assert_eq!(eval("TODAY()+30", now), Value::Date(45_322.0));
    assert_eq!(result_text(&eval("TODAY()+30", now)), "2024-01-31");
    assert_eq!(result_text(&eval("NOW()", now)), "2024-01-01 18:00:00");
    assert_eq!(result_text(&eval("NOW()-TODAY()", now)), "0.75");
    assert_eq!(
        eval("DATEDIF(DATE(2000,2,29),TODAY(),\"Y\")", now),
        Value::Number(23.0)
    );
    assert_eq!(eval("EOMONTH(TODAY(),1)", now), Value::Date(45_351.0));
}

#[test]
fn time_values_show_as_times() {
    assert_eq!(result_text(&eval("TIME(13,5,7)", 0.0)), "13:05:07");
    assert_eq!(
        result_text(&eval("TIMEVALUE(\"6:30 PM\")", 0.0)),
        "18:30:00"
    );
}

#[test]
fn only_the_clock_functions_are_volatile() {
    assert!(is_volatile("TODAY") && is_volatile("NOW"));
    for f in ["DATE", "TIME", "YEAR", "WEEKDAY", "WORKDAY", "DATEVALUE"] {
        assert!(!is_volatile(f), "{f}");
    }
}

#[test]
fn a_date_plus_a_time_shows_as_a_date_time() {
    let v = eval("DATE(2024,1,1)+TIME(6,0,0)", 45_292.5);
    assert_eq!(result_text(&v), "2024-01-01 06:00:00");
}
