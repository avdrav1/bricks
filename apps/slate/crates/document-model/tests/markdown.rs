//! SLATE-2: every construct of the Markdown fixture maps to its block or run, and what the
//! model can't show is kept as Raw source.
//!
//! The fixture is read into an outline, one line per block, and compared with the outline
//! written out below. A run shows as its text in quotes after its look's letters:
//! b(old), i(talic), s(trike), c(ode), r(aw HTML), and `->URL` for a link.

use document_model::{
    read_markdown, Align, Block, Encoding, Format, ImageFormat, ImageSource, ListKind, Paragraph,
    RawKind,
};
use std::path::Path;

fn data(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

fn runs(p: &Paragraph) -> String {
    let mut out = Vec::new();
    for r in &p.runs {
        let s = &r.style;
        let mut look = String::new();
        for (on, c) in [
            (s.bold == Some(true), 'b'),
            (s.italic == Some(true), 'i'),
            (s.strike == Some(true), 's'),
            (s.code, 'c'),
            (s.raw, 'r'),
        ] {
            if on {
                look.push(c);
            }
        }
        let link = s
            .link
            .as_ref()
            .map(|l| format!("->{l}"))
            .unwrap_or_default();
        out.push(format!("{look}{:?}{link}", r.text));
    }
    out.join(" ")
}

fn align(a: Align) -> &'static str {
    match a {
        Align::Start => "",
        Align::Center => " centre",
        Align::End => " right",
        Align::Justify => " justify",
    }
}

fn outline(blocks: &[Block], prefix: &str, out: &mut Vec<String>) {
    for b in blocks {
        let line = match b {
            Block::Paragraph(p) => {
                let style = match p.style.as_str() {
                    "Normal" => "P".to_owned(),
                    "Quote" => "Quote".to_owned(),
                    id => id.replace("Heading", "H"),
                };
                let indent = if p.indent > 0 {
                    format!(" indent {}", p.indent)
                } else {
                    String::new()
                };
                format!("{style}{indent}{}: {}", align(p.align), runs(p))
            }
            Block::ListItem(item) => {
                let kind = match &item.kind {
                    ListKind::Bullet => "bullet".to_owned(),
                    ListKind::Ordered { label, .. } => label.clone(),
                    ListKind::Task { checked: true } => "[x]".to_owned(),
                    ListKind::Task { checked: false } => "[ ]".to_owned(),
                };
                format!("Item {} {kind}: {}", item.level, runs(&item.paragraph))
            }
            Block::CodeBlock(code) => format!(
                "Code {} indent {}: {:?}",
                code.language.as_deref().unwrap_or("-"),
                code.indent,
                code.text
            ),
            Block::Table(table) => {
                out.push(format!("{prefix}Table"));
                for row in &table.rows {
                    out.push(format!(
                        "{prefix}  Row{}",
                        if row.header { " header" } else { "" }
                    ));
                    for cell in &row.cells {
                        out.push(format!("{prefix}    Cell"));
                        outline(&cell.blocks, &format!("{prefix}      "), out);
                    }
                }
                continue;
            }
            Block::Image(image) => format!(
                "Image {:?} {}",
                image.alt,
                match &image.source {
                    ImageSource::Link(l) => format!("link {l}"),
                    ImageSource::Embedded { format, bytes } => {
                        format!("{format:?} {} bytes", bytes.len())
                    }
                }
            ),
            Block::PageBreak => "PageBreak".to_owned(),
            Block::Rule => "Rule".to_owned(),
            Block::Raw(raw) => format!("Raw {:?}: {:?}", raw.kind, raw.source),
        };
        out.push(format!("{prefix}{line}"));
    }
}

const EXPECTED: &str = r#"
H1: "Slate Markdown fixture"
P: "A paragraph with " i"italic" ", " b"bold" ", " bi"bold italic" ", " s"struck" ", " c"inline code" ", and a " "link"->https://example.com/slate " across a soft break. A hard break follows this line\nand this line continues the paragraph."
H2: "Second level heading"
H3: "Third level heading"
H4: "Fourth level heading"
H5: "Fifth level heading"
H6: "Sixth level heading"
Quote: "A block quote with " b"bold" " inside."
Quote indent 1: "A nested quote."
Item 0 bullet: "First bullet"
Item 0 bullet: "Second bullet"
Item 1 bullet: "Nested bullet"
Item 2 bullet: "Third level bullet"
Item 0 bullet: "Back to the first level"
Item 0 1.: "First number"
Item 0 2.: "Second number"
Item 1 1.: "Nested number"
Item 0 3.: "Third number"
P: "A list that starts at seven:"
Item 0 7.: "Seven starts this list"
Item 0 8.: "Eight follows"
Item 0 [ ]: "An open task"
Item 0 [x]: "A done task"
Code rust indent 0: "fn main() {\n    println!(\"hello\");\n}\n"
Code - indent 0: "indented code block\n"
Table
  Row header
    Cell
      P: "Name"
    Cell
      P centre: "Count"
    Cell
      P right: "Note"
  Row
    Cell
      P: "Apples"
    Cell
      P centre: "3"
    Cell
      P right: i"fresh"
  Row
    Cell
      P: "Pears"
    Cell
      P centre: "12"
    Cell
      P right: c"ripe"
Rule
Image "A blue picture" link picture.png
Raw Html: "<div class=\"note\">\nRaw <b>HTML</b> block.\n</div>"
P: "Text with inline " r"<kbd>" "HTML" r"</kbd>" " and a reference " "link"->https://example.com/ref " and a footnote[^1]."
Raw ReferenceDefinition: "[ref]: https://example.com/ref \"Reference\""
Raw FootnoteDefinition: "[^1]: The footnote's text."
P: "The last paragraph."
"#;

#[test]
fn every_construct_maps_to_its_block_or_run() {
    let doc = read_markdown(&std::fs::read(data("fixture.md")).unwrap());
    let mut got = Vec::new();
    outline(&doc.blocks, "", &mut got);
    let expected: Vec<&str> = EXPECTED.trim().lines().collect();
    assert_eq!(got, expected, "\ngot:\n{}", got.join("\n"));
    assert!(matches!(doc.format, Format::Markdown(i) if i.encoding == Encoding::Utf8));
}

#[test]
fn raw_blocks_hold_their_source_exactly() {
    let text = std::fs::read_to_string(data("fixture.md")).unwrap();
    let doc = read_markdown(text.as_bytes());
    let raws: Vec<_> = doc
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Raw(r) => Some(r),
            _ => None,
        })
        .collect();
    assert_eq!(
        raws.iter().map(|r| r.kind).collect::<Vec<_>>(),
        [
            RawKind::Html,
            RawKind::ReferenceDefinition,
            RawKind::FootnoteDefinition
        ]
    );
    for raw in raws {
        assert!(
            text.contains(&raw.source),
            "{:?} is in the file",
            raw.source
        );
    }
}

#[test]
fn nested_list_paragraphs_and_code_keep_their_level() {
    let doc =
        read_markdown(b"- item one\n\n  more of item one\n\n  ```\n  code\n  ```\n- item two\n");
    let mut got = Vec::new();
    outline(&doc.blocks, "", &mut got);
    assert_eq!(
        got,
        [
            "Item 0 bullet: \"item one\"",
            "P indent 1: \"more of item one\"",
            "Code - indent 1: \"code\\n\"",
            "Item 0 bullet: \"item two\"",
        ]
    );
}

#[test]
fn local_pictures_load_from_the_files_directory() {
    let mut doc = read_markdown(
        b"![one](picture.png)\n\n![two](missing.png)\n\n![three](https://example.com/x.png)\n",
    );
    document_model::load_linked_images(&mut doc.blocks, &data(""));
    let sources: Vec<_> = doc
        .blocks
        .iter()
        .map(|b| match b {
            Block::Image(i) => &i.source,
            other => panic!("not an image: {other:?}"),
        })
        .collect();
    assert!(matches!(
        sources[0],
        ImageSource::Embedded {
            format: ImageFormat::Png,
            ..
        }
    ));
    assert_eq!(*sources[1], ImageSource::Link("missing.png".into()));
    assert_eq!(
        *sources[2],
        ImageSource::Link("https://example.com/x.png".into())
    );
}
