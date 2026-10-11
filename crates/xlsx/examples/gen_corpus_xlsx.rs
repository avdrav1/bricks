//! Write `corpus/rows_1m.xlsx` (BENCH-2, XLSX-1's `import_1m`): one sheet "rows" with the
//! columns of `scripts/gen_corpus.py` and 1,048,575 data rows, filling a sheet. Values are
//! typed as Excel would hold them: integers and decimals as numbers, dates as dates,
//! `TRUE`/`FALSE` as booleans, zero-padded codes as text. Deterministic; constant memory.
//!
//! Usage: `cargo run --release -p xlsx --example gen_corpus_xlsx [OUT]`

use rust_xlsxwriter::{ExcelDateTime, Format, Workbook, XlsxError};
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

fn main() -> Result<(), XlsxError> {
    let out = std::env::args_os().nth(1).map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/rows_1m.xlsx"),
        PathBuf::from,
    );
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let start = std::time::Instant::now();
    let mut book = Workbook::new();
    let date = Format::new().set_num_format("yyyy-mm-dd");
    let sheet = book.add_worksheet_with_constant_memory();
    sheet.set_name("rows")?;
    for (c, h) in HEADER.iter().enumerate() {
        sheet.write_string(0, c as u16, *h)?;
    }
    let mut rng = Rng(42);
    for i in 0..ROWS {
        let r = i + 1;
        sheet.write_number(r, 0, f64::from(i))?;
        sheet.write_string(r, 1, NAMES[rng.below(NAMES.len() as u64) as usize])?;
        sheet.write_string(r, 2, CITIES[rng.below(CITIES.len() as u64) as usize])?;
        sheet.write_number(r, 3, rng.below(100_000) as f64)?;
        // Two decimals, like gen_corpus.py's `f"{x:.2f}"`.
        sheet.write_number(r, 4, rng.below(100_000) as f64 / 100.0)?;
        let day = ExcelDateTime::from_ymd(2026, 1 + rng.below(12) as u8, 1 + rng.below(28) as u8)?;
        sheet.write_datetime_with_format(r, 5, day, &date)?;
        sheet.write_boolean(r, 6, rng.below(2) == 1)?;
        sheet.write_string(r, 7, format!("{:05}", rng.below(1000)))?;
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
