//! The tabular model: source data, edit overlay, inferred types, and views.
//!
//! Layers (spec section 16), each kept separate:
//! source (csv-engine) -> overlay (edits) -> view (sort/filter) -> grid.

use std::collections::HashMap;

pub type Row = u64;
pub type Col = u32;

/// A cell address in *physical* row space (before sort/filter).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CellRef {
    pub row: Row,
    pub col: Col,
}

/// Semantic type inferred for a column. Never changes the raw value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InferredType {
    Text,
    Integer,
    Decimal,
    Boolean,
    Date,
    DateTime,
}

/// Sparse cell edits layered over the source file (spec section 17).
///
/// Row and column inserts/deletes live in a logical-to-physical row map,
/// designed in DEC-3 and built in EDIT-3/EDIT-4.
#[derive(Debug, Default)]
pub struct EditOverlay {
    cells: HashMap<CellRef, Box<str>>,
}

impl EditOverlay {
    /// Store an edit. Returns the previous overlay value, if any, for undo.
    pub fn set(&mut self, at: CellRef, raw: impl Into<Box<str>>) -> Option<Box<str>> {
        self.cells.insert(at, raw.into())
    }

    pub fn get(&self, at: CellRef) -> Option<&str> {
        self.cells.get(&at).map(|s| &**s)
    }

    pub fn clear(&mut self, at: CellRef) -> Option<Box<str>> {
        self.cells.remove(&at)
    }

    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_keeps_raw_text_exactly() {
        let mut o = EditOverlay::default();
        let at = CellRef { row: 1_532_817, col: 3 };
        assert!(o.set(at, "00123").is_none());
        assert_eq!(o.get(at), Some("00123"));
        assert_eq!(o.set(at, "x").as_deref(), Some("00123"));
    }
}
