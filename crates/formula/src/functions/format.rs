//! Number format codes, as `TEXT` reads them (FML-6), with LibreOffice's results as the
//! target. Supported: digit placeholders `0` and `#`, the decimal point `.`, thousands
//! separators `,` (a `,` after the last integer placeholder divides by 1,000), `%`, literal
//! text in `"…"` or after `\` (and the characters `$ - + / ( ) : ! ^ & ' ~ { } < > =`, space,
//! and any non-ASCII character as themselves), `General`, `@`, sections split by `;`
//! (positive; negative; zero; text), and the date and time tokens `yyyy yy mmmm mmm mm m
//! dddd ddd dd d hh h mm ss AM/PM` in any case. `m`/`mm` right after an hour or right before
//! seconds means minutes. Anything else is `#VALUE!`.

use crate::ast::ErrorKind;
use crate::scalar::civil_from_serial;
use crate::value::number_text;
use std::fmt::Write;

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

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Literal(String),
    /// A digit placeholder: `0` (true: shows a zero where there is no digit) or `#`.
    Digit(bool),
    Point,
    Comma,
    Percent,
    General,
    /// `@`: the text itself.
    Text,
    Year(usize),
    Month(usize),
    Minute(usize),
    Day(usize),
    Hour(usize),
    Second(usize),
    AmPm,
}

impl Token {
    fn is_digit(&self) -> bool {
        matches!(self, Token::Digit(_))
    }

    fn is_date(&self) -> bool {
        matches!(
            self,
            Token::Year(_)
                | Token::Month(_)
                | Token::Minute(_)
                | Token::Day(_)
                | Token::Hour(_)
                | Token::Second(_)
                | Token::AmPm
        )
    }
}

/// `v` formatted by `code`.
pub fn format(v: f64, code: &str) -> Result<String, ErrorKind> {
    if !v.is_finite() {
        return Err(ErrorKind::Num);
    }
    if code.is_empty() {
        return Ok(String::new());
    }
    let sections = parse(code)?;
    let (section, x, signed) = match sections.len().min(3) {
        1 => (&sections[0], v, true),
        2 if v >= 0.0 => (&sections[0], v, false),
        2 => (&sections[1], -v, false),
        _ if v > 0.0 => (&sections[0], v, false),
        _ if v < 0.0 => (&sections[1], -v, false),
        _ => (&sections[2], v, false),
    };
    if section.contains(&Token::Text) {
        return Ok(number_text(x));
    }
    let date = section.iter().any(Token::is_date);
    if date && section.iter().any(|t| t.is_digit() || *t == Token::General) {
        return Err(ErrorKind::Value);
    }
    if date {
        date_time(section, x)
    } else {
        Ok(number(section, x, signed))
    }
}

/// Text formatted by `code`: through the text section (the fourth, or a first section
/// holding `@`), else unchanged.
pub fn format_text(text: &str, code: &str) -> Result<String, ErrorKind> {
    let sections = parse(code)?;
    let section = match sections.len() {
        4 => &sections[3],
        _ if sections[0].contains(&Token::Text) => &sections[0],
        _ => return Ok(text.to_string()),
    };
    let mut out = String::new();
    for t in section {
        match t {
            Token::Literal(s) => out.push_str(s),
            Token::Text => out.push_str(text),
            _ => {}
        }
    }
    Ok(out)
}

/// The code's sections (at most 4), each as tokens.
fn parse(code: &str) -> Result<Vec<Vec<Token>>, ErrorKind> {
    let mut sections = Vec::new();
    let (mut start, mut quoted, mut escaped) = (0, false, false);
    for (i, c) in code.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '"' => quoted = !quoted,
            '\\' if !quoted => escaped = true,
            ';' if !quoted => {
                sections.push(tokens(&code[start..i])?);
                start = i + 1;
            }
            _ => {}
        }
    }
    sections.push(tokens(&code[start..])?);
    if sections.len() > 4 {
        return Err(ErrorKind::Value);
    }
    Ok(sections)
}

fn starts_with_ci(chars: &[char], word: &str) -> bool {
    let mut it = chars.iter();
    word.chars()
        .all(|w| it.next().is_some_and(|c| c.eq_ignore_ascii_case(&w)))
}

fn tokens(section: &str) -> Result<Vec<Token>, ErrorKind> {
    let chars: Vec<char> = section.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let lower = c.to_ascii_lowercase();
        let run = chars[i..]
            .iter()
            .take_while(|d| d.to_ascii_lowercase() == lower)
            .count();
        let (token, len) = match lower {
            '"' => {
                let end = chars[i + 1..]
                    .iter()
                    .position(|&d| d == '"')
                    .ok_or(ErrorKind::Value)?;
                (
                    Token::Literal(chars[i + 1..i + 1 + end].iter().collect()),
                    end + 2,
                )
            }
            '\\' => {
                let next = chars.get(i + 1).ok_or(ErrorKind::Value)?;
                (Token::Literal(next.to_string()), 2)
            }
            '0' => (Token::Digit(true), 1),
            '#' => (Token::Digit(false), 1),
            '.' => (Token::Point, 1),
            ',' => (Token::Comma, 1),
            '%' => (Token::Percent, 1),
            '@' => (Token::Text, 1),
            'y' if run == 2 || run == 4 => (Token::Year(run), run),
            'm' if run <= 4 => (Token::Month(run), run),
            'd' if run <= 4 => (Token::Day(run), run),
            'h' if run <= 2 => (Token::Hour(run), run),
            's' if run <= 2 => (Token::Second(run), run),
            'g' if starts_with_ci(&chars[i..], "general") => (Token::General, 7),
            'a' if starts_with_ci(&chars[i..], "am/pm") => (Token::AmPm, 5),
            ' ' | '$' | '-' | '+' | '/' | '(' | ')' | ':' | '!' | '^' | '&' | '\'' | '~' | '{'
            | '}' | '<' | '>' | '=' => (Token::Literal(c.to_string()), 1),
            _ if !c.is_ascii() => (Token::Literal(c.to_string()), 1),
            _ => return Err(ErrorKind::Value),
        };
        out.push(token);
        i += len;
    }
    Ok(out)
}

/// A number section: `x` is the value to show (made positive for a negative section);
/// `signed` adds a `-` when a negative `x` doesn't round to zero.
fn number(section: &[Token], x: f64, signed: bool) -> String {
    let point = section.iter().position(|t| *t == Token::Point);
    let int_end = point.unwrap_or(section.len());
    let n_int = section[..int_end].iter().filter(|t| t.is_digit()).count();
    let zeros_after: Vec<bool> = point.map_or_else(Vec::new, |p| {
        section[p + 1..]
            .iter()
            .filter_map(|t| match t {
                Token::Digit(zero) => Some(*zero),
                _ => None,
            })
            .collect()
    });
    let places = zeros_after.len();
    // A comma followed by a digit placeholder in its part groups thousands (in the integer
    // part) or is literal (in the fraction); one with none after it divides by 1,000.
    let digit_follows = |i: usize| {
        let end = if i < int_end { int_end } else { section.len() };
        section[i + 1..end].iter().any(Token::is_digit)
    };
    let mut grouping = false;
    let mut y = x.abs();
    for (i, t) in section.iter().enumerate() {
        match t {
            Token::Comma if digit_follows(i) => grouping |= i < int_end,
            Token::Comma => y /= 1_000.0,
            Token::Percent => y *= 100.0,
            _ => {}
        }
    }
    let (int, frac) = round_decimal(y, places);
    let mut out = String::new();
    let shows_digits = n_int + places > 0;
    let nonzero = if shows_digits {
        !int.is_empty() || frac.iter().any(|&d| d != b'0')
    } else {
        y != 0.0
    };
    if signed && x < 0.0 && nonzero {
        out.push('-');
    }
    // A `#` in the fraction shows only when it or a later placeholder has something to show.
    let mut shown = vec![false; places];
    let mut later = false;
    for j in (0..places).rev() {
        later |= zeros_after[j] || frac[j] != b'0';
        shown[j] = later;
    }
    let push_int_digit = |out: &mut String, k: usize, d: u8| {
        out.push(char::from(d));
        if grouping && k > 0 && k.is_multiple_of(3) {
            out.push(',');
        }
    };
    let (mut i, mut j) = (0, 0);
    let len = int.len();
    for (at, t) in section.iter().enumerate() {
        match t {
            Token::Digit(zero) if point.is_none_or(|p| at < p) => {
                let r = n_int - 1 - i;
                if i == 0 && len > r {
                    for (idx, &d) in int[..len - r].iter().enumerate() {
                        push_int_digit(&mut out, len - 1 - idx, d);
                    }
                } else if r < len {
                    push_int_digit(&mut out, r, int[len - 1 - r]);
                } else if *zero {
                    push_int_digit(&mut out, r, b'0');
                }
                i += 1;
            }
            Token::Digit(_) => {
                if shown[j] {
                    out.push(char::from(frac[j]));
                }
                j += 1;
            }
            Token::Point => {
                if n_int == 0 {
                    out.extend(int.iter().map(|&d| char::from(d)));
                }
                if shown.first() == Some(&true) {
                    out.push('.');
                }
            }
            Token::Comma if at > int_end && digit_follows(at) => out.push(','),
            Token::Percent => out.push('%'),
            Token::Literal(s) => out.push_str(s),
            Token::General => out.push_str(&number_text(y)),
            _ => {}
        }
    }
    out
}

/// `y` (not negative) rounded half away from zero to `places` decimals, after rounding to
/// 15 significant digits as LibreOffice does (so `1.005` gives `1.01`): the integer digits
/// without leading zeros (none for 0) and exactly `places` fraction digits, as ASCII.
fn round_decimal(y: f64, places: usize) -> (Vec<u8>, Vec<u8>) {
    if y == 0.0 {
        return (Vec::new(), vec![b'0'; places]);
    }
    let s = format!("{y:.14e}");
    let (mantissa, exp) = s.split_once('e').expect("{:e} has an exponent");
    let exp: i64 = exp.parse().expect("an integer exponent");
    let mut digits: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).collect();
    // The value is 0.d1d2… × 10^point.
    let mut point = exp + 1;
    let keep = point + places as i64;
    if keep < 0 {
        return (Vec::new(), vec![b'0'; places]);
    }
    let keep = keep as usize;
    if keep < digits.len() {
        let up = digits[keep] >= b'5';
        digits.truncate(keep);
        if up {
            let mut i = keep;
            loop {
                if i == 0 {
                    digits.insert(0, b'1');
                    point += 1;
                    break;
                }
                i -= 1;
                if digits[i] == b'9' {
                    digits[i] = b'0';
                } else {
                    digits[i] += 1;
                    break;
                }
            }
        }
    }
    let digit = |pos: i64| -> u8 {
        usize::try_from(pos)
            .ok()
            .and_then(|p| digits.get(p).copied())
            .unwrap_or(b'0')
    };
    let int: Vec<u8> = (0..point.max(0))
        .map(digit)
        .skip_while(|&d| d == b'0')
        .collect();
    let frac = (0..places as i64).map(|j| digit(point + j)).collect();
    (int, frac)
}

/// A date and time section: the serial `x` (days since 1899-12-30, the time as the
/// fraction), with the seconds cut off as LibreOffice does.
fn date_time(section: &[Token], x: f64) -> Result<String, ErrorKind> {
    if x.abs() > 1e8 {
        return Err(ErrorKind::Value);
    }
    let secs: f64 = format!("{:.14e}", x * 86_400.0)
        .parse()
        .expect("{:e} parses as f64");
    let secs = secs.floor() as i64;
    let (days, time) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, m, d) = civil_from_serial(days);
    let weekday = (days + 6).rem_euclid(7) as usize;
    let (hour, minute, second) = (time / 3_600, time / 60 % 60, time % 60);
    let twelve = section.contains(&Token::AmPm);
    let mut out = String::new();
    let dated: Vec<(usize, &Token)> = section
        .iter()
        .enumerate()
        .filter(|(_, t)| t.is_date())
        .collect();
    for (at, t) in section.iter().enumerate() {
        let t = match t {
            Token::Month(n) if *n <= 2 => {
                let k = dated
                    .iter()
                    .position(|(i, _)| *i == at)
                    .expect("a date token is listed");
                let after_hour = k > 0 && matches!(dated[k - 1].1, Token::Hour(_));
                let before_second = matches!(dated.get(k + 1), Some((_, Token::Second(_))));
                if after_hour || before_second {
                    &Token::Minute(*n)
                } else {
                    t
                }
            }
            t => t,
        };
        let two = |out: &mut String, n: i64, width: usize| {
            let _ = write!(out, "{n:0width$}");
        };
        match t {
            Token::Literal(s) => out.push_str(s),
            Token::Year(2) => two(&mut out, i64::from(y).rem_euclid(100), 2),
            Token::Year(_) => two(&mut out, i64::from(y), 4),
            Token::Month(3) => out.push_str(&MONTHS[m as usize - 1][..3]),
            Token::Month(4) => out.push_str(MONTHS[m as usize - 1]),
            Token::Month(n) => two(&mut out, i64::from(m), *n),
            Token::Day(3) => out.push_str(&DAYS[weekday][..3]),
            Token::Day(4) => out.push_str(DAYS[weekday]),
            Token::Day(n) => two(&mut out, i64::from(d), *n),
            Token::Hour(n) => {
                let h = if twelve { (hour + 11) % 12 + 1 } else { hour };
                two(&mut out, h, *n);
            }
            Token::Minute(n) => two(&mut out, minute, *n),
            Token::Second(n) => two(&mut out, second, *n),
            Token::AmPm => out.push_str(if hour < 12 { "AM" } else { "PM" }),
            Token::Comma => out.push(','),
            Token::Point => out.push('.'),
            Token::Percent => out.push('%'),
            _ => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_codes_are_value_errors() {
        for code in [
            "0.0E+00",
            "# ?/?",
            "[Red]0",
            "[h]:mm",
            "hh:mm:ss.00",
            "x",
            "y",
            "yyy",
            "mmmmm",
            "hh:mm A/P",
            "_(0",
            "*-0",
            "\"unclosed",
            "0;0;0;@;0",
        ] {
            assert_eq!(format(5.0, code), Err(ErrorKind::Value), "{code}");
        }
    }
}
