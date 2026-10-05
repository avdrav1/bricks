//! Safe saving (spec section 19): write a temp file in the same directory,
//! verify it, fsync, then atomically rename over the original. SAVE-1.

use std::path::Path;

#[derive(Debug)]
pub enum SaveError {
    Io(std::io::Error),
    Verify(String),
}

impl From<std::io::Error> for SaveError {
    fn from(e: std::io::Error) -> Self {
        SaveError::Io(e)
    }
}

/// Atomically replace `dest` with bytes produced by `write`.
pub fn save_atomic<F>(_dest: &Path, _write: F) -> Result<(), SaveError>
where
    F: FnOnce(&mut dyn std::io::Write) -> std::io::Result<()>,
{
    // TODO(SAVE-1): tempfile in dest's dir, write, flush, fsync, verify row count, rename, fsync dir.
    todo!("SAVE-1")
}
