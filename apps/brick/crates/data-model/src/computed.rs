//! Formula cells and their results as jobs see them (ADR 0007).

use crate::{CellRef, ColId, RowId};
use std::collections::{BTreeMap, HashMap};

/// Result text of every formula cell of a sheet, set on a table snapshot so sort, filter,
/// find, type inference, and copy see results where the raw text is a formula. Read paths
/// that produce shown text consult it; raw paths (`cell_value`, save, journal) never do.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Computed {
    rows: HashMap<RowId, BTreeMap<ColId, Box<str>>>,
    cells: usize,
}

impl Computed {
    pub fn insert(&mut self, at: CellRef, text: impl Into<Box<str>>) {
        let prev = self
            .rows
            .entry(at.row)
            .or_default()
            .insert(at.col, text.into());
        self.cells += usize::from(prev.is_none());
    }

    pub fn get(&self, at: CellRef) -> Option<&str> {
        self.rows.get(&at.row)?.get(&at.col).map(|s| &**s)
    }

    /// The results in one row, by column id.
    pub fn row(&self, id: RowId) -> Option<&BTreeMap<ColId, Box<str>>> {
        self.rows.get(&id)
    }

    /// Every row with results, in no particular order.
    pub fn iter_rows(&self) -> impl Iterator<Item = (&RowId, &BTreeMap<ColId, Box<str>>)> {
        self.rows.iter()
    }

    /// Number of cells with a result.
    pub fn len(&self) -> usize {
        self.cells
    }

    pub fn is_empty(&self) -> bool {
        self.cells == 0
    }
}
