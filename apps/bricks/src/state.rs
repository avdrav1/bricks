//! The app's state folder: `$XDG_STATE_HOME/bricks` (default `~/.local/state/bricks`),
//! holding the recent-files list (APP-4) and crash recovery journals (APP-7).
//!
//! Releases before the rename to Bricks (PKG-4) kept it in `…/spreadsheet`: the first use
//! moves that folder over, so recent files and unrestored edits carry across the upgrade.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

const NAME: &str = "bricks";
const OLD_NAME: &str = "spreadsheet";

static DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state")
        });
    resolve(&base)
});

/// The state folder (it may not exist yet).
pub fn dir() -> &'static Path {
    &DIR
}

/// `base/bricks`, after moving `base/spreadsheet` there if only the old one exists.
fn resolve(base: &Path) -> PathBuf {
    let dir = base.join(NAME);
    let old = base.join(OLD_NAME);
    if !dir.exists() && old.is_dir() {
        if let Err(e) = std::fs::rename(&old, &dir) {
            eprintln!(
                "bricks: cannot move {} to {}: {e}",
                old.display(),
                dir.display()
            );
        }
    }
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pkg4-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_old_folder_moves_over_once() {
        let b = base("move");
        std::fs::create_dir_all(b.join("spreadsheet/recovery")).unwrap();
        std::fs::write(b.join("spreadsheet/recent"), "/data/a.csv\n").unwrap();
        let dir = resolve(&b);
        assert_eq!(dir, b.join("bricks"));
        assert_eq!(
            std::fs::read_to_string(dir.join("recent")).unwrap(),
            "/data/a.csv\n"
        );
        assert!(dir.join("recovery").is_dir());
        assert!(!b.join("spreadsheet").exists());
        std::fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn an_existing_bricks_folder_wins() {
        let b = base("both");
        std::fs::create_dir_all(b.join("spreadsheet")).unwrap();
        std::fs::write(b.join("spreadsheet/recent"), "/old.csv\n").unwrap();
        std::fs::create_dir_all(b.join("bricks")).unwrap();
        std::fs::write(b.join("bricks/recent"), "/new.csv\n").unwrap();
        let dir = resolve(&b);
        assert_eq!(
            std::fs::read_to_string(dir.join("recent")).unwrap(),
            "/new.csv\n"
        );
        assert!(
            b.join("spreadsheet/recent").exists(),
            "the old one is left alone"
        );
        std::fs::remove_dir_all(&b).unwrap();
    }
}
