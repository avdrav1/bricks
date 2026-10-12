//! XLSX-1: the LibreOffice fixture imports as LibreOffice reads it; saves reopen with every
//! cell's raw text unchanged.

use data_model::{
    CellRef, ColId, CsvTable, DelimiterChoice, Edit, Origin, RowBlock, RowId, SaveProgress,
    SheetId, Workbook,
};
use formula::{literal, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;
use std::time::Instant;
use xlsx::{
    check_limits, import, kind, refusal, save, Imported, Kind, Package, Progress, SaveBook,
    SheetSizes,
};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../..");

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

fn corpus(name: &str) -> Option<PathBuf> {
    let p = Path::new(ROOT).join("corpus").join(name);
    if p.exists() {
        Some(p)
    } else {
        eprintln!(
            "skipped: {} is missing (python3 scripts/gen_corpus.py)",
            p.display()
        );
        None
    }
}

/// A fresh path in a per-test directory under the system temp dir.
fn temp(test: &str, name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("brick-xlsx-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn no_cancel() -> AtomicBool {
    AtomicBool::new(false)
}

fn import_file(path: &Path) -> Imported {
    import(path, &no_cancel(), &Progress::default()).unwrap()
}

/// Every non-empty cell's raw text, by (position among all rows, view column).
fn cells(table: &mut CsvTable) -> BTreeMap<(u64, u32), String> {
    let mut out = BTreeMap::new();
    let mut block = RowBlock::full_text();
    let total = table.total_rows();
    let mut start = 0;
    while start < total {
        let end = (start + 65_536).min(total);
        table.read_all(start..end, &mut block);
        for r in start..end {
            for c in 0..block.cells_in_row(r) {
                let t = block.cell(r, c).unwrap();
                if !t.is_empty() {
                    out.insert((r, c), t.to_string());
                }
            }
        }
        start = end;
    }
    out
}

/// The workbook an import read, as the app makes it, and the package it came from.
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

/// Save `book` to `dest` as the app does, patching `package` if there is one.
fn save_book(
    dest: &Path,
    book: &Workbook,
    package: Option<Arc<Package>>,
    sizes: &HashMap<SheetId, SheetSizes>,
) -> data_model::SaveStats {
    let book = SaveBook::from_workbook(
        book.snapshot(),
        package,
        sizes,
        Arc::new(no_cancel()),
        &AtomicU64::new(0),
    )
    .unwrap();
    save(dest, book, &SaveProgress::default(), &no_cancel()).unwrap_or_else(|e| panic!("{e}"))
}

struct Dumped {
    at: (u64, u32),
    kind: String,
    value: String,
    formula: String,
}

/// `scripts/lo_xlsx.py dump` output: sheets in order, each with its cells.
fn parse_dump(text: &str) -> Vec<(String, Vec<Dumped>)> {
    let mut sheets: Vec<(String, Vec<Dumped>)> = Vec::new();
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("== ") {
            sheets.push((name.to_string(), Vec::new()));
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        let Some(formula::Reference {
            area: formula::Area::Cell(a),
            ..
        }) = formula::parse_reference(f[0])
        else {
            panic!("not a cell: {line}");
        };
        sheets.last_mut().unwrap().1.push(Dumped {
            at: (a.row, a.col),
            kind: f[1].to_string(),
            value: f[2].to_string(),
            formula: f[3].to_string(),
        });
    }
    sheets
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) | Value::Date(n) => Some(*n),
        Value::Bool(b) => Some(f64::from(u8::from(*b))),
        _ => None,
    }
}

fn canonical(formula: &str) -> String {
    formula::render(&formula::parse(formula).unwrap_or_else(|e| panic!("{formula}: {e:?}")))
}

#[test]
fn fixture_imports_as_libreoffice_reads_it() {
    let dump = parse_dump(&std::fs::read_to_string(data("fixture-libreoffice.txt")).unwrap());
    let imported = import_file(&data("fixture.xlsx"));
    let names: Vec<&str> = imported.sheets.iter().map(|s| s.name.as_str()).collect();
    let dumped: Vec<&str> = dump.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, dumped);
    assert_eq!(
        imported.names,
        vec![("Rates".to_string(), "Data!$C$2:$C$51".to_string())]
    );
    let indices: Vec<u32> = imported.sheets.iter().map(|s| s.index).collect();
    assert_eq!(indices, [0, 1, 2, 3]);

    let (mut book, _) = workbook(imported);
    let mut formulas = 0;
    for (k, (name, expected)) in dump.iter().enumerate() {
        let id = SheetId(k as u32);
        let raw = cells(&mut book.sheet_mut(id).unwrap().table);
        assert_eq!(raw.len(), expected.len(), "cells on {name}");
        let table = &book.sheet(id).unwrap().table;
        let refs: Vec<CellRef> = expected
            .iter()
            .map(|d| CellRef {
                row: table.id_at(d.at.0),
                col: table.col_id(d.at.1),
            })
            .collect();
        let targets: Vec<(SheetId, CellRef)> = refs.iter().map(|&r| (id, r)).collect();
        let values = book
            .evaluate_all(&targets, Arc::new(no_cancel()), &AtomicU64::new(0))
            .unwrap();
        for (d, (_, v)) in expected.iter().zip(values) {
            let text = &raw[&d.at];
            let place = format!("{name}!{:?} ({text})", d.at);
            match d.kind.as_str() {
                "text" => assert_eq!(text, &d.value, "{place}"),
                "value" => {
                    let n = number(&literal(text)).unwrap_or_else(|| panic!("{place}"));
                    assert!(close(n, d.value.parse().unwrap()), "{place}: {}", d.value);
                }
                kind => {
                    formulas += 1;
                    let body = text.strip_prefix('=').expect("a formula");
                    assert_eq!(canonical(body), canonical(&d.formula), "{place}");
                    match kind {
                        "formula:value" => {
                            let n = number(&v).unwrap_or_else(|| panic!("{place}: {v:?}"));
                            assert!(close(n, d.value.parse().unwrap()), "{place}: {v:?}");
                        }
                        "formula:text" | "formula:error" => {
                            assert_eq!(formula::result_text(&v), d.value, "{place}")
                        }
                        other => panic!("{other}"),
                    }
                }
            }
        }
    }
    assert_eq!(formulas, 16);
    // BAHTTEXT is not in the registry: its result is the one the file stored.
    assert!(!formula::functions::is_known("BAHTTEXT"));
    let summary = book.sheet(SheetId(2)).unwrap();
    assert_eq!(summary.table.formula_cells().unwrap().len(), 2);
}

/// Each sheet's name and raw cells.
type SheetCells = Vec<(String, BTreeMap<(u64, u32), String>)>;

/// Import the fixture, edit one cell per sheet, and save it; returns the saved path and
/// the cells saved.
fn edited_fixture_save(test: &str) -> (PathBuf, SheetCells) {
    let (mut book, package) = workbook(import_file(&data("fixture.xlsx")));
    let at = |row, col| CellRef {
        row: RowId::source(row),
        col: ColId::source(col),
    };
    let edits = [
        (0, at(1, 0), "99"),
        (1, at(0, 0), "=Data!B3*3"),
        (2, at(0, 0), "Title, \"edited\""),
    ];
    for (k, at, text) in edits {
        book.sheet_mut(SheetId(k))
            .unwrap()
            .table
            .apply(Edit::set(at, text));
    }
    book.set_active(SheetId(1));
    let mut expected = Vec::new();
    let mut rows = 0;
    for k in 0..4 {
        let sheet = book.sheet_mut(SheetId(k)).unwrap();
        rows += sheet.table.total_rows();
        expected.push((sheet.name.clone(), cells(&mut sheet.table)));
    }
    let sizes = HashMap::from([(
        SheetId(0),
        SheetSizes {
            widths: vec![(ColId::source(0), 17.0), (ColId::source(3), 9.0)],
            heights: vec![(RowId::source(2), 30.0)],
        },
    )]);
    let dest = temp(test, "saved.xlsx");
    let stats = save_book(&dest, &book, Some(package), &sizes);
    assert_eq!(stats.rows, rows);
    assert_eq!(stats.bytes, std::fs::metadata(&dest).unwrap().len());
    (dest, expected)
}

#[test]
fn edited_fixture_saves_and_reopens_the_same() {
    let (dest, expected) = edited_fixture_save("round-trip");
    assert_eq!(kind(&dest).unwrap(), Kind::Xlsx);
    let imported = import_file(&dest);
    assert_eq!(
        imported.names,
        vec![("Rates".to_string(), "Data!$C$2:$C$51".to_string())]
    );
    let mut cached = Vec::new();
    for (mut sheet, (name, cells_before)) in imported.sheets.into_iter().zip(&expected) {
        assert_eq!(&sheet.name, name);
        assert_eq!(&cells(&mut sheet.table), cells_before, "cells on {name}");
        cached.push(sheet.cached);
    }
    let at = |row, col| CellRef {
        row: RowId::source(row),
        col: ColId::source(col),
    };
    // The results Brick computed are stored with the formulas.
    assert_eq!(cached[1].get(&at(0, 0)).map(|s| &**s), Some("75.75"));
    assert_eq!(cached[1].get(&at(1, 1)).map(|s| &**s), Some("12.695"));
    // A function Brick doesn't compute keeps the result the file had.
    assert_eq!(cached[2].get(&at(2, 0)).map(|s| &**s), Some("หนึ่งบาทถ้วน"));
    assert_eq!(cached[0].get(&at(1, 7)).map(|s| &**s), Some("#DIV/0!"));
}

/// Save `table` as a one-sheet workbook and import it: every cell's raw text is the same.
fn assert_round_trip(test: &str, name: &str, mut table: CsvTable) {
    table.index().build(&no_cancel()).unwrap();
    let before = cells(&mut table);
    let dest = temp(test, "saved.xlsx");
    let book = Workbook::single(name.to_string(), table, Origin::Csv);
    save_book(&dest, &book, None, &HashMap::new());
    let mut imported = import_file(&dest);
    assert_eq!(imported.sheets.len(), 1);
    let after = cells(&mut imported.sheets[0].table);
    assert_eq!(before.len(), after.len(), "{test}: cell count");
    for (at, text) in &before {
        assert_eq!(Some(text), after.get(at), "{test}: cell {at:?}");
    }
    std::fs::remove_file(dest).unwrap();
}

#[test]
fn csv_files_save_as_xlsx_with_every_cell_unchanged() {
    let Some(rows) = corpus("rows_10mb.csv") else {
        return;
    };
    let table = CsvTable::open(&rows, DelimiterChoice::Auto).unwrap();
    assert_round_trip("rows_10mb", "rows", table);
    let mut nasty: Vec<PathBuf> = std::fs::read_dir(Path::new(ROOT).join("corpus/nasty"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    nasty.sort();
    assert!(nasty.len() >= 10);
    for path in nasty {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let table = CsvTable::open(&path, DelimiterChoice::Auto).unwrap();
        assert_round_trip(&name, "Sheet1", table);
    }
}

#[test]
fn typed_values_round_trip_and_near_misses_stay_text() {
    let csv = "\
1,-0,1.5,0.1,123456789012345,1234567890123456789,1e5,00123,.5,+1
TRUE,FALSE,true,True,2024-02-29,1900-01-01,1900-03-01,2024-01-01 12:30:45,2024-01-01 00:00:00,2024-1-1
#N/A,#DIV/0!,=1/0,=A1+,=SUM(A1:B1),'=x,=\"a\"&\"b\",\"with, comma\",\"with \"\"quote\"\"\",  padded  
";
    let path = temp("typed", "typed.csv");
    std::fs::write(&path, csv).unwrap();
    let table = CsvTable::open(&path, DelimiterChoice::Fixed(b',')).unwrap();
    assert_round_trip("typed", "Typed", table);
}

#[test]
fn limits_name_the_sheet_that_does_not_fit() {
    assert_eq!(
        check_limits(&[
            ("ok".into(), 1_048_576, 16_384),
            ("Big".into(), 2_000_000, 3),
        ]),
        Err("“Big” has 2,000,000 rows; an .xlsx sheet holds at most 1,048,576".into())
    );
    assert_eq!(
        check_limits(&[("Wide".into(), 10, 16_385)]),
        Err("“Wide” has 16,385 columns; an .xlsx sheet holds at most 16,384".into())
    );
    assert_eq!(check_limits(&[("ok".into(), 1_048_576, 16_384)]), Ok(()));
}

#[test]
fn other_spreadsheet_formats_are_refused() {
    let cfb = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0, 0, 0, 0];
    let xls = temp("refusal", "old.XLS");
    std::fs::write(&xls, cfb).unwrap();
    assert_eq!(kind(&xls).unwrap(), Kind::Xls);
    assert_eq!(
        refusal(Kind::Xls),
        Some(
            "Old Excel .xls files can't be opened. Save it as .xlsx in Excel or LibreOffice first."
        )
    );
    let encrypted = temp("refusal", "locked.xlsx");
    std::fs::write(&encrypted, cfb).unwrap();
    assert_eq!(kind(&encrypted).unwrap(), Kind::Encrypted);
    assert_eq!(
        refusal(Kind::Encrypted),
        Some("This workbook is password-protected. Remove the password in Excel or LibreOffice first.")
    );
    let csv = temp("refusal", "plain.xlsx");
    std::fs::write(&csv, "PK,but text\n").unwrap();
    assert_eq!(kind(&csv).unwrap(), Kind::Text);
    assert_eq!(refusal(Kind::Text), None);
    assert_eq!(kind(&data("fixture.xlsx")).unwrap(), Kind::Xlsx);
    assert_eq!(refusal(Kind::Xlsx), None);
}

/// A sheet whose rows are stored out of order is read whole instead of streamed.
#[test]
fn out_of_order_cells_are_read_whole() {
    use std::io::{Read, Write};
    let mut book = rust_xlsxwriter::Workbook::new();
    let ws = book.add_worksheet();
    ws.set_name("Swapped").unwrap();
    ws.write_string(0, 0, "a").unwrap();
    ws.write_formula(0, 1, "=B2*2").unwrap();
    ws.write_number(1, 1, 2).unwrap();
    ws.write_string(1, 2, "x,y").unwrap();
    let bytes = book.save_to_buffer().unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let path = temp("out-of-order", "swapped.xlsx");
    let mut out = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        let name = entry.name().to_string();
        let mut body = String::new();
        entry.read_to_string(&mut body).unwrap();
        if name == "xl/worksheets/sheet1.xml" {
            let (r1, r2) = (
                body.find("<row r=\"1\"").unwrap(),
                body.find("<row r=\"2\"").unwrap(),
            );
            let end = body[r2..].find("</row>").unwrap() + r2 + "</row>".len();
            body = format!(
                "{}{}{}{}",
                &body[..r1],
                &body[r2..end],
                &body[r1..r2],
                &body[end..]
            );
        }
        out.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        out.write_all(body.as_bytes()).unwrap();
    }
    out.finish().unwrap();
    let mut imported = import_file(&path);
    let sheet = &mut imported.sheets[0];
    let got = cells(&mut sheet.table);
    let want: BTreeMap<(u64, u32), String> = [
        ((0, 0), "a"),
        ((0, 1), "=B2*2"),
        ((1, 1), "2"),
        ((1, 2), "x,y"),
    ]
    .into_iter()
    .map(|(at, t)| (at, t.to_string()))
    .collect();
    assert_eq!(got, want);
    assert_eq!(sheet.formulas.file_cells(), &[(0, 1)]);
}

#[test]
fn cancel_stops_an_import() {
    let cancel = AtomicBool::new(true);
    let err = import(&data("fixture.xlsx"), &cancel, &Progress::default());
    // The fixture is smaller than one cancel check; the index build sees the flag.
    assert!(matches!(err, Err(xlsx::ImportError::Cancelled)));
}

/// LibreOffice reads the round-trip save as the fixture plus the edits.
#[test]
#[ignore = "needs LibreOffice"]
fn libreoffice_reads_save() {
    let (dest, _) = edited_fixture_save("libreoffice");
    let out = Command::new("python3")
        .arg(Path::new(ROOT).join("scripts/lo_xlsx.py"))
        .arg("dump")
        .arg(&dest)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let fixture = std::fs::read_to_string(data("fixture-libreoffice.txt")).unwrap();
    let expected = fixture
        .replace("A2\tvalue\t1.0\t", "A2\tvalue\t99.0\t")
        .replace(
            "A1\tformula:value\t25.5\tData!B2*2",
            "A1\tformula:value\t75.75\tData!B3*3",
        )
        .replace("A1\ttext\tSummary\t", "A1\ttext\tTitle, \"edited\"\t")
        .replace(
            "A2\tformula:value\t138.0\tAVERAGE",
            "A2\tformula:value\t143.025\tAVERAGE",
        );
    let got = String::from_utf8(out.stdout).unwrap();
    for (g, e) in got.lines().zip(expected.lines()) {
        assert_eq!(g, e);
    }
    assert_eq!(got.lines().count(), expected.lines().count());
}

fn peak_rss_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
        .unwrap_or(0)
}

#[test]
#[ignore = "perf: needs corpus/rows_1m.xlsx (cargo run --release -p xlsx --example gen_corpus_xlsx)"]
fn import_1m() {
    let Some(path) = corpus("rows_1m.xlsx") else {
        return;
    };
    let start = Instant::now();
    let imported = import_file(&path);
    let secs = start.elapsed().as_secs_f64();
    let rows = imported.sheets[0].table.total_rows();
    println!(
        "import_1m: {rows} rows in {secs:.2} s, peak RSS {} MiB",
        peak_rss_kib() / 1024
    );
    assert_eq!(rows, 1_048_576);
    if cfg!(not(debug_assertions)) {
        assert!(secs < 10.0, "{secs:.2} s");
    }
}

#[test]
#[ignore = "perf: needs corpus/rows_1m.xlsx (cargo run --release -p xlsx --example gen_corpus_xlsx)"]
fn save_1m() {
    let Some(path) = corpus("rows_1m.xlsx") else {
        return;
    };
    // One edit: the sheet is written again, every unchanged cell copied from the file.
    let (mut book, package) = workbook(import_file(&path));
    let at = CellRef {
        row: RowId::source(500_000),
        col: ColId::source(1),
    };
    book.active_mut().apply(Edit::set(at, "edited"));
    let dest = temp("save_1m", "saved.xlsx");
    let start = Instant::now();
    let stats = save_book(&dest, &book, Some(package), &HashMap::new());
    let secs = start.elapsed().as_secs_f64();
    println!(
        "save_1m: {} rows, {} MiB in {secs:.2} s, peak RSS {} MiB",
        stats.rows,
        stats.bytes >> 20,
        peak_rss_kib() / 1024
    );
    std::fs::remove_file(dest).unwrap();
    if cfg!(not(debug_assertions)) {
        assert!(secs < 30.0, "{secs:.2} s");
    }
}
