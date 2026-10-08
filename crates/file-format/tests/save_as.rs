//! SAVE-2 acceptance: Save As can change the delimiter and the encoding.
//!
//! One table (a UTF-8 comma file with a BOM, CRLF rows, quoted fields, an edit, an
//! inserted row, and a cleared cell) is saved as tab-separated UTF-16 LE, semicolon
//! Windows-1252, pipe UTF-8 without a BOM, and comma UTF-8 with its BOM again. Each file
//! must decode to the same cell values, re-quoted only where the new delimiter needs it,
//! keep each row's line ending, carry the encoding's BOM, and reopen with that delimiter
//! and encoding detected. The original file is untouched. A character the target encoding
//! can't hold fails the save, leaving no file.

use csv_engine::{Charset, Encoding};
use data_model::{CellRef, CsvTable, DelimiterChoice, Edit, RowBlock, TableSource};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

const SOURCE: &[u8] = b"\xEF\xBB\xBFid,name,note\r\n1,\"Smith, Ann\",caf\xC3\xA9\r\n2,\"say \"\"hi\"\"\",x;y|z\r\n3,Zo\xC3\xAB,tab\there\r\n";

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("save2-{}-{name}", std::process::id()))
}

fn open(path: &Path) -> CsvTable {
    let t = CsvTable::open(path, DelimiterChoice::Auto).unwrap();
    t.index().build(&AtomicBool::new(false)).unwrap();
    t
}

/// Every cell as the table shows it, header row included.
fn cells(t: &mut CsvTable) -> Vec<Vec<String>> {
    t.set_header(false);
    let n = t.row_count();
    let mut b = RowBlock::full_text();
    t.read_rows(0..n, &mut b);
    (0..n)
        .map(|r| {
            (0..b.cells_in_row(r))
                .map(|c| b.cell(r, c).unwrap().to_owned())
                .collect()
        })
        .collect()
}

fn decode(bytes: &[u8], enc: Encoding) -> String {
    match enc.charset {
        Charset::Utf8 => String::from_utf8(bytes.to_vec()).unwrap(),
        Charset::Utf16Le => encoding_rs::UTF_16LE.decode(bytes).0.into_owned(),
        Charset::Utf16Be => encoding_rs::UTF_16BE.decode(bytes).0.into_owned(),
        Charset::Windows1252 => encoding_rs::WINDOWS_1252.decode(bytes).0.into_owned(),
    }
}

#[test]
fn save_as_changes_delimiter_and_encoding() {
    let src = temp("source.csv");
    std::fs::write(&src, SOURCE).unwrap();
    let mut table = open(&src);
    let at = |t: &CsvTable, r, c| CellRef {
        row: t.row_id(r),
        col: t.col_id(c),
    };
    table.apply(Edit::set(at(&table, 2, 1), "Zoë; \"Z\""));
    let row = table.insert_rows(table.row_count(), 1).unwrap();
    table.apply(row);
    table.apply(Edit::set(at(&table, 3, 0), "4"));
    table.apply(Edit::set(at(&table, 3, 2), "new|row"));
    let clear = table.clear_cells(0..1, 2..3).unwrap();
    table.apply(clear);
    let want = cells(&mut table);
    table.set_header(true);
    assert_eq!(
        want[1],
        ["1", "Smith, Ann", ""],
        "the cleared cell is empty"
    );

    let utf16 = Encoding {
        charset: Charset::Utf16Le,
        bom: true,
    };
    let w1252 = Encoding {
        charset: Charset::Windows1252,
        bom: false,
    };
    let utf8_bom = Encoding {
        charset: Charset::Utf8,
        bom: true,
    };
    let cases: [(&str, u8, Encoding, &str); 4] = [
        (
            "tab-utf16.tsv",
            b'\t',
            utf16,
            "id\tname\tnote\r\n1\tSmith, Ann\t\r\n2\t\"say \"\"hi\"\"\"\tx;y|z\r\n3\t\"Zoë; \"\"Z\"\"\"\t\"tab\there\"\r\n4\t\tnew|row\r\n",
        ),
        (
            "semicolon-1252.csv",
            b';',
            w1252,
            "id;name;note\r\n1;Smith, Ann;\r\n2;\"say \"\"hi\"\"\";\"x;y|z\"\r\n3;\"Zoë; \"\"Z\"\"\";tab\there\r\n4;;new|row\r\n",
        ),
        (
            "pipe-utf8.csv",
            b'|',
            Encoding::UTF8,
            "id|name|note\r\n1|Smith, Ann|\r\n2|\"say \"\"hi\"\"\"|\"x;y|z\"\r\n3|\"Zoë; \"\"Z\"\"\"|tab\there\r\n4||\"new|row\"\r\n",
        ),
        (
            "comma-utf8-bom.csv",
            b',',
            utf8_bom,
            "id,name,note\r\n1,\"Smith, Ann\",\r\n2,\"say \"\"hi\"\"\",x;y|z\r\n3,\"Zoë; \"\"Z\"\"\",tab\there\r\n4,,new|row\r\n",
        ),
    ];
    for (name, delimiter, encoding, text) in cases {
        let dest = temp(name);
        let job = table.save_job().unwrap().with_format(delimiter, encoding);
        file_format::save_csv(&dest, &job).unwrap();
        let bytes = std::fs::read(&dest).unwrap();
        let bom: &[u8] = match (encoding.charset, encoding.bom) {
            (Charset::Utf8, true) => b"\xEF\xBB\xBF",
            (Charset::Utf16Le, true) => b"\xFF\xFE",
            _ => b"",
        };
        assert!(bytes.starts_with(bom), "{name}: BOM");
        assert_eq!(decode(&bytes[bom.len()..], encoding), text, "{name}: text");

        let mut reopened = open(&dest);
        assert_eq!(
            reopened.dialect().delimiter,
            delimiter,
            "{name}: delimiter detected"
        );
        assert_eq!(reopened.encoding(), encoding, "{name}: encoding detected");
        assert_eq!(cells(&mut reopened), want, "{name}: same cells");
        std::fs::remove_file(dest).unwrap();
    }
    assert_eq!(
        std::fs::read(&src).unwrap(),
        SOURCE,
        "the original is untouched"
    );

    let unmappable = temp("ascii-fails.csv");
    table.apply(Edit::set(at(&table, 0, 1), "東京"));
    let job = table.save_job().unwrap().with_format(b',', w1252);
    let err = file_format::save_csv(&unmappable, &job).unwrap_err();
    assert!(err.to_string().contains('東'), "{err}");
    assert!(!unmappable.exists(), "a failed save leaves no file");
    std::fs::remove_file(src).unwrap();
}
