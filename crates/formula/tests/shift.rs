//! FML-12: a pasted formula's relative references follow it; absolute ones stay.

use formula::shift_formula;

#[test]
fn pasted_formulas_shift_relative_references() {
    let cases: &[(&str, i64, i64, &str)] = &[
        ("=A2*2", 1, 0, "=A3*2"),
        ("=A2*2", 0, 1, "=B2*2"),
        ("=A2*2", 5, 2, "=C7*2"),
        ("=$A$2*2", 7, 3, "=$A$2*2"),
        ("=$A2+A$2", 2, 2, "=$A4+C$2"),
        ("=SUM(A1:B3)", 1, 1, "=SUM(B2:C4)"),
        ("=SUM($A$1:B3)", 1, 1, "=SUM($A$1:C4)"),
        ("=SUM(A:A)", 4, 1, "=SUM(B:B)"),
        ("=SUM(2:3)", 1, 9, "=SUM(3:4)"),
        ("='Q1 Sales'!B2+Data!C3", 1, 0, "='Q1 Sales'!B3+Data!C4"),
        ("=A3*2", -2, 0, "=A1*2"),
        ("=sum(b4)", 0, 0, "=SUM(B4)"),
    ];
    for &(raw, rows, cols, want) in cases {
        assert_eq!(
            shift_formula(raw, rows, cols).as_deref(),
            Some(want),
            "{raw} by ({rows}, {cols})"
        );
    }
}

#[test]
fn references_pushed_off_the_sheet_are_ref_errors() {
    assert_eq!(shift_formula("=A2*2", -3, 0).as_deref(), Some("=#REF!*2"));
    assert_eq!(shift_formula("=B2", 0, -2).as_deref(), Some("=#REF!"));
    assert_eq!(
        shift_formula("=$A$1+A2", -3, 0).as_deref(),
        Some("=$A$1+#REF!"),
        "only the reference that fell off"
    );
    assert_eq!(
        shift_formula("=SUM(A1:A5)", -1, 0).as_deref(),
        Some("=SUM(#REF!)")
    );
}

#[test]
fn text_that_isnt_a_formula_is_none() {
    for raw in ["A2*2", "", "=", "=A1+", "'=A1", "00123"] {
        assert_eq!(shift_formula(raw, 1, 0), None, "{raw}");
    }
}
