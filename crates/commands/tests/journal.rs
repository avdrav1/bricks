//! APP-7: the crash recovery journal. A table's unsaved edits, every kind, written to a
//! journal and read back into a freshly opened workbook, save to the same bytes; putting
//! them back is one undo step; and a journal that is damaged, cut short, or written for
//! another file is refused rather than half-applied.

use commands::{Batch, ClearCells, Reshape, SetCell, SheetEdit, UndoStack};
use csv_engine::{Charset, Encoding};
use data_model::{
    read_book_journal, read_journal_header, CellRef, CsvTable, DelimiterChoice, FileKind,
    JournalError, JournalHeader, Origin, SaveProgress, SortOrder, TableSource, Workbook,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64};

const CSV: &str = "id,city,score\n1,Denver,10\n2,boston,3\n3,DENVER,7\n4,Austin,1\n5,Boston,9\n";

struct Tmp(PathBuf);

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn file(name: &str, text: &str) -> Tmp {
    let p = std::env::temp_dir().join(format!("app7-{}-{name}.csv", std::process::id()));
    std::fs::write(&p, text).unwrap();
    Tmp(p)
}

fn open(path: &Path) -> CsvTable {
    let mut t = CsvTable::open(path, DelimiterChoice::Auto).unwrap();
    t.index().build(&AtomicBool::new(false)).unwrap();
    let types = t.infer_types(&AtomicBool::new(false)).unwrap();
    t.set_types(types);
    t
}

fn saved(t: &CsvTable) -> String {
    let mut out = Vec::new();
    t.save_job()
        .unwrap()
        .write_to(&mut out, &SaveProgress::default(), &AtomicBool::new(false))
        .unwrap();
    String::from_utf8(out).unwrap()
}

fn header(path: Option<&Path>) -> JournalHeader {
    JournalHeader {
        path: path.map(Path::to_owned),
        kind: FileKind::Csv,
        file_size: 123,
        file_mtime_ns: 456,
        delimiter: b',',
        has_header: true,
        encoding: Encoding {
            charset: Charset::Utf8,
            bom: false,
        },
        saved_ms: 1_760_000_000_000,
        changes: 9,
        rows: 6,
        pid: 4242,
    }
}

/// The one-sheet workbook a window holds for `t`, as the app builds it.
fn book(t: CsvTable, titled: bool) -> Workbook {
    let origin = if titled { Origin::Csv } else { Origin::New };
    Workbook::single("Sheet1".into(), t, origin)
}

fn journal(t: &CsvTable, h: &JournalHeader) -> Vec<u8> {
    let mut out = Vec::new();
    book(t.snapshot(), h.path.is_some())
        .write_journal(h, &mut out)
        .unwrap();
    out
}

fn at(t: &CsvTable, r: u64, c: u32) -> CellRef {
    CellRef {
        row: t.row_id(r),
        col: t.col_id(c),
    }
}

/// Every kind of edit, as the app makes them.
fn edit_everything(t: &mut CsvTable, undo: &mut UndoStack<CsvTable>) {
    undo.execute(Box::new(SetCell::new(at(t, 0, 1), "Paris, \"TX\"")), t);
    let e = t.insert_rows(2, 2).unwrap();
    undo.execute(Box::new(Reshape::new(e, at(t, 0, 0))), t);
    undo.execute(Box::new(SetCell::new(at(t, 2, 0), "new")), t);
    let e = t.delete_rows(4, 1).unwrap();
    undo.execute(Box::new(Reshape::new(e, at(t, 0, 0))), t);
    let e = t.insert_cols(1, 1).unwrap();
    undo.execute(Box::new(Reshape::new(e, at(t, 0, 0))), t);
    undo.execute(Box::new(SetCell::new(at(t, 1, 1), "added col")), t);
    let e = t.delete_cols(3, 1).unwrap();
    undo.execute(Box::new(Reshape::new(e, at(t, 0, 0))), t);
    let e = t.clear_cells(5..6, 0..2).unwrap();
    undo.execute(Box::new(ClearCells::new(e, at(t, 0, 0))), t);
    let e = t
        .snapshot()
        .sort_rows(
            0,
            SortOrder::Descending,
            &AtomicBool::new(false),
            &AtomicU64::new(0),
        )
        .unwrap()
        .unwrap();
    undo.execute(Box::new(Batch::new("Sort", vec![e], at(t, 0, 0))), t);
}

/// Restore `bytes` onto `book` as the app does: one undoable step.
fn restore(
    book: &mut Workbook,
    undo: &mut UndoStack<Workbook>,
    bytes: &[u8],
) -> Result<(), JournalError> {
    let (_, state) = read_book_journal(bytes)?;
    let plan = book.restore_plan(state)?;
    let show = Some(book.active_id());
    undo.execute(
        Box::new(SheetEdit::new("Restore unsaved edits", plan, show)),
        book,
    );
    Ok(())
}

/// APP-7 acceptance (data side): what a crashed session journaled comes back exactly.
#[test]
fn every_kind_of_edit_comes_back_from_the_journal() {
    let src = file("all", CSV);
    let mut before = open(&src.0);
    let mut undo = UndoStack::default();
    edit_everything(&mut before, &mut undo);
    let want = saved(&before);
    assert_ne!(want, CSV);
    let h = header(Some(&src.0));
    let bytes = journal(&before, &h);
    assert_eq!(read_journal_header(&bytes[..]).unwrap(), h, "header alone");

    // "Relaunch": a new workbook on the same file.
    let mut after = book(open(&src.0), true);
    let mut undo = UndoStack::default();
    restore(&mut after, &mut undo, &bytes).unwrap();
    assert_eq!(saved(after.active()), want, "saves the same bytes");
    assert_eq!(after.active().changes(), before.changes());
    assert_eq!(undo.len(), 1, "one step");
    undo.undo(&mut after).unwrap();
    assert_eq!(saved(after.active()), CSV, "undo takes it all back");
    undo.redo(&mut after).unwrap();
    assert_eq!(saved(after.active()), want);

    // New edits after a restore get fresh ids: an inserted row isn't handed out twice.
    let t = after.active_mut();
    let mut undo = UndoStack::default();
    let e = t.insert_rows(0, 1).unwrap();
    undo.execute(Box::new(Reshape::new(e, at(t, 0, 0))), t);
    undo.execute(Box::new(SetCell::new(at(t, 0, 0), "fresh")), t);
    let text = saved(t);
    assert!(text.contains("fresh") && text.contains("new"), "{text}");
}

#[test]
fn journals_are_deterministic_and_unsorted_orders_round_trip() {
    let src = file("runs", CSV);
    let mut t = open(&src.0);
    let mut undo = UndoStack::default();
    let e = t.insert_rows(1, 1).unwrap();
    undo.execute(Box::new(Reshape::new(e, at(&t, 0, 0))), &mut t);
    let h = header(Some(&src.0));
    assert_eq!(journal(&t, &h), journal(&t, &h), "same state, same bytes");
    let mut back = book(open(&src.0), true);
    restore(&mut back, &mut UndoStack::default(), &journal(&t, &h)).unwrap();
    assert_eq!(saved(back.active()), saved(&t));
}

#[test]
fn damaged_or_foreign_journals_are_refused() {
    let src = file("bad", CSV);
    let mut t = open(&src.0);
    let mut undo = UndoStack::default();
    edit_everything(&mut t, &mut undo);
    let bytes = journal(&t, &header(Some(&src.0)));
    let mut fresh = book(open(&src.0), true);
    let mut undo = UndoStack::default();

    let mut flipped = bytes.clone();
    let mid = flipped.len() / 2;
    flipped[mid] ^= 0x40;
    assert!(matches!(
        restore(&mut fresh, &mut undo, &flipped),
        Err(JournalError::Corrupt(_))
    ));
    let cut = &bytes[..bytes.len() - 3];
    assert!(matches!(
        restore(&mut fresh, &mut undo, cut),
        Err(JournalError::Corrupt(_))
    ));
    assert!(matches!(
        read_book_journal(&b"not a journal at all"[..]),
        Err(JournalError::Corrupt(_))
    ));
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(matches!(
        read_book_journal(&longer[..]),
        Err(JournalError::Corrupt(_))
    ));

    // Written for a 5-row file, applied to a 2-row one: the rows aren't there.
    let small = file("small", "id,city,score\n1,Denver,10\n2,boston,3\n");
    let mut other = book(open(&small.0), true);
    assert!(matches!(
        restore(&mut other, &mut undo, &bytes),
        Err(JournalError::Mismatch(_))
    ));
    assert_eq!(
        saved(other.active()),
        "id,city,score\n1,Denver,10\n2,boston,3\n",
        "untouched"
    );
    assert_eq!(saved(fresh.active()), CSV, "nothing half-applied");
    assert!(undo.is_empty(), "no step for a refused journal");
}

#[test]
fn an_untitled_table_comes_back_too() {
    let mut t = CsvTable::untitled();
    let mut undo = UndoStack::default();
    let e = t.insert_rows(0, 3).unwrap();
    undo.execute(Box::new(Reshape::new(e, at(&t, 0, 0))), &mut t);
    for (r, v) in ["a", "b", "c"].iter().enumerate() {
        undo.execute(Box::new(SetCell::new(at(&t, r as u64, 0), *v)), &mut t);
        undo.execute(Box::new(SetCell::new(at(&t, r as u64, 1), "x")), &mut t);
    }
    let bytes = journal(&t, &header(None));
    assert_eq!(read_journal_header(&bytes[..]).unwrap().path, None);
    let mut back = Workbook::default();
    restore(&mut back, &mut UndoStack::default(), &bytes).unwrap();
    assert_eq!(back.sheets().len(), 1);
    assert_eq!(back.sheets()[0].name, "Sheet1");
    assert_eq!(back.sheets()[0].table.row_count(), 3);
    assert_eq!(saved(&back.sheets()[0].table), saved(&t));
}

/// APP-7 at scale: the biggest journal, a sorted 1 GB table (its order is 19.1M rows),
/// written (job pool), read back (job pool), then checked and applied (UI thread).
/// `cargo test --release -p commands --test journal -- --ignored --nocapture`
#[test]
#[ignore = "needs corpus/rows_1024mb.csv; run with --release --ignored"]
fn a_sorted_1gb_table_journals_and_restores() {
    use std::time::Instant;
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/rows_1024mb.csv");
    let mut t = open(&path);
    let mut undo = UndoStack::default();
    let e = t
        .snapshot()
        .sort_rows(
            4,
            SortOrder::Ascending,
            &AtomicBool::new(false),
            &AtomicU64::new(0),
        )
        .unwrap()
        .unwrap();
    undo.execute(Box::new(Batch::new("Sort", vec![e], at(&t, 0, 0))), &mut t);
    undo.execute(Box::new(SetCell::new(at(&t, 7, 1), "edited")), &mut t);
    let file = std::env::temp_dir().join(format!("app7-1gb-{}.journal", std::process::id()));
    let start = Instant::now();
    book(t.snapshot(), true)
        .write_journal(&header(Some(&path)), std::fs::File::create(&file).unwrap())
        .unwrap();
    let wrote = start.elapsed();
    let bytes = std::fs::metadata(&file).unwrap().len();
    let start = Instant::now();
    let (_, state) = read_book_journal(std::fs::File::open(&file).unwrap()).unwrap();
    let read = start.elapsed();
    let mut back = book(open(&path), true);
    let start = Instant::now();
    for edit in back.restore_plan(state).unwrap() {
        back.apply(edit);
    }
    let applied = start.elapsed();
    std::fs::remove_file(&file).unwrap();
    let back = back.active();
    assert_eq!(back.cell_value(7, 1).as_deref(), Some("edited"));
    assert_eq!(back.row_id(12345), t.row_id(12345), "same order");
    eprintln!(
        "journal {:.1} MB: write {:.0} ms, read {:.0} ms, check and apply {:.0} ms (UI thread)",
        bytes as f64 / 1e6,
        wrote.as_secs_f64() * 1e3,
        read.as_secs_f64() * 1e3,
        applied.as_secs_f64() * 1e3
    );
}
