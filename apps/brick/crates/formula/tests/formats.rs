//! STYLE-2: every case in `tests/data/formats.tsv` shows as LibreOffice Calc showed it
//! (`formats-libreoffice.tsv`, recorded by `scripts/lo_formats.py`), or, where a case names
//! an `excel` result, as Excel shows it.
//!
//! Fill and padding convention: LibreOffice's `getString()` leaves a `*x` fill out, writes
//! `_x` as one space, and pads a `?` placeholder with a figure space (U+2007). Brick's
//! `Formatted.text` also leaves the fill out (its char and place are in `Formatted.fill`)
//! and writes `_x` as a space, but pads `?` with a plain space, as Excel's text does; so
//! the recorded text is compared with each figure space read as a space.
//!
//! `#REJECTED` is a code LibreOffice refuses; Brick must refuse it too. An `excel` column
//! is `#REJECTED` (Excel refuses a code LibreOffice takes) or the quoted text Excel shows.
//! The `color` column is the colour LibreOffice's formatter picks (`RRGGBB`), empty for
//! none.

use formula::scalar::iso_serial;
use formula::{builtin_format, FormatColor, Formatted, NumberFormat, Value};

fn value(spec: &str) -> Value {
    if let Some(t) = spec.strip_prefix("text:") {
        return Value::text(t);
    }
    match spec {
        "TRUE" => Value::Bool(true),
        "FALSE" => Value::Bool(false),
        _ => match iso_serial(spec) {
            Some(serial) => Value::Date(serial),
            None => Value::Number(spec.parse().unwrap_or_else(|_| panic!("bad value {spec}"))),
        },
    }
}

/// The recorded text with `\u{XX}` and `\\` escapes undone.
fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('\\') {
        out.push_str(&rest[..i]);
        rest = &rest[i + 1..];
        if let Some(r) = rest.strip_prefix('\\') {
            out.push('\\');
            rest = r;
        } else {
            let end = rest.find('}').expect("\\u{XX}");
            let code = u32::from_str_radix(&rest[2..end], 16).expect("hex");
            out.push(char::from_u32(code).expect("a char"));
            rest = &rest[end + 1..];
        }
    }
    out.push_str(rest);
    out
}

/// LibreOffice's RGB for a colour tag; any colour for an indexed one.
fn same_color(lo: &str, got: Option<FormatColor>) -> bool {
    let rgb = match got {
        None => return lo.is_empty(),
        Some(FormatColor::Indexed(_)) => return !lo.is_empty(),
        Some(FormatColor::Black) => "000000",
        Some(FormatColor::Blue) => "0000FF",
        Some(FormatColor::Cyan) => "00FFFF",
        Some(FormatColor::Green) => "00FF00",
        Some(FormatColor::Magenta) => "FF00FF",
        Some(FormatColor::Red) => "FF0000",
        Some(FormatColor::White) => "FFFFFF",
        Some(FormatColor::Yellow) => "FFFF00",
    };
    lo == rgb
}

/// `#REJECTED` or the text, as the fixture writes an expectation.
fn shown(r: &Result<Formatted, formula::ErrorKind>) -> String {
    match r {
        Ok(f) => f.text.clone(),
        Err(_) => "#REJECTED".to_string(),
    }
}

#[test]
fn formats_match_libreoffice() {
    let data = include_str!("data/formats-libreoffice.tsv");
    let mut failures = Vec::new();
    let mut cases = 0;
    let mut overrides = 0;
    for line in data.lines().skip(1) {
        let cols: Vec<&str> = line.split('\t').collect();
        let [code, spec, text, color, excel, why] = cols[..] else {
            panic!("bad fixture line {line:?}")
        };
        cases += 1;
        let lo = unescape(text).replace('\u{2007}', " ");
        let want = if excel.is_empty() {
            lo.clone()
        } else {
            overrides += 1;
            assert!(
                !why.is_empty(),
                "{code} {spec}: an Excel override needs a reason"
            );
            let want = match excel.strip_prefix('"').and_then(|e| e.strip_suffix('"')) {
                Some(quoted) => quoted.to_string(),
                None => {
                    assert_eq!(
                        excel, "#REJECTED",
                        "{code} {spec}: excel is \"text\" or #REJECTED"
                    );
                    excel.to_string()
                }
            };
            assert_ne!(
                want, lo,
                "{code} {spec}: LibreOffice now agrees with Excel; drop the override"
            );
            want
        };
        let got = NumberFormat::parse(code).map(|f| f.display(&value(spec)));
        let got_text = shown(&got);
        let color_ok = match &got {
            Ok(f) if excel.is_empty() => same_color(color, f.color),
            _ => true,
        };
        if got_text != want || !color_ok {
            failures.push(format!(
                "{code:?} {spec}: want {want:?} [{color}], got {got_text:?} [{:?}]",
                got.as_ref().ok().and_then(|f| f.color)
            ));
        }
    }
    assert!(cases >= 300, "{cases} cases");
    assert!(
        failures.is_empty(),
        "{} of {cases} failures ({overrides} overrides):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn every_builtin_code_is_a_fixture_case() {
    let data = include_str!("data/formats-libreoffice.tsv");
    for id in 0..=200 {
        if let Some(code) = builtin_format(id) {
            assert!(NumberFormat::parse(code).is_ok(), "{id}: {code}");
            assert!(
                data.lines().any(|l| l.split('\t').next() == Some(code)),
                "built-in {id} ({code}) has no fixture case"
            );
        }
    }
}

#[test]
fn fill_marks_where_the_char_repeats() {
    let accounting = NumberFormat::parse(r#"_(* #,##0_);_(* \(#,##0\);_(* "-"??_);_(@_)"#)
        .expect("Excel's accounting code");
    let f = accounting.display(&Value::Number(-1234.0));
    assert_eq!((f.text.as_str(), f.fill), (" (1,234)", Some((1, ' '))));
    let f = accounting.display(&Value::text("abc"));
    assert_eq!((f.text.as_str(), f.fill), (" abc ", None));
    // The sign comes before the fill.
    let f = NumberFormat::parse("*x0")
        .unwrap()
        .display(&Value::Number(-5.0));
    assert_eq!((f.text.as_str(), f.fill), ("-5", Some((1, 'x'))));
    let f = NumberFormat::parse("0.0*-")
        .unwrap()
        .display(&Value::Number(2.0));
    assert_eq!((f.text.as_str(), f.fill), ("2.0", Some((3, '-'))));
    // A date Excel can't show fills the cell with #.
    let f = NumberFormat::parse("yyyy")
        .unwrap()
        .display(&Value::Number(3e6));
    assert_eq!((f.text.as_str(), f.fill), ("", Some((0, '#'))));
}

#[test]
fn only_general_codes_are_general() {
    for (code, general) in [
        ("General", true),
        ("general", true),
        ("General;-General", true),
        ("[Red]General", false),
        ("[>5]General;0", false),
        ("0", false),
        ("@", false),
        ("\"x\"General", false),
    ] {
        assert_eq!(
            NumberFormat::parse(code).unwrap().is_general(),
            general,
            "{code}"
        );
    }
}

#[test]
fn values_other_than_numbers_and_text() {
    let f = NumberFormat::parse("0.00;(0.00);\"zero\";\"text\"").unwrap();
    assert_eq!(f.display(&Value::Bool(false)).text, "FALSE");
    assert_eq!(
        f.display(&Value::Error(formula::ErrorKind::Div0)).text,
        "#DIV/0!"
    );
    assert_eq!(f.display(&Value::Empty).text, "");
    let array = formula::Array {
        rows: 1,
        cols: 2,
        values: vec![Value::Number(-2.0), Value::Number(3.0)],
    };
    assert_eq!(f.display(&Value::Array(array.into())).text, "(2.00)");
}
