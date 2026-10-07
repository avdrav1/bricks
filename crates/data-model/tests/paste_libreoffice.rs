//! CLIP-1 acceptance, LibreOffice part: a copied range pastes into Calc cell for cell.
//! The copy's clipboard flavors go onto a headless Calc's clipboard through UNO and Edit >
//! Paste runs, the same import a paste from the desktop clipboard goes through
//! (`scripts/lo_paste.py`). Every cell must come back with the file's text: numbers as
//! numbers, while leading zeros, long IDs, and `=` stay text, and quotes, commas, line
//! breaks, tabs, runs of spaces, markup characters, and non-ASCII survive.
//!
//! The plain-text flavor alone pastes nothing headless: for multi-line text Calc asks how
//! to split it (the Text Import dialog). That is why the copy also carries HTML.
//!
//! Needs LibreOffice with Python UNO:
//! `cargo test -p data-model --test paste_libreoffice -- --ignored --nocapture`

use data_model::{CsvTable, DelimiterChoice};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64};

const CSV: &str = "id,code,name,note,amount\n\
1,00692,\"Smith, Ann\",\"line one\nline two\",97196\n\
2,1234567890123456789,=1+1,a & <b> \"q\",275.03\n\
3,-7,Zoë 東京,\"  two  spaces \",\n\
4,,\"tab\there\",TRUE,2026-04-05\n";

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[test]
#[ignore = "needs LibreOffice with Python UNO; run with --ignored"]
fn a_copied_range_pastes_into_libreoffice_cell_for_cell() {
    let dir = std::env::temp_dir().join(format!("paste-lo-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let csv = dir.join("in.csv");
    std::fs::write(&csv, CSV).unwrap();
    let mut table = CsvTable::open(&csv, DelimiterChoice::Auto).unwrap();
    table.index().build(&AtomicBool::new(false)).unwrap();
    let copied = table
        .copy_range(0..4, 0..5, &AtomicBool::new(false), &AtomicU64::new(0))
        .unwrap();
    let (html, tsv) = (dir.join("copy.html"), dir.join("copy.tsv"));
    std::fs::write(&html, copied.html.as_deref().unwrap()).unwrap();
    std::fs::write(&tsv, &copied.tsv).unwrap();

    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/lo_paste.py");
    let run = Command::new("python3")
        .arg(&script)
        .arg("html+text")
        .args([&html, &tsv])
        .output()
        .unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let pasted: Vec<Vec<(String, String)>> = String::from_utf8(run.stdout)
        .unwrap()
        .lines()
        .map(|line| {
            line.split('\t')
                .map(|cell| {
                    let (kind, text) = cell.split_once(':').unwrap();
                    (kind.to_owned(), unescape(text))
                })
                .collect()
        })
        .collect();
    eprintln!("{pasted:#?}");

    let v = |t: &str| ("value".to_owned(), t.to_owned());
    let t = |t: &str| ("text".to_owned(), t.to_owned());
    let empty = ("empty".to_owned(), String::new());
    assert_eq!(
        pasted,
        [
            vec![
                v("1"),
                t("00692"),
                t("Smith, Ann"),
                t("line one\nline two"),
                v("97196")
            ],
            vec![
                v("2"),
                t("1234567890123456789"),
                t("=1+1"),
                t("a & <b> \"q\""),
                v("275.03")
            ],
            vec![
                v("3"),
                v("-7"),
                t("Zoë 東京"),
                t("  two  spaces "),
                empty.clone()
            ],
            vec![v("4"), empty, t("tab\there"), t("TRUE"), t("2026-04-05")],
        ]
    );
}
