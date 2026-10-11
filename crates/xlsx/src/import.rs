//! XLSX import (ADR 0006): each worksheet's cells are streamed into a CSV copy in the cache
//! directory, which is then mapped and indexed like any CSV file. A formula cell's raw text
//! is its formula (`=SUM(A1:A3)`); the result the file stored is kept beside it.

use calamine::{
    CellErrorType, Data, DataRef, Dimensions, ExcelDateTime, Reader, SheetType, Xlsx, XlsxError,
};
use csv_engine::{encode_field, Dialect, IndexError, LineEnding, Source, SparseRowIndex};
use data_model::{is_formula, CachedResults, CellRef, ColId, CsvTable, FormulaCells, RowId};
use formula::scalar::{civil_from_serial, serial_from_civil};
use formula::ErrorKind;
use std::fmt;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

/// How imported copies are written: comma-separated, `"`-quoted, LF, no header.
pub(crate) const DIALECT: Dialect = Dialect {
    delimiter: b',',
    quote: b'"',
    has_header: false,
    line_ending: LineEnding::Lf,
};

/// Cells between cancel checks and progress updates.
const CHECK_CELLS: u32 = 4_096;

/// A sheet delivered out of row-major order is read whole into memory instead, up to this
/// many cells.
const DENSE_MAX_CELLS: u64 = 50_000_000;

/// A running import's progress, shared with the UI: rows read so far, of the rows the
/// sheets declare.
#[derive(Debug, Default)]
pub struct Progress {
    pub rows: AtomicU64,
    pub rows_total: AtomicU64,
}

/// One worksheet, ready to show.
pub struct ImportedSheet {
    pub name: String,
    /// Position among the file's worksheets ([`data_model::Origin::Xlsx`]).
    pub index: u32,
    /// The sheet's copy, fully indexed, its formula cells set.
    pub table: CsvTable,
    /// The formula cells of the copy, complete.
    pub formulas: FormulaCells,
    /// The result text the file stored for each formula cell.
    pub cached: CachedResults,
}

/// A whole workbook.
pub struct Imported {
    pub sheets: Vec<ImportedSheet>,
    /// Defined names and their formulas, without `=`; Excel's built-in `_xlnm.` names
    /// (print areas and the like) left out.
    pub names: Vec<(String, String)>,
}

#[derive(Debug)]
pub enum ImportError {
    Io(io::Error),
    /// The workbook can't be read.
    Xlsx(String),
    Cancelled,
}

impl From<io::Error> for ImportError {
    fn from(e: io::Error) -> Self {
        ImportError::Io(e)
    }
}

impl From<XlsxError> for ImportError {
    fn from(e: XlsxError) -> Self {
        match e {
            XlsxError::Io(e) => ImportError::Io(e),
            e => ImportError::Xlsx(e.to_string()),
        }
    }
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::Io(e) => write!(f, "{e}"),
            ImportError::Xlsx(m) => write!(f, "{m}"),
            ImportError::Cancelled => write!(f, "opening was cancelled"),
        }
    }
}

impl std::error::Error for ImportError {}

pub type Result<T, E = ImportError> = std::result::Result<T, E>;

/// Read every worksheet of the workbook at `path`. Setting `cancel` stops it with
/// [`ImportError::Cancelled`].
pub fn import(path: &Path, cancel: &AtomicBool, progress: &Progress) -> Result<Imported> {
    let mut book: Xlsx<BufReader<File>> = calamine::open_workbook(path)?;
    let names = book
        .defined_names()
        .iter()
        .filter(|(name, _)| !name.starts_with("_xlnm."))
        .map(|(name, def)| {
            (
                name.clone(),
                def.strip_prefix('=').unwrap_or(def).to_string(),
            )
        })
        .collect();
    let worksheets: Vec<String> = book
        .sheets_metadata()
        .iter()
        .filter(|s| s.typ == SheetType::WorkSheet)
        .map(|s| s.name.clone())
        .collect();
    let mut sheets = Vec::with_capacity(worksheets.len());
    for (index, name) in worksheets.into_iter().enumerate() {
        let (table, formulas, cached) = import_sheet(&mut book, &name, cancel, progress)?;
        sheets.push(ImportedSheet {
            name,
            index: index as u32,
            table,
            formulas,
            cached,
        });
    }
    Ok(Imported { sheets, names })
}

fn import_sheet(
    book: &mut Xlsx<BufReader<File>>,
    name: &str,
    cancel: &AtomicBool,
    progress: &Progress,
) -> Result<(CsvTable, FormulaCells, CachedResults)> {
    let copy = csv_engine::scratch_path()?;
    let result = write_copy(book, name, &copy, cancel, progress).and_then(|copied| {
        let source = Source::open(&copy)?;
        Ok((source, copied))
    });
    let _ = std::fs::remove_file(&copy); // the mapping stays valid after unlink
    let (source, (cells, cached)) = result?;
    let index = SparseRowIndex::new(Arc::new(source), &DIALECT);
    index.build(cancel).map_err(|e| match e {
        IndexError::Cancelled => ImportError::Cancelled,
        e => ImportError::Xlsx(format!("sheet “{name}”: {e}")),
    })?;
    let mut table = CsvTable::new(index, DIALECT);
    let formulas = FormulaCells::new(cells);
    table.set_formula_cells(formulas.clone());
    Ok((table, formulas, cached))
}

/// Write sheet `name` as CSV to `path`: streamed in file order, or read whole if the file
/// stores its cells out of order.
fn write_copy(
    book: &mut Xlsx<BufReader<File>>,
    name: &str,
    path: &Path,
    cancel: &AtomicBool,
    progress: &Progress,
) -> Result<(Vec<(u64, u32)>, CachedResults)> {
    let base = progress.rows.load(Ordering::Relaxed);
    let mut reader = book.worksheet_cells_reader(name)?;
    let dims = reader.dimensions();
    progress
        .rows_total
        .fetch_add(u64::from(dims.end.0) + 1, Ordering::Relaxed);
    let mut copy = SheetCopy::create(path, cancel, progress, base)?;
    while let Some(cell) = reader.next_cell_with_formula()? {
        let (row, col) = cell.pos;
        if !copy.in_order(row, col) {
            drop(reader);
            drop(copy); // flushed before the dense pass truncates the file
            return write_dense(book, name, dims, path, cancel, progress, base);
        }
        copy.put(row, col, &cell.value, cell.formula.as_deref())?;
    }
    copy.finish()
}

/// The fallback for a sheet whose cells arrive out of row-major order: read its values and
/// formulas whole, then write them in order.
fn write_dense(
    book: &mut Xlsx<BufReader<File>>,
    name: &str,
    dims: Dimensions,
    path: &Path,
    cancel: &AtomicBool,
    progress: &Progress,
    base: u64,
) -> Result<(Vec<(u64, u32)>, CachedResults)> {
    let out_of_order = || ImportError::Xlsx(format!("cells out of order in sheet “{name}”"));
    if dims.len() > DENSE_MAX_CELLS {
        return Err(out_of_order());
    }
    let values = book.worksheet_range(name)?;
    let formulas = book.worksheet_formula(name)?;
    let mut copy = SheetCopy::create(path, cancel, progress, base)?;
    let bounds = [
        values.start(),
        values.end(),
        formulas.start(),
        formulas.end(),
    ];
    let (Some(first_row), Some(last_row), Some(first_col), Some(last_col)) = (
        bounds.iter().flatten().map(|p| p.0).min(),
        bounds.iter().flatten().map(|p| p.0).max(),
        bounds.iter().flatten().map(|p| p.1).min(),
        bounds.iter().flatten().map(|p| p.1).max(),
    ) else {
        return copy.finish();
    };
    if (u64::from(last_row - first_row) + 1) * (u64::from(last_col - first_col) + 1)
        > DENSE_MAX_CELLS
    {
        return Err(out_of_order());
    }
    for row in first_row..=last_row {
        for col in first_col..=last_col {
            let value = values
                .get_value((row, col))
                .map_or(DataRef::Empty, data_ref);
            let formula = formulas.get_value((row, col)).map(String::as_str);
            copy.put(row, col, &value, formula)?;
        }
    }
    copy.finish()
}

fn data_ref(d: &Data) -> DataRef<'_> {
    match d {
        Data::Int(i) => DataRef::Int(*i),
        Data::Float(f) => DataRef::Float(*f),
        Data::String(s) => DataRef::SharedString(s),
        Data::Bool(b) => DataRef::Bool(*b),
        Data::DateTime(dt) => DataRef::DateTime(*dt),
        Data::DateTimeIso(s) => DataRef::DateTimeIso(s.clone()),
        Data::DurationIso(s) => DataRef::DurationIso(s.clone()),
        Data::Error(e) => DataRef::Error(e.clone()),
        Data::Empty => DataRef::Empty,
    }
}

/// A sheet's CSV copy being written, cell by cell in row-major order.
struct SheetCopy<'a> {
    out: BufWriter<File>,
    /// The row being written, and whether any cell was written yet.
    row: u32,
    started: bool,
    /// Delimiters written in the current row: the column the next field starts at.
    commas: u32,
    /// The last cell seen, for the order check.
    last: Option<(u32, u32)>,
    field: Vec<u8>,
    formulas: Vec<(u64, u32)>,
    cached: CachedResults,
    cells: u32,
    cancel: &'a AtomicBool,
    progress: &'a Progress,
    base: u64,
}

impl<'a> SheetCopy<'a> {
    fn create(
        path: &Path,
        cancel: &'a AtomicBool,
        progress: &'a Progress,
        base: u64,
    ) -> io::Result<Self> {
        progress.rows.store(base, Ordering::Relaxed);
        Ok(Self {
            out: BufWriter::with_capacity(1 << 20, File::create(path)?),
            row: 0,
            started: false,
            commas: 0,
            last: None,
            field: Vec::new(),
            formulas: Vec::new(),
            cached: CachedResults::new(),
            cells: 0,
            cancel,
            progress,
            base,
        })
    }

    fn in_order(&self, row: u32, col: u32) -> bool {
        self.last.is_none_or(|last| (row, col) > last)
    }

    fn put(&mut self, row: u32, col: u32, value: &DataRef, formula: Option<&str>) -> Result<()> {
        self.last = Some((row, col));
        self.cells += 1;
        if self.cells.is_multiple_of(CHECK_CELLS) {
            if self.cancel.load(Ordering::Relaxed) {
                return Err(ImportError::Cancelled);
            }
            self.progress
                .rows
                .store(self.base + u64::from(row), Ordering::Relaxed);
        }
        let raw = match formula.filter(|f| !f.is_empty()) {
            Some(f) => {
                let f = f.strip_prefix('=').unwrap_or(f);
                let cached = value_text(value);
                if !cached.is_empty() {
                    let at = CellRef {
                        row: RowId::source(u64::from(row)),
                        col: ColId::source(col),
                    };
                    self.cached.insert(at, cached.into());
                }
                match formula::parse(f) {
                    Ok(e) => format!("={}", formula::render(&e)),
                    Err(_) => format!("={f}"),
                }
            }
            None => value_text(value),
        };
        if raw.is_empty() {
            return Ok(());
        }
        if is_formula(&raw) {
            self.formulas.push((u64::from(row), col));
        }
        self.started = true;
        while self.row < row {
            self.out.write_all(b"\n")?;
            self.row += 1;
            self.commas = 0;
        }
        while self.commas < col {
            self.out.write_all(b",")?;
            self.commas += 1;
        }
        self.field.clear();
        encode_field(raw.as_bytes(), &DIALECT, &mut self.field);
        self.out.write_all(&self.field)?;
        Ok(())
    }

    fn finish(mut self) -> Result<(Vec<(u64, u32)>, CachedResults)> {
        if self.started {
            self.out.write_all(b"\n")?;
        }
        self.out.flush()?;
        self.progress
            .rows
            .store(self.base + u64::from(self.row) + 1, Ordering::Relaxed);
        Ok((self.formulas, self.cached))
    }
}

/// A cell value as Bricks' raw text: numbers in plain decimal, booleans as `TRUE`/`FALSE`,
/// dates as ISO text, errors as their literal (`#N/A`).
pub fn value_text(v: &DataRef) -> String {
    match v {
        DataRef::Int(i) => i.to_string(),
        DataRef::Float(f) => float_text(*f),
        DataRef::String(s) => s.clone(),
        DataRef::SharedString(s) => (*s).to_string(),
        DataRef::Bool(b) => (if *b { "TRUE" } else { "FALSE" }).to_string(),
        DataRef::DateTime(dt) => datetime_text(dt),
        DataRef::DateTimeIso(s) | DataRef::DurationIso(s) => s.clone(),
        DataRef::Error(e) => error_kind(e).text().to_string(),
        DataRef::Empty => String::new(),
    }
}

fn float_text(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{f:.0}")
    } else {
        format!("{f}")
    }
}

fn error_kind(e: &CellErrorType) -> ErrorKind {
    match e {
        CellErrorType::Div0 => ErrorKind::Div0,
        CellErrorType::NA => ErrorKind::NA,
        CellErrorType::Name => ErrorKind::Name,
        CellErrorType::Null => ErrorKind::Null,
        CellErrorType::Num => ErrorKind::Num,
        CellErrorType::Ref => ErrorKind::Ref,
        CellErrorType::Value => ErrorKind::Value,
        CellErrorType::GettingData => ErrorKind::NA,
    }
}

const DAY_SECONDS: i64 = 86_400;

fn datetime_text(dt: &ExcelDateTime) -> String {
    let serial = dt.as_f64();
    if dt.is_duration() {
        let total = (serial * DAY_SECONDS as f64).round() as i64;
        let sign = if total < 0 { "-" } else { "" };
        let total = total.abs();
        return format!(
            "{sign}{}:{:02}:{:02}",
            total / 3600,
            total / 60 % 60,
            total % 60
        );
    }
    if serial < 0.0 {
        return float_text(serial);
    }
    let (y, mo, d, h, mi, s, ms) = dt.to_ymd_hms_milli();
    let mut secs = i64::from(h) * 3600 + i64::from(mi) * 60 + i64::from(s) + i64::from(ms >= 500);
    if serial < 1.0 {
        let secs = secs.min(DAY_SECONDS - 1);
        return format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60);
    }
    let (mut y, mut mo, mut d) = (i32::from(y), u32::from(mo), u32::from(d));
    if secs >= DAY_SECONDS {
        secs -= DAY_SECONDS;
        (y, mo, d) = civil_from_serial(serial_from_civil(y, mo, d) + 1);
    }
    if secs == 0 {
        format!("{y:04}-{mo:02}-{d:02}")
    } else {
        format!(
            "{y:04}-{mo:02}-{d:02} {:02}:{:02}:{:02}",
            secs / 3600,
            secs / 60 % 60,
            secs % 60
        )
    }
}
