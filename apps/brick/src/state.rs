//! The app's state folder: `$XDG_STATE_HOME/brick` (default `~/.local/state/brick`),
//! holding the recent-files list (APP-4) and crash recovery journals (APP-7).
//!
//! Earlier releases kept it under their own names: `…/bricks` (v0.1.1 to v0.2.1, PKG-4)
//! and `…/spreadsheet` (v0.1.0). The first use moves the newest of those over, so recent
//! files and unrestored edits carry across the upgrade (PKG-5).

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

const NAME: &str = "brick";
/// The folders of earlier releases, newest first.
const OLD_NAMES: [&str; 2] = ["bricks", "spreadsheet"];

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

/// The recent files (APP-4), kept in `recent` in the state folder.
pub fn recent() -> suite_file::recent::Recent {
    suite_file::recent::Recent::load_from(dir().join("recent"))
}

/// `base/brick`, after moving the newest earlier release's folder there if it doesn't
/// exist yet.
fn resolve(base: &Path) -> PathBuf {
    let dir = base.join(NAME);
    if dir.exists() {
        return dir;
    }
    if let Some(old) = OLD_NAMES.iter().map(|n| base.join(n)).find(|p| p.is_dir()) {
        if let Err(e) = std::fs::rename(&old, &dir) {
            eprintln!(
                "brick: cannot move {} to {}: {e}",
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
        let d = std::env::temp_dir().join(format!("pkg5-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn an_old_folder_moves_over_once() {
        for old in OLD_NAMES {
            let b = base(old);
            std::fs::create_dir_all(b.join(old).join("recovery")).unwrap();
            std::fs::write(b.join(old).join("recent"), "/data/a.csv\n").unwrap();
            let dir = resolve(&b);
            assert_eq!(dir, b.join("brick"));
            assert_eq!(
                std::fs::read_to_string(dir.join("recent")).unwrap(),
                "/data/a.csv\n"
            );
            assert!(dir.join("recovery").is_dir());
            assert!(!b.join(old).exists());
            std::fs::remove_dir_all(&b).unwrap();
        }
    }

    #[test]
    fn the_newest_old_folder_wins() {
        let b = base("two-old");
        std::fs::create_dir_all(b.join("spreadsheet")).unwrap();
        std::fs::write(b.join("spreadsheet/recent"), "/oldest.csv\n").unwrap();
        std::fs::create_dir_all(b.join("bricks")).unwrap();
        std::fs::write(b.join("bricks/recent"), "/newer.csv\n").unwrap();
        let dir = resolve(&b);
        assert_eq!(
            std::fs::read_to_string(dir.join("recent")).unwrap(),
            "/newer.csv\n"
        );
        assert!(b.join("spreadsheet/recent").exists(), "left alone");
        std::fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn an_existing_brick_folder_wins() {
        let b = base("both");
        std::fs::create_dir_all(b.join("bricks")).unwrap();
        std::fs::write(b.join("bricks/recent"), "/old.csv\n").unwrap();
        std::fs::create_dir_all(b.join("brick")).unwrap();
        std::fs::write(b.join("brick/recent"), "/new.csv\n").unwrap();
        let dir = resolve(&b);
        assert_eq!(
            std::fs::read_to_string(dir.join("recent")).unwrap(),
            "/new.csv\n"
        );
        assert!(
            b.join("bricks/recent").exists(),
            "the old one is left alone"
        );
        std::fs::remove_dir_all(&b).unwrap();
    }
}
