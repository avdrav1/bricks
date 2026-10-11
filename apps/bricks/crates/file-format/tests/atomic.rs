//! `save_atomic` failure paths: whatever goes wrong before the rename, the original stays as
//! it was and no temp file is left behind.

use file_format::{save_atomic, SaveError};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

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
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o640)).unwrap();
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
    assert_eq!(
        std::fs::metadata(&f).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert_eq!(entries(&d), ["a.csv"]);
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
