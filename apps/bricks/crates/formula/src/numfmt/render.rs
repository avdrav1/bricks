//! Showing a value through a compiled section. Nothing here allocates except `out`.

use super::{AmPm, DateTime, Fraction, Item, Number, OutOfRange, Ph, Scientific, Unit};
use crate::scalar::{civil_from_serial, serial_from_civil};
use crate::value::{push_number_text, round15, StackText};
use std::fmt::Write;

type Fill = Option<(usize, char)>;

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

const DAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// The first serial past Excel's last date, 9999-12-31.
const END_SERIAL: f64 = 2_958_466.0;

/// A non-negative number's decimal digits, rounded to 15 significant digits:
/// the value is `0.d[0]d[1]…d[n-1] × 10^point`, with `d[0] != 0` unless it is zero (`n == 0`).
#[derive(Clone, Copy)]
struct Digits {
    d: [u8; 16],
    n: usize,
    point: i32,
}

impl Digits {
    const ZERO: Digits = Digits {
        d: [0; 16],
        n: 0,
        point: 0,
    };

    fn of(y: f64) -> Digits {
        if y == 0.0 {
            return Digits::ZERO;
        }
        let mut buf = StackText::default();
        let _ = write!(buf, "{y:.14e}");
        let (mantissa, exp) = buf.as_str().split_once('e').expect("{:e} has an exponent");
        let exp: i32 = exp.parse().expect("an integer exponent");
        let mut r = Digits {
            point: exp + 1,
            ..Digits::ZERO
        };
        for b in mantissa.bytes().filter(u8::is_ascii_digit) {
            r.d[r.n] = b - b'0';
            r.n += 1;
        }
        r.trim();
        r
    }

    fn trim(&mut self) {
        while self.n > 0 && self.d[self.n - 1] == 0 {
            self.n -= 1;
        }
        if self.n == 0 {
            self.point = 0;
        }
    }

    fn is_zero(&self) -> bool {
        self.n == 0
    }

    /// Rounded half away from zero to `places` decimals.
    fn round(mut self, places: i32) -> Digits {
        let keep = self.point + places;
        if keep >= self.n as i32 {
            return self;
        }
        if keep < 0 {
            return Digits::ZERO;
        }
        let keep = keep as usize;
        let up = self.d[keep] >= 5;
        self.n = keep;
        if up {
            let mut i = keep;
            loop {
                if i == 0 {
                    // All nines carried over: 10^point.
                    self.d[0] = 1;
                    self.n = 1;
                    self.point += 1;
                    break;
                }
                i -= 1;
                if self.d[i] == 9 {
                    self.d[i] = 0;
                } else {
                    self.d[i] += 1;
                    break;
                }
            }
        }
        self.trim();
        self
    }

    /// The digit at `pos` from the left (`0` = `d[0]`); zero outside the digits.
    fn digit(&self, pos: i32) -> u8 {
        usize::try_from(pos)
            .ok()
            .filter(|&p| p < self.n)
            .map_or(0, |p| self.d[p])
    }

    /// How many digits the integer part has (none for a value below 1).
    fn int_len(&self) -> usize {
        if self.n == 0 {
            0
        } else {
            self.point.max(0) as usize
        }
    }
}

fn literal(item: &Item, out: &mut String, fill: &mut Fill) {
    match item {
        Item::Lit(s) => out.push_str(s),
        // Only the first fill counts.
        Item::Fill(c) if fill.is_none() => *fill = Some((out.len(), *c)),
        _ => {}
    }
}

/// Integer placeholder `i` (from the left, of `count`) for an integer of `len` digits
/// (`digit(k)` from the left): the first placeholder also takes any digits beyond `count`.
fn int_place(
    out: &mut String,
    (i, count): (usize, usize),
    len: usize,
    ph: Ph,
    group: bool,
    digit: &dyn Fn(usize) -> u8,
) {
    let separator = |out: &mut String, pos: usize, c: char| {
        if group && pos > 0 && pos.is_multiple_of(3) {
            out.push(c);
        }
    };
    let put = |out: &mut String, k: usize| {
        out.push(char::from(b'0' + digit(k)));
        separator(out, len - 1 - k, ',');
    };
    let pos = count - 1 - i;
    if i == 0 && len > count {
        for k in 0..len - count {
            put(out, k);
        }
    }
    if pos < len {
        put(out, len - 1 - pos);
    } else {
        match ph {
            Ph::Zero => {
                out.push('0');
                separator(out, pos, ',');
            }
            Ph::Space => {
                out.push(' ');
                separator(out, pos, ' ');
            }
            Ph::Hash => {}
        }
    }
}

/// Fraction placeholders: the last one that shows a digit (a `0`, or a nonzero digit).
fn last_shown(fracs: &[Ph], r: &Digits) -> Option<usize> {
    (0..fracs.len())
        .rev()
        .find(|&j| fracs[j] == Ph::Zero || r.digit(r.point + j as i32) != 0)
}

pub(super) fn general(items: &[Item], y: f64, neg: bool, out: &mut String, fill: &mut Fill) {
    if neg && round15(y) != 0.0 {
        out.push('-');
    }
    for item in items {
        match item {
            Item::General => push_number_text(y, out),
            other => literal(other, out, fill),
        }
    }
}

pub(super) fn number(n: &Number, y: f64, neg: bool, out: &mut String, fill: &mut Fill) -> Shown {
    let mut y = y;
    for _ in 0..n.thousands {
        y /= 1_000.0;
    }
    for _ in 0..n.percents {
        y *= 100.0;
    }
    finite(y)?;
    let r = Digits::of(y).round(n.fracs.len() as i32);
    let shows_digits = n.ints + n.fracs.len() > 0;
    if neg && (if shows_digits { !r.is_zero() } else { y != 0.0 }) {
        out.push('-');
    }
    let len = r.int_len();
    let last = last_shown(&n.fracs, &r);
    let digit = |k: usize| r.digit(k as i32);
    let (mut i, mut j) = (0, 0);
    for item in &n.items {
        match item {
            Item::Int(ph) => {
                int_place(out, (i, n.ints), len, *ph, n.group, &digit);
                i += 1;
            }
            Item::Point => {
                if n.ints == 0 {
                    (0..len).for_each(|k| out.push(char::from(b'0' + digit(k))));
                }
                if last.is_some() || n.fracs.contains(&Ph::Space) {
                    out.push('.');
                }
            }
            Item::Frac(ph) => {
                if last.is_some_and(|l| j <= l) {
                    out.push(char::from(b'0' + r.digit(r.point + j as i32)));
                } else if *ph == Ph::Space {
                    out.push(' ');
                }
                j += 1;
            }
            other => literal(other, out, fill),
        }
    }
    Ok(())
}

pub(super) fn scientific(
    s: &Scientific,
    y: f64,
    neg: bool,
    out: &mut String,
    fill: &mut Fill,
) -> Shown {
    let mut y = y;
    for _ in 0..s.percents {
        y *= 100.0;
    }
    finite(y)?;
    let places = s.fracs.len() as i32;
    let d = Digits::of(y);
    let (m, exp) = if d.is_zero() {
        (d, 0)
    } else {
        // The exponent of the value rounded to the digits shown; with several integer
        // placeholders, a multiple of their count (engineering notation for `##0.0E+0`).
        let ints = s.ints as i32;
        let sig = if ints == 0 {
            places.max(1)
        } else {
            ints + places
        };
        let e0 = d.round(sig - d.point).point - 1;
        let exp = match ints {
            0 => e0 + 1,
            1 => e0,
            _ => e0 - e0.rem_euclid(ints),
        };
        let shifted = Digits {
            point: d.point - exp,
            ..d
        };
        (shifted.round(places), exp)
    };
    if neg && !m.is_zero() {
        out.push('-');
    }
    // A zero mantissa still shows its one digit.
    let len = if m.is_zero() { 1 } else { m.int_len() };
    let last = last_shown(&s.fracs, &m);
    let digit = |k: usize| m.digit(k as i32);
    let e = exp.unsigned_abs();
    let elen = e.checked_ilog10().map_or(1, |l| l as usize + 1);
    let edigit = |k: usize| (e / 10u32.pow((elen - 1 - k) as u32) % 10) as u8;
    let (mut i, mut j, mut q) = (0, 0, 0);
    for item in &s.items {
        match item {
            Item::Int(ph) => {
                int_place(out, (i, s.ints), len, *ph, false, &digit);
                i += 1;
            }
            Item::Point => {
                if s.ints == 0 {
                    (0..m.int_len()).for_each(|k| out.push(char::from(b'0' + digit(k))));
                }
                if last.is_some() || s.fracs.contains(&Ph::Space) {
                    out.push('.');
                }
            }
            Item::Frac(ph) => {
                if last.is_some_and(|l| j <= l) {
                    out.push(char::from(b'0' + m.digit(m.point + j as i32)));
                } else if *ph == Ph::Space {
                    out.push(' ');
                }
                j += 1;
            }
            Item::Exp { letter, plus } => {
                out.push(*letter);
                if exp < 0 {
                    out.push('-');
                } else if *plus {
                    out.push('+');
                }
            }
            Item::ExpDigit(ph) => {
                int_place(out, (q, s.exp_digits.len()), elen, *ph, false, &edigit);
                q += 1;
            }
            other => literal(other, out, fill),
        }
    }
    Ok(())
}

/// The closest `p/q` to `x` with `q <= max` (the smallest `q` on ties).
fn closest(x: f64, max: f64) -> (f64, f64) {
    let mut best = (x.round(), 1.0);
    let mut err = (x - best.0).abs();
    let mut q = 2.0;
    while q <= max && err > 0.0 {
        let p = (x * q).round();
        let e = (x - p / q).abs();
        if e < err {
            best = (p, q);
            err = e;
        }
        q += 1.0;
    }
    best
}

pub(super) fn fraction(
    f: &Fraction,
    y: f64,
    neg: bool,
    out: &mut String,
    fill: &mut Fill,
) -> Shown {
    let mut y = y;
    for _ in 0..f.percents {
        y *= 100.0;
    }
    let whole = f.ints > 0;
    let (mut int, part) = if whole {
        (y.trunc(), y - y.trunc())
    } else {
        (0.0, y)
    };
    let (mut num, den) = match f.fixed {
        Some(d) => ((part * f64::from(d)).round(), f64::from(d)),
        None => closest(part, 10f64.powi(f.dens.len() as i32) - 1.0),
    };
    finite(int)?;
    finite(num)?;
    if whole && num != 0.0 && num == den {
        int += 1.0;
        num = 0.0;
    }
    if neg && (int != 0.0 || num != 0.0) {
        out.push('-');
    }
    let zero = num == 0.0;
    let int_digits = Digits::of(int);
    // A zero whole number shows its 0 when the fraction is zero too.
    let int_len = match int_digits.int_len() {
        0 if zero => 1,
        n => n,
    };
    let int_digit = |k: usize| int_digits.digit(k as i32);
    let num_digits = Digits::of(num);
    let num_len = num_digits.int_len().max(1);
    let num_digit = |k: usize| num_digits.digit(k as i32);
    let (mut i, mut k) = (0, 0);
    while k < f.items.len() {
        if whole && zero && k == f.blank_from {
            (0..f.blank).for_each(|_| out.push(' '));
            k = f.blank_to;
            continue;
        }
        match &f.items[k] {
            Item::Int(ph) => {
                int_place(out, (i, f.ints), int_len, *ph, false, &int_digit);
                i += 1;
            }
            Item::Num => {
                for (q, ph) in f.nums.iter().enumerate() {
                    int_place(out, (q, f.nums.len()), num_len, *ph, false, &num_digit);
                }
            }
            Item::Bar => out.push('/'),
            Item::Den => {
                let _ = write!(out, "{den}");
                let len = Digits::of(den).int_len();
                for ph in f.dens.iter().skip(len) {
                    if *ph != Ph::Hash {
                        out.push(' ');
                    }
                }
            }
            other => literal(other, out, fill),
        }
        k += 1;
    }
    Ok(())
}

type Shown = Result<(), OutOfRange>;

/// A number scaled past what a double holds can't be shown.
fn finite(y: f64) -> Shown {
    if y.is_finite() {
        Ok(())
    } else {
        Err(OutOfRange)
    }
}

pub(super) fn date_time(
    dt: &DateTime,
    x: f64,
    out: &mut String,
    fill: &mut Fill,
) -> Result<(), OutOfRange> {
    if !(serial_from_civil(1, 1, 1) as f64..END_SERIAL).contains(&x) {
        return Err(OutOfRange);
    }
    // A time rounds to the smallest unit shown (a second, or its fraction), carrying into
    // minutes, hours, and days, as Excel does; a date alone is the serial's day.
    let per_second = 10i64.pow(u32::from(dt.sub));
    let per_day = 86_400 * per_second;
    let mut ticks = if dt.timed {
        round15(x * 86_400.0 * per_second as f64).round() as i64
    } else {
        round15(x).floor() as i64 * per_day
    };
    if ticks < 0 && dt.elapsed {
        out.push('-');
        ticks = -ticks;
    }
    let days = ticks.div_euclid(per_day);
    let rem = ticks.rem_euclid(per_day);
    let (year, month, day) = civil_from_serial(days);
    let weekday = (days + 6).rem_euclid(7) as usize;
    let secs = rem / per_second;
    let (hour, minute, second) = (secs / 3_600, secs / 60 % 60, secs % 60);
    let num = |out: &mut String, n: i64, width: usize| {
        let _ = write!(out, "{n:0width$}");
    };
    let width = |two: bool| if two { 2 } else { 1 };
    for item in &dt.items {
        match item {
            Item::Year(true) => num(out, i64::from(year), 4),
            Item::Year(false) => num(out, i64::from(year).rem_euclid(100), 2),
            Item::Month(3) => out.push_str(&MONTHS[month as usize - 1][..3]),
            Item::Month(4) => out.push_str(MONTHS[month as usize - 1]),
            Item::Month(5) => out.push_str(&MONTHS[month as usize - 1][..1]),
            Item::Month(n) => num(out, i64::from(month), usize::from(*n)),
            Item::Day(3) => out.push_str(&DAYS[weekday][..3]),
            Item::Day(4) => out.push_str(DAYS[weekday]),
            Item::Day(n) => num(out, i64::from(day), usize::from(*n)),
            Item::Hour(two) => {
                let h = if dt.twelve {
                    (hour + 11) % 12 + 1
                } else {
                    hour
                };
                num(out, h, width(*two));
            }
            Item::Minute(two) => num(out, minute, width(*two)),
            Item::Second(two) => num(out, second, width(*two)),
            Item::SubSecond(n) => {
                let sub = rem % per_second / 10i64.pow(u32::from(dt.sub - n));
                out.push('.');
                num(out, sub, usize::from(*n));
            }
            Item::Elapsed(unit, w) => {
                let per = match unit {
                    Unit::Hour => 3_600,
                    Unit::Minute => 60,
                    Unit::Second => 1,
                } * per_second;
                num(out, ticks / per, usize::from(*w));
            }
            Item::AmPm(AmPm::Full) => out.push_str(if hour < 12 { "AM" } else { "PM" }),
            Item::AmPm(AmPm::Short { a, p }) => out.push(if hour < 12 { *a } else { *p }),
            other => literal(other, out, fill),
        }
    }
    Ok(())
}

pub(super) fn text(items: &[Item], t: &str, out: &mut String, fill: &mut Fill) {
    for item in items {
        match item {
            Item::At => out.push_str(t),
            other => literal(other, out, fill),
        }
    }
}
