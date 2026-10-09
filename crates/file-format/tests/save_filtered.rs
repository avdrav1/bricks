//! FILT-3: saving with an active filter keeps the hidden rows (invariant 7). The filter is a
//! view; the save, atomic as ever (SAVE-1), writes every row, edits included, in the
//! table's order.
//!
//! The 1 GB case copies `corpus/rows_1024mb.csv` next to itself (a real disk, so the
//! fsync is real) and removes it afterwards:
//! `cargo test --release -p file-format --test save_filtered -- --ignored --nocapture`

use csv_engine::RowIndex;
use data_model::{
    CellRef, Compare, CsvTable, DelimiterChoice, Edit, Filter, SaveProgress, SortOrder,
    TableSource, Test,
};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64};

fn open(path: &Path) -> CsvTable {
    let mut t = CsvTable::open(path, DelimiterChoice::Auto).unwrap();
    t.index().build(&AtomicBool::new(false)).unwrap();
    let types = t.infer_types(&AtomicBool::new(false)).unwrap();
    t.set_types(types);
    t
}

fn filter(t: &mut CsvTable, col: u32, test: Test) {
    let f = Filter {
        col: t.col_id(col),
        test,
    };
    let rows = t
        .snapshot()
        .filter_rows(&f, &AtomicBool::new(false), &AtomicU64::new(0))
        .unwrap();
    t.set_filter(f, rows);
}

fn save(t: &CsvTable, path: &Path) {
    let job = t.save_job().unwrap();
    file_format::save_csv(
        path,
        &job,
        &SaveProgress::default(),
        &AtomicBool::new(false),
    )
    .unwrap();
}

#[test]
fn a_filtered_table_saves_every_row() {
    let path = std::env::temp_dir().join(format!("filt3-{}.csv", std::process::id()));
    std::fs::write(
        &path,
        "id,city,score\n1,Denver,10\n2,Boston,3\n3,Denver,7\n4,Austin,1\n5,Boston,9\n",
    )
    .unwrap();
    let mut t = open(&path);
    filter(&mut t, 1, Test::Equals("denver".into()));
    assert_eq!((t.row_count(), t.unfiltered_row_count()), (2, 5));
    // An edit to a row shown, and a sort of the whole table while filtered.
    t.apply(Edit::set(
        CellRef {
            row: t.row_id(1),
            col: t.col_id(2),
        },
        "70",
    ));
    let sort = t
        .snapshot()
        .sort_rows(
            2,
            SortOrder::Descending,
            &AtomicBool::new(false),
            &AtomicU64::new(0),
        )
        .unwrap()
        .unwrap();
    t.apply(sort);
    save(&t, &path);

    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "id,city,score\n3,Denver,70\n1,Denver,10\n5,Boston,9\n2,Boston,3\n4,Austin,1\n",
        "every row, hidden ones too, in the sorted order, with the edit"
    );
    let reopened = open(&path);
    assert_eq!(reopened.row_count(), 5, "on-disk row count unchanged");
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "needs corpus/ and 1 GB of free disk; run with --ignored"]
fn a_filtered_1gb_table_saves_every_row() {
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/rows_1024mb.csv");
    let path = corpus.with_file_name(format!(".save-filtered-{}.csv", std::process::id()));
    std::fs::copy(&corpus, &path).unwrap();
    let mut t = open(&path);
    let all = t.row_count();
    filter(&mut t, 2, Test::Equals("Portland".into()));
    filter(&mut t, 3, Test::Number(Compare::Gt, 50_000.0));
    let shown = t.row_count();
    assert!(shown < all / 10, "{shown} of {all} shown");
    // Edit the last row shown: a row deep in the file.
    let row = t.row_id(shown - 1);
    t.apply(Edit::set(
        CellRef {
            row,
            col: t.col_id(1),
        },
        "Edited",
    ));
    save(&t, &path);
    drop(t);

    let reopened = open(&path);
    assert_eq!(reopened.row_count(), all, "on-disk row count unchanged");
    assert_eq!(reopened.index().row_count(), all + 1, "header included");
    // Line for line the corpus, but for the one edited row.
    let lines = |p: &Path| BufReader::new(std::fs::File::open(p).unwrap()).lines();
    let mut differ = Vec::new();
    for (n, (a, b)) in lines(&corpus).zip(lines(&path)).enumerate() {
        let (a, b) = (a.unwrap(), b.unwrap());
        if a != b {
            differ.push((n, b));
        }
    }
    std::fs::remove_file(&path).unwrap();
    assert_eq!(differ.len(), 1, "{differ:?}");
    let (line, text) = &differ[0];
    assert_eq!(*line as u64, row.0, "the edited row, in place");
    assert!(text.split(',').nth(1) == Some("Edited"), "{text}");
    eprintln!("saved {all} rows with {shown} shown; line {line} edited");
}
