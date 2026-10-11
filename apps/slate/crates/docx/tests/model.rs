//! SLATE-3: what LibreOffice's text dump can't show: pictures with their bytes, size, and
//! alt text, links, font size and colour, alignment, page breaks, merged cells, and
//! errors for files that aren't Word documents.

use document_model::{Align, Block, ImageFormat, ImageSource, Paragraph, Run};
use std::path::{Path, PathBuf};

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

fn paragraphs(blocks: &[Block]) -> Vec<&Paragraph> {
    let mut out = Vec::new();
    for b in blocks {
        match b {
            Block::Paragraph(p) => out.push(p),
            Block::ListItem(i) => out.push(&i.paragraph),
            Block::Table(t) => {
                for cell in t.rows.iter().flat_map(|r| &r.cells) {
                    out.extend(paragraphs(&cell.blocks));
                }
            }
            _ => {}
        }
    }
    out
}

fn run<'a>(blocks: &'a [Block], text: &str) -> &'a Run {
    paragraphs(blocks)
        .into_iter()
        .flat_map(|p| &p.runs)
        .find(|r| r.text == text)
        .unwrap_or_else(|| panic!("no run {text:?}"))
}

fn paragraph<'a>(blocks: &'a [Block], text: &str) -> &'a Paragraph {
    paragraphs(blocks)
        .into_iter()
        .find(|p| p.text() == text)
        .unwrap_or_else(|| panic!("no paragraph {text:?}"))
}

#[test]
fn fixture_runs_carry_size_colour_and_links() {
    let doc = docx::read(&data("fixture.docx")).unwrap();
    assert_eq!(run(&doc.blocks, "big").style.size, Some(36), "18 pt");
    assert_eq!(run(&doc.blocks, "red").style.color, Some([0xC0, 0, 0]));
    assert_eq!(
        run(&doc.blocks, "the Slate page").style.link.as_deref(),
        Some("https://example.com/slate")
    );
    for (text, align) in [
        ("A centred paragraph.", Align::Center),
        ("A right-aligned paragraph.", Align::End),
        ("The end.", Align::Start),
    ] {
        assert_eq!(paragraph(&doc.blocks, text).align, align, "{text}");
    }
    assert_eq!(
        paragraph(&doc.blocks, "A justified paragraph that is long enough to wrap onto a second line in a normal page width, so that the justification shows on its first line.").align,
        Align::Justify
    );
}

#[test]
fn fixture_picture_page_break_and_merged_cell() {
    let doc = docx::read(&data("fixture.docx")).unwrap();
    let images: Vec<_> = doc
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Image(i) => Some(i),
            _ => None,
        })
        .collect();
    assert_eq!(images.len(), 1);
    let image = images[0];
    assert_eq!(image.alt, "A blue picture");
    let ImageSource::Embedded { format, bytes } = &image.source else {
        panic!("not embedded");
    };
    assert_eq!(*format, ImageFormat::Png);
    let picture = std::fs::read(data("../../../document-model/tests/data/picture.png")).unwrap();
    assert_eq!(
        bytes.as_ref(),
        picture.as_slice(),
        "the picture's own bytes"
    );
    let (w, h) = image.size.unwrap();
    assert!((w - 120.0).abs() < 0.5 && (h - 80.0).abs() < 0.5, "{w}x{h}");

    let at = |text: &str| {
        doc.blocks
            .iter()
            .position(|b| matches!(b, Block::Paragraph(p) if p.text() == text))
            .unwrap()
    };
    let (before, after) = (at("After the table."), at("After a page break."));
    assert_eq!(after, before + 2);
    assert_eq!(doc.blocks[before + 1], Block::PageBreak);

    let table = doc
        .blocks
        .iter()
        .find_map(|b| match b {
            Block::Table(t) => Some(t),
            _ => None,
        })
        .unwrap();
    let spans: Vec<Vec<u32>> = table
        .rows
        .iter()
        .map(|r| r.cells.iter().map(|c| c.span).collect())
        .collect();
    assert_eq!(spans, [vec![1, 1, 1], vec![1, 1, 1], vec![2, 1]]);
}

#[test]
fn a_word_gif_is_a_picture() {
    let doc = docx::read(&data("poi/WithGIF.docx")).unwrap();
    let formats: Vec<_> = doc
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Image(i) => match &i.source {
                ImageSource::Embedded { format, .. } => Some(*format),
                ImageSource::Link(_) => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(formats, [ImageFormat::Gif]);
}

#[test]
fn files_that_are_not_word_documents_are_refused() {
    let dir = std::env::temp_dir().join(format!("slate-docx-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let text = dir.join("text.docx");
    std::fs::write(&text, "just text").unwrap();
    assert!(matches!(
        docx::read(&text),
        Err(docx::DocxError::Package(_))
    ));

    let other = dir.join("other.docx");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&other).unwrap());
    zip.start_file("hello.txt", zip::write::SimpleFileOptions::default())
        .unwrap();
    std::io::Write::write_all(&mut zip, b"hi").unwrap();
    zip.finish().unwrap();
    assert!(matches!(docx::read(&other), Err(docx::DocxError::NotDocx)));
    std::fs::remove_dir_all(&dir).unwrap();
}
