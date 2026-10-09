//! TYPE-1 on the 1 GB corpus: the sample finds every column's type, codes with leading
//! zeros stay Text, and it takes milliseconds, so it never holds up the window.
//!
//! Needs the corpus (`python3 scripts/gen_corpus.py`):
//! `cargo test --release -p data-model --test infer_1gb -- --ignored --nocapture`

use data_model::{CsvTable, DelimiterChoice, InferredType as T};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

#[test]
#[ignore = "needs corpus/rows_1024mb.csv; run with --release --ignored"]
fn the_corpus_columns_get_their_types_from_a_sample() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/rows_1024mb.csv");
    assert!(
        path.exists(),
        "missing {}; run python3 scripts/gen_corpus.py",
        path.display()
    );
    let mut table = CsvTable::open(&path, DelimiterChoice::Auto).unwrap();
    table.index().build(&AtomicBool::new(false)).unwrap();

    let t = Instant::now();
    let types = table
        .snapshot()
        .infer_types(&AtomicBool::new(false))
        .unwrap();
    let took = t.elapsed();
    table.set_types(types);
    eprintln!("inferred 8 columns from 2,000 sampled rows in {took:.2?}");

    // id,name,city,revenue,score,date,active,code (code is `%05d`: 00997)
    let want = [
        T::Integer,
        T::Text,
        T::Text,
        T::Integer,
        T::Decimal,
        T::Date,
        T::Boolean,
        T::Text,
    ];
    let got: Vec<_> = (0..8).map(|c| table.column_type(c).unwrap()).collect();
    assert_eq!(got, want);
    assert_eq!(table.column_type(8), None, "no ninth column");
    if !cfg!(debug_assertions) {
        assert!(took.as_millis() < 200, "{took:?}");
    }
}
