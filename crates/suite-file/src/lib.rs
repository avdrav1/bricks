//! Format-independent file infrastructure shared by every app in the suite.
//!
//! Atomic saving lives here ([`save_atomic`]). Save-as, file metadata, recent files, temp files, and
//! autosave/recovery may move here once two apps need them. Format parsers (CSV, XLSX,
//! DOCX, PPTX) stay in their app's crates.

mod save;

pub use save::{save_atomic, SaveError, WriteSeek, TEMP_SUFFIX};

use std::borrow::Cow;
use std::path::Path;

/// The name a document is shown by, in window titles and messages: the path's file name,
/// or the whole path when it has none (`/`, `..`). Non-UTF-8 bytes become U+FFFD.
pub fn document_name(path: &Path) -> Cow<'_, str> {
    match path.file_name() {
        Some(name) => name.to_string_lossy(),
        None => path.to_string_lossy(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    #[test]
    fn document_name_is_the_file_name() {
        assert_eq!(document_name(Path::new("/home/me/sales.csv")), "sales.csv");
        assert_eq!(document_name(Path::new("notes")), "notes");
    }

    #[test]
    fn document_name_without_a_file_name_is_the_path() {
        assert_eq!(document_name(Path::new("/")), "/");
        assert_eq!(document_name(Path::new("a/..")), "a/..");
    }

    #[test]
    fn document_name_replaces_invalid_utf8() {
        let path = Path::new(OsStr::from_bytes(b"/tmp/bad\xff.csv"));
        assert_eq!(document_name(path), "bad\u{fffd}.csv");
    }
}
