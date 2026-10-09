//! TYPE-1 acceptance: column types are inferred from a sample, and the raw value is never
//! altered: `00123` stays `00123` on screen and on disk, whatever its column's type.

use data_model::{CsvTable, DelimiterChoice, InferredType as T, SaveProgress};
use std::sync::atomic::AtomicBool;

const CSV: &str = "\
zip,code,id,amount,ratio,day,stamp,flag,note,mixed
02134,00123,1,12,0.5,2026-04-05,2026-04-05T10:30,true,NA,1
07030,000,2,-7,1e3,2026-02-28,2026-04-05 10:30:15,False,hello,two
10001,00999,+3,0,-2.25,2024-02-29,2026-04-05,YES,,3
";

fn open(name: &str, text: &str) -> (std::path::PathBuf, CsvTable) {
    let path = std::env::temp_dir().join(format!("type1-{}-{name}.csv", std::process::id()));
    std::fs::write(&path, text).unwrap();
    let mut t = CsvTable::open(&path, DelimiterChoice::Auto).unwrap();
    t.index().build(&AtomicBool::new(false)).unwrap();
    let types = t.infer_types(&AtomicBool::new(false)).unwrap();
    t.set_types(types);
    (path, t)
}

fn types(t: &CsvTable, cols: u32) -> Vec<Option<T>> {
    (0..cols).map(|c| t.column_type(c)).collect()
}

#[test]
fn types_come_from_the_values_and_leading_zeros_mean_text() {
    let (path, t) = open("kinds", CSV);
    assert_eq!(
        types(&t, 10),
        [
            Some(T::Text),     // zip: 02134 has a leading zero
            Some(T::Text),     // code: 00123, 000
            Some(T::Integer),  // id, with a + sign
            Some(T::Integer),  // amount: 12, -7, 0
            Some(T::Decimal),  // ratio: 0.5, 1e3, -2.25
            Some(T::Date),     // day, including a leap day
            Some(T::DateTime), // stamp: date-times with a plain date among them
            Some(T::Boolean),  // flag: true, False, YES
            Some(T::Text),     // note: NA and empty are ignored, "hello" is text
            Some(T::Text),     // mixed: one word among numbers
        ]
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn inference_never_alters_a_value_on_screen_or_on_disk() {
    let (path, t) = open("raw", CSV);
    let lines: Vec<Vec<&str>> = CSV
        .lines()
        .skip(1)
        .map(|l| l.split(',').collect())
        .collect();
    for (r, line) in lines.iter().enumerate() {
        for (c, raw) in line.iter().enumerate() {
            assert_eq!(
                t.cell_value(r as u64, c as u32).as_deref(),
                Some(*raw),
                "row {r} col {c}"
            );
        }
    }
    assert_eq!(t.cell_value(0, 1).as_deref(), Some("00123"));
    let mut saved = Vec::new();
    t.save_job()
        .unwrap()
        .write_to(
            &mut saved,
            &SaveProgress::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
    assert_eq!(String::from_utf8(saved).unwrap(), CSV);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn the_sample_reaches_the_last_row() {
    // 5,000 numbers, then a word in the very last row: the spread sample includes it.
    let mut text = String::from("n\n");
    for i in 0..5_000 {
        text.push_str(&format!("{i}\n"));
    }
    text.push_str("oops\n");
    let (path, t) = open("tail", &text);
    assert_eq!(t.column_type(0), Some(T::Text));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn a_header_flip_forgets_the_types() {
    let (path, mut t) = open("flip", CSV);
    t.set_header(false);
    assert_eq!(
        t.column_type(2),
        None,
        "inferred with other rows; stale now"
    );
    let types = t.infer_types(&AtomicBool::new(false)).unwrap();
    t.set_types(types);
    assert_eq!(t.column_type(2), Some(T::Text), "the title row is data now");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn a_cancelled_inference_returns_nothing() {
    let (path, mut t) = open("cancel", CSV);
    assert_eq!(t.infer_types(&AtomicBool::new(true)), None);
    std::fs::remove_file(path).unwrap();
}
