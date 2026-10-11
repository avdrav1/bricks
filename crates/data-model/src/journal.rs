//! Crash recovery journal (APP-7, XLSX-1). Everything the user changed that a save would
//! write is the workbook's sheets (names, order, where each comes from) and each sheet's
//! edit state ([`EditState`]): cell edits, the row order (sorts, inserted and deleted
//! rows), the column order, and cleared blocks. The undo history can't stand in for it:
//! its memory budget trims old steps. The app writes it, behind a header naming the file
//! it belongs to ([`JournalHeader`]), and after a crash puts it back as one undoable step
//! ([`Workbook::restore_plan`]).
//!
//! Format, little-endian: magic `BRICKSJ2`; the header; `sheet_count u32`, `active u32`
//! (a position); per sheet its id, name, origin, header flag and state; then an FNV-1a
//! hash of everything before it, so a cut or damaged journal is refused rather than
//! half-applied. Journals of v0.1.x (`BRICKSJ1`: one table's state, no kind byte) are
//! still read, as one sheet.

use crate::cleared::{Cleared, ColSet};
use crate::colmap::ColMap;
use crate::rowmap::{RowOrder, Run};
use crate::workbook::{Origin, Sheet, SheetId, Workbook};
use crate::{BookEdit, CellRef, ColId, CsvTable, Edit, EditOverlay, RowId};
use csv_engine::{Charset, Encoding, RowIndex};
use std::io::{self, Read, Write};
use std::ops::Range;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use std::sync::Arc;

const MAGIC: &[u8; 8] = b"BRICKSJ2";
/// The single-table journal of v0.1.x, still read.
const MAGIC_V1: &[u8; 8] = b"BRICKSJ1";
/// More sheets than any workbook has: a damaged count is refused.
const MAX_SHEETS: u32 = 1 << 16;

/// What the user changed, as a save would write it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditState {
    pub(crate) overlay: EditOverlay,
    pub(crate) rows: Option<RowOrder>,
    pub(crate) cols: Option<ColMap>,
    pub(crate) cleared: Cleared,
    pub(crate) next_inserted: u64,
    pub(crate) next_inserted_col: u32,
}

impl EditState {
    /// Nothing changed.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    pub fn heap_bytes(&self) -> usize {
        self.overlay.len() * 48 + self.rows.as_ref().map_or(0, RowOrder::heap_bytes)
    }
}

/// What kind of file the journal's document was opened from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Csv,
    Xlsx,
}

/// Which file a journal belongs to and how it was read, plus what the start window shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalHeader {
    /// `None` for an untitled document.
    pub path: Option<PathBuf>,
    /// CSV or .xlsx; journals of v0.1.x are CSV.
    pub kind: FileKind,
    /// The file as it was when opened: a journal applies only to the same bytes.
    pub file_size: u64,
    pub file_mtime_ns: u64,
    pub delimiter: u8,
    pub has_header: bool,
    pub encoding: Encoding,
    /// When the journal was written, in milliseconds since the Unix epoch.
    pub saved_ms: u64,
    /// Unsaved changes and rows shown when written (for the offer's wording).
    pub changes: u64,
    pub rows: u64,
    /// The process that wrote it.
    pub pid: u32,
}

/// One sheet as journaled: who it is, where its rows come from, and what changed.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetState {
    pub id: SheetId,
    pub name: String,
    pub origin: Origin,
    pub has_header: bool,
    pub state: EditState,
}

/// A journaled workbook: its sheets in order and the position of the active one.
#[derive(Debug, Clone, PartialEq)]
pub struct BookState {
    pub active: u32,
    pub sheets: Vec<SheetState>,
}

#[derive(Debug)]
pub enum JournalError {
    Io(io::Error),
    /// Not a journal, from a newer version, cut short, or damaged.
    Corrupt(&'static str),
    /// The state names rows or columns the file doesn't have.
    Mismatch(&'static str),
}

impl std::fmt::Display for JournalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Corrupt(why) => write!(f, "the recovery journal is damaged ({why})"),
            Self::Mismatch(why) => write!(f, "the edits don't fit the file ({why})"),
        }
    }
}

impl From<io::Error> for JournalError {
    fn from(e: io::Error) -> Self {
        if e.kind() == io::ErrorKind::UnexpectedEof {
            Self::Corrupt("cut short")
        } else {
            Self::Io(e)
        }
    }
}

/// FNV-1a over everything written or read, so the end of a journal can be checked.
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
    }
}

struct Writer<W> {
    inner: W,
    hash: Fnv,
}

impl<W: Write> Writer<W> {
    fn bytes(&mut self, b: &[u8]) -> io::Result<()> {
        self.hash.feed(b);
        self.inner.write_all(b)
    }
    fn u8(&mut self, v: u8) -> io::Result<()> {
        self.bytes(&[v])
    }
    fn u32(&mut self, v: u32) -> io::Result<()> {
        self.bytes(&v.to_le_bytes())
    }
    fn u64(&mut self, v: u64) -> io::Result<()> {
        self.bytes(&v.to_le_bytes())
    }
    fn blob(&mut self, b: &[u8]) -> io::Result<()> {
        self.u64(b.len() as u64)?;
        self.bytes(b)
    }
}

struct Reader<R> {
    inner: R,
    hash: Fnv,
}

impl<R: Read> Reader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            hash: Fnv::new(),
        }
    }
    fn bytes(&mut self, n: usize) -> Result<Vec<u8>, JournalError> {
        let mut b = vec![0; n];
        self.inner.read_exact(&mut b)?;
        self.hash.feed(&b);
        Ok(b)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], JournalError> {
        let mut b = [0; N];
        self.inner.read_exact(&mut b)?;
        self.hash.feed(&b);
        Ok(b)
    }
    fn u8(&mut self) -> Result<u8, JournalError> {
        Ok(self.array::<1>()?[0])
    }
    fn u32(&mut self) -> Result<u32, JournalError> {
        Ok(u32::from_le_bytes(self.array()?))
    }
    fn u64(&mut self) -> Result<u64, JournalError> {
        Ok(u64::from_le_bytes(self.array()?))
    }
    /// A count of items at least `min_bytes` each: refused if bigger than any real
    /// journal, so a damaged count can't ask for terabytes.
    fn count(&mut self, min_bytes: u64) -> Result<usize, JournalError> {
        let n = self.u64()?;
        if n.saturating_mul(min_bytes.max(1)) > 1 << 40 {
            return Err(JournalError::Corrupt("impossible count"));
        }
        Ok(n as usize)
    }
    fn blob(&mut self) -> Result<Vec<u8>, JournalError> {
        let n = self.count(1)?;
        self.bytes(n)
    }
}

fn charset_code(c: Charset) -> u8 {
    match c {
        Charset::Utf8 => 0,
        Charset::Utf16Le => 1,
        Charset::Utf16Be => 2,
        Charset::Windows1252 => 3,
    }
}

fn charset_of(code: u8) -> Result<Charset, JournalError> {
    Ok(match code {
        0 => Charset::Utf8,
        1 => Charset::Utf16Le,
        2 => Charset::Utf16Be,
        3 => Charset::Windows1252,
        _ => return Err(JournalError::Corrupt("unknown encoding")),
    })
}

fn write_header<W: Write>(w: &mut Writer<W>, h: &JournalHeader) -> io::Result<()> {
    w.bytes(MAGIC)?;
    w.blob(
        h.path
            .as_ref()
            .map_or(&[][..], |p| p.as_os_str().as_bytes()),
    )?;
    w.u8(u8::from(h.path.is_some()))?;
    w.u8(match h.kind {
        FileKind::Csv => 0,
        FileKind::Xlsx => 1,
    })?;
    w.u64(h.file_size)?;
    w.u64(h.file_mtime_ns)?;
    w.u8(h.delimiter)?;
    w.u8(u8::from(h.has_header))?;
    w.u8(charset_code(h.encoding.charset))?;
    w.u8(u8::from(h.encoding.bom))?;
    w.u64(h.saved_ms)?;
    w.u64(h.changes)?;
    w.u64(h.rows)?;
    w.u32(h.pid)
}

/// The header, and whether the journal is a v0.1.x one (`BRICKSJ1`).
fn read_header_from<R: Read>(r: &mut Reader<R>) -> Result<(JournalHeader, bool), JournalError> {
    let magic = r.array::<8>()?;
    let v1 = match &magic {
        m if m == MAGIC => false,
        m if m == MAGIC_V1 => true,
        _ => return Err(JournalError::Corrupt("not a journal")),
    };
    let path = r.blob()?;
    let titled = r.u8()? == 1;
    let kind = if v1 {
        FileKind::Csv
    } else {
        match r.u8()? {
            0 => FileKind::Csv,
            1 => FileKind::Xlsx,
            _ => return Err(JournalError::Corrupt("unknown file kind")),
        }
    };
    let header = JournalHeader {
        path: titled.then(|| PathBuf::from(std::ffi::OsString::from_vec(path))),
        kind,
        file_size: r.u64()?,
        file_mtime_ns: r.u64()?,
        delimiter: r.u8()?,
        has_header: r.u8()? == 1,
        encoding: Encoding {
            charset: charset_of(r.u8()?)?,
            bom: r.u8()? == 1,
        },
        saved_ms: r.u64()?,
        changes: r.u64()?,
        rows: r.u64()?,
        pid: r.u32()?,
    };
    Ok((header, v1))
}

/// Read only the header (either version): enough to list journals without reading their
/// state.
pub fn read_journal_header(r: impl Read) -> Result<JournalHeader, JournalError> {
    read_header_from(&mut Reader::new(r)).map(|(h, _)| h)
}

fn write_colset<W: Write>(w: &mut Writer<W>, c: &ColSet) -> io::Result<()> {
    let (ids, rest) = c.parts();
    w.u64(ids.len() as u64)?;
    for id in ids {
        w.u32(id.0)?;
    }
    w.u8(u8::from(rest.is_some()))?;
    w.u32(rest.unwrap_or(0))
}

fn read_colset<R: Read>(r: &mut Reader<R>) -> Result<ColSet, JournalError> {
    let n = r.count(4)?;
    let ids = (0..n)
        .map(|_| r.u32().map(ColId))
        .collect::<Result<_, _>>()?;
    let has_rest = r.u8()? == 1;
    let rest = r.u32()?;
    Ok(ColSet::new(ids, has_rest.then_some(rest)))
}

fn write_state<W: Write>(w: &mut Writer<W>, t: &CsvTable) -> io::Result<()> {
    // Cell edits, sorted so the same state always writes the same bytes.
    let mut rows: Vec<_> = t.overlay.iter_rows().collect();
    rows.sort_unstable_by_key(|(id, _)| **id);
    w.u64(rows.len() as u64)?;
    for (id, cells) in rows {
        w.u64(id.0)?;
        w.u64(cells.len() as u64)?;
        for (col, value) in cells {
            w.u32(col.0)?;
            w.blob(value.as_bytes())?;
        }
    }
    match &t.rows {
        None => w.u8(0)?,
        Some(RowOrder::Runs(m)) => {
            w.u8(1)?;
            let mut runs = Vec::new();
            m.for_each(|r| runs.push(*r));
            w.u64(runs.len() as u64)?;
            for r in runs {
                w.u64(r.first.0)?;
                w.u64(r.len)?;
            }
        }
        Some(RowOrder::Sorted(ids)) => {
            w.u8(2)?;
            w.u64(ids.len() as u64)?;
            // One write for the whole order: it can be 19M rows.
            let bytes: Vec<u8> = ids.iter().flat_map(|v| v.to_le_bytes()).collect();
            w.bytes(&bytes)?;
        }
    }
    match &t.cols {
        None => w.u8(0)?,
        Some(m) => {
            w.u8(1)?;
            let (explicit, tail) = m.parts();
            w.u32(tail)?;
            w.u64(explicit.len() as u64)?;
            for id in explicit {
                w.u32(id.0)?;
            }
        }
    }
    let blocks: Vec<_> = t.cleared.blocks().collect();
    w.u64(blocks.len() as u64)?;
    for (ranges, cols) in blocks {
        w.u64(ranges.len() as u64)?;
        for r in ranges {
            w.u64(r.start)?;
            w.u64(r.end)?;
        }
        write_colset(w, cols)?;
    }
    w.u64(t.next_inserted)?;
    w.u32(t.next_inserted_col)
}

fn read_state<R: Read>(r: &mut Reader<R>) -> Result<EditState, JournalError> {
    let mut s = EditState::default();
    for _ in 0..r.count(16)? {
        let row = RowId(r.u64()?);
        for _ in 0..r.count(12)? {
            let col = ColId(r.u32()?);
            let value = String::from_utf8(r.blob()?)
                .map_err(|_| JournalError::Corrupt("a value isn't UTF-8"))?;
            s.overlay.set(CellRef { row, col }, value);
        }
    }
    s.rows = match r.u8()? {
        0 => None,
        1 => {
            let runs = (0..r.count(16)?)
                .map(|_| {
                    Ok(Run {
                        first: RowId(r.u64()?),
                        len: r.u64()?,
                    })
                })
                .collect::<Result<Vec<_>, JournalError>>()?;
            Some(RowOrder::from_runs(&runs))
        }
        2 => {
            let n = r.count(4)?;
            let bytes = r.bytes(n * 4)?;
            let ids = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|&c| u32::from_le_bytes(c))
                .collect();
            Some(RowOrder::Sorted(Arc::new(ids)))
        }
        _ => return Err(JournalError::Corrupt("unknown row order")),
    };
    s.cols = match r.u8()? {
        0 => None,
        1 => {
            let tail = r.u32()?;
            let explicit = (0..r.count(4)?)
                .map(|_| r.u32().map(ColId))
                .collect::<Result<_, _>>()?;
            Some(ColMap::from_parts(explicit, tail))
        }
        _ => return Err(JournalError::Corrupt("unknown column order")),
    };
    for _ in 0..r.count(24)? {
        let ranges: Vec<Range<u64>> = (0..r.count(16)?)
            .map(|_| Ok(r.u64()?..r.u64()?))
            .collect::<Result<_, JournalError>>()?;
        let cols = read_colset(r)?;
        s.cleared.push(ranges, cols);
    }
    s.next_inserted = r.u64()?;
    s.next_inserted_col = r.u32()?;
    Ok(s)
}

impl Workbook {
    /// Write the journal of every sheet (on a snapshot, off the UI thread).
    pub fn write_journal(&self, header: &JournalHeader, out: impl Write) -> io::Result<()> {
        let mut w = Writer {
            inner: io::BufWriter::new(out),
            hash: Fnv::new(),
        };
        write_header(&mut w, header)?;
        w.u32(self.sheets().len() as u32)?;
        let active = self
            .index_of(self.active_id())
            .expect("the active sheet exists");
        w.u32(active as u32)?;
        for s in self.sheets() {
            w.u32(s.id.0)?;
            w.blob(s.name.as_bytes())?;
            match s.origin {
                Origin::Csv => w.u8(0)?,
                Origin::Xlsx(k) => {
                    w.u8(1)?;
                    w.u32(k)?;
                }
                Origin::New => w.u8(2)?,
            }
            w.u8(u8::from(s.table.has_header()))?;
            write_state(&mut w, &s.table)?;
        }
        let hash = w.hash.0;
        w.inner.write_all(&hash.to_le_bytes())?;
        w.inner.flush()
    }

    /// The steps that turn this freshly opened workbook into the journaled one: sheets the
    /// journal lacks removed, sheets added in the crashed session put back (empty, then
    /// restored), names and order as journaled, and every sheet's edits as
    /// [`Edit::Restore`]. Apply them in order (one undo step). Each state is checked first
    /// ([`CsvTable::check_edit_state`]) against the sheet with the same origin, so nothing is
    /// half-applied. Header-row flags aren't part of the plan: the caller sets them from
    /// `state`, and activates the sheet at `state.active`.
    pub fn restore_plan(&self, state: BookState) -> Result<Vec<BookEdit>, JournalError> {
        let untitled = CsvTable::untitled();
        // The fresh sheet each journal sheet becomes (`None`: added in the session).
        let mut matched: Vec<Option<SheetId>> = Vec::with_capacity(state.sheets.len());
        let mut used = vec![false; self.sheets().len()];
        for s in &state.sheets {
            if s.origin == Origin::New {
                untitled.check_edit_state(&s.state)?;
                matched.push(None);
                continue;
            }
            let Some(i) =
                (0..used.len()).find(|&i| !used[i] && self.sheets()[i].origin == s.origin)
            else {
                return Err(JournalError::Mismatch(
                    "a sheet in the journal isn't in the file",
                ));
            };
            used[i] = true;
            self.sheets()[i].table.check_edit_state(&s.state)?;
            matched.push(Some(self.sheets()[i].id));
        }

        let mut plan = Vec::new();
        // The sheets, in order, as the plan so far leaves them.
        let mut order: Vec<SheetId> = self.sheets().iter().map(|s| s.id).collect();
        // Added sheets first, at the end, so removing never empties the workbook.
        let mut next = self.sheets().iter().map(|s| s.id.0 + 1).max().unwrap_or(0);
        let mut ids = Vec::with_capacity(state.sheets.len());
        for (s, m) in state.sheets.iter().zip(&matched) {
            let id = m.unwrap_or_else(|| {
                let id = SheetId(next);
                next += 1;
                plan.push(BookEdit::Insert {
                    at: order.len(),
                    sheet: Box::new(Sheet {
                        id,
                        name: s.name.clone(),
                        table: CsvTable::untitled(),
                        origin: Origin::New,
                        cached: Default::default(),
                    }),
                });
                order.push(id);
                id
            });
            ids.push(id);
        }
        for at in (0..used.len()).rev().filter(|&i| !used[i]) {
            plan.push(BookEdit::Remove { at });
            order.remove(at);
        }
        for (to, id) in ids.iter().enumerate() {
            let from = order
                .iter()
                .position(|o| o == id)
                .expect("every journal sheet is in the book");
            if from != to {
                plan.push(BookEdit::Move { from, to });
                let id = order.remove(from);
                order.insert(to, id);
            }
        }
        let mut restores = Vec::new();
        for ((s, m), &id) in state.sheets.into_iter().zip(&matched).zip(&ids) {
            if m.is_some() && self.sheet(id).is_some_and(|f| f.name != s.name) {
                plan.push(BookEdit::Rename { id, name: s.name });
            }
            if !s.state.is_empty() {
                restores.push((id, Edit::Restore(Box::new(s.state))));
            }
        }
        if !restores.is_empty() {
            plan.push(BookEdit::Sheets(restores));
        }
        Ok(plan)
    }
}

impl CsvTable {
    /// Whether `state` fits this table: rows and columns it names exist (each row once),
    /// and inserted ones are below its counters. Needs the whole file indexed.
    pub fn check_edit_state(&self, s: &EditState) -> Result<(), JournalError> {
        use JournalError::Mismatch;
        if !self.index.is_complete() {
            return Err(Mismatch("the file isn't indexed yet"));
        }
        let file_rows = self.index.row_count();
        let row_ok = |id: RowId| {
            if id.is_inserted() {
                id.0 & !(1 << 63) < s.next_inserted
            } else {
                id.0 < file_rows
            }
        };
        let col_ok = |id: ColId| !id.is_inserted() || id.0 & !(1 << 31) < s.next_inserted_col;
        for (id, cells) in s.overlay.iter_rows() {
            if !row_ok(*id) || !cells.keys().all(|&c| col_ok(c)) {
                return Err(Mismatch(
                    "an edit names a row or column the file doesn't have",
                ));
            }
        }
        if let Some(order) = &s.rows {
            let mut seen = vec![0u64; (file_rows as usize).div_ceil(64)];
            let mut bad = false;
            order.for_each(|run| {
                for i in 0..run.len {
                    let id = RowId(run.first.0 + i);
                    if !row_ok(id) {
                        bad = true;
                    } else if !id.is_inserted() {
                        let (w, b) = ((id.0 / 64) as usize, id.0 % 64);
                        bad |= seen[w] >> b & 1 == 1;
                        seen[w] |= 1 << b;
                    }
                }
            });
            if bad {
                return Err(Mismatch(
                    "the row order names a row twice or one the file lacks",
                ));
            }
        }
        if let Some(m) = &s.cols {
            if !m.parts().0.iter().all(|&c| col_ok(c)) {
                return Err(Mismatch(
                    "the column order names a column that doesn't exist",
                ));
            }
        }
        if s.cleared
            .blocks()
            .any(|(ranges, _)| ranges.iter().any(|r| r.end > file_rows))
        {
            return Err(Mismatch("a cleared block reaches past the last row"));
        }
        Ok(())
    }

    pub(crate) fn take_edit_state(&mut self) -> EditState {
        EditState {
            overlay: std::mem::take(&mut self.overlay),
            rows: self.rows.take(),
            cols: self.cols.take(),
            cleared: std::mem::take(&mut self.cleared),
            next_inserted: self.next_inserted,
            next_inserted_col: self.next_inserted_col,
        }
    }

    pub(crate) fn put_edit_state(&mut self, s: EditState) {
        self.overlay = s.overlay;
        self.rows = s.rows;
        self.cols = s.cols;
        self.cleared = s.cleared;
        // Counters only grow, so an undone restore can't hand out an id twice.
        self.next_inserted = self.next_inserted.max(s.next_inserted);
        self.next_inserted_col = self.next_inserted_col.max(s.next_inserted_col);
    }
}

/// Read a whole journal (either version), after checking the hash: its header and the
/// workbook state. A v0.1.x journal reads as one sheet, named after its file.
pub fn read_book_journal(r: impl Read) -> Result<(JournalHeader, BookState), JournalError> {
    let mut r = Reader::new(io::BufReader::new(r));
    let (header, v1) = read_header_from(&mut r)?;
    let book = if v1 {
        let name = header
            .path
            .as_deref()
            .and_then(|p| p.file_stem())
            .map_or_else(|| "Sheet1".to_owned(), |s| s.to_string_lossy().into_owned());
        BookState {
            active: 0,
            sheets: vec![SheetState {
                id: SheetId(0),
                name,
                origin: if header.path.is_some() {
                    Origin::Csv
                } else {
                    Origin::New
                },
                has_header: header.has_header,
                state: read_state(&mut r)?,
            }],
        }
    } else {
        read_sheets(&mut r)?
    };
    let want = r.hash.0;
    let mut got = [0; 8];
    r.inner.read_exact(&mut got)?;
    if u64::from_le_bytes(got) != want {
        return Err(JournalError::Corrupt("checksum mismatch"));
    }
    if r.inner.read(&mut [0])? != 0 {
        return Err(JournalError::Corrupt("trailing bytes"));
    }
    Ok((header, book))
}

fn read_sheets<R: Read>(r: &mut Reader<R>) -> Result<BookState, JournalError> {
    let count = r.u32()?;
    if count == 0 || count > MAX_SHEETS {
        return Err(JournalError::Corrupt("impossible sheet count"));
    }
    let active = r.u32()?;
    if active >= count {
        return Err(JournalError::Corrupt("the active sheet isn't there"));
    }
    let mut sheets: Vec<SheetState> = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let id = SheetId(r.u32()?);
        if sheets.iter().any(|s| s.id == id) {
            return Err(JournalError::Corrupt("two sheets share an id"));
        }
        let name = String::from_utf8(r.blob()?)
            .map_err(|_| JournalError::Corrupt("a sheet name isn't UTF-8"))?;
        let origin = match r.u8()? {
            0 => Origin::Csv,
            1 => Origin::Xlsx(r.u32()?),
            2 => Origin::New,
            _ => return Err(JournalError::Corrupt("unknown sheet origin")),
        };
        let has_header = r.u8()? == 1;
        let state = read_state(r)?;
        sheets.push(SheetState {
            id,
            name,
            origin,
            has_header,
            state,
        });
    }
    Ok(BookState { active, sheets })
}
