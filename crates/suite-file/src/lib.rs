//! Format-independent file infrastructure shared by every app in the suite.
//!
//! Atomic saving lives here ([`save_atomic`]), crash recovery journals' directory
//! ([`recovery`]: atomic journal writes, finding a crashed process's journals), which
//! Brick, Slate, and Etch share, owner locks telling which processes keeping files in a
//! folder still run ([`owner`]: recovery's and Slate's package copies'), and the recent
//! files list ([`recent`]), Brick's and Etch's. So do the paths the apps compare and keep:
//! [`canonical`] names an open file the same way every time, and [`path_bytes`] and
//! [`path_from_bytes`] keep a path in the suite's own files (recent lists, journals).
//! Save-as, file metadata, and temp files may move here once two apps need them. Format
//! parsers (CSV, XLSX, DOCX, PPTX) stay in their app's crates.

pub mod owner;
pub mod recent;
pub mod recovery;
mod save;

pub use save::{save_atomic, SaveError, WriteSeek, TEMP_SUFFIX};

use std::borrow::Cow;
use std::path::{Path, PathBuf};

/// The name a document is shown by, in window titles and messages: the path's file name,
/// or the whole path when it has none (`/`, `..`). Non-UTF-8 bytes become U+FFFD.
pub fn document_name(path: &Path) -> Cow<'_, str> {
    match path.file_name() {
        Some(name) => name.to_string_lossy(),
        None => path.to_string_lossy(),
    }
}

/// `path` made absolute with every link resolved, so one file always has one name: an
/// app finds the window already showing it, and the recent list keeps it once. `path`
/// itself if that fails (a file that's gone). On Windows the name is the one people know
/// (`C:\Users\me\a.csv`), not the `\\?\C:\Users\me\a.csv` that resolving gives, unless only
/// the `\\?\` form names the file (a name too long, or one Windows reserves, like `CON`).
pub fn canonical(path: &Path) -> PathBuf {
    without_verbatim_prefix(std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned()))
}

/// `path` without its `\\?\` (or `\\?\UNC\`) prefix, when the path means the same file
/// without it: a drive or network path short enough for every program (under `MAX_PATH`,
/// 260), whose parts Windows takes as they are (no reserved device name, no trailing dot
/// or space). Only Windows paths have prefixes; others come back as they are.
fn without_verbatim_prefix(path: PathBuf) -> PathBuf {
    use std::path::{Component, Prefix};
    let Some(text) = path.to_str() else {
        return path;
    };
    let plain = match path.components().next() {
        Some(Component::Prefix(p)) => match p.kind() {
            Prefix::VerbatimDisk(_) => text[r"\\?\".len()..].to_owned(),
            Prefix::VerbatimUNC(..) => format!(r"\\{}", &text[r"\\?\UNC\".len()..]),
            _ => return path,
        },
        _ => return path,
    };
    const MAX_PATH: usize = 260;
    const RESERVED: [&str; 4] = ["CON", "PRN", "AUX", "NUL"];
    let usable = |name: &str| {
        let stem = name.split('.').next().unwrap_or(name).trim_end().as_bytes();
        let device = RESERVED
            .iter()
            .any(|r| stem.eq_ignore_ascii_case(r.as_bytes()))
            || (stem.len() == 4
                && ["COM", "LPT"]
                    .iter()
                    .any(|r| stem[..3].eq_ignore_ascii_case(r.as_bytes()))
                && stem[3].is_ascii_digit());
        !device && !name.ends_with(['.', ' ']) && !name.contains('/')
    };
    let plain_path = Path::new(&plain);
    let fine = plain.encode_utf16().count() < MAX_PATH
        && plain_path.is_absolute()
        && plain_path.components().all(|c| match c {
            Component::Prefix(_) | Component::RootDir => true,
            Component::Normal(name) => name.to_str().is_some_and(usable),
            Component::CurDir | Component::ParentDir => false,
        });
    if fine {
        PathBuf::from(plain)
    } else {
        path
    }
}

/// `path` as bytes for a file the suite keeps for itself and reads back on the same
/// system (recent lists, recovery journals). On Unix that's any path, as its own bytes; on
/// Windows a path that's Unicode, as UTF-8, which every path GTK gives an app is (GLib's
/// file names are UTF-8 there), and `None` for one that isn't.
pub fn path_bytes(path: &Path) -> Option<&[u8]> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Some(path.as_os_str().as_bytes())
    }
    #[cfg(not(unix))]
    {
        path.to_str().map(str::as_bytes)
    }
}

/// The path [`path_bytes`] gave `bytes` for; `None` if no path would have.
pub fn path_from_bytes(bytes: &[u8]) -> Option<PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Some(PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
    }
    #[cfg(not(unix))]
    {
        std::str::from_utf8(bytes).ok().map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[cfg(unix)]
    #[test]
    fn document_name_replaces_invalid_utf8() {
        use std::os::unix::ffi::OsStrExt;
        let path = Path::new(std::ffi::OsStr::from_bytes(b"/tmp/bad\xff.csv"));
        assert_eq!(document_name(path), "bad\u{fffd}.csv");
    }

    #[test]
    fn paths_round_trip_through_bytes() {
        let dir = std::env::temp_dir();
        for path in [
            dir.join("spa ce, ünï.csv"),
            dir.join("a\nb.csv"),
            PathBuf::new(),
        ] {
            let bytes = path_bytes(&path).unwrap();
            assert_eq!(path_from_bytes(bytes), Some(path.clone()), "{path:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn any_unix_path_round_trips_through_bytes() {
        use std::os::unix::ffi::OsStrExt;
        let path = Path::new(std::ffi::OsStr::from_bytes(b"/tmp/caf\xe9.csv"));
        assert_eq!(path_bytes(path), Some(&b"/tmp/caf\xe9.csv"[..]));
        assert_eq!(path_from_bytes(b"/tmp/caf\xe9.csv").as_deref(), Some(path));
    }

    #[cfg(windows)]
    #[test]
    fn windows_paths_are_kept_as_utf8() {
        use std::os::windows::ffi::OsStringExt;
        let path = PathBuf::from(r"C:\Users\me\café.csv");
        assert_eq!(path_bytes(&path), Some(r"C:\Users\me\café.csv".as_bytes()));
        // An unpaired surrogate: no UTF-8 for it.
        let odd = PathBuf::from(std::ffi::OsString::from_wide(&[0x43, 0x3a, 0x5c, 0xd800]));
        assert_eq!(path_bytes(&odd), None);
        assert_eq!(path_from_bytes(b"C:\\caf\xe9.csv"), None);
    }

    #[test]
    fn canonical_names_a_file_one_way() {
        let dir = std::env::temp_dir().join(format!("suite-file-canonical-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let file = dir.join("a.csv");
        std::fs::write(&file, "").unwrap();
        let name = canonical(&file);
        assert!(name.is_absolute(), "{name:?}");
        assert_eq!(canonical(&dir.join("sub").join("..").join("a.csv")), name);
        assert_eq!(canonical(&name), name);
        assert!(
            !name.to_string_lossy().starts_with(r"\\?\"),
            "no verbatim prefix: {name:?}"
        );
        let gone = dir.join("gone.csv");
        assert_eq!(canonical(&gone), gone, "a missing file keeps its path");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn verbatim_prefixes_go_only_when_the_path_means_the_same_without() {
        let plain = |p: &str| without_verbatim_prefix(PathBuf::from(p));
        assert_eq!(
            plain(r"\\?\C:\Users\me\a.csv"),
            Path::new(r"C:\Users\me\a.csv")
        );
        assert_eq!(plain(r"\\?\C:\"), Path::new(r"C:\"));
        assert_eq!(
            plain(r"\\?\UNC\server\share\a.csv"),
            Path::new(r"\\server\share\a.csv")
        );
        assert_eq!(plain(r"C:\Users\me\a.csv"), Path::new(r"C:\Users\me\a.csv"));
        for kept in [
            r"\\?\C:\Users\me\CON".to_owned(),
            r"\\?\C:\Users\me\nul.txt".to_owned(),
            r"\\?\C:\Users\me\com1.csv".to_owned(),
            r"\\?\C:\Users\me\dot.".to_owned(),
            r"\\?\C:\Users\me\space ".to_owned(),
            format!(r"\\?\C:\{}", "x".repeat(300)),
            r"\\?\Volume{12345678-1234-1234-1234-123456789abc}\a.csv".to_owned(),
        ] {
            assert_eq!(plain(&kept), Path::new(&kept), "{kept}");
        }
        assert_eq!(
            plain(r"\\?\C:\Users\me\console.csv"),
            Path::new(r"C:\Users\me\console.csv")
        );
    }
}
