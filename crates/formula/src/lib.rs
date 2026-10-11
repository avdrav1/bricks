//! The formula language (ADR 0007): parsing, canonical rendering, A1 references and their
//! adjustment, and the plain scalar grammars (numbers, ISO dates) that type inference and
//! evaluation share. No dependencies, no clock, no I/O: callers supply cells through a
//! context.
//!
//! A formula's text is stored without its leading `=`: [`parse`] takes `SUM(A1:A3)`, and
//! [`render`] gives it back in canonical form.

pub mod ast;
pub mod coerce;
pub mod eval;
pub mod exact;
pub mod functions;
pub mod parse;
pub mod refs;
pub mod render;
pub mod scalar;
pub mod value;

pub use ast::{Area, BinaryOp, CellAddr, ErrorKind, Expr, Reference, UnaryOp};
pub use eval::{evaluate, unknown_functions, Context, Interrupt};
pub use exact::{ExactIndex, Key};
pub use parse::{parse, ParseError};
pub use refs::{
    adjust, column_index, column_name, format_reference, names_sheet, parse_reference, Axis, Change,
};
pub use render::{render, render_xlsx};
pub use value::{literal, result_text, Array, Value};
