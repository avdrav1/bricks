//! Crash recovery journals (Brick's APP-7, Slate's SLATE-17): one directory per app. Each
//! open window with unsaved edits keeps one journal, `<pid>-<n>.journal`, written
//! atomically (temporary file, fsync, rename). Each process holds its owner lock
//! ([`crate::owner`]) in the directory while it runs, so a journal whose owner's lock can
//! be taken belongs to a process that ended without closing its windows: that is a crash
//! (or a kill), and its edits are offered back. Another running instance's journals are
//! left alone. What a journal holds is the app's business.
//!
//! A process's id can be one an earlier, crashed process had (Windows hands ids out again
//! soon, WIN-2): a journal named for this process that it didn't name itself is that
//! earlier process's, offered back like any other, and a new journal never takes an
//! existing one's name.

use crate::owner;
use parking_lot::Mutex;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// An app's recovery directory.
pub struct Store {
    dir: PathBuf,
}

static NEXT: AtomicU64 = AtomicU64::new(0);

/// The journals this process named and hasn't removed.
static ISSUED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

impl Store {
    pub fn at(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// A journal name for a new window of this process, one no file has.
    pub fn new_journal(&self) -> PathBuf {
        let pid = std::process::id();
        loop {
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let journal = self.dir.join(format!("{pid}-{n}.journal"));
            if !journal.exists() && !journal.with_extension("tmp").exists() {
                ISSUED.lock().push(journal.clone());
                return journal;
            }
        }
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
        let me = std::process::id();
        for e in entries.flatten() {
            let path = e.path();
            if path.extension().is_none_or(|x| x != "journal") {
                continue;
            }
            let Some(pid) = journal_pid(&path) else {
                continue;
            };
            let alive = if pid == me {
                ISSUED.lock().contains(&path)
            } else {
                *live
                    .entry(pid)
                    .or_insert_with(|| owner::alive(&self.dir, pid))
            };
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
        ISSUED.lock().retain(|j| j != journal);
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

    /// WIN-2: an earlier process with this process's id left journals: they're offered,
    /// and this process's new journals take other names.
    #[test]
    fn an_earlier_process_with_this_id_left_its_journals() {
        let d = dir("reused");
        let store = Store::at(d.0.clone());
        let me = std::process::id();
        let next = NEXT.load(Ordering::Relaxed);
        let earlier: Vec<PathBuf> = (next..next + 3)
            .map(|n| d.0.join(format!("{me}-{n}.journal")))
            .collect();
        for j in &earlier {
            write(&store, j, "earlier");
        }
        let mine = store.new_journal();
        assert!(!earlier.contains(&mine), "{}", mine.display());
        write(&store, &mine, "mine");
        let mut crashed = store.crashed();
        crashed.sort();
        assert_eq!(crashed, earlier, "the earlier process's, not this one's");
        store.remove(&earlier[0]);
        assert!(
            owner::lock_path(&d.0, me).exists(),
            "this process's own lock stays"
        );
        assert_eq!(std::fs::read_to_string(&mine).unwrap(), "mine");
    }

    /// Run in a child process by [`a_killed_processs_journal_is_offered`]: writes a
    /// journal in `$SUITE_RECOVERY_CHILD` and runs until it's killed (or its stdin closes).
    #[test]
    fn write_a_journal_for_parent() {
        let Some(dir) = std::env::var_os("SUITE_RECOVERY_CHILD") else {
            return;
        };
        let store = Store::at(PathBuf::from(dir));
        let journal = store.new_journal();
        write(&store, &journal, "unsaved");
        println!("journal\t{}", journal.display());
        let _ = std::io::stdin().read_line(&mut String::new());
    }

    /// WIN-2: another process's journal is left alone while it runs and offered once it's
    /// killed, its lock file going with its journal; on every system, Windows' file locks
    /// included.
    #[test]
    fn a_killed_processs_journal_is_offered() {
        use std::io::BufRead;
        use std::process::{Command, Stdio};
        let d = dir("killed");
        let store = Store::at(d.0.clone());
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "recovery::tests::write_a_journal_for_parent",
                "--nocapture",
            ])
            .env("SUITE_RECOVERY_CHILD", &d.0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let journal = std::io::BufReader::new(child.stdout.take().unwrap())
            .lines()
            .find_map(|l| l.ok()?.strip_prefix("journal\t").map(PathBuf::from))
            .expect("the child's journal");
        assert_eq!(journal.parent(), Some(d.0.as_path()));
        assert!(
            store.crashed().is_empty(),
            "a running process's journal stays"
        );

        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(
            after_exit(|| store.crashed()),
            std::slice::from_ref(&journal)
        );
        assert_eq!(std::fs::read_to_string(&journal).unwrap(), "unsaved");
        store.remove(&journal);
        assert!(!journal.exists());
        assert!(!owner::lock_path(&d.0, child.id()).exists());
    }

    /// What `look` finds once it finds anything, asking for up to 10 s: Windows drops a
    /// killed process's file locks soon after it ends, not always at once.
    fn after_exit<T>(look: impl Fn() -> Vec<T>) -> Vec<T> {
        let start = std::time::Instant::now();
        loop {
            let found = look();
            if !found.is_empty() || start.elapsed() > std::time::Duration::from_secs(10) {
                return found;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}
