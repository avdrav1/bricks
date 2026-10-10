//! Recent files (APP-4): the last [`MAX`] files opened or saved, newest first. Kept in
//! `$XDG_STATE_HOME/spreadsheet/recent` (default `~/.local/state/spreadsheet/recent`),
//! one absolute path per line, so the list survives restarts. Every change reads the file
//! again first: each launch is its own process, and two windows must not drop each other's
//! files. Writes go to a temporary file renamed over the old one.

use std::ffi::OsStr;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

pub const MAX: usize = 10;

pub struct Recent {
    store: PathBuf,
    files: Vec<PathBuf>,
}

impl Recent {
    /// The list in the user's state directory; empty if there is none yet.
    pub fn load() -> Self {
        Self::load_from(default_store())
    }

    /// The list kept in `store`; empty if it can't be read.
    pub fn load_from(store: PathBuf) -> Self {
        let files = std::fs::read(&store)
            .map(|bytes| {
                bytes
                    .split(|&b| b == b'\n')
                    .filter(|l| l.first() == Some(&b'/'))
                    .map(|l| PathBuf::from(OsStr::from_bytes(l)))
                    .take(MAX)
                    .collect()
            })
            .unwrap_or_default();
        Self { store, files }
    }

    /// Newest first.
    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }

    /// `file` (made absolute) goes to the top, once; the oldest past [`MAX`] go.
    pub fn add(&mut self, file: &Path) -> std::io::Result<()> {
        let file = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_owned());
        if !file.is_absolute() || file.as_os_str().as_bytes().contains(&b'\n') {
            return Ok(()); // can't be kept one per line
        }
        self.reload();
        self.files.retain(|f| *f != file);
        self.files.insert(0, file);
        self.files.truncate(MAX);
        self.save()
    }

    /// Take `file` off the list (it can't be opened any more).
    pub fn remove(&mut self, file: &Path) -> std::io::Result<()> {
        self.reload();
        self.files.retain(|f| f != file);
        self.save()
    }

    pub fn clear(&mut self) -> std::io::Result<()> {
        self.files.clear();
        self.save()
    }

    fn reload(&mut self) {
        self.files = Self::load_from(self.store.clone()).files;
    }

    fn save(&self) -> std::io::Result<()> {
        let dir = self.store.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir)?;
        let tmp = self
            .store
            .with_extension(format!("tmp-{}", std::process::id()));
        let mut out = std::fs::File::create(&tmp)?;
        for f in &self.files {
            out.write_all(f.as_os_str().as_bytes())?;
            out.write_all(b"\n")?;
        }
        out.sync_all()?;
        std::fs::rename(&tmp, &self.store)
    }
}

fn default_store() -> PathBuf {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").unwrap_or_default();
            PathBuf::from(home).join(".local/state")
        });
    state.join("spreadsheet/recent")
}

/// How the menu shows `file`: its name, then its folder with the home folder as `~`.
pub fn label(file: &Path) -> (String, String) {
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = file.parent().unwrap_or(Path::new("/"));
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let dir = match home.as_deref().and_then(|h| dir.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Some(rest) => format!("~/{}", rest.display()),
        None => dir.display().to_string(),
    };
    (name, dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dir(PathBuf);

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn dir(name: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("app4-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }

    /// APP-4 acceptance: the last 10 files survive a restart, newest first.
    #[test]
    fn the_last_ten_files_survive_a_restart() {
        let d = dir("ten");
        let store = d.0.join("state/spreadsheet/recent");
        let file = |i: usize| d.0.join(format!("f{i}.csv"));
        let mut recent = Recent::load_from(store.clone());
        assert!(recent.files().is_empty(), "nothing yet, and no file needed");
        for i in 0..12 {
            std::fs::write(file(i), "a\n").unwrap();
            recent.add(&file(i)).unwrap();
        }
        // A new process reads what the old one wrote.
        let after = Recent::load_from(store.clone());
        let want: Vec<PathBuf> = (2..12).rev().map(file).collect();
        assert_eq!(after.files(), want);

        // Opening one again moves it to the top, once.
        let mut recent = after;
        recent.add(&file(5)).unwrap();
        let files = Recent::load_from(store).files().to_vec();
        assert_eq!(files[0], file(5));
        assert_eq!(files.len(), MAX);
        assert_eq!(files.iter().filter(|f| **f == file(5)).count(), 1);
    }

    #[test]
    fn two_windows_keep_each_others_files() {
        let d = dir("two");
        let store = d.0.join("recent");
        let (a, b) = (d.0.join("a.csv"), d.0.join("b.csv"));
        std::fs::write(&a, "").unwrap();
        std::fs::write(&b, "").unwrap();
        let mut first = Recent::load_from(store.clone());
        let mut second = Recent::load_from(store.clone());
        first.add(&a).unwrap();
        second.add(&b).unwrap(); // loaded before `a` was added
        assert_eq!(
            Recent::load_from(store.clone()).files(),
            [b.clone(), a.clone()]
        );
        first.remove(&a).unwrap();
        assert_eq!(Recent::load_from(store.clone()).files(), [b]);
        first.clear().unwrap();
        assert!(Recent::load_from(store).files().is_empty());
    }

    #[test]
    fn relative_paths_are_made_absolute_and_odd_names_kept_raw() {
        let d = dir("odd");
        let store = d.0.join("recent");
        let odd = d.0.join("spa ce, ünï.csv");
        std::fs::write(&odd, "").unwrap();
        let mut recent = Recent::load_from(store.clone());
        recent.add(&odd).unwrap();
        recent.add(Path::new("relative-and-missing.csv")).unwrap();
        let files = Recent::load_from(store).files().to_vec();
        assert_eq!(
            files.len(),
            1,
            "a path that can't be made absolute is skipped"
        );
        assert_eq!(files[0], std::fs::canonicalize(&odd).unwrap());
    }

    #[test]
    fn labels_show_the_folder_from_home() {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        assert_eq!(
            label(&home.join("data/x.csv")),
            ("x.csv".into(), "~/data".into())
        );
        assert_eq!(label(&home.join("y.csv")), ("y.csv".into(), "~".into()));
        assert_eq!(
            label(Path::new("/srv/z.csv")),
            ("z.csv".into(), "/srv".into())
        );
    }
}
