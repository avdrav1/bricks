//! What the grid reads: display text for blocks of rows, behind [`TableSource`] so the grid
//! never touches the file (layers: source -> overlay -> view -> grid).

use crate::cleared::Cleared;
use crate::colmap::ColMap;
use crate::filter::{Filter, RowSet};
use crate::rowmap::{RowMap, RowOrder, Run};
use crate::{CellRef, Col, ColId, ColSet, Edit, EditOverlay, InferredType, Row, RowId};
use csv_engine::{
    detect_dialect, detect_header, open_text, open_text_as, split_fields, Dialect, Encoding, Field,
    RowIndex, Source, SparseRowIndex, DETECT_SAMPLE_BYTES,
};
use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::{Bound, Range, RangeBounds};
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
    /// Keep whole cells, past [`MAX_DISPLAY_BYTES`] (copying, not display).
    full: bool,
}

impl RowBlock {
    /// A block that keeps every cell's whole text, for copying.
    pub fn full_text() -> Self {
        Self {
            full: true,
            ..Self::default()
        }
    }

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
    /// Invalid UTF-8 shows as U+FFFD; unless the block is [`Self::full_text`], text past
    /// [`MAX_DISPLAY_BYTES`] is cut at a character boundary.
    pub fn push_cell(&mut self, cell: &[u8]) {
        let s = String::from_utf8_lossy(cell);
        let mut end = if self.full {
            s.len()
        } else {
            s.len().min(MAX_DISPLAY_BYTES)
        };
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
    /// Row order once rows were inserted, deleted, or sorted (EDIT-3, SORT-1, ADR 0003);
    /// `None` while it is still file order.
    pub(crate) rows: Option<RowOrder>,
    /// Inserted rows so far: the next inserted row's number.
    next_inserted: u64,
    /// Column order once columns were inserted or deleted (EDIT-4); `None` while it is
    /// still file order.
    pub(crate) cols: Option<ColMap>,
    /// Inserted columns so far: the next inserted column's number.
    next_inserted_col: u32,
    /// Cells cleared in blocks (EDIT-5).
    pub(crate) cleared: Cleared,
    /// Inferred column types (TYPE-1), by column identity; empty until inferred.
    pub(crate) types: HashMap<ColId, InferredType>,
    /// Column filters (FILT-1) and the rows each passed.
    pub(crate) filters: Vec<(Filter, Arc<RowSet>)>,
    /// While filtered: positions of the rows shown, in order (the header excluded).
    /// Table rows count through it; `None` shows every row.
    pub(crate) visible: Option<Arc<Vec<u32>>>,
    spans: Vec<Range<u64>>,
    fields: Vec<Field>,
}

impl CsvTable {
    /// A table over an index of UTF-8 text, with a BOM if the text starts with one.
    pub fn new(index: Arc<SparseRowIndex>, dialect: Dialect) -> Self {
        let encoding = Encoding {
            bom: index.source().bytes().starts_with(UTF8_BOM),
            ..Encoding::UTF8
        };
        Self::with_encoding(index, dialect, encoding)
    }

    /// A table whose source was decoded from `encoding`; saves encode back to it.
    pub fn with_encoding(index: Arc<SparseRowIndex>, dialect: Dialect, encoding: Encoding) -> Self {
        Self {
            index,
            dialect,
            encoding,
            overlay: EditOverlay::default(),
            rows: None,
            next_inserted: 0,
            cols: None,
            next_inserted_col: 0,
            cleared: Cleared::default(),
            types: HashMap::new(),
            filters: Vec::new(),
            visible: None,
            spans: Vec::new(),
            fields: Vec::new(),
        }
    }

    /// The table as it is now, for a worker thread (CLIP-1 copies a selection from it
    /// while editing goes on): shares the file and its index, and copies the edits, the
    /// row and column order, and the clears. Costs what the edits hold, not the file.
    pub fn snapshot(&self) -> Self {
        Self {
            index: self.index.clone(),
            dialect: self.dialect,
            encoding: self.encoding,
            overlay: self.overlay.clone(),
            rows: self.rows.clone(),
            next_inserted: self.next_inserted,
            cols: self.cols.clone(),
            next_inserted_col: self.next_inserted_col,
            cleared: self.cleared.clone(),
            types: self.types.clone(),
            filters: self.filters.clone(),
            visible: self.visible.clone(),
            spans: Vec::new(),
            fields: Vec::new(),
        }
    }

    /// A new table with no file behind it (SAVE-2): no rows yet, comma-separated UTF-8,
    /// nothing to index. It gets a file through Save As.
    pub fn untitled() -> Self {
        let dialect = Dialect::default();
        let index = SparseRowIndex::new(Arc::new(Source::empty()), &dialect);
        index
            .build(&std::sync::atomic::AtomicBool::new(false))
            .expect("an empty source indexes at once");
        Self::new(index, dialect)
    }

    /// Open `path` as `encoding` rather than detecting it: the encoding a Save As wrote
    /// (a Windows-1252 file that happens to be plain ASCII would otherwise read as UTF-8).
    pub fn open_as(
        path: &Path,
        choice: DelimiterChoice,
        encoding: Encoding,
    ) -> std::io::Result<Self> {
        let source = open_text_as(path, encoding)?;
        let dialect = dialect_for(&source, choice);
        Ok(Self::with_encoding(
            SparseRowIndex::new(Arc::new(source), &dialect),
            dialect,
            encoding,
        ))
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
        let changes = self.changes();
        if changes > 0 {
            return Err(RereadError::UnsavedEdits(changes));
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
    /// edits stay with their file rows: only which row is shown where moves. A flip drops
    /// the inferred types: they were taken with the first row on the other side.
    pub fn set_header(&mut self, on: bool) {
        if on != self.dialect.has_header {
            self.types.clear();
        }
        self.dialect.has_header = on;
        self.update_view();
    }

    /// Position of table row 0 among all rows: past the header row, if there is one.
    pub(crate) fn first_row(&self) -> u64 {
        u64::from(self.dialect.has_header)
    }

    /// Rows in file order plus inserts minus deletes, the header row included.
    pub(crate) fn total_rows(&self) -> u64 {
        self.rows
            .as_ref()
            .map_or_else(|| self.index.row_count(), RowOrder::len)
    }

    /// Id of the row at position `k` among all rows (< `total_rows()`).
    fn id_at(&self, k: u64) -> RowId {
        self.rows.as_ref().map_or(RowId::source(k), |m| m.get(k))
    }

    /// Id of the column shown at position `col`.
    pub fn col_id(&self, col: Col) -> ColId {
        self.cols
            .as_ref()
            .map_or(ColId::source(col), |m| m.get(col))
    }

    /// Where column `id` is shown; `None` while it is deleted.
    pub fn col_of(&self, id: ColId) -> Option<Col> {
        match &self.cols {
            Some(m) => m.position(id),
            None => (!id.is_inserted()).then_some(id.0),
        }
    }

    /// An edit inserting `count` empty columns before column `col`.
    pub fn insert_cols(&mut self, col: Col, count: Col) -> Option<Edit> {
        if count == 0 {
            return None;
        }
        let first = self.next_inserted_col;
        self.next_inserted_col += count;
        Some(Edit::InsertCols {
            at: col,
            cols: (first..first + count).map(ColId::inserted).collect(),
        })
    }

    /// An edit deleting `count` columns from column `col` on. Columns past a row's end
    /// delete nothing in that row.
    pub fn delete_cols(&self, col: Col, count: Col) -> Option<Edit> {
        col.checked_add(count)?;
        (count > 0).then_some(Edit::DeleteCols { at: col, count })
    }

    /// An edit emptying table rows `rows` in columns `cols` (EDIT-5). An open end (`2..`)
    /// reaches every column, also those of wider rows further down. `None` when there are
    /// no such rows or columns.
    pub fn clear_cells(&self, rows: Range<Row>, cols: impl RangeBounds<Col>) -> Option<Edit> {
        if rows.is_empty() || rows.end > self.row_count() {
            return None;
        }
        let start = match cols.start_bound() {
            Bound::Included(&c) => c,
            Bound::Excluded(&c) => c.checked_add(1)?,
            Bound::Unbounded => 0,
        };
        let cols = match cols.end_bound() {
            Bound::Unbounded => match &self.cols {
                None => ColSet::new(Vec::new(), Some(start)),
                Some(map) => {
                    let (ids, rest) = map.from(start);
                    ColSet::new(ids, Some(rest))
                }
            },
            end => {
                let end = match end {
                    Bound::Included(&c) => c.checked_add(1)?,
                    Bound::Excluded(&c) => c,
                    Bound::Unbounded => unreachable!(),
                };
                if start >= end {
                    return None;
                }
                ColSet::new((start..end).map(|c| self.col_id(c)).collect(), None)
            }
        };
        let ranges = self.view_ranges(rows);
        let rows = match &self.rows {
            // A clear is a set of rows: shown in another order or with rows hidden
            // between, they go back to file order, so a whole column stays one run.
            Some(order) if self.visible.is_some() || matches!(order, RowOrder::Sorted(_)) => {
                merge_runs(ranges.into_iter().flat_map(|r| order.runs_in(r)).collect())
            }
            Some(order) => ranges.into_iter().flat_map(|r| order.runs_in(r)).collect(),
            None => merge_runs(
                ranges
                    .into_iter()
                    .map(|r| Run {
                        first: RowId::source(r.start),
                        len: r.end - r.start,
                    })
                    .collect(),
            ),
        };
        Some(Edit::ClearCells { rows, cols })
    }

    /// Title of column `col` when the first row is a header (with any edit to it).
    pub fn header_cell(&self, col: Col) -> Option<Cow<'_, str>> {
        if self.has_header() && self.total_rows() > 0 {
            self.cell_of(self.id_at(0), self.col_id(col))
        } else {
            None
        }
    }

    /// True once a read found the file truncated or replaced in place underneath us.
    pub fn file_changed(&self) -> bool {
        self.index.source().changed()
    }

    /// Identity of the row shown at `row` (< `row_count()`).
    pub fn row_id(&self, row: Row) -> RowId {
        self.id_at(self.pos(row))
    }

    /// Position among all rows (header and hidden rows included) of table row `row`.
    fn pos(&self, row: Row) -> u64 {
        match &self.visible {
            Some(v) => u64::from(v[row as usize]),
            None => row + self.first_row(),
        }
    }

    /// Positions of table rows `rows`, as ranges of consecutive positions.
    #[allow(clippy::single_range_in_vec_init)] // one range of positions, not a list of them
    fn view_ranges(&self, rows: Range<Row>) -> Vec<Range<u64>> {
        match &self.visible {
            None => vec![rows.start + self.first_row()..rows.end.saturating_add(self.first_row())],
            Some(v) => {
                let end = (rows.end as usize).min(v.len());
                let mut out: Vec<Range<u64>> = Vec::new();
                for &p in &v[(rows.start as usize).min(end)..end] {
                    let p = u64::from(p);
                    match out.last_mut() {
                        Some(r) if r.end == p => r.end += 1,
                        _ => out.push(p..p + 1),
                    }
                }
                out
            }
        }
    }

    /// Unsaved changes: edited cells, plus rows inserted and source rows deleted, plus one
    /// for a sorted row order (SORT-1).
    pub fn changes(&self) -> usize {
        let rows = self.rows.as_ref().map_or(0, |m| {
            let (mut inserted, mut kept) = (0, 0);
            m.for_each(|run| {
                if run.first.is_inserted() {
                    inserted += run.len;
                } else {
                    kept += run.len;
                }
            });
            inserted + self.index.row_count().saturating_sub(kept)
        });
        let sorted = usize::from(matches!(self.rows, Some(RowOrder::Sorted(_))));
        let cols = self.cols.as_ref().map_or(0, ColMap::changes);
        self.overlay.len() + rows as usize + sorted + cols + self.cleared.len()
    }

    /// An edit inserting `count` empty rows before table row `row` (at the end when
    /// `row == row_count()`). `None` until the whole file is indexed (the row order is
    /// fixed only then), and while filtered (FILT-1).
    pub fn insert_rows(&mut self, row: Row, count: u64) -> Option<Edit> {
        if !self.index.is_complete() || self.is_filtered() || count == 0 || row > self.row_count() {
            return None;
        }
        let first = RowId::inserted(self.next_inserted);
        self.next_inserted += count;
        Some(Edit::InsertRows {
            at: row + self.first_row(),
            rows: vec![Run { first, len: count }],
        })
    }

    /// An edit deleting `count` table rows from `row` on. `None` until the whole file is
    /// indexed, while filtered (FILT-1), or when the rows don't exist.
    pub fn delete_rows(&self, row: Row, count: u64) -> Option<Edit> {
        let end = row.checked_add(count)?;
        if !self.index.is_complete() || self.is_filtered() || count == 0 || end > self.row_count() {
            return None;
        }
        Some(Edit::DeleteRows {
            at: row + self.first_row(),
            count,
        })
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
        let inverse = self.apply_edit(edit);
        // Rows moved, came, or went: the filtered view follows (FILT-1).
        if self.is_filtered()
            && matches!(
                inverse,
                Edit::InsertRows { .. } | Edit::DeleteRows { .. } | Edit::Reorder(_)
            )
        {
            self.update_view();
        }
        inverse
    }

    fn apply_edit(&mut self, edit: Edit) -> Edit {
        match edit {
            Edit::Cell { at, value } => {
                let before = match value {
                    Some(v) => self.overlay.set(at, v),
                    None => self.overlay.clear(at),
                };
                Edit::Cell { at, value: before }
            }
            Edit::InsertRows { at, rows } => {
                let count = rows.iter().map(|r| r.len).sum();
                self.row_map().insert(at, &rows);
                Edit::DeleteRows { at, count }
            }
            Edit::DeleteRows { at, count: 0 } => Edit::DeleteRows { at, count: 0 },
            Edit::DeleteRows { at, count } => Edit::InsertRows {
                at,
                rows: self.row_map().remove(at, count),
            },
            Edit::InsertCols { at, cols } => {
                let count = cols.len() as Col;
                self.cols
                    .get_or_insert_with(ColMap::default)
                    .insert(at, &cols);
                Edit::DeleteCols { at, count }
            }
            Edit::DeleteCols { at, count } => Edit::InsertCols {
                at,
                cols: self
                    .cols
                    .get_or_insert_with(ColMap::default)
                    .remove(at, count),
            },
            Edit::ClearCells { rows, cols } => {
                let ids = id_ranges(&rows);
                let edits = self.overlay.take_in(&ids, &cols);
                self.cleared.push(file_rows(ids), cols.file_columns());
                Edit::UnclearCells { rows, cols, edits }
            }
            Edit::UnclearCells { rows, cols, edits } => {
                self.cleared
                    .remove(file_rows(id_ranges(&rows)), cols.file_columns());
                for (at, value) in edits {
                    self.overlay.set(at, value);
                }
                Edit::ClearCells { rows, cols }
            }
            Edit::Cells(cells) => {
                let mut before: Vec<_> = cells
                    .into_iter()
                    .map(|(at, value)| {
                        let prev = match value {
                            Some(v) => self.overlay.set(at, v),
                            None => self.overlay.clear(at),
                        };
                        (at, prev)
                    })
                    .collect();
                // Undone last-first, so a cell listed twice gets its first value back.
                before.reverse();
                Edit::Cells(before)
            }
            Edit::Reorder(order) => {
                debug_assert_eq!(
                    order.as_ref().map_or(self.index.row_count(), RowOrder::len),
                    self.total_rows(),
                    "a reorder keeps every row"
                );
                Edit::Reorder(std::mem::replace(&mut self.rows, order))
            }
        }
    }

    /// The row order, made from file order on the first insert or delete.
    fn row_map(&mut self) -> &mut RowOrder {
        // A map made before indexing ends would freeze the row count and drop the rest
        // of the file on save; `insert_rows`/`delete_rows` return `None` until then.
        assert!(
            self.rows.is_some() || self.index.is_complete(),
            "row inserts and deletes need the whole file indexed"
        );
        let rows = self.index.row_count();
        self.rows.get_or_insert_with(|| {
            RowOrder::Runs(RowMap::new(Run {
                first: RowId::source(0),
                len: rows,
            }))
        })
    }

    /// Where row `id` is shown, if it is one of the table's rows (not the header row, nor
    /// a row a filter hides).
    pub fn row_of(&self, id: RowId) -> Option<Row> {
        let k = match &self.rows {
            Some(m) => m.position(id)?,
            None => id.0,
        };
        let row = match &self.visible {
            Some(v) => v.binary_search(&u32::try_from(k).ok()?).ok()? as u64,
            None => k.checked_sub(self.first_row())?,
        };
        (row < self.row_count()).then_some(row)
    }

    pub fn overlay(&self) -> &EditOverlay {
        &self.overlay
    }

    /// Full text of one cell: the edit if there is one, else the source field. `None` when
    /// the row or the column does not exist. Invalid UTF-8 in the source shows as U+FFFD.
    pub fn cell_value(&self, row: Row, col: Col) -> Option<Cow<'_, str>> {
        if row >= self.row_count() {
            return None;
        }
        self.cell_of(self.id_at(self.pos(row)), self.col_id(col))
    }

    pub(crate) fn cell_of(&self, row: RowId, col: ColId) -> Option<Cow<'_, str>> {
        if let Some(v) = self.overlay.get(CellRef { row, col }) {
            return Some(Cow::Borrowed(v));
        }
        if col.is_inserted() {
            // An inserted column is in every row, empty until edited.
            return Some(Cow::Borrowed(""));
        }
        if row.is_inserted() {
            return None;
        }
        let mut fields = Vec::new();
        let bytes = self.index.row_fields(row.0, &mut fields)?;
        let field = fields.get(col.0 as usize)?;
        if self.cleared.at(row.0).is_some_and(|c| c.contains(col)) {
            return Some(Cow::Borrowed(""));
        }
        let value = field.value(bytes, self.dialect.quote);
        Some(match value {
            Cow::Borrowed(b) => String::from_utf8_lossy(b),
            Cow::Owned(v) => Cow::Owned(String::from_utf8_lossy(&v).into_owned()),
        })
    }

    /// Append source rows `file_rows` (with their edits) to `out`.
    fn read_source(&mut self, file_rows: Range<u64>, out: &mut RowBlock, cols: Option<&ColMap>) {
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
            let edits = self.overlay.row(RowId::source(file_row));
            let cleared = self.cleared.at(file_row);
            // `value` borrows unless the field has `""` escapes to undo.
            let value = |id: ColId, f: &Field| match cleared {
                Some(c) if c.contains(id) => Cow::Borrowed(&b""[..]),
                _ => f.value(line, quote),
            };
            match (cols, edits) {
                (None, None) if cleared.is_none() => self
                    .fields
                    .iter()
                    .for_each(|f| out.push_cell(&f.value(line, quote))),
                (None, edits) => {
                    let last_edit = edits
                        .and_then(|e| e.keys().next_back())
                        .map_or(0, |c| c.0 as usize + 1);
                    for c in 0..self.fields.len().max(last_edit) {
                        let id = ColId::source(c as Col);
                        match (edits.and_then(|e| e.get(&id)), self.fields.get(c)) {
                            (Some(v), _) => out.push_cell(v.as_bytes()),
                            (None, Some(f)) => out.push_cell(&value(id, f)),
                            (None, None) => out.push_cell(b""),
                        }
                    }
                }
                (Some(map), edits) => {
                    let nf = self.fields.len() as Col;
                    let width = match edits {
                        Some(e) => map.width_with(nf, e.keys().copied()),
                        None => map.width(nf, |_| false),
                    };
                    for pos in 0..width {
                        let id = map.get(pos);
                        let field = (!id.is_inserted())
                            .then(|| self.fields.get(id.0 as usize))
                            .flatten();
                        match (edits.and_then(|e| e.get(&id)), field) {
                            (Some(v), _) => out.push_cell(v.as_bytes()),
                            (None, Some(f)) => out.push_cell(&value(id, f)),
                            (None, None) => out.push_cell(b""),
                        }
                    }
                }
            }
            out.end_row();
        }
    }

    /// Append inserted rows (only their edits; the rest is empty) to `out`.
    fn read_inserted(&self, run: Run, out: &mut RowBlock, cols: Option<&ColMap>) {
        for id in run.first.0..run.first.0 + run.len {
            if let Some(edits) = self.overlay.row(RowId(id)) {
                match cols {
                    None => {
                        let width = edits.keys().next_back().map_or(0, |c| c.0 + 1);
                        for c in 0..width {
                            let v = edits.get(&ColId::source(c));
                            out.push_cell(v.map_or(&b""[..], |v| v.as_bytes()));
                        }
                    }
                    Some(map) => {
                        for pos in 0..map.width_with(0, edits.keys().copied()) {
                            let v = edits.get(&map.get(pos));
                            out.push_cell(v.map_or(&b""[..], |v| v.as_bytes()));
                        }
                    }
                }
            }
            out.end_row();
        }
    }
}

/// The row ids of `runs` as sorted, merged ranges of id values.
fn id_ranges(runs: &[Run]) -> Vec<Range<u64>> {
    let mut ids: Vec<Range<u64>> = runs.iter().map(|r| r.first.0..r.first.0 + r.len).collect();
    ids.sort_unstable_by_key(|r| r.start);
    let mut merged: Vec<Range<u64>> = Vec::with_capacity(ids.len());
    for r in ids {
        match merged.last_mut() {
            Some(last) if last.end >= r.start => last.end = last.end.max(r.end),
            _ => merged.push(r),
        }
    }
    merged
}

/// The file rows among merged id ranges (inserted rows have nothing in the file).
fn file_rows(mut ids: Vec<Range<u64>>) -> Vec<Range<u64>> {
    ids.retain(|r| !RowId(r.start).is_inserted());
    ids
}

/// Runs of file rows in file order, adjacent ones joined (inserted rows after them).
fn merge_runs(mut runs: Vec<Run>) -> Vec<Run> {
    runs.sort_unstable_by_key(|r| r.first);
    let mut out: Vec<Run> = Vec::with_capacity(runs.len());
    for r in runs {
        match out.last_mut() {
            Some(last) if last.first.0 + last.len == r.first.0 => last.len += r.len,
            _ => out.push(r),
        }
    }
    out
}
impl TableSource for CsvTable {
    fn row_count(&self) -> u64 {
        match &self.visible {
            Some(v) => v.len() as u64,
            None => self.unfiltered_row_count(),
        }
    }

    fn is_complete(&self) -> bool {
        self.index.is_complete()
    }

    fn read_rows(&mut self, rows: Range<u64>, out: &mut RowBlock) {
        out.reset(rows.start);
        for positions in self.view_ranges(rows) {
            self.read_positions(positions, out);
        }
    }
}

impl CsvTable {
    /// Append the rows at `positions` (among all rows, header and hidden ones included).
    fn read_positions(&mut self, positions: Range<u64>, out: &mut RowBlock) {
        // Out of `self` while reading, so the read paths can use the field buffer.
        let cols = self.cols.take();
        match self.rows.as_ref().map(|m| {
            let end = positions.end.min(m.len());
            m.runs_in(positions.start..end)
        }) {
            // File order: positions are file rows.
            None => self.read_source(positions, out, cols.as_ref()),
            Some(runs) => {
                for run in runs {
                    if run.first.is_inserted() {
                        self.read_inserted(run, out, cols.as_ref());
                    } else {
                        let file_rows = run.first.0..run.first.0 + run.len;
                        self.read_source(file_rows, out, cols.as_ref());
                    }
                }
            }
        }
        self.cols = cols;
    }

    /// Table rows `rows` as if no filter were on (type inference samples them all).
    pub(crate) fn read_unfiltered(&mut self, rows: Range<Row>, out: &mut RowBlock) {
        out.reset(rows.start);
        let first = self.first_row();
        self.read_positions(rows.start + first..rows.end.saturating_add(first), out);
    }

    /// The text of file row `row` (at `span`) in column `col`, edits and clears included.
    pub(crate) fn file_cell<'a>(
        &'a self,
        row: u64,
        span: &Range<u64>,
        col: ColId,
        fields: &mut Vec<Field>,
    ) -> Cow<'a, str> {
        if let Some(v) = self.overlay.get(CellRef {
            row: RowId::source(row),
            col,
        }) {
            return Cow::Borrowed(v);
        }
        if col.is_inserted() || self.cleared.at(row).is_some_and(|c| c.contains(col)) {
            return Cow::Borrowed("");
        }
        let bytes = self.index.source().bytes();
        let mut line = &bytes[span.start as usize..span.end as usize];
        if span.start == 0 {
            line = line.strip_prefix(UTF8_BOM).unwrap_or(line);
        }
        split_fields(line, &self.dialect, fields);
        match fields
            .get(col.0 as usize)
            .map(|f| f.value(line, self.dialect.quote))
        {
            None => Cow::Borrowed(""),
            Some(Cow::Borrowed(b)) => String::from_utf8_lossy(b),
            Some(Cow::Owned(v)) => Cow::Owned(String::from_utf8_lossy(&v).into_owned()),
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
            col: ColId::source(1),
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
                col: ColId::source(4),
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
                col: ColId::source(0),
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
                col: ColId::source(1),
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
                col: ColId::source(1),
            },
            "who",
        ));

        t.set_header(true);
        assert_eq!(t.header_cell(1).as_deref(), Some("who"));
        assert_eq!(row_text(&mut t, 0), ["1", "Ana"]);
        assert_eq!(t.row_count(), 2);
        let mut saved = Vec::new();
        t.save_job()
            .unwrap()
            .write_to(
                &mut saved,
                &crate::SaveProgress::default(),
                &std::sync::atomic::AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(
            saved, b"id,who\n1,Ana\n2,Bo\n",
            "the header row is saved as a row"
        );
        assert_eq!(std::fs::read(&path).unwrap(), content, "file untouched");
        std::fs::remove_file(path).unwrap();
    }

    fn opened(name: &str, content: &[u8]) -> (std::path::PathBuf, CsvTable) {
        let path =
            std::env::temp_dir().join(format!("data-model-{name}-{}.csv", std::process::id()));
        std::fs::write(&path, content).unwrap();
        let t = CsvTable::open(&path, DelimiterChoice::Auto).unwrap();
        build(&t);
        (path, t)
    }

    fn saved(t: &CsvTable) -> Vec<u8> {
        let mut out = Vec::new();
        let stats = t
            .save_job()
            .unwrap()
            .write_to(
                &mut out,
                &crate::SaveProgress::default(),
                &std::sync::atomic::AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(stats.rows, t.row_count() + u64::from(t.has_header()));
        out
    }

    /// EDIT-3: inserted rows are written where they are shown, deleted rows are left out,
    /// every other row keeps its bytes; the BOM stays first and a last row that had no
    /// line ending gets one when rows follow it. Undoing everything gives back the file.
    #[test]
    fn inserts_and_deletes_save_in_place_and_undo_to_the_original_bytes() {
        let content = "\u{feff}id,name\r\n1,Ann\r\n2,Bo".as_bytes();
        let (path, mut t) = opened("rows", content);
        assert!(t.has_header());
        let mut undo = Vec::new();

        let insert = t.insert_rows(0, 1).unwrap(); // above the first data row
        undo.push(t.apply(insert));
        let new = t.row_id(0);
        assert!(new.is_inserted());
        undo.push(t.apply(Edit::set(
            CellRef {
                row: new,
                col: ColId::source(0),
            },
            "0",
        )));
        undo.push(t.apply(Edit::set(
            CellRef {
                row: new,
                col: ColId::source(1),
            },
            "Zoë, \"q\"",
        )));
        let at_end = t.insert_rows(t.row_count(), 1).unwrap();
        undo.push(t.apply(at_end));
        let ann = t.delete_rows(1, 1).unwrap();
        undo.push(t.apply(ann));

        assert_eq!(t.row_count(), 3);
        assert_eq!(row_text(&mut t, 0), ["0", "Zoë, \"q\""]);
        assert_eq!(row_text(&mut t, 1), ["2", "Bo"]);
        assert_eq!(
            row_text(&mut t, 2),
            Vec::<String>::new(),
            "empty inserted row"
        );
        assert_eq!(t.cell_value(0, 1).as_deref(), Some("Zoë, \"q\""));
        assert_eq!(t.cell_value(2, 0), None);
        assert_eq!(t.row_of(new), Some(0));
        assert_eq!(
            t.changes(),
            2 + 2 + 1,
            "two cells, two rows in, one row out"
        );
        assert_eq!(
            saved(&t),
            "\u{feff}id,name\r\n0,\"Zoë, \"\"q\"\"\"\r\n2,Bo\r\n,\r\n".as_bytes()
        );

        // The header flips over the same rows: positions in edits count it.
        t.set_header(false);
        assert_eq!(row_text(&mut t, 1), ["0", "Zoë, \"q\""]);
        t.set_header(true);

        while let Some(inverse) = undo.pop() {
            t.apply(inverse);
        }
        assert_eq!(t.changes(), 0);
        assert_eq!(saved(&t), content, "back to the original bytes");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_deleted_row_comes_back_with_its_edits() {
        let (path, mut t) = opened("delete", b"id,v\n1,a\n2,b\n3,c\n");
        let b = CellRef {
            row: t.row_id(1),
            col: ColId::source(1),
        };
        t.apply(Edit::set(b, "B"));
        let delete = t.delete_rows(1, 2).unwrap();
        let restore = t.apply(delete);
        assert_eq!(t.row_count(), 1);
        assert_eq!(saved(&t), b"id,v\n1,a\n");
        assert_eq!(t.row_of(b.row), None, "not shown while deleted");
        t.apply(restore);
        assert_eq!(row_text(&mut t, 1), ["2", "B"]);
        assert_eq!(row_text(&mut t, 2), ["3", "c"]);
        assert!(t.delete_rows(2, 2).is_none(), "past the end");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn row_edits_wait_for_the_whole_file_to_be_indexed() {
        let path =
            std::env::temp_dir().join(format!("data-model-unbuilt-{}.csv", std::process::id()));
        std::fs::write(&path, "a\n1\n").unwrap();
        let mut t = CsvTable::open(&path, DelimiterChoice::Auto).unwrap();
        assert!(t.insert_rows(0, 1).is_none());
        assert!(t.delete_rows(0, 1).is_none());
        std::fs::remove_file(path).unwrap();
    }

    /// EDIT-4: inserted columns show and save empty (or with their edits), deleted ones
    /// are left out of every row, other fields keep their raw bytes (quotes included),
    /// short rows stay short; undoing everything gives back the file byte for byte.
    #[test]
    fn column_inserts_and_deletes_reassemble_rows_on_save() {
        let content = b"id,name,city\n1,\"Ann, A\",Paris\n2,Bo\n";
        let (path, mut t) = opened("cols", content);
        let mut undo = Vec::new();
        let insert = t.insert_cols(1, 1).unwrap();
        undo.push(t.apply(insert));
        let new = t.col_id(1);
        assert!(new.is_inserted());
        let at = CellRef {
            row: t.row_id(0),
            col: new,
        };
        undo.push(t.apply(Edit::set(at, "x")));
        let city = t.delete_cols(3, 1).unwrap();
        undo.push(t.apply(city));
        let row = t.insert_rows(t.row_count(), 1).unwrap();
        undo.push(t.apply(row));
        let last = CellRef {
            row: t.row_id(2),
            col: new,
        };
        undo.push(t.apply(Edit::set(last, "y")));

        assert_eq!(row_text(&mut t, 0), ["1", "x", "Ann, A"]);
        assert_eq!(row_text(&mut t, 1), ["2", "", "Bo"]);
        assert_eq!(
            t.header_cell(1).as_deref(),
            Some(""),
            "a title for the new column"
        );
        assert_eq!(t.header_cell(2).as_deref(), Some("name"));
        assert_eq!(t.col_of(ColId::source(2)), None, "city is gone");
        assert_eq!(t.col_of(ColId::source(1)), Some(2));
        assert_eq!(
            t.changes(),
            2 + 1 + 1 + 1,
            "cells, column in, column out, row in"
        );
        assert_eq!(saved(&t), b"id,,name\n1,x,\"Ann, A\"\n2,,Bo\n,y,\n");

        while let Some(inverse) = undo.pop() {
            t.apply(inverse);
        }
        assert_eq!(t.changes(), 0);
        assert_eq!(saved(&t), content, "back to the original bytes");
        std::fs::remove_file(path).unwrap();
    }

    /// EDIT-5: cleared cells read and save empty; other fields keep their raw bytes
    /// (quotes included), short rows stay short, an open-ended clear reaches a wider
    /// row's extra field, edits inside a clear go and come back with its undo, typing
    /// into a cleared cell wins. Undoing everything gives back the file byte for byte.
    #[test]
    fn cleared_cells_read_and_save_empty_and_undo_exactly() {
        let content =
            b"id,name,city\n1,\"Ann, A\",Paris\n2,Bo\n3,x,y,extra\n4,\"q\",z\n5,keep,me\n";
        let (path, mut t) = opened("clear", content);
        let mut undo = Vec::new();
        let at = |t: &CsvTable, r, c| CellRef {
            row: t.row_id(r),
            col: t.col_id(c),
        };
        undo.push(t.apply(Edit::set(at(&t, 3, 2), "covered")));
        let rect = t.clear_cells(0..2, 1..3).unwrap();
        undo.push(t.apply(rect));
        let open = t.clear_cells(2..4, 2..).unwrap();
        undo.push(t.apply(open));
        let row = t.insert_rows(5, 1).unwrap();
        undo.push(t.apply(row));
        undo.push(t.apply(Edit::set(at(&t, 5, 0), "new")));
        let inserted = t.clear_cells(5..6, 0..1).unwrap();
        undo.push(t.apply(inserted));
        undo.push(t.apply(Edit::set(at(&t, 0, 1), "typed")));

        assert_eq!(row_text(&mut t, 0), ["1", "typed", ""]);
        assert_eq!(row_text(&mut t, 1), ["2", ""]);
        assert_eq!(row_text(&mut t, 2), ["3", "x", "", ""]);
        assert_eq!(row_text(&mut t, 3), ["4", "q", ""]);
        assert_eq!(row_text(&mut t, 5), Vec::<String>::new());
        assert_eq!(t.cell_value(3, 2).as_deref(), Some(""));
        assert_eq!(
            t.cell_value(1, 2),
            None,
            "past a short row's end there is no cell"
        );
        assert_eq!(t.header_cell(1).as_deref(), Some("name"));
        assert_eq!(
            t.changes(),
            2 + 1 + 1,
            "two file clears, one cell, one row in"
        );
        assert_eq!(
            saved(&t),
            b"id,name,city\n1,typed,\n2,\n3,x,,\n4,\"q\",\n5,keep,me\n,,\n"
        );

        while let Some(inverse) = undo.pop() {
            t.apply(inverse);
        }
        assert_eq!(t.changes(), 0);
        assert_eq!(saved(&t), content, "back to the original bytes");
        std::fs::remove_file(path).unwrap();
    }
}
