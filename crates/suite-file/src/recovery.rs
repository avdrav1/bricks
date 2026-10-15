//! Crash recovery journals (Brick's APP-7, Slate's SLATE-17): one directory per app. Each
//! open window with unsaved edits keeps one journal, `<pid>-<n>.journal`, written
//! atomically (temporary file, fsync, rename). Each process holds its owner lock
//! ([`crate::owner`]) in the directory while it runs, so a journal whose owner's lock can
//! be taken belongs to a process that ended without closing its windows: that is a crash
//! (or a kill), and its edits are offered back. Another running instance's journals are
//! left alone. What a journal holds is the app's business.

use crate::owner;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// An app's recovery directory.
pub struct Store {
    dir: PathBuf,
}

static NEXT: AtomicU64 = AtomicU64::new(0);

impl Store {
    pub fn at(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// A journal name for a new window of this process.
    pub fn new_journal(&self) -> PathBuf {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        self.dir.join(format!("{}-{n}.journal", std::process::id()))
    }

    /// Write `journal` from `fill`: to a temporary file, synced, then renamed over it, so
    /// a crash mid-write leaves the previous journal whole.
    pub fn write(
        &self,
        journal: &Path,
        fill: impl FnOnce(&mut File) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        owner::hold(&self.dir)?;
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

    /// The journals of processes that are gone, newest first (by when they were written).
    pub fn crashed(&self) -> Vec<PathBuf> {
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
            let Some(pid) = journal_pid(&path) else {
                continue;
            };
            let alive = *live
                .entry(pid)
                .or_insert_with(|| owner::alive(&self.dir, pid));
            if !alive {
                let written = e.metadata().and_then(|m| m.modified()).ok();
                found.push((written, path));
            }
        }
        found.sort_by(|a, b| b.cmp(a));
        found.into_iter().map(|(_, p)| p).collect()
    }

    /// Delete a journal (saved, discarded, restored, or its window closed). Once a gone
    /// process has no journals left, its lock file goes too.
    pub fn remove(&self, journal: &Path) {
        let _ = std::fs::remove_file(journal);
        if let Some(pid) = journal_pid(journal).filter(|&p| p != std::process::id()) {
            let prefix = format!("{pid}-");
            let left = std::fs::read_dir(&self.dir)
                .into_iter()
                .flatten()
                .flatten()
                .any(|e| e.file_name().to_string_lossy().starts_with(&prefix));
            if !left {
                owner::release(&self.dir, pid);
            }
        }
    }
}

/// The process that wrote `journal`, from its name.
fn journal_pid(journal: &Path) -> Option<u32> {
    journal
        .file_stem()?
        .to_str()?
        .split('-')
        .next()?
        .parse()
        .ok()
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

    struct Dir(PathBuf);

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn dir(name: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("suite-recovery-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        Dir(d)
    }

    fn write(store: &Store, journal: &Path, text: &str) {
        store
            .write(journal, |f| f.write_all(text.as_bytes()))
            .unwrap();
    }

    /// A journal whose writer is gone is offered; this process's and a live one's aren't.
    #[test]
    fn only_journals_of_gone_processes_count_as_crashed() {
        let d = dir("owners");
        let store = Store::at(d.0.clone());
        let mine = store.new_journal();
        write(&store, &mine, "mine");
        // A process that crashed: its journal, and its lock file nobody holds.
        let gone = d.0.join("999999-0.journal");
        write(&store, &gone, "gone");
        File::create(owner::lock_path(&d.0, 999_999)).unwrap();
        // A live other instance: its lock file is held (here, by this test).
        std::thread::sleep(std::time::Duration::from_millis(20));
        let live = d.0.join("888888-0.journal");
        write(&store, &live, "live");
        let held = File::create(owner::lock_path(&d.0, 888_888)).unwrap();
        held.lock().unwrap();

        assert_eq!(
            store.crashed(),
            std::slice::from_ref(&gone),
            "only the gone process's journal"
        );

        drop(held); // that instance ends without closing: now it crashed too
        assert_eq!(
            store.crashed(),
            [live.clone(), gone.clone()],
            "newest first"
        );

        store.remove(&gone);
        assert!(
            !owner::lock_path(&d.0, 999_999).exists(),
            "its lock file goes with its last journal"
        );
        assert_eq!(store.crashed(), [live]);
    }

    #[test]
    fn a_write_replaces_the_journal_whole() {
        let d = dir("atomic");
        let store = Store::at(d.0.clone());
        let j = store.new_journal();
        write(&store, &j, "first");
        let err = store.write(&j, |f| {
            f.write_all(b"half a journal")?;
            Err(std::io::Error::other("disk full"))
        });
        assert!(err.is_err());
        assert_eq!(
            std::fs::read_to_string(&j).unwrap(),
            "first",
            "the earlier journal is still whole"
        );
        assert!(!j.with_extension("tmp").exists(), "no temporary file left");
    }
}
