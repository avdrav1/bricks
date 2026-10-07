//! Writing a table back out (SAVE-1): untouched rows are copied from the source as raw byte
//! ranges, so they come out byte for byte (invariant 1, SAVE-4). In an edited row only the
//! edited cells are encoded; every other field keeps its exact bytes, and the row keeps its
//! line ending.

use crate::{Col, CsvTable, RowId};
use csv_engine::{encode_field, split_fields, Dialect, Field, RowIndex, SparseRowIndex};
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::sync::Arc;

const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// Everything a save needs, detached from the UI so it can run on a worker thread: the
/// source (shared, read-only) and a copy of the edits as they were when the save started.
pub struct SaveJob {
    index: Arc<SparseRowIndex>,
    dialect: Dialect,
    /// Edited rows in file order.
    edits: Vec<(u64, BTreeMap<Col, Box<str>>)>,
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
        let mut edits: Vec<(u64, BTreeMap<Col, Box<str>>)> = self
            .overlay
            .iter_rows()
            .map(|(RowId(r), cols)| (*r, cols.clone()))
            .collect();
        edits.sort_unstable_by_key(|(r, _)| *r);
        Ok(SaveJob {
            index: self.index.clone(),
            dialect: self.dialect,
            edits,
        })
    }
}

/// Counts bytes on their way to the real writer.
struct Counting<'a> {
    inner: &'a mut dyn Write,
    bytes: u64,
}

impl Write for Counting<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.bytes += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl SaveJob {
    /// The dialect the file was read with; the save writes the same one.
    pub fn dialect(&self) -> &Dialect {
        &self.dialect
    }

    /// Rows the saved file will have.
    pub fn row_count(&self) -> u64 {
        self.index.row_count()
    }

    /// Write the whole table to `w`.
    pub fn write_to(&self, w: &mut dyn Write) -> io::Result<SaveStats> {
        let mut w = Counting { inner: w, bytes: 0 };
        let rows = self.index.row_count();
        let (mut next, mut fields, mut line_buf) = (0u64, Vec::new(), Vec::new());
        for (row, cols) in &self.edits {
            self.copy_rows(next..*row, &mut w)?;
            self.write_edited(*row, cols, &mut fields, &mut line_buf)?;
            w.write_all(&line_buf)?;
            next = row + 1;
        }
        self.copy_rows(next..rows, &mut w)?;
        if self.index.source().changed() {
            return Err(io::Error::other(
                "the source file changed on disk during the save",
            ));
        }
        Ok(SaveStats {
            rows,
            bytes: w.bytes,
        })
    }

    fn span(&self, row: u64) -> io::Result<std::ops::Range<usize>> {
        let s = self
            .index
            .row_span(row)
            .ok_or_else(|| io::Error::other("the source file changed on disk"))?;
        Ok(s.start as usize..s.end as usize)
    }

    /// Copy source rows `[a, b)` as one byte range.
    fn copy_rows(&self, rows: std::ops::Range<u64>, w: &mut dyn Write) -> io::Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let (start, end) = (self.span(rows.start)?.start, self.span(rows.end - 1)?.end);
        w.write_all(&self.index.source().bytes()[start..end])
    }

    /// Re-assemble one edited row into `out`.
    fn write_edited(
        &self,
        row: u64,
        cols: &BTreeMap<Col, Box<str>>,
        fields: &mut Vec<Field>,
        out: &mut Vec<u8>,
    ) -> io::Result<()> {
        out.clear();
        let span = self.span(row)?;
        let mut line = &self.index.source().bytes()[span.clone()];
        if span.start == 0 && line.starts_with(UTF8_BOM) {
            out.extend_from_slice(UTF8_BOM);
            line = &line[UTF8_BOM.len()..];
        }
        split_fields(line, &self.dialect, fields);
        let content_end = fields.last().map_or(0, |f| f.raw.end as usize);
        let width = fields
            .len()
            .max(cols.keys().next_back().map_or(0, |&c| c as usize + 1));
        for c in 0..width {
            if c > 0 {
                out.push(self.dialect.delimiter);
            }
            match (cols.get(&(c as Col)), fields.get(c)) {
                (Some(v), _) => encode_field(v.as_bytes(), &self.dialect, out),
                (None, Some(f)) => out.extend_from_slice(f.raw(line)),
                (None, None) => {}
            }
        }
        out.extend_from_slice(&line[content_end..]); // the row's own line ending, if any
        Ok(())
    }
}
