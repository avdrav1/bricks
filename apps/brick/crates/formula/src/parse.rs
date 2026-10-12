//! Formula text to [`Expr`]. Precedence, highest first: primary, postfix `%`, unary `-`/`+`
//! (so `-2^2` is 4, as in Excel), `^` (left-associative), `*` `/`, `+` `-`, `&`, then the
//! comparisons `= <> < <= > >=`.

use crate::ast::{Area, BinaryOp, CellAddr, ErrorKind, Expr, Reference, UnaryOp};
use std::fmt;

/// The largest row number a reference may name (1-based).
pub const MAX_ROW: u64 = 1 << 40;
/// Function name prefixes that XLSX files write for newer functions.
const PREFIXES: [&str; 3] = ["_XLFN.", "_XLWS.", "_XLUDF."];

/// Why a formula doesn't parse: `at` is the character index in the text given to
/// [`parse`] (without the leading `=`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub at: usize,
    pub message: &'static str,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at character {}", self.message, self.at + 1)
    }
}

impl std::error::Error for ParseError {}

/// Parses a formula given without its leading `=`.
pub fn parse(src: &str) -> Result<Expr, ParseError> {
    let mut p = Parser {
        s: src.chars().collect(),
        pos: 0,
    };
    let e = p.expr()?;
    p.ws();
    if p.pos < p.s.len() {
        return Err(p.unexpected());
    }
    Ok(e)
}

type Result<T, E = ParseError> = std::result::Result<T, E>;

struct Parser {
    s: Vec<char>,
    pos: usize,
}

fn is_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | '\n')
}

fn starts_operand(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '$' | '\'' | '(' | '"' | '#' | '{' | '.')
}

fn ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '.'
}

impl Parser {
    fn at(&self, i: usize) -> Option<char> {
        self.s.get(i).copied()
    }

    fn peek(&self) -> Option<char> {
        self.at(self.pos)
    }

    fn ws(&mut self) {
        while self.peek().is_some_and(is_ws) {
            self.pos += 1;
        }
    }

    fn fail<T>(&self, at: usize, message: &'static str) -> Result<T> {
        Err(ParseError { at, message })
    }

    /// The error for a character where an operator or a closing token was expected.
    fn unexpected(&self) -> ParseError {
        let message = match self.peek() {
            None => "unexpected end",
            Some(c) if self.pos > 0 && is_ws(self.s[self.pos - 1]) && starts_operand(c) => {
                "intersection isn't supported"
            }
            Some(_) => "unexpected character",
        };
        ParseError {
            at: self.pos,
            message,
        }
    }

    fn expr(&mut self) -> Result<Expr> {
        let mut l = self.concat()?;
        loop {
            self.ws();
            let (op, n) = match (self.peek(), self.at(self.pos + 1)) {
                (Some('<'), Some('>')) => (BinaryOp::Ne, 2),
                (Some('<'), Some('=')) => (BinaryOp::Le, 2),
                (Some('>'), Some('=')) => (BinaryOp::Ge, 2),
                (Some('<'), _) => (BinaryOp::Lt, 1),
                (Some('>'), _) => (BinaryOp::Gt, 1),
                (Some('='), _) => (BinaryOp::Eq, 1),
                _ => return Ok(l),
            };
            self.pos += n;
            let r = self.concat()?;
            l = Expr::Binary(op, Box::new(l), Box::new(r));
        }
    }

    fn binary_level(
        &mut self,
        ops: &[(char, BinaryOp)],
        next: fn(&mut Self) -> Result<Expr>,
    ) -> Result<Expr> {
        let mut l = next(self)?;
        loop {
            self.ws();
            let Some(&(_, op)) = ops.iter().find(|(c, _)| self.peek() == Some(*c)) else {
                return Ok(l);
            };
            self.pos += 1;
            let r = next(self)?;
            l = Expr::Binary(op, Box::new(l), Box::new(r));
        }
    }

    fn concat(&mut self) -> Result<Expr> {
        self.binary_level(&[('&', BinaryOp::Concat)], Self::additive)
    }

    fn additive(&mut self) -> Result<Expr> {
        self.binary_level(
            &[('+', BinaryOp::Add), ('-', BinaryOp::Sub)],
            Self::multiplicative,
        )
    }

    fn multiplicative(&mut self) -> Result<Expr> {
        self.binary_level(&[('*', BinaryOp::Mul), ('/', BinaryOp::Div)], Self::power)
    }

    fn power(&mut self) -> Result<Expr> {
        self.binary_level(&[('^', BinaryOp::Pow)], Self::unary)
    }

    fn unary(&mut self) -> Result<Expr> {
        self.ws();
        let op = match self.peek() {
            Some('-') => UnaryOp::Neg,
            Some('+') => UnaryOp::Plus,
            _ => return self.percent(),
        };
        self.pos += 1;
        Ok(Expr::Unary(op, Box::new(self.unary()?)))
    }

    fn percent(&mut self) -> Result<Expr> {
        let mut e = self.primary()?;
        loop {
            self.ws();
            if self.peek() != Some('%') {
                return Ok(e);
            }
            self.pos += 1;
            e = Expr::Percent(Box::new(e));
        }
    }

    fn primary(&mut self) -> Result<Expr> {
        self.ws();
        let start = self.pos;
        let Some(c) = self.peek() else {
            return self.fail(start, "unexpected end");
        };
        match c {
            '"' => self.string().map(Expr::Text),
            '{' => self.array(),
            '#' => self.error_literal().map(Expr::Error),
            '(' => {
                self.pos += 1;
                let e = self.expr()?;
                self.ws();
                match self.peek() {
                    Some(')') => {
                        self.pos += 1;
                        Ok(Expr::Paren(Box::new(e)))
                    }
                    None => self.fail(start, "unclosed parenthesis"),
                    Some(_) => Err(self.unexpected()),
                }
            }
            '0'..='9' | '.' => match self.ref_body()? {
                Some(area) => Ok(Expr::Ref(Reference { sheet: None, area })),
                None => self.number().map(Expr::Number),
            },
            '$' => match self.ref_body()? {
                Some(area) => Ok(Expr::Ref(Reference { sheet: None, area })),
                None => self.fail(start, "unexpected character"),
            },
            '\'' => {
                let name = self.quoted_sheet()?;
                self.sheet_ref(name)
            }
            c if c.is_ascii_alphabetic() || c == '_' => self.word(),
            ')' | ',' | ';' | '}' | '*' | '/' | '^' | '&' | '=' | '<' | '>' | '%' => {
                self.fail(start, "missing operand")
            }
            _ => self.fail(start, "unexpected character"),
        }
    }

    fn string(&mut self) -> Result<String> {
        let start = self.pos;
        self.pos += 1;
        let mut out = String::new();
        loop {
            match self.peek() {
                None => return self.fail(start, "unclosed string"),
                Some('"') if self.at(self.pos + 1) == Some('"') => {
                    out.push('"');
                    self.pos += 2;
                }
                Some('"') => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some(c) => {
                    out.push(c);
                    self.pos += 1;
                }
            }
        }
    }

    fn error_literal(&mut self) -> Result<ErrorKind> {
        for kind in ErrorKind::ALL {
            let text = kind.text();
            let n = text.chars().count();
            let Some(have) = self.s.get(self.pos..self.pos + n) else {
                continue;
            };
            if have
                .iter()
                .zip(text.chars())
                .all(|(a, b)| a.eq_ignore_ascii_case(&b))
            {
                self.pos += n;
                return Ok(kind);
            }
        }
        self.fail(self.pos, "unexpected character")
    }

    fn digits_end(&self, mut i: usize) -> usize {
        while self.at(i).is_some_and(|c| c.is_ascii_digit()) {
            i += 1;
        }
        i
    }

    fn number(&mut self) -> Result<f64> {
        let start = self.pos;
        let mut i = self.digits_end(start);
        let int = i > start;
        if self.at(i) == Some('.') {
            let end = self.digits_end(i + 1);
            if !int && end == i + 1 {
                return self.fail(start, "unexpected character");
            }
            i = end;
        }
        if matches!(self.at(i), Some('e' | 'E')) {
            let mut j = i + 1;
            if matches!(self.at(j), Some('+' | '-')) {
                j += 1;
            }
            let end = self.digits_end(j);
            if end > j {
                i = end;
            }
        }
        let text: String = self.s[start..i].iter().collect();
        self.pos = i;
        Ok(text.parse().expect("number grammar parses as f64"))
    }

    /// `[A-Za-z_][A-Za-z0-9_.]*` from the cursor.
    fn ident(&mut self) -> String {
        let start = self.pos;
        while self.peek().is_some_and(ident_char) {
            self.pos += 1;
        }
        self.s[start..self.pos].iter().collect()
    }

    fn word(&mut self) -> Result<Expr> {
        let start = self.pos;
        let ident = self.ident();
        match self.peek() {
            Some('(') => return self.call(ident),
            Some('!') => {
                self.pos += 1;
                return self.sheet_ref_after_bang(ident);
            }
            _ => {}
        }
        let end = self.pos;
        self.pos = start;
        if let Some(area) = self.ref_body()? {
            return Ok(Expr::Ref(Reference { sheet: None, area }));
        }
        self.pos = end;
        if ident.eq_ignore_ascii_case("TRUE") {
            Ok(Expr::Bool(true))
        } else if ident.eq_ignore_ascii_case("FALSE") {
            Ok(Expr::Bool(false))
        } else {
            Ok(Expr::Name(ident))
        }
    }

    fn call(&mut self, name: String) -> Result<Expr> {
        let open = self.pos;
        self.pos += 1;
        let mut name = name.to_ascii_uppercase();
        if let Some(p) = PREFIXES.iter().find(|p| name.starts_with(*p)) {
            name.drain(..p.len());
        }
        let mut args = Vec::new();
        self.ws();
        if self.peek() == Some(')') {
            self.pos += 1;
            return Ok(Expr::Call(name, args));
        }
        loop {
            self.ws();
            let arg = if matches!(self.peek(), Some(',' | ')')) {
                Expr::Missing
            } else {
                self.expr()?
            };
            args.push(arg);
            self.ws();
            match self.peek() {
                Some(',') => self.pos += 1,
                Some(')') => {
                    self.pos += 1;
                    return Ok(Expr::Call(name, args));
                }
                None => return self.fail(open, "unclosed parenthesis"),
                Some(_) => return Err(self.unexpected()),
            }
        }
    }

    fn quoted_sheet(&mut self) -> Result<String> {
        let start = self.pos;
        self.pos += 1;
        let mut out = String::new();
        loop {
            match self.peek() {
                None => return self.fail(start, "unclosed string"),
                Some('\'') if self.at(self.pos + 1) == Some('\'') => {
                    out.push('\'');
                    self.pos += 2;
                }
                Some('\'') => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some(c) => {
                    out.push(c);
                    self.pos += 1;
                }
            }
        }
    }

    fn sheet_ref(&mut self, name: String) -> Result<Expr> {
        if self.peek() != Some('!') {
            return Err(self.unexpected());
        }
        self.pos += 1;
        self.sheet_ref_after_bang(name)
    }

    fn sheet_ref_after_bang(&mut self, name: String) -> Result<Expr> {
        match self.ref_body()? {
            Some(area) => Ok(Expr::Ref(Reference {
                sheet: Some(name),
                area,
            })),
            None if self.peek().is_none() => self.fail(self.pos, "unexpected end"),
            None => self.fail(self.pos, "unexpected character"),
        }
    }

    /// Whether the text at `i` would continue an identifier, so a reference can't end
    /// there.
    fn continues(&self, i: usize) -> bool {
        self.at(i)
            .is_some_and(|c| ident_char(c) || c == '$' || c == '(')
    }

    /// `$?LETTERS` (1 to 3) at `i`: the column index, whether it is absolute, and the end.
    fn col_at(&self, i: usize) -> Option<(u32, bool, usize)> {
        let abs = self.at(i) == Some('$');
        let mut j = i + usize::from(abs);
        let start = j;
        while self.at(j).is_some_and(|c| c.is_ascii_alphabetic()) {
            j += 1;
        }
        if j == start || j - start > 3 {
            return None;
        }
        let letters: String = self.s[start..j].iter().collect();
        Some((crate::column_index(&letters)?, abs, j))
    }

    /// `$?DIGITS` (not starting with 0) at `i`: the 0-based row, whether it is absolute,
    /// and the end.
    fn row_at(&self, i: usize) -> Result<Option<(u64, bool, usize)>> {
        let abs = self.at(i) == Some('$');
        let start = i + usize::from(abs);
        if !self.at(start).is_some_and(|c| matches!(c, '1'..='9')) {
            return Ok(None);
        }
        let end = self.digits_end(start);
        let mut row: u64 = 0;
        for c in &self.s[start..end] {
            row = row
                .saturating_mul(10)
                .saturating_add(u64::from(*c as u8 - b'0'));
        }
        if row > MAX_ROW {
            return self.fail(start, "row out of range");
        }
        Ok(Some((row - 1, abs, end)))
    }

    /// Where `$?DIGITS` (not starting with 0) at `i` ends, if it is there.
    fn row_end(&self, i: usize) -> Option<usize> {
        let start = i + usize::from(self.at(i) == Some('$'));
        self.at(start)
            .is_some_and(|c| matches!(c, '1'..='9'))
            .then(|| self.digits_end(start))
    }

    fn cell_at(&self, i: usize) -> Result<Option<(CellAddr, usize)>> {
        let Some((col, col_abs, j)) = self.col_at(i) else {
            return Ok(None);
        };
        if self.at(j).is_some_and(|c| c.is_ascii_alphabetic()) {
            return Ok(None);
        }
        Ok(self.row_at(j)?.map(|(row, row_abs, end)| {
            (
                CellAddr {
                    row,
                    col,
                    row_abs,
                    col_abs,
                },
                end,
            )
        }))
    }

    /// A cell, a range, whole columns, or whole rows at the cursor; the cursor moves past
    /// it only if one is found.
    fn ref_body(&mut self) -> Result<Option<Area>> {
        let i = self.pos;
        if let Some((a, after)) = self.cell_at(i)? {
            if self.at(after) == Some(':') {
                if let Some((b, end)) = self.cell_at(after + 1)? {
                    if !self.continues(end) {
                        self.pos = end;
                        return Ok(Some(range(a, b)));
                    }
                }
            }
            if !self.continues(after) {
                self.pos = after;
                return Ok(Some(Area::Cell(a)));
            }
            return Ok(None);
        }
        if let Some((first, first_abs, after)) = self.col_at(i) {
            if self.at(after) == Some(':') {
                if let Some((last, last_abs, end)) = self.col_at(after + 1) {
                    if !self.continues(end) {
                        self.pos = end;
                        let (first, last, first_abs, last_abs) =
                            ordered(first, last, first_abs, last_abs);
                        return Ok(Some(Area::Cols {
                            first,
                            last,
                            first_abs,
                            last_abs,
                        }));
                    }
                }
            }
        }
        let is_rows = self
            .row_end(i)
            .is_some_and(|after| self.at(after) == Some(':') && self.row_end(after + 1).is_some());
        if !is_rows {
            return Ok(None);
        }
        if let Some((first, first_abs, after)) = self.row_at(i)? {
            if self.at(after) == Some(':') {
                if let Some((last, last_abs, end)) = self.row_at(after + 1)? {
                    if !self.continues(end) {
                        self.pos = end;
                        let (first, last, first_abs, last_abs) =
                            ordered(first, last, first_abs, last_abs);
                        return Ok(Some(Area::Rows {
                            first,
                            last,
                            first_abs,
                            last_abs,
                        }));
                    }
                }
            }
        }
        Ok(None)
    }

    fn array(&mut self) -> Result<Expr> {
        self.pos += 1;
        let mut rows: Vec<Vec<Expr>> = vec![Vec::new()];
        loop {
            let item = self.array_item()?;
            rows.last_mut().expect("a row").push(item);
            self.ws();
            match self.peek() {
                Some(',') => self.pos += 1,
                Some(c @ (';' | '}')) => {
                    let n = rows.last().expect("a row").len();
                    if rows.len() > 1 && n != rows[0].len() {
                        return self.fail(self.pos, "unexpected character");
                    }
                    self.pos += 1;
                    if c == '}' {
                        return Ok(Expr::Array(rows));
                    }
                    rows.push(Vec::new());
                }
                _ => return Err(self.unexpected()),
            }
        }
    }

    fn array_item(&mut self) -> Result<Expr> {
        self.ws();
        let start = self.pos;
        match self.peek() {
            None => self.fail(start, "unexpected end"),
            Some(',' | ';' | '}') => self.fail(start, "missing operand"),
            Some('"') => self.string().map(Expr::Text),
            Some('#') => self.error_literal().map(Expr::Error),
            Some(c @ ('-' | '+' | '0'..='9' | '.')) => {
                if matches!(c, '-' | '+') {
                    self.pos += 1;
                    if !self.peek().is_some_and(|d| d.is_ascii_digit() || d == '.') {
                        return self.fail(self.pos, "unexpected character");
                    }
                }
                let n = self.number()?;
                Ok(Expr::Number(if c == '-' { -n } else { n }))
            }
            Some(c) if c.is_ascii_alphabetic() => {
                let word = self.ident();
                if word.eq_ignore_ascii_case("TRUE") {
                    Ok(Expr::Bool(true))
                } else if word.eq_ignore_ascii_case("FALSE") {
                    Ok(Expr::Bool(false))
                } else {
                    self.fail(start, "unexpected character")
                }
            }
            Some(_) => self.fail(start, "unexpected character"),
        }
    }
}

fn ordered<T: PartialOrd>(a: T, b: T, a_abs: bool, b_abs: bool) -> (T, T, bool, bool) {
    if a <= b {
        (a, b, a_abs, b_abs)
    } else {
        (b, a, b_abs, a_abs)
    }
}

/// A range with its corners ordered on both axes, as Excel stores `B5:A1` (`A1:B5`).
fn range(a: CellAddr, b: CellAddr) -> Area {
    let (r0, r1, ra0, ra1) = ordered(a.row, b.row, a.row_abs, b.row_abs);
    let (c0, c1, ca0, ca1) = ordered(a.col, b.col, a.col_abs, b.col_abs);
    let start = CellAddr {
        row: r0,
        col: c0,
        row_abs: ra0,
        col_abs: ca0,
    };
    let end = CellAddr {
        row: r1,
        col: c1,
        row_abs: ra1,
        col_abs: ca1,
    };
    Area::Range(start, end)
}
