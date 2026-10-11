//! XLSX export: every sheet's cells as typed values (numbers, booleans, dates, text),
//! formulas with their results, defined names, and column widths. A cell is written typed
//! only when its text is exactly how [`crate::import`] reads that value back, so a saved
//! workbook reopens with every cell's raw text unchanged (invariant 1).

use crate::import::value_text;
use calamine::{DataRef, Reader, Xlsx};
use data_model::{is_formula, CellRef, Computed, CsvTable, RowBlock, SaveProgress, SaveStats};
use file_format::SaveError;
use formula::{literal, Value};
use rust_xlsxwriter::{ExcelDateTime, Format, Formula, Workbook, Worksheet, XlsxError};
use std::cell::RefCell;
use std::fs::File;
use std::io::{self, BufReader, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// Rows an .xlsx sheet holds.
pub const MAX_ROWS: u64 = 1_048_576;
/// Columns an .xlsx sheet holds.
pub const MAX_COLS: u32 = 16_384;

/// Rows read per chunk, and between cancel checks.
const CHUNK: u64 = 65_536;

/// One sheet to write.
pub struct ExportSheet {
    pub name: String,
    /// A snapshot of the sheet, with no computed results set (raw text is written).
    pub table: CsvTable,
    /// Every formula cell's result text ([`data_model::Workbook::computed_for`]), stored
    /// in the file beside the formula.
    pub computed: Computed,
    /// `(view column, width in pixels)` for columns the user sized.
    pub widths: Vec<(u32, u32)>,
    /// The sheet shown when the file opens.
    pub active: bool,
}

/// Check that every sheet, `(name, rows, columns)`, fits an .xlsx sheet; the error names
/// the first one that doesn't.
pub fn check_limits(sheets: &[(String, u64, u32)]) -> Result<(), String> {
    for (name, rows, cols) in sheets {
        if *rows > MAX_ROWS {
            return Err(format!(
                "“{name}” has {} rows; an .xlsx sheet holds at most {}",
                group_digits(*rows),
                group_digits(MAX_ROWS)
            ));
        }
        if *cols > MAX_COLS {
            return Err(format!(
                "“{name}” has {} columns; an .xlsx sheet holds at most {}",
                group_digits(u64::from(*cols)),
                group_digits(u64::from(MAX_COLS))
            ));
        }
    }
    Ok(())
}

fn group_digits(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// What [`write_book`] wrote of each sheet, for verification: its name and the last
/// non-empty row.
type Layout = Vec<(String, Option<u64>)>;

/// Write `sheets` and the defined `names` as an .xlsx workbook to `out`. `progress` counts
/// rows read; setting `cancel` stops with [`io::ErrorKind::Interrupted`].
pub fn write_xlsx(
    out: &mut (dyn Write + Send),
    sheets: Vec<ExportSheet>,
    names: &[(String, String)],
    progress: &SaveProgress,
    cancel: &AtomicBool,
) -> io::Result<SaveStats> {
    write_book(out, sheets, names, progress, cancel).map(|(stats, _)| stats)
}

fn write_book(
    out: &mut (dyn Write + Send),
    sheets: Vec<ExportSheet>,
    names: &[(String, String)],
    progress: &SaveProgress,
    cancel: &AtomicBool,
) -> io::Result<(SaveStats, Layout)> {
    let mut book = Workbook::new();
    let scratch = csv_engine::scratch_path()?;
    let dir = scratch.parent().expect("a scratch path is in a directory");
    book.set_tempdir(dir).map_err(xlsx_error)?;
    let formats = Formats {
        date: Format::new().set_num_format("yyyy-mm-dd"),
        datetime: Format::new().set_num_format("yyyy-mm-dd hh:mm:ss"),
    };
    let mut layout = Vec::with_capacity(sheets.len());
    let mut rows = 0;
    for mut sheet in sheets {
        let ws = book.add_worksheet_with_constant_memory();
        ws.set_name(&sheet.name)
            .map_err(|e| sheet_error(&sheet.name, e))?;
        for &(col, px) in &sheet.widths {
            let col = u16::try_from(col).map_err(|_| too_many_cols(&sheet.name))?;
            ws.set_column_width_pixels(col, px)
                .map_err(|e| sheet_error(&sheet.name, e))?;
        }
        if sheet.active {
            ws.set_active(true);
        }
        let last = write_cells(ws, &mut sheet, &formats, progress, cancel)?;
        rows += sheet.table.total_rows();
        layout.push((sheet.name, last));
    }
    for (name, def) in names {
        book.define_name(name.as_str(), &format!("={def}"))
            .map_err(|e| io::Error::other(format!("name “{name}”: {e}")))?;
    }
    let mut counted = Counted {
        inner: out,
        bytes: 0,
    };
    book.save_to_writer(&mut counted).map_err(xlsx_error)?;
    Ok((
        SaveStats {
            rows,
            bytes: counted.bytes,
        },
        layout,
    ))
}

struct Formats {
    date: Format,
    datetime: Format,
}

/// Write one sheet's cells in row order; returns the last non-empty row.
fn write_cells(
    ws: &mut Worksheet,
    sheet: &mut ExportSheet,
    formats: &Formats,
    progress: &SaveProgress,
    cancel: &AtomicBool,
) -> io::Result<Option<u64>> {
    let total = sheet.table.total_rows();
    let mut block = RowBlock::full_text();
    let mut last = None;
    let mut start = 0;
    while start < total {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "save cancelled"));
        }
        let end = (start + CHUNK).min(total);
        sheet.table.read_all(start..end, &mut block);
        for r in start..end {
            for c in 0..block.cells_in_row(r) {
                let text = block.cell(r, c).expect("a cell of the row");
                if text.is_empty() {
                    continue;
                }
                if r >= MAX_ROWS {
                    return Err(io::Error::other(format!(
                        "“{}” has more rows than an .xlsx sheet holds",
                        sheet.name
                    )));
                }
                if c >= MAX_COLS {
                    return Err(too_many_cols(&sheet.name));
                }
                write_cell(ws, sheet, r, c, text, formats)
                    .map_err(|e| sheet_error(&sheet.name, e))?;
                last = Some(r);
            }
        }
        progress.add_written(end - start);
        start = end;
    }
    Ok(last)
}

fn write_cell(
    ws: &mut Worksheet,
    sheet: &ExportSheet,
    r: u64,
    c: u32,
    text: &str,
    formats: &Formats,
) -> Result<(), XlsxError> {
    let (row, col) = (r as u32, c as u16);
    if is_formula(text) {
        if let Ok(expr) = formula::parse(&text[1..]) {
            let at = CellRef {
                row: sheet.table.id_at(r),
                col: sheet.table.col_id(c),
            };
            let f = Formula::new(format!("={}", formula::render_xlsx(&expr)))
                .set_result(sheet.computed.get(at).unwrap_or(""));
            ws.write_formula(row, col, f)?;
            return Ok(());
        }
    }
    match literal(text) {
        Value::Number(n) if value_text(&DataRef::Float(n)) == text => {
            ws.write_number(row, col, n)?;
        }
        Value::Bool(b) if text == "TRUE" || text == "FALSE" => {
            ws.write_boolean(row, col, b)?;
        }
        Value::Date(_) => match canonical_date(text) {
            Some((dt, false)) => {
                ws.write_datetime_with_format(row, col, dt, &formats.date)?;
            }
            Some((dt, true)) => {
                ws.write_datetime_with_format(row, col, dt, &formats.datetime)?;
            }
            None => {
                ws.write_string(row, col, text)?;
            }
        },
        _ => {
            ws.write_string(row, col, text)?;
        }
    }
    Ok(())
}

/// `YYYY-MM-DD` or `YYYY-MM-DD HH:MM:SS` exactly, years 1900 to 9999, as a datetime and
/// whether it has a time: the forms import gives back for a date cell.
fn canonical_date(text: &str) -> Option<(ExcelDateTime, bool)> {
    let b = text.as_bytes();
    let digits = |r: std::ops::Range<usize>| -> Option<u16> {
        b[r].iter().try_fold(0u16, |n, &d| {
            d.is_ascii_digit().then(|| n * 10 + u16::from(d - b'0'))
        })
    };
    if !(b.len() == 10 || b.len() == 19) || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let (y, m, d) = (digits(0..4)?, digits(5..7)?, digits(8..10)?);
    if !(1900..=9999).contains(&y) {
        return None;
    }
    let date = ExcelDateTime::from_ymd(y, m as u8, d as u8).ok()?;
    if b.len() == 10 {
        return Some((date, false));
    }
    if b[10] != b' ' || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let (h, mi, s) = (digits(11..13)?, digits(14..16)?, digits(17..19)?);
    if (h, mi, s) == (0, 0, 0) {
        return None; // import shows midnight as the date alone
    }
    Some((date.and_hms(h, mi as u8, s as u8).ok()?, true))
}

/// Save `sheets` as an .xlsx workbook at `dest`, atomically (invariant 6). Verification
/// reopens the written file and checks its sheet names and each sheet's row count.
pub fn save_xlsx(
    dest: &Path,
    sheets: Vec<ExportSheet>,
    names: &[(String, String)],
    progress: &SaveProgress,
    cancel: &AtomicBool,
) -> Result<SaveStats, SaveError> {
    let written = RefCell::new(None);
    let saved = file_format::save_atomic(
        dest,
        cancel,
        |w| {
            *written.borrow_mut() = Some(write_book(w, sheets, names, progress, cancel)?);
            Ok(())
        },
        |tmp| {
            progress.set_checking();
            let written = written.borrow();
            let (_, layout) = written.as_ref().expect("write ran before verify");
            verify(tmp, layout)
                .map_err(|m| format!("the saved workbook reads back differently: {m}"))
        },
    );
    match saved {
        Ok(()) => Ok(written.take().expect("save succeeded").0),
        Err(_) if cancel.load(Ordering::Relaxed) => Err(SaveError::Cancelled),
        Err(e) => Err(e),
    }
}

fn verify(path: &Path, layout: &Layout) -> Result<(), String> {
    let mut book: Xlsx<BufReader<File>> =
        calamine::open_workbook(path).map_err(|e: calamine::XlsxError| e.to_string())?;
    let names = book.sheet_names();
    let expected: Vec<&str> = layout.iter().map(|(n, _)| n.as_str()).collect();
    if names != expected {
        return Err(format!("sheets {names:?}, expected {expected:?}"));
    }
    for (name, last) in layout {
        let reader = book
            .worksheet_cells_reader(name)
            .map_err(|e| e.to_string())?;
        let rows = u64::from(reader.dimensions().end.0) + 1;
        // An empty sheet's dimension is A1.
        let want = last.map_or(1, |r| r + 1);
        if rows != want {
            return Err(format!("“{name}” has {rows} rows, {want} written"));
        }
    }
    Ok(())
}

fn xlsx_error(e: XlsxError) -> io::Error {
    match e {
        XlsxError::IoError(e) => e,
        e => io::Error::other(e.to_string()),
    }
}

fn sheet_error(name: &str, e: XlsxError) -> io::Error {
    match e {
        XlsxError::IoError(e) => e,
        e => io::Error::other(format!("“{name}”: {e}")),
    }
}

fn too_many_cols(name: &str) -> io::Error {
    io::Error::other(format!(
        "“{name}” has more columns than an .xlsx sheet holds"
    ))
}

/// Counts the bytes written through it.
struct Counted<'a> {
    inner: &'a mut (dyn Write + Send),
    bytes: u64,
}

impl Write for Counted<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.bytes += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
