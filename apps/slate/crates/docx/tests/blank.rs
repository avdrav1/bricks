//! SLATE-18: Slate's built-in blank Word document reads as one empty normal paragraph, and
//! what is typed into it saves as a Word document that reads back, and that LibreOffice
//! reads, with the text and its styles (the ignored test).

use document_edit::{normalize, replace, set_style, split, text_len, Container, History, Pos};
use document_model::{Block, Document, ParagraphStyle};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

fn temp(test: &str, name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join(format!("docx-blank-{}", std::process::id()))
        .join(test);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// The blank document, as Slate opens it: from a private copy, runs merged.
fn open_blank(test: &str) -> (PathBuf, Document, docx::DocxSource) {
    let original = temp(test, "original.docx");
    std::fs::write(&original, docx::blank()).unwrap();
    let (mut doc, mut source) = docx::read_source(&original).unwrap();
    normalize(&mut doc);
    source.set_blocks(doc.blocks.clone());
    (original, doc, source)
}

/// Type a title, Enter, and a line, as the editor does.
fn typed(mut doc: Document) -> Document {
    let mut h = History::new();
    let mut apply = |doc: &mut Document, edit| h.apply(doc, edit);
    let at = Pos::new(0, 0);
    let edit = replace(&doc, at, at, "Plans", &document_edit::look(&doc, at)).unwrap();
    apply(&mut doc, edit);
    let edit = set_style(&doc, Container::Top, 0..1, ParagraphStyle::Heading(1)).unwrap();
    apply(&mut doc, edit);
    let end = Pos::new(0, text_len(&doc, Container::Top, 0).unwrap());
    let edit = split(&doc, end, end).unwrap();
    apply(&mut doc, edit);
    let at = Pos::new(1, 0);
    let edit = replace(&doc, at, at, "First line", &document_edit::look(&doc, at)).unwrap();
    apply(&mut doc, edit);
    doc
}

fn texts(blocks: &[Block]) -> Vec<(ParagraphStyle, String)> {
    blocks
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some((p.style, p.text())),
            _ => None,
        })
        .collect()
}

#[test]
fn the_blank_document_is_one_empty_normal_paragraph() {
    let (_, doc, _) = open_blank("empty");
    assert_eq!(
        texts(&doc.blocks),
        [(ParagraphStyle::Normal, String::new())]
    );
    assert_eq!(doc.blocks.len(), 1);
}

#[test]
fn typing_into_it_saves_a_word_document() {
    let (original, doc, source) = open_blank("typed");
    let doc = typed(doc);
    let dest = temp("typed", "plans.docx");
    docx::save(
        &dest,
        &original,
        &doc.blocks,
        &source,
        &AtomicBool::new(false),
    )
    .unwrap();
    let back = docx::read(&dest).unwrap();
    assert_eq!(
        texts(&back.blocks),
        [
            (ParagraphStyle::Heading(1), "Plans".to_owned()),
            (ParagraphStyle::Normal, "First line".to_owned()),
        ]
    );
}

/// `scripts/lo_docx.py dump FILE` through a LibreOffice listening on `port`.
fn lo_dump(file: &Path, port: u16) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../..");
    let out = std::process::Command::new("python3")
        .arg(root.join("scripts/lo_docx.py"))
        .args(["dump", &file.to_string_lossy()])
        .env("LO_PORT", port.to_string())
        .output()
        .expect("python3");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

#[test]
#[ignore = "needs LibreOffice: cargo test -p docx --test blank -- --ignored"]
fn libreoffice_reads_a_saved_new_document() {
    let (original, doc, source) = open_blank("libreoffice");
    let doc = typed(doc);
    let dest = temp("libreoffice", "plans.docx");
    docx::save(
        &dest,
        &original,
        &doc.blocks,
        &source,
        &AtomicBool::new(false),
    )
    .unwrap();
    let dump = lo_dump(&dest, 2183);
    let paras: Vec<&str> = dump.lines().filter(|l| l.starts_with("para\t")).collect();
    assert_eq!(
        paras,
        [
            "para\tHeading 1\t-\t\"\"\t\"Plans\"",
            "para\tDefault Paragraph Style\t-\t\"\"\t\"First line\"",
        ],
        "{dump}"
    );
}
