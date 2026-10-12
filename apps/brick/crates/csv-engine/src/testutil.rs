//! Test-only helpers.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// A file in the temp dir, removed on drop.
pub struct TempFile(PathBuf);

impl TempFile {
    pub fn new(content: &[u8]) -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let name = format!(
            "csv-engine-test-{}-{}.csv",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, content).unwrap();
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
