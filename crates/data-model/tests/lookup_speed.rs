//! FML-7: exact lookups use the workbook's cached index, which follows edits; 10,000 exact
//! VLOOKUPs into 1,000,000 rows take under 2 s and one takes under 1 s.

use csv_engine::{Dialect, Source, SparseRowIndex};
use data_model::{Budget, CellRef, CsvTable, Edit, Origin, SheetId, Workbook};
use formula::Value;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;
use std::time::Instant;

fn table(name: &str, content: &[u8]) -> CsvTable {
    let path: PathBuf = std::env::temp_dir().join(format!("lookup-{}-{name}", std::process::id()));
    std::fs::write(&path, content).unwrap();
    let index = SparseRowIndex::new(Arc::new(Source::open(&path).unwrap()), &Dialect::default());
    index.build(&AtomicBool::new(false)).unwrap();
    let mut t = CsvTable::new(index, Dialect::default());
    let found = t
        .scan_formulas(&AtomicBool::new(false), &AtomicU64::new(0))
        .unwrap();
    t.set_formula_cells(found);
    std::fs::remove_file(path).unwrap();
    t
}

/// A workbook of sheet `data` and sheet `f`, whose column A holds one formula per row.
fn book(name: &str, data: &[u8], formulas: &[String]) -> Workbook {
    let mut csv = String::new();
    for f in formulas {
        csv.push('"');
        csv.push_str(&f.replace('"', "\"\""));
        csv.push_str("\"\n");
    }
    Workbook::from_sheets(
        vec![
            (
                "data".into(),
                table(&format!("{name}-data.csv"), data),
                Origin::Csv,
                Default::default(),
            ),
            (
                "f".into(),
                table(&format!("{name}-f.csv"), csv.as_bytes()),
                Origin::Csv,
                Default::default(),
            ),
        ],
        Vec::new(),
    )
}

fn sheet(b: &Workbook, i: usize) -> SheetId {
    b.sheets()[i].id
}

fn at(b: &Workbook, sheet: SheetId, row: u64, col: u32) -> CellRef {
    let t = &b.sheet(sheet).unwrap().table;
    CellRef {
        row: t.row_id(row),
        col: t.col_id(col),
    }
}

fn results(b: &mut Workbook) -> Vec<Value> {
    let f = sheet(b, 1);
    let n = b.sheet(f).unwrap().table.total_rows();
    let mut budget = Budget::unlimited(Arc::new(AtomicBool::new(false)));
    (0..n)
        .map(|r| {
            let cell = at(b, f, r, 0);
            b.value_of(f, cell, &mut budget).unwrap()
        })
        .collect()
}

#[test]
fn exact_lookups_find_the_first_match_and_follow_edits() {
    let formulas = [
        "=VLOOKUP(2,data!A:B,2,FALSE)",
        "=MATCH(\"X\",data!A:A,0)",
        "=XLOOKUP(3,data!A:A,data!B:B,\"none\")",
        "=XLOOKUP(2,data!A:A,data!B:B,,0,-1)",
        "=HLOOKUP(\"k\",data!A1:B2,2,FALSE)",
    ]
    .map(String::from);
    let mut b = book("edits", b"k,v\n1,a\n2,b\n2,dup\nx,c\n", &formulas);
    let text = |s: &str| Value::text(s);
    assert_eq!(
        results(&mut b),
        [
            text("b"),
            Value::Number(5.0),
            text("none"),
            text("dup"),
            Value::Number(1.0)
        ]
    );

    // Row 3's key 2 becomes 3: the cached index must not answer with the old key.
    let data = sheet(&b, 0);
    let cell = at(&b, data, 2, 0);
    b.sheet_mut(data).unwrap().table.apply(Edit::set(cell, "3"));
    assert_eq!(
        results(&mut b),
        [
            text("dup"),
            Value::Number(5.0),
            text("b"),
            text("dup"),
            Value::Number(1.0)
        ]
    );
}

const ROWS: u64 = 1_000_000;
const LOOKUPS: u64 = 10_000;

/// Row n's key: a permutation of 1..=ROWS, so keys aren't in order.
fn key(n: u64) -> u64 {
    n * 7_919 % ROWS + 1
}

/// FML-7 acceptance: exact VLOOKUPs into 1,000,000 rows.
#[test]
#[ignore = "timing asserted in release only: cargo test --release -p data-model --test lookup_speed -- --ignored --nocapture"]
fn lookup_speed() {
    let mut data = String::with_capacity(20 * ROWS as usize);
    for n in 0..ROWS {
        let k = key(n);
        data.push_str(&format!("{k},v{k}\n"));
    }
    let wanted: Vec<u64> = (0..LOOKUPS).map(|i| key(i * 97 % ROWS)).collect();
    let formulas: Vec<String> = wanted
        .iter()
        .map(|k| format!("=VLOOKUP({k},data!A:B,2,FALSE)"))
        .chain([format!("=VLOOKUP({},data!A:B,2,FALSE)", key(ROWS - 1))])
        .collect();
    let b = book("speed", data.as_bytes(), &formulas);
    drop(data);
    let f = sheet(&b, 1);
    let cancel = Arc::new(AtomicBool::new(false));

    // One lookup of the last row's key, on a fresh calculation: builds the index.
    let last = [(f, at(&b, f, LOOKUPS, 0))];
    let mut snap = b.snapshot();
    let start = Instant::now();
    let one = snap
        .evaluate_all(&last, cancel.clone(), &AtomicU64::new(0))
        .unwrap();
    let one_took = start.elapsed();
    assert_eq!(one[0].1, Value::text(format!("v{}", key(ROWS - 1))));

    let cells: Vec<(SheetId, CellRef)> = (0..LOOKUPS).map(|r| (f, at(&b, f, r, 0))).collect();
    let mut snap = b.snapshot();
    let start = Instant::now();
    let all = snap
        .evaluate_all(&cells, cancel, &AtomicU64::new(0))
        .unwrap();
    let all_took = start.elapsed();
    for ((_, v), k) in all.iter().zip(&wanted) {
        assert_eq!(*v, Value::text(format!("v{k}")));
    }

    println!("1 VLOOKUP into {ROWS} rows: {one_took:.2?}");
    println!("{LOOKUPS} VLOOKUPs into {ROWS} rows: {all_took:.2?}");
    if !cfg!(debug_assertions) {
        assert!(one_took.as_secs_f64() < 1.0, "{one_took:?}");
        assert!(all_took.as_secs_f64() < 2.0, "{all_took:?}");
    }
}
