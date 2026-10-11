//! Plain scalar grammars shared by type inference (TYPE-1) and evaluation: numbers without
//! thousands separators, ISO dates and date-times, booleans, null markers, and the serial
//! day numbers spreadsheets use for dates.

/// A null marker: `NA`, `N/A`, `null` (any case) or `-`.
pub fn is_null(raw: &str) -> bool {
    raw == "-"
        || ["na", "n/a", "null"]
            .iter()
            .any(|n| raw.eq_ignore_ascii_case(n))
}

/// `true`, `false`, `yes`, or `no` in any case.
pub fn is_boolean(raw: &str) -> bool {
    ["true", "false", "yes", "no"]
        .iter()
        .any(|b| raw.eq_ignore_ascii_case(b))
}

/// `Some(has fraction or exponent)` if `raw` is a plain number: an optional sign, `0` or
/// digits not starting with `0`, then optionally `.digits` and `e[+-]digits`.
pub fn number(raw: &str) -> Option<bool> {
    let b = raw.strip_prefix(['+', '-']).unwrap_or(raw).as_bytes();
    let int = b.iter().take_while(|c| c.is_ascii_digit()).count();
    if int == 0 || (int > 1 && b[0] == b'0') {
        return None;
    }
    let mut i = int;
    let mut fraction = false;
    if b.get(i) == Some(&b'.') {
        let digits = b[i + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 {
            return None;
        }
        i += 1 + digits;
        fraction = true;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let digits = b[i..].iter().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 {
            return None;
        }
        i += digits;
        fraction = true;
    }
    (i == b.len()).then_some(fraction)
}

/// `n` ASCII digits at the start of `b`, as a number.
fn digits(b: &[u8], n: usize) -> Option<u32> {
    let d = b.get(..n)?;
    d.iter()
        .all(u8::is_ascii_digit)
        .then(|| d.iter().fold(0, |v, c| v * 10 + u32::from(c - b'0')))
}

/// A valid ISO date `YYYY-MM-DD` at the start of `raw`; returns what follows it.
pub fn date(raw: &str) -> Option<&str> {
    let b = raw.as_bytes();
    let (year, month, day) = (
        digits(b, 4)?,
        digits(b.get(5..)?, 2)?,
        digits(b.get(8..)?, 2)?,
    );
    if b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    (1..=days).contains(&day).then(|| &raw[10..])
}

/// After a date: `T` or a space, `HH:MM`, optional `:SS` and `.fraction`, optional `Z` or
/// `+HH:MM`/`-HH:MM`.
pub fn time(rest: &str) -> bool {
    let b = rest.as_bytes();
    if !matches!(b.first(), Some(b'T' | b' ')) {
        return false;
    }
    let b = &b[1..];
    let hm = |b: &[u8]| -> bool {
        matches!((digits(b, 2), b.get(2), digits(b.get(3..).unwrap_or(&[]), 2)),
            (Some(h), Some(b':'), Some(m)) if h < 24 && m < 60)
    };
    if !hm(b) {
        return false;
    }
    let mut i = 5;
    if b.get(i) == Some(&b':') {
        match digits(&b[i + 1..], 2) {
            Some(s) if s < 60 => i += 3,
            _ => return false,
        }
        if b.get(i) == Some(&b'.') {
            let n = b[i + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
            if n == 0 {
                return false;
            }
            i += 1 + n;
        }
    }
    match &b[i..] {
        [] | [b'Z'] => true,
        [b'+' | b'-', zone @ ..] => zone.len() == 5 && hm(zone),
        _ => false,
    }
}

/// Days since 0000-03-01 (H. Hinnant's days_from_civil, years starting in March).
fn days_from_march_0(y: i64, m: i64, d: i64) -> i64 {
    let (y, m) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    y * 365 + y.div_euclid(4) - y.div_euclid(100) + y.div_euclid(400) + (153 * m + 2) / 5 + d - 1
}

/// [`days_from_march_0`] of 1899-12-30, the day before serial 1.
const SERIAL_EPOCH: i64 = 693_899;

/// Microseconds since 0000-03-01T00:00Z of an ISO date or date-time, for sorting (SORT-1):
/// a date is its midnight, a time without a zone is taken as UTC, and fractions finer than
/// a microsecond are dropped.
pub fn instant(raw: &str) -> Option<i64> {
    let rest = date(raw)?;
    let b = raw.as_bytes();
    let (y, m, d) = (digits(b, 4)?, digits(&b[5..], 2)?, digits(&b[8..], 2)?);
    let days = days_from_march_0(i64::from(y), i64::from(m), i64::from(d));
    let mut micros = days * 86_400_000_000;
    if rest.is_empty() {
        return Some(micros);
    }
    if !time(rest) {
        return None;
    }
    let t = &rest.as_bytes()[1..];
    let minutes = |b: &[u8]| Some(i64::from(digits(b, 2)? * 60 + digits(&b[3..], 2)?));
    micros += minutes(t)? * 60_000_000;
    let mut i = 5;
    if t.get(i) == Some(&b':') {
        micros += i64::from(digits(&t[i + 1..], 2)?) * 1_000_000;
        i += 3;
        if t.get(i) == Some(&b'.') {
            let n = t[i + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
            let mut frac = 0;
            for k in 0..6 {
                frac = frac * 10
                    + t.get(i + 1 + k)
                        .filter(|_| k < n)
                        .map_or(0, |c| i64::from(c - b'0'));
            }
            micros += frac;
            i += 1 + n;
        }
    }
    match t.get(i) {
        Some(b'+') => micros -= minutes(&t[i + 1..])? * 60_000_000,
        Some(b'-') => micros += minutes(&t[i + 1..])? * 60_000_000,
        _ => {}
    }
    Some(micros)
}

/// The serial day number of a date: days since 1899-12-30, LibreOffice's epoch. It equals
/// Excel's serial for dates from 1900-03-01 on (Excel's fake 1900-02-29 is not modelled).
pub fn serial_from_civil(y: i32, m: u32, d: u32) -> i64 {
    days_from_march_0(i64::from(y), i64::from(m), i64::from(d)) - SERIAL_EPOCH
}

/// The date of serial day `days` (see [`serial_from_civil`]).
pub fn civil_from_serial(days: i64) -> (i32, u32, u32) {
    // H. Hinnant's civil_from_days, from days since 0000-03-01.
    let z = days + SERIAL_EPOCH;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y as i32, m as u32, d as u32)
}

/// An ISO date or date-time accepted by [`date`] and [`time`] as a serial day number, the
/// time of day as its fraction. A time zone suffix is ignored.
pub fn iso_serial(raw: &str) -> Option<f64> {
    let rest = date(raw)?;
    let b = raw.as_bytes();
    let (y, m, d) = (digits(b, 4)?, digits(&b[5..], 2)?, digits(&b[8..], 2)?);
    let days = serial_from_civil(y as i32, m, d) as f64;
    if rest.is_empty() {
        return Some(days);
    }
    if !time(rest) {
        return None;
    }
    let t = &rest.as_bytes()[1..];
    let mut seconds = f64::from(digits(t, 2)? * 3_600 + digits(&t[3..], 2)? * 60);
    if t.get(5) == Some(&b':') {
        seconds += f64::from(digits(&t[6..], 2)?);
        if t.get(8) == Some(&b'.') {
            let n = t[9..].iter().take_while(|c| c.is_ascii_digit()).count();
            let frac: f64 = std::str::from_utf8(&t[8..9 + n])
                .ok()?
                .parse::<f64>()
                .ok()?;
            seconds += frac;
        }
    }
    Some(days + seconds / 86_400.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serials_match_spreadsheets() {
        assert_eq!(serial_from_civil(1899, 12, 30), 0);
        assert_eq!(serial_from_civil(1900, 3, 1), 61);
        assert_eq!(serial_from_civil(2024, 1, 1), 45_292);
        assert_eq!(serial_from_civil(1970, 1, 1), 25_569);
        for s in -700_000..3_000_000 {
            let (y, m, d) = civil_from_serial(s);
            assert_eq!(serial_from_civil(y, m, d), s);
        }
    }

    #[test]
    fn iso_serials() {
        assert_eq!(iso_serial("2024-01-01"), Some(45_292.0));
        assert_eq!(iso_serial("2024-01-01T12:00"), Some(45_292.5));
        assert_eq!(iso_serial("2024-01-01 06:00:00Z"), Some(45_292.25));
        let t = iso_serial("2024-01-01T00:00:01.5").unwrap();
        assert!((t - (45_292.0 + 1.5 / 86_400.0)).abs() < 1e-12);
        assert_eq!(iso_serial("2024-02-30"), None);
        assert_eq!(iso_serial("2024-01-01x"), None);
    }
}
