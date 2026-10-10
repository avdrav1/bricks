//! Where crash recovery journals live (APP-7): `$XDG_STATE_HOME/spreadsheet/recovery`
//! (default `~/.local/state/spreadsheet/recovery`). Each open window with unsaved edits
//! keeps one journal, `<pid>-<n>.journal`, written atomically (temporary file, fsync,
//! rename). Each process holds `owner-<pid>.lock` locked while it runs, so a journal whose
//! owner's lock can be taken belongs to a process that ended without closing its windows:
//! that is a crash (or a kill), and its edits are offered back. Another running instance's
//! journals are left alone.

use data_model::{read_journal_header, JournalHeader};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::LazyLock;

/// A journal left by a process that is gone.
#[derive(Debug, Clone)]
pub struct Crashed {
    pub journal: PathBuf,
    pub header: JournalHeader,
}

pub struct Store {
    dir: PathBuf,
}

/// This process's lock, taken with its first journal and held until it exits.
static OWNER: LazyLock<std::sync::Mutex<Option<File>>> =
    LazyLock::new(|| std::sync::Mutex::new(None));
static NEXT: AtomicU64 = AtomicU64::new(0);

impl Store {
    /// The user's recovery directory.
    pub fn open() -> Self {
        let state = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state")
            });
        Self::at(state.join("spreadsheet/recovery"))
    }

    pub fn at(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// A journal name for a new window of this process.
    pub fn new_journal(&self) -> PathBuf {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        self.dir.join(format!("{}-{n}.journal", std::process::id()))
    }

    fn lock_path(&self, pid: u32) -> PathBuf {
        self.dir.join(format!("owner-{pid}.lock"))
    }

    /// Take this process's owner lock, once.
    fn hold_owner_lock(&self) -> std::io::Result<()> {
        let mut owner = OWNER.lock().unwrap_or_else(|e| e.into_inner());
        if owner.is_none() {
            let f = File::create(self.lock_path(std::process::id()))?;
            f.lock()?;
            *owner = Some(f);
        }
        Ok(())
    }

    /// Write `journal` from `fill`: to a temporary file, synced, then renamed over it, so
    /// a crash mid-write leaves the previous journal whole.
    pub fn write(
        &self,
        journal: &Path,
        fill: impl FnOnce(&mut File) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        self.hold_owner_lock()?;
        let tmp = journal.with_extension("tmp");
        let mut f = File::create(&tmp)?;
        let written = fill(&mut f)
            .and_then(|_| f.flush())
            .and_then(|_| f.sync_all());
        if let Err(e) = written {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
        std::fs::rename(&tmp, journal)
    }

    /// Journals whose process is gone, newest first. Lock files of finished processes
    /// with nothing left are cleaned up on the way.
    pub fn crashed(&self) -> Vec<Crashed> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut found = Vec::new();
        let mut live = std::collections::HashMap::new();
        for e in entries.flatten() {
            let path = e.path();
            if path.extension().is_none_or(|x| x != "journal") {
                continue;
            }
            let Some(header) = File::open(&path)
                .ok()
                .and_then(|f| read_journal_header(f).ok())
            else {
                continue; // being written, or not ours
            };
            let alive = *live
                .entry(header.pid)
                .or_insert_with(|| self.owner_alive(header.pid));
            if !alive {
                found.push(Crashed {
                    journal: path,
                    header,
                });
            }
        }
        found.sort_by_key(|c| std::cmp::Reverse(c.header.saved_ms));
        found
    }

    /// Whether process `pid`'s owner lock is held: by this process, or another running one.
    fn owner_alive(&self, pid: u32) -> bool {
        if pid == std::process::id() {
            return true;
        }
        match File::open(self.lock_path(pid)) {
            Ok(f) => matches!(f.try_lock(), Err(std::fs::TryLockError::WouldBlock)),
            Err(_) => false,
        }
    }

    /// Delete a journal (saved, discarded, restored, or its window closed). Once a gone
    /// process has no journals left, its lock file goes too.
    pub fn remove(&self, journal: &Path) {
        let _ = std::fs::remove_file(journal);
        let pid = journal
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.split('-').next())
            .and_then(|p| p.parse::<u32>().ok());
        if let Some(pid) = pid.filter(|&p| p != std::process::id()) {
            let prefix = format!("{pid}-");
            let left = std::fs::read_dir(&self.dir)
                .into_iter()
                .flatten()
                .flatten()
                .any(|e| e.file_name().to_string_lossy().starts_with(&prefix));
            if !left && !self.owner_alive(pid) {
                let _ = std::fs::remove_file(self.lock_path(pid));
            }
        }
    }
}

/// The file's size and modification time, which a journal records so its edits are only
/// put back onto the same bytes.
pub fn identity(path: &Path) -> Option<(u64, u64)> {
    let m = std::fs::metadata(path).ok()?;
    let mtime = m
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    Some((m.len(), mtime.as_nanos() as u64))
}

/// Milliseconds since the Unix epoch, for a journal's time.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use csv_engine::Encoding;

    struct Dir(PathBuf);

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn dir(name: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("app7-store-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        Dir(d)
    }

    fn header(pid: u32, saved_ms: u64) -> JournalHeader {
        JournalHeader {
            path: Some("/tmp/x.csv".into()),
            file_size: 1,
            file_mtime_ns: 2,
            delimiter: b',',
            has_header: true,
            encoding: Encoding::UTF8,
            saved_ms,
            changes: 3,
            rows: 4,
            pid,
        }
    }

    fn write(store: &Store, journal: &Path, h: &JournalHeader) {
        store
            .write(journal, |f| {
                data_model::CsvTable::untitled().write_journal(h, f)
            })
            .unwrap();
    }

    /// A journal whose writer is gone is offered; this process's and a live one's aren't.
    #[test]
    fn only_journals_of_gone_processes_count_as_crashed() {
        let d = dir("owners");
        let store = Store::at(d.0.clone());
        let mine = store.new_journal();
        write(&store, &mine, &header(std::process::id(), 10));
        // A process that crashed: its journal, and its lock file nobody holds.
        let gone = d.0.join("999999-0.journal");
        write(&store, &gone, &header(999_999, 20));
        File::create(store.lock_path(999_999)).unwrap();
        // A live other instance: its lock file is held (here, by this test).
        let live = d.0.join("888888-0.journal");
        write(&store, &live, &header(888_888, 30));
        let held = File::create(store.lock_path(888_888)).unwrap();
        held.lock().unwrap();

        let crashed = store.crashed();
        let paths: Vec<_> = crashed.iter().map(|c| c.journal.clone()).collect();
        assert_eq!(
            paths,
            std::slice::from_ref(&gone),
            "only the gone process's journal"
        );
        assert_eq!(crashed[0].header.changes, 3);

        drop(held); // that instance ends without closing: now it crashed too
        let paths: Vec<_> = store.crashed().iter().map(|c| c.journal.clone()).collect();
        assert_eq!(paths, [live.clone(), gone.clone()], "newest first");

        store.remove(&gone);
        assert!(
            !store.lock_path(999_999).exists(),
            "its lock file goes with its last journal"
        );
        assert_eq!(store.crashed().len(), 1);
    }

    #[test]
    fn a_write_replaces_the_journal_whole() {
        let d = dir("atomic");
        let store = Store::at(d.0.clone());
        let j = store.new_journal();
        write(&store, &j, &header(std::process::id(), 1));
        let err = store.write(&j, |f| {
            f.write_all(b"half a journal")?;
            Err(std::io::Error::other("disk full"))
        });
        assert!(err.is_err());
        let h = read_journal_header(File::open(&j).unwrap()).unwrap();
        assert_eq!(h.saved_ms, 1, "the earlier journal is still whole");
        assert!(!j.with_extension("tmp").exists(), "no temporary file left");
    }
}
