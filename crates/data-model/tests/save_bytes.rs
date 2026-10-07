//! SAVE-1 acceptance, content half: a save is byte-identical to the source except the edited
//! cells. Untouched rows are copied verbatim; in an edited row, untouched fields keep their
//! exact bytes (quotes, escapes) and only edited cells are re-encoded.

use csv_engine::{Dialect, RowIndex, Source, SparseRowIndex};
use data_model::{CellRef, CsvTable, Edit};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn table(content: &[u8], name: &str) -> (std::path::PathBuf, CsvTable) {
    let path = std::env::temp_dir().join(format!("save-bytes-{}-{name}.csv", std::process::id()));
    std::fs::write(&path, content).unwrap();
    let index = SparseRowIndex::new(Arc::new(Source::open(&path).unwrap()), &Dialect::default());
    index.build(&AtomicBool::new(false)).unwrap();
    (path, CsvTable::new(index, Dialect::default()))
}

fn save(table: &CsvTable) -> Vec<u8> {
    let mut out = Vec::new();
    let stats = table.save_job().unwrap().write_to(&mut out).unwrap();
    assert_eq!(stats.bytes, out.len() as u64);
    out
}

fn set(t: &mut CsvTable, row: u64, col: u32, v: &str) {
    let at = CellRef {
        row: t.row_id(row),
        col: t.col_id(col),
    };
    t.apply(Edit::set(at, v));
}

#[test]
fn unedited_save_is_the_source_byte_for_byte() {
    let content = b"\xEF\xBB\xBFid,\"note\"\r\n1,\"a \"\"q\"\"\nb\"\r\n2,x";
    let (path, t) = table(content, "noedit");
    assert_eq!(save(&t), content);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn only_edited_cells_change() {
    let content = b"\xEF\xBB\xBFid,name,note\r\n1,\"Smith, J\",\"keep \"\"me\"\"\"\r\n2,Lee,  spaced  \r\n3,last,no newline";
    let (path, mut t) = table(content, "edits");
    set(&mut t, 0, 2, "Note, with comma");
    set(&mut t, 1, 0, "01");
    set(&mut t, 2, 4, "grown \"row\"\nline two");
    set(&mut t, 3, 1, "");
    let got = save(&t);
    let want: &[u8] = b"\xEF\xBB\xBFid,name,\"Note, with comma\"\r\n\
        01,\"Smith, J\",\"keep \"\"me\"\"\"\r\n\
        2,Lee,  spaced  ,,\"grown \"\"row\"\"\nline two\"\r\n\
        3,,no newline";
    assert_eq!(String::from_utf8_lossy(&got), String::from_utf8_lossy(want));
    assert_eq!(
        std::fs::read(&path).unwrap(),
        content,
        "saving to a writer never touches the source"
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn random_edits_change_only_their_rows_and_read_back() {
    let mut content = Vec::new();
    for i in 0..5_000u32 {
        let line = match i % 4 {
            0 => format!("{i},plain,{:05}\n", i % 977),
            1 => format!("{i},\"quoted, comma\",\"multi\nline\"\n"),
            2 => format!("{i},\"esc \"\"q\"\"\",x\r\n"),
            _ => format!("{i},short\n"),
        };
        content.extend_from_slice(line.as_bytes());
    }
    let (path, mut t) = table(&content, "random");
    let original_rows: Vec<Vec<u8>> = {
        let idx = SparseRowIndex::new(Arc::new(Source::open(&path).unwrap()), &Dialect::default());
        idx.build(&AtomicBool::new(false)).unwrap();
        (0..idx.row_count())
            .map(|r| {
                let s = idx.row_span(r).unwrap();
                content[s.start as usize..s.end as usize].to_vec()
            })
            .collect()
    };
    let mut x = 7u64;
    let mut edited = std::collections::BTreeMap::new();
    for k in 0..300 {
        x = x
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let (row, col) = ((x >> 33) % 5_000, ((x >> 20) % 5) as u32);
        let v = format!("v{k},\"{}\"", "\n".repeat(k % 2));
        set(&mut t, row, col, &v);
        edited.insert((row, col), v);
    }
    let saved = save(&t);

    let out_path = path.with_extension("out.csv");
    std::fs::write(&out_path, &saved).unwrap();
    let idx = SparseRowIndex::new(
        Arc::new(Source::open(&out_path).unwrap()),
        &Dialect::default(),
    );
    idx.build(&AtomicBool::new(false)).unwrap();
    assert_eq!(
        idx.row_count(),
        5_000,
        "edits with newlines and quotes never split rows"
    );
    let reread = CsvTable::new(idx.clone(), Dialect::default());
    for r in 0..5_000u64 {
        let s = idx.row_span(r).unwrap();
        let row = &saved[s.start as usize..s.end as usize];
        if edited.keys().any(|&(er, _)| er == r) {
            for (&(er, c), v) in &edited {
                if er == r {
                    assert_eq!(
                        reread.cell_value(r, c).as_deref(),
                        Some(v.as_str()),
                        "row {r} col {c}"
                    );
                }
            }
        } else {
            assert_eq!(
                row, original_rows[r as usize],
                "untouched row {r} is byte-identical"
            );
        }
    }
    std::fs::remove_file(path).unwrap();
    std::fs::remove_file(out_path).unwrap();
}
