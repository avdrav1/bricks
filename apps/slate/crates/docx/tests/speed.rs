//! SLATE-3: a 500-page .docx reads in under 1 s (asserted in release builds).
//!
//!   cargo test --release -p docx --test speed -- --ignored --nocapture
//!
//! The document is written here: 500 pages, each a heading, eight paragraphs of about 60
//! words in runs of three looks, three numbered list items, a table on every fifth page,
//! and a page break; styles and numbering as Word writes them.

use document_model::Block;
use std::fmt::Write as _;
use std::io::{Cursor, Write};
use std::time::Instant;
use zip::write::SimpleFileOptions;

const PAGES: usize = 500;

const WORDS: [&str; 16] = [
    "lorem",
    "ipsum",
    "dolor",
    "sit",
    "amet",
    "consectetur",
    "adipiscing",
    "elit",
    "sed",
    "do",
    "eiusmod",
    "tempor",
    "incididunt",
    "ut",
    "labore",
    "magna",
];

fn sentence(seed: usize, words: usize) -> String {
    (0..words)
        .map(|i| WORDS[(seed * 7 + i * 3) % WORDS.len()])
        .collect::<Vec<_>>()
        .join(" ")
}

fn run(out: &mut String, props: &str, text: &str) {
    let _ = write!(
        out,
        r#"<w:r><w:rPr>{props}</w:rPr><w:t xml:space="preserve">{text} </w:t></w:r>"#
    );
}

fn document() -> String {
    let mut body = String::new();
    for page in 0..PAGES {
        let _ = write!(
            body,
            r#"<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Chapter {page}</w:t></w:r></w:p>"#
        );
        for p in 0..8 {
            body.push_str(r#"<w:p><w:pPr><w:jc w:val="both"/></w:pPr>"#);
            run(&mut body, "", &sentence(page + p, 30));
            run(&mut body, "<w:b/>", &sentence(page + p + 1, 6));
            run(
                &mut body,
                r#"<w:i/><w:color w:val="C00000"/>"#,
                &sentence(page + p + 2, 24),
            );
            body.push_str("</w:p>");
        }
        for item in 0..3 {
            let _ = write!(
                body,
                r#"<w:p><w:pPr><w:pStyle w:val="ListParagraph"/><w:numPr><w:ilvl w:val="{}"/><w:numId w:val="1"/></w:numPr></w:pPr>"#,
                item % 2
            );
            run(&mut body, "", &sentence(page + item, 8));
            body.push_str("</w:p>");
        }
        if page % 5 == 0 {
            body.push_str("<w:tbl><w:tblPr/><w:tblGrid/>");
            for r in 0..4 {
                body.push_str("<w:tr>");
                for c in 0..3 {
                    let _ = write!(
                        body,
                        "<w:tc><w:tcPr/><w:p><w:r><w:t>cell {r} {c}</w:t></w:r></w:p></w:tc>"
                    );
                }
                body.push_str("</w:tr>");
            }
            body.push_str("</w:tbl>");
        }
        if page + 1 < PAGES {
            body.push_str(r#"<w:p><w:r><w:br w:type="page"/></w:r></w:p>"#);
        }
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body>{body}<w:sectPr/></w:body></w:document>"#
    )
}

const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val="22"/></w:rPr></w:rPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:rPr><w:b/><w:sz w:val="32"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="ListParagraph"><w:name w:val="List Paragraph"/><w:basedOn w:val="Normal"/></w:style></w:styles>"#;

const NUMBERING: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/></w:lvl><w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="lowerLetter"/><w:lvlText w:val="%2)"/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num></w:numbering>"#;

fn package(document: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default();
    let parts = [
        ("[Content_Types].xml", r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#.to_owned()),
        ("_rels/.rels", r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.to_owned()),
        ("word/_rels/document.xml.rels", r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/></Relationships>"#.to_owned()),
        ("word/styles.xml", STYLES.to_owned()),
        ("word/numbering.xml", NUMBERING.to_owned()),
        ("word/document.xml", document.to_owned()),
    ];
    for (name, text) in parts {
        zip.start_file(name, opts).unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
#[ignore = "perf: run in release with --ignored"]
fn a_500_page_document_reads_in_under_a_second() {
    let xml = document();
    let bytes = package(&xml);
    let t0 = Instant::now();
    let doc = docx::read_from(Cursor::new(&bytes)).unwrap();
    let elapsed = t0.elapsed();
    let breaks = doc
        .blocks
        .iter()
        .filter(|b| matches!(b, Block::PageBreak))
        .count();
    assert_eq!(breaks, PAGES - 1, "one page break per page");
    println!(
        "read {PAGES} pages ({:.1} MB of document.xml, {} blocks, {} words) in {:.0} ms",
        xml.len() as f64 / 1e6,
        doc.blocks.len(),
        doc.word_count(),
        elapsed.as_secs_f64() * 1e3
    );
    if !cfg!(debug_assertions) {
        assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
    }
}
