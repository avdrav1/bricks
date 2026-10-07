//! The tabular model: source data, edit overlay, inferred types, and views.
//!
//! Layers (spec section 16), each kept separate:
//! source (csv-engine) -> overlay (edits) -> view (sort/filter) -> grid.

mod colmap;
mod rowmap;
mod save;
mod table;

pub use rowmap::Run;
pub use save::{SaveJob, SaveJobError, SaveStats};
pub use table::{CsvTable, DelimiterChoice, RereadError, RowBlock, TableSource, MAX_DISPLAY_BYTES};

use std::collections::{BTreeMap, HashMap};

pub type Row = u64;
pub type Col = u32;

/// Stable identity of a row (ADR 0003). Sorting, filtering, and inserts change where a
/// row shows, never its id, so edits stay attached to the right row. Source rows use their
/// physical row number; inserted rows (EDIT-3) have the high bit set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RowId(pub u64);

impl RowId {
    const INSERTED: u64 = 1 << 63;

    pub const fn source(row: Row) -> Self {
        Self(row)
    }

    /// The `n`th row inserted in this session.
    pub const fn inserted(n: u64) -> Self {
        Self(Self::INSERTED | n)
    }

    /// A row the user added: it has no bytes in the file, only edits.
    pub const fn is_inserted(self) -> bool {
        self.0 & Self::INSERTED != 0
    }
}

/// Stable identity of a column (EDIT-4, ADR 0003): its position in the file, or for an
/// inserted column a number with the high bit set. Edits key on it, so they stay with
/// their column when columns are inserted or deleted in front of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ColId(pub u32);

impl ColId {
    const INSERTED: u32 = 1 << 31;

    pub const fn source(col: Col) -> Self {
        Self(col)
    }

    /// The `n`th column inserted in this session.
    pub const fn inserted(n: u32) -> Self {
        Self(Self::INSERTED | n)
    }

    /// A column the user added: no field in the file, only edits.
    pub const fn is_inserted(self) -> bool {
        self.0 & Self::INSERTED != 0
    }
}

/// A cell address by row and column identity, not screen position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CellRef {
    pub row: RowId,
    pub col: ColId,
}

/// A change to the table's data, as an operation (invariant 5). [`CsvTable::apply`] is the
/// only way to change data, and it returns the inverse: applying that undoes the change.
///
/// Row positions here count every row in file order, the header row included, so an
/// edit means the same rows whether or not the first row is shown as a header. Build
/// row and column edits with [`CsvTable::insert_rows`], [`CsvTable::delete_rows`],
/// [`CsvTable::insert_cols`], and [`CsvTable::delete_cols`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// Give a cell this raw value, or with `None` drop its edit so the source shows again.
    Cell {
        at: CellRef,
        value: Option<Box<str>>,
    },
    /// Show these rows, in order, from position `at` on; later rows move down.
    InsertRows { at: u64, rows: Vec<Run> },
    /// Take `count` rows out from position `at`. Their ids, and so their edits, are kept
    /// in the inverse, so undo brings them back as they were.
    DeleteRows { at: u64, count: u64 },
    /// Show these columns, in order, from column `at` on; later columns move right.
    InsertCols { at: Col, cols: Vec<ColId> },
    /// Take `count` columns out from column `at`; the inverse keeps their ids.
    DeleteCols { at: Col, count: Col },
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

    /// An edit that changes nothing; a placeholder while a command swaps its edit.
    pub const fn none() -> Self {
        Self::DeleteRows { at: 0, count: 0 }
    }

    /// The cell a cell edit changes.
    pub fn cell(&self) -> Option<CellRef> {
        match self {
            Self::Cell { at, .. } => Some(*at),
            _ => None,
        }
    }

    /// Heap the edit holds beyond its own size.
    pub fn heap_bytes(&self) -> usize {
        match self {
            Self::Cell { value, .. } => value.as_ref().map_or(0, |v| v.len()),
            Self::InsertRows { rows, .. } => rows.capacity() * std::mem::size_of::<Run>(),
            Self::InsertCols { cols, .. } => cols.capacity() * std::mem::size_of::<ColId>(),
            Self::DeleteRows { .. } | Self::DeleteCols { .. } => 0,
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
    rows: HashMap<RowId, BTreeMap<ColId, Box<str>>>,
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

    /// Edits in one row, by column id.
    pub fn row(&self, row: RowId) -> Option<&BTreeMap<ColId, Box<str>>> {
        self.rows.get(&row)
    }

    /// Every edited row with its edits, in no particular order.
    pub fn iter_rows(&self) -> impl Iterator<Item = (&RowId, &BTreeMap<ColId, Box<str>>)> {
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
            col: ColId::source(3),
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
        o.set(
            CellRef {
                row: r,
                col: ColId::source(5),
            },
            "f",
        );
        o.set(
            CellRef {
                row: r,
                col: ColId::source(1),
            },
            "b",
        );
        o.set(
            CellRef {
                row: RowId::source(8),
                col: ColId::source(0),
            },
            "other row",
        );
        let cols: Vec<(ColId, &str)> = o.row(r).unwrap().iter().map(|(c, v)| (*c, &**v)).collect();
        assert_eq!(cols, [(ColId(1), "b"), (ColId(5), "f")]);
        assert_eq!(o.len(), 3);

        let (b, f) = (
            CellRef {
                row: r,
                col: ColId(1),
            },
            CellRef {
                row: r,
                col: ColId(5),
            },
        );
        assert_eq!(o.clear(b).as_deref(), Some("b"));
        assert_eq!(o.clear(f).as_deref(), Some("f"));
        assert!(o.row(r).is_none(), "a row with no edits left is forgotten");
        assert_eq!(o.clear(f), None);
        assert_eq!(o.len(), 1);
    }
}
