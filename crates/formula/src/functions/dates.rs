//! Date and time functions (FML-8). A date is a serial day number since 1899-12-30
//! (LibreOffice's epoch, see [`serial_from_civil`]) with the time of day as its fraction.
//! Where LibreOffice and Excel differ, LibreOffice's result is the target, except that
//! LibreOffice's `Err:502` ("invalid argument") is Excel's `#NUM!` or `#VALUE!`.

use super::{add, add_volatile, each_value, scalar, Outcome, Registry};
use crate::ast::{ErrorKind, Expr};
use crate::coerce::{approx_equal, to_number, to_text};
use crate::eval::{as_reference, Context, Interrupt};
use crate::scalar::{civil_from_serial, serial_from_civil};
use crate::value::Value;

pub(super) fn register(m: &mut Registry) {
    add(m, "DATE", 3, Some(3), |a, c| done(date(a, c)));
    add(m, "TIME", 3, Some(3), |a, c| done(time(a, c)));
    add_volatile(m, "TODAY", 0, Some(0), |_, c| {
        Ok(Value::Date(c.today().floor()))
    });
    add_volatile(m, "NOW", 0, Some(0), |_, c| Ok(Value::Date(c.now())));
    add(m, "YEAR", 1, Some(1), |a, c| done(civil_part(a, c, 0)));
    add(m, "MONTH", 1, Some(1), |a, c| done(civil_part(a, c, 1)));
    add(m, "DAY", 1, Some(1), |a, c| done(civil_part(a, c, 2)));
    add(m, "HOUR", 1, Some(1), |a, c| done(clock_part(a, c, 0)));
    add(m, "MINUTE", 1, Some(1), |a, c| done(clock_part(a, c, 1)));
    add(m, "SECOND", 1, Some(1), |a, c| done(clock_part(a, c, 2)));
    add(m, "WEEKDAY", 1, Some(2), |a, c| done(weekday(a, c)));
    add(m, "WEEKNUM", 1, Some(2), |a, c| done(weeknum(a, c)));
    add(m, "EDATE", 2, Some(2), |a, c| done(add_months(a, c, false)));
    add(m, "EOMONTH", 2, Some(2), |a, c| {
        done(add_months(a, c, true))
    });
    add(m, "DATEDIF", 3, Some(3), |a, c| done(datedif(a, c)));
    add(m, "DATEVALUE", 1, Some(1), |a, c| {
        done(datevalue(a, c, false))
    });
    add(m, "TIMEVALUE", 1, Some(1), |a, c| {
        done(datevalue(a, c, true))
    });
    add(m, "NETWORKDAYS", 2, Some(3), |a, c| done(networkdays(a, c)));
    add(m, "WORKDAY", 2, Some(3), |a, c| done(workday(a, c)));
    add(m, "DAYS", 2, Some(2), |a, c| done(days(a, c)));
}

/// Why a date function stopped: an error value it returns, or an interrupt to propagate.
enum Stop {
    Error(ErrorKind),
    Interrupt(Interrupt),
}

impl From<ErrorKind> for Stop {
    fn from(e: ErrorKind) -> Self {
        Stop::Error(e)
    }
}

impl From<Interrupt> for Stop {
    fn from(i: Interrupt) -> Self {
        Stop::Interrupt(i)
    }
}

type Step<T, E = Stop> = Result<T, E>;

fn done(r: Step<Value>) -> Outcome {
    match r {
        Ok(v) => Ok(v),
        Err(Stop::Error(e)) => Ok(Value::Error(e)),
        Err(Stop::Interrupt(i)) => Err(i),
    }
}

/// Argument `i` as a number; an absent or missing argument is `default`.
fn number(args: &[Expr], i: usize, ctx: &mut dyn Context, default: f64) -> Step<f64> {
    match args.get(i) {
        None | Some(Expr::Missing) => Ok(default),
        Some(a) => Ok(to_number(&scalar(a, ctx)?)?),
    }
}

/// `floor(x)`, taking a value within rounding of the next integer as that integer
/// (LibreOffice's `approxFloor`).
fn approx_floor(x: f64) -> f64 {
    let f = x.floor();
    if approx_equal(x, f + 1.0) {
        f + 1.0
    } else {
        f
    }
}

/// `trunc(x)`, taking a value within rounding of the next integer away from zero as it.
fn approx_trunc(x: f64) -> f64 {
    let t = x.trunc();
    let next = t + x.signum();
    if approx_equal(x, next) {
        next
    } else {
        t
    }
}

/// The largest serial day magnitude accepted, so years stay well inside `i32`.
const MAX_DAYS: f64 = 2_147_483_647.0;

/// Argument `i` as a serial day (its time of day dropped).
fn day(args: &[Expr], i: usize, ctx: &mut dyn Context) -> Step<i64> {
    let d = approx_floor(number(args, i, ctx, 0.0)?);
    if d.abs() > MAX_DAYS {
        return Err(ErrorKind::Num.into());
    }
    Ok(d as i64)
}

/// Argument `i` truncated to an integer within ±`limit`, else `#NUM!`.
fn integer(args: &[Expr], i: usize, ctx: &mut dyn Context, default: f64, limit: f64) -> Step<i64> {
    let n = approx_trunc(number(args, i, ctx, default)?);
    if n.abs() > limit {
        return Err(ErrorKind::Num.into());
    }
    Ok(n as i64)
}

fn serial(y: i64, m: i64, d: i64) -> i64 {
    // Callers keep years inside i32 and months in 1..=12; days may overflow the month.
    serial_from_civil(y as i32, m as u32, 1) + d - 1
}

fn civil(d: i64) -> (i64, i64, i64) {
    let (y, m, d) = civil_from_serial(d);
    (i64::from(y), i64::from(m), i64::from(d))
}

/// Month `m` (any integer, 1 = January of year `y`) as (year, month 1..=12).
fn normal_month(y: i64, m: i64) -> (i64, i64) {
    (y + (m - 1).div_euclid(12), (m - 1).rem_euclid(12) + 1)
}

fn days_in_month(y: i64, m: i64) -> i64 {
    let (ny, nm) = normal_month(y, m + 1);
    serial(ny, nm, 1) - serial(y, m, 1)
}

/// Monday 0 … Sunday 6 (serial 2, 1900-01-01, was a Monday).
fn monday_index(d: i64) -> i64 {
    (d + 5).rem_euclid(7)
}

/// `DATE(year, month, day)`: arguments truncated; months and days overflow into the next
/// year and month (`DATE(2024,14,1)` is 2025-02-01). Years 0–49 are 2000–2049 and 50–99
/// are 1950–1999 (LibreOffice's two-digit years; Excel adds 1900 to years below 1900);
/// years 100 up to the Gregorian reform (1582-10-15) are `#VALUE!` as in LibreOffice.
fn date(args: &[Expr], ctx: &mut dyn Context) -> Step<Value> {
    const LIMIT: f64 = 32_767.0;
    let y = integer(args, 0, ctx, 0.0, LIMIT)?;
    let m = integer(args, 1, ctx, 0.0, LIMIT)?;
    let d = integer(args, 2, ctx, 0.0, LIMIT)?;
    if y < 0 {
        return Err(ErrorKind::Num.into());
    }
    let full = match y {
        0..50 => y + 2000,
        50..100 => y + 1900,
        _ => y,
    };
    let (ny, nm) = normal_month(full, m);
    let first = serial(ny, nm, 1);
    if y >= 100 && first < serial(1582, 10, 15) {
        return Err(ErrorKind::Value.into());
    }
    Ok(Value::Date((first + d - 1) as f64))
}

/// `TIME(hour, minute, second)`: the time of day of that many seconds, wrapping past 24
/// hours. Arguments are not truncated (LibreOffice; Excel truncates); a negative total
/// is `#NUM!`.
fn time(args: &[Expr], ctx: &mut dyn Context) -> Step<Value> {
    let h = number(args, 0, ctx, 0.0)?;
    let m = number(args, 1, ctx, 0.0)?;
    let s = number(args, 2, ctx, 0.0)?;
    let t = (h * 3_600.0 + m * 60.0 + s) % 86_400.0;
    if t < 0.0 || !t.is_finite() {
        return Err(ErrorKind::Num.into());
    }
    Ok(Value::Date(t / 86_400.0))
}

/// `YEAR`, `MONTH`, `DAY` (`part` 0, 1, 2) of a serial.
fn civil_part(args: &[Expr], ctx: &mut dyn Context, part: usize) -> Step<Value> {
    let (y, m, d) = civil(day(args, 0, ctx)?);
    Ok(Value::Number([y, m, d][part] as f64))
}

fn round_to(x: f64, decimals: i32) -> f64 {
    let scale = 10f64.powi(decimals);
    (x * scale).round() / scale
}

/// Hours, minutes, whole seconds, and the second's fraction of a serial's time of day,
/// as LibreOffice's `tools::Time::GetClock` splits it: rounded to the precision the value
/// carries, then truncated, so 23:59:59.99 stays 23:59:59.
fn clock(x: f64) -> (u32, u32, u32, f64) {
    let t = x - approx_floor(x);
    if t <= 0.0 || t >= 1.0 || !t.is_finite() {
        return (0, 0, 0, 0.0);
    }
    let raw = t * 86_400.0;
    let decimals = if x.abs() >= 1.0 {
        (10 - x.abs().log10().ceil() as i32).clamp(2, 9)
    } else {
        9
    };
    let mut secs = round_to(raw, decimals);
    if secs >= 86_400.0 {
        secs = raw;
    }
    let h = (secs / 3_600.0) as u32;
    secs -= f64::from(h) * 3_600.0;
    let m = (secs / 60.0) as u32;
    secs -= f64::from(m) * 60.0;
    let s = secs as u32;
    (h, m, s, secs - f64::from(s))
}

/// `HOUR`, `MINUTE`, `SECOND` (`part` 0, 1, 2). Hours and minutes truncate; seconds
/// round (to 59 at most: 59.5 s is second 0, as in LibreOffice).
fn clock_part(args: &[Expr], ctx: &mut dyn Context, part: usize) -> Step<Value> {
    let (h, m, s, frac) = clock(number(args, 0, ctx, 0.0)?);
    let n = match part {
        0 => h,
        1 => m,
        _ => {
            let mut f = round_to(frac, 1);
            if f >= 1.0 {
                f = (frac * 10.0).trunc() / 10.0;
            }
            if f >= 0.5 {
                (s + 1) % 60
            } else {
                s
            }
        }
    };
    Ok(Value::Number(f64::from(n)))
}

/// `WEEKDAY(date, [type])`: type 1 (default) Sunday = 1 … Saturday = 7; 2 Monday = 1 …
/// Sunday = 7; 3 Monday = 0 … Sunday = 6; 11–17 Monday … Sunday = 1. Others are `#NUM!`.
fn weekday(args: &[Expr], ctx: &mut dyn Context) -> Step<Value> {
    let d = day(args, 0, ctx)?;
    let kind = integer(args, 1, ctx, 1.0, MAX_DAYS)?;
    let mon0 = monday_index(d);
    let n = match kind {
        1 => (mon0 + 1) % 7 + 1,
        2 => mon0 + 1,
        3 => mon0,
        11..=17 => (mon0 - (kind - 11)).rem_euclid(7) + 1,
        _ => return Err(ErrorKind::Num.into()),
    };
    Ok(Value::Number(n as f64))
}

/// `WEEKNUM(date, [type])`: week 1 holds January 1 and weeks start on Sunday (type 1,
/// default), Monday (2, 11), Tuesday (12) … Sunday (17); type 21 is the ISO week.
fn weeknum(args: &[Expr], ctx: &mut dyn Context) -> Step<Value> {
    let d = day(args, 0, ctx)?;
    let kind = integer(args, 1, ctx, 1.0, MAX_DAYS)?;
    let start = match kind {
        1 => 6,
        2 => 0,
        11..=17 => kind - 11,
        21 => {
            let thursday = d - monday_index(d) + 3;
            let (y, _, _) = civil(thursday);
            return Ok(Value::Number(((thursday - serial(y, 1, 1)) / 7 + 1) as f64));
        }
        _ => return Err(ErrorKind::Num.into()),
    };
    let (y, _, _) = civil(d);
    let jan1 = serial(y, 1, 1);
    let offset = (monday_index(jan1) - start).rem_euclid(7);
    Ok(Value::Number(((d - jan1 + offset) / 7 + 1) as f64))
}

/// `EDATE(start, months)` (the same day that many months on, clamped to the month's end)
/// and `EOMONTH(start, months)` (the last day of that month). Both truncate their
/// arguments, as LibreOffice does.
fn add_months(args: &[Expr], ctx: &mut dyn Context, end_of_month: bool) -> Step<Value> {
    let start = integer(args, 0, ctx, 0.0, MAX_DAYS)?;
    let months = integer(args, 1, ctx, 0.0, MAX_DAYS)?;
    let (y, m, d) = civil(start);
    let (ny, nm) = normal_month(y, m + months);
    let last = days_in_month(ny, nm);
    let nd = if end_of_month { last } else { d.min(last) };
    Ok(Value::Date(serial(ny, nm, nd) as f64))
}

/// `DATEDIF(start, end, unit)` with LibreOffice's algorithm (Excel's, including its
/// `"MD"` roll-over: a day that doesn't exist in the month before `end` rolls into the
/// next month, which can give a negative count). `start > end` and unknown units are
/// `#NUM!`.
fn datedif(args: &[Expr], ctx: &mut dyn Context) -> Step<Value> {
    let start = day(args, 0, ctx)?;
    let end = day(args, 1, ctx)?;
    let unit = to_text(&scalar(&args[2], ctx)?)?.to_ascii_lowercase();
    if start > end {
        return Err(ErrorKind::Num.into());
    }
    let dd = end - start;
    if dd == 0 || unit == "d" {
        return Ok(Value::Number(dd as f64));
    }
    let (y1, m1, d1) = civil(start);
    let (y2, m2, d2) = civil(end);
    let months = || m2 - m1 + 12 * (y2 - y1) - i64::from(d1 > d2);
    let anniversary_passed = m2 > m1 || (m2 == m1 && d2 >= d1);
    let n = match unit.as_str() {
        "m" => months(),
        "ym" => months() % 12,
        "y" => y2 - y1 - i64::from(y2 > y1 && !anniversary_passed),
        "md" if d1 <= d2 => d2 - d1,
        "md" => {
            let (y, m) = normal_month(y2, m2 - 1);
            end - serial(y, m, d1)
        }
        "yd" => end - serial(y2 - i64::from(!anniversary_passed), m1, d1),
        _ => return Err(ErrorKind::Num.into()),
    };
    Ok(Value::Number(n as f64))
}

/// `DATEVALUE(text)` (the date, `time == false`) and `TIMEVALUE(text)` (the time of day)
/// of a date or time written as text; see [`date_time`] for the forms read. Anything
/// that isn't such text is `#VALUE!`.
fn datevalue(args: &[Expr], ctx: &mut dyn Context, time: bool) -> Step<Value> {
    let v = scalar(&args[0], ctx)?;
    let text = match &v {
        Value::Text(t) => t,
        Value::Error(e) => return Err((*e).into()),
        _ => return Err(ErrorKind::Value.into()),
    };
    let (d, t) = date_time(text).ok_or(ErrorKind::Value)?;
    match (time, d, t) {
        (false, Some(d), _) => Ok(Value::Date(d as f64)),
        (true, _, Some(t)) => Ok(Value::Date(t)),
        _ => Err(ErrorKind::Value.into()),
    }
}

/// A cursor over date or time text.
struct Cursor<'a> {
    b: &'a [u8],
    i: usize,
}

impl Cursor<'_> {
    fn eat(&mut self, c: u8) -> bool {
        let hit = self.b.get(self.i) == Some(&c);
        self.i += usize::from(hit);
        hit
    }

    fn spaces(&mut self) -> usize {
        let n = self.b[self.i..].iter().take_while(|c| **c == b' ').count();
        self.i += n;
        n
    }

    /// `min` to `max` digits.
    fn digits(&mut self, min: usize, max: usize) -> Option<(i64, usize)> {
        let n = self.b[self.i..]
            .iter()
            .take_while(|c| c.is_ascii_digit())
            .count();
        if !(min..=max).contains(&n) {
            return None;
        }
        let v = self.b[self.i..self.i + n]
            .iter()
            .fold(0, |v, c| v * 10 + i64::from(c - b'0'));
        self.i += n;
        Some((v, n))
    }

    /// An English month name: in full, its first three letters, or `Sept`.
    fn month(&mut self) -> Option<i64> {
        const NAMES: [&str; 12] = [
            "january",
            "february",
            "march",
            "april",
            "may",
            "june",
            "july",
            "august",
            "september",
            "october",
            "november",
            "december",
        ];
        let n = self.b[self.i..]
            .iter()
            .take_while(|c| c.is_ascii_alphabetic())
            .count();
        let word = std::str::from_utf8(&self.b[self.i..self.i + n])
            .ok()?
            .to_ascii_lowercase();
        let m = NAMES.iter().position(|name| {
            word.len() >= 3 && name.starts_with(&word) && (word.len() == 3 || word == *name)
        });
        let m = m.or_else(|| (word == "sept").then_some(8))?;
        self.i += n;
        Some(m as i64 + 1)
    }

    /// A year of 4 digits, or 2 (0–49 → 2000s, 50–99 → 1900s, as LibreOffice reads them).
    fn year(&mut self) -> Option<i64> {
        let (y, n) = self.digits(2, 4)?;
        match n {
            2 if y < 50 => Some(y + 2000),
            2 => Some(y + 1900),
            4 => Some(y),
            _ => None,
        }
    }

    fn at_end(&self) -> bool {
        self.i == self.b.len()
    }
}

fn checked_date(y: i64, m: i64, d: i64) -> Option<i64> {
    ((1..=12).contains(&m) && (1..=days_in_month(y, m)).contains(&d)).then(|| serial(y, m, d))
}

/// A date at the cursor: `YYYY-M-D`, `M/D/YY[YY]`, `D-Mon-YY[YY]`, or `Mon D[,] YYYY`
/// (English month names, US order, as LibreOffice reads them in its default locale).
/// The second value tells whether a `T` may separate a time (ISO only).
fn date_part(c: &mut Cursor) -> Option<(i64, bool)> {
    let start = c.i;
    if let Some((y, 4)) = c.digits(4, 4) {
        if c.eat(b'-') {
            let (m, _) = c.digits(1, 2)?;
            if c.eat(b'-') {
                let (d, _) = c.digits(1, 2)?;
                return Some((checked_date(y, m, d)?, true));
            }
        }
        return None;
    }
    c.i = start;
    if let Some((first, _)) = c.digits(1, 2) {
        if c.eat(b'/') {
            let (d, _) = c.digits(1, 2)?;
            if !c.eat(b'/') {
                return None;
            }
            return Some((checked_date(c.year()?, first, d)?, false));
        }
        if c.eat(b'-') {
            let m = c.month()?;
            if !c.eat(b'-') {
                return None;
            }
            return Some((checked_date(c.year()?, m, first)?, false));
        }
        return None;
    }
    let m = c.month()?;
    if c.spaces() == 0 {
        return None;
    }
    let (d, _) = c.digits(1, 2)?;
    c.eat(b',');
    if c.spaces() == 0 {
        return None;
    }
    let (y, _) = c.digits(4, 4)?;
    Some((checked_date(y, m, d)?, false))
}

/// A time at the cursor as a fraction of a day: `H[:MM[:SS[.f]]]` with an optional
/// `AM`/`PM` (required without minutes). Minutes and seconds may exceed 59 and hours 23
/// (they carry over; a time past midnight wraps), but with `AM`/`PM` hours are 0–12.
fn time_part(c: &mut Cursor) -> Option<f64> {
    let (h, _) = c.digits(1, 2)?;
    let mut secs = 0.0;
    let mut minutes = false;
    if c.eat(b':') {
        minutes = true;
        let (m, _) = c.digits(1, 2)?;
        secs += (m * 60) as f64;
        if c.eat(b':') {
            let (s, _) = c.digits(1, 2)?;
            secs += s as f64;
            if c.eat(b'.') {
                let from = c.i - 1;
                let n = c.b[c.i..].iter().take_while(|b| b.is_ascii_digit()).count();
                if n == 0 {
                    return None;
                }
                c.i += n;
                let frac: f64 = std::str::from_utf8(&c.b[from..c.i]).ok()?.parse().ok()?;
                secs += frac;
            }
        }
    }
    c.spaces();
    let rest = &c.b[c.i..];
    let pm = if rest.eq_ignore_ascii_case(b"am") {
        Some(false)
    } else if rest.eq_ignore_ascii_case(b"pm") {
        Some(true)
    } else {
        None
    };
    let h = match pm {
        Some(pm) => {
            c.i = c.b.len();
            if h > 12 {
                return None;
            }
            h % 12 + if pm { 12 } else { 0 }
        }
        None if minutes => h,
        None => return None,
    };
    Some(((h * 3_600) as f64 + secs) % 86_400.0 / 86_400.0)
}

/// Text holding a date, a time, or both (the date first, then spaces or ISO's `T`), with
/// spaces around it. Returns the serial day and the time of day.
fn date_time(text: &str) -> Option<(Option<i64>, Option<f64>)> {
    let mut c = Cursor {
        b: text.trim_matches(' ').as_bytes(),
        i: 0,
    };
    if let Some((d, iso)) = date_part(&mut c) {
        if c.at_end() {
            return Some((Some(d), None));
        }
        if !(c.spaces() > 0 || (iso && c.eat(b'T'))) {
            return None;
        }
        let t = time_part(&mut c)?;
        return c.at_end().then_some((Some(d), Some(t)));
    }
    c.i = 0;
    let t = time_part(&mut c)?;
    c.at_end().then_some((None, Some(t)))
}

/// The serial days `arg` lists as holidays, sorted and without repeats. Numbers and
/// dates count, booleans and empty cells are skipped, errors propagate. Text in a
/// reference is skipped unless `strict`; text anywhere else is `#VALUE!`.
fn holidays(arg: Option<&Expr>, ctx: &mut dyn Context, strict: bool) -> Step<Vec<i64>> {
    let Some(arg) = arg else {
        return Ok(Vec::new());
    };
    let in_reference = as_reference(arg, ctx).is_some();
    let mut out = Vec::new();
    let mut err = None;
    each_value(arg, ctx, &mut |v, _| {
        match v {
            Value::Number(n) | Value::Date(n) => {
                let d = approx_floor(*n);
                if d.abs() > MAX_DAYS {
                    err = Some(ErrorKind::Num);
                } else {
                    out.push(d as i64);
                }
            }
            Value::Text(_) if in_reference && !strict => {}
            Value::Text(_) => err = Some(ErrorKind::Value),
            Value::Error(e) => err = Some(*e),
            Value::Empty | Value::Bool(_) | Value::Array(_) => {}
        }
        err.is_none()
    })?;
    if let Some(e) = err {
        return Err(e.into());
    }
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

fn is_workday(d: i64) -> bool {
    monday_index(d) < 5
}

/// Weekdays (Monday to Friday) among the serial days before `d`, counted from serial 2.
fn weekdays_before(d: i64) -> i64 {
    let n = d - 2;
    n.div_euclid(7) * 5 + n.rem_euclid(7).min(5)
}

/// `NETWORKDAYS(start, end, [holidays])`: the weekdays from `start` to `end` inclusive
/// that aren't holidays; negative when `end` is before `start`. Text in a holidays range
/// is skipped (LibreOffice).
fn networkdays(args: &[Expr], ctx: &mut dyn Context) -> Step<Value> {
    let start = day(args, 0, ctx)?;
    let end = day(args, 1, ctx)?;
    let holidays = holidays(args.get(2), ctx, false)?;
    let (lo, hi) = (start.min(end), start.max(end));
    let off = holidays
        .iter()
        .filter(|h| (lo..=hi).contains(h) && is_workday(**h))
        .count() as i64;
    let n = weekdays_before(hi + 1) - weekdays_before(lo) - off;
    Ok(Value::Number(if start > end { -n } else { n } as f64))
}

/// `WORKDAY(start, days, [holidays])`: the weekday that many working days after (or,
/// negative, before) `start`; `start` itself when `days` is 0. Text among the holidays is
/// `#VALUE!` (LibreOffice gives Err:502).
fn workday(args: &[Expr], ctx: &mut dyn Context) -> Step<Value> {
    let start = day(args, 0, ctx)?;
    let days = integer(args, 1, ctx, 0.0, MAX_DAYS)?;
    let holidays = holidays(args.get(2), ctx, true)?;
    let step = days.signum();
    let (mut d, mut left) = (start, days.abs());
    // Whole weeks first: each holds 5 weekdays less the weekday holidays in it.
    while left > 5 {
        let ahead = if step > 0 {
            holidays.len() - holidays.partition_point(|h| *h <= d)
        } else {
            holidays.partition_point(|h| *h < d)
        };
        if ahead == 0 {
            let weeks = (left - 1) / 5;
            d += 7 * weeks * step;
            left -= 5 * weeks;
            break;
        }
        let next = d + 7 * step;
        let (lo, hi) = if step > 0 {
            (d + 1, next)
        } else {
            (next, d - 1)
        };
        let from = holidays.partition_point(|h| *h < lo);
        let to = holidays.partition_point(|h| *h <= hi);
        let worked = 5 - holidays[from..to]
            .iter()
            .filter(|h| is_workday(**h))
            .count() as i64;
        if worked >= left {
            break;
        }
        d = next;
        left -= worked;
    }
    while left > 0 {
        d += step;
        if is_workday(d) && holidays.binary_search(&d).is_err() {
            left -= 1;
        }
    }
    if (d as f64).abs() > MAX_DAYS {
        return Err(ErrorKind::Num.into());
    }
    Ok(Value::Date(d as f64))
}

/// `DAYS(end, start)`: `end − start`, fractions kept (LibreOffice; Excel truncates both
/// dates first).
fn days(args: &[Expr], ctx: &mut dyn Context) -> Step<Value> {
    let end = number(args, 0, ctx, 0.0)?;
    let start = number(args, 1, ctx, 0.0)?;
    Ok(Value::Number(end - start))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_text_forms() {
        let d = |y, m, d| Some(serial(y, m, d));
        let cases: &[(&str, Option<i64>, Option<f64>)] = &[
            ("2024-01-05", d(2024, 1, 5), None),
            (" 2024-1-5 ", d(2024, 1, 5), None),
            ("2024-01-01T06:00", d(2024, 1, 1), Some(0.25)),
            ("1/2/2024", d(2024, 1, 2), None),
            ("1/2/24", d(2024, 1, 2), None),
            ("1/2/70", d(1970, 1, 2), None),
            ("2-Jan-2024", d(2024, 1, 2), None),
            ("5-sep-24", d(2024, 9, 5), None),
            ("Sept 5, 2024", d(2024, 9, 5), None),
            ("January 2 2024", d(2024, 1, 2), None),
            ("02/03/2024 10:00 PM", d(2024, 2, 3), Some(22.0 / 24.0)),
            ("6:30 pm", None, Some(18.5 / 24.0)),
            ("12 AM", None, Some(0.0)),
            ("25:00", None, Some(1.0 / 24.0)),
        ];
        for (text, day, time) in cases {
            assert_eq!(date_time(text), Some((*day, *time)), "{text}");
        }
        for bad in [
            "2024/01/05",
            "13/1/2024",
            "Feb 29, 2023",
            "2 Jan 2024",
            "Jan-2-2024",
            "13:00 PM",
            "12",
            "abc",
            "",
            "2024-01-01 x",
        ] {
            assert_eq!(date_time(bad), None, "{bad}");
        }
    }

    #[test]
    fn weekdays_count_from_any_start() {
        for a in -20..20 {
            for b in a..a + 30 {
                let slow = (a..=b).filter(|d| is_workday(*d)).count() as i64;
                assert_eq!(
                    weekdays_before(b + 1) - weekdays_before(a),
                    slow,
                    "{a}..={b}"
                );
            }
        }
    }
}
