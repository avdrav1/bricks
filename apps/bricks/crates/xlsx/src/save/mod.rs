//! Saving an .xlsx (ADR 0010): the original package is patched. Parts no change touches
//! are copied as compressed bytes; changed sheets, the styles the pool gained, and the
//! parts that list sheets are rewritten. A workbook with no package (from a CSV, or new)
//! is written from a minimal one.

mod cells;
mod parts;
mod sheet;
mod styles;

use crate::package::{self, Package};
use calamine::{Reader as _, Xlsx};
use csv_engine::Source;
use data_model::{
    ColId, Computed, CsvTable, Origin, RowId, SaveProgress, SaveStats, SheetId, StylePool, Workbook,
};
use formula::{adjust, Change};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use suite_file::{SaveError, WriteSeek};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

pub const MAX_ROWS: u64 = 1_048_576;
pub const MAX_COLS: u32 = 16_384;

const PART_CALC_CHAIN: &str = "/calcChain";
const REL_VBA: &str = "/vbaProject";

/// One sheet to save.
pub struct SaveSheet {
    pub name: String,
    /// A snapshot of the sheet, with no computed results set (raw text is written).
    pub table: CsvTable,
    /// Every formula cell's result text ([`data_model::Workbook::computed_for`]), stored
    /// beside the formula.
    pub computed: Computed,
    /// The worksheet of the package the sheet came from ([`data_model::Origin::Xlsx`]).
    pub origin: Option<u32>,
    /// The sheet shown when the file opens.
    pub active: bool,
    /// Column widths the user set, in characters, and row heights, in points.
    pub widths: Vec<(ColId, f64)>,
    pub heights: Vec<(RowId, f64)>,
}

impl SaveSheet {
    /// The sheet differs from its part in the package.
    fn changed(&self) -> bool {
        self.table.changes() > 0 || !self.widths.is_empty() || !self.heights.is_empty()
    }
}

/// A whole workbook to save.
pub struct SaveBook {
    pub sheets: Vec<SaveSheet>,
    /// Defined names and their formulas, without `=`.
    pub names: Vec<(String, String)>,
    pub styles: Arc<StylePool>,
    /// The file the workbook was read from, if it came from one.
    pub package: Option<Arc<Package>>,
}

/// Sizes the user set on a sheet, in Excel's units.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SheetSizes {
    /// Column widths in characters of the default font's digit.
    pub widths: Vec<(ColId, f64)>,
    /// Row heights in points.
    pub heights: Vec<(RowId, f64)>,
}

impl SaveBook {
    /// Everything a save writes of `book` (a snapshot, on a worker): each sheet with its
    /// formula results. `None` once `cancel` is set. `done` counts formula cells computed.
    pub fn from_workbook(
        mut book: Workbook,
        package: Option<Arc<Package>>,
        sizes: &HashMap<SheetId, SheetSizes>,
        cancel: Arc<AtomicBool>,
        done: &AtomicU64,
    ) -> Option<SaveBook> {
        let ids: Vec<SheetId> = book.sheets().iter().map(|s| s.id).collect();
        let mut sheets = Vec::with_capacity(ids.len());
        for id in ids {
            let computed = book.computed_for(id, cancel.clone(), done)?;
            let active = id == book.active_id();
            let sheet = book.sheet(id).expect("a sheet of the book");
            let sizes = sizes.get(&id).cloned().unwrap_or_default();
            sheets.push(SaveSheet {
                name: sheet.name.clone(),
                table: sheet.table.snapshot(),
                computed,
                origin: match sheet.origin {
                    Origin::Xlsx(k) => Some(k),
                    Origin::Csv | Origin::New => None,
                },
                active,
                widths: sizes.widths,
                heights: sizes.heights,
            });
        }
        Some(SaveBook {
            sheets,
            names: book.names().to_vec(),
            styles: book.styles().clone(),
            package,
        })
    }
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

/// Save `book` as an .xlsx workbook at `dest`, atomically (invariant 6). An `.xlsm`
/// destination keeps the package's macros; an `.xlsx` one drops them. Verification
/// reopens the written file and checks its sheet names and each written sheet's rows.
pub fn save(
    dest: &Path,
    book: SaveBook,
    progress: &SaveProgress,
    cancel: &AtomicBool,
) -> Result<SaveStats, SaveError> {
    let macros = dest
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("xlsm"));
    let written = RefCell::new(None);
    let book = RefCell::new(Some(book));
    let saved = suite_file::save_atomic(
        dest,
        cancel,
        |w| {
            let book = book.borrow_mut().take().expect("written once");
            *written.borrow_mut() = Some(write_book(w, book, macros, progress, cancel)?);
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

/// Each sheet's name and, for those written, the rows its dimension gives.
type Layout = Vec<(String, Option<u64>)>;

fn options() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated)
}

fn zip_error(e: zip::result::ZipError) -> io::Error {
    match e {
        zip::result::ZipError::Io(e) => e,
        e => io::Error::other(e.to_string()),
    }
}

/// What the package is written to (LibreOffice can't read a zip streamed with data
/// descriptors, so each part's sizes go back into its header: the writer seeks).
type Out<'a> = &'a mut (dyn WriteSeek + Send);

fn write_book(
    out: Out<'_>,
    mut book: SaveBook,
    macros: bool,
    progress: &SaveProgress,
    cancel: &AtomicBool,
) -> io::Result<(SaveStats, Layout)> {
    let mut pool = (*book.styles).clone();
    let mut zip = ZipWriter::new(out);
    let rows = book.sheets.iter().map(|s| s.table.total_rows()).sum();
    let layout = match book.package.clone() {
        Some(pkg) => patch(
            &mut zip, &pkg, &mut book, &mut pool, macros, progress, cancel,
        )?,
        None => fresh(&mut zip, &mut book, &mut pool, progress, cancel)?,
    };
    let out = zip.finish().map_err(zip_error)?;
    let bytes = out.seek(SeekFrom::End(0))?;
    Ok((SaveStats { rows, bytes }, layout))
}

/// Part `part` of the package, unpacked to a scratch file and mapped.
fn unpacked(src: &mut package::Zip, part: &str) -> io::Result<Source> {
    let mut entry = src.by_name(part).map_err(zip_error)?;
    let path = csv_engine::scratch_path()?;
    let result = (|| {
        let mut file = BufWriter::with_capacity(1 << 20, File::create(&path)?);
        io::copy(&mut entry, &mut file)?;
        file.flush()?;
        drop(file);
        Source::open(&path)
    })();
    let _ = std::fs::remove_file(&path); // the mapping survives
    result
}

#[allow(clippy::too_many_arguments)]
fn patch(
    zip: &mut ZipWriter<Out<'_>>,
    pkg: &Package,
    book: &mut SaveBook,
    pool: &mut StylePool,
    macros: bool,
    progress: &SaveProgress,
    cancel: &AtomicBool,
) -> io::Result<Layout> {
    let parts = &pkg.parts;
    let mut src = package::open(pkg.path())?;
    let workbook_xml = package::read_part(&mut src, &parts.workbook)?.unwrap_or_default();
    let book_dir = parts
        .workbook
        .rsplit_once('/')
        .map_or("", |(d, _)| d)
        .to_owned();
    let book_rels_name = package::rels_name(&parts.workbook);
    let book_rels = package::rels(&mut src, &parts.workbook)?;
    let listed = parts::sheet_rel_ids(&workbook_xml)?;

    // Which of the file's worksheets each sheet is, which are new, which are gone.
    let kept: Vec<Option<usize>> = book
        .sheets
        .iter()
        .map(|s| {
            s.origin
                .map(|k| k as usize)
                .filter(|&k| k < parts.worksheets.len())
        })
        .collect();
    let kept_set: HashSet<usize> = kept.iter().flatten().copied().collect();
    let existing: HashSet<String> = (0..src.len())
        .filter_map(|i| src.name_for_index(i).map(str::to_owned))
        .collect();
    let rel_ids: HashSet<&str> = book_rels.iter().map(|r| r.id.as_str()).collect();
    let mut news: Vec<(usize, String, String)> = Vec::new(); // (sheet, part, rel id)
    let (mut n, mut r) = (1, 1);
    for (i, k) in kept.iter().enumerate() {
        if k.is_none() {
            while existing.contains(&format!("xl/worksheets/sheet{n}.xml"))
                || news
                    .iter()
                    .any(|(_, p, _)| *p == format!("xl/worksheets/sheet{n}.xml"))
            {
                n += 1;
            }
            while rel_ids.contains(format!("rIdBricks{r}").as_str()) {
                r += 1;
            }
            news.push((
                i,
                format!("xl/worksheets/sheet{n}.xml"),
                format!("rIdBricks{r}"),
            ));
            n += 1;
            r += 1;
        }
    }
    let removed: Vec<&package::SheetPart> = parts
        .worksheets
        .iter()
        .enumerate()
        .filter(|(k, _)| !kept_set.contains(k))
        .map(|(_, p)| p)
        .collect();
    let mut dropped: Vec<String> = removed
        .iter()
        .flat_map(|p| [p.part.clone(), package::rels_name(&p.part)])
        .collect();
    for rel in &book_rels {
        if rel.kind.ends_with(PART_CALC_CHAIN) || (!macros && rel.kind.ends_with(REL_VBA)) {
            dropped.push(rel.target.clone());
        }
    }
    let had_macros = book_rels.iter().any(|r| r.kind.ends_with(REL_VBA));
    // A package without a styles part (rare) gets one, with its relationship and type.
    let new_styles = parts.styles.is_none();

    // The sheet list: worksheets in their new order; other sheets (chart sheets) after
    // the worksheet they followed.
    let worksheet_index = |rel_id: &str| parts.worksheets.iter().position(|w| w.rel_id == rel_id);
    let mut entries: Vec<parts::Listed> = Vec::new();
    let mut active = 0;
    let leading: Vec<usize> = listed
        .iter()
        .enumerate()
        .take_while(|(_, id)| worksheet_index(id).is_none())
        .map(|(i, _)| i)
        .collect();
    entries.extend(leading.iter().map(|&i| parts::Listed::Old {
        index: i,
        name: None,
    }));
    for (i, sheet) in book.sheets.iter().enumerate() {
        if sheet.active {
            active = entries.len();
        }
        match kept[i] {
            Some(k) => {
                let rel_id = &parts.worksheets[k].rel_id;
                let at = listed.iter().position(|id| id == rel_id).unwrap_or(0);
                entries.push(parts::Listed::Old {
                    index: at,
                    name: Some(sheet.name.clone()),
                });
                // Sheets of other kinds that followed this one in the file.
                let follow = listed[at + 1..]
                    .iter()
                    .enumerate()
                    .take_while(|(_, id)| worksheet_index(id).is_none())
                    .map(|(j, _)| at + 1 + j);
                entries.extend(follow.map(|index| parts::Listed::Old { index, name: None }));
            }
            None => {
                let (_, _, rel_id) = news.iter().find(|(s, _, _)| *s == i).expect("a new sheet");
                entries.push(parts::Listed::New {
                    rel_id: rel_id.clone(),
                    name: sheet.name.clone(),
                });
            }
        }
    }
    let renames: Vec<(String, String)> = book
        .sheets
        .iter()
        .zip(&kept)
        .filter_map(|(s, k)| {
            let old = &parts.worksheets[(*k)?].name;
            (*old != s.name).then(|| (old.clone(), s.name.clone()))
        })
        .collect();
    let gone: Vec<String> = removed.iter().map(|p| p.name.clone()).collect();
    let refs = |text: &str| -> String {
        let Ok(mut expr) = formula::parse(text) else {
            return text.to_owned();
        };
        for (from, to) in &renames {
            adjust(&mut expr, "", &Change::RenameSheet { from, to });
        }
        for name in &gone {
            adjust(&mut expr, "", &Change::DeleteSheet { name });
        }
        formula::render_xlsx(&expr)
    };
    let names: Vec<(String, String)> = book
        .names
        .iter()
        .map(|(n, d)| {
            (
                n.clone(),
                formula::parse(d).map_or_else(|_| d.clone(), |e| formula::render_xlsx(&e)),
            )
        })
        .collect();
    let workbook = parts::workbook(
        &workbook_xml,
        &parts::WorkbookChange {
            sheets: &entries,
            active,
            names: &names,
            refs: &refs,
        },
    )?;
    let sheets_changed = !news.is_empty() || !removed.is_empty() || !renames.is_empty();
    let any_change = sheets_changed || book.sheets.iter().any(SaveSheet::changed);

    // Parts in the file's order, then new sheets, then the styles (written last: writing
    // cells can add date formats).
    let sheet_of_part = |part: &str| -> Option<usize> {
        let k = parts.worksheets.iter().position(|w| w.part == part)?;
        kept.iter().position(|x| *x == Some(k))
    };
    let mut layout: Layout = book.sheets.iter().map(|s| (s.name.clone(), None)).collect();
    let content_types_name = "[Content_Types].xml";
    for i in 0..src.len() {
        let name = src.name_for_index(i).unwrap_or_default().to_owned();
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "save cancelled"));
        }
        if dropped.contains(&name) || Some(&name) == parts.styles.as_ref() {
            continue;
        }
        if name == parts.workbook {
            zip.start_file(&name, options()).map_err(zip_error)?;
            zip.write_all(workbook.as_bytes())?;
        } else if name == book_rels_name {
            let xml = package::read_part(&mut src, &name)?.unwrap_or_default();
            let mut add: Vec<(String, &str, String)> = news
                .iter()
                .map(|(_, part, id)| {
                    let target = part
                        .strip_prefix(&format!("{book_dir}/"))
                        .unwrap_or(part)
                        .to_owned();
                    (id.clone(), parts::REL_WORKSHEET, target)
                })
                .collect();
            if new_styles {
                add.push((
                    "rIdBricksStyles".into(),
                    parts::REL_STYLES,
                    "styles.xml".into(),
                ));
            }
            let xml = parts::rels(&xml, &book_dir, &dropped, &add)?;
            zip.start_file(&name, options()).map_err(zip_error)?;
            zip.write_all(xml.as_bytes())?;
        } else if name == content_types_name {
            let xml = package::read_part(&mut src, &name)?.unwrap_or_default();
            let mut add: Vec<(String, &str)> = news
                .iter()
                .map(|(_, p, _)| (p.clone(), parts::CT_WORKSHEET))
                .collect();
            if new_styles {
                add.push((format!("{book_dir}/styles.xml"), parts::CT_STYLES));
            }
            let switch =
                (had_macros && !macros).then_some((parts.workbook.as_str(), parts::CT_WORKBOOK));
            let drop_bin = (had_macros && !macros).then_some("bin");
            let xml = parts::content_types(&xml, &dropped, &add, switch, drop_bin)?;
            zip.start_file(&name, options()).map_err(zip_error)?;
            zip.write_all(xml.as_bytes())?;
        } else if name == "docProps/app.xml" && sheets_changed {
            let xml = package::read_part(&mut src, &name)?.unwrap_or_default();
            zip.start_file(&name, options()).map_err(zip_error)?;
            zip.write_all(parts::app_properties(&xml)?.as_bytes())?;
        } else if let Some(s) = sheet_of_part(&name) {
            let sheet = &mut book.sheets[s];
            let formulas = sheet.table.has_formulas();
            if sheet.changed() || (formulas && any_change) {
                let part = unpacked(&mut src, &name)?;
                zip.start_file(&name, options()).map_err(zip_error)?;
                let mut cells = cells::CellStyles::new(pool);
                sheet::write_sheet(zip, Some(part.bytes()), sheet, &mut cells, progress, cancel)?;
                layout[s].1 = Some(sheet.table.total_rows());
            } else {
                let file = src.by_index_raw(i).map_err(zip_error)?;
                zip.raw_copy_file(file).map_err(zip_error)?;
                progress.add_written(sheet.table.total_rows());
            }
        } else {
            let file = src.by_index_raw(i).map_err(zip_error)?;
            zip.raw_copy_file(file).map_err(zip_error)?;
        }
    }
    for (s, part, _) in &news {
        zip.start_file(part, options()).map_err(zip_error)?;
        let mut cells = cells::CellStyles::new(pool);
        let sheet = &mut book.sheets[*s];
        sheet::write_sheet(zip, None, sheet, &mut cells, progress, cancel)?;
        layout[*s].1 = Some(sheet.table.total_rows());
    }
    let styles_part = parts
        .styles
        .clone()
        .unwrap_or_else(|| format!("{book_dir}/styles.xml"));
    let file_styles = match &parts.styles {
        Some(p) => package::read_part(&mut src, p)?,
        None => None,
    };
    let xml = match &file_styles {
        Some(xml) => {
            let ids: Vec<u32> = crate::styles::file_num_fmt_ids(xml)?;
            if styles::grew(pool, &ids) {
                styles::spliced(xml, pool, &ids)
                    .ok_or_else(|| io::Error::other("the styles part has no style lists"))?
            } else {
                xml.clone()
            }
        }
        None => styles::whole(pool),
    };
    zip.start_file(&styles_part, options()).map_err(zip_error)?;
    zip.write_all(xml.as_bytes())?;
    Ok(layout)
}

/// A workbook with no package: written from a minimal one.
fn fresh(
    zip: &mut ZipWriter<Out<'_>>,
    book: &mut SaveBook,
    pool: &mut StylePool,
    progress: &SaveProgress,
    cancel: &AtomicBool,
) -> io::Result<Layout> {
    let mut layout: Layout = book.sheets.iter().map(|s| (s.name.clone(), None)).collect();
    let mut types = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/>",
    );
    let _ = write!(
        types,
        "<Override PartName=\"/xl/workbook.xml\" ContentType=\"{}\"/><Override PartName=\"/xl/styles.xml\" ContentType=\"{}\"/>",
        parts::CT_WORKBOOK,
        parts::CT_STYLES
    );
    let mut rels = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
    );
    let mut sheets = String::new();
    let mut active = 0;
    for (i, s) in book.sheets.iter().enumerate() {
        let n = i + 1;
        let _ = write!(
            types,
            "<Override PartName=\"/xl/worksheets/sheet{n}.xml\" ContentType=\"{}\"/>",
            parts::CT_WORKSHEET
        );
        let _ = write!(
            rels,
            "<Relationship Id=\"rId{n}\" Type=\"{}\" Target=\"worksheets/sheet{n}.xml\"/>",
            parts::REL_WORKSHEET
        );
        sheets.push_str("<sheet name=\"");
        cells::escape(&s.name, &mut sheets);
        let _ = write!(sheets, "\" sheetId=\"{n}\" r:id=\"rId{n}\"/>");
        if s.active {
            active = i;
        }
    }
    let n = book.sheets.len() + 1;
    let _ = write!(
        rels,
        "<Relationship Id=\"rId{n}\" Type=\"{}\" Target=\"styles.xml\"/></Relationships>",
        parts::REL_STYLES
    );
    types.push_str("</Types>");
    let mut names = String::new();
    for (name, def) in &book.names {
        let def = formula::parse(def).map_or_else(|_| def.clone(), |e| formula::render_xlsx(&e));
        names.push_str("<definedName name=\"");
        cells::escape(name, &mut names);
        names.push_str("\">");
        cells::escape(&def, &mut names);
        names.push_str("</definedName>");
    }
    if !names.is_empty() {
        names = format!("<definedNames>{names}</definedNames>");
    }
    let workbook = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><bookViews><workbookView activeTab=\"{active}\"/></bookViews><sheets>{sheets}</sheets>{names}<calcPr fullCalcOnLoad=\"1\"/></workbook>"
    );
    let root_rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>";
    for (name, xml) in [
        ("[Content_Types].xml", types.as_str()),
        ("_rels/.rels", root_rels),
        ("xl/workbook.xml", workbook.as_str()),
        ("xl/_rels/workbook.xml.rels", rels.as_str()),
    ] {
        zip.start_file(name, options()).map_err(zip_error)?;
        zip.write_all(xml.as_bytes())?;
    }
    for (i, sheet) in book.sheets.iter_mut().enumerate() {
        zip.start_file(format!("xl/worksheets/sheet{}.xml", i + 1), options())
            .map_err(zip_error)?;
        let mut cells = cells::CellStyles::new(pool);
        sheet::write_sheet(zip, None, sheet, &mut cells, progress, cancel)?;
        layout[i].1 = Some(sheet.table.total_rows());
    }
    zip.start_file("xl/styles.xml", options())
        .map_err(zip_error)?;
    zip.write_all(styles::whole(pool).as_bytes())?;
    Ok(layout)
}

fn verify(path: &Path, layout: &Layout) -> Result<(), String> {
    let mut book: Xlsx<BufReader<File>> =
        calamine::open_workbook(path).map_err(|e: calamine::XlsxError| e.to_string())?;
    let names = book.sheet_names();
    let expected: Vec<&str> = layout.iter().map(|(n, _)| n.as_str()).collect();
    if names != expected {
        return Err(format!("sheets {names:?}, expected {expected:?}"));
    }
    for (name, written) in layout {
        let Some(want) = written else {
            continue; // copied as it was
        };
        let reader = book
            .worksheet_cells_reader(name)
            .map_err(|e| e.to_string())?;
        let rows = u64::from(reader.dimensions().end.0) + 1;
        // An empty sheet's dimension is A1.
        if rows != (*want).max(1) {
            return Err(format!("“{name}” has {rows} rows, {want} written"));
        }
    }
    Ok(())
}
