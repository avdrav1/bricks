//! Where crash recovery journals live (APP-7): `recovery` in the state folder
//! (`~/.local/state/brick/recovery` on Linux, `%LOCALAPPDATA%\Cronkite\Brick\recovery` on
//! Windows, see [`crate::state`]), kept by the suite's
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

    /// The CSV a crashing Brick has open, and what it was then.
    const SAVED: &str = "id,name\n1,apple\n";

    fn open(path: &std::path::Path) -> data_model::CsvTable {
        let t = data_model::CsvTable::open(path, data_model::DelimiterChoice::Auto).unwrap();
        t.index()
            .build(&std::sync::atomic::AtomicBool::new(false))
            .unwrap();
        t
    }

    /// Run in a child process by [`a_killed_bricks_journal_is_offered_back`]: a Brick with
    /// `$BRICK_CRASH_FILE` open (so in its recent files), a cell edited, and the workbook in
    /// its journal, until it's killed (or its stdin closes). Prints the journal's path.
    #[test]
    fn crash_with_unsaved_edits_for_parent() {
        let Some(file) = std::env::var_os("BRICK_CRASH_FILE").map(PathBuf::from) else {
            return;
        };
        crate::state::recent().add(&file).unwrap();
        let mut book =
            data_model::Workbook::single("data".into(), open(&file), data_model::Origin::Csv);
        let t = &mut book.sheet_mut(data_model::SheetId(0)).unwrap().table;
        let cell = data_model::CellRef {
            row: t.row_id(0),
            col: t.col_id(1),
        };
        t.apply(data_model::Edit::set(cell, "pear"));
        let (file_size, file_mtime_ns) = identity(&file).unwrap();
        let header = JournalHeader {
            pid: std::process::id(),
            path: Some(file),
            file_size,
            file_mtime_ns,
            saved_ms: now_ms(),
            changes: 1,
            rows: 1,
            ..header(0, 0)
        };
        let store = store();
        let journal = store.new_journal();
        store
            .write(&journal, |f| book.snapshot().write_journal(&header, f))
            .unwrap();
        println!("journal\t{}", journal.display());
        let _ = std::io::stdin().read_line(&mut String::new());
    }

    /// WIN-2: a Brick killed with unsaved edits leaves its journal in Brick's state folder
    /// (`%LOCALAPPDATA%\Cronkite\Brick\recovery` on Windows), which another Brick leaves
    /// alone while it runs and offers back once it's gone, and whose edits go back onto the
    /// file; its recent files are in `%APPDATA%\Cronkite\Brick` on Windows, in the state
    /// folder elsewhere.
    #[test]
    fn a_killed_bricks_journal_is_offered_back() {
        use std::io::BufRead;
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};
        let base = std::env::temp_dir().join(format!("brick-crash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let file = base.join("fruit.csv");
        std::fs::write(&file, SAVED).unwrap();
        let mut brick = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "recovery::tests::crash_with_unsaved_edits_for_parent",
                "--nocapture",
            ])
            .env("BRICK_CRASH_FILE", &file)
            .env("HOME", base.join("home"))
            .env("APPDATA", base.join("Roaming"))
            .env("LOCALAPPDATA", base.join("Local"))
            .env("XDG_CONFIG_HOME", base.join("config"))
            .env("XDG_STATE_HOME", base.join("state"))
            .env("XDG_CACHE_HOME", base.join("cache"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let journal = std::io::BufReader::new(brick.stdout.take().unwrap())
            .lines()
            .find_map(|l| l.ok()?.strip_prefix("journal\t").map(PathBuf::from))
            .expect("the other Brick's journal");
        // Each system's folder for Brick under `base`.
        let folder = |linux: &str, windows: &str| {
            if cfg!(windows) {
                base.join(windows).join("Cronkite").join("Brick")
            } else if cfg!(target_os = "macos") {
                base.join("home/Library/Application Support/dev.cronkite.Brick")
            } else {
                base.join(linux).join("brick")
            }
        };
        let recovery = folder("state", "Local").join("recovery");
        assert_eq!(journal.parent(), Some(recovery.as_path()));
        let recent = folder("state", "Roaming").join("recent");
        assert_eq!(
            suite_file::recent::Recent::load_from(recent).files(),
            [suite_file::canonical(&file)]
        );

        let store = Store::at(recovery.clone());
        assert!(
            crashed_in(&store).is_empty(),
            "a running Brick's journal stays"
        );
        brick.kill().unwrap();
        brick.wait().unwrap();
        let start = Instant::now();
        let found = loop {
            // Windows drops a killed process's locks soon after it ends.
            let found = crashed_in(&store);
            if !found.is_empty() || start.elapsed() > Duration::from_secs(10) {
                break found;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].journal, journal);
        assert_eq!(found[0].header.path.as_deref(), Some(file.as_path()));
        assert_eq!(found[0].header.pid, brick.id());
        assert_eq!(
            (found[0].header.file_size, found[0].header.file_mtime_ns),
            identity(&file).unwrap(),
            "the file is as it was"
        );
        let (_, state) = data_model::read_book_journal(File::open(&journal).unwrap()).unwrap();
        let mut book =
            data_model::Workbook::single("data".into(), open(&file), data_model::Origin::Csv);
        for edit in book.restore_plan(state).unwrap() {
            book.apply(edit);
        }
        let mut out = Vec::new();
        book.sheets()[0]
            .table
            .save_job()
            .unwrap()
            .write_to(
                &mut out,
                &data_model::SaveProgress::default(),
                &std::sync::atomic::AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "id,name\n1,pear\n");
        store.remove(&journal);
        assert!(!journal.exists());
        assert!(!suite_file::owner::lock_path(&recovery, brick.id()).exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}
