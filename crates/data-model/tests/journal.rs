//! XLSX-1: the crash recovery journal holds the whole workbook. Sheets of every origin,
//! with edits, renamed, reordered, added and removed, come back from the journal onto a
//! freshly opened workbook and save the same bytes; journals of v0.1.x still restore; and
//! damaged journals of either version are refused.

use csv_engine::{Charset, Encoding};
use data_model::{
    read_book_journal, read_journal_header, BookEdit, BookState, CellRef, CsvTable,
    DelimiterChoice, Edit, FileKind, JournalError, JournalHeader, Origin, SaveProgress, SheetId,
    SortOrder, Workbook,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64};

const DATA: &str = "id,city,score\n1,Denver,10\n2,boston,3\n3,DENVER,7\n4,Austin,1\n5,Boston,9\n";
const IMPORTED: &str = "a,b\n1,x\n2,y\n3,z\n";
const GONE: &str = "only\n1\n";

struct Tmp(PathBuf);

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn file(name: &str, text: &str) -> Tmp {
    let p = std::env::temp_dir().join(format!("journal-{}-{name}.csv", std::process::id()));
    std::fs::write(&p, text).unwrap();
    Tmp(p)
}

fn open(path: &Path) -> CsvTable {
    let t = CsvTable::open(path, DelimiterChoice::Auto).unwrap();
    t.index().build(&AtomicBool::new(false)).unwrap();
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

fn header(path: Option<&Path>, kind: FileKind) -> JournalHeader {
    JournalHeader {
        path: path.map(Path::to_owned),
        kind,
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

fn journal(book: &Workbook, h: &JournalHeader) -> Vec<u8> {
    let mut out = Vec::new();
    book.snapshot().write_journal(h, &mut out).unwrap();
    out
}

fn at(t: &CsvTable, r: u64, c: u32) -> CellRef {
    CellRef {
        row: t.row_id(r),
        col: t.col_id(c),
    }
}

/// The workbook as freshly opened: the CSV-origin "data", .xlsx sheets 0 and 1.
fn fresh(data: &Path, imported: &Path, gone: &Path) -> Workbook {
    Workbook::from_sheets(
        vec![
            ("data".into(), open(data), Origin::Csv, HashMap::new()),
            (
                "Imported".into(),
                open(imported),
                Origin::Xlsx(0),
                HashMap::new(),
            ),
            ("Gone".into(), open(gone), Origin::Xlsx(1), HashMap::new()),
        ],
        Vec::new(),
    )
}

fn sheet_mut(book: &mut Workbook, id: u32) -> &mut CsvTable {
    &mut book.sheet_mut(SheetId(id)).unwrap().table
}

/// What the crashed session did: edits of every kind on each sheet, a sheet added, one
/// removed, one renamed, and the order changed.
fn crashed_session(book: &mut Workbook) {
    // "data": cell edits, an inserted row, a sort.
    let t = sheet_mut(book, 0);
    t.apply(Edit::set(at(t, 0, 1), "Paris, \"TX\""));
    let e = t.insert_rows(2, 1).unwrap();
    t.apply(e);
    t.apply(Edit::set(at(t, 2, 0), "new"));
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
    t.apply(e);
    // "Imported": a clear, a deleted row, an inserted column.
    let t = sheet_mut(book, 1);
    let e = t.clear_cells(0..1, 0..1).unwrap();
    t.apply(e);
    let e = t.delete_rows(2, 1).unwrap();
    t.apply(e);
    let e = t.insert_cols(1, 1).unwrap();
    t.apply(e);
    t.apply(Edit::set(at(t, 1, 1), "mid"));
    // A sheet added in the session, with rows typed in.
    let mut added = book.new_sheet("Notes".into(), CsvTable::untitled(), Origin::New);
    let e = added.table.insert_rows(0, 2).unwrap();
    added.table.apply(e);
    for r in 0..2 {
        let cell = at(&added.table, r, 0);
        added.table.apply(Edit::set(cell, format!("note {r}")));
    }
    let added_id = added.id;
    book.apply(BookEdit::Insert {
        at: 3,
        sheet: Box::new(added),
    });
    book.apply(BookEdit::Remove { at: 2 }); // "Gone"
    book.apply(BookEdit::Rename {
        id: SheetId(1),
        name: "Q1 Sales".into(),
    });
    book.apply(BookEdit::Move { from: 2, to: 0 }); // Notes first
    book.set_active(added_id);
}

fn layout(book: &Workbook) -> Vec<(String, Origin, String)> {
    book.sheets()
        .iter()
        .map(|s| (s.name.clone(), s.origin, saved(&s.table)))
        .collect()
}

/// XLSX-1 acceptance (data side): edits on several sheets come back after a crash.
#[test]
fn a_workbook_comes_back_from_the_journal() {
    let (data, imported, gone) = (
        file("data", DATA),
        file("imported", IMPORTED),
        file("gone", GONE),
    );
    let mut before = fresh(&data.0, &imported.0, &gone.0);
    crashed_session(&mut before);
    let want = layout(&before);
    assert_eq!(
        want.iter().map(|(n, ..)| n.as_str()).collect::<Vec<_>>(),
        ["Notes", "data", "Q1 Sales"]
    );
    assert_ne!(want[1].2, DATA);
    assert_ne!(want[2].2, IMPORTED);

    let h = header(Some(&data.0), FileKind::Xlsx);
    let bytes = journal(&before, &h);
    assert_eq!(bytes, journal(&before, &h), "same workbook, same bytes");
    assert_eq!(read_journal_header(&bytes[..]).unwrap(), h, "header alone");
    let (back_h, state) = read_book_journal(&bytes[..]).unwrap();
    assert_eq!(back_h, h);
    assert_eq!(state.active, 0, "Notes was active");
    assert_eq!(
        state
            .sheets
            .iter()
            .map(|s| (s.name.as_str(), s.origin, s.has_header))
            .collect::<Vec<_>>(),
        [
            ("Notes", Origin::New, false),
            ("data", Origin::Csv, true),
            ("Q1 Sales", Origin::Xlsx(0), true)
        ]
    );

    // "Relaunch": the same files opened again.
    let mut after = fresh(&data.0, &imported.0, &gone.0);
    let plan = after.restore_plan(state.clone()).unwrap();
    for edit in plan {
        after.apply(edit);
    }
    assert_eq!(layout(&after), want, "names, order, and saved bytes");
    let active = after.sheets()[state.active as usize].id;
    after.set_active(active);
    assert_eq!(after.active_sheet().name, "Notes");

    // Ids handed out after the restore are fresh.
    let ids: Vec<SheetId> = after.sheets().iter().map(|s| s.id).collect();
    let next = after.new_sheet("x".into(), CsvTable::untitled(), Origin::New);
    assert!(!ids.contains(&next.id));
}

#[test]
fn an_unchanged_workbook_needs_no_steps() {
    let (data, imported, gone) = (
        file("same-data", DATA),
        file("same-imported", IMPORTED),
        file("same-gone", GONE),
    );
    let book = fresh(&data.0, &imported.0, &gone.0);
    let bytes = journal(&book, &header(Some(&data.0), FileKind::Xlsx));
    let (_, state) = read_book_journal(&bytes[..]).unwrap();
    let plan = fresh(&data.0, &imported.0, &gone.0)
        .restore_plan(state)
        .unwrap();
    assert!(plan.is_empty());
}

#[test]
fn a_csv_sheet_deleted_in_the_session_comes_back_deleted() {
    let data = file("deleted", DATA);
    let mut before = Workbook::single("data".into(), open(&data.0), Origin::Csv);
    let mut added = before.new_sheet("Sheet2".into(), CsvTable::untitled(), Origin::New);
    let e = added.table.insert_rows(0, 1).unwrap();
    added.table.apply(e);
    let cell = at(&added.table, 0, 0);
    added.table.apply(Edit::set(cell, "kept"));
    before.apply(BookEdit::Insert {
        at: 1,
        sheet: Box::new(added),
    });
    before.apply(BookEdit::Remove { at: 0 });
    let (_, state) =
        read_book_journal(&journal(&before, &header(Some(&data.0), FileKind::Csv))[..]).unwrap();
    let mut after = Workbook::single("data".into(), open(&data.0), Origin::Csv);
    for edit in after.restore_plan(state).unwrap() {
        after.apply(edit);
    }
    assert_eq!(layout(&after), layout(&before));
    assert_eq!(after.sheets().len(), 1);
    assert_eq!(saved(&after.sheets()[0].table), "kept\n");
}

#[test]
fn journals_that_dont_fit_the_file_are_refused() {
    let (data, imported, gone) = (
        file("fit-data", DATA),
        file("fit-imported", IMPORTED),
        file("fit-gone", GONE),
    );
    let mut before = fresh(&data.0, &imported.0, &gone.0);
    crashed_session(&mut before);
    let (_, state) =
        read_book_journal(&journal(&before, &header(Some(&data.0), FileKind::Xlsx))[..]).unwrap();

    // The .xlsx sheet the journal names isn't in the file any more.
    let only_csv = Workbook::single("data".into(), open(&data.0), Origin::Csv);
    assert!(matches!(
        only_csv.restore_plan(state.clone()),
        Err(JournalError::Mismatch(
            "a sheet in the journal isn't in the file"
        ))
    ));
    // The file got shorter: the edits name rows it lacks.
    let small = file("fit-small", "id,city,score\n1,Denver,10\n");
    let shorter = Workbook::from_sheets(
        vec![
            ("data".into(), open(&small.0), Origin::Csv, HashMap::new()),
            (
                "Imported".into(),
                open(&imported.0),
                Origin::Xlsx(0),
                HashMap::new(),
            ),
        ],
        Vec::new(),
    );
    assert!(matches!(
        shorter.restore_plan(state),
        Err(JournalError::Mismatch(_))
    ));
}

#[test]
fn damaged_journals_of_either_version_are_refused() {
    let (data, imported, gone) = (
        file("bad-data", DATA),
        file("bad-imported", IMPORTED),
        file("bad-gone", GONE),
    );
    let mut book = fresh(&data.0, &imported.0, &gone.0);
    crashed_session(&mut book);
    let v2 = journal(&book, &header(Some(&data.0), FileKind::Xlsx));
    let v1 = std::fs::read(fixture("v1.journal")).unwrap();
    let corrupt = |bytes: &[u8]| matches!(read_book_journal(bytes), Err(JournalError::Corrupt(_)));
    for bytes in [&v2, &v1] {
        assert!(!corrupt(bytes));
        let mut flipped = bytes.clone();
        let mid = flipped.len() / 2;
        flipped[mid] ^= 0x40;
        assert!(corrupt(&flipped), "a flipped bit");
        assert!(corrupt(&bytes[..bytes.len() - 3]), "cut short");
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(corrupt(&longer), "trailing bytes");
    }
    assert!(corrupt(b"not a journal at all"));
    assert!(matches!(
        read_journal_header(&b"BRICKSJ3........"[..]),
        Err(JournalError::Corrupt("not a journal"))
    ));
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

/// A journal left by v0.1.x (written by its `CsvTable::write_journal` for
/// `tests/data/v1.csv`: B2 edited, a row inserted and typed in, the last row deleted).
#[test]
fn a_v1_journal_still_restores() {
    let bytes = std::fs::read(fixture("v1.journal")).unwrap();
    let h = read_journal_header(&bytes[..]).unwrap();
    assert_eq!(h.kind, FileKind::Csv);
    assert_eq!(h.path.as_deref(), Some(Path::new("/home/user/data/v1.csv")));
    assert_eq!((h.pid, h.file_size), (777, 37));
    let (back_h, state) = read_book_journal(&bytes[..]).unwrap();
    assert_eq!(back_h, h);
    let BookState { active, sheets } = state.clone();
    assert_eq!(active, 0);
    assert_eq!(sheets.len(), 1);
    assert_eq!(
        (sheets[0].id, sheets[0].name.as_str(), sheets[0].origin),
        (SheetId(0), "v1", Origin::Csv)
    );
    assert!(sheets[0].has_header);

    let mut book = Workbook::single("v1".into(), open(&fixture("v1.csv")), Origin::Csv);
    for edit in book.restore_plan(state).unwrap() {
        book.apply(edit);
    }
    assert_eq!(
        saved(book.active()),
        "id,name,qty\n1,green apple,3\nnew,,\n2,pear,5\n"
    );
}
