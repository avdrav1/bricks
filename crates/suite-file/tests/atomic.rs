//! `save_atomic` failure paths: whatever goes wrong before the rename, the original stays as
//! it was and no temp file is left behind.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use suite_file::{save_atomic, SaveError};

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("save1-atomic-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn entries(d: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(d)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    v.sort();
    v
}

#[test]
fn replaces_contents_and_keeps_permissions() {
    let d = dir("ok");
    let f = d.join("a.csv");
    std::fs::write(&f, "old\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o640)).unwrap();
    }
    save_atomic(
        &f,
        &AtomicBool::new(false),
        |w| w.write_all(b"new\n"),
        |tmp| {
            assert_eq!(
                std::fs::read(tmp).unwrap(),
                b"new\n",
                "verify sees the complete temp file"
            );
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(std::fs::read(&f).unwrap(), b"new\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&f).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o640);
    }
    assert_eq!(entries(&d), ["a.csv"]);
    std::fs::remove_dir_all(d).unwrap();
}

/// A file open for reading is replaced, and the open file still reads the old bytes, on
/// Windows too (WIN-1: the rename needs no folder sync, and Rust opens files shared for
/// delete).
#[test]
fn replaces_a_file_that_is_open() {
    let d = dir("open");
    let f = d.join("a.csv");
    std::fs::write(&f, "old\n").unwrap();
    let mut open = std::fs::File::open(&f).unwrap();
    save_atomic(
        &f,
        &AtomicBool::new(false),
        |w| w.write_all(b"new\n"),
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(std::fs::read(&f).unwrap(), b"new\n");
    let mut old = String::new();
    open.read_to_string(&mut old).unwrap();
    assert_eq!(old, "old\n");
    drop(open);
    assert_eq!(entries(&d), ["a.csv"]);
    std::fs::remove_dir_all(d).unwrap();
}

/// A read-only file isn't replaced on Windows: the save fails, and the original stays
/// with no temp file left (Unix's counterpart is a read-only folder: image-io's tests).
#[cfg(windows)]
#[test]
fn read_only_file_on_windows_stays_and_no_temp() {
    let d = dir("read-only");
    let f = d.join("a.csv");
    std::fs::write(&f, "old\n").unwrap();
    let mut perms = std::fs::metadata(&f).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&f, perms.clone()).unwrap();
    let r = save_atomic(
        &f,
        &AtomicBool::new(false),
        |w| w.write_all(b"new\n"),
        |_| Ok(()),
    );
    assert!(
        matches!(&r, Err(SaveError::Io(e)) if e.kind() == std::io::ErrorKind::PermissionDenied),
        "{r:?}"
    );
    assert_eq!(std::fs::read(&f).unwrap(), b"old\n");
    assert_eq!(entries(&d), ["a.csv"]);
    #[allow(clippy::permissions_set_readonly_false)] // Windows: clears the attribute
    perms.set_readonly(false);
    std::fs::set_permissions(&f, perms).unwrap();
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn write_error_leaves_original_and_no_temp() {
    let d = dir("write-err");
    let f = d.join("a.csv");
    std::fs::write(&f, "old\n").unwrap();
    let r = save_atomic(
        &f,
        &AtomicBool::new(false),
        |w| {
            w.write_all(b"partial")?;
            Err(std::io::Error::other("disk full"))
        },
        |_| Ok(()),
    );
    assert!(matches!(r, Err(SaveError::Io(_))));
    assert_eq!(std::fs::read(&f).unwrap(), b"old\n");
    assert_eq!(entries(&d), ["a.csv"]);
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn failed_verification_leaves_original_and_no_temp() {
    let d = dir("verify-err");
    let f = d.join("a.csv");
    std::fs::write(&f, "old\n").unwrap();
    let r = save_atomic(
        &f,
        &AtomicBool::new(false),
        |w| w.write_all(b"new\n"),
        |_| Err("row count 1, expected 2".into()),
    );
    assert!(matches!(&r, Err(SaveError::Verify(m)) if m.contains("row count")));
    assert_eq!(std::fs::read(&f).unwrap(), b"old\n");
    assert_eq!(entries(&d), ["a.csv"]);
    std::fs::remove_dir_all(d).unwrap();
}
