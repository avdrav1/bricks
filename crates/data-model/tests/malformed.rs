//! APP-8: malformed files are found with the line and the offending text. A file that
//! opens fine has no problems (the nasty corpus included: odd but well-formed); a quote
//! that never closes, text after a closing quote, and binary data are each placed exactly,
//! across chunks and after quoted newlines; and cancelling stops the check.

use data_model::{CsvTable, DelimiterChoice, ProblemKind, Problems};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

struct Tmp(PathBuf);

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn tmp(name: &str, bytes: &[u8]) -> Tmp {
    let path = std::env::temp_dir().join(format!("app8-{}-{name}", std::process::id()));
    std::fs::write(&path, bytes).unwrap();
    Tmp(path)
}

fn open(path: &Path) -> CsvTable {
    let t = CsvTable::open(path, DelimiterChoice::Auto).unwrap();
    t.index().build(&AtomicBool::new(false)).unwrap();
    t
}

fn check(path: &Path) -> Problems {
    open(path).check_file(&AtomicBool::new(false)).unwrap()
}

#[test]
fn well_formed_files_have_no_problems() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/nasty");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("corpus/nasty: run python3 scripts/gen_corpus.py")
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    assert!(files.len() >= 13);
    for f in files {
        let t = open(&f);
        assert_eq!(t.binary_at_start(), None, "{}", f.display());
        let p = t.check_file(&AtomicBool::new(false)).unwrap();
        assert!(p.is_empty(), "{}: {p:?}", f.display());
    }
}

#[test]
fn an_unclosed_quote_is_placed_after_quoted_newlines() {
    let f = tmp(
        "unclosed.csv",
        b"id,name,note\n1,\"Smith, J\",\"two\nlines\"\n2,Jones,ok\n3,\"Lee, K,never closed\n4,x,y\n",
    );
    let p = check(&f.0);
    assert_eq!(p.total, 1);
    let first = &p.first[0];
    assert_eq!(first.kind, ProblemKind::UnclosedQuote);
    assert_eq!(first.line, 5, "the quoted newline in row 1 is a line too");
    assert_eq!(first.file_row, 3, "file row 3 (the header is row 0)");
    assert_eq!(first.excerpt, "3,\"Lee, K,never closed");
}

#[test]
fn text_after_a_quote_counts_every_one_and_keeps_the_first_five() {
    // 200k rows: problems in several chunks of the parallel check, in file order.
    let mut csv = String::from("id,name\n");
    let bad = [10u64, 70_000, 70_001, 140_000, 150_000, 190_000, 199_999];
    for i in 0..200_000u64 {
        if bad.contains(&i) {
            csv.push_str(&format!("{i},\"name {i}\"x\n"));
        } else {
            csv.push_str(&format!("{i},\"name {i}\"\n"));
        }
    }
    let f = tmp("after.csv", csv.as_bytes());
    let p = check(&f.0);
    assert_eq!(p.total, bad.len() as u64);
    let lines: Vec<u64> = p.first.iter().map(|p| p.line).collect();
    assert_eq!(lines, [12, 70_002, 70_003, 140_002, 150_002]);
    assert!(p
        .first
        .iter()
        .all(|p| p.kind == ProblemKind::TextAfterQuote));
    assert_eq!(p.first[1].excerpt, "70000,\"name 70000\"x");
    assert_eq!(p.first[1].file_row, 70_001);
}

#[test]
fn binary_data_is_found_at_the_start_and_further_in() {
    let mut start = b"PK\x03\x04\0\0".to_vec();
    start.extend(std::iter::repeat_n(b'x', 1000));
    let f = tmp("zip.csv", &start);
    let t = open(&f.0);
    let b = t.binary_at_start().expect("binary");
    assert_eq!((b.kind, b.line), (ProblemKind::Binary, 1));
    assert!(b.excerpt.starts_with("PK"), "{}", b.excerpt);
    assert!(b.excerpt.contains("\\0"), "{}", b.excerpt);

    // Past the sample, a zero byte is a problem like the others.
    let mut csv = String::from("a,b\n");
    for i in 0..20_000 {
        csv.push_str(&format!("{i},value\n"));
    }
    csv.push_str("x,y\0z\n");
    let f = tmp("late-zero.csv", csv.as_bytes());
    let t = open(&f.0);
    assert_eq!(t.binary_at_start(), None);
    let p = t.check_file(&AtomicBool::new(false)).unwrap();
    assert_eq!(p.total, 1);
    assert_eq!(
        (p.first[0].kind, p.first[0].line),
        (ProblemKind::Binary, 20_002)
    );
    assert_eq!(p.first[0].excerpt, "x,y\\0z");
}

#[test]
fn a_cancelled_check_gives_nothing() {
    let f = tmp("cancel.csv", b"a,b\n1,\"2\"x\n");
    let t = open(&f.0);
    assert_eq!(t.check_file(&AtomicBool::new(true)), None);
}

/// APP-8 at scale: the whole 1 GB file is checked on the job pool.
/// `cargo test --release -p data-model --test malformed -- --ignored --nocapture`
#[test]
#[ignore = "needs corpus/rows_1024mb.csv; run with --release --ignored"]
fn checking_1gb() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/rows_1024mb.csv");
    let t = open(&path);
    let start = std::time::Instant::now();
    let p = t.check_file(&AtomicBool::new(false)).unwrap();
    eprintln!(
        "checked 1 GB in {:.0} ms: {} problems",
        start.elapsed().as_secs_f64() * 1e3,
        p.total
    );
    assert!(p.is_empty(), "{p:?}");
}
