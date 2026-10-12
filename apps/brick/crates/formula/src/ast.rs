//! The parsed form of a formula.

/// One cell's address. Both coordinates are 0-based; `row` counts every file row, the
/// header included (ADR 0006), and `col` is a view column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellAddr {
    pub row: u64,
    pub col: u32,
    pub row_abs: bool,
    pub col_abs: bool,
}

/// The cells a reference covers. Ranges are stored with start ≤ end on both axes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Area {
    Cell(CellAddr),
    Range(CellAddr, CellAddr),
    /// Whole columns, `A:C`.
    Cols {
        first: u32,
        last: u32,
        first_abs: bool,
        last_abs: bool,
    },
    /// Whole rows, `1:3`.
    Rows {
        first: u64,
        last: u64,
        first_abs: bool,
        last_abs: bool,
    },
}

/// A reference, optionally on another sheet (`'Q1 Sales'!B2`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub sheet: Option<String>,
    pub area: Area,
}

/// The error values of the language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    Null,
    Div0,
    Value,
    Ref,
    Name,
    Num,
    NA,
    /// A formula that depends on itself (not an Excel literal; Excel shows 0 and warns).
    Circ,
}

impl ErrorKind {
    pub const ALL: [ErrorKind; 8] = [
        ErrorKind::Null,
        ErrorKind::Div0,
        ErrorKind::Value,
        ErrorKind::Ref,
        ErrorKind::Name,
        ErrorKind::Num,
        ErrorKind::NA,
        ErrorKind::Circ,
    ];

    pub fn text(self) -> &'static str {
        match self {
            ErrorKind::Null => "#NULL!",
            ErrorKind::Div0 => "#DIV/0!",
            ErrorKind::Value => "#VALUE!",
            ErrorKind::Ref => "#REF!",
            ErrorKind::Name => "#NAME?",
            ErrorKind::Num => "#NUM!",
            ErrorKind::NA => "#N/A",
            ErrorKind::Circ => "#CIRC!",
        }
    }

    /// The error whose text is exactly `s`, ignoring ASCII case.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|k| k.text().eq_ignore_ascii_case(s))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Plus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Concat,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl BinaryOp {
    pub fn symbol(self) -> &'static str {
        match self {
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Pow => "^",
            BinaryOp::Concat => "&",
            BinaryOp::Eq => "=",
            BinaryOp::Ne => "<>",
            BinaryOp::Lt => "<",
            BinaryOp::Le => "<=",
            BinaryOp::Gt => ">",
            BinaryOp::Ge => ">=",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number(f64),
    Text(String),
    Bool(bool),
    Error(ErrorKind),
    Ref(Reference),
    /// A defined name (`Rates`).
    Name(String),
    Unary(UnaryOp, Box<Expr>),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    /// Postfix `%`.
    Percent(Box<Expr>),
    /// The user's parentheses, kept so rendering reproduces them.
    Paren(Box<Expr>),
    /// A function call; the name is uppercase without `_xlfn.`-style prefixes.
    Call(String, Vec<Expr>),
    /// An array constant `{1,2;3,4}`: rows of literals.
    Array(Vec<Vec<Expr>>),
    /// An empty argument, as in `IF(A1,,2)`.
    Missing,
}

impl Expr {
    /// Calls `f` on this expression and every expression inside it, parents first.
    pub fn visit(&self, f: &mut dyn FnMut(&Expr)) {
        f(self);
        match self {
            Expr::Unary(_, x) | Expr::Percent(x) | Expr::Paren(x) => x.visit(f),
            Expr::Binary(_, l, r) => {
                l.visit(f);
                r.visit(f);
            }
            Expr::Call(_, args) => args.iter().for_each(|a| a.visit(f)),
            Expr::Array(rows) => rows.iter().flatten().for_each(|a| a.visit(f)),
            _ => {}
        }
    }

    /// Calls `f` on every expression inside this one and then on this one, children
    /// first, so `f` may replace the expression it is given.
    pub fn visit_mut(&mut self, f: &mut dyn FnMut(&mut Expr)) {
        match self {
            Expr::Unary(_, x) | Expr::Percent(x) | Expr::Paren(x) => x.visit_mut(f),
            Expr::Binary(_, l, r) => {
                l.visit_mut(f);
                r.visit_mut(f);
            }
            Expr::Call(_, args) => args.iter_mut().for_each(|a| a.visit_mut(f)),
            Expr::Array(rows) => rows.iter_mut().flatten().for_each(|a| a.visit_mut(f)),
            _ => {}
        }
        f(self);
    }
}
