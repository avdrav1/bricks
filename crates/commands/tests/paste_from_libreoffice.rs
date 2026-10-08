//! CLIP-2 acceptance: 10k rows copied in LibreOffice Calc paste in under 1 s. Calc opens
//! the first 10,000 data rows of the 10 MB corpus file, copies them, and its clipboard text
//! is what gets pasted (`scripts/lo_copy.py`). They are pasted near the end of the same
//! file, so 6,689 of them become new rows: parsing, planning, applying as one command,
//! and reading the first screen back is timed. Every pasted cell must read back as
//! Calc's text, and one undo must restore the table.
//!
//! Needs LibreOffice with Python UNO and the corpus; timing is asserted in release builds:
//! `cargo test --release -p commands --test paste_from_libreoffice -- --ignored --nocapture`

use commands::{Batch, UndoStack};
use data_model::{parse_tsv, CellRef, CsvTable, DelimiterChoice, RowBlock, TableSource};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

const ROWS: u64 = 10_000;
const AT: u64 = 186_622; // 6,689 rows before the end

#[test]
#[ignore = "needs LibreOffice with Python UNO and corpus/rows_10mb.csv; run with --release --ignored"]
fn ten_thousand_rows_from_libreoffice_paste_in_under_a_second() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let csv = root.join("corpus/rows_10mb.csv");
    assert!(
        csv.exists(),
        "missing {}; run python3 scripts/gen_corpus.py",
        csv.display()
    );
    let tsv_path = std::env::temp_dir().join(format!("lo-copy-{}.tsv", std::process::id()));
    let run = Command::new("python3")
        .arg(root.join("scripts/lo_copy.py"))
        .arg(&csv)
        .arg(format!("A2:H{}", ROWS + 1))
        .arg(&tsv_path)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let clipboard = std::fs::read_to_string(&tsv_path).unwrap();
    std::fs::remove_file(&tsv_path).unwrap();

    let mut table = CsvTable::open(&csv, DelimiterChoice::Auto).unwrap();
    table.index().build(&AtomicBool::new(false)).unwrap();
    let before = table.row_count();
    let mut undo = UndoStack::default();

    let t = Instant::now();
    let cells = parse_tsv(&clipboard);
    let edits = table.paste(AT, 0, &cells).unwrap();
    let focus = CellRef {
        row: table.row_id(AT),
        col: table.col_id(0),
    };
    undo.execute(Box::new(Batch::new("Paste", edits, focus)), &mut table);
    let mut screen = RowBlock::default();
    table.read_rows(AT..AT + 60, &mut screen);
    let took = t.elapsed();
    eprintln!(
        "pasted {} rows × {} cells ({} KB from LibreOffice) in {:.1} ms; table {} -> {} rows",
        cells.len(),
        cells[0].len(),
        clipboard.len() / 1024,
        took.as_secs_f64() * 1e3,
        before,
        table.row_count()
    );

    assert_eq!(cells.len() as u64, ROWS);
    assert_eq!(table.row_count(), AT + ROWS, "rows added past the end");
    let mut block = RowBlock::full_text();
    table.read_rows(AT..AT + ROWS, &mut block);
    for (r, row) in (AT..).zip(&cells) {
        for (c, value) in (0..).zip(row) {
            assert_eq!(block.cell(r, c), Some(value.as_str()), "row {r} col {c}");
        }
    }
    assert_eq!(screen.cell(AT, 1), Some(cells[0][1].as_str()));
    if !cfg!(debug_assertions) {
        assert!(took.as_secs_f64() < 1.0, "the paste took {took:?}");
    }

    undo.undo(&mut table).unwrap();
    assert_eq!(table.row_count(), before);
    assert_eq!(table.changes(), 0, "one undo takes the whole paste back");
}
