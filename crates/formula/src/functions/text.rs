//! Text functions (FML-6). Lengths and positions count Unicode scalar values; numbers read
//! as text through [`to_text`] (15 significant digits, a date as its serial number).

use super::{add, format, scalar, wildcard, Outcome, Registry};
use crate::ast::{ErrorKind, Expr};
use crate::coerce::{text_to_number, to_bool, to_number, to_text};
use crate::eval::{as_reference, bounds, evaluate, Context, Interrupt};
use crate::value::Value;

/// The longest text `REPT` builds, in characters (Excel's cell text limit).
const MAX_TEXT: usize = 32_767;

/// Windows-1252 for codes 128–159; the undefined codes map to themselves.
const CP1252: [char; 32] = [
    '\u{20AC}', '\u{0081}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{008D}', '\u{017D}', '\u{008F}',
    '\u{0090}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}', '\u{009D}', '\u{017E}', '\u{0178}',
];

/// Why a text function stopped early: an error value for the cell, or an interrupt.
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

type Result<T, E = Stop> = std::result::Result<T, E>;

/// A function's result with errors as values and interrupts passed on.
fn done(r: Result<Value>) -> Outcome {
    match r {
        Ok(v) => Ok(v),
        Err(Stop::Error(e)) => Ok(Value::Error(e)),
        Err(Stop::Interrupt(i)) => Err(i),
    }
}

pub(super) fn register(m: &mut Registry) {
    add(m, "CONCAT", 0, None, |a, c| done(concat(a, c)));
    add(m, "CONCATENATE", 0, None, |a, c| done(concatenate(a, c)));
    add(m, "TEXTJOIN", 3, None, |a, c| done(textjoin(a, c)));
    add(m, "LEFT", 1, Some(2), |a, c| done(left(a, c)));
    add(m, "RIGHT", 1, Some(2), |a, c| done(right(a, c)));
    add(m, "MID", 3, Some(3), |a, c| done(mid(a, c)));
    add(m, "LEN", 1, Some(1), |a, c| {
        done(text(&a[0], c).map(|s| Value::Number(s.chars().count() as f64)))
    });
    add(m, "UPPER", 1, Some(1), |a, c| {
        done(text(&a[0], c).map(|s| Value::text(s.to_uppercase())))
    });
    add(m, "LOWER", 1, Some(1), |a, c| {
        done(text(&a[0], c).map(|s| Value::text(s.to_lowercase())))
    });
    add(m, "PROPER", 1, Some(1), |a, c| {
        done(text(&a[0], c).map(|s| Value::text(proper(&s))))
    });
    add(m, "TRIM", 1, Some(1), |a, c| {
        done(text(&a[0], c).map(|s| Value::text(trim(&s))))
    });
    add(m, "CLEAN", 1, Some(1), |a, c| {
        done(text(&a[0], c).map(|s| Value::text(clean(&s))))
    });
    add(m, "FIND", 2, Some(3), |a, c| done(find(a, c, false)));
    add(m, "SEARCH", 2, Some(3), |a, c| done(find(a, c, true)));
    add(m, "SUBSTITUTE", 3, Some(4), |a, c| done(substitute(a, c)));
    add(m, "REPLACE", 4, Some(4), |a, c| done(replace(a, c)));
    add(m, "REPT", 2, Some(2), |a, c| done(rept(a, c)));
    add(m, "TEXT", 2, Some(2), |a, c| done(text_fn(a, c)));
    add(m, "VALUE", 1, Some(1), |a, c| done(value(a, c)));
    add(m, "EXACT", 2, Some(2), |a, c| {
        done(text(&a[0], c).and_then(|x| Ok(Value::Bool(x == text(&a[1], c)?))))
    });
    add(m, "CHAR", 1, Some(1), |a, c| done(char_fn(a, c)));
    add(m, "CODE", 1, Some(1), |a, c| done(code(a, c)));
    add(m, "T", 1, Some(1), |a, c| {
        done(scalar(&a[0], c).map_err(Stop::from).and_then(|v| match v {
            Value::Text(_) => Ok(v),
            Value::Error(e) => Err(e.into()),
            _ => Ok(Value::text("")),
        }))
    });
}

/// An argument's scalar value as text.
fn text(arg: &Expr, ctx: &mut dyn Context) -> Result<String> {
    Ok(to_text(&scalar(arg, ctx)?)?)
}

/// An argument's scalar value as a number, truncated toward zero.
fn whole(arg: &Expr, ctx: &mut dyn Context) -> Result<f64> {
    Ok(to_number(&scalar(arg, ctx)?)?.trunc())
}

/// A count or position argument: `#VALUE!` below `min`, else a `usize` (saturating).
fn count(arg: &Expr, ctx: &mut dyn Context, min: f64) -> Result<usize> {
    let n = whole(arg, ctx)?;
    if n < min {
        return Err(ErrorKind::Value.into());
    }
    Ok(n as usize)
}

/// Calls `f` with every value of an argument as text: each cell of a reference, empty ones
/// included (as `""`), in row-major order; each element of an array; or the argument's own
/// value. The first error value stops it and is returned.
fn each_text(arg: &Expr, ctx: &mut dyn Context, f: &mut dyn FnMut(&str)) -> Result<()> {
    let Some(r) = as_reference(arg, ctx) else {
        return match evaluate(arg, ctx)? {
            Value::Array(a) => a.values.iter().try_for_each(|v| visit(v, f)),
            v => visit(&v, f),
        };
    };
    let (rows, cols) = bounds(&r, ctx)?;
    let width = u64::from(cols.end - cols.start);
    let mut next = 0u64;
    let mut err = None;
    ctx.area(
        r.sheet.as_deref(),
        rows.clone(),
        cols.clone(),
        &mut |row, col, v| {
            let at = (row - rows.start) * width + u64::from(col - cols.start);
            while next < at {
                f("");
                next += 1;
            }
            next = at + 1;
            match visit(v, f) {
                Ok(()) => true,
                Err(e) => {
                    err = Some(e);
                    false
                }
            }
        },
    )?;
    if let Some(e) = err {
        return Err(e);
    }
    let total = (rows.end - rows.start) * width;
    while next < total {
        f("");
        next += 1;
    }
    Ok(())
}

fn visit(v: &Value, f: &mut dyn FnMut(&str)) -> Result<()> {
    f(&to_text(v)?);
    Ok(())
}

/// `CONCAT(text, …)`: every value of every argument, ranges included, joined.
fn concat(args: &[Expr], ctx: &mut dyn Context) -> Result<Value> {
    let mut out = String::new();
    for a in args {
        each_text(a, ctx, &mut |s| out.push_str(s))?;
    }
    Ok(Value::text(out))
}

/// `CONCATENATE(text, …)`: each argument as a scalar, joined.
fn concatenate(args: &[Expr], ctx: &mut dyn Context) -> Result<Value> {
    let mut out = String::new();
    for a in args {
        out.push_str(&text(a, ctx)?);
    }
    Ok(Value::text(out))
}

/// `TEXTJOIN(delimiter, ignore_empty, text, …)`: the delimiter may be a range or array,
/// whose values are used in turn.
fn textjoin(args: &[Expr], ctx: &mut dyn Context) -> Result<Value> {
    let mut delimiters = Vec::new();
    each_text(&args[0], ctx, &mut |s| delimiters.push(s.to_string()))?;
    let ignore_empty = to_bool(&scalar(&args[1], ctx)?)?;
    let mut out = String::new();
    let mut joined = 0usize;
    for a in &args[2..] {
        each_text(a, ctx, &mut |s| {
            if ignore_empty && s.is_empty() {
                return;
            }
            if joined > 0 {
                out.push_str(&delimiters[(joined - 1) % delimiters.len()]);
            }
            out.push_str(s);
            joined += 1;
        })?;
    }
    Ok(Value::text(out))
}

fn left(args: &[Expr], ctx: &mut dyn Context) -> Result<Value> {
    let s = text(&args[0], ctx)?;
    let n = match args.get(1) {
        Some(a) => count(a, ctx, 0.0)?,
        None => 1,
    };
    Ok(Value::text(s.chars().take(n).collect::<String>()))
}

fn right(args: &[Expr], ctx: &mut dyn Context) -> Result<Value> {
    let s = text(&args[0], ctx)?;
    let n = match args.get(1) {
        Some(a) => count(a, ctx, 0.0)?,
        None => 1,
    };
    let len = s.chars().count();
    Ok(Value::text(
        s.chars().skip(len.saturating_sub(n)).collect::<String>(),
    ))
}

fn mid(args: &[Expr], ctx: &mut dyn Context) -> Result<Value> {
    let s = text(&args[0], ctx)?;
    let start = count(&args[1], ctx, 1.0)?;
    let n = count(&args[2], ctx, 0.0)?;
    Ok(Value::text(
        s.chars().skip(start - 1).take(n).collect::<String>(),
    ))
}

/// Upper case for a letter after a non-letter, lower case for the rest.
fn proper(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut after_letter = false;
    for c in s.chars() {
        if c.is_alphabetic() {
            if after_letter {
                out.extend(c.to_lowercase());
            } else {
                out.extend(c.to_uppercase());
            }
            after_letter = true;
        } else {
            out.push(c);
            after_letter = false;
        }
    }
    out
}

/// Spaces removed at both ends and inner runs of spaces collapsed to one.
fn trim(s: &str) -> String {
    s.split(' ')
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Control characters removed: 0–31 and, as LibreOffice does, 127–159.
fn clean(s: &str) -> String {
    s.chars()
        .filter(|&c| !matches!(u32::from(c), 0..=31 | 127..=159))
        .collect()
}

/// `FIND`/`SEARCH(find, within, [start])`: the 1-based position of the first match at or
/// after `start`. SEARCH ignores case and takes wildcards; an empty `find` is `#VALUE!`.
fn find(args: &[Expr], ctx: &mut dyn Context, search: bool) -> Result<Value> {
    let needle = text(&args[0], ctx)?;
    let within = text(&args[1], ctx)?;
    let start = match args.get(2) {
        Some(a) => count(a, ctx, 1.0)?,
        None => 1,
    } - 1;
    let chars: Vec<char> = within.chars().collect();
    if needle.is_empty() || start >= chars.len() {
        return Err(ErrorKind::Value.into());
    }
    let at = if search {
        wildcard::find(&needle, &chars, start).map(|(s, _)| s)
    } else {
        let offset = within
            .char_indices()
            .nth(start)
            .map_or(within.len(), |(b, _)| b);
        within[offset..]
            .find(&needle)
            .map(|b| start + within[offset..offset + b].chars().count())
    };
    match at {
        Some(at) => Ok(Value::Number((at + 1) as f64)),
        None => Err(ErrorKind::Value.into()),
    }
}

/// `SUBSTITUTE(text, old, new, [instance])`: every occurrence of `old` (matching case),
/// or only the `instance`th.
fn substitute(args: &[Expr], ctx: &mut dyn Context) -> Result<Value> {
    let s = text(&args[0], ctx)?;
    let old = text(&args[1], ctx)?;
    let new = text(&args[2], ctx)?;
    let instance = match args.get(3) {
        Some(a) => Some(count(a, ctx, 1.0)?),
        None => None,
    };
    if old.is_empty() {
        return Ok(Value::text(s));
    }
    Ok(Value::text(match instance {
        None => s.replace(&old, &new),
        Some(n) => match s.match_indices(&old).nth(n - 1) {
            Some((at, _)) => format!("{}{new}{}", &s[..at], &s[at + old.len()..]),
            None => s,
        },
    }))
}

/// `REPLACE(text, start, count, new)`: `count` characters from `start` replaced by `new`.
fn replace(args: &[Expr], ctx: &mut dyn Context) -> Result<Value> {
    let s = text(&args[0], ctx)?;
    let start = count(&args[1], ctx, 1.0)? - 1;
    let n = count(&args[2], ctx, 0.0)?;
    let new = text(&args[3], ctx)?;
    let mut out: String = s.chars().take(start).collect();
    out.push_str(&new);
    out.extend(s.chars().skip(start.saturating_add(n)));
    Ok(Value::text(out))
}

/// `REPT(text, times)`; a result over 32,767 characters is `#VALUE!`.
fn rept(args: &[Expr], ctx: &mut dyn Context) -> Result<Value> {
    let s = text(&args[0], ctx)?;
    let n = count(&args[1], ctx, 0.0)?;
    if s.chars().count().saturating_mul(n) > MAX_TEXT {
        return Err(ErrorKind::Value.into());
    }
    Ok(Value::text(s.repeat(n)))
}

/// `TEXT(value, format)`: numbers, dates, and text that reads as a number go through the
/// format code; other text (and TRUE/FALSE) through its text section.
fn text_fn(args: &[Expr], ctx: &mut dyn Context) -> Result<Value> {
    let v = scalar(&args[0], ctx)?;
    let code = text(&args[1], ctx)?;
    let shown = match &v {
        Value::Error(e) => return Err((*e).into()),
        Value::Text(t) => match text_to_number(t) {
            Some(n) => format::format(n, &code)?,
            None => format::format_text(t, &code)?,
        },
        Value::Bool(_) => format::format_text(&to_text(&v)?, &code)?,
        v => format::format(to_number(v)?, &code)?,
    };
    Ok(Value::text(shown))
}

/// `VALUE(text)`: text read as a number as arithmetic reads it; TRUE/FALSE is `#VALUE!`.
fn value(args: &[Expr], ctx: &mut dyn Context) -> Result<Value> {
    match scalar(&args[0], ctx)? {
        Value::Number(n) | Value::Date(n) => Ok(Value::Number(n)),
        Value::Empty => Ok(Value::Number(0.0)),
        Value::Text(t) => Ok(Value::Number(text_to_number(&t).ok_or(ErrorKind::Value)?)),
        Value::Error(e) => Err(e.into()),
        Value::Bool(_) | Value::Array(_) => Err(ErrorKind::Value.into()),
    }
}

/// `CHAR(n)` for 1–255.
fn char_fn(args: &[Expr], ctx: &mut dyn Context) -> Result<Value> {
    let n = whole(&args[0], ctx)?;
    if !(1.0..=255.0).contains(&n) {
        return Err(ErrorKind::Value.into());
    }
    Ok(Value::text(char_of(n as u32).to_string()))
}

/// `CODE(text)`: the first character's code; 0 for empty text, as LibreOffice gives.
fn code(args: &[Expr], ctx: &mut dyn Context) -> Result<Value> {
    let s = text(&args[0], ctx)?;
    Ok(Value::Number(f64::from(
        s.chars().next().map_or(0, code_of),
    )))
}

/// The character for code `n` (1–255): Windows-1252 for 128–159, else Unicode.
fn char_of(n: u32) -> char {
    match n {
        128..=159 => CP1252[(n - 128) as usize],
        n => char::from_u32(n).expect("codes up to 255 are characters"),
    }
}

/// A character's code: Windows-1252 for the characters it puts at 128–159, else Unicode.
fn code_of(c: char) -> u32 {
    match CP1252.iter().position(|&x| x == c) {
        Some(i) => 128 + i as u32,
        None => u32::from(c),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn char_and_code_use_windows_1252() {
        assert_eq!(char_of(128), '€');
        assert_eq!(char_of(153), '™');
        assert_eq!(char_of(233), 'é');
        assert_eq!(code_of('€'), 128);
        assert_eq!(code_of('—'), 151);
        assert_eq!(code_of('é'), 233);
        assert_eq!(code_of('中'), 20_013);
        for n in 1..=255 {
            assert_eq!(code_of(char_of(n)), n);
        }
    }

    #[test]
    fn clean_drops_c0_and_c1_controls() {
        assert_eq!(clean("a\tb\u{7f}\u{85}\u{9f}c\u{a0}"), "abc\u{a0}");
    }
}
