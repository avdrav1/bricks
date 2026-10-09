//! Writing a table back out (SAVE-1): untouched rows are copied from the source as raw byte
//! ranges, so they come out byte for byte (invariant 1, SAVE-4). In an edited row only the
//! edited cells are encoded; every other field keeps its exact bytes, and the row keeps its
//! line ending. Rows are written in the table's order (EDIT-3): runs of source rows copy as
//! ranges, inserted rows are encoded from their edits, and deleted rows are left out.
//! Rows with cleared cells (EDIT-5) are re-assembled with those fields empty. Save As
//! (SAVE-2) can write another delimiter and encoding: then every row is re-assembled, each
//! field's value re-quoted for the new delimiter.

use crate::cleared::{cleared_at, ColSet, Segment};
use crate::colmap::ColMap;
use crate::rowmap::Run;
use crate::{Col, ColId, CsvTable, RowId};
use csv_engine::{
    encode_field, split_fields, Charset, Dialect, Encoding, Field, LineEnding, RowIndex,
    SparseRowIndex,
};
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";
/// Rows whose spans a column-changed save fetches at once.
const SPAN_CHUNK: u64 = 4_096;
/// Untouched rows are copied in pieces this big, so a cancel is seen at least this often.
const COPY_CHUNK: usize = 4 << 20;

/// A running save's progress, shared with the UI (SAVE-3). Its cancel flag is the job's
/// (ENG-8), passed to [`SaveJob::write_to`] beside it.
#[derive(Debug, Default)]
pub struct SaveProgress {
    written: AtomicU64,
    checking: AtomicBool,
}

impl SaveProgress {
    /// UTF-8 bytes written so far.
    pub fn written(&self) -> u64 {
        self.written.load(Ordering::Relaxed)
    }

    /// Verification of the written file has started.
    pub fn set_checking(&self) {
        self.checking.store(true, Ordering::Relaxed);
    }

    pub fn is_checking(&self) -> bool {
        self.checking.load(Ordering::Relaxed)
    }
}

/// Everything a save needs, detached from the UI so it can run on a worker thread: the
/// source (shared, read-only) and a copy of the row order and edits as they were when the
/// save started.
pub struct SaveJob {
    index: Arc<SparseRowIndex>,
    /// The dialect the source is read with.
    source: Dialect,
    /// The dialect rows are written in: the source's, or another delimiter (Save As).
    out: Dialect,
    /// The encoding written: the file's own, or another (Save As).
    encoding: Encoding,
    /// Rows in the order they are written: source rows by file row, inserted rows by id.
    order: Vec<Run>,
    /// Column order, if columns were inserted or deleted (EDIT-4): then every row is
    /// re-assembled, fields still copied as their raw bytes.
    cols: Option<ColMap>,
    /// Edited rows, sorted by id (source rows first, in file order).
    edits: Vec<(RowId, BTreeMap<ColId, Box<str>>)>,
    /// Cleared cells by file row (EDIT-5).
    cleared: Arc<Vec<Segment>>,
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
            source: self.dialect,
            out: self.dialect,
            encoding: self.encoding,
            order,
            cols,
            edits,
            cleared: self.cleared.segments().clone(),
            width: width.max(1),
        })
    }
}

/// The real writer, plus the byte count and whether the output so far ends a line.
struct Out<'a> {
    inner: &'a mut dyn Write,
    bytes: u64,
    ended: bool,
    progress: &'a SaveProgress,
    cancel: &'a AtomicBool,
}

impl Out<'_> {
    fn put(&mut self, b: &[u8]) -> io::Result<()> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "save cancelled"));
        }
        if let Some(&last) = b.last() {
            self.inner.write_all(b)?;
            self.bytes += b.len() as u64;
            self.ended = last == b'\n';
            self.progress
                .written
                .fetch_add(b.len() as u64, Ordering::Relaxed);
        }
        Ok(())
    }
}

impl SaveJob {
    /// Write `delimiter` and `encoding` instead of the file's own (Save As). Another
    /// delimiter re-assembles every row; another encoding only changes the bytes written.
    pub fn with_format(mut self, delimiter: u8, encoding: Encoding) -> Self {
        self.out.delimiter = delimiter;
        self.encoding = encoding;
        self
    }

    /// The dialect the save writes (the file's own unless [`Self::with_format`] changed it).
    pub fn dialect(&self) -> &Dialect {
        &self.out
    }

    /// The encoding the save writes. [`SaveJob::write_to`] writes UTF-8 (with the BOM if
    /// this is UTF-8 with one); the caller encodes (`csv_engine::EncodeWriter`) when this
    /// is anything else.
    pub fn encoding(&self) -> Encoding {
        self.encoding
    }

    /// Every source row is written field by field: the columns moved, or the delimiter
    /// changed.
    fn reassemble_all(&self) -> bool {
        self.cols.is_some() || self.out.delimiter != self.source.delimiter
    }

    /// Rows the saved file will have.
    pub fn row_count(&self) -> u64 {
        self.order.iter().map(|r| r.len).sum()
    }

    /// Roughly the bytes [`Self::write_to`] will write: the source's size.
    pub fn estimated_bytes(&self) -> u64 {
        self.index.source().bytes().len() as u64
    }

    fn line_ending(&self) -> &'static [u8] {
        match self.out.line_ending {
            LineEnding::Lf => b"\n",
            LineEnding::CrLf => b"\r\n",
        }
    }

    fn has_bom(&self) -> bool {
        self.index.source().bytes().starts_with(UTF8_BOM)
    }

    /// Write the whole table to `w`, counting bytes into `progress` and stopping with an
    /// `Interrupted` error once `cancel` is set.
    pub fn write_to(
        &self,
        w: &mut dyn Write,
        progress: &SaveProgress,
        cancel: &AtomicBool,
    ) -> io::Result<SaveStats> {
        let mut w = Out {
            inner: w,
            bytes: 0,
            ended: true,
            progress,
            cancel,
        };
        // The byte-order mark belongs to the file, not to whichever row comes first. Other
        // encodings write their own (`EncodeWriter`).
        if self.encoding.charset == Charset::Utf8 && self.encoding.bom {
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

    /// Source rows `rows`: copied as byte ranges between the rows that need re-assembling
    /// (edited, or with cleared cells), or every one re-assembled when the columns changed.
    fn write_source(
        &self,
        rows: std::ops::Range<u64>,
        w: &mut Out,
        fields: &mut Vec<Field>,
        line: &mut Vec<u8>,
    ) -> io::Result<()> {
        let lo = self.edits.partition_point(|(id, _)| id.0 < rows.start);
        let hi = self.edits.partition_point(|(id, _)| id.0 < rows.end);
        let mut edits = self.edits[lo..hi].iter().peekable();
        if self.reassemble_all() {
            return self.assemble_rows(rows, &mut edits, w, fields, line);
        }
        let first = self.cleared.partition_point(|s| s.rows.end <= rows.start);
        let mut cleared = self.cleared[first..]
            .iter()
            .take_while(|s| s.rows.start < rows.end)
            .map(|s| s.rows.start.max(rows.start)..s.rows.end.min(rows.end))
            .peekable();
        let mut next = rows.start;
        loop {
            let edited = edits.peek().map(|(id, _)| id.0);
            let dirty = match (edited, cleared.peek()) {
                (None, None) => break,
                (Some(e), Some(c)) if e < c.start => e..e + 1,
                (Some(e), None) => e..e + 1,
                (_, Some(_)) => cleared.next().unwrap_or_default(),
            };
            self.copy_rows(next..dirty.start, w)?;
            next = dirty.end;
            self.assemble_rows(dirty, &mut edits, w, fields, line)?;
        }
        self.copy_rows(next..rows.end, w)
    }

    /// Re-assemble source rows `rows`, taking their edits from the front of `edits`.
    /// Spans are fetched a chunk at a time (one index lookup and one forward scan per
    /// chunk, not per row).
    fn assemble_rows<'e>(
        &self,
        rows: std::ops::Range<u64>,
        edits: &mut std::iter::Peekable<
            impl Iterator<Item = &'e (RowId, BTreeMap<ColId, Box<str>>)>,
        >,
        w: &mut Out,
        fields: &mut Vec<Field>,
        line: &mut Vec<u8>,
    ) -> io::Result<()> {
        let mut spans = Vec::new();
        let mut start = rows.start;
        while start < rows.end {
            let end = rows.end.min(start + SPAN_CHUNK);
            let found = self.index.row_spans(start..end, &mut spans).ok();
            if found != Some((end - start) as usize) {
                return Err(io::Error::other("the source file changed on disk"));
            }
            for (row, span) in (start..end).zip(&spans) {
                let cols = edits.next_if(|(id, _)| id.0 == row).map(|(_, c)| c);
                let cleared = cleared_at(&self.cleared, row);
                let span = span.start as usize..span.end as usize;
                self.assemble(span, cols, cleared, fields, line);
                self.start_row(w)?;
                w.put(line)?;
            }
            start = end;
        }
        Ok(())
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

    /// Copy source rows `[a, b)` as one byte range (without the file's BOM), in
    /// [`COPY_CHUNK`] pieces.
    fn copy_rows(&self, rows: std::ops::Range<u64>, w: &mut Out) -> io::Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let (mut start, end) = (self.span(rows.start)?.start, self.span(rows.end - 1)?.end);
        if start == 0 && self.has_bom() {
            start = UTF8_BOM.len();
        }
        self.start_row(w)?;
        for chunk in self.index.source().bytes()[start..end].chunks(COPY_CHUNK) {
            w.put(chunk)?;
        }
        Ok(())
    }

    /// Re-assemble one source row into `out`: edits encoded, cleared fields empty, every
    /// other field copied as its raw bytes, the row's own line ending kept.
    fn assemble(
        &self,
        span: std::ops::Range<usize>,
        edits: Option<&BTreeMap<ColId, Box<str>>>,
        cleared: Option<&ColSet>,
        fields: &mut Vec<Field>,
        out: &mut Vec<u8>,
    ) {
        out.clear();
        let mut line = &self.index.source().bytes()[span.clone()];
        if span.start == 0 {
            line = line.strip_prefix(UTF8_BOM).unwrap_or(line);
        }
        split_fields(line, &self.source, fields);
        let content_end = fields.last().map_or(0, |f| f.raw.end as usize);
        for pos in 0..self.row_width(fields.len(), edits) {
            if pos > 0 {
                out.push(self.out.delimiter);
            }
            let id = self.col_at(pos);
            let field = (!id.is_inserted())
                .then(|| fields.get(id.0 as usize))
                .flatten()
                .filter(|_| !cleared.is_some_and(|c| c.contains(id)));
            match (edits.and_then(|e| e.get(&id)), field) {
                (Some(v), _) => encode_field(v.as_bytes(), &self.out, out),
                (None, Some(f)) if self.out.delimiter == self.source.delimiter => {
                    out.extend_from_slice(f.raw(line))
                }
                (None, Some(f)) => encode_field(&f.value(line, self.source.quote), &self.out, out),
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
                out.push(self.out.delimiter);
            }
            if let Some(v) = edits.and_then(|e| e.get(&self.col_at(pos))) {
                encode_field(v.as_bytes(), &self.out, out);
            }
        }
        out.extend_from_slice(self.line_ending());
    }
}
