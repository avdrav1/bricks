//! XLSX workbooks (ADR 0006): import streams every worksheet into a CSV copy in the cache
//! directory, indexed like any CSV file, so every table feature works per sheet; export
//! writes typed values, formulas with their results, defined names, and column widths.
//! Formatting, charts, comments, and the rest of a workbook are not modelled.

mod detect;
pub mod export;
mod import;
pub mod losses;

pub use detect::{kind, refusal, Kind};
pub use export::{check_limits, save_xlsx, write_xlsx, ExportSheet, MAX_COLS, MAX_ROWS};
pub use import::{import, value_text, ImportError, Imported, ImportedSheet, Progress};
pub use losses::{Loss, Losses};
