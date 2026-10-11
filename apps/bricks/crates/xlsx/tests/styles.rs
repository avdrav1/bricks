//! STYLE-1 acceptance: every cell of the fixture resolves to the font, fill, borders,
//! alignment, and number format LibreOffice reports for it, and merged ranges, column
//! widths, row heights, and hidden rows and columns match (`scripts/lo_xlsx.py styles`
//! recorded `fixture-styles-libreoffice.txt`).

use data_model::{
    CellRef, ColId, CsvTable, Fill, HAlign, LineStyle, Pattern, RowId, StylePool, Underline, VAlign,
};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use xlsx::{import, Progress};

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

#[derive(Default)]
struct Dumped {
    name: String,
    cells: Vec<(u64, u32, Vec<String>)>,
    merges: BTreeSet<String>,
    cols: Vec<(u32, f64, bool)>,
    rows: Vec<(u64, f64, bool)>,
}

fn cell_ref(r: &str) -> (u64, u32) {
    let split = r.find(|c: char| c.is_ascii_digit()).unwrap();
    let col = formula::column_index(&r[..split]).unwrap();
    (r[split..].parse::<u64>().unwrap() - 1, col)
}

fn parse(dump: &str) -> Vec<Dumped> {
    let mut sheets: Vec<Dumped> = Vec::new();
    for line in dump.lines() {
        if let Some(name) = line.strip_prefix("== ") {
            sheets.push(Dumped {
                name: name.into(),
                ..Dumped::default()
            });
            continue;
        }
        let sheet = sheets.last_mut().unwrap();
        let words: Vec<&str> = line.split(' ').collect();
        match words[0] {
            "merge" => {
                sheet.merges.insert(words[1].into());
            }
            "col" => sheet.cols.push((
                formula::column_index(words[1]).unwrap(),
                words[2].parse().unwrap(),
                words[3] == "1",
            )),
            "row" => sheet.rows.push((
                words[1].parse::<u64>().unwrap() - 1,
                words[2].parse().unwrap(),
                words[3] == "1",
            )),
            _ => {
                let fields: Vec<&str> = line.split('\t').collect();
                let (row, col) = cell_ref(fields[0]);
                sheet.cells.push((
                    row,
                    col,
                    fields[1..].iter().map(|f| f.to_string()).collect(),
                ));
            }
        }
    }
    sheets
}

fn letters(col: u32) -> String {
    let mut s = String::new();
    formula::column_name(col, &mut s);
    s
}

fn hex(rgb: Option<u32>, unset: &str) -> String {
    rgb.map_or_else(|| unset.to_owned(), |c| format!("{c:06X}"))
}

/// A cell's style written the way the dump writes LibreOffice's view of it.
fn described(pool: &StylePool, table: &CsvTable, row: u64, col: u32) -> Vec<String> {
    let at = CellRef {
        row: RowId::source(row),
        col: ColId::source(col),
    };
    let xf = pool.xf(table.style_of(at));
    let font = pool.font(xf);
    let size = f64::from(font.size) / 100.0;
    let underline = match font.underline {
        Underline::None => 0,
        Underline::Single | Underline::SingleAccounting => 1,
        Underline::Double | Underline::DoubleAccounting => 2,
    };
    let font = format!(
        "font={}|{}|{}|{}|u{underline}|s{}|{}",
        font.name,
        size,
        if font.bold { "b" } else { "" },
        if font.italic { "i" } else { "" },
        u8::from(font.strike),
        hex(pool.rgb(font.color), "auto"),
    );
    let fill = match pool.fill(xf) {
        Some(Fill::Pattern {
            pattern: Pattern::Solid,
            fg,
            ..
        }) => hex(pool.rgb(*fg), "none"),
        _ => "none".into(),
    };
    let border = pool.border(xf).cloned().unwrap_or_default();
    // LibreOffice's BorderLineStyle and width (1/100 mm) for each line style it writes.
    let side = |e: &data_model::Edge| {
        let (style, width) = match e.style {
            LineStyle::None => return "-".to_owned(),
            LineStyle::Thin => (0, 26),
            LineStyle::Medium => (0, 62),
            LineStyle::Thick => (0, 88),
            LineStyle::Dashed => (14, 26),
            LineStyle::Dotted => (1, 26),
            LineStyle::DashDot => (16, 26),
            other => panic!("the fixture has no {other:?} borders"),
        };
        format!("{style}:{width}:{}", hex(pool.rgb(e.color), "000000"))
    };
    let border = format!(
        "border={},{},{},{}",
        side(&border.top),
        side(&border.bottom),
        side(&border.left),
        side(&border.right)
    );
    let a = xf.align;
    let horizontal = match a.horizontal {
        HAlign::General => "STANDARD",
        HAlign::Left => "LEFT",
        HAlign::Center | HAlign::CenterContinuous => "CENTER",
        HAlign::Right => "RIGHT",
        HAlign::Justify | HAlign::Distributed => "BLOCK",
        HAlign::Fill => "REPEAT",
    };
    let vertical = match a.vertical {
        VAlign::Top => 1,
        VAlign::Center => 2,
        VAlign::Bottom => 3,
        VAlign::Justify | VAlign::Distributed => 4,
    };
    let rotation = match a.rotation {
        r @ 0..=90 => u32::from(r) * 100,
        r @ 91..=180 => (360 - (u32::from(r) - 90)) * 100,
        _ => 0,
    };
    let align = format!(
        "align={horizontal}|{vertical}|{}|{rotation}|{}",
        if a.wrap { "wrap" } else { "" },
        a.indent
    );
    let code = match pool.custom_format(xf.num_fmt) {
        Some(code) => code.to_owned(),
        None if xf.num_fmt == 0 => "General".to_owned(),
        None => panic!(
            "the fixture's cells use custom formats, not built-in {}",
            xf.num_fmt
        ),
    };
    vec![
        font,
        format!("fill={fill}"),
        border,
        align,
        format!("fmt={}", normal_format(&code)),
    ]
}

/// A format code as both sides spell it: LibreOffice adds locale tags and escapes and
/// drops quotes around single characters; the meaning is the same.
fn normal_format(code: &str) -> String {
    let mut out = String::new();
    let mut chars = code.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '[' if chars.peek() == Some(&'$') => {
                let tag: String = chars.by_ref().take_while(|&c| c != ']').collect();
                if !tag.starts_with("$-") {
                    out.push('[');
                    out.push_str(&tag);
                    out.push(']');
                }
            }
            '\\' => out.extend(chars.next()),
            '"' => out.extend(chars.by_ref().take_while(|&c| c != '"')),
            c => out.push(c),
        }
    }
    out.to_uppercase()
}

/// The dump's alignment field with LibreOffice's indent (1/100 mm) as indent steps.
fn indent_steps(field: &str) -> String {
    let (head, indent) = field.rsplit_once('|').unwrap();
    let steps = (indent.parse::<f64>().unwrap() / 296.5).round();
    format!("{head}|{steps}")
}

#[test]
fn every_cell_resolves_to_the_style_libreoffice_reads() {
    let dump = parse(&std::fs::read_to_string(data("fixture-styles-libreoffice.txt")).unwrap());
    let imported = import(
        &data("fixture.xlsx"),
        &AtomicBool::new(false),
        &Progress::default(),
    )
    .unwrap();
    let pool = &imported.styles;
    assert_eq!(imported.sheets.len(), dump.len());
    let mut styled = 0;
    for (sheet, dumped) in imported.sheets.iter().zip(&dump) {
        assert_eq!(sheet.name, dumped.name);
        for (row, col, want) in &dumped.cells {
            let mut want = want.clone();
            want[3] = indent_steps(&want[3]);
            let fmt = want[4].strip_prefix("fmt=").unwrap();
            want[4] = format!("fmt={}", normal_format(fmt));
            let got = described(pool, &sheet.table, *row, *col);
            assert_eq!(got, want, "{}!{}{}", sheet.name, letters(*col), row + 1);
            styled += usize::from(
                sheet.table.style_of(CellRef {
                    row: RowId::source(*row),
                    col: ColId::source(*col),
                }) != data_model::StyleId(0),
            );
        }
    }
    assert!(
        styled > 60,
        "only {styled} styled cells: the fixture lost its styles"
    );
}

#[test]
fn merges_widths_heights_and_hidden_match() {
    let dump = parse(&std::fs::read_to_string(data("fixture-styles-libreoffice.txt")).unwrap());
    let imported = import(
        &data("fixture.xlsx"),
        &AtomicBool::new(false),
        &Progress::default(),
    )
    .unwrap();
    for (sheet, dumped) in imported.sheets.iter().zip(&dump) {
        let layout = sheet.table.layout();
        let merges: BTreeSet<String> = layout
            .merges
            .iter()
            .map(|m| {
                format!(
                    "{}{}:{}{}",
                    letters(m.first_col),
                    m.first_row + 1,
                    letters(m.last_col),
                    m.last_row + 1
                )
            })
            .collect();
        assert_eq!(merges, dumped.merges, "{}", sheet.name);

        // Widths compare as multiples of the default column width: LibreOffice measures
        // characters with its own font metrics.
        let lo_default = dumped.cols.iter().map(|c| c.1).fold(f64::MAX, f64::min);
        let default = layout.default_col_width.unwrap();
        for &(col, lo_width, lo_hidden) in &dumped.cols {
            let spec = layout.col(col);
            let width = spec.and_then(|s| s.width).unwrap_or(default);
            let hidden = spec.is_some_and(|s| s.hidden);
            assert_eq!(hidden, lo_hidden, "{} column {col} hidden", sheet.name);
            if !hidden {
                let ratio = (width / default) / (lo_width / lo_default);
                assert!(
                    (ratio - 1.0).abs() < 0.01,
                    "{} column {col}: {width} vs {lo_width}",
                    sheet.name
                );
            }
        }
        let default = layout.default_row_height.unwrap();
        for &(row, lo_height, lo_hidden) in &dumped.rows {
            let spec = layout.row(row);
            let height = spec.and_then(|s| s.height).unwrap_or(default);
            assert_eq!(
                spec.is_some_and(|s| s.hidden),
                lo_hidden,
                "{} row {}",
                sheet.name,
                row + 1
            );
            let hundredths_mm = height * 2540.0 / 72.0;
            assert!(
                (hundredths_mm - lo_height).abs() <= 2.0,
                "{} row {}: {height} pt vs {lo_height}",
                sheet.name,
                row + 1
            );
        }
    }
}

/// A sheet saved with grid lines off reads that way, another sheet of the same book
/// doesn't, and Excel's Center Across Selection reads as `CenterContinuous`.
#[test]
fn grid_lines_off_and_centre_across_read_per_sheet() {
    use rust_xlsxwriter::{Format, FormatAlign, Workbook};
    let mut book = Workbook::new();
    let across = Format::new().set_align(FormatAlign::CenterAcross);
    let plain = book.add_worksheet();
    plain.write_string(0, 0, "grid").unwrap();
    let bare = book.add_worksheet();
    bare.set_screen_gridlines(false);
    bare.write_string_with_format(1, 1, "Title", &across)
        .unwrap();
    bare.write_blank(1, 2, &across).unwrap();
    let dir = std::env::temp_dir().join(format!("bricks-xlsx-grid-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("grid.xlsx");
    book.save(&path).unwrap();
    let imported = import(&path, &AtomicBool::new(false), &Progress::default()).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    let hidden: Vec<bool> = imported
        .sheets
        .iter()
        .map(|s| s.table.layout().hide_grid_lines)
        .collect();
    assert_eq!(hidden, [false, true]);
    let table = &imported.sheets[1].table;
    for col in [1, 2] {
        let id = table.style_of(CellRef {
            row: RowId(1),
            col: ColId(col),
        });
        let xf = &imported.styles.xfs[id.0 as usize];
        assert_eq!(
            xf.align.horizontal,
            HAlign::CenterContinuous,
            "column {col}"
        );
    }
}
