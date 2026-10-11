//! SLATE-1: plain text in every encoding and line ending reads to the same paragraphs, and
//! the reader records what it read.

use document_model::{read_text, Block, Encoding, Format, LineEnding, TextInfo};

const LINES: [&str; 4] = [
    "Café – “quoted” déjà vu",
    "Second line ½ €5",
    "",
    "Fourth line after an empty one",
];

fn paragraphs(bytes: &[u8]) -> (Vec<String>, TextInfo) {
    let doc = read_text(bytes);
    let Format::Text(info) = doc.format else {
        panic!("not read as text: {:?}", doc.format);
    };
    let texts = doc
        .blocks
        .iter()
        .map(|b| match b {
            Block::Paragraph(p) => p.text(),
            other => panic!("not a paragraph: {other:?}"),
        })
        .collect();
    (texts, info)
}

fn data(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

#[test]
fn every_encoding_reads_to_the_same_paragraphs() {
    let cases = [
        ("utf8-lf.txt", Encoding::Utf8, LineEnding::Lf, true),
        (
            "utf8-bom-crlf.txt",
            Encoding::Utf8Bom,
            LineEnding::CrLf,
            true,
        ),
        (
            "utf16le-crlf.txt",
            Encoding::Utf16Le,
            LineEnding::CrLf,
            true,
        ),
        ("utf16be-lf.txt", Encoding::Utf16Be, LineEnding::Lf, false),
        (
            "windows1252-cr.txt",
            Encoding::Windows1252,
            LineEnding::Cr,
            true,
        ),
    ];
    for (name, encoding, line_ending, final_newline) in cases {
        let (texts, info) = paragraphs(&data(name));
        assert_eq!(texts, LINES, "{name}");
        assert_eq!(
            info,
            TextInfo {
                encoding,
                line_ending,
                final_newline
            },
            "{name}"
        );
    }
}

#[test]
fn crlf_cr_and_lf_each_end_a_line() {
    let (texts, info) = paragraphs(b"a\r\nb\rc\nd");
    assert_eq!(texts, ["a", "b", "c", "d"]);
    assert_eq!(
        info.line_ending,
        LineEnding::CrLf,
        "the first break is recorded"
    );
    assert!(!info.final_newline);

    let (texts, info) = paragraphs(b"a\r\rb\r");
    assert_eq!(
        texts,
        ["a", "", "b"],
        "CR CR is an empty line, not one break"
    );
    assert_eq!(info.line_ending, LineEnding::Cr);
    assert!(info.final_newline);

    let (texts, _) = paragraphs(b"\n\n");
    assert_eq!(texts, ["", ""], "two breaks end two empty lines");
}

#[test]
fn a_file_without_line_breaks_is_one_paragraph() {
    let (texts, info) = paragraphs(b"only");
    assert_eq!(texts, ["only"]);
    assert_eq!(info.line_ending, LineEnding::Lf);
    assert!(!info.final_newline);
    let (texts, _) = paragraphs(b"");
    assert!(texts.is_empty());
}

#[test]
fn bytes_that_are_not_utf8_read_as_windows_1252() {
    // 0x80-0x9F are Windows-1252's extra letters, not C1 controls.
    let (texts, info) = paragraphs(b"\x80 \x93hi\x94 na\xefve");
    assert_eq!(texts, ["€ “hi” naïve"]);
    assert_eq!(info.encoding, Encoding::Windows1252);
}

#[test]
fn word_count_counts_across_paragraphs() {
    let doc = read_text(b"one two\nthree\n\n  four  five ");
    assert_eq!(doc.word_count(), 5);
}
