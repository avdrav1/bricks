//! ENG-6 acceptance: encoding and BOM round-trip on save.
//!
//! For UTF-8 (with and without BOM), UTF-16 LE/BE (with BOM, and LE without), and
//! Windows-1252 ("Latin-1"): open with detection, edit one cell, save. The saved file must be
//! the original text with that one cell changed, in the original encoding with the original
//! BOM, byte for byte; an unedited save must reproduce the original exactly.

use csv_engine::{Charset, Encoding};
use data_model::{CellRef, CsvTable, DelimiterChoice};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

const TEXT: &str = "id,name,note\r\n1,Müller,\"a, \"\"b\"\"\"\r\n2,Zoë,café\r\n3,Ñandú,€5\r\n";
const EDITED: &str =
    "id,name,note\r\n1,Müller,\"a, \"\"b\"\"\"\r\n2,\"Ångström, Å\",café\r\n3,Ñandú,€5\r\n";

fn encode(text: &str, enc: Encoding) -> Vec<u8> {
    let mut out = Vec::new();
    match enc.charset {
        Charset::Utf8 => {
            if enc.bom {
                out.extend_from_slice(b"\xEF\xBB\xBF");
            }
            out.extend_from_slice(text.as_bytes());
        }
        Charset::Utf16Le | Charset::Utf16Be => {
            let le = enc.charset == Charset::Utf16Le;
            if enc.bom {
                out.extend_from_slice(if le { b"\xFF\xFE" } else { b"\xFE\xFF" });
            }
            for u in text.encode_utf16() {
                out.extend_from_slice(&if le { u.to_le_bytes() } else { u.to_be_bytes() });
            }
        }
        Charset::Windows1252 => {
            let (bytes, _, unmappable) = encoding_rs::WINDOWS_1252.encode(text);
            assert!(!unmappable);
            out.extend_from_slice(&bytes);
        }
    }
    out
}

fn temp(name: &str, bytes: &[u8]) -> PathBuf {
    let p = std::env::temp_dir().join(format!("eng6-{}-{name}.csv", std::process::id()));
    std::fs::write(&p, bytes).unwrap();
    p
}

fn open(path: &std::path::Path) -> CsvTable {
    let t = CsvTable::open(path, DelimiterChoice::Auto).unwrap();
    t.index().build(&AtomicBool::new(false)).unwrap();
    t
}

const CASES: [(&str, Encoding); 6] = [
    (
        "utf8",
        Encoding {
            charset: Charset::Utf8,
            bom: false,
        },
    ),
    (
        "utf8-bom",
        Encoding {
            charset: Charset::Utf8,
            bom: true,
        },
    ),
    (
        "utf16le-bom",
        Encoding {
            charset: Charset::Utf16Le,
            bom: true,
        },
    ),
    (
        "utf16be-bom",
        Encoding {
            charset: Charset::Utf16Be,
            bom: true,
        },
    ),
    (
        "utf16le",
        Encoding {
            charset: Charset::Utf16Le,
            bom: false,
        },
    ),
    (
        "cp1252",
        Encoding {
            charset: Charset::Windows1252,
            bom: false,
        },
    ),
];

#[test]
fn encoding_and_bom_round_trip_on_save() {
    for (name, enc) in CASES {
        let original = encode(TEXT, enc);
        let path = temp(name, &original);
        let table = open(&path);
        assert_eq!(table.encoding(), enc, "{name}: detected encoding");
        assert_eq!(
            table.cell_value(2, 1).as_deref(),
            Some("Zoë"),
            "{name}: decoded text"
        );
        assert_eq!(table.cell_value(3, 2).as_deref(), Some("€5"), "{name}");

        // Unedited save reproduces the file exactly.
        file_format::save_csv(&path, &table.save_job().unwrap()).unwrap();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            original,
            "{name}: unedited save"
        );

        // One edit: only that cell changes, everything else byte for byte.
        let mut table = open(&path);
        table.set_cell(
            CellRef {
                row: table.row_id(2),
                col: 1,
            },
            "Ångström, Å",
        );
        file_format::save_csv(&path, &table.save_job().unwrap()).unwrap();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            encode(EDITED, enc),
            "{name}: edited save"
        );

        let reopened = open(&path);
        assert_eq!(reopened.encoding(), enc, "{name}: encoding after save");
        assert_eq!(
            reopened.cell_value(2, 1).as_deref(),
            Some("Ångström, Å"),
            "{name}"
        );
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn text_the_encoding_cannot_hold_fails_the_save_and_keeps_the_file() {
    let original = encode(TEXT, CASES[5].1);
    let path = temp("cp1252-unmappable", &original);
    let mut table = open(&path);
    table.set_cell(
        CellRef {
            row: table.row_id(1),
            col: 1,
        },
        "東京",
    );
    let err = file_format::save_csv(&path, &table.save_job().unwrap())
        .unwrap_err()
        .to_string();
    assert!(err.contains('東') && err.contains("Windows-1252"), "{err}");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        original,
        "original untouched"
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn every_windows_1252_byte_survives() {
    let mut original = b"byte\n".to_vec();
    for b in 0x20u8..=0xFF {
        if b != b'"' && b != b',' && b != 0x7F {
            original.extend_from_slice(&[b, b'\n']);
        }
    }
    let path = temp("cp1252-all", &original);
    let table = open(&path);
    assert_eq!(table.encoding().charset, Charset::Windows1252);
    file_format::save_csv(&path, &table.save_job().unwrap()).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), original);
    std::fs::remove_file(path).unwrap();
}
