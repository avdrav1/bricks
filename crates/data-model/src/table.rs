//! What the grid reads: display text for blocks of rows, behind [`TableSource`] so the grid
//! never touches the file (layers: source -> overlay -> view -> grid).

use crate::{CellRef, Col, EditOverlay, Row, RowId};
use csv_engine::{split_fields, Dialect, Field, RowIndex, SparseRowIndex};
use std::borrow::Cow;
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

/// A CSV file read through its row index (ADR 0002), with cell edits in an overlay on top
/// (ADR 0003). Edits never touch the file; they reach disk only through save (SAVE-1).
pub struct CsvTable {
    pub(crate) index: Arc<SparseRowIndex>,
    pub(crate) dialect: Dialect,
    pub(crate) overlay: EditOverlay,
    spans: Vec<Range<u64>>,
    fields: Vec<Field>,
}

impl CsvTable {
    pub fn new(index: Arc<SparseRowIndex>, dialect: Dialect) -> Self {
        Self {
            index,
            dialect,
            overlay: EditOverlay::default(),
            spans: Vec::new(),
            fields: Vec::new(),
        }
    }

    /// True once a read found the file truncated or replaced in place underneath us.
    pub fn file_changed(&self) -> bool {
        self.index.source().changed()
    }

    /// Identity of the row shown at `row`. Rows are in file order until the row map
    /// (EDIT-3) puts inserts, deletes, and sorting in front.
    pub fn row_id(&self, row: Row) -> RowId {
        RowId::source(row)
    }

    /// Set a cell's raw value. Returns the previous edit, if any (for undo).
    pub fn set_cell(&mut self, at: CellRef, raw: impl Into<Box<str>>) -> Option<Box<str>> {
        self.overlay.set(at, raw)
    }

    /// Drop a cell's edit so the source value shows again. Returns the removed edit.
    pub fn clear_cell(&mut self, at: CellRef) -> Option<Box<str>> {
        self.overlay.clear(at)
    }

    pub fn overlay(&self) -> &EditOverlay {
        &self.overlay
    }

    /// Full text of one cell: the edit if there is one, else the source field. `None` when
    /// the row or the column does not exist. Invalid UTF-8 in the source shows as U+FFFD.
    pub fn cell_value(&self, row: Row, col: Col) -> Option<Cow<'_, str>> {
        if let Some(v) = self.overlay.get(CellRef {
            row: self.row_id(row),
            col,
        }) {
            return Some(Cow::Borrowed(v));
        }
        let mut fields = Vec::new();
        let bytes = self.index.row_fields(row, &mut fields)?;
        let value = fields.get(col as usize)?.value(bytes, self.dialect.quote);
        Some(match value {
            Cow::Borrowed(b) => String::from_utf8_lossy(b),
            Cow::Owned(v) => Cow::Owned(String::from_utf8_lossy(&v).into_owned()),
        })
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
        if self.index.row_spans(rows.clone(), &mut self.spans).is_err() {
            return;
        }
        let bytes = self.index.source().bytes();
        let quote = self.dialect.quote;
        for (row, span) in (rows.start..).zip(&self.spans) {
            let mut line = &bytes[span.start as usize..span.end as usize];
            if span.start == 0 {
                line = line.strip_prefix(UTF8_BOM).unwrap_or(line);
            }
            split_fields(line, &self.dialect, &mut self.fields);
            match self.overlay.row(RowId::source(row)) {
                // `value` borrows unless the field has `""` escapes to undo.
                None => self
                    .fields
                    .iter()
                    .for_each(|f| out.push_cell(&f.value(line, quote))),
                Some(edits) => {
                    let last_edit = edits.keys().next_back().map_or(0, |&c| c as usize + 1);
                    for c in 0..self.fields.len().max(last_edit) {
                        match (edits.get(&(c as Col)), self.fields.get(c)) {
                            (Some(v), _) => out.push_cell(v.as_bytes()),
                            (None, Some(f)) => out.push_cell(&f.value(line, quote)),
                            (None, None) => out.push_cell(b""),
                        }
                    }
                }
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

    fn open(content: &[u8]) -> (std::path::PathBuf, CsvTable) {
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "data-model-test-{}-{}.csv",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, content).unwrap();
        let source = Arc::new(csv_engine::Source::open(&path).unwrap());
        let index = SparseRowIndex::new(source, &Dialect::default());
        index.build(&AtomicBool::new(false)).unwrap();
        (path, CsvTable::new(index, Dialect::default()))
    }

    fn row_text(table: &mut CsvTable, row: u64) -> Vec<String> {
        let mut b = RowBlock::default();
        table.read_rows(row..row + 1, &mut b);
        (0..b.cells_in_row(row))
            .map(|c| b.cell(row, c).unwrap().to_owned())
            .collect()
    }

    /// EDIT-1 acceptance: an edit shows on the next read and never touches the file.
    #[test]
    fn edit_is_visible_immediately_and_source_file_is_untouched() {
        let content = b"id,name,code\n1,\"Smith, J\",00123\n2,Lee,7\n";
        let (path, mut table) = open(content);
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(row_text(&mut table, 1), ["1", "Smith, J", "00123"]);

        let at = CellRef {
            row: table.row_id(1),
            col: 1,
        };
        assert_eq!(table.set_cell(at, "Smith, \"Jo\""), None);
        assert_eq!(row_text(&mut table, 1), ["1", "Smith, \"Jo\"", "00123"]);
        assert_eq!(table.cell_value(1, 1).as_deref(), Some("Smith, \"Jo\""));
        assert_eq!(
            table.cell_value(1, 2).as_deref(),
            Some("00123"),
            "untouched cells read from the source"
        );
        assert_eq!(
            row_text(&mut table, 2),
            ["2", "Lee", "7"],
            "other rows unaffected"
        );

        // Past the end of a short row: the row grows; the gap is empty.
        table.set_cell(
            CellRef {
                row: table.row_id(2),
                col: 4,
            },
            "new",
        );
        assert_eq!(row_text(&mut table, 2), ["2", "Lee", "7", "", "new"]);

        // Clearing the edit restores the source value.
        assert_eq!(table.clear_cell(at).as_deref(), Some("Smith, \"Jo\""));
        assert_eq!(row_text(&mut table, 1), ["1", "Smith, J", "00123"]);

        assert_eq!(
            std::fs::read(&path).unwrap(),
            content,
            "source bytes unchanged"
        );
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            modified,
            "source never written"
        );
        std::fs::remove_file(path).unwrap();
    }
}
