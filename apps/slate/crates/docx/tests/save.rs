//! SLATE-8: saving a Word document patches the package it was read from. An edited
//! document keeps every part but `word/document.xml` byte for byte, and in it every
//! paragraph and table not edited; it reads back with the edits. LibreOffice's reading of
//! the saved document: the ignored test at the end.

use document_edit::{
    delete, normalize, paste, replace, set_style, split, text_len, toggle, toggle_list, Container,
    Edit, Flag, Fragment, History, Pos,
};
use document_model::{Block, Document, ImageSource, ListKind, Paragraph, ParagraphStyle, RunStyle};
use docx::DocxSource;
use quick_xml::events::Event;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

/// The Word documents of the test corpus.
fn corpus() -> Vec<PathBuf> {
    let mut out = vec![data("fixture.docx")];
    let mut poi: Vec<PathBuf> = std::fs::read_dir(data("poi"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "docx"))
        .collect();
    poi.sort();
    out.extend(poi);
    out
}

/// A file `name` in a directory of test `test`'s own.
fn temp(test: &str, name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join(format!("docx-save-{}", std::process::id()))
        .join(test);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// Read a document as Slate opens it: runs merged, the source describing them.
fn open(path: &Path) -> (Document, DocxSource) {
    let (mut doc, mut source) = docx::read_source(path).unwrap();
    normalize(&mut doc);
    source.set_blocks(doc.blocks.clone());
    (doc, source)
}

fn save(dest: &Path, original: &Path, doc: &Document, source: &DocxSource) -> DocxSource {
    docx::save(dest, original, &doc.blocks, source, &AtomicBool::new(false)).unwrap()
}

/// The document at `path` as Slate shows it.
fn reread(path: &Path) -> Vec<Block> {
    open(path).0.blocks
}

/// Make an edit of the document as it is, and apply it.
fn apply(doc: &mut Document, history: &mut History, edit: impl FnOnce(&Document) -> Option<Edit>) {
    let edit = edit(doc).expect("an edit");
    history.apply(doc, edit);
}

/// The top-level paragraph or list item with text `text`.
fn top(doc: &Document, text: &str) -> usize {
    doc.blocks
        .iter()
        .position(|b| match b {
            Block::Paragraph(p) => p.text() == text,
            Block::ListItem(i) => i.paragraph.text() == text,
            _ => false,
        })
        .unwrap_or_else(|| panic!("no paragraph {text:?}"))
}

fn end(doc: &Document, block: usize) -> Pos {
    Pos::new(block, text_len(doc, Container::Top, block).unwrap())
}

/// Type `text` at `at`, in the look there.
fn typing(doc: &Document, at: Pos, text: &str) -> Option<Edit> {
    replace(doc, at, at, text, &document_edit::look(doc, at))
}

/// Every part of a package, in order, uncompressed.
fn parts(path: &Path) -> Vec<(String, Vec<u8>)> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    (0..zip.len())
        .map(|i| {
            let mut f = zip.by_index(i).unwrap();
            let mut bytes = Vec::new();
            f.read_to_end(&mut bytes).unwrap();
            (f.name().to_owned(), bytes)
        })
        .collect()
}

fn part(path: &Path, name: &str) -> Option<String> {
    parts(path)
        .into_iter()
        .find(|(n, _)| n == name)
        .map(|(_, b)| String::from_utf8(b).unwrap())
}

/// The parts of `saved` not named in `changed` are `original`'s, in the same order; the
/// parts in `added` are new, after them.
fn untouched_but(original: &Path, saved: &Path, changed: &[&str], added: &[&str]) {
    let (before, after) = (parts(original), parts(saved));
    let names = |p: &[(String, Vec<u8>)]| p.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>();
    let mut want = names(&before);
    want.extend(added.iter().map(|s| s.to_string()));
    assert_eq!(names(&after), want, "{}", saved.display());
    for ((name, a), (_, b)) in before.iter().zip(&after) {
        if changed.contains(&name.as_str()) {
            assert_ne!(a, b, "{name} should have changed");
        } else {
            assert!(a == b, "{name} changed in {}", saved.display());
        }
    }
}

/// The markup of each child of `word/document.xml`'s body.
fn body_children(xml: &str) -> Vec<String> {
    let mut r = quick_xml::Reader::from_str(xml);
    let mut out = Vec::new();
    let mut depth = 0;
    loop {
        let before = r.buffer_position() as usize;
        match r.read_event().unwrap() {
            Event::Start(e) if depth == 2 => {
                let name = e.name().as_ref().to_vec();
                r.read_to_end(quick_xml::name::QName(&name)).unwrap();
                out.push(xml[before..r.buffer_position() as usize].to_owned());
            }
            Event::Empty(_) if depth == 2 => {
                out.push(xml[before..r.buffer_position() as usize].to_owned());
            }
            Event::Start(_) => depth += 1,
            Event::End(_) => depth -= 1,
            Event::Eof => return out,
            _ => {}
        }
    }
}

/// A copy of fixture.docx whose body is `body`.
fn with_body(test: &str, body: &str) -> PathBuf {
    let path = temp(test, "source.docx");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    for (name, bytes) in parts(&data("fixture.docx")) {
        zip.start_file(&name, options).unwrap();
        if name == "word/document.xml" {
            let xml = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document \
                 xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
                 xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">\
                 <w:body>{body}</w:body></w:document>"
            );
            zip.write_all(xml.as_bytes()).unwrap();
        } else {
            zip.write_all(&bytes).unwrap();
        }
    }
    zip.finish().unwrap();
    path
}

#[test]
fn an_unedited_document_saves_as_the_package_it_was() {
    for path in corpus() {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let (doc, source) = open(&path);
        let dest = temp("unedited", &name);
        let again = save(&dest, &path, &doc, &source);
        untouched_but(&path, &dest, &[], &[]);
        // And again from the copy, as the app does.
        let next = temp("unedited", &format!("again-{name}"));
        save(&next, &dest, &doc, &again);
        untouched_but(&path, &next, &[], &[]);
    }
}

/// fixture.docx with an edit of each kind: typing, italic, a heading style the document
/// doesn't have, Enter in a numbered item, typing in a table cell, and a paragraph deleted.
fn edited() -> (Document, DocxSource) {
    let (mut doc, source) = open(&data("fixture.docx"));
    let mut h = History::new();
    let i = top(&doc, "A centred paragraph.");
    apply(&mut doc, &mut h, |d| typing(d, end(d, i), " Edited"));
    let i = top(&doc, "A quotation from somebody wise.");
    apply(&mut doc, &mut h, |d| {
        toggle(d, Pos::new(i, 2), Pos::new(i, 11), Flag::Italic)
    });
    let i = top(&doc, "A right-aligned paragraph.");
    apply(&mut doc, &mut h, |d| {
        set_style(d, Container::Top, i..i + 1, ParagraphStyle::Heading(4))
    });
    let i = top(&doc, "First number");
    apply(&mut doc, &mut h, |d| {
        split(d, Pos::new(i, 5), Pos::new(i, 5))
    });
    let t = doc
        .blocks
        .iter()
        .position(|b| matches!(b, Block::Table(_)))
        .unwrap();
    let (from, to) = (Pos::in_cell(t, 1, 0, 0, 0), Pos::in_cell(t, 1, 0, 0, 6));
    apply(&mut doc, &mut h, |d| {
        replace(d, from, to, "Pears", &document_edit::look(d, from))
    });
    let i = top(&doc, "The table follows.");
    apply(&mut doc, &mut h, |d| delete(d, end(d, i - 1), end(d, i)));
    (doc, source)
}

#[test]
fn edits_change_only_their_paragraphs() {
    let (doc, source) = edited();
    let original = data("fixture.docx");
    let dest = temp("edits", "saved.docx");
    save(&dest, &original, &doc, &source);
    // The heading style is new to the document.
    untouched_but(
        &original,
        &dest,
        &["word/document.xml", "word/styles.xml"],
        &[],
    );
    let (old, new) = (
        part(&original, "word/styles.xml").unwrap(),
        part(&dest, "word/styles.xml").unwrap(),
    );
    let at = old.rfind("</w:styles>").unwrap();
    assert!(new.starts_with(&old[..at]) && new.ends_with(&old[at..]));
    assert!(
        new[at..].contains("<w:name w:val=\"heading 4\"/>"),
        "{}",
        &new[at..]
    );

    // Every body child not edited is still there, as it was, in order; the paragraph
    // deleted is gone and the item split is two.
    let before = body_children(&part(&original, "word/document.xml").unwrap());
    let after = body_children(&part(&dest, "word/document.xml").unwrap());
    let edited = [
        "A centred paragraph.",
        "quotation",
        "A right-aligned paragraph.",
        "First number",
        "Apples",
        "The table follows.",
    ];
    let kept: Vec<&String> = before
        .iter()
        .filter(|x| !edited.iter().any(|e| x.contains(e)))
        .collect();
    let mut rest = after.iter();
    for x in &kept {
        assert!(rest.any(|y| y == *x), "not kept as it was: {x}");
    }
    assert_eq!(after.len(), before.len());
    assert_eq!(kept.len(), before.len() - edited.len());

    // It reads back as edited; the new heading looks as its style does.
    let back = reread(&dest);
    assert_eq!(back.len(), doc.blocks.len());
    let heading = top(&doc, "A right-aligned paragraph.");
    for (i, (a, b)) in doc.blocks.iter().zip(&back).enumerate() {
        if i == heading {
            let Block::Paragraph(p) = b else {
                panic!("{b:?}")
            };
            assert_eq!(p.style, ParagraphStyle::Heading(4));
            assert_eq!(p.align, document_model::Align::End);
            assert_eq!(p.text(), "A right-aligned paragraph.");
            assert!(p.runs.iter().all(|r| r.style.bold), "{p:?}");
        } else {
            assert_eq!(a, b, "block {i}");
        }
    }
}

#[test]
fn a_second_save_patches_the_first() {
    let (mut doc, source) = edited();
    let first = temp("second", "first.docx");
    let source = save(&first, &data("fixture.docx"), &doc, &source);
    let mut h = History::new();
    let i = top(&doc, "Second heading");
    apply(&mut doc, &mut h, |d| typing(d, end(d, i), " again"));
    let second = temp("second", "second.docx");
    save(&second, &first, &doc, &source);
    untouched_but(&first, &second, &["word/document.xml"], &[]);
    let (a, b) = (
        body_children(&part(&first, "word/document.xml").unwrap()),
        body_children(&part(&second, "word/document.xml").unwrap()),
    );
    let differ: Vec<usize> = (0..a.len()).filter(|&i| a[i] != b[i]).collect();
    assert_eq!(differ.len(), 1, "only the paragraph typed in");
    assert!(b[differ[0]].contains("Second heading again"));
    assert_eq!(top(&open(&second).0, "Second heading again"), i);
}

#[test]
fn every_paragraph_of_the_corpus_takes_an_edit() {
    for path in corpus() {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let (mut doc, source) = open(&path);
        let mut h = History::new();
        let mut typed = 0;
        for i in 0..doc.blocks.len() {
            if matches!(doc.blocks[i], Block::Paragraph(_) | Block::ListItem(_)) {
                apply(&mut doc, &mut h, |d| typing(d, end(d, i), " ✎"));
                typed += 1;
            }
            if let Block::Table(t) = &doc.blocks[i] {
                let cells: Vec<(usize, usize)> = t
                    .rows
                    .iter()
                    .enumerate()
                    .flat_map(|(r, row)| (0..row.cells.len()).map(move |c| (r, c)))
                    .collect();
                for (r, c) in cells {
                    let at = Container::Cell {
                        table: i,
                        row: r,
                        cell: c,
                    };
                    if let Some(len) = text_len(&doc, at, 0) {
                        let pos = Pos::in_cell(i, r, c, 0, len);
                        apply(&mut doc, &mut h, |d| typing(d, pos, " ✎"));
                        typed += 1;
                    }
                }
            }
        }
        let dest = temp("corpus", &name);
        save(&dest, &path, &doc, &source);
        untouched_but(&path, &dest, &["word/document.xml"], &[]);
        let back = reread(&dest);
        let marks = |blocks: &[Block]| {
            let mut n = 0;
            Document {
                blocks: blocks.to_vec(),
                format: document_model::Format::Docx,
            }
            .visit_text(&mut |t| n += t.matches(" ✎").count());
            n
        };
        assert_eq!(marks(&back), typed, "{name}");
        assert_eq!(marks(&back), marks(&doc.blocks), "{name}");
    }
}

#[test]
fn a_section_break_stays_with_the_paragraph_that_ends_it() {
    let path = with_body(
        "section",
        "<w:p><w:r><w:t>Section one</w:t></w:r></w:p>\
         <w:p><w:pPr><w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/></w:sectPr></w:pPr>\
         <w:r><w:t>End of one</w:t></w:r></w:p>\
         <w:p><w:r><w:t>Section two</w:t></w:r></w:p>\
         <w:sectPr><w:pgSz w:w=\"16838\" w:h=\"11906\" w:orient=\"landscape\"/></w:sectPr>",
    );
    let (mut doc, source) = open(&path);
    assert!(matches!(doc.blocks[2], Block::PageBreak));
    let mut h = History::new();
    // Enter inside the paragraph that ends the section, then typing in its second half.
    let i = top(&doc, "End of one");
    apply(&mut doc, &mut h, |d| {
        split(d, Pos::new(i, 3), Pos::new(i, 3))
    });
    apply(&mut doc, &mut h, |d| typing(d, end(d, i + 1), "!"));
    let dest = temp("section", "saved.docx");
    save(&dest, &path, &doc, &source);
    let xml = part(&dest, "word/document.xml").unwrap();
    let children = body_children(&xml);
    assert_eq!(children.len(), 5, "{children:#?}");
    assert!(!children[1].contains("sectPr"), "{}", children[1]);
    assert!(
        children[2].contains("<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/></w:sectPr>")
            && children[2].contains(" of one!"),
        "{}",
        children[2]
    );
    assert_eq!(xml.matches("<w:sectPr").count(), 2);
    assert_eq!(reread(&dest), doc.blocks);
}

#[test]
fn a_list_in_a_document_without_numbering_gets_numbering() {
    let path = data("poi/SampleDoc.docx");
    let (mut doc, source) = open(&path);
    let mut h = History::new();
    apply(&mut doc, &mut h, |d| {
        toggle_list(d, Container::Top, 0..2, true)
    });
    apply(&mut doc, &mut h, |d| {
        toggle_list(d, Container::Top, 3..4, false)
    });
    let dest = temp("numbering", "saved.docx");
    save(&dest, &path, &doc, &source);
    untouched_but(
        &path,
        &dest,
        &[
            "[Content_Types].xml",
            "word/_rels/document.xml.rels",
            "word/document.xml",
        ],
        &["word/numbering.xml"],
    );
    let back = reread(&dest);
    let kinds: Vec<Option<ListKind>> = back[..4]
        .iter()
        .map(|b| match b {
            Block::ListItem(i) => Some(i.kind.clone()),
            _ => None,
        })
        .collect();
    let ordered = |n: u64| {
        Some(ListKind::Ordered {
            number: n,
            label: format!("{n}."),
        })
    };
    assert_eq!(
        kinds,
        [ordered(1), ordered(2), None, Some(ListKind::Bullet)]
    );
    assert_eq!(back, doc.blocks);
}

#[test]
fn new_links_and_pictures_get_relationships_and_parts() {
    let path = data("fixture.docx");
    let (mut doc, source) = open(&path);
    let gif = docx::read(&data("poi/WithGIF.docx"))
        .unwrap()
        .blocks
        .into_iter()
        .find(|b| matches!(b, Block::Image(_)))
        .unwrap();
    let mut h = History::new();
    // A link pasted at the end of the paragraph after "Second heading", in its look.
    let i = top(&doc, "Second heading");
    let at = end(&doc, i + 1);
    let mut link = Paragraph::default();
    link.push(
        "a new link",
        &RunStyle {
            link: Some("https://example.org/new?a=1&b=2".into()),
            ..document_edit::look(&doc, at)
        },
    );
    let fragment = Fragment {
        blocks: vec![Block::Paragraph(link)],
    };
    apply(&mut doc, &mut h, |d| paste(d, at, at, &fragment));
    let i = top(&doc, "A centred paragraph.");
    let at = end(&doc, i);
    let fragment = Fragment {
        blocks: vec![
            Block::Paragraph(Paragraph::default()),
            gif.clone(),
            Block::Paragraph(Paragraph::default()),
        ],
    };
    apply(&mut doc, &mut h, |d| paste(d, at, at, &fragment));
    let dest = temp("links", "saved.docx");
    save(&dest, &path, &doc, &source);
    untouched_but(
        &path,
        &dest,
        &[
            "[Content_Types].xml",
            "word/_rels/document.xml.rels",
            "word/document.xml",
        ],
        &["word/media/slate-image1.gif"],
    );
    let rels = part(&dest, "word/_rels/document.xml.rels").unwrap();
    assert!(
        rels.contains("Target=\"https://example.org/new?a=1&amp;b=2\" TargetMode=\"External\"")
            && rels.contains("Target=\"media/slate-image1.gif\""),
        "{rels}"
    );
    let types = part(&dest, "[Content_Types].xml").unwrap();
    assert!(types.contains("<Default Extension=\"gif\" ContentType=\"image/gif\"/>"));
    let back = reread(&dest);
    assert_eq!(back, doc.blocks);
    let Block::Image(image) = &gif else {
        unreachable!()
    };
    assert!(back
        .iter()
        .any(|b| matches!(b, Block::Image(i) if i.source == image.source)));
    assert!(matches!(image.source, ImageSource::Embedded { .. }));
}

/// `scripts/lo_docx.py dump FILE`: LibreOffice's reading of a document, through a
/// LibreOffice listening on `port`.
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
#[ignore = "needs LibreOffice: cargo test -p docx --test save -- --ignored"]
fn libreoffice_reads_the_edits_and_everything_else_as_before() {
    let (doc, source) = edited();
    // LibreOffice locks a document it opens: dump a copy.
    let original = temp("libreoffice", "fixture.docx");
    std::fs::copy(data("fixture.docx"), &original).unwrap();
    let dest = temp("libreoffice", "saved.docx");
    save(&dest, &original, &doc, &source);
    let (before, after) = (lo_dump(&original, 2181), lo_dump(&dest, 2181));
    // The dump of the original with the edits made by hand.
    let para = |style: &str, list: &str, label: &str, text: &str| {
        format!("para\t{style}\t{list}\t\"{label}\"\t\"{text}\"")
    };
    let normal = "Default Paragraph Style";
    let mut want: Vec<String> = Vec::new();
    let mut lines = before.lines().peekable();
    while let Some(line) = lines.next() {
        let text = |t: &str| line.ends_with(&format!("\"{t}\""));
        if line.starts_with("para\t") && text("A centred paragraph.") {
            want.push(para(normal, "-", "", "A centred paragraph. Edited"));
            want.push("run\t-\t\"A centred paragraph. Edited\"".into());
            lines.next();
        } else if line.starts_with("run\t") && text("A quotation from somebody wise.") {
            want.push("run\t-\t\"A \"".into());
            want.push("run\ti\t\"quotation\"".into());
            want.push("run\t-\t\" from somebody wise.\"".into());
        } else if line.starts_with("para\t") && text("A right-aligned paragraph.") {
            want.push(para("Heading 4", "-", "", "A right-aligned paragraph."));
            want.push("run\tb\t\"A right-aligned paragraph.\"".into());
            lines.next();
        } else if line.starts_with("para\t") && text("First number") {
            want.push(para(normal, "0", "1.", "First"));
            want.push("run\t-\t\"First\"".into());
            want.push(para(normal, "0", "2.", " number"));
            want.push("run\t-\t\" number\"".into());
            lines.next();
        } else if line.starts_with("para\t") && text("Second number") {
            want.push(para(normal, "0", "3.", "Second number"));
        } else if line.starts_with("para\t") && text("Third number") {
            want.push(para(normal, "0", "4.", "Third number"));
        } else if text("Apples") {
            want.push(line.replace("Apples", "Pears"));
        } else if line.starts_with("para\t") && text("The table follows.") {
            lines.next();
        } else {
            want.push(line.to_owned());
        }
    }
    let got: Vec<&str> = after.lines().collect();
    for (n, (w, g)) in want.iter().zip(&got).enumerate() {
        assert_eq!(w, g, "line {}", n + 1);
    }
    assert_eq!(want.len(), got.len());
}
