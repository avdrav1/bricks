//! FML-1: every grammar case round-trips parse then render, and a bad formula reports the
//! character where it fails.

use formula::{
    adjust, column_index, column_name, format_reference, parse, parse_reference, render,
    render_xlsx, Area, Axis, CellAddr, Change, Expr,
};

/// `(input, canonical)`: `render(parse(input)) == canonical`.
const CASES: &[(&str, &str)] = &[
    // Numbers.
    ("1", "1"),
    ("0", "0"),
    ("1.5", "1.5"),
    ("1.", "1"),
    (".5", "0.5"),
    ("1e3", "1000"),
    ("1.5E-3", "0.0015"),
    ("2.5e+2", "250"),
    ("1e20", "1E+20"),
    ("1e-9", "1E-9"),
    ("0.1", "0.1"),
    ("123456789012", "123456789012"),
    // Strings.
    ("\"abc\"", "\"abc\""),
    ("\"\"", "\"\""),
    ("\"a\"\"b\"", "\"a\"\"b\""),
    ("\"héllo wörld\"", "\"héllo wörld\""),
    // Booleans.
    ("TRUE", "TRUE"),
    ("false", "FALSE"),
    ("True", "TRUE"),
    // Error literals.
    ("#DIV/0!", "#DIV/0!"),
    ("#n/a", "#N/A"),
    ("#REF!", "#REF!"),
    ("#VALUE!", "#VALUE!"),
    ("#NAME?", "#NAME?"),
    ("#NUM!", "#NUM!"),
    ("#NULL!", "#NULL!"),
    // Cell references.
    ("A1", "A1"),
    ("a1", "A1"),
    ("$A1", "$A1"),
    ("A$1", "A$1"),
    ("$A$1", "$A$1"),
    ("ZZ100", "ZZ100"),
    ("xfd1048576", "XFD1048576"),
    // Ranges.
    ("A1:B2", "A1:B2"),
    ("$A$1:$B$10", "$A$1:$B$10"),
    ("B5:A1", "A1:B5"),
    ("A1:A1", "A1:A1"),
    // Whole columns and rows.
    ("A:A", "A:A"),
    ("a:c", "A:C"),
    ("$B:$D", "$B:$D"),
    ("1:1", "1:1"),
    ("$2:$5", "$2:$5"),
    ("5:2", "2:5"),
    // Sheets.
    ("Sheet1!A1", "Sheet1!A1"),
    ("'Q1 Sales'!A1", "'Q1 Sales'!A1"),
    ("'It''s'!B2", "'It''s'!B2"),
    ("'Sheet1'!A1", "Sheet1!A1"),
    ("'A1'!B2", "'A1'!B2"),
    ("'2024'!A1", "'2024'!A1"),
    ("Data!B2:B51", "Data!B2:B51"),
    ("Data!$C:$C", "Data!$C:$C"),
    ("Data!3:4", "Data!3:4"),
    ("my_sheet.v2!A1", "my_sheet.v2!A1"),
    // Names.
    ("Rates", "Rates"),
    ("_tax.rate", "_tax.rate"),
    ("ABCD1", "ABCD1"),
    // Operators and precedence.
    ("1+2", "1+2"),
    ("1 + 2", "1+2"),
    ("1+2*3", "1+2*3"),
    ("(1+2)*3", "(1+2)*3"),
    ("-2^2", "-2^2"),
    ("2^3^2", "2^3^2"),
    ("2^-1", "2^-1"),
    ("--1", "--1"),
    ("+A1", "+A1"),
    ("A1-B1", "A1-B1"),
    ("A1/B1", "A1/B1"),
    ("\"a\"&1=1", "\"a\"&1=1"),
    ("A1<>B1", "A1<>B1"),
    ("A1<=B1", "A1<=B1"),
    ("A1>=B1", "A1>=B1"),
    ("A1<B1", "A1<B1"),
    ("A1>B1", "A1>B1"),
    ("A1=B1", "A1=B1"),
    ("50%", "50%"),
    ("A1%%", "A1%%"),
    ("-5%", "-5%"),
    ("((1))", "((1))"),
    (" 1 ", "1"),
    // Calls.
    ("SUM(A1:A3)", "SUM(A1:A3)"),
    ("sum(1,2,3)", "SUM(1,2,3)"),
    ("SUM( 1 , 2 )", "SUM(1,2)"),
    ("PI()", "PI()"),
    ("IF(A1,,2)", "IF(A1,,2)"),
    ("IF(A1,)", "IF(A1,)"),
    (
        "IF(A1>0,SUM(B1:B3),MAX(C1,C2)*2)",
        "IF(A1>0,SUM(B1:B3),MAX(C1,C2)*2)",
    ),
    ("_xlfn.XLOOKUP(1,A:A,B:B)", "XLOOKUP(1,A:A,B:B)"),
    ("_xlfn.STDEV.S(A1:A9)", "STDEV.S(A1:A9)"),
    ("_xlws.SORT(A1:A3)", "SORT(A1:A3)"),
    ("LOG10(100)", "LOG10(100)"),
    ("TRUE()", "TRUE()"),
    ("SUM('Q1 Sales'!A1:A10)", "SUM('Q1 Sales'!A1:A10)"),
    // Array constants.
    ("{1,2;3,4}", "{1,2;3,4}"),
    ("{1,-2.5,\"x\",TRUE,#N/A}", "{1,-2.5,\"x\",TRUE,#N/A}"),
    ("{ 1 ; 2 }", "{1;2}"),
    ("SUM({1,2,3})", "SUM({1,2,3})"),
];

#[test]
fn grammar_cases_round_trip() {
    assert!(CASES.len() >= 80, "{} cases", CASES.len());
    for &(input, canonical) in CASES {
        let e = parse(input).unwrap_or_else(|e| panic!("{input:?}: {e}"));
        assert_eq!(render(&e), canonical, "{input:?}");
        let again = parse(canonical).unwrap_or_else(|e| panic!("{canonical:?}: {e}"));
        assert_eq!(again, e, "{canonical:?} parses to the same tree");
    }
}

#[test]
fn precedence_shapes_the_tree() {
    use formula::{BinaryOp::*, UnaryOp::*};
    let n = |v: f64| Box::new(Expr::Number(v));
    assert_eq!(
        parse("-2^2").unwrap(),
        Expr::Binary(Pow, Box::new(Expr::Unary(Neg, n(2.0))), n(2.0))
    );
    assert_eq!(
        parse("2^3^2").unwrap(),
        Expr::Binary(Pow, Box::new(Expr::Binary(Pow, n(2.0), n(3.0))), n(2.0))
    );
    assert_eq!(
        parse("1+2*3").unwrap(),
        Expr::Binary(Add, n(1.0), Box::new(Expr::Binary(Mul, n(2.0), n(3.0))))
    );
    let Expr::Binary(Eq, l, _) = parse("\"a\"&1=1").unwrap() else {
        panic!("= binds loosest")
    };
    assert!(matches!(*l, Expr::Binary(Concat, ..)));
}

#[test]
fn xlsx_rendering_prefixes_newer_functions() {
    let e = parse("XLOOKUP(1,A:A,B:B)+SUM(1)+STDEV.S(A1:A2)+IFS(TRUE,1)").unwrap();
    assert_eq!(
        render_xlsx(&e),
        "_xlfn.XLOOKUP(1,A:A,B:B)+SUM(1)+_xlfn.STDEV.S(A1:A2)+_xlfn.IFS(TRUE,1)"
    );
}

#[test]
fn bad_formulas_report_where_they_fail() {
    let cases: &[(&str, usize, &str)] = &[
        ("", 0, "unexpected end"),
        ("1+", 2, "unexpected end"),
        ("A1+", 3, "unexpected end"),
        ("SUM(1,", 6, "unexpected end"),
        ("1+*2", 2, "missing operand"),
        ("*1", 0, "missing operand"),
        ("SUM(1,2", 3, "unclosed parenthesis"),
        ("(1+2", 0, "unclosed parenthesis"),
        ("\"abc", 0, "unclosed string"),
        ("1+\"abc", 2, "unclosed string"),
        ("'Q1 Sales!A1", 0, "unclosed string"),
        ("A1 B1", 3, "intersection isn't supported"),
        ("SUM(A1:A3 B1:B3)", 10, "intersection isn't supported"),
        ("1)", 1, "unexpected character"),
        ("1 2", 2, "intersection isn't supported"),
        ("A1?", 2, "unexpected character"),
        ("#FOO!", 0, "unexpected character"),
        ("A1:", 2, "unexpected character"),
        ("Sheet1!", 7, "unexpected end"),
        ("Sheet1!foo", 7, "unexpected character"),
        ("A1099511627777", 1, "row out of range"),
        ("{1,2;3}", 6, "unexpected character"),
        ("{1,,2}", 3, "missing operand"),
        ("$", 0, "unexpected character"),
        ("SUM(1;2)", 5, "unexpected character"),
    ];
    for &(src, at, message) in cases {
        let e = parse(src).expect_err(src);
        assert_eq!((e.at, e.message), (at, message), "{src:?}");
    }
    // Positions count characters, not bytes.
    let e = parse("\"é\"+").unwrap_err();
    assert_eq!(e.at, 4);
}

fn adjusted(src: &str, own: &str, change: Change) -> String {
    let mut e = parse(src).unwrap();
    let changed = adjust(&mut e, own, &change);
    let out = render(&e);
    assert_eq!(changed, out != src, "{src:?} reports whether it changed");
    out
}

#[test]
fn inserted_rows_and_columns_move_references() {
    let rows = |at, count| Change::Insert {
        sheet: "S",
        axis: Axis::Rows,
        at,
        count,
    };
    let cols = |at, count| Change::Insert {
        sheet: "S",
        axis: Axis::Cols,
        at,
        count,
    };
    // Row position 4 is A5.
    assert_eq!(adjusted("SUM(A1:A10)", "S", rows(4, 1)), "SUM(A1:A11)");
    assert_eq!(adjusted("A5", "S", rows(4, 1)), "A6");
    assert_eq!(adjusted("$A$5", "S", rows(4, 1)), "$A$6");
    assert_eq!(adjusted("A4", "S", rows(4, 1)), "A4");
    assert_eq!(adjusted("A5:B9", "S", rows(4, 3)), "A8:B12");
    assert_eq!(adjusted("3:7", "S", rows(4, 2)), "3:9");
    assert_eq!(adjusted("A:A", "S", rows(0, 5)), "A:A");
    assert_eq!(adjusted("C1", "S", cols(1, 1)), "D1");
    assert_eq!(adjusted("A1:C1", "S", cols(1, 2)), "A1:E1");
    assert_eq!(adjusted("B:D", "S", cols(0, 1)), "C:E");
    assert_eq!(adjusted("A1", "S", cols(1, 1)), "A1");
    // Other sheets: only references that name the changed sheet follow.
    assert_eq!(adjusted("A5", "T", rows(4, 1)), "A5");
    assert_eq!(adjusted("S!A5+A5", "T", rows(4, 1)), "S!A6+A5");
    assert_eq!(adjusted("s!A5", "T", rows(4, 1)), "s!A6");
}

#[test]
fn deleted_rows_and_columns_shrink_or_break_references() {
    let rows = |at, count| Change::Delete {
        sheet: "S",
        axis: Axis::Rows,
        at,
        count,
    };
    let cols = |at, count| Change::Delete {
        sheet: "S",
        axis: Axis::Cols,
        at,
        count,
    };
    assert_eq!(adjusted("A5", "S", rows(4, 1)), "#REF!");
    assert_eq!(adjusted("A5+1", "S", rows(4, 1)), "#REF!+1");
    assert_eq!(adjusted("A6", "S", rows(4, 1)), "A5");
    assert_eq!(adjusted("A4", "S", rows(4, 1)), "A4");
    assert_eq!(adjusted("SUM(A1:A10)", "S", rows(4, 1)), "SUM(A1:A9)");
    // The deleted span cuts the start of the range.
    assert_eq!(adjusted("A5:A10", "S", rows(2, 4)), "A3:A6");
    // The deleted span cuts the end of the range.
    assert_eq!(adjusted("A1:A5", "S", rows(3, 4)), "A1:A3");
    // The whole range is deleted.
    assert_eq!(adjusted("A3:A5", "S", rows(2, 3)), "#REF!");
    assert_eq!(adjusted("2:4", "S", rows(0, 2)), "1:2");
    assert_eq!(adjusted("C1", "S", cols(2, 1)), "#REF!");
    assert_eq!(adjusted("D1", "S", cols(2, 1)), "C1");
    assert_eq!(adjusted("A1:D1", "S", cols(1, 2)), "A1:B1");
    assert_eq!(adjusted("B:C", "S", cols(1, 2)), "#REF!");
    assert_eq!(adjusted("1:3", "S", cols(0, 5)), "1:3");
}

#[test]
fn moved_formulas_shift_relative_references() {
    let shift = |rows, cols| Change::Shift { rows, cols };
    assert_eq!(adjusted("A2*2", "S", shift(1, 0)), "A3*2");
    assert_eq!(adjusted("$A$2*2", "S", shift(1, 1)), "$A$2*2");
    assert_eq!(adjusted("$A2+A$2", "S", shift(2, 3)), "$A4+D$2");
    assert_eq!(adjusted("A2", "S", shift(-2, 0)), "#REF!");
    assert_eq!(adjusted("B1", "S", shift(0, -2)), "#REF!");
    assert_eq!(adjusted("A1:B2", "S", shift(1, 1)), "B2:C3");
    assert_eq!(adjusted("A:A", "S", shift(5, 1)), "B:B");
    assert_eq!(adjusted("2:2", "S", shift(1, 5)), "3:3");
    assert_eq!(adjusted("Other!A1", "S", shift(1, 0)), "Other!A2");
}

#[test]
fn sheet_renames_and_deletions() {
    let rename = Change::RenameSheet {
        from: "Q1 Sales",
        to: "Q2",
    };
    // `Q2` reads as a cell, so the new name is quoted, as Excel writes it.
    assert_eq!(adjusted("'Q1 Sales'!B2", "S", rename), "'Q2'!B2");
    assert_eq!(adjusted("'q1 sales'!B2*2", "S", rename), "'Q2'!B2*2");
    assert_eq!(adjusted("B2", "Q1 Sales", rename), "B2");
    assert_eq!(
        adjusted(
            "'Q2'!A1",
            "S",
            Change::RenameSheet {
                from: "Q2",
                to: "It's"
            }
        ),
        "'It''s'!A1"
    );
    let delete = Change::DeleteSheet { name: "Q1 Sales" };
    assert_eq!(adjusted("'Q1 Sales'!B2+1", "S", delete), "#REF!+1");
    assert_eq!(adjusted("B2", "Q1 Sales", delete), "B2");
    assert!(formula::names_sheet(
        &parse("SUM('Q1 SALES'!A:A)").unwrap(),
        "q1 sales"
    ));
    assert!(!formula::names_sheet(&parse("SUM(A:A)").unwrap(), "S"));
}

#[test]
fn column_names_round_trip() {
    let mut s = String::new();
    for (c, want) in [
        (0, "A"),
        (25, "Z"),
        (26, "AA"),
        (51, "AZ"),
        (52, "BA"),
        (701, "ZZ"),
        (702, "AAA"),
        (16_383, "XFD"),
    ] {
        column_name(c, &mut s);
        assert_eq!(s, want);
    }
    for c in 0..20_000 {
        column_name(c, &mut s);
        assert_eq!(column_index(&s), Some(c), "{s}");
        assert_eq!(column_index(&s.to_lowercase()), Some(c));
    }
    assert_eq!(column_index(""), None);
    assert_eq!(column_index("A1"), None);
}

#[test]
fn name_box_references() {
    let r = parse_reference(" c5 ").unwrap();
    assert_eq!(r.sheet, None);
    assert_eq!(
        r.area,
        Area::Cell(CellAddr {
            row: 4,
            col: 2,
            row_abs: false,
            col_abs: false
        })
    );
    assert_eq!(
        parse_reference("Sheet2!B3").unwrap().sheet.as_deref(),
        Some("Sheet2")
    );
    for text in ["B12", "B12:D20", "'Q1 Sales'!A1", "A:C", "3:5"] {
        assert_eq!(format_reference(&parse_reference(text).unwrap()), text);
    }
    for bad in ["", "Nope", "1+1", "SUM(A1)", "A0"] {
        assert_eq!(parse_reference(bad), None, "{bad:?}");
    }
}
