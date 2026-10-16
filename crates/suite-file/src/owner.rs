//! Owner locks: which processes that keep files in a folder are still running, the same
//! way on Linux, macOS (MAC-2), and Windows (WIN-2). A process holds `owner-<pid>.lock` in
//! the folder locked (`File::lock`: `flock` on Unix, `LockFileEx` on Windows) from its first
//! file there until it exits. The system drops the lock however the process ends, crash or
//! kill included (Windows soon after, not always at once), so files named for a process
//! whose lock can be taken belong to one that's gone. Unlike looking the pid up
//! (`/proc/<pid>`, which macOS and Windows lack, or `kill(pid, 0)`), a gone process's pid
//! used again by another program doesn't keep its files. Crash recovery journals
//! ([`crate::recovery`]) and Slate's package copies use it.

use parking_lot::Mutex;
use std::fs::File;
use std::path::{Path, PathBuf};

/// The folders this process holds its lock in, each with its locked file; held until it
/// exits.
static HELD: Mutex<Vec<(PathBuf, File)>> = Mutex::new(Vec::new());

/// Process `pid`'s lock file in `dir`.
pub fn lock_path(dir: &Path, pid: u32) -> PathBuf {
    dir.join(format!("owner-{pid}.lock"))
}

/// Take this process's lock in `dir` (which must exist), once; it's held until the process
/// exits. Take it before putting files there, so they're never without a live owner.
pub fn hold(dir: &Path) -> std::io::Result<()> {
    let mut held = HELD.lock();
    if held.iter().any(|(d, _)| d == dir) {
        return Ok(());
    }
    let f = File::create(lock_path(dir, std::process::id()))?;
    f.lock()?;
    held.push((dir.to_owned(), f));
    Ok(())
}

/// Whether process `pid`'s lock in `dir` is held: by this process, or another running one.
pub fn alive(dir: &Path, pid: u32) -> bool {
    if pid == std::process::id() {
        return true;
    }
    match File::open(lock_path(dir, pid)) {
        Ok(f) => matches!(f.try_lock(), Err(std::fs::TryLockError::WouldBlock)),
        Err(_) => false,
    }
}

/// Remove gone process `pid`'s lock file from `dir`, once its files there are gone too.
pub fn release(dir: &Path, pid: u32) {
    if !alive(dir, pid) {
        let _ = std::fs::remove_file(lock_path(dir, pid));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lock_is_alive_while_held() {
        let dir = std::env::temp_dir().join(format!("suite-owner-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        hold(&dir).unwrap();
        hold(&dir).unwrap();
        assert!(alive(&dir, std::process::id()));
        assert!(lock_path(&dir, std::process::id()).exists());
        // Another process's lock: held here through a file of its own.
        let other = File::create(lock_path(&dir, 777_777)).unwrap();
        other.lock().unwrap();
        assert!(alive(&dir, 777_777));
        release(&dir, 777_777);
        assert!(lock_path(&dir, 777_777).exists(), "a live lock stays");
        drop(other);
        assert!(!alive(&dir, 777_777), "gone once its holder is");
        release(&dir, 777_777);
        assert!(!lock_path(&dir, 777_777).exists());
        assert!(!alive(&dir, 666_666), "no lock file: gone");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
