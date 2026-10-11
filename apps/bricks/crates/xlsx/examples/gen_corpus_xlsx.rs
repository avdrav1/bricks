//! Write `corpus/rows_1m.xlsx` (BENCH-2, XLSX-1's `import_1m`): one sheet "rows" with the
//! columns of `scripts/gen_corpus.py` and 1,048,575 data rows, filling a sheet. Values are
//! typed as Excel would hold them: integers and decimals as numbers, dates as dates,
//! `TRUE`/`FALSE` as booleans, zero-padded codes as text. Deterministic; constant memory.
//!
//! With `--styled`, writes `corpus/styled_1m.xlsx` instead (STYLE-3, BENCH-3): the same
//! rows with a bold header on a dark fill, number formats per column, an italic coloured
//! column, and every 100th row a bold total row on a light fill with a top border.
//!
//! Usage: `cargo run --release -p xlsx --example gen_corpus_xlsx [--styled] [OUT]`

use rust_xlsxwriter::{Color, ExcelDateTime, Format, FormatBorder, Workbook, XlsxError};
use std::path::PathBuf;

const ROWS: u32 = 1_048_575;
const HEADER: [&str; 8] = [
    "id", "name", "city", "revenue", "score", "date", "active", "code",
];
const CITIES: [&str; 7] = [
    "Boston", "Philly", "Chicago", "Austin", "Denver", "Portland", "Miami",
];
const NAMES: [&str; 8] = [
    "Alice", "David", "Maya", "Omar", "Lena", "Yuki", "Priya", "Sam",
];

/// SplitMix64: a fixed-seed generator, so every run writes the same file.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `0..n`.
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// The formats of the styled corpus; `None` for the plain one.
struct Styles {
    header: Format,
    revenue: Format,
    score: Format,
    city: Format,
    total: Format,
    total_revenue: Format,
    total_score: Format,
    total_date: Format,
}

impl Styles {
    fn new() -> Self {
        let total = Format::new()
            .set_bold()
            .set_background_color(Color::RGB(0xDDEBF7))
            .set_border_top(FormatBorder::Thin);
        Self {
            header: Format::new()
                .set_bold()
                .set_font_color(Color::White)
                .set_background_color(Color::RGB(0x305496))
                .set_border_bottom(FormatBorder::Medium),
            revenue: Format::new().set_num_format("#,##0"),
            score: Format::new().set_num_format("0.00"),
            city: Format::new()
                .set_italic()
                .set_font_color(Color::Theme(4, 4)),
            total_revenue: total.clone().set_num_format("#,##0"),
            total_score: total.clone().set_num_format("0.00"),
            total_date: total.clone().set_num_format("yyyy-mm-dd"),
            total,
        }
    }
}

fn main() -> Result<(), XlsxError> {
    let mut args: Vec<_> = std::env::args_os().skip(1).collect();
    let styled = args.first().is_some_and(|a| a == "--styled");
    if styled {
        args.remove(0);
    }
    let name = if styled {
        "styled_1m.xlsx"
    } else {
        "rows_1m.xlsx"
    };
    let out = args.first().map_or_else(
        || {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../../../corpus")
                .join(name)
        },
        PathBuf::from,
    );
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let start = std::time::Instant::now();
    let mut book = Workbook::new();
    let date = Format::new().set_num_format("yyyy-mm-dd");
    let styles = styled.then(Styles::new);
    let sheet = book.add_worksheet_with_constant_memory();
    sheet.set_name("rows")?;
    if styles.is_some() {
        for (c, w) in [(1, 12.0), (2, 14.0), (3, 12.0), (5, 12.0)] {
            sheet.set_column_width(c, w)?;
        }
        sheet.set_row_height(0, 24.0)?;
    }
    for (c, h) in HEADER.iter().enumerate() {
        match &styles {
            Some(s) => sheet.write_string_with_format(0, c as u16, *h, &s.header)?,
            None => sheet.write_string(0, c as u16, *h)?,
        };
    }
    let mut rng = Rng(42);
    for i in 0..ROWS {
        let r = i + 1;
        let total = styles.as_ref().filter(|_| i % 100 == 99);
        let (id, name, city) = (
            f64::from(i),
            NAMES[rng.below(NAMES.len() as u64) as usize],
            CITIES[rng.below(CITIES.len() as u64) as usize],
        );
        let (revenue, score) = (rng.below(100_000) as f64, rng.below(100_000) as f64 / 100.0);
        let day = ExcelDateTime::from_ymd(2026, 1 + rng.below(12) as u8, 1 + rng.below(28) as u8)?;
        let (active, code) = (rng.below(2) == 1, format!("{:05}", rng.below(1000)));
        match (&styles, total) {
            (None, _) => {
                sheet.write_number(r, 0, id)?;
                sheet.write_string(r, 1, name)?;
                sheet.write_string(r, 2, city)?;
                sheet.write_number(r, 3, revenue)?;
                // Two decimals, like gen_corpus.py's `f"{x:.2f}"`.
                sheet.write_number(r, 4, score)?;
                sheet.write_datetime_with_format(r, 5, day, &date)?;
                sheet.write_boolean(r, 6, active)?;
                sheet.write_string(r, 7, code)?;
            }
            (Some(_), Some(s)) => {
                sheet.write_number_with_format(r, 0, id, &s.total)?;
                sheet.write_string_with_format(r, 1, name, &s.total)?;
                sheet.write_string_with_format(r, 2, city, &s.total)?;
                sheet.write_number_with_format(r, 3, revenue, &s.total_revenue)?;
                sheet.write_number_with_format(r, 4, score, &s.total_score)?;
                sheet.write_datetime_with_format(r, 5, day, &s.total_date)?;
                sheet.write_boolean_with_format(r, 6, active, &s.total)?;
                sheet.write_string_with_format(r, 7, code, &s.total)?;
            }
            (Some(s), None) => {
                sheet.write_number(r, 0, id)?;
                sheet.write_string(r, 1, name)?;
                sheet.write_string_with_format(r, 2, city, &s.city)?;
                sheet.write_number_with_format(r, 3, revenue, &s.revenue)?;
                sheet.write_number_with_format(r, 4, score, &s.score)?;
                sheet.write_datetime_with_format(r, 5, day, &date)?;
                sheet.write_boolean(r, 6, active)?;
                sheet.write_string(r, 7, code)?;
            }
        }
    }
    book.save(&out)?;
    println!(
        "{}: {} rows in {:.1} s",
        out.display(),
        ROWS + 1,
        start.elapsed().as_secs_f64()
    );
    Ok(())
}
