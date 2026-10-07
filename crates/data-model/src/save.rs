//! Writing a table back out (SAVE-1): untouched rows are copied from the source as raw byte
//! ranges, so they come out byte for byte (invariant 1, SAVE-4). In an edited row only the
//! edited cells are encoded; every other field keeps its exact bytes, and the row keeps its
//! line ending. Rows are written in the table's order (EDIT-3): runs of source rows copy as
//! ranges, inserted rows are encoded from their edits, and deleted rows are left out.

use crate::colmap::ColMap;
use crate::rowmap::Run;
use crate::{Col, ColId, CsvTable, RowId};
use csv_engine::{
    encode_field, split_fields, Dialect, Encoding, Field, LineEnding, RowIndex, SparseRowIndex,
};
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::sync::Arc;

const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";
/// Rows whose spans a column-changed save fetches at once.
const SPAN_CHUNK: u64 = 4_096;

/// Everything a save needs, detached from the UI so it can run on a worker thread: the
/// source (shared, read-only) and a copy of the row order and edits as they were when the
/// save started.
pub struct SaveJob {
    index: Arc<SparseRowIndex>,
    dialect: Dialect,
    encoding: Encoding,
    /// Rows in the order they are written: source rows by file row, inserted rows by id.
    order: Vec<Run>,
    /// Column order, if columns were inserted or deleted (EDIT-4): then every row is
    /// re-assembled, fields still copied as their raw bytes.
    cols: Option<ColMap>,
    /// Edited rows, sorted by id (source rows first, in file order).
    edits: Vec<(RowId, BTreeMap<ColId, Box<str>>)>,
    /// Fields an inserted row has at least: the first row's, so an empty inserted row is
    /// written as delimiters, not as a blank line some readers skip.
    width: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveStats {
    pub rows: u64,
    pub bytes: u64,
}

#[derive(Debug)]
pub enum SaveJobError {
    /// Indexing has not finished, so the row count is not known yet.
    NotIndexed,
}

impl CsvTable {
    /// Snapshot the table for saving.
    pub fn save_job(&self) -> Result<SaveJob, SaveJobError> {
        if !self.index.is_complete() {
            return Err(SaveJobError::NotIndexed);
        }
        let order = match &self.rows {
            Some(map) => {
                let mut runs = Vec::new();
                map.for_each(|r| runs.push(*r));
                runs
            }
            None => vec![Run {
                first: RowId::source(0),
                len: self.index.row_count(),
            }],
        };
        let mut edits: Vec<(RowId, BTreeMap<ColId, Box<str>>)> = self
            .overlay
            .iter_rows()
            .map(|(&id, cols)| (id, cols.clone()))
            .collect();
        edits.sort_unstable_by_key(|(id, _)| *id);
        let cols = self.cols.clone().filter(|m| !m.is_file_order());
        let mut fields = Vec::new();
        let first = self.index.row_fields(0, &mut fields).map(|_| fields.len());
        let width = match (&cols, first) {
            (Some(map), Some(n)) => map.width(n as Col, |_| false) as usize,
            (None, Some(n)) => n,
            (_, None) => 1,
        };
        Ok(SaveJob {
            index: self.index.clone(),
            dialect: self.dialect,
            encoding: self.encoding,
            order,
            cols,
            edits,
            width: width.max(1),
        })
    }
}

/// The real writer, plus the byte count and whether the output so far ends a line.
struct Out<'a> {
    inner: &'a mut dyn Write,
    bytes: u64,
    ended: bool,
}

impl Out<'_> {
    fn put(&mut self, b: &[u8]) -> io::Result<()> {
        if let Some(&last) = b.last() {
            self.inner.write_all(b)?;
            self.bytes += b.len() as u64;
            self.ended = last == b'\n';
        }
        Ok(())
    }
}

impl SaveJob {
    /// The dialect the file was read with; the save writes the same one.
    pub fn dialect(&self) -> &Dialect {
        &self.dialect
    }

    /// The file's encoding on disk. [`SaveJob::write_to`] writes UTF-8; the caller encodes
    /// (`csv_engine::EncodeWriter`) when this is anything else.
    pub fn encoding(&self) -> Encoding {
        self.encoding
    }

    /// Rows the saved file will have.
    pub fn row_count(&self) -> u64 {
        self.order.iter().map(|r| r.len).sum()
    }

    fn line_ending(&self) -> &'static [u8] {
        match self.dialect.line_ending {
            LineEnding::Lf => b"\n",
            LineEnding::CrLf => b"\r\n",
        }
    }

    fn has_bom(&self) -> bool {
        self.index.source().bytes().starts_with(UTF8_BOM)
    }

    /// Write the whole table to `w`.
    pub fn write_to(&self, w: &mut dyn Write) -> io::Result<SaveStats> {
        let mut w = Out {
            inner: w,
            bytes: 0,
            ended: true,
        };
        // The byte-order mark belongs to the file, not to whichever row comes first.
        if self.has_bom() {
            w.inner.write_all(UTF8_BOM)?;
            w.bytes += UTF8_BOM.len() as u64;
        }
        let (mut fields, mut line) = (Vec::new(), Vec::new());
        for run in &self.order {
            let ids = run.first.0..run.first.0 + run.len;
            if run.first.is_inserted() {
                for id in ids {
                    self.write_inserted(RowId(id), &mut line);
                    self.start_row(&mut w)?;
                    w.put(&line)?;
                }
            } else {
                self.write_source(ids, &mut w, &mut fields, &mut line)?;
            }
        }
        if self.index.source().changed() {
            return Err(io::Error::other(
                "the source file changed on disk during the save",
            ));
        }
        Ok(SaveStats {
            rows: self.row_count(),
            bytes: w.bytes,
        })
    }

    /// Before a row: end the previous one if it had no line ending (the file's last row,
    /// now followed by others).
    fn start_row(&self, w: &mut Out) -> io::Result<()> {
        if !w.ended {
            w.put(self.line_ending())?;
        }
        Ok(())
    }

    /// Source rows `rows`: copied as byte ranges between their edited rows, or every one
    /// re-assembled when the columns changed.
    fn write_source(
        &self,
        rows: std::ops::Range<u64>,
        w: &mut Out,
        fields: &mut Vec<Field>,
        line: &mut Vec<u8>,
    ) -> io::Result<()> {
        let lo = self.edits.partition_point(|(id, _)| id.0 < rows.start);
        let hi = self.edits.partition_point(|(id, _)| id.0 < rows.end);
        let edits = &self.edits[lo..hi];
        if self.cols.is_some() {
            // Every row is re-assembled: fetch spans a chunk at a time (one index lookup
            // and one forward scan per chunk, not per row).
            let (mut edits, mut spans) = (edits.iter().peekable(), Vec::new());
            let mut start = rows.start;
            while start < rows.end {
                let end = rows.end.min(start + SPAN_CHUNK);
                let found = self.index.row_spans(start..end, &mut spans).ok();
                if found != Some((end - start) as usize) {
                    return Err(io::Error::other("the source file changed on disk"));
                }
                for (row, span) in (start..end).zip(&spans) {
                    let cols = edits.next_if(|(id, _)| id.0 == row).map(|(_, c)| c);
                    self.assemble(span.start as usize..span.end as usize, cols, fields, line);
                    self.start_row(w)?;
                    w.put(line)?;
                }
                start = end;
            }
            return Ok(());
        }
        let mut next = rows.start;
        for (id, cols) in edits {
            self.copy_rows(next..id.0, w)?;
            self.write_edited(id.0, Some(cols), fields, line)?;
            self.start_row(w)?;
            w.put(line)?;
            next = id.0 + 1;
        }
        self.copy_rows(next..rows.end, w)
    }

    /// The column shown at position `pos`.
    fn col_at(&self, pos: usize) -> ColId {
        let pos = pos as Col;
        self.cols
            .as_ref()
            .map_or(ColId::source(pos), |m| m.get(pos))
    }

    /// Positions a row spans, given its file fields and edits.
    fn row_width(&self, fields: usize, edits: Option<&BTreeMap<ColId, Box<str>>>) -> usize {
        match (&self.cols, edits) {
            (Some(map), Some(e)) => map.width_with(fields as Col, e.keys().copied()) as usize,
            (Some(map), None) => map.width(fields as Col, |_| false) as usize,
            (None, e) => {
                let last = e.and_then(|e| e.keys().next_back());
                fields.max(last.map_or(0, |c| c.0 as usize + 1))
            }
        }
    }

    fn span(&self, row: u64) -> io::Result<std::ops::Range<usize>> {
        let s = self
            .index
            .row_span(row)
            .ok_or_else(|| io::Error::other("the source file changed on disk"))?;
        Ok(s.start as usize..s.end as usize)
    }

    /// Copy source rows `[a, b)` as one byte range (without the file's BOM).
    fn copy_rows(&self, rows: std::ops::Range<u64>, w: &mut Out) -> io::Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let (mut start, end) = (self.span(rows.start)?.start, self.span(rows.end - 1)?.end);
        if start == 0 && self.has_bom() {
            start = UTF8_BOM.len();
        }
        self.start_row(w)?;
        w.put(&self.index.source().bytes()[start..end])
    }

    /// Re-assemble one source row into `out`: edits encoded, every other field copied
    /// as its raw bytes, the row's own line ending kept.
    fn write_edited(
        &self,
        row: u64,
        edits: Option<&BTreeMap<ColId, Box<str>>>,
        fields: &mut Vec<Field>,
        out: &mut Vec<u8>,
    ) -> io::Result<()> {
        let span = self.span(row)?;
        self.assemble(span, edits, fields, out);
        Ok(())
    }

    /// [`Self::write_edited`] for a row whose span is known.
    fn assemble(
        &self,
        span: std::ops::Range<usize>,
        edits: Option<&BTreeMap<ColId, Box<str>>>,
        fields: &mut Vec<Field>,
        out: &mut Vec<u8>,
    ) {
        out.clear();
        let mut line = &self.index.source().bytes()[span.clone()];
        if span.start == 0 {
            line = line.strip_prefix(UTF8_BOM).unwrap_or(line);
        }
        split_fields(line, &self.dialect, fields);
        let content_end = fields.last().map_or(0, |f| f.raw.end as usize);
        for pos in 0..self.row_width(fields.len(), edits) {
            if pos > 0 {
                out.push(self.dialect.delimiter);
            }
            let id = self.col_at(pos);
            let field = (!id.is_inserted())
                .then(|| fields.get(id.0 as usize))
                .flatten();
            match (edits.and_then(|e| e.get(&id)), field) {
                (Some(v), _) => encode_field(v.as_bytes(), &self.dialect, out),
                (None, Some(f)) => out.extend_from_slice(f.raw(line)),
                (None, None) => {}
            }
        }
        out.extend_from_slice(&line[content_end..]); // the row's own line ending, if any
    }

    /// Encode one inserted row from its edits into `out`, line ending included.
    fn write_inserted(&self, id: RowId, out: &mut Vec<u8>) {
        out.clear();
        let edits = self
            .edits
            .binary_search_by_key(&id, |(i, _)| *i)
            .ok()
            .map(|i| &self.edits[i].1);
        let width = self.row_width(0, edits).max(self.width);
        for pos in 0..width {
            if pos > 0 {
                out.push(self.dialect.delimiter);
            }
            if let Some(v) = edits.and_then(|e| e.get(&self.col_at(pos))) {
                encode_field(v.as_bytes(), &self.dialect, out);
            }
        }
        out.extend_from_slice(self.line_ending());
    }
}
