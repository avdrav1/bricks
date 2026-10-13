//! Where crash recovery journals live (APP-7): `recovery` in the state folder
//! (`~/.local/state/brick/recovery`, see [`crate::state`]), kept by the suite's
//! [`suite_file::recovery::Store`]: one journal per window with unsaved edits, and an owner
//! lock per process, so a journal whose process is gone is a crash's. What a journal holds
//! is the workbook's ([`data_model`]'s journal format).

use data_model::{read_journal_header, JournalHeader};
use std::fs::File;
use std::path::PathBuf;
pub use suite_file::recovery::{identity, now_ms, Store};

/// A journal left by a process that is gone.
#[derive(Debug, Clone)]
pub struct Crashed {
    pub journal: PathBuf,
    pub header: JournalHeader,
}

/// The user's recovery directory.
pub fn store() -> Store {
    Store::at(crate::state::dir().join("recovery"))
}

/// The journals of Brick processes that are gone, newest first.
pub fn crashed() -> Vec<Crashed> {
    crashed_in(&store())
}

fn crashed_in(store: &Store) -> Vec<Crashed> {
    let mut found: Vec<Crashed> = store
        .crashed()
        .into_iter()
        .filter_map(|journal| {
            let header = File::open(&journal)
                .ok()
                .and_then(|f| read_journal_header(f).ok())?; // being written, or not ours
            Some(Crashed { journal, header })
        })
        .collect();
    found.sort_by_key(|c| std::cmp::Reverse(c.header.saved_ms));
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use csv_engine::Encoding;

    fn header(pid: u32, saved_ms: u64) -> JournalHeader {
        JournalHeader {
            path: Some("/tmp/x.csv".into()),
            kind: data_model::FileKind::Csv,
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

    /// Gone processes' journals come with their headers, the latest saved first; a file
    /// that isn't a journal is left out.
    #[test]
    fn crashed_journals_read_their_headers_newest_first() {
        let dir = std::env::temp_dir().join(format!("app7-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::at(dir.clone());
        for (pid, saved_ms) in [(999_997, 30), (999_998, 10)] {
            let j = dir.join(format!("{pid}-0.journal"));
            store
                .write(&j, |f| {
                    data_model::Workbook::default().write_journal(&header(pid, saved_ms), f)
                })
                .unwrap();
        }
        store
            .write(&dir.join("999999-0.journal"), |f| {
                std::io::Write::write_all(f, b"not a journal")
            })
            .unwrap();
        let crashed = crashed_in(&store);
        let found: Vec<_> = crashed
            .iter()
            .map(|c| (c.header.pid, c.header.changes))
            .collect();
        assert_eq!(found, [(999_997, 3), (999_998, 3)]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
