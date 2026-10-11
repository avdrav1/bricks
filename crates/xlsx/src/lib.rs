//! XLSX workbooks (ADR 0006): import streams every worksheet into a CSV copy in the cache
//! directory, indexed like any CSV file, so every table feature works per sheet, and reads
//! the workbook's styles and each sheet's cell styles, sizes, and merged ranges (ADR 0009);
//! export writes typed values, formulas with their results, defined names, and column
//! widths.

mod detect;
pub mod export;
mod import;
mod layout;
pub mod losses;
mod package;
mod styles;
mod xml;

pub use detect::{kind, refusal, Kind};
pub use export::{check_limits, save_xlsx, write_xlsx, ExportSheet, MAX_COLS, MAX_ROWS};
pub use import::{import, value_text, ImportError, Imported, ImportedSheet, Progress};
pub use losses::{Loss, Losses};
