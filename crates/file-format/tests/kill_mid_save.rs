//! SAVE-1 acceptance, safety half: `kill -9` in the middle of a save leaves the original
//! file intact.
//!
//! The test re-runs its own binary as a child (`child_saver`) that saves a 64 MB CSV with
//! one edit, slowed to ~1 ms per MiB so the parent can catch it mid-write. The parent kills
//! it with SIGKILL once the temp file has data in it, then checks the original. A second
//! child runs to completion to show the same save does replace the file.

use csv_engine::{Dialect, Source, SparseRowIndex};
use data_model::{CellRef, CsvTable};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

const CHILD_ENV: &str = "SAVE1_CHILD_FILE";

/// Writes through to `inner`, pausing 1 ms per MiB.
struct Slow<'a>(&'a mut dyn Write, usize);

impl Write for Slow<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.0.write(&buf[..buf.len().min(64 * 1024)])?;
        self.1 += n;
        if self.1 >= 1 << 20 {
            self.1 = 0;
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

#[test]
#[ignore = "child process for kill_9_mid_save_leaves_the_original_intact"]
fn child_saver() {
    let Some(file) = std::env::var_os(CHILD_ENV) else {
        return;
    };
    let path = PathBuf::from(file);
    let index = SparseRowIndex::new(Arc::new(Source::open(&path).unwrap()), &Dialect::default());
    index.build(&AtomicBool::new(false)).unwrap();
    let mut table = CsvTable::new(index, Dialect::default());
    table.set_cell(
        CellRef {
            row: table.row_id(1),
            col: 1,
        },
        "EDITED",
    );
    let job = table.save_job().unwrap();
    file_format::save_atomic(
        &path,
        |w| job.write_to(&mut Slow(w, 0)).map(|_| ()),
        |_| Ok(()),
    )
    .unwrap();
}

fn spawn_child(file: &Path) -> Child {
    Command::new(std::env::current_exe().unwrap())
        .args([
            "child_saver",
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_ENV, file)
        .spawn()
        .unwrap()
}

fn temp_files(dir: &Path) -> Vec<(PathBuf, u64)> {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| {
            let e = e.ok()?;
            let name = e.file_name().into_string().ok()?;
            let len = e.metadata().ok()?.len();
            name.ends_with(file_format::TEMP_SUFFIX)
                .then(|| (e.path(), len))
        })
        .collect()
}

#[test]
fn kill_9_mid_save_leaves_the_original_intact() {
    let dir = std::env::temp_dir().join(format!("save1-kill-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("data.csv");
    let mut content = Vec::with_capacity(64 << 20);
    let mut i = 0u64;
    while content.len() < 64 << 20 {
        content.extend_from_slice(format!("{i},\"name {i}\",{:05}\n", i % 99_999).as_bytes());
        i += 1;
    }
    std::fs::write(&file, &content).unwrap();

    // Kill mid-write.
    let mut child = spawn_child(&file);
    let deadline = Instant::now() + Duration::from_secs(30);
    let caught = loop {
        if let Some((_, len)) = temp_files(&dir).first() {
            if *len > 4 << 20 {
                break true;
            }
        }
        if child.try_wait().unwrap().is_some() || Instant::now() > deadline {
            break false;
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    child.kill().unwrap(); // SIGKILL on Unix
    child.wait().unwrap();
    assert!(
        caught,
        "the child finished or never started writing before it could be killed"
    );
    let partial = temp_files(&dir);
    assert_eq!(
        partial.len(),
        1,
        "the interrupted save left its temp file, not the original"
    );
    assert!(
        partial[0].1 < content.len() as u64,
        "the temp file was only partly written"
    );
    assert!(
        std::fs::read(&file).unwrap() == content,
        "original changed by an interrupted save"
    );

    // The same save run to completion replaces the file.
    std::fs::remove_file(&partial[0].0).unwrap();
    assert!(spawn_child(&file).wait().unwrap().success());
    let saved = std::fs::read(&file).unwrap();
    assert!(
        saved.starts_with(b"0,\"name 0\",00000\n1,EDITED,00001\n2,"),
        "{:?}",
        String::from_utf8_lossy(&saved[..60])
    );
    assert_eq!(
        saved.len(),
        content.len() - "\"name 1\"".len() + "EDITED".len()
    );
    assert!(
        temp_files(&dir).is_empty(),
        "a finished save leaves no temp file"
    );
    std::fs::remove_dir_all(dir).unwrap();
}
