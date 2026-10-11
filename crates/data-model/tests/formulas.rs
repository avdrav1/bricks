//! FML-3: formula cells in a CSV show their results, follow edits, and save as raw text;
//! jobs see results through `Computed`; deep chains and whole-column sums compute.

use csv_engine::{Dialect, Source, SparseRowIndex};
use data_model::{
    Budget, CellRef, CsvTable, DelimiterChoice, Edit, Origin, RowBlock, SaveProgress, ShownSheet,
    SortOrder, TableSource, Workbook,
};
use formula::Value;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;
use std::time::Instant;

fn temp(name: &str, content: &[u8]) -> PathBuf {
    let path = std::env::temp_dir().join(format!("formulas-{}-{name}", std::process::id()));
    std::fs::write(&path, content).unwrap();
    path
}

fn table(path: &Path) -> CsvTable {
    let index = SparseRowIndex::new(Arc::new(Source::open(path).unwrap()), &Dialect::default());
    index.build(&AtomicBool::new(false)).unwrap();
    let mut t = CsvTable::new(index, Dialect::default());
    let found = t
        .scan_formulas(&AtomicBool::new(false), &AtomicU64::new(0))
        .unwrap();
    t.set_formula_cells(found);
    t
}

fn book(content: &str, name: &str) -> Workbook {
    let path = temp(name, content.as_bytes());
    let t = table(&path);
    std::fs::remove_file(path).unwrap();
    Workbook::single("Sheet1".into(), t, Origin::Csv)
}

/// Every row as the grid shows it.
fn shown(book: &mut Workbook) -> Vec<Vec<String>> {
    let mut shown = ShownSheet::new(book);
    let n = shown.row_count();
    let mut block = RowBlock::default();
    shown.read_rows(0..n, &mut block);
    (0..n)
        .map(|r| {
            (0..block.cells_in_row(r))
                .map(|c| block.cell(r, c).unwrap().to_string())
                .collect()
        })
        .collect()
}

fn at(t: &CsvTable, row: u64, col: u32) -> CellRef {
    CellRef {
        row: t.row_id(row),
        col: t.col_id(col),
    }
}

fn save(t: &CsvTable) -> Vec<u8> {
    let mut out = Vec::new();
    t.save_job()
        .unwrap()
        .write_to(&mut out, &SaveProgress::default(), &AtomicBool::new(false))
        .unwrap();
    out
}

const CSV: &str = "id,a,b\n1,2,=B2*2\n2,5,=SUM(B2:B3)\n";

#[test]
fn a_csv_formula_shows_its_result_and_saves_as_written() {
    let mut b = book(CSV, "basic.csv");
    let rows = shown(&mut b);
    assert_eq!(rows[1], ["1", "2", "4"]);
    assert_eq!(rows[2], ["2", "5", "7"]);

    let edit = Edit::set(at(b.active(), 1, 1), "3");
    b.active_mut().apply(edit);
    let rows = shown(&mut b);
    assert_eq!(rows[1][2], "6");
    assert_eq!(rows[2][2], "8");

    assert_eq!(
        String::from_utf8(save(b.active())).unwrap(),
        "id,a,b\n1,3,=B2*2\n2,5,=SUM(B2:B3)\n",
        "the file keeps the formulas' text"
    );
}

#[test]
fn header_row_doesnt_change_what_a_formula_means() {
    let mut b = book(CSV, "header.csv");
    b.active_mut().set_header(true);
    let rows = shown(&mut b);
    assert_eq!(rows[0], ["1", "2", "4"], "B2 is still the second line");
}

#[test]
fn errors_and_text_that_isnt_a_formula() {
    let mut b = book("x\n=1/0\n=A1+\n=\n'=1\n", "errors.csv");
    let rows = shown(&mut b);
    assert_eq!(rows[1], ["#DIV/0!"]);
    assert_eq!(
        rows[2],
        ["=A1+"],
        "a formula that doesn't parse shows as text"
    );
    assert_eq!(rows[3], ["="]);
    assert_eq!(rows[4], ["'=1"]);
}

#[test]
fn edited_formulas_evaluate_and_undo() {
    let mut b = book(CSV, "edit.csv");
    let cell = at(b.active(), 1, 0);
    let inverse = b.active_mut().apply(Edit::set(cell, "=C2+C3"));
    assert_eq!(shown(&mut b)[1][0], "11");
    assert!(b.active().formula_cells().unwrap().contains(&cell));
    b.active_mut().apply(inverse);
    assert_eq!(shown(&mut b)[1][0], "1");
}

#[test]
fn over_budget_cells_wait_for_a_job() {
    let mut csv = String::from("n,total\n");
    for i in 1..=20_000 {
        csv.push_str(&format!("{i},=SUM(A$2:A{})\n", i + 1));
    }
    let mut b = book(&csv, "budget.csv");
    let mut shown = ShownSheet::new(&mut b);
    let mut block = RowBlock::default();
    shown.read_rows(19_990..20_001, &mut block);
    let pending = shown.into_pending();
    assert!(!pending.is_empty());
    assert_eq!(block.cell(20_000, 1), Some(data_model::PENDING));

    let revision = b.revision();
    let mut snap = b.snapshot();
    let cells: Vec<_> = pending.iter().map(|&c| (b.active_id(), c)).collect();
    let values = snap
        .evaluate_all(&cells, Arc::new(AtomicBool::new(false)), &AtomicU64::new(0))
        .unwrap();
    b.store_values(revision, values);
    let mut shown = ShownSheet::new(&mut b);
    shown.read_rows(20_000..20_001, &mut block);
    assert_eq!(block.cell(20_000, 1), Some("200010000"));
}

#[test]
fn scan_finds_exactly_the_formula_cells() {
    let mut files: Vec<PathBuf> =
        std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/nasty"))
            .map(|d| d.map(|e| e.unwrap().path()).collect())
            .unwrap_or_default();
    let mut generated = String::from("a,b,c\n");
    for i in 0..20_000 {
        match i % 5 {
            0 => generated.push_str(&format!("{i},=A{}*2,x\n", i + 2)),
            1 => generated.push_str(&format!("{i},\"=A1,B1\",a=b\n")),
            2 => generated.push_str(&format!("{i},=,\"say \"\"=1\"\"\"\n")),
            3 => generated.push_str(&format!("=SUM(A1:A{i}),,=\"q\"\n")),
            _ => generated.push_str(&format!("{i},{i},{i}\n")),
        }
    }
    let path = temp("scan.csv", generated.as_bytes());
    files.push(path.clone());
    let mut checked = 0;
    for file in &files {
        let mut t = CsvTable::open(file, DelimiterChoice::Auto).unwrap();
        t.index().build(&AtomicBool::new(false)).unwrap();
        let found = t
            .scan_formulas(&AtomicBool::new(false), &AtomicU64::new(0))
            .unwrap();
        let mut naive = Vec::new();
        let mut block = RowBlock::full_text();
        t.read_all(0..t.total_rows(), &mut block);
        for r in 0..t.total_rows() {
            for c in 0..block.cells_in_row(r) {
                if data_model::is_formula(block.cell(r, c).unwrap()) {
                    naive.push((r, c));
                }
            }
        }
        assert_eq!(found.file_cells(), naive, "{}", file.display());
        checked += naive.len();
    }
    assert!(checked >= 12_000, "{checked}");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn jobs_sort_by_results() {
    let mut b = book("v,f\n3,=A2*-1\n1,=A3*-1\n2,=A4*-1\n", "sort.csv");
    b.active_mut().set_header(true);
    let sheet = b.active_id();
    let computed = b
        .snapshot()
        .computed_for(sheet, Arc::new(AtomicBool::new(false)), &AtomicU64::new(0))
        .unwrap();
    assert_eq!(computed.len(), 3);
    let mut snap = b.active().snapshot();
    snap.set_computed(Some(Arc::new(computed)));
    let edit = snap
        .sort_rows(
            1,
            SortOrder::Ascending,
            &AtomicBool::new(false),
            &AtomicU64::new(0),
        )
        .unwrap()
        .unwrap();
    b.active_mut().apply(edit);
    let order: Vec<String> = (1..4)
        .map(|r| b.active().cell_value(r - 1, 0).unwrap().into_owned())
        .collect();
    // Results as text: -1, -2, -3 (raw text would keep 3, 1, 2).
    assert_eq!(order, ["1", "2", "3"]);
}

#[test]
fn running_total_of_200k_rows() {
    const N: u64 = 200_000;
    let mut csv = String::from("1,=A1\n");
    for n in 2..=N {
        csv.push_str(&format!("{n},=B{}+A{n}\n", n - 1));
    }
    let mut b = book(&csv, "running.csv");
    let sheet = b.active_id();
    let last = at(b.active(), N - 1, 1);
    let mut budget = Budget::unlimited(Arc::new(AtomicBool::new(false)));
    let v = b.value_of(sheet, last, &mut budget).unwrap();
    assert_eq!(v, Value::Number((N * (N + 1) / 2) as f64));
    for r in 0..N {
        let cell = at(b.active(), r, 1);
        // A cached result costs nothing: no budget at all still answers.
        assert!(
            b.value_of(sheet, cell, &mut Budget::cells(0)).is_ok(),
            "row {r}"
        );
    }
}

#[test]
fn cycles_show_circ() {
    let mut b = book("=A2\n=A1\n=A3+1\n", "cycle.csv");
    let rows = shown(&mut b);
    assert_eq!(rows, [["#CIRC!"], ["#CIRC!"], ["#CIRC!"]]);
}

/// FML-3 acceptance: `=SUM(rows!D:D)` over the 1 GB file computes in the background in
/// under 5 s.
#[test]
#[ignore = "needs corpus/ (python3 scripts/gen_corpus.py); timing asserted in release only"]
fn sum_1gb() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/rows_1024mb.csv");
    let mut rows = CsvTable::open(&path, DelimiterChoice::Auto).unwrap();
    rows.index().build(&AtomicBool::new(false)).unwrap();

    let mut want = 0.0;
    let mut block = RowBlock::full_text();
    let total = rows.total_rows();
    let mut at_row = 1;
    while at_row < total {
        let end = (at_row + 65_536).min(total);
        rows.read_all(at_row..end, &mut block);
        for r in at_row..end {
            want += block.cell(r, 3).unwrap().parse::<f64>().unwrap();
        }
        at_row = end;
    }

    let mut sum = CsvTable::untitled();
    let edit = sum.insert_rows(0, 1).unwrap();
    sum.apply(edit);
    let cell = at(&sum, 0, 0);
    sum.apply(Edit::set(cell, "=SUM(rows!D:D)"));
    let mut b = Workbook::from_sheets(
        vec![
            ("rows".into(), rows, Origin::Csv, Default::default()),
            ("Sheet2".into(), sum, Origin::New, Default::default()),
        ],
        Vec::new(),
    );
    let sheet = b.sheets()[1].id;
    let mut snap = b.snapshot();
    let start = Instant::now();
    let values = snap
        .evaluate_all(
            &[(sheet, cell)],
            Arc::new(AtomicBool::new(false)),
            &AtomicU64::new(0),
        )
        .unwrap();
    let took = start.elapsed();
    println!("SUM(rows!D:D) over {} rows: {took:.2?}", total - 1);
    assert_eq!(values[0].1, Value::Number(want));
    b.store_values(b.revision(), values);
    if !cfg!(debug_assertions) {
        assert!(took.as_secs_f64() < 5.0, "{took:?}");
    }
}
