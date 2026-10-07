//! The tabular model: source data, edit overlay, inferred types, and views.
//!
//! Layers (spec section 16), each kept separate:
//! source (csv-engine) -> overlay (edits) -> view (sort/filter) -> grid.

mod save;
mod table;

pub use save::{SaveJob, SaveJobError, SaveStats};
pub use table::{CsvTable, DelimiterChoice, RereadError, RowBlock, TableSource, MAX_DISPLAY_BYTES};

use std::collections::{BTreeMap, HashMap};

pub type Row = u64;
pub type Col = u32;

/// Stable identity of a row (ADR 0003). Sorting, filtering, and inserts change where a
/// row shows, never its id, so edits stay attached to the right row. Source rows use their
/// physical row number; inserted rows (EDIT-3) will set the high bit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RowId(pub u64);

impl RowId {
    pub const fn source(row: Row) -> Self {
        Self(row)
    }
}

/// A cell address by row identity, not screen position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CellRef {
    pub row: RowId,
    pub col: Col,
}

/// A change to the table's data, as an operation (invariant 5). [`CsvTable::apply`] is the
/// only way to change data, and it returns the inverse: applying that undoes the change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// Give a cell this raw value, or with `None` drop its edit so the source shows again.
    Cell {
        at: CellRef,
        value: Option<Box<str>>,
    },
}

impl Edit {
    pub fn set(at: CellRef, value: impl Into<Box<str>>) -> Self {
        Self::Cell {
            at,
            value: Some(value.into()),
        }
    }

    pub fn clear(at: CellRef) -> Self {
        Self::Cell { at, value: None }
    }

    /// The cell the edit changes.
    pub fn cell(&self) -> CellRef {
        match self {
            Self::Cell { at, .. } => *at,
        }
    }

    /// Heap the edit holds beyond its own size: the text of the value it carries.
    pub fn heap_bytes(&self) -> usize {
        match self {
            Self::Cell { value, .. } => value.as_ref().map_or(0, |v| v.len()),
        }
    }
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

/// Sparse cell edits layered over the source file (spec section 17, ADR 0003). Values are
/// raw text exactly as entered. Grouped by row so reading a row costs one lookup, and
/// rows without edits cost nothing.
#[derive(Debug, Default)]
pub struct EditOverlay {
    rows: HashMap<RowId, BTreeMap<Col, Box<str>>>,
    cells: usize,
}

impl EditOverlay {
    /// Store an edit. Returns the previous overlay value, if any, for undo.
    pub fn set(&mut self, at: CellRef, raw: impl Into<Box<str>>) -> Option<Box<str>> {
        let prev = self
            .rows
            .entry(at.row)
            .or_default()
            .insert(at.col, raw.into());
        self.cells += usize::from(prev.is_none());
        prev
    }

    pub fn get(&self, at: CellRef) -> Option<&str> {
        self.rows.get(&at.row)?.get(&at.col).map(|s| &**s)
    }

    /// Remove an edit so the source value shows again. Returns the removed value.
    pub fn clear(&mut self, at: CellRef) -> Option<Box<str>> {
        let cols = self.rows.get_mut(&at.row)?;
        let prev = cols.remove(&at.col)?;
        if cols.is_empty() {
            self.rows.remove(&at.row);
        }
        self.cells -= 1;
        Some(prev)
    }

    /// Edits in one row, by column.
    pub fn row(&self, row: RowId) -> Option<&BTreeMap<Col, Box<str>>> {
        self.rows.get(&row)
    }

    /// Every edited row with its edits, in no particular order.
    pub fn iter_rows(&self) -> impl Iterator<Item = (&RowId, &BTreeMap<Col, Box<str>>)> {
        self.rows.iter()
    }

    /// Number of edited cells.
    pub fn len(&self) -> usize {
        self.cells
    }

    pub fn is_empty(&self) -> bool {
        self.cells == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_keeps_raw_text_exactly() {
        let mut o = EditOverlay::default();
        let at = CellRef {
            row: RowId::source(1_532_817),
            col: 3,
        };
        assert!(o.set(at, "00123").is_none());
        assert_eq!(o.get(at), Some("00123"));
        assert_eq!(o.set(at, "x").as_deref(), Some("00123"));
        assert_eq!(o.len(), 1);
    }

    #[test]
    fn overlay_groups_edits_by_row_in_column_order() {
        let mut o = EditOverlay::default();
        let r = RowId::source(7);
        o.set(CellRef { row: r, col: 5 }, "f");
        o.set(CellRef { row: r, col: 1 }, "b");
        o.set(
            CellRef {
                row: RowId::source(8),
                col: 0,
            },
            "other row",
        );
        let cols: Vec<(Col, &str)> = o.row(r).unwrap().iter().map(|(c, v)| (*c, &**v)).collect();
        assert_eq!(cols, [(1, "b"), (5, "f")]);
        assert_eq!(o.len(), 3);

        assert_eq!(o.clear(CellRef { row: r, col: 1 }).as_deref(), Some("b"));
        assert_eq!(o.clear(CellRef { row: r, col: 5 }).as_deref(), Some("f"));
        assert!(o.row(r).is_none(), "a row with no edits left is forgotten");
        assert_eq!(o.clear(CellRef { row: r, col: 5 }), None);
        assert_eq!(o.len(), 1);
    }
}
