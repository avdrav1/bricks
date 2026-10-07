//! What the grid reads: display text for blocks of rows, behind [`TableSource`] so the grid
//! never touches the file (layers: source -> overlay -> view -> grid).

use crate::{CellRef, Col, Edit, EditOverlay, Row, RowId};
use csv_engine::{
    detect_dialect, detect_header, open_text, split_fields, Dialect, Encoding, Field, RowIndex,
    Source, SparseRowIndex, DETECT_SAMPLE_BYTES,
};
use std::borrow::Cow;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

/// How a file's delimiter is chosen: detected (ENG-4) or set by the user (ENG-5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelimiterChoice {
    Auto,
    Fixed(u8),
}

#[derive(Debug)]
pub enum RereadError {
    /// Edits are tied to columns a new delimiter would reshuffle; save them first.
    /// Holds the number of edited cells.
    UnsavedEdits(usize),
}

fn dialect_for(source: &Source, choice: DelimiterChoice) -> Dialect {
    let bytes = source.bytes();
    let sample = &bytes[..bytes.len().min(DETECT_SAMPLE_BYTES)];
    let detected = detect_dialect(sample);
    match choice {
        DelimiterChoice::Auto => detected,
        DelimiterChoice::Fixed(delimiter) => {
            let dialect = Dialect {
                delimiter,
                ..detected
            };
            Dialect {
                has_header: detect_header(sample, &dialect),
                ..dialect
            }
        }
    }
}

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
/// Text is always UTF-8 here; files in other encodings are decoded at open (ENG-6).
pub struct CsvTable {
    pub(crate) index: Arc<SparseRowIndex>,
    pub(crate) dialect: Dialect,
    pub(crate) encoding: Encoding,
    pub(crate) overlay: EditOverlay,
    spans: Vec<Range<u64>>,
    fields: Vec<Field>,
}

impl CsvTable {
    /// A table over an index of UTF-8 text.
    pub fn new(index: Arc<SparseRowIndex>, dialect: Dialect) -> Self {
        Self::with_encoding(index, dialect, Encoding::UTF8)
    }

    /// A table whose source was decoded from `encoding`; saves encode back to it.
    pub fn with_encoding(index: Arc<SparseRowIndex>, dialect: Dialect, encoding: Encoding) -> Self {
        Self {
            index,
            dialect,
            encoding,
            overlay: EditOverlay::default(),
            spans: Vec::new(),
            fields: Vec::new(),
        }
    }

    /// Map `path`, detect its encoding (decoding to UTF-8 if needed), and pick its dialect.
    /// Rows appear as the caller builds the index (`index().build`, normally on a worker).
    pub fn open(path: &Path, choice: DelimiterChoice) -> std::io::Result<Self> {
        let (source, encoding) = open_text(path)?;
        let dialect = dialect_for(&source, choice);
        Ok(Self::with_encoding(
            SparseRowIndex::new(Arc::new(source), &dialect),
            dialect,
            encoding,
        ))
    }

    /// The same open file read with another delimiter: a new, unbuilt index over the same
    /// mapping. Refused while there are unsaved edits.
    pub fn reread(&self, choice: DelimiterChoice) -> Result<Self, RereadError> {
        if !self.overlay.is_empty() {
            return Err(RereadError::UnsavedEdits(self.overlay.len()));
        }
        let source = self.index.source().clone();
        let dialect = dialect_for(&source, choice);
        Ok(Self::with_encoding(
            SparseRowIndex::new(source, &dialect),
            dialect,
            self.encoding,
        ))
    }

    /// The file's encoding on disk.
    pub fn encoding(&self) -> Encoding {
        self.encoding
    }

    pub fn index(&self) -> &Arc<SparseRowIndex> {
        &self.index
    }

    pub fn dialect(&self) -> &Dialect {
        &self.dialect
    }

    /// The first row of the file names the columns (ENG-7): it is not one of the table's
    /// rows but their titles ([`Self::header_cell`]). Detected on open; the user can flip it.
    pub fn has_header(&self) -> bool {
        self.dialect.has_header
    }

    /// Treat the first row as column titles, or as data. Nothing in the file changes, and
    /// edits stay with their file rows: only which row is shown where moves.
    pub fn set_header(&mut self, on: bool) {
        self.dialect.has_header = on;
    }

    /// File row of table row 0.
    fn first_row(&self) -> u64 {
        u64::from(self.dialect.has_header)
    }

    /// Title of column `col` when the first row is a header (with any edit to it).
    pub fn header_cell(&self, col: Col) -> Option<Cow<'_, str>> {
        if self.has_header() {
            self.cell_in_file_row(0, col)
        } else {
            None
        }
    }

    /// True once a read found the file truncated or replaced in place underneath us.
    pub fn file_changed(&self) -> bool {
        self.index.source().changed()
    }

    /// Identity of the row shown at `row`. Rows are in file order, after the header row if
    /// there is one, until the row map (EDIT-3) puts inserts, deletes, and sorting in front.
    pub fn row_id(&self, row: Row) -> RowId {
        RowId::source(row + self.first_row())
    }

    /// Change the data (invariant 5): the one mutating entry point, so every change is an
    /// operation that can be undone. Returns the inverse edit.
    ///
    /// ```
    /// # fn f(t: &mut data_model::CsvTable, at: data_model::CellRef) {
    /// let inverse = t.apply(data_model::Edit::set(at, "x"));
    /// t.apply(inverse); // back as it was
    /// # }
    /// ```
    ///
    /// There is no other way in: the table has no public setters.
    ///
    /// ```compile_fail
    /// # fn f(t: &mut data_model::CsvTable, at: data_model::CellRef) {
    /// t.set_cell(at, "bypasses undo");
    /// # }
    /// ```
    pub fn apply(&mut self, edit: Edit) -> Edit {
        match edit {
            Edit::Cell { at, value } => {
                let before = match value {
                    Some(v) => self.overlay.set(at, v),
                    None => self.overlay.clear(at),
                };
                Edit::Cell { at, value: before }
            }
        }
    }

    /// Where row `id` is shown, if it is one of the table's rows (the header row is not).
    pub fn row_of(&self, id: RowId) -> Option<Row> {
        let row = id.0.checked_sub(self.first_row())?;
        (row < self.row_count()).then_some(row)
    }

    pub fn overlay(&self) -> &EditOverlay {
        &self.overlay
    }

    /// Full text of one cell: the edit if there is one, else the source field. `None` when
    /// the row or the column does not exist. Invalid UTF-8 in the source shows as U+FFFD.
    pub fn cell_value(&self, row: Row, col: Col) -> Option<Cow<'_, str>> {
        self.cell_in_file_row(row + self.first_row(), col)
    }

    fn cell_in_file_row(&self, file_row: u64, col: Col) -> Option<Cow<'_, str>> {
        if let Some(v) = self.overlay.get(CellRef {
            row: RowId::source(file_row),
            col,
        }) {
            return Some(Cow::Borrowed(v));
        }
        let mut fields = Vec::new();
        let bytes = self.index.row_fields(file_row, &mut fields)?;
        let value = fields.get(col as usize)?.value(bytes, self.dialect.quote);
        Some(match value {
            Cow::Borrowed(b) => String::from_utf8_lossy(b),
            Cow::Owned(v) => Cow::Owned(String::from_utf8_lossy(&v).into_owned()),
        })
    }
}

impl TableSource for CsvTable {
    fn row_count(&self) -> u64 {
        self.index.row_count().saturating_sub(self.first_row())
    }

    fn is_complete(&self) -> bool {
        self.index.is_complete()
    }

    fn read_rows(&mut self, rows: Range<u64>, out: &mut RowBlock) {
        out.reset(rows.start);
        let first = self.first_row();
        let file_rows = rows.start + first..rows.end.saturating_add(first);
        if self
            .index
            .row_spans(file_rows.clone(), &mut self.spans)
            .is_err()
        {
            return;
        }
        let bytes = self.index.source().bytes();
        let quote = self.dialect.quote;
        for (file_row, span) in file_rows.zip(&self.spans) {
            let mut line = &bytes[span.start as usize..span.end as usize];
            if span.start == 0 {
                line = line.strip_prefix(UTF8_BOM).unwrap_or(line);
            }
            split_fields(line, &self.dialect, &mut self.fields);
            match self.overlay.row(RowId::source(file_row)) {
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
        assert_eq!(
            table.apply(Edit::set(at, "Smith, \"Jo\"")),
            Edit::clear(at),
            "the inverse of a first edit drops it"
        );
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
        table.apply(Edit::set(
            CellRef {
                row: table.row_id(2),
                col: 4,
            },
            "new",
        ));
        assert_eq!(row_text(&mut table, 2), ["2", "Lee", "7", "", "new"]);

        // Clearing the edit restores the source value.
        assert_eq!(
            table.apply(Edit::clear(at)),
            Edit::set(at, "Smith, \"Jo\""),
            "the inverse of a clear restores the edit"
        );
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

    fn build(table: &CsvTable) {
        table
            .index()
            .build(&std::sync::atomic::AtomicBool::new(false))
            .unwrap();
    }

    /// ENG-5: a delimiter override re-reads the same open file, no restart needed.
    #[test]
    fn override_rereads_the_open_file_with_another_delimiter() {
        let content = b"id;city;coords\n1;Paris;48.86, 2.29\n2;Lyon;45.76, 4.83\n";
        let path =
            std::env::temp_dir().join(format!("data-model-override-{}.csv", std::process::id()));
        std::fs::write(&path, content).unwrap();

        let mut auto = CsvTable::open(&path, DelimiterChoice::Auto).unwrap();
        build(&auto);
        assert_eq!(auto.dialect().delimiter, b';', "detected on open");
        assert_eq!(row_text(&mut auto, 0), ["1", "Paris", "48.86, 2.29"]);

        let mut comma = auto.reread(DelimiterChoice::Fixed(b',')).unwrap();
        assert_eq!(
            comma.row_count(),
            0,
            "the new reading starts unindexed; the caller builds it"
        );
        build(&comma);
        assert_eq!(comma.dialect().delimiter, b',');
        assert_eq!(row_text(&mut comma, 0), ["1;Paris;48.86", " 2.29"]);
        assert!(
            Arc::ptr_eq(comma.index().source(), auto.index().source()),
            "same mapping, no reopen"
        );

        let mut back = comma.reread(DelimiterChoice::Auto).unwrap();
        build(&back);
        assert_eq!(row_text(&mut back, 0), ["1", "Paris", "48.86, 2.29"]);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn override_waits_for_unsaved_edits() {
        let (path, mut table) = open(b"a,b\n1,2\n");
        table.apply(Edit::set(
            CellRef {
                row: table.row_id(1),
                col: 0,
            },
            "x",
        ));
        assert!(matches!(
            table.reread(DelimiterChoice::Fixed(b';')),
            Err(RereadError::UnsavedEdits(1))
        ));
        assert_eq!(
            table.cell_value(1, 0).as_deref(),
            Some("x"),
            "the edit is still there"
        );
        std::fs::remove_file(path).unwrap();
    }

    /// ENG-7 acceptance: the user can flip "first row is header". Flipping moves the first
    /// row between column titles and data; edits stay on their file rows; the file and a
    /// save keep every row.
    #[test]
    fn flipping_first_row_is_header() {
        let content = b"id,name\n1,Ann\n2,Bo\n";
        let path =
            std::env::temp_dir().join(format!("data-model-header-{}.csv", std::process::id()));
        std::fs::write(&path, content).unwrap();
        let mut t = CsvTable::open(&path, DelimiterChoice::Auto).unwrap();
        build(&t);

        assert!(t.has_header(), "detected: names over numbers");
        assert_eq!(t.row_count(), 2);
        assert_eq!(t.header_cell(1).as_deref(), Some("name"));
        assert_eq!(row_text(&mut t, 0), ["1", "Ann"]);
        assert_eq!(t.cell_value(1, 1).as_deref(), Some("Bo"));
        // An edit to table row 0 lands on file row 1.
        t.apply(Edit::set(
            CellRef {
                row: t.row_id(0),
                col: 1,
            },
            "Ana",
        ));

        t.set_header(false);
        assert_eq!(t.row_count(), 3);
        assert_eq!(t.header_cell(1), None);
        assert_eq!(row_text(&mut t, 0), ["id", "name"]);
        assert_eq!(
            row_text(&mut t, 1),
            ["1", "Ana"],
            "the edit stayed on its row"
        );
        // Renaming a column: edit the first row while it is data, then flip back.
        t.apply(Edit::set(
            CellRef {
                row: t.row_id(0),
                col: 1,
            },
            "who",
        ));

        t.set_header(true);
        assert_eq!(t.header_cell(1).as_deref(), Some("who"));
        assert_eq!(row_text(&mut t, 0), ["1", "Ana"]);
        assert_eq!(t.row_count(), 2);
        let mut saved = Vec::new();
        t.save_job().unwrap().write_to(&mut saved).unwrap();
        assert_eq!(
            saved, b"id,who\n1,Ana\n2,Bo\n",
            "the header row is saved as a row"
        );
        assert_eq!(std::fs::read(&path).unwrap(), content, "file untouched");
        std::fs::remove_file(path).unwrap();
    }
}
