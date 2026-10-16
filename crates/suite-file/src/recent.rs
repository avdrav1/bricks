//! Recent files: the last [`MAX`] files opened or saved, newest first, kept in a file in
//! the app's recent-files folder (`suite_platform::dirs::recent_dir`: Brick's
//! `~/.local/state/brick/recent`, Etch's `~/.local/state/etch/recent`; on Windows
//! `%APPDATA%\Cronkite\<App>\recent`), one absolute path per line ([`crate::path_bytes`]), so
//! the list survives restarts. Paths are [`crate::canonical`]: on Windows `C:\…`, never
//! `\\?\C:\…`. Every change reads the file again first: each launch is its own process,
//! and two windows must not drop each other's files. Writes go to a temporary file,
//! synced, renamed over the old one. Brick's (APP-4) and Etch's (ETCH-22).

use std::io::Write;
use std::path::{Path, PathBuf};

pub const MAX: usize = 10;

pub struct Recent {
    store: PathBuf,
    files: Vec<PathBuf>,
}

impl Recent {
    /// The list kept in `store`; empty if it can't be read.
    pub fn load_from(store: PathBuf) -> Self {
        let files = std::fs::read(&store)
            .map(|bytes| {
                bytes
                    .split(|&b| b == b'\n')
                    .filter_map(crate::path_from_bytes)
                    .filter(|p| p.is_absolute())
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

    /// `file` (made [`crate::canonical`]) goes to the top, once; the oldest past [`MAX`]
    /// go.
    pub fn add(&mut self, file: &Path) -> std::io::Result<()> {
        let file = crate::canonical(file);
        let kept = crate::path_bytes(&file).is_some_and(|b| !b.contains(&b'\n'));
        if !file.is_absolute() || !kept {
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
            // Every listed path was kept by `add`, or read back from bytes.
            if let Some(bytes) = crate::path_bytes(f) {
                out.write_all(bytes)?;
                out.write_all(b"\n")?;
            }
        }
        out.sync_all()?;
        std::fs::rename(&tmp, &self.store)
    }
}

/// How the menu shows `file`: its name, then its folder, on Unix with the home folder as
/// `~` (`~/data`), on Windows in full (`C:\Users\me\Documents`), as Windows' own recent
/// lists show it: `~` means nothing there, and `$HOME`, when a Unix shell sets one, isn't
/// the user's profile folder.
pub fn label(file: &Path) -> (String, String) {
    let home = if cfg!(windows) {
        None
    } else {
        std::env::var_os("HOME").map(PathBuf::from)
    };
    label_from(file, home.as_deref())
}

/// [`label`] with the home folder `home` shortened to `~`.
fn label_from(file: &Path, home: Option<&Path>) -> (String, String) {
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = file.parent().unwrap_or(Path::new("/"));
    let dir = match home.and_then(|h| dir.strip_prefix(h).ok()) {
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

    /// A fresh folder for one test, by its canonical path: the list stores canonical paths,
    /// and the temp dir may sit behind a symlink (macOS: `/var` → `/private/var`) or a
    /// short name (Windows: `C:\Users\RUNNER~1`).
    fn dir(name: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("app4-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Dir(crate::canonical(&d))
    }

    /// APP-4 acceptance: the last 10 files survive a restart, newest first.
    #[test]
    fn the_last_ten_files_survive_a_restart() {
        let d = dir("ten");
        let store = d.0.join("state/brick/recent");
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

    /// ETCH-22: images opened and saved, interleaved, keep one entry each, newest first,
    /// ten at most; a file deleted since stays listed until opening it fails.
    #[test]
    fn opened_and_saved_images_list_once_newest_first_ten_at_most() {
        let d = dir("etch");
        let store = d.0.join("state/etch/recent");
        let image = |i: usize| d.0.join(format!("image {i}.png"));
        for i in 0..14 {
            std::fs::write(image(i), "").unwrap();
        }
        let mut recent = Recent::load_from(store.clone());
        // Opened 0 to 11, then 3 saved, then 0 opened again (it fell off the end).
        for i in (0..12).chain([3, 0]) {
            recent.add(&image(i)).unwrap();
        }
        let want: Vec<PathBuf> = [0, 3, 11, 10, 9, 8, 7, 6, 5, 4]
            .into_iter()
            .map(image)
            .collect();
        assert_eq!(Recent::load_from(store.clone()).files(), want);
        // Deleted: still listed (the menu shows it) until it's taken off.
        std::fs::remove_file(image(3)).unwrap();
        let mut again = Recent::load_from(store.clone());
        assert_eq!(again.files()[1], image(3));
        again.remove(&image(3)).unwrap();
        let files = Recent::load_from(store).files().to_vec();
        assert_eq!(files.len(), MAX - 1);
        assert!(!files.contains(&image(3)));
        assert_eq!(files[..2], [image(0), image(11)]);
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
        assert_eq!(files[0], crate::canonical(&odd));
    }

    /// WIN-1: the list keeps each file by its canonical name, which on Windows has no
    /// `\\?\` prefix, in the file too.
    #[test]
    fn files_are_listed_by_their_canonical_names() {
        let d = dir("names");
        let store = d.0.join("recent");
        std::fs::create_dir_all(d.0.join("sub")).unwrap();
        let file = d.0.join("a.csv");
        std::fs::write(&file, "").unwrap();
        let mut recent = Recent::load_from(store.clone());
        recent
            .add(&d.0.join("sub").join("..").join("a.csv"))
            .unwrap();
        recent.add(&file).unwrap();
        assert_eq!(
            Recent::load_from(store.clone()).files(),
            std::slice::from_ref(&file)
        );
        let kept = std::fs::read(&store).unwrap();
        assert_eq!(kept, [crate::path_bytes(&file).unwrap(), b"\n"].concat());
        assert!(
            !kept.starts_with(br"\\?\"),
            "{}",
            String::from_utf8_lossy(&kept)
        );
    }

    #[cfg(unix)]
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

    /// WIN-2: Windows shows the whole folder, whatever `HOME` a Unix shell set.
    #[cfg(windows)]
    #[test]
    fn labels_show_the_whole_folder_on_windows() {
        let profile = std::env::home_dir().unwrap();
        let documents = profile.join("Documents");
        assert_eq!(
            label(&documents.join("x.csv")),
            ("x.csv".into(), documents.display().to_string())
        );
        assert_eq!(
            label(Path::new(r"C:\Users\me\Documents\x.csv")),
            ("x.csv".into(), r"C:\Users\me\Documents".into())
        );
        assert_eq!(
            label(Path::new(r"D:\y.csv")),
            ("y.csv".into(), r"D:\".into())
        );
        assert_eq!(
            label(Path::new(r"\\server\share\z.csv")),
            ("z.csv".into(), r"\\server\share\".into())
        );
    }

    #[test]
    fn the_home_folder_is_shortened_when_given() {
        let home = std::env::temp_dir().join("me");
        let label = |p: PathBuf| label_from(&p, Some(&home));
        assert_eq!(label(home.join("a.csv")), ("a.csv".into(), "~".into()));
        assert_eq!(
            label(home.join("data").join("b.csv")),
            ("b.csv".into(), "~/data".into())
        );
        assert_eq!(
            label_from(&home.join("c.csv"), None).1,
            home.display().to_string()
        );
    }
}
