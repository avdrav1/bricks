//! STYLE-6: formats the user set save. A bold cell, a filled range, a column style, and a
//! merge on the fixture (a patched save) and on a CSV (a whole new package) read back with
//! the same look for every cell, and LibreOffice reads them (`scripts/lo_xlsx.py styles`).

use data_model::{
    BookEdit, CellArea, CellRef, Color, CsvTable, DelimiterChoice, Fill, FormatTarget, Origin,
    Pattern, SaveProgress, SheetId, StylePatch, TableSource, Workbook,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;
use xlsx::{import, save, Package, Progress, SaveBook};

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

fn temp(test: &str, name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("brick-format-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn open(path: &Path) -> (Workbook, Arc<Package>) {
    let imported = import(path, &AtomicBool::new(false), &Progress::default()).unwrap();
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

fn save_book(dest: &Path, book: &Workbook, package: Option<Arc<Package>>) {
    let book = SaveBook::from_workbook(
        book.snapshot(),
        package,
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

fn format(book: &mut Workbook, sheet: SheetId, target: FormatTarget, patch: StylePatch) {
    let edit = book.format_edit(sheet, target, &patch).unwrap();
    book.apply(BookEdit::Sheets(vec![(sheet, edit)]));
}

/// Bold A1, a yellow B2:C3, an italic column E, and G1:H2 merged, on the first sheet.
fn formatted(book: &mut Workbook) {
    let sheet = book.sheets()[0].id;
    let cells = |rows, cols| FormatTarget::Cells { rows, cols };
    format(
        book,
        sheet,
        cells(0..1, 0..1),
        StylePatch {
            bold: Some(true),
            ..StylePatch::default()
        },
    );
    format(
        book,
        sheet,
        cells(1..3, 1..3),
        StylePatch {
            fill: Some(Some(Color::Rgb(0xFFFF00))),
            ..StylePatch::default()
        },
    );
    format(
        book,
        sheet,
        FormatTarget::Cols(4..5),
        StylePatch {
            italic: Some(true),
            ..StylePatch::default()
        },
    );
    let t = &mut book.sheet_mut(sheet).unwrap().table;
    let plan = t
        .merge_plan(CellArea {
            first_row: 0,
            first_col: 6,
            last_row: 1,
            last_col: 7,
        })
        .unwrap();
    book.apply(BookEdit::Sheets(
        plan.edits.into_iter().map(|e| (sheet, e)).collect(),
    ));
}

/// Every cell's look on every sheet (rows shown, columns A to J): what its style
/// resolves to, so pools with other indices compare.
fn looks(book: &Workbook) -> Vec<(String, Vec<String>)> {
    let pool = book.styles();
    book.sheets()
        .iter()
        .map(|s| {
            let t = &s.table;
            let mut out = Vec::new();
            for k in 0..t.total_rows() {
                for c in 0..10 {
                    let xf = pool.xf(t.style_of(CellRef {
                        row: t.id_at(k),
                        col: t.col_id(c),
                    }));
                    let font = pool.font(xf);
                    let fill = match pool.fill(xf) {
                        Some(Fill::Pattern {
                            pattern: Pattern::Solid,
                            fg,
                            ..
                        }) => pool.rgb(*fg),
                        _ => None,
                    };
                    out.push(format!(
                        "{k},{c}: {} {} {} {} {:?} {fill:?} {:?} {:?}",
                        font.name,
                        font.size,
                        font.bold,
                        font.italic,
                        pool.rgb(font.color),
                        pool.border(xf).map(|b| (b.top.style, b.bottom.style)),
                        xf.align.horizontal
                    ));
                }
            }
            (s.name.clone(), out)
        })
        .collect()
}

#[test]
fn formats_on_the_fixture_save_by_patching_and_read_back() {
    let (mut book, package) = open(&data("fixture.xlsx"));
    let unformatted = looks(&book);
    formatted(&mut book);
    let want = looks(&book);
    assert_ne!(want, unformatted);
    let dest = temp("patched", "saved.xlsx");
    save_book(&dest, &book, Some(package));
    let (back, _) = open(&dest);
    assert_eq!(looks(&back), want);
    assert_eq!(
        back.sheets()[0].table.merges().len(),
        book.sheets()[0].table.merges().len()
    );
    // Other sheets were copied as they were.
    for (a, b) in looks(&back).iter().zip(&unformatted).skip(1) {
        assert_eq!(a, b);
    }
}

fn csv_book(test: &str) -> Workbook {
    let path = temp(test, "t.csv");
    let rows: String = (0..6)
        .map(|r| format!("{r},a{r},b{r},c{r},d{r},e{r},f{r},g{r},h{r}\n"))
        .collect();
    std::fs::write(&path, rows).unwrap();
    let mut t = CsvTable::open(&path, DelimiterChoice::Fixed(b',')).unwrap();
    t.index().build(&AtomicBool::new(false)).unwrap();
    t.set_header(false);
    Workbook::single("csv".into(), t, Origin::Csv)
}

#[test]
fn formats_on_a_csv_save_in_a_new_package_and_read_back() {
    let mut book = csv_book("fresh");
    formatted(&mut book);
    assert!(book.has_formatting());
    let want = looks(&book);
    let dest = temp("fresh", "saved.xlsx");
    save_book(&dest, &book, None);
    let (back, _) = open(&dest);
    assert_eq!(looks(&back), want);
    assert_eq!(
        back.sheets()[0].table.merges(),
        book.sheets()[0].table.merges()
    );
    assert!(back.sheets()[0].table.row_count() > 0);
}

/// Run `scripts/lo_xlsx.py MODE FILE` on LibreOffice's port `LO_PORT` (2141 unless set).
fn lo(mode: &str, file: &Path) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../..");
    let port = std::env::var("LO_PORT").unwrap_or_else(|_| "2141".into());
    let out = std::process::Command::new("python3")
        .arg(root.join("scripts/lo_xlsx.py"))
        .args([mode, &file.to_string_lossy()])
        .env("LO_PORT", port)
        .output()
        .expect("python3");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// The `styles` dump's line for cell `cell` on the first sheet.
fn line<'a>(dump: &'a str, cell: &str) -> &'a str {
    let first = dump.split("\n== ").next().unwrap();
    first
        .lines()
        .find(|l| l.split('\t').next() == Some(cell))
        .unwrap_or_else(|| panic!("no {cell} in {first}"))
}

fn font_flags(line: &str) -> (bool, bool) {
    let font = line.split('\t').nth(1).unwrap();
    let parts: Vec<&str> = font.split('|').collect();
    (parts[2] == "b", parts[3] == "i")
}

#[test]
#[ignore = "needs LibreOffice: LO_PORT=2141 cargo test -p xlsx --test format -- --ignored"]
fn libreoffice_reads_the_formats() {
    let (mut fixture, package) = open(&data("fixture.xlsx"));
    formatted(&mut fixture);
    let patched = temp("lo", "patched.xlsx");
    save_book(&patched, &fixture, Some(package));
    let mut csv = csv_book("lo");
    formatted(&mut csv);
    let fresh = temp("lo", "fresh.xlsx");
    save_book(&fresh, &csv, None);
    for file in [&patched, &fresh] {
        let dump = lo("styles", file);
        assert_eq!(font_flags(line(&dump, "A1")), (true, false), "A1 bold");
        assert_eq!(font_flags(line(&dump, "B1")), (false, false), "B1 plain");
        for cell in ["B2", "C2", "B3", "C3"] {
            assert!(
                line(&dump, cell).contains("\tfill=FFFF00\t"),
                "{cell} yellow"
            );
        }
        assert!(line(&dump, "D2").contains("\tfill=none\t"), "D2 not filled");
        for cell in ["E1", "E2", "E5"] {
            assert_eq!(
                font_flags(line(&dump, cell)),
                (false, true),
                "{cell} italic"
            );
        }
        assert!(dump.contains("\nmerge G1:H2\n"), "G1:H2 merged");
    }
}
