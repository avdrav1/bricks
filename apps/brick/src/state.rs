//! The app's state folder ([`suite_platform::dirs::state_dir`]): `$XDG_STATE_HOME/brick`
//! (default `~/.local/state/brick`) on Linux, `~/Library/Application
//! Support/dev.cronkite.Brick` on macOS, holding the recent-files list (APP-4) and crash
//! recovery journals (APP-7).
//!
//! Earlier releases, all Linux-only, kept it under their own names: `…/bricks` (v0.1.1 to
//! v0.2.1, PKG-4) and `…/spreadsheet` (v0.1.0). On Linux the first use moves the newest of
//! those over, so recent files and unrestored edits carry across the upgrade (PKG-5).

use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use suite_core::AppKind;

/// The folders of earlier releases, newest first.
const OLD_NAMES: [&str; 2] = ["bricks", "spreadsheet"];

static DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    let dir = suite_platform::dirs::state_dir(AppKind::Brick);
    if cfg!(target_os = "linux") {
        adopt_old(&dir);
    }
    dir
});

/// The state folder (it may not exist yet).
pub fn dir() -> &'static Path {
    &DIR
}

/// The recent files (APP-4), kept in `recent` in the state folder.
pub fn recent() -> suite_file::recent::Recent {
    suite_file::recent::Recent::load_from(dir().join("recent"))
}

/// Unless `dir` exists, move the newest earlier release's folder beside it there.
fn adopt_old(dir: &Path) {
    let Some(base) = dir.parent().filter(|_| !dir.exists()) else {
        return;
    };
    if let Some(old) = OLD_NAMES.iter().map(|n| base.join(n)).find(|p| p.is_dir()) {
        if let Err(e) = std::fs::rename(&old, dir) {
            eprintln!(
                "brick: cannot move {} to {}: {e}",
                old.display(),
                dir.display()
            );
        }
    }
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

    /// `base/brick`, as the state folder in `base` (Linux's) is found.
    fn resolve(base: &Path) -> PathBuf {
        let dir = base.join("brick");
        adopt_old(&dir);
        dir
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
