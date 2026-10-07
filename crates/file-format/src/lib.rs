//! Safe saving (spec section 19, invariant 6): write a temp file in the same directory,
//! fsync it, verify it, then atomically rename it over the original and fsync the
//! directory. A crash or `kill -9` at any point leaves either the old file or the new one,
//! never a mix. The rename also keeps the app's open mapping of the old file valid (ADR 0002).

use csv_engine::{open_text_as, Charset, EncodeWriter, RowIndex, SparseRowIndex};
use data_model::{SaveJob, SaveStats};
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

/// Temp files end in this, next to the file being saved: `.<name>.<pid>-<n>.bricks-save`.
pub const TEMP_SUFFIX: &str = ".bricks-save";

#[derive(Debug)]
pub enum SaveError {
    Io(io::Error),
    Verify(String),
}

impl From<io::Error> for SaveError {
    fn from(e: io::Error) -> Self {
        SaveError::Io(e)
    }
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::Io(e) => write!(f, "{e}"),
            SaveError::Verify(m) => write!(f, "the saved file did not check out: {m}"),
        }
    }
}

impl std::error::Error for SaveError {}

fn temp_path(dest: &Path) -> io::Result<PathBuf> {
    static N: AtomicU64 = AtomicU64::new(0);
    let name = dest
        .file_name()
        .ok_or_else(|| io::Error::other("save path has no file name"))?;
    let tmp = format!(
        ".{}.{}-{}{TEMP_SUFFIX}",
        name.to_string_lossy(),
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    );
    Ok(dest.with_file_name(tmp))
}

/// Atomically replace `dest` with the bytes `write` produces. `verify` inspects the fully
/// written and fsynced temp file before it is renamed into place. On any error the original
/// is untouched and the temp file is removed.
pub fn save_atomic<W, V>(dest: &Path, write: W, verify: V) -> Result<(), SaveError>
where
    W: FnOnce(&mut dyn Write) -> io::Result<()>,
    V: FnOnce(&Path) -> Result<(), String>,
{
    let tmp = temp_path(dest)?;
    let result = write_verify_rename(dest, &tmp, write, verify);
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn write_verify_rename<W, V>(dest: &Path, tmp: &Path, write: W, verify: V) -> Result<(), SaveError>
where
    W: FnOnce(&mut dyn Write) -> io::Result<()>,
    V: FnOnce(&Path) -> Result<(), String>,
{
    let file = OpenOptions::new().write(true).create_new(true).open(tmp)?;
    if let Ok(meta) = std::fs::metadata(dest) {
        file.set_permissions(meta.permissions())?;
    }
    let mut out = BufWriter::with_capacity(1 << 20, file);
    write(&mut out)?;
    let file = out.into_inner().map_err(|e| e.into_error())?;
    file.sync_all()?;
    drop(file);
    verify(tmp).map_err(SaveError::Verify)?;
    std::fs::rename(tmp, dest)?;
    // Make the rename itself durable.
    let dir = dest
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    File::open(dir)?.sync_all()?;
    Ok(())
}

/// Save a table to `dest` (usually the file it was opened from), in the file's own encoding
/// and BOM (ENG-6). Verification decodes the written file the same way, re-indexes it, and
/// checks its size and row count against what was written.
pub fn save_csv(dest: &Path, job: &SaveJob) -> Result<SaveStats, SaveError> {
    let encoding = job.encoding();
    let stats = std::cell::Cell::new(None);
    save_atomic(
        dest,
        |w| {
            // UTF-8 BOMs are part of the source bytes and copy through with row 0.
            if encoding.charset == Charset::Utf8 {
                stats.set(Some(job.write_to(w)?));
            } else {
                let mut encoded = EncodeWriter::new(w, encoding);
                stats.set(Some(job.write_to(&mut encoded)?));
                encoded.finish()?;
            }
            Ok(())
        },
        |tmp| {
            let written = stats.get().expect("write ran before verify");
            let source = open_text_as(tmp, encoding).map_err(|e| e.to_string())?;
            if source.bytes().len() as u64 != written.bytes {
                return Err(format!(
                    "{} bytes on disk, {} written",
                    source.bytes().len(),
                    written.bytes
                ));
            }
            let index = SparseRowIndex::new(Arc::new(source), job.dialect());
            index
                .build(&AtomicBool::new(false))
                .map_err(|e| e.to_string())?;
            if index.row_count() != written.rows {
                return Err(format!(
                    "{} rows on disk, {} written",
                    index.row_count(),
                    written.rows
                ));
            }
            Ok(())
        },
    )?;
    Ok(stats.get().expect("save succeeded"))
}
