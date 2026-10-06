//! The source file, memory-mapped read-only (ADR 0002).
//!
//! A mapped file that another program truncates while we read it raises SIGBUS, which
//! would kill the app and every unsaved edit with it. [`Source`] registers its mapping with
//! a process-wide SIGBUS handler: a fault inside a registered mapping swaps a zero page in
//! at the faulting address and flags the source as changed, and the read continues. Callers
//! check [`Source::changed`] after reading and report "file changed on disk" instead of
//! crashing. Faults outside our mappings go to the previous handler (Rust's stack-overflow
//! reporter, or the default action).
//!
//! Our own saves replace the file by rename (SAVE-1), which leaves an open mapping valid.

use memmap2::Mmap;
use std::fs::File;
use std::io;
use std::path::Path;

pub struct Source {
    map: Option<Mmap>,
    slot: Option<usize>,
}

impl Source {
    /// Map `path` read-only. An empty file has no mapping and no bytes.
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        if file.metadata()?.len() == 0 {
            return Ok(Self {
                map: None,
                slot: None,
            });
        }
        // SAFETY: the mapping is read-only. Concurrent truncation by another process is
        // contained by the SIGBUS guard registered next; in-place rewrites can still change
        // bytes under us, which is inherent to mmap and documented in ADR 0002.
        #[allow(unsafe_code)]
        let map = unsafe { Mmap::map(&file)? };
        let slot = guard::register(map.as_ptr() as usize, map.len())
            .ok_or_else(|| io::Error::other("too many files open at once"))?;
        Ok(Self {
            map: Some(map),
            slot: Some(slot),
        })
    }

    /// The file's bytes. Pages lost to truncation read as zeros; see [`Self::changed`].
    pub fn bytes(&self) -> &[u8] {
        self.map.as_deref().unwrap_or(&[])
    }

    /// True once any read hit a page the file no longer backs (it shrank while mapped).
    pub fn changed(&self) -> bool {
        self.slot.is_some_and(guard::faulted)
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        if let Some(slot) = self.slot {
            guard::unregister(slot);
        }
    }
}

#[allow(unsafe_code)] // signal handling has no safe interface; kept to this module
mod guard {
    use std::cell::UnsafeCell;
    use std::mem::MaybeUninit;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Once;

    struct Slot {
        used: AtomicBool,
        start: AtomicUsize,
        len: AtomicUsize,
        faulted: AtomicBool,
    }

    #[allow(clippy::declare_interior_mutable_const)] // template for the array below
    const EMPTY: Slot = Slot {
        used: AtomicBool::new(false),
        start: AtomicUsize::new(0),
        len: AtomicUsize::new(0),
        faulted: AtomicBool::new(false),
    };
    static SLOTS: [Slot; 64] = [EMPTY; 64];
    static PAGE: AtomicUsize = AtomicUsize::new(4096);
    static INSTALL: Once = Once::new();

    /// The SIGBUS action that was installed before ours, written once before ours goes in.
    struct Prev(UnsafeCell<MaybeUninit<libc::sigaction>>);
    // SAFETY: written exactly once inside `INSTALL`, before the handler that reads it exists.
    unsafe impl Sync for Prev {}
    static PREV: Prev = Prev(UnsafeCell::new(MaybeUninit::uninit()));

    extern "C" fn on_sigbus(
        _sig: libc::c_int,
        info: *mut libc::siginfo_t,
        _ctx: *mut libc::c_void,
    ) {
        // Async-signal-safe: atomics and one mmap syscall only.
        // SAFETY: the kernel passes a valid siginfo for SA_SIGINFO handlers.
        let addr = unsafe { (*info).si_addr() } as usize;
        for slot in &SLOTS {
            let start = slot.start.load(Ordering::Acquire);
            if start != 0 && addr >= start && addr < start + slot.len.load(Ordering::Relaxed) {
                let page = addr & !(PAGE.load(Ordering::Relaxed) - 1);
                // SAFETY: replaces one page inside our own read-only mapping with zeros.
                let r = unsafe {
                    libc::mmap(
                        page as *mut libc::c_void,
                        PAGE.load(Ordering::Relaxed),
                        libc::PROT_READ,
                        libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_FIXED,
                        -1,
                        0,
                    )
                };
                if r != libc::MAP_FAILED {
                    slot.faulted.store(true, Ordering::Release);
                    return;
                }
            }
        }
        // Not ours: put the previous action back; returning re-runs the faulting access.
        // SAFETY: PREV was initialized before this handler was installed.
        unsafe { libc::sigaction(libc::SIGBUS, (*PREV.0.get()).as_ptr(), std::ptr::null_mut()) };
    }

    fn install() {
        INSTALL.call_once(|| {
            // SAFETY: plain libc calls with valid, zero-initialized structs.
            unsafe {
                PAGE.store(
                    libc::sysconf(libc::_SC_PAGESIZE) as usize,
                    Ordering::Relaxed,
                );
                libc::sigaction(libc::SIGBUS, std::ptr::null(), (*PREV.0.get()).as_mut_ptr());
                let mut action: libc::sigaction = std::mem::zeroed();
                let handler: extern "C" fn(libc::c_int, *mut libc::siginfo_t, *mut libc::c_void) =
                    on_sigbus;
                action.sa_sigaction = handler as usize;
                action.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK;
                libc::sigemptyset(&mut action.sa_mask);
                libc::sigaction(libc::SIGBUS, &action, std::ptr::null_mut());
            }
        });
    }

    pub fn register(start: usize, len: usize) -> Option<usize> {
        install();
        let i = SLOTS.iter().position(|s| {
            s.used
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
        })?;
        let s = &SLOTS[i];
        s.faulted.store(false, Ordering::Relaxed);
        s.len.store(len, Ordering::Relaxed);
        s.start.store(start, Ordering::Release);
        Some(i)
    }

    pub fn unregister(i: usize) {
        let s = &SLOTS[i];
        s.start.store(0, Ordering::Release);
        s.len.store(0, Ordering::Relaxed);
        s.used.store(false, Ordering::Release);
    }

    pub fn faulted(i: usize) -> bool {
        SLOTS[i].faulted.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempFile;
    use crate::{Dialect, IndexError, SparseRowIndex};
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    #[test]
    fn truncation_while_mapped_reports_changed_instead_of_crashing() {
        let content: Vec<u8> = (0..200_000)
            .flat_map(|i| format!("{i},x\n").into_bytes())
            .collect();
        let file = TempFile::new(&content);
        let source = Arc::new(Source::open(file.path()).unwrap());
        assert!(!source.changed());

        std::fs::OpenOptions::new()
            .write(true)
            .open(file.path())
            .unwrap()
            .set_len(0)
            .unwrap();
        let tail = source.bytes()[content.len() - 10];
        assert_eq!(tail, 0, "lost page reads as zeros");
        assert!(source.changed());

        let idx = SparseRowIndex::new(source, &Dialect::default());
        assert_eq!(
            idx.build(&AtomicBool::new(false)),
            Err(IndexError::FileChanged)
        );
    }

    #[test]
    fn empty_file_has_no_bytes() {
        let file = TempFile::new(b"");
        let source = Source::open(file.path()).unwrap();
        assert!(source.bytes().is_empty());
        assert!(!source.changed());
    }
}
