//! STYLE-5 acceptance: saving patches the original package. Edits on two sheets, a new
//! sheet, a deleted row, and a renamed sheet save; the chart, comments, conditional
//! format, validation, hyperlink, styles, and merges survive; parts nothing touched are
//! copied byte for byte; macros stay in an .xlsm and leave with an .xlsx.

use data_model::{
    BookEdit, CellRef, ColId, CsvTable, Edit, Origin, RowBlock, RowId, SaveProgress, SheetId,
    StyleId, Workbook,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;
use xlsx::{import, save, Imported, Package, Progress, SaveBook};

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

fn temp(test: &str, name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("brick-patch-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn import_file(path: &Path) -> Imported {
    import(path, &AtomicBool::new(false), &Progress::default()).unwrap()
}

fn workbook(imported: Imported) -> (Workbook, Arc<Package>) {
    let mut book = Workbook::from_sheets(
        imported
            .sheets
            .into_iter()
            .map(|s| (s.name, s.table, Origin::Xlsx(s.index), s.cached))
            .collect(),
        imported.names,
    );
    book.set_styles(imported.styles);
    (book, imported.package)
}

fn save_book(dest: &Path, book: &Workbook, package: Arc<Package>) {
    let book = SaveBook::from_workbook(
        book.snapshot(),
        Some(package),
        &HashMap::new(),
        Arc::new(AtomicBool::new(false)),
        &AtomicU64::new(0),
    )
    .unwrap();
    save(
        dest,
        book,
        &SaveProgress::default(),
        &AtomicBool::new(false),
    )
    .unwrap();
}

/// Every part of a package, unpacked.
fn parts(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    (0..zip.len())
        .map(|i| {
            let mut f = zip.by_index(i).unwrap();
            let mut bytes = Vec::new();
            f.read_to_end(&mut bytes).unwrap();
            (f.name().to_owned(), bytes)
        })
        .collect()
}

fn text(parts: &BTreeMap<String, Vec<u8>>, name: &str) -> String {
    String::from_utf8(parts[name].clone()).unwrap()
}

/// Every non-empty cell's raw text, by (position among all rows, column).
fn cells(table: &mut CsvTable) -> BTreeMap<(u64, u32), String> {
    let mut out = BTreeMap::new();
    let mut block = RowBlock::full_text();
    let total = table.total_rows();
    table.read_all(0..total, &mut block);
    for r in 0..total {
        for c in 0..block.cells_in_row(r) {
            let t = block.cell(r, c).unwrap();
            if !t.is_empty() {
                out.insert((r, c), t.to_owned());
            }
        }
    }
    out
}

fn at(row: u64, col: u32) -> CellRef {
    CellRef {
        row: RowId::source(row),
        col: ColId::source(col),
    }
}

const STYLES: SheetId = SheetId(3);
/// Styles' row 4 (the fills): deleted, so everything below it moves up.
const DELETED: u64 = 3;

/// The fixture with edits on Data and Q1 Sales, row 4 of Styles deleted, Summary renamed
/// Report, and a new sheet Added; saved by patching. Returns the saved path and the book.
fn edited(test: &str) -> (PathBuf, Workbook) {
    let (mut book, package) = workbook(import_file(&data("fixture.xlsx")));
    book.sheet_mut(SheetId(0))
        .unwrap()
        .table
        .apply(Edit::set(at(1, 1), "99"));
    book.sheet_mut(SheetId(1))
        .unwrap()
        .table
        .apply(Edit::set(at(0, 0), "=Data!B3*3"));
    let styles = &mut book.sheet_mut(STYLES).unwrap().table;
    let delete = styles.delete_rows(DELETED, 1).unwrap();
    // As the app does it: the merges move in the same step (STYLE-4).
    let merges = styles.merges_after(&delete);
    styles.apply(delete);
    styles.apply(merges.expect("the merges below the row move up"));
    book.apply(BookEdit::Rename {
        id: SheetId(2),
        name: "Report".into(),
    });
    let mut added = CsvTable::untitled();
    let insert = added.insert_rows(0, 1).unwrap();
    added.apply(insert);
    let first = CellRef {
        row: added.row_id(0),
        col: added.col_id(0),
    };
    added.apply(Edit::set(first, "new"));
    let sheet = book.new_sheet("Added".into(), added, Origin::New);
    book.apply(BookEdit::Insert {
        at: 4,
        sheet: Box::new(sheet),
    });
    let dest = temp(test, "saved.xlsx");
    save_book(&dest, &book, package);
    (dest, book)
}

#[test]
fn edits_and_new_sheets_save_and_everything_else_survives() {
    let before = parts(&data("fixture.xlsx"));
    let (dest, mut book) = edited("survives");
    let after = parts(&dest);

    // The same parts, plus the new sheet's.
    let mut want: BTreeSet<&str> = before.keys().map(String::as_str).collect();
    want.insert("xl/worksheets/sheet5.xml");
    assert_eq!(
        after.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        want
    );

    // Parts no change touched are as they were, byte for byte: the chart, the comments,
    // the drawings, the theme, the styles, the shared strings.
    let rewritten = [
        "xl/workbook.xml",
        "xl/_rels/workbook.xml.rels",
        "[Content_Types].xml",
        "docProps/app.xml",
        "xl/worksheets/sheet1.xml",
        "xl/worksheets/sheet2.xml",
        "xl/worksheets/sheet3.xml", // formulas: their results are written again
        "xl/worksheets/sheet4.xml",
        "xl/worksheets/sheet5.xml",
    ];
    for (name, bytes) in &before {
        if !rewritten.contains(&name.as_str()) {
            assert_eq!(&after[name], bytes, "{name} changed");
        }
    }

    // What the edits did reads back.
    let imported = import_file(&dest);
    let names: Vec<&str> = imported.sheets.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Data", "Q1 Sales", "Report", "Styles", "Added"]);
    let saved_pool = imported.styles.clone();
    let mut saved: Vec<_> = imported.sheets.into_iter().collect();
    for (k, sheet) in saved.iter_mut().enumerate() {
        let want = cells(&mut book.sheet_mut(book.sheets()[k].id).unwrap().table);
        assert_eq!(cells(&mut sheet.table), want, "cells of {}", sheet.name);
    }

    // Styles and merges moved up with their rows.
    let original = workbook(import_file(&data("fixture.xlsx"))).0;
    let old = &original.sheet(STYLES).unwrap().table;
    let new = &saved[3].table;
    for r in 0..18u64 {
        for c in 0..9u32 {
            let want = old.style_of(at(r, c));
            match r.cmp(&DELETED) {
                std::cmp::Ordering::Less => {
                    assert_eq!(new.style_of(at(r, c)), want, "Styles row {} col {c}", r + 1)
                }
                std::cmp::Ordering::Greater => {
                    assert_eq!(
                        new.style_of(at(r - 1, c)),
                        want,
                        "Styles row {} col {c}",
                        r + 1
                    )
                }
                std::cmp::Ordering::Equal => {}
            }
        }
    }
    assert_eq!(saved_pool.xfs, original.styles().xfs, "no new cell formats");
    let merges: Vec<(u64, u32, u64, u32)> = new
        .layout()
        .merges
        .iter()
        .map(|m| (m.first_row, m.first_col, m.last_row, m.last_col))
        .collect();
    assert_eq!(merges, [(0, 0, 0, 3), (6, 4, 7, 6)], "A1:D1 and E7:G8");
    assert!(new.style_of(at(0, 0)) != StyleId(0));

    // Ranges elsewhere in the sheet followed the rows: the conditional format, the
    // validation, the hyperlink.
    let sheet = text(&after, "xl/worksheets/sheet4.xml");
    assert!(
        sheet.contains("sqref=\"C7:C14\""),
        "conditional format: {sheet}"
    );
    assert!(sheet.contains("sqref=\"D7:D14\""), "validation");
    assert!(sheet.contains("ref=\"A17\""), "hyperlink");
    // The workbook knows Report, keeps the chart sheet's relationships, and recalculates.
    let wb = text(&after, "xl/workbook.xml");
    assert!(wb.contains("name=\"Report\"") && !wb.contains("name=\"Summary\""));
    assert!(wb.contains("fullCalcOnLoad=\"1\""));
    assert!(
        wb.contains("name=\"Rates\"") && wb.contains(">Data!$C$2:$C$51</definedName>"),
        "{wb}"
    );
}

#[test]
fn an_unedited_workbook_saves_its_sheets_as_they_were() {
    let (book, package) = workbook(import_file(&data("fixture.xlsx")));
    let dest = temp("unedited", "saved.xlsx");
    save_book(&dest, &book, package);
    let (before, after) = (parts(&data("fixture.xlsx")), parts(&dest));
    for (name, bytes) in &before {
        if name.starts_with("xl/worksheets/") {
            assert_eq!(&after[name], bytes, "{name} changed");
        }
    }
}

/// The fixture as an .xlsm: a macro part, its relationship, and the macro workbook type.
fn with_macros(dest: &Path) {
    let parts = parts(&data("fixture.xlsx"));
    let mut zip = zip::ZipWriter::new(std::fs::File::create(dest).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    for (name, bytes) in parts {
        let bytes = match name.as_str() {
            "[Content_Types].xml" => String::from_utf8(bytes)
                .unwrap()
                .replace(
                    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml",
                    "application/vnd.ms-excel.sheet.macroEnabled.main+xml",
                )
                .replace(
                    "<Default Extension=\"xml\"",
                    "<Default Extension=\"bin\" ContentType=\"application/vnd.ms-office.vbaProject\"/><Default Extension=\"xml\"",
                )
                .into_bytes(),
            "xl/_rels/workbook.xml.rels" => String::from_utf8(bytes)
                .unwrap()
                .replace(
                    "</Relationships>",
                    "<Relationship Id=\"rIdVba\" Type=\"http://schemas.microsoft.com/office/2006/relationships/vbaProject\" Target=\"vbaProject.bin\"/></Relationships>",
                )
                .into_bytes(),
            _ => bytes,
        };
        zip.start_file(name, options).unwrap();
        zip.write_all(&bytes).unwrap();
    }
    zip.start_file("xl/vbaProject.bin", options).unwrap();
    zip.write_all(b"macros").unwrap();
    zip.finish().unwrap();
}

#[test]
fn macros_stay_in_an_xlsm_and_leave_with_an_xlsx() {
    let source = temp("macros", "with.xlsm");
    with_macros(&source);
    assert!(parts(&source)["[Content_Types].xml"]
        .windows(9)
        .any(|w| w == b"vbaProjec" || w == b"macroEnab"));
    for (dest, keeps) in [("saved.xlsm", true), ("saved.xlsx", false)] {
        let (mut book, package) = workbook(import_file(&source));
        book.active_mut().apply(Edit::set(at(1, 1), "7"));
        let dest = temp("macros", dest);
        save_book(&dest, &book, package);
        let after = parts(&dest);
        let types = text(&after, "[Content_Types].xml");
        let rels = text(&after, "xl/_rels/workbook.xml.rels");
        assert_eq!(after.contains_key("xl/vbaProject.bin"), keeps);
        assert_eq!(rels.contains("vbaProject"), keeps);
        assert_eq!(types.contains("macroEnabled"), keeps, "{types}");
        assert_eq!(types.contains("Extension=\"bin\""), keeps);
        assert_eq!(import_file(&dest).sheets.len(), 4);
    }
}

/// Run `scripts/lo_xlsx.py MODE FILE`.
fn lo(mode: &str, file: &Path) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../..");
    let out = std::process::Command::new("python3")
        .arg(root.join("scripts/lo_xlsx.py"))
        .args([mode, &file.to_string_lossy()])
        .env("LO_PORT", "2107")
        .output()
        .expect("python3");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// The row number a dump line is about: `B12<TAB>…` and `row 12 …`; `None` for others.
fn line_row(line: &str) -> Option<u64> {
    if let Some(rest) = line.strip_prefix("row ") {
        return rest.split(' ').next()?.parse().ok();
    }
    let first = line.split('\t').next()?;
    let digits = first.trim_start_matches(|c: char| c.is_ascii_uppercase());
    (digits.len() < first.len())
        .then(|| digits.parse().ok())
        .flatten()
}

/// The line with its row number one less (`B12` → `B11`, `row 12` → `row 11`).
fn moved_up(line: &str, row: u64) -> String {
    if let Some(rest) = line.strip_prefix("row ") {
        let tail = rest.split_once(' ').map_or("", |(_, t)| t);
        return format!("row {} {tail}", row - 1);
    }
    let (first, tail) = line.split_once('\t').unwrap_or((line, ""));
    let letters = first.trim_end_matches(|c: char| c.is_ascii_digit());
    format!("{letters}{}\t{tail}", row - 1)
}

/// A dump's lines by sheet, as the edited workbook should read: Summary is Report, and on
/// Styles the deleted row's lines go and those below it move up. Merges compare apart;
/// formula lines lose their value (edits change results).
fn expected(dump: &str) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut sheet = String::new();
    for line in dump.lines() {
        if let Some(name) = line.strip_prefix("== ") {
            sheet = if name == "Summary" {
                "Report".into()
            } else {
                name.into()
            };
            out.entry(sheet.clone()).or_default();
            continue;
        }
        if line.starts_with("merge ") {
            continue;
        }
        let mut line = without_value(line);
        if sheet == "Styles" {
            match line_row(&line) {
                Some(r) if r == DELETED + 1 => continue,
                Some(r) if r > DELETED + 1 => line = moved_up(&line, r),
                _ => {}
            }
        }
        out.get_mut(&sheet).unwrap().insert(line);
    }
    out
}

fn without_value(line: &str) -> String {
    let fields: Vec<&str> = line.split('\t').collect();
    match fields.get(1) {
        Some(kind) if kind.starts_with("formula:") => {
            format!("{}\t{kind}\t\t{}", fields[0], fields[3])
        }
        _ => line.to_owned(),
    }
}

fn actual(dump: &str) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut sheet = String::new();
    for line in dump.lines() {
        if let Some(name) = line.strip_prefix("== ") {
            sheet = name.into();
            out.entry(sheet.clone()).or_default();
        } else if !line.starts_with("merge ") {
            out.get_mut(&sheet).unwrap().insert(without_value(line));
        }
    }
    out
}

#[test]
#[ignore = "needs LibreOffice: cargo test -p xlsx --test patch -- --ignored --test-threads=1"]
fn libreoffice_reads_the_patched_save_as_the_original_plus_the_edits() {
    let (dest, _) = edited("libreoffice");
    for mode in ["styles", "dump"] {
        let want = expected(&lo(mode, &data("fixture.xlsx")));
        let got = actual(&lo(mode, &dest));
        assert_eq!(
            got.keys().collect::<Vec<_>>(),
            ["Added", "Data", "Q1 Sales", "Report", "Styles"]
        );
        for (sheet, lines) in &want {
            for line in lines {
                // The edited cells differ; every other line is LibreOffice's reading of the
                // original.
                let edited = (sheet == "Data" && line.starts_with("B2\t"))
                    || (sheet == "Q1 Sales" && line.starts_with("A1\t"));
                if !edited {
                    assert!(got[sheet].contains(line), "{mode}: {sheet} lacks {line:?}");
                }
            }
        }
    }
    let styles = lo("styles", &dest);
    assert!(
        styles.contains("merge A1:D1") && styles.contains("merge E7:G8"),
        "{styles}"
    );
}
