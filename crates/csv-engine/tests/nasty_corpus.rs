//! ENG-3 acceptance: the nasty corpus parses with no row misalignment.
//!
//! Generates the corpus with the real generator (`scripts/gen_corpus.py --max-mb 0`), so this
//! runs in CI without `corpus/`. Each file is opened the way the app opens it (`open_text`:
//! encoding detected, non-UTF-8 decoded, ENG-6). For every file: the row spans tile the text
//! exactly (no byte lost, duplicated, or split across rows), and every row's field values
//! match. Every nasty file must be listed here.

use csv_engine::{open_text, Dialect, RowIndex, SparseRowIndex};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn generate() -> Dir {
    let out = std::env::temp_dir().join(format!("csv-engine-nasty-{}", std::process::id()));
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/gen_corpus.py");
    let status = Command::new("python3")
        .arg(script)
        .args(["--max-mb", "0", "--out"])
        .arg(&out)
        .output()
        .expect("run python3 scripts/gen_corpus.py");
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    Dir(out)
}

fn parse(path: &Path, delimiter: u8) -> Vec<Vec<Vec<u8>>> {
    let (source, _) = open_text(path).unwrap();
    let len = source.bytes().len() as u64;
    let index = SparseRowIndex::new(
        Arc::new(source),
        &Dialect {
            delimiter,
            ..Dialect::default()
        },
    );
    index.build(&AtomicBool::new(false)).unwrap();
    assert!(index.is_complete());

    let mut spans = Vec::new();
    index.row_spans(0..index.row_count(), &mut spans).unwrap();
    let mut pos = 0;
    for s in &spans {
        assert_eq!(
            s.start,
            pos,
            "{}: gap or overlap before byte {pos}",
            path.display()
        );
        pos = s.end;
    }
    assert_eq!(pos, len, "{}: rows do not reach the end", path.display());

    let mut fields = Vec::new();
    (0..index.row_count())
        .map(|r| {
            let row = index.row_fields(r, &mut fields).unwrap();
            fields
                .iter()
                .map(|f| f.value(row, b'"').into_owned())
                .collect()
        })
        .collect()
}

#[test]
fn nasty_corpus_parses_with_no_row_misalignment() {
    let dir = generate();
    let nasty = dir.0.join("nasty");
    type Rows = &'static [&'static [&'static [u8]]];
    let cases: &[(&str, u8, Rows)] = &[
        ("bom.csv", b',', &[&[b"a", b"b"], &[b"1", b"2"]]),
        (
            "crlf.csv",
            b',',
            &[&[b"a", b"b"], &[b"1", b"2"], &[b"3", b"4"]],
        ),
        ("empty.csv", b',', &[]),
        ("header_only.csv", b',', &[&[b"a", b"b", b"c"]]),
        (
            "latin1.csv",
            b',',
            &[&[b"name"], &[b"Jos\xc3\xa9"], &[b"M\xc3\xbcller"]], // decoded from Windows-1252
        ),
        (
            "leading_zeros.csv",
            b',',
            &[
                &[b"zip", b"code"],
                &[b"02134", b"00123"],
                &[b"07030", b"000"],
            ],
        ),
        (
            "no_trailing_newline.csv",
            b',',
            &[&[b"a", b"b"], &[b"1", b"2"]],
        ),
        ("pipe.csv", b'|', &[&[b"a", b"b"], &[b"1", b"2"]]),
        (
            "quoted_newlines.csv",
            b',',
            &[
                &[b"id", b"note"],
                &[b"1", b"line one\nline two"],
                &[b"2", b"has \"quotes\""],
                &[b"3", b"plain"],
            ],
        ),
        (
            "ragged.csv",
            b',',
            &[
                &[b"a", b"b", b"c"],
                &[b"1", b"2"],
                &[b"3", b"4", b"5", b"6"],
                &[b"7", b"8", b"9"],
            ],
        ),
        (
            "semicolon.csv",
            b';',
            &[&[b"a", b"b", b"c"], &[b"1,5", b"2", b"3"]],
        ),
        ("tab.tsv", b'\t', &[&[b"a", b"b"], &[b"1", b"2"]]),
        ("utf16le.csv", b',', &[&[b"a", b"b"], &[b"1", b"2"]]),
    ];
    for (name, delimiter, want) in cases {
        let got = parse(&nasty.join(name), *delimiter);
        let want: Vec<Vec<Vec<u8>>> = want
            .iter()
            .map(|r| r.iter().map(|f| f.to_vec()).collect())
            .collect();
        assert_eq!(got, want, "{name}");
    }

    let on_disk: BTreeSet<String> = std::fs::read_dir(&nasty)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    let covered: BTreeSet<String> = cases.iter().map(|c| c.0.to_owned()).collect();
    assert_eq!(on_disk, covered, "every nasty file is checked");
}
