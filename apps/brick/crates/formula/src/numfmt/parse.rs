//! Compiling a format code: tokens per section, then each section's kind and layout.

use super::{
    AmPm, Body, Cond, DateTime, FormatColor, Fraction, Item, Number, NumberFormat, Op, Ph, Result,
    Scientific, Section, Unit,
};
use crate::ast::ErrorKind;

const BAD: ErrorKind = ErrorKind::Value;

/// `[$-F800]`: the system long date, as en-US writes it.
const LONG_DATE: &str = "dddd, mmmm d, yyyy";
/// `[$-F400]`: the system long time, as en-US writes it.
const LONG_TIME: &str = "h:mm:ss AM/PM";

const COLORS: [(&str, FormatColor); 8] = [
    ("black", FormatColor::Black),
    ("blue", FormatColor::Blue),
    ("cyan", FormatColor::Cyan),
    ("green", FormatColor::Green),
    ("magenta", FormatColor::Magenta),
    ("red", FormatColor::Red),
    ("white", FormatColor::White),
    ("yellow", FormatColor::Yellow),
];

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Lit(String),
    Fill(char),
    Ph(Ph),
    /// A bare `1`–`9`: only a fixed fraction denominator may hold one.
    Digit(u8),
    Point,
    Comma,
    Percent,
    At,
    Slash,
    /// `E+`, `E-`, `e+`, `e-`: the letter shown and whether a `+` shows.
    Exp {
        letter: char,
        plus: bool,
    },
    General,
    /// A run of `y`, `m`, `d`, `h`, or `s` (lowercased) and its length.
    Date(char, usize),
    AmPm(AmPm),
    Elapsed(Unit, u8),
}

/// One section's tokens and bracket tags.
#[derive(Default)]
struct Raw {
    toks: Vec<Tok>,
    color: Option<FormatColor>,
    cond: Option<Cond>,
    /// `[$-F800]` or `[$-F400]` gave the section's tokens; the rest are ignored.
    system: bool,
}

pub(super) fn parse(code: &str) -> Result<NumberFormat> {
    let raws = sections(code)?;
    if raws.len() > 4 || code.is_empty() {
        return Err(BAD);
    }
    // Conditions only in the first two sections, the second only after the first.
    for (i, raw) in raws.iter().enumerate() {
        if raw.cond.is_some() && (i > 1 || (i == 1 && raws[0].cond.is_none())) {
            return Err(BAD);
        }
    }
    let last = raws.len() - 1;
    let mut numeric = Vec::new();
    let mut text = None;
    for (i, raw) in raws.into_iter().enumerate() {
        let is_text = raw.toks.contains(&Tok::At);
        if i == 3 || (is_text && i == last) {
            if raw.cond.is_some() {
                return Err(BAD);
            }
            let body = if raw.toks.is_empty() {
                Body::Empty
            } else {
                Body::Text(literal_section(raw.toks, Tok::At, Item::At)?)
            };
            text = Some(Section {
                color: raw.color,
                body,
            });
        } else if is_text {
            return Err(BAD);
        } else {
            let section = Section {
                color: raw.color,
                body: body(raw.toks)?,
            };
            numeric.push((section, raw.cond));
        }
    }
    let general = numeric.first().is_some_and(|(s, cond)| {
        s.color.is_none() && cond.is_none() && s.body == Body::General(vec![Item::General])
    });
    // Default conditions, as LibreOffice and Excel set them.
    let n = numeric.len();
    let mut first = numeric.first().and_then(|(_, c)| *c);
    let mut second = numeric.get(1).and_then(|(_, c)| *c);
    let zero = |op| Some(Cond { op, limit: 0.0 });
    if n >= 2 && first.is_none() {
        first = zero(Op::Gt);
    }
    if n >= 3 && second.is_none() {
        second = zero(Op::Lt);
    }
    if n < 3 && Cond::is(first, Op::Gt) && second.is_none() {
        first = zero(Op::Ge);
    }
    Ok(NumberFormat {
        numeric: numeric.into_iter().map(|(s, _)| s).collect(),
        text,
        first,
        second,
        general,
    })
}

fn starts_with_ci(chars: &[char], word: &str) -> bool {
    let mut it = chars.iter();
    word.chars()
        .all(|w| it.next().is_some_and(|c| c.eq_ignore_ascii_case(&w)))
}

/// The code's sections, tokenized.
fn sections(code: &str) -> Result<Vec<Raw>> {
    let chars: Vec<char> = code.chars().collect();
    let mut out = vec![Raw::default()];
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let raw = out.last_mut().expect("at least one section");
        let next = chars.get(i + 1).copied();
        let mut len = 1;
        let tok = match c.to_ascii_lowercase() {
            ';' => {
                out.push(Raw::default());
                i += 1;
                continue;
            }
            '"' => {
                let end = chars[i + 1..].iter().position(|&d| d == '"').ok_or(BAD)?;
                len = end + 2;
                Tok::Lit(chars[i + 1..i + 1 + end].iter().collect())
            }
            '\\' => {
                len = 2;
                Tok::Lit(next.ok_or(BAD)?.to_string())
            }
            '_' => {
                next.ok_or(BAD)?;
                len = 2;
                Tok::Lit(" ".to_string())
            }
            '*' => {
                len = 2;
                Tok::Fill(next.ok_or(BAD)?)
            }
            '[' => {
                let end = chars[i + 1..].iter().position(|&d| d == ']').ok_or(BAD)?;
                let inner: String = chars[i + 1..i + 1 + end].iter().collect();
                if let Some(toks) = bracket(&inner, raw)? {
                    raw.toks.extend(toks);
                }
                i += end + 2;
                continue;
            }
            '0' => Tok::Ph(Ph::Zero),
            '#' => Tok::Ph(Ph::Hash),
            '?' => Tok::Ph(Ph::Space),
            '1'..='9' => Tok::Digit(c as u8 - b'0'),
            '.' => Tok::Point,
            ',' => Tok::Comma,
            '%' => Tok::Percent,
            '@' => Tok::At,
            '/' => Tok::Slash,
            'e' => match next {
                Some(s @ ('+' | '-')) => {
                    len = 2;
                    Tok::Exp {
                        letter: if c == 'e' && s == '-' { 'e' } else { 'E' },
                        plus: s == '+',
                    }
                }
                _ => return Err(BAD),
            },
            'g' if starts_with_ci(&chars[i..], "general") => {
                len = 7;
                Tok::General
            }
            'a' if starts_with_ci(&chars[i..], "am/pm") => {
                len = 5;
                Tok::AmPm(AmPm::Full)
            }
            'a' if starts_with_ci(&chars[i..], "a/p") => {
                len = 3;
                Tok::AmPm(AmPm::Short {
                    a: c,
                    p: chars[i + 2],
                })
            }
            l @ ('y' | 'm' | 'd' | 'h' | 's') => {
                len = chars[i..]
                    .iter()
                    .take_while(|d| d.to_ascii_lowercase() == l)
                    .count();
                Tok::Date(l, len)
            }
            ' ' | '$' | '-' | '+' | '(' | ')' | ':' | '!' | '^' | '&' | '\'' | '~' | '{' | '}'
            | '<' | '>' | '=' => Tok::Lit(c.to_string()),
            _ if !c.is_ascii() => Tok::Lit(c.to_string()),
            _ => return Err(BAD),
        };
        if !raw.system {
            raw.toks.push(tok);
        }
        i += len;
    }
    Ok(out)
}

/// A `[…]` tag: a condition or colour goes into `raw`; a currency symbol or elapsed time
/// gives tokens; a locale, `[DBNum1]`, and other tags are ignored.
fn bracket(inner: &str, raw: &mut Raw) -> Result<Option<Vec<Tok>>> {
    let lower = inner.to_ascii_lowercase();
    if inner.is_empty() {
        return Err(BAD);
    }
    if inner.starts_with(['<', '>', '=']) {
        if raw.cond.is_some() {
            return Err(BAD);
        }
        let (op, rest) = [
            ("<=", Op::Le),
            (">=", Op::Ge),
            ("<>", Op::Ne),
            ("<", Op::Lt),
            (">", Op::Gt),
            ("=", Op::Eq),
        ]
        .into_iter()
        .find_map(|(s, op)| inner.strip_prefix(s).map(|rest| (op, rest)))
        .expect("starts with an operator");
        let limit: f64 = rest.trim().parse().map_err(|_| BAD)?;
        if !limit.is_finite() {
            return Err(BAD);
        }
        raw.cond = Some(Cond { op, limit });
        return Ok(None);
    }
    if let Some(&(_, color)) = COLORS.iter().find(|(name, _)| *name == lower) {
        raw.color = Some(color);
        return Ok(None);
    }
    if let Some(n) = lower.strip_prefix("color") {
        if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) {
            let n: u8 = n.parse().map_err(|_| BAD)?;
            if !(1..=56).contains(&n) {
                return Err(BAD);
            }
            raw.color = Some(FormatColor::Indexed(n));
            return Ok(None);
        }
    }
    if let Some(rest) = inner.strip_prefix('$') {
        if raw.system {
            return Ok(None);
        }
        let (symbol, locale) = rest.split_once('-').unwrap_or((rest, ""));
        let mut toks = Vec::new();
        if !symbol.is_empty() {
            toks.push(Tok::Lit(symbol.to_string()));
        }
        // The system long date and time replace the section's own tokens.
        let system = match locale.to_ascii_uppercase().as_str() {
            "F800" => Some(LONG_DATE),
            "F400" => Some(LONG_TIME),
            _ => None,
        };
        if let Some(code) = system {
            let mut sub = sections(code)?;
            raw.toks.clear();
            raw.system = true;
            toks.append(&mut sub[0].toks);
        }
        return Ok(Some(toks));
    }
    let first = lower.chars().next().expect("not empty");
    if matches!(first, 'h' | 'm' | 's') && lower.chars().all(|c| c == first) {
        let unit = match first {
            'h' => Unit::Hour,
            'm' => Unit::Minute,
            _ => Unit::Second,
        };
        let width = u8::try_from(lower.len()).map_err(|_| BAD)?;
        return Ok(Some(vec![Tok::Elapsed(unit, width)]));
    }
    Ok(None)
}

/// A number section's kind and layout from its tokens.
fn body(toks: Vec<Tok>) -> Result<Body> {
    if toks.is_empty() {
        return Ok(Body::Empty);
    }
    let has = |f: &dyn Fn(&Tok) -> bool| toks.iter().any(f);
    if has(&|t| matches!(t, Tok::Date(..) | Tok::AmPm(_) | Tok::Elapsed(..))) {
        return date_time(toks).map(Body::DateTime);
    }
    if has(&|t| *t == Tok::General) {
        return literal_section(toks, Tok::General, Item::General).map(Body::General);
    }
    if has(&|t| matches!(t, Tok::Exp { .. })) {
        return scientific(toks).map(Body::Scientific);
    }
    if let Some(slash) = fraction_bar(&toks) {
        return fraction(toks, slash).map(Body::Fraction);
    }
    number(toks).map(Body::Number)
}

/// The literal form of a token outside its meaning (`.` `,` `%` `/`), or the token's item.
fn literal(tok: &Tok) -> Option<Item> {
    Some(match tok {
        Tok::Lit(s) => Item::Lit(s.as_str().into()),
        Tok::Fill(c) => Item::Fill(*c),
        Tok::Point => Item::Lit(".".into()),
        Tok::Comma => Item::Lit(",".into()),
        Tok::Percent => Item::Lit("%".into()),
        Tok::Slash => Item::Lit("/".into()),
        _ => return None,
    })
}

/// Adjacent literals merged into one.
fn push(items: &mut Vec<Item>, item: Item) {
    if let (Some(Item::Lit(prev)), Item::Lit(s)) = (items.last_mut(), &item) {
        let mut joined = prev.to_string();
        joined.push_str(s);
        *prev = joined.into();
    } else {
        items.push(item);
    }
}

/// A text or General section: literals around `@` or `General`.
fn literal_section(toks: Vec<Tok>, key: Tok, item: Item) -> Result<Vec<Item>> {
    let mut items = Vec::new();
    for t in &toks {
        if *t == key {
            items.push(item.clone());
        } else {
            push(&mut items, literal(t).ok_or(BAD)?);
        }
    }
    Ok(items)
}

fn date_time(toks: Vec<Tok>) -> Result<DateTime> {
    let mut items: Vec<Item> = Vec::new();
    let mut k = 0;
    while k < toks.len() {
        let t = &toks[k];
        k += 1;
        let item = match t {
            Tok::Date('y', n) => Item::Year(*n > 2),
            Tok::Date('m', n) if *n <= 5 => Item::Month(*n as u8),
            Tok::Date('d', n) => Item::Day((*n).min(4) as u8),
            Tok::Date('h', n) => Item::Hour(*n > 1),
            Tok::Date('s', n) => Item::Second(*n > 1),
            Tok::Date(..) => return Err(BAD),
            Tok::AmPm(a) => Item::AmPm(*a),
            Tok::Elapsed(u, w) => Item::Elapsed(*u, *w),
            Tok::Point => {
                let zeros = toks[k..]
                    .iter()
                    .take_while(|t| **t == Tok::Ph(Ph::Zero))
                    .count();
                let after_seconds = matches!(
                    items.last(),
                    Some(Item::Second(_) | Item::Elapsed(Unit::Second, _))
                );
                if zeros > 0 && after_seconds {
                    if zeros > 3 {
                        return Err(BAD);
                    }
                    k += zeros;
                    Item::SubSecond(zeros as u8)
                } else {
                    Item::Lit(".".into())
                }
            }
            t => literal(t).ok_or(BAD)?,
        };
        push(&mut items, item);
    }
    // `m`/`mm` right after an hour or right before seconds means minutes.
    let times: Vec<usize> = (0..items.len())
        .filter(|&i| !matches!(items[i], Item::Lit(_) | Item::Fill(_)))
        .collect();
    for (k, &i) in times.iter().enumerate() {
        if let Item::Month(n @ (1 | 2)) = items[i] {
            let after_hour = k > 0
                && matches!(
                    items[times[k - 1]],
                    Item::Hour(_) | Item::Elapsed(Unit::Hour, _)
                );
            let before_second = times.get(k + 1).is_some_and(|&j| {
                matches!(items[j], Item::Second(_) | Item::Elapsed(Unit::Second, _))
            });
            if after_hour || before_second {
                items[i] = Item::Minute(n == 2);
            }
        }
    }
    let sub = items
        .iter()
        .filter_map(|i| match i {
            Item::SubSecond(n) => Some(*n),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    let twelve = items.iter().any(|i| matches!(i, Item::AmPm(_)));
    let elapsed = items.iter().any(|i| matches!(i, Item::Elapsed(..)));
    let timed = elapsed
        || items.iter().any(|i| {
            matches!(
                i,
                Item::Hour(_)
                    | Item::Minute(_)
                    | Item::Second(_)
                    | Item::SubSecond(_)
                    | Item::AmPm(_)
            )
        });
    Ok(DateTime {
        items,
        sub,
        twelve,
        timed,
        elapsed,
    })
}

fn number(toks: Vec<Tok>) -> Result<Number> {
    let point = toks.iter().position(|t| *t == Tok::Point);
    let int_end = point.unwrap_or(toks.len());
    let is_ph = |t: &Tok| matches!(t, Tok::Ph(_));
    let mut n = Number {
        items: Vec::new(),
        ints: 0,
        fracs: Vec::new(),
        group: false,
        thousands: 0,
        percents: 0,
    };
    for (k, t) in toks.iter().enumerate() {
        let int = k < int_end;
        let item = match t {
            Tok::Ph(p) if int => {
                n.ints += 1;
                Item::Int(*p)
            }
            Tok::Ph(p) => {
                n.fracs.push(*p);
                Item::Frac(*p)
            }
            Tok::Point if Some(k) == point => Item::Point,
            Tok::Comma => {
                let zone_end = if int { int_end } else { toks.len() };
                let ph_after = toks[k + 1..zone_end].iter().any(is_ph);
                let ph_before = toks[..k].iter().any(is_ph);
                match (ph_before, ph_after) {
                    (true, true) if int => {
                        n.group = true;
                        continue;
                    }
                    (true, false) => {
                        n.thousands += 1;
                        continue;
                    }
                    _ => Item::Lit(",".into()),
                }
            }
            Tok::Percent => {
                n.percents += 1;
                Item::Lit("%".into())
            }
            t => literal(t).ok_or(BAD)?,
        };
        push(&mut n.items, item);
    }
    Ok(n)
}

fn scientific(toks: Vec<Tok>) -> Result<Scientific> {
    let mut s = Scientific {
        items: Vec::new(),
        ints: 0,
        fracs: Vec::new(),
        exp_digits: Vec::new(),
        percents: 0,
    };
    let (mut point, mut exp) = (false, false);
    for t in &toks {
        let item = match t {
            Tok::Ph(p) if exp => {
                s.exp_digits.push(*p);
                Item::ExpDigit(*p)
            }
            Tok::Ph(p) if point => {
                s.fracs.push(*p);
                Item::Frac(*p)
            }
            Tok::Ph(p) => {
                s.ints += 1;
                Item::Int(*p)
            }
            Tok::Point if !point && !exp => {
                point = true;
                Item::Point
            }
            Tok::Exp { letter, plus } if !exp => {
                exp = true;
                Item::Exp {
                    letter: *letter,
                    plus: *plus,
                }
            }
            Tok::Percent => {
                s.percents += 1;
                Item::Lit("%".into())
            }
            Tok::Comma if !exp => continue,
            Tok::Lit(_) | Tok::Fill(_) | Tok::Slash => literal(t).expect("a literal"),
            _ => return Err(BAD),
        };
        push(&mut s.items, item);
    }
    if s.ints + s.fracs.len() == 0 || s.exp_digits.is_empty() {
        return Err(BAD);
    }
    Ok(s)
}

/// The `/` of a fraction: digit placeholders right before it and a placeholder or digit
/// right after it.
fn fraction_bar(toks: &[Tok]) -> Option<usize> {
    (1..toks.len().saturating_sub(1)).find(|&k| {
        toks[k] == Tok::Slash
            && matches!(toks[k - 1], Tok::Ph(_))
            && matches!(toks[k + 1], Tok::Ph(_) | Tok::Digit(_))
    })
}

fn fraction(toks: Vec<Tok>, slash: usize) -> Result<Fraction> {
    let num_start = toks[..slash]
        .iter()
        .rposition(|t| !matches!(t, Tok::Ph(_)))
        .map_or(0, |k| k + 1);
    // The denominator: placeholders, or digits for a fixed one (`0` is a digit there).
    let fixed = matches!(toks[slash + 1], Tok::Digit(_));
    let den_end = slash
        + 1
        + toks[slash + 1..]
            .iter()
            .take_while(|t| {
                if fixed {
                    matches!(t, Tok::Digit(_) | Tok::Ph(Ph::Zero))
                } else {
                    matches!(t, Tok::Ph(_))
                }
            })
            .count();
    let mut f = Fraction {
        items: Vec::new(),
        ints: 0,
        nums: Vec::new(),
        dens: Vec::new(),
        fixed: None,
        blank_from: 0,
        blank_to: 0,
        blank: 0,
        percents: 0,
    };
    if fixed {
        let mut d: u32 = 0;
        for t in &toks[slash + 1..den_end] {
            let digit = match t {
                Tok::Digit(n) => u32::from(*n),
                _ => 0,
            };
            d = d
                .checked_mul(10)
                .and_then(|d| d.checked_add(digit))
                .ok_or(BAD)?;
        }
        f.fixed = Some(d);
    }
    let mut gap = 0;
    for (k, t) in toks.iter().enumerate() {
        let item = match t {
            Tok::Ph(p) if k < num_start => {
                f.ints += 1;
                Item::Int(*p)
            }
            Tok::Ph(p) if k < slash => {
                f.nums.push(*p);
                if f.nums.len() > 1 {
                    continue;
                }
                Item::Num
            }
            _ if k == slash => Item::Bar,
            Tok::Ph(p) if k < den_end && !fixed => {
                f.dens.push(*p);
                if f.dens.len() > 1 {
                    continue;
                }
                Item::Den
            }
            _ if k > slash && k < den_end => {
                if k > slash + 1 {
                    continue;
                }
                Item::Den
            }
            Tok::Ph(_) | Tok::Digit(_) | Tok::Point => return Err(BAD),
            Tok::Percent => {
                f.percents += 1;
                Item::Lit("%".into())
            }
            t => {
                let item = literal(t).ok_or(BAD)?;
                if f.ints > 0 && k < num_start {
                    if let Item::Lit(s) = &item {
                        if f.blank_from == 0 {
                            f.blank_from = f.items.len();
                        }
                        gap += s.chars().count();
                    }
                }
                item
            }
        };
        f.items.push(item);
    }
    f.blank_to = 1 + f
        .items
        .iter()
        .position(|i| *i == Item::Den)
        .expect("a denominator");
    if f.ints > 0 && f.blank_from == 0 {
        f.blank_from = f
            .items
            .iter()
            .position(|i| *i == Item::Num)
            .expect("a numerator");
    }
    f.blank = gap + f.nums.len() + 1 + f.dens.len();
    Ok(f)
}
