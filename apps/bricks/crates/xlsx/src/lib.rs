//! XLSX workbooks (ADR 0006): import streams every worksheet into a CSV copy in the cache
//! directory, indexed like any CSV file, so every table feature works per sheet, and reads
//! the workbook's styles and each sheet's cell styles, sizes, and merged ranges (ADR 0009);
//! save patches the package the workbook was read from, keeping what Bricks doesn't edit
//! (ADR 0010).

mod detect;
mod import;
mod layout;
mod package;
mod save;
mod styles;
mod xml;

pub use detect::{kind, refusal, Kind};
pub use import::{import, value_text, ImportError, Imported, ImportedSheet, Progress};
pub use package::Package;
pub use save::{check_limits, save, SaveBook, SaveSheet, SheetSizes, MAX_COLS, MAX_ROWS};
