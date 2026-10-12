//! Saving a table as CSV (spec section 19): written atomically through
//! [`suite_file::save_atomic`], then verified by re-reading it.

use csv_engine::{open_text_as, Charset, EncodeWriter, RowIndex, SparseRowIndex};
use data_model::{SaveJob, SaveProgress, SaveStats};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use suite_file::{save_atomic, SaveError};

/// Save a table to `dest` (usually the file it was opened from), in the file's own encoding
/// and BOM (ENG-6). Verification decodes the written file the same way, re-indexes it, and
/// checks its size and row count against what was written. `progress` counts the bytes
/// written; setting `cancel` stops the save, which then ends in [`SaveError::Cancelled`]
/// (SAVE-3), unless it was already renaming.
pub fn save_csv(
    dest: &Path,
    job: &SaveJob,
    progress: &SaveProgress,
    cancel: &AtomicBool,
) -> Result<SaveStats, SaveError> {
    let encoding = job.encoding();
    let stats = std::cell::Cell::new(None);
    let saved = save_atomic(
        dest,
        cancel,
        |w| {
            // UTF-8 BOMs are part of the source bytes and copy through with row 0.
            if encoding.charset == Charset::Utf8 {
                stats.set(Some(job.write_to(w, progress, cancel)?));
            } else {
                let mut encoded = EncodeWriter::new(w, encoding);
                stats.set(Some(job.write_to(&mut encoded, progress, cancel)?));
                encoded.finish()?;
            }
            Ok(())
        },
        |tmp| {
            progress.set_checking();
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
            index.build(cancel).map_err(|e| e.to_string())?;
            if index.row_count() != written.rows {
                return Err(format!(
                    "{} rows on disk, {} written",
                    index.row_count(),
                    written.rows
                ));
            }
            Ok(())
        },
    );
    match saved {
        Ok(()) => Ok(stats.get().expect("save succeeded")),
        Err(_) if cancel.load(Ordering::Relaxed) => Err(SaveError::Cancelled),
        Err(e) => Err(e),
    }
}
