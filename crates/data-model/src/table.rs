//! What the grid reads: display text for blocks of rows, behind [`TableSource`] so the grid
//! never touches the file (layers: source -> overlay -> view -> grid).

use csv_engine::{split_fields, Dialect, Field, RowIndex, SparseRowIndex};
use std::ops::Range;
use std::sync::Arc;

/// Longest cell text kept for display. Raw values are untouched; this only bounds the
/// grid's memory so it stays constant whatever the file holds.
pub const MAX_DISPLAY_BYTES: usize = 256;

const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// Display text of consecutive rows in flat, reusable buffers.
#[derive(Debug, Default)]
pub struct RowBlock {
    first_row: u64,
    text: String,
    /// End offset in `text` of each cell, all rows back to back.
    cell_ends: Vec<u32>,
    /// End index in `cell_ends` of each row.
    row_ends: Vec<u32>,
}

impl RowBlock {
    /// Empty the block, keeping its allocations, ready to receive rows from `first_row`.
    pub fn reset(&mut self, first_row: u64) {
        self.first_row = first_row;
        self.text.clear();
        self.cell_ends.clear();
        self.row_ends.clear();
    }

    /// Rows held, by row number.
    pub fn rows(&self) -> Range<u64> {
        self.first_row..self.first_row + self.row_ends.len() as u64
    }

    /// Append one cell to the row being built; finish the row with [`Self::end_row`].
    /// Invalid UTF-8 shows as U+FFFD; text past [`MAX_DISPLAY_BYTES`] is cut at a character
    /// boundary.
    pub fn push_cell(&mut self, cell: &[u8]) {
        let s = String::from_utf8_lossy(cell);
        let mut end = s.len().min(MAX_DISPLAY_BYTES);
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        self.text.push_str(&s[..end]);
        self.cell_ends.push(self.text.len() as u32);
    }

    pub fn end_row(&mut self) {
        self.row_ends.push(self.cell_ends.len() as u32);
    }

    fn cell_range(&self, row: u64) -> Option<Range<usize>> {
        let r = row.checked_sub(self.first_row)? as usize;
        let end = *self.row_ends.get(r)? as usize;
        let start = if r == 0 {
            0
        } else {
            self.row_ends[r - 1] as usize
        };
        Some(start..end)
    }

    /// Number of cells in `row` (rows can be ragged), or 0 if the row is not held.
    pub fn cells_in_row(&self, row: u64) -> u32 {
        self.cell_range(row).map_or(0, |r| r.len() as u32)
    }

    /// Display text of a cell; `None` if the row is not held or has no such column.
    pub fn cell(&self, row: u64, col: u32) -> Option<&str> {
        let cells = self.cell_range(row)?;
        let i = cells.start + col as usize;
        if i >= cells.end {
            return None;
        }
        let start = if i == 0 {
            0
        } else {
            self.cell_ends[i - 1] as usize
        };
        Some(&self.text[start..self.cell_ends[i] as usize])
    }

    /// Heap held by the block.
    pub fn heap_bytes(&self) -> usize {
        self.text.capacity() + self.cell_ends.capacity() * 4 + self.row_ends.capacity() * 4
    }
}

/// A table the grid can read. Row numbers are physical file rows until the row map
/// (EDIT-3) puts a logical order in front.
pub trait TableSource {
    /// Rows available now. Grows while background indexing runs.
    fn row_count(&self) -> u64;
    /// True once the whole file is indexed.
    fn is_complete(&self) -> bool;
    /// Load the available rows of `rows` into `out` (reset first).
    fn read_rows(&mut self, rows: Range<u64>, out: &mut RowBlock);
}

/// A CSV file read through its row index (ADR 0002).
pub struct CsvTable {
    index: Arc<SparseRowIndex>,
    dialect: Dialect,
    spans: Vec<Range<u64>>,
    fields: Vec<Field>,
}

impl CsvTable {
    pub fn new(index: Arc<SparseRowIndex>, dialect: Dialect) -> Self {
        Self {
            index,
            dialect,
            spans: Vec::new(),
            fields: Vec::new(),
        }
    }

    /// True once a read found the file truncated or replaced in place underneath us.
    pub fn file_changed(&self) -> bool {
        self.index.source().changed()
    }
}

impl TableSource for CsvTable {
    fn row_count(&self) -> u64 {
        self.index.row_count()
    }

    fn is_complete(&self) -> bool {
        self.index.is_complete()
    }

    fn read_rows(&mut self, rows: Range<u64>, out: &mut RowBlock) {
        out.reset(rows.start);
        if self.index.row_spans(rows, &mut self.spans).is_err() {
            return;
        }
        let bytes = self.index.source().bytes();
        for span in &self.spans {
            let mut row = &bytes[span.start as usize..span.end as usize];
            if span.start == 0 {
                row = row.strip_prefix(UTF8_BOM).unwrap_or(row);
            }
            split_fields(row, &self.dialect, &mut self.fields);
            for f in &self.fields {
                // `value` borrows unless the field has `""` escapes to undo.
                out.push_cell(&f.value(row, self.dialect.quote));
            }
            out.end_row();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_holds_ragged_rows_and_bounds_long_cells() {
        let mut b = RowBlock::default();
        b.reset(10);
        for cell in [&b"a"[..], b"\"q\""] {
            b.push_cell(cell);
        }
        b.end_row();
        let long = "é".repeat(MAX_DISPLAY_BYTES); // 2 bytes per char
        for cell in [long.as_bytes(), b"x", b"\xFF"] {
            b.push_cell(cell);
        }
        b.end_row();
        assert_eq!(b.rows(), 10..12);
        assert_eq!(b.cells_in_row(10), 2);
        assert_eq!(b.cell(10, 1), Some("\"q\""));
        assert_eq!(b.cell(10, 2), None);
        assert_eq!(b.cell(11, 0).unwrap().len(), MAX_DISPLAY_BYTES);
        assert_eq!(b.cell(11, 2), Some("\u{FFFD}"));
        assert_eq!(b.cell(9, 0), None);
        assert_eq!(b.cell(12, 0), None);
    }
}
