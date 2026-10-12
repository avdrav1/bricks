//! Excel number formats (STYLE-2): how a cell shows its value under a format code, for the
//! grid and for `TEXT`. LibreOffice's formatted text is the oracle
//! (`tests/data/formats-libreoffice.tsv`); where Excel shows something else, Brick follows
//! Excel and the fixture says why.
//!
//! A code has up to four sections, `positive;negative;zero;text`, or two conditional
//! sections and an else (`[>=100]0;[<0]-0;0`). Sections are picked as LibreOffice and Excel
//! pick them; a minus sign is added only in the first section, and not when its condition
//! already means negative numbers (`[<0]`). [`NumberFormat::parse`] does all the string
//! work; [`NumberFormat::display`] allocates only the output text.

mod parse;
mod render;

use crate::ast::ErrorKind;
use crate::value::Value;

type Result<T, E = ErrorKind> = std::result::Result<T, E>;

/// A colour tag of a format section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatColor {
    Black,
    Blue,
    Cyan,
    Green,
    Magenta,
    Red,
    White,
    Yellow,
    /// `[ColorN]`, 1..=56: an entry of the workbook's indexed palette.
    Indexed(u8),
}

/// What a cell shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Formatted {
    pub text: String,
    /// The colour tag of the section that showed the value.
    pub color: Option<FormatColor>,
    /// A `*x` fill: the char repeats to fill the cell width at this byte offset of `text`
    /// (the char itself is not in `text`). A value the format can't show (a date past
    /// 9999-12-31) has empty text and fills the cell with `#`, as Excel shows it.
    pub fill: Option<(usize, char)>,
}

/// A compiled Excel number format code.
#[derive(Debug, Clone, PartialEq)]
pub struct NumberFormat {
    /// The sections for numbers, in order (at most 3).
    numeric: Vec<Section>,
    /// The section for text: the fourth, or a last section holding `@`.
    text: Option<Section>,
    /// When section 0 applies; `None` = always.
    first: Option<Cond>,
    /// When section 1 applies (if section 0 didn't); `None` = always.
    second: Option<Cond>,
    /// The first section is plain `General`, with no colour or condition.
    general: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct Section {
    color: Option<FormatColor>,
    body: Body,
}

#[derive(Debug, Clone, PartialEq)]
enum Body {
    /// An explicitly empty section: shows nothing.
    Empty,
    /// Literals around `General`.
    General(Vec<Item>),
    Number(Number),
    Scientific(Scientific),
    Fraction(Fraction),
    DateTime(DateTime),
    /// Literals around `@`.
    Text(Vec<Item>),
}

/// A digit placeholder: `0` shows a zero, `#` nothing, `?` a space where no digit is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ph {
    Zero,
    Hash,
    Space,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unit {
    Hour,
    Minute,
    Second,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AmPm {
    /// `AM/PM` in any case: `AM` or `PM`.
    Full,
    /// `A/P`: each letter in the case it is written.
    Short { a: char, p: char },
}

#[derive(Debug, Clone, PartialEq)]
enum Item {
    Lit(Box<str>),
    Fill(char),
    General,
    At,
    /// An integer digit placeholder (number, scientific mantissa, fraction integer part).
    Int(Ph),
    Point,
    Frac(Ph),
    /// `E+`, `E-`, `e+`, `e-`: the letter shown (`E`, except `e` for `e-`, as LibreOffice
    /// writes them) and whether a `+` shows for positive exponents.
    Exp {
        letter: char,
        plus: bool,
    },
    ExpDigit(Ph),
    /// A fraction's numerator, denominator (rendered whole at the first placeholder), and bar.
    Num,
    Den,
    Bar,
    /// `yy` (false) or `yyyy` (true).
    Year(bool),
    /// `m` 1, `mm` 2, `mmm` 3, `mmmm` 4, `mmmmm` 5 (the first letter).
    Month(u8),
    /// `d` 1, `dd` 2, `ddd` 3, `dddd` 4.
    Day(u8),
    /// One digit (false) or two (true).
    Hour(bool),
    Minute(bool),
    Second(bool),
    /// Fraction-of-second digits after `ss.`.
    SubSecond(u8),
    /// `[h]`, `[mm]`, `[ss]`…: the total count of the unit, at least this many digits.
    Elapsed(Unit, u8),
    AmPm(AmPm),
}

#[derive(Debug, Clone, PartialEq)]
struct Number {
    items: Vec<Item>,
    /// Integer placeholders.
    ints: usize,
    /// Fraction placeholders, in order.
    fracs: Vec<Ph>,
    /// `,` between integer placeholders: thousands separators.
    group: bool,
    /// `,` after the last placeholder: each divides by 1,000.
    thousands: u32,
    /// `%`: each multiplies by 100.
    percents: u32,
}

#[derive(Debug, Clone, PartialEq)]
struct Scientific {
    items: Vec<Item>,
    ints: usize,
    fracs: Vec<Ph>,
    exp_digits: Vec<Ph>,
    percents: u32,
}

#[derive(Debug, Clone, PartialEq)]
struct Fraction {
    items: Vec<Item>,
    ints: usize,
    nums: Vec<Ph>,
    /// The denominator's placeholders, or empty for a fixed denominator.
    dens: Vec<Ph>,
    fixed: Option<u32>,
    /// With an integer part, a zero fraction shows `blank` spaces in place of the items
    /// `blank_from..blank_to` (the gap, numerator, bar, and denominator).
    blank_from: usize,
    blank_to: usize,
    blank: usize,
    percents: u32,
}

#[derive(Debug, Clone, PartialEq)]
struct DateTime {
    items: Vec<Item>,
    /// The most fraction-of-second digits shown: times round to that precision.
    sub: u8,
    twelve: bool,
    /// Shows a time of day or an elapsed time; a date alone isn't rounded to the second.
    timed: bool,
    elapsed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Cond {
    op: Op,
    limit: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

impl Cond {
    fn holds(self, x: f64) -> bool {
        match self.op {
            Op::Lt => x < self.limit,
            Op::Le => x <= self.limit,
            Op::Gt => x > self.limit,
            Op::Ge => x >= self.limit,
            Op::Eq => x == self.limit,
            Op::Ne => x != self.limit,
        }
    }

    fn is(c: Option<Cond>, op: Op) -> bool {
        c.is_some_and(|c| c.op == op && c.limit == 0.0)
    }
}

/// A value the format can't show: a date or time out of range, or a number that scaling
/// (`%`) takes past the largest double.
pub(crate) struct OutOfRange;

impl NumberFormat {
    /// Compile an Excel number format code. `Err(ErrorKind::Value)` for a code Excel
    /// rejects.
    pub fn parse(code: &str) -> Result<NumberFormat> {
        parse::parse(code)
    }

    /// What a cell holding `v` shows under this format.
    pub fn display(&self, v: &Value) -> Formatted {
        let mut text = String::new();
        let shown = match v.scalar() {
            Value::Empty => Ok((None, None)),
            Value::Number(x) | Value::Date(x) if !x.is_finite() => {
                text.push_str(ErrorKind::Num.text());
                Ok((None, None))
            }
            Value::Number(x) | Value::Date(x) => self.number_into(*x, &mut text),
            Value::Text(t) => Ok(self.text_into(t, &mut text)),
            Value::Bool(b) => {
                text.push_str(if *b { "TRUE" } else { "FALSE" });
                Ok((None, None))
            }
            Value::Error(e) => {
                text.push_str(e.text());
                Ok((None, None))
            }
            Value::Array(_) => unreachable!("scalar() unwraps arrays"),
        };
        match shown {
            Ok((color, fill)) => Formatted { text, color, fill },
            Err(OutOfRange) => Formatted {
                text: String::new(),
                color: None,
                fill: Some((0, '#')),
            },
        }
    }

    /// True when the code is General (or only General in its first section).
    pub fn is_general(&self) -> bool {
        self.general
    }

    /// `TEXT(v, code)`'s text: the text [`display`](Self::display) shows, without the `*x`
    /// fill (LibreOffice's `TEXT` leaves it out too). A value the format can't show is
    /// `#VALUE!`.
    pub(crate) fn text_function(&self, v: &Value) -> Result<String> {
        match v {
            Value::Number(x) | Value::Date(x) if !x.is_finite() => Err(ErrorKind::Num),
            Value::Number(x) | Value::Date(x) => {
                let mut text = String::new();
                self.number_into(*x, &mut text)
                    .map_err(|OutOfRange| ErrorKind::Value)?;
                Ok(text)
            }
            v => Ok(self.display(v).text),
        }
    }

    /// The index of the section that shows `x` (it may not exist).
    fn select(&self, x: f64) -> usize {
        match (self.first, self.second) {
            (None, _) => 0,
            (Some(c), _) if c.holds(x) => 0,
            (_, None) => 1,
            (_, Some(c)) if c.holds(x) => 1,
            _ => 2,
        }
    }

    /// Whether section 0 is really for negative numbers (`[<0]`), so it shows no sign.
    fn first_is_negative(&self) -> bool {
        let limits = self.first.map_or(0.0, |c| c.limit) == 0.0
            && self.second.map_or(0.0, |c| c.limit) == 0.0;
        let (op1, op2) = (self.first.map(|c| c.op), self.second.map(|c| c.op));
        limits
            && match op1 {
                Some(Op::Lt) => matches!(op2, Some(Op::Gt | Op::Eq | Op::Ge) | None),
                Some(Op::Le) => matches!(op2, Some(Op::Gt) | None),
                _ => false,
            }
    }

    /// Whether section 1 is really for negative numbers, so it shows no sign.
    fn second_is_negative(&self) -> bool {
        let limits = self.first.map_or(0.0, |c| c.limit) == 0.0
            && self.second.map_or(0.0, |c| c.limit) == 0.0;
        let (op1, op2) = (self.first.map(|c| c.op), self.second.map(|c| c.op));
        limits
            && matches!(
                (op1, op2),
                (Some(Op::Ge), None) | (Some(Op::Gt), Some(Op::Lt)) | (None, None)
            )
    }

    /// Append how the number `x` shows; returns the section's colour and fill.
    fn number_into(&self, x: f64, out: &mut String) -> Shown {
        let idx = self.select(x);
        let mut x = x;
        if x < 0.0
            && ((idx == 0 && self.first_is_negative()) || (idx == 1 && self.second_is_negative()))
        {
            x = -x;
        }
        // Only the first section adds a minus sign.
        let neg = idx == 0 && x < 0.0;
        let Some(section) = self.numeric.get(idx) else {
            // A number without a section (a text-only code, or a conditional code without a
            // section for it) shows as General.
            render::general(&[Item::General], x.abs(), neg, out, &mut None);
            return Ok((None, None));
        };
        let mut fill = None;
        match &section.body {
            Body::Empty => {}
            Body::General(items) => render::general(items, x.abs(), neg, out, &mut fill),
            Body::Number(n) => render::number(n, x.abs(), neg, out, &mut fill)?,
            Body::Scientific(s) => render::scientific(s, x.abs(), neg, out, &mut fill)?,
            Body::Fraction(f) => render::fraction(f, x.abs(), neg, out, &mut fill)?,
            Body::DateTime(d) => render::date_time(d, x, out, &mut fill)?,
            Body::Text(_) => unreachable!("text sections aren't numeric"),
        }
        Ok((section.color, fill))
    }

    /// Append how the text `t` shows: through the text section, else unchanged.
    fn text_into(&self, t: &str, out: &mut String) -> (Option<FormatColor>, Option<(usize, char)>) {
        let Some(section) = &self.text else {
            out.push_str(t);
            return (None, None);
        };
        let mut fill = None;
        if let Body::Text(items) = &section.body {
            render::text(items, t, out, &mut fill);
        }
        (section.color, fill)
    }
}

type Shown = std::result::Result<(Option<FormatColor>, Option<(usize, char)>), OutOfRange>;

/// Excel's built-in format codes by numFmtId (those ECMA-376 fixes for every locale, in
/// en-US form); `None` for the others.
pub fn builtin_format(id: u32) -> Option<&'static str> {
    Some(match id {
        0 => "General",
        1 => "0",
        2 => "0.00",
        3 => "#,##0",
        4 => "#,##0.00",
        9 => "0%",
        10 => "0.00%",
        11 => "0.00E+00",
        12 => "# ?/?",
        13 => "# ??/??",
        14 => "m/d/yyyy",
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => "m/d/yyyy h:mm",
        37 => "#,##0_);(#,##0)",
        38 => "#,##0_);[Red](#,##0)",
        39 => "#,##0.00_);(#,##0.00)",
        40 => "#,##0.00_);[Red](#,##0.00)",
        45 => "mm:ss",
        46 => "[h]:mm:ss",
        47 => "mmss.0",
        48 => "##0.0E+0",
        49 => "@",
        _ => return None,
    })
}
