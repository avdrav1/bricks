//! SAVE-4 acceptance: saving an unedited file produces an empty diff, and one edit changes
//! only the edited row's lines. Untouched rows keep their quoting, line endings, BOM, and
//! encoding.
//!
//! Runs over every file in `corpus/nasty/` (`python3 scripts/gen_corpus.py`) and
//! `corpus/realworld/` (`python3 scripts/fetch_delimiter_sample.py`):
//! `cargo test --release -p file-format --test untouched_rows -- --ignored --nocapture`

use csv_engine::{open_text_as, RowIndex};
use data_model::{CellRef, CsvTable, DelimiterChoice, Edit, SaveProgress, TableSource};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

fn corpus_files() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
    let mut files = Vec::new();
    for dir in ["nasty", "realworld"] {
        let found: Vec<PathBuf> = std::fs::read_dir(root.join(dir))
            .unwrap_or_else(|e| panic!("corpus/{dir}: {e} (see this file's docs)"))
            .map(|e| e.unwrap().path())
            .filter(|p| p.is_file())
            .collect();
        assert!(!found.is_empty(), "corpus/{dir} is empty");
        files.extend(found);
    }
    files.sort();
    files
}

fn open(path: &Path) -> CsvTable {
    let t = CsvTable::open(path, DelimiterChoice::Auto).unwrap();
    t.index().build(&AtomicBool::new(false)).unwrap();
    t
}

fn save(path: &Path, t: &CsvTable) {
    file_format::save_csv(path, &t.save_job().unwrap(), &SaveProgress::default()).unwrap();
}

/// The file's text in `t`'s encoding, as the app reads it (UTF-8), split into lines.
fn lines(path: &Path, t: &CsvTable) -> Vec<Vec<u8>> {
    let text = open_text_as(path, t.encoding()).unwrap();
    text.bytes()
        .split(|&b| b == b'\n')
        .map(<[u8]>::to_vec)
        .collect()
}

/// Save `file` unedited, then with one cell edited; returns whether the edit step ran, or
/// what went wrong.
fn check(file: &Path, dir: &Path) -> Result<bool, String> {
    let name = file.file_name().unwrap().to_string_lossy().into_owned();
    let copy = dir.join(&name);
    std::fs::copy(file, &copy).unwrap();
    let original = std::fs::read(file).unwrap();
    let mut table = open(&copy);
    save(&copy, &table);
    if std::fs::read(&copy).unwrap() != original {
        return Err("an unedited save changed the bytes".into());
    }
    if ["empty.csv", "header_only.csv"].contains(&name.as_str()) || table.row_count() < 2 {
        return Ok(false);
    }

    // Edit the middle row's first cell; find its physical lines in the original text.
    let row = table.row_count() / 2;
    let file_row = row + u64::from(table.has_header());
    let span = table.index().row_span(file_row).unwrap();
    let text = table.index().source().bytes();
    let quoted = text[span.start as usize..span.end as usize].contains(&b'"');
    let first_line = text[..span.start as usize]
        .iter()
        .filter(|&&b| b == b'\n')
        .count();
    let row_lines = text[span.start as usize..span.end as usize]
        .strip_suffix(b"\n")
        .unwrap_or(&text[span.start as usize..span.end as usize])
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1;
    let before = lines(&copy, &table);
    let at = CellRef {
        row: table.row_id(row),
        col: table.col_id(0),
    };
    table.apply(Edit::set(at, "edited"));
    save(&copy, &table);
    let after = lines(&copy, &table);

    let kept_after = before.len() - first_line - row_lines;
    if after.len() < first_line + 1 + kept_after {
        return Err(format!(
            "{} lines before, {} after",
            before.len(),
            after.len()
        ));
    }
    if before[..first_line] != after[..first_line] {
        let l = (0..first_line).find(|&l| before[l] != after[l]).unwrap();
        return Err(format!("line {} changed (before the edited row)", l + 1));
    }
    if !after[first_line].starts_with(b"edited") {
        return Err(format!(
            "line {} is {:?}, not the edited row",
            first_line + 1,
            String::from_utf8_lossy(&after[first_line])
        ));
    }
    let (tail_before, tail_after) = (
        &before[before.len() - kept_after..],
        &after[after.len() - kept_after..],
    );
    if tail_before != tail_after {
        let l = (0..kept_after)
            .find(|&l| tail_before[l] != tail_after[l])
            .unwrap();
        return Err(format!(
            "line {} changed (after the edited row)",
            before.len() - kept_after + l + 1
        ));
    }
    if after.len() - kept_after - first_line != row_lines && !quoted {
        return Err("the edited row's line count changed".into());
    }
    Ok(true)
}

#[test]
#[ignore = "needs corpus/nasty and corpus/realworld; run with --ignored"]
fn unedited_saves_are_byte_identical_and_one_edit_changes_one_line() {
    let dir = std::env::temp_dir().join(format!("save4-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let files = corpus_files();
    let (mut edited, mut failures) = (0, Vec::new());
    for f in &files {
        match check(f, &dir) {
            Ok(e) => edited += usize::from(e),
            Err(e) => failures.push(format!("{}: {e}", f.file_name().unwrap().to_string_lossy())),
        }
    }
    std::fs::remove_dir_all(&dir).unwrap();
    eprintln!(
        "{} files saved unedited, {edited} also with an edit",
        files.len()
    );
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
