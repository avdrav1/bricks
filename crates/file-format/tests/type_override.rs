//! TYPE-2 acceptance: overriding a column's type changes sort and filter only. Values,
//! edits, and the saved file stay exactly as they were (invariant 1).

use data_model::{
    Compare, CsvTable, DelimiterChoice, Filter, InferredType, SaveProgress, SortOrder, TableSource,
    Test,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64};

const CSV: &str = "id,code,score\n1,00999,7\n2,10,10\n3,9,9\n4,abc,x\n5,,2\n";

struct Tmp(PathBuf);

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn tmp(name: &str) -> Tmp {
    Tmp(std::env::temp_dir().join(format!("type2-{}-{name}.csv", std::process::id())))
}

fn open(path: &Path) -> CsvTable {
    let mut t = CsvTable::open(path, DelimiterChoice::Auto).unwrap();
    t.index().build(&AtomicBool::new(false)).unwrap();
    let types = t.infer_types(&AtomicBool::new(false)).unwrap();
    t.set_types(types);
    t
}

fn column(t: &CsvTable, col: u32) -> Vec<String> {
    (0..t.row_count())
        .map(|r| t.cell_value(r, col).unwrap_or_default().into_owned())
        .collect()
}

/// Column `col` sorted ascending, read back; the table is left as it was.
fn sorted(t: &CsvTable, col: u32) -> Vec<String> {
    let mut s = t.snapshot();
    let edit = s
        .sort_rows(
            col,
            SortOrder::Ascending,
            &AtomicBool::new(false),
            &AtomicU64::new(0),
        )
        .unwrap();
    if let Some(edit) = edit {
        s.apply(edit); // `None`: already in order
    }
    column(&s, col)
}

/// Values of column `col` in the rows `test` passes.
fn filtered(t: &CsvTable, col: u32, test: Test) -> Vec<String> {
    let mut s = t.snapshot();
    let f = Filter {
        col: s.col_id(col),
        test,
    };
    let rows = s
        .filter_rows(&f, &AtomicBool::new(false), &AtomicU64::new(0))
        .unwrap();
    s.set_filter(f, rows);
    column(&s, col)
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
fn an_override_changes_sort_and_filter_only() {
    let src = tmp("src");
    std::fs::write(&src.0, CSV).unwrap();
    let mut t = open(&src.0);
    assert_eq!(
        t.column_type(1),
        Some(InferredType::Text),
        "00999 is a code"
    );
    let over_500 = || Test::Number(Compare::Gt, 500.0);
    assert_eq!(sorted(&t, 1), ["00999", "10", "9", "abc", ""]);
    assert_eq!(filtered(&t, 1, over_500()), Vec::<String>::new());

    t.set_type_override(1, Some(InferredType::Decimal));
    assert_eq!(t.column_type(1), Some(InferredType::Decimal));
    assert_eq!(t.inferred_type(1), Some(InferredType::Text));
    assert_eq!(sorted(&t, 1), ["9", "10", "00999", "abc", ""], "as numbers");
    assert_eq!(filtered(&t, 1, over_500()), ["00999"], "00999 is 999");

    // A number column read as text: sorted as text.
    t.set_type_override(2, Some(InferredType::Text));
    assert_eq!(sorted(&t, 2), ["10", "2", "7", "9", "x"]);

    // Nothing else changed: values, edits, the saved file.
    assert_eq!(column(&t, 1), ["00999", "10", "9", "abc", ""]);
    assert_eq!(t.changes(), 0);
    let out = tmp("out");
    save(&t, &out.0);
    assert_eq!(std::fs::read_to_string(&out.0).unwrap(), CSV);

    // Automatic again: the inferred type.
    t.set_type_override(1, None);
    assert_eq!(sorted(&t, 1), ["00999", "10", "9", "abc", ""]);
}

#[test]
fn an_override_outlives_header_flips_and_new_inference() {
    let src = tmp("flip");
    std::fs::write(&src.0, CSV).unwrap();
    let mut t = open(&src.0);
    t.set_type_override(1, Some(InferredType::Decimal));
    t.set_header(false);
    assert_eq!(
        t.column_type(1),
        Some(InferredType::Decimal),
        "flip keeps it"
    );
    assert_eq!(t.inferred_type(1), None, "the inferred types went");
    let types = t.infer_types(&AtomicBool::new(false)).unwrap();
    t.set_types(types);
    assert_eq!(
        t.inferred_type(1),
        Some(InferredType::Text),
        "the title is text now"
    );
    assert_eq!(
        t.column_type(1),
        Some(InferredType::Decimal),
        "still the user's"
    );
}
