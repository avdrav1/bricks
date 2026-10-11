//! Exact-match lookup keys and the index a [`crate::Context`] may build over a lookup
//! area, so that repeated exact lookups (`VLOOKUP(…, FALSE)`, `MATCH(…, 0)`, `XLOOKUP`)
//! into a large area cost a hash probe instead of a scan (FML-7).

use crate::value::Value;
use std::collections::HashMap;

/// What an exact lookup compares: numbers and dates by value (`-0` is `0`), text ignoring
/// case, booleans as themselves. Numbers never equal text. Empty cells and errors have no
/// key and match nothing.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    Number(u64),
    Text(Box<str>),
    Bool(bool),
}

impl Key {
    pub fn of(v: &Value) -> Option<Key> {
        match v {
            Value::Number(n) | Value::Date(n) => {
                Some(Key::Number(if *n == 0.0 { 0 } else { n.to_bits() }))
            }
            Value::Text(t) => Some(Key::Text(fold(t))),
            Value::Bool(b) => Some(Key::Bool(*b)),
            Value::Empty | Value::Error(_) | Value::Array(_) => None,
        }
    }
}

/// Lowercase text, as [`crate::coerce::compare_text`] compares it.
fn fold(t: &str) -> Box<str> {
    if t.is_ascii() {
        t.to_ascii_lowercase().into()
    } else {
        t.chars().flat_map(char::to_lowercase).collect()
    }
}

/// The first offset along a lookup vector at which each key occurs.
#[derive(Debug, Default)]
pub struct ExactIndex(HashMap<Key, u64>);

impl ExactIndex {
    /// Records `v` at `offset`, keeping the smallest offset of each key.
    pub fn insert(&mut self, v: &Value, offset: u64) {
        if let Some(key) = Key::of(v) {
            self.0
                .entry(key)
                .and_modify(|at| *at = (*at).min(offset))
                .or_insert(offset);
        }
    }

    /// The first offset holding a value equal to `v`.
    pub fn get(&self, v: &Value) -> Option<u64> {
        self.0.get(&Key::of(v)?).copied()
    }
}
