//! SLATE-26: a paragraph's own spacing and indents (ADR 0009). `w:spacing`, `w:ind`, and
//! `w:contextualSpacing` in a paragraph's `w:pPr` (a table cell's too) override its
//! style's, attribute by attribute; Word's `start`, `end`, and `hanging` read as left,
//! right, and a negative first line; automatic spacing is 14 pt. Saving keeps them: an
//! edited paragraph keeps its elements byte for byte, both halves of one split by Enter
//! keep them, and a paragraph whose own looks differ from its template's writes the
//! model's; LibreOffice reads every edited paragraph's as before (`-- --ignored`).
//! `tests/data/spacing.docx` is `python3 scripts/lo_docx.py spacing`'s.

use document_edit::{normalize, replace, split, text_len, toggle_list, Container, History, Pos};
use document_model::{Block, Document, LineSpacing, ParaOverrides, ParaSpacing, Paragraph};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

fn data() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/spacing.docx")
}

/// Every paragraph, list items' and table cells' included, in document order.
fn paragraphs(blocks: &[Block]) -> Vec<&Paragraph> {
    let mut out = Vec::new();
    for b in blocks {
        match b {
            Block::Paragraph(p) => out.push(p),
            Block::ListItem(item) => out.push(&item.paragraph),
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

/// The paragraph whose text starts with `start`.
fn para<'a>(doc: &'a Document, start: &str) -> &'a Paragraph {
    paragraphs(&doc.blocks)
        .into_iter()
        .find(|p| p.text().starts_with(start))
        .unwrap_or_else(|| panic!("no paragraph starts with {start:?}"))
}

/// The top-level block whose text starts with `start`.
fn block(doc: &Document, start: &str) -> usize {
    doc.blocks
        .iter()
        .position(|b| match b {
            Block::Paragraph(p) => p.text().starts_with(start),
            Block::ListItem(item) => item.paragraph.text().starts_with(start),
            _ => false,
        })
        .unwrap()
}

#[test]
fn own_spacing_and_indents_go_over_the_style() {
    let doc = docx::read(&data()).unwrap();
    let own = |start: &str| para(&doc, start).own;
    let spacing = |start: &str| doc.styles.spacing(para(&doc, start));
    let normal = spacing("Normal: the document defaults'");
    assert_eq!(
        normal,
        ParaSpacing {
            space_after: 160,
            line: LineSpacing::Auto(259.0 / 240.0),
            ..ParaSpacing::default()
        }
    );
    assert!(own("Normal: the document defaults'").is_empty());

    assert_eq!(
        own("Own space"),
        ParaOverrides {
            space_before: Some(120),
            space_after: Some(0),
            ..ParaOverrides::default()
        }
    );
    assert_eq!(own("Own double").line, Some(LineSpacing::Auto(2.0)));
    assert_eq!(own("Own exact").line, Some(LineSpacing::Exact(300)));
    assert_eq!(
        own("Own lines at least").line,
        Some(LineSpacing::AtLeast(400))
    );
    assert_eq!(
        own("A line without its rule").line,
        Some(LineSpacing::Auto(1.5))
    );
    assert_eq!(
        own("Start, end, and hanging"),
        ParaOverrides {
            indent_left: Some(720),
            indent_right: Some(1080),
            indent_first: Some(-540),
            ..ParaOverrides::default()
        }
    );
    let negative = spacing("Negative indents");
    assert_eq!((negative.indent_left, negative.indent_right), (-360, -720));

    // A direct value overrides only what it sets: the style's left and right stay.
    assert_eq!(
        spacing("Spaced, with no space before"),
        ParaSpacing {
            space_before: 0,
            space_after: 120,
            line: LineSpacing::Auto(1.5),
            indent_left: 720,
            indent_right: 360,
            indent_first: 0,
            contextual: false,
        }
    );
    let overridden = spacing("Spaced, overridden");
    assert_eq!(
        (
            overridden.space_after,
            overridden.line,
            overridden.indent_left
        ),
        (480, LineSpacing::Auto(1.0), 0)
    );

    // Contextual spacing from the style, turned off by the paragraph, or its own.
    assert!(spacing("Tight, contextual").contextual);
    assert_eq!(own("Tight, contextual").contextual, None);
    assert!(!spacing("Tight with contextual spacing turned off").contextual);
    assert!(spacing("Normal with its own contextual").contextual);

    let auto = spacing("Automatic spacing");
    assert_eq!((auto.space_before, auto.space_after), (280, 280));

    let item = own("A list item with its own indents");
    assert_eq!(
        (item.indent_left, item.indent_first, item.space_after),
        (Some(1080), Some(-360), Some(0))
    );
    let cell = own("A cell paragraph");
    assert_eq!((cell.indent_left, cell.space_after), (Some(200), Some(0)));
}

/// A file `name` in a directory of test `test`'s own.
fn temp(test: &str, name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join(format!("docx-spacing-{}", std::process::id()))
        .join(test);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn part(path: &Path, name: &str) -> String {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut s = String::new();
    zip.by_name(name).unwrap().read_to_string(&mut s).unwrap();
    s
}

/// The spacing document copied for test `test`, opened as Slate opens it.
fn opened(test: &str) -> (PathBuf, Document, docx::DocxSource) {
    let path = temp(test, "spacing.docx");
    std::fs::copy(data(), &path).unwrap();
    let (mut doc, mut source) = docx::read_source(&path).unwrap();
    normalize(&mut doc);
    source.set_blocks(doc.blocks.clone());
    (path, doc, source)
}

fn save(path: &Path, test: &str, doc: &Document, source: &docx::DocxSource) -> PathBuf {
    let dest = temp(test, "saved.docx");
    docx::save(&dest, path, &doc.blocks, source, &AtomicBool::new(false)).unwrap();
    dest
}

const HANGING: &str = "<w:ind w:start=\"720\" w:end=\"1080\" w:hanging=\"540\"/>";
const AUTOMATIC: &str = "<w:spacing w:before=\"100\" w:beforeAutospacing=\"1\" w:after=\"100\" \
                         w:afterAutospacing=\"1\"/>";

#[test]
fn an_edited_paragraph_keeps_its_own_spacing_and_indents() {
    let (path, mut doc, source) = opened("edited");
    let original = part(&path, "word/document.xml");
    assert_eq!(original.matches(HANGING).count(), 1);
    assert_eq!(original.matches(AUTOMATIC).count(), 1);
    let hanging = para(&doc, "Start, end, and hanging").own;
    let automatic = para(&doc, "Automatic spacing").own;

    let mut history = History::new();
    let i = block(&doc, "Automatic spacing");
    let at = Pos::new(i, text_len(&doc, Container::Top, i).unwrap());
    let edit = replace(&doc, at, at, " Edited", &document_edit::look(&doc, at)).unwrap();
    history.apply(&mut doc, edit);
    // Enter after "Start,": both halves keep the paragraph's own looks, as in Word.
    let i = block(&doc, "Start, end, and hanging");
    let edit = split(&doc, Pos::new(i, 6), Pos::new(i, 6)).unwrap();
    history.apply(&mut doc, edit);
    assert_eq!(para(&doc, " end, and hanging").own, hanging);
    assert_eq!(para(&doc, "Start,").own, hanging);

    let dest = save(&path, "edited", &doc, &source);
    let xml = part(&dest, "word/document.xml");
    // Written as they were: Word's `start` and `end`, and automatic spacing's `before`
    // and `after`, which the model doesn't hold.
    assert_eq!(xml.matches(HANGING).count(), 2, "{xml}");
    assert_eq!(xml.matches(AUTOMATIC).count(), 1, "{xml}");
    let back = docx::read(&dest).unwrap();
    assert_eq!(para(&back, "Start,").own, hanging);
    assert_eq!(para(&back, " end, and hanging").own, hanging);
    assert_eq!(
        para(&back, "Automatic spacing: 14 pt before and after. Edited").own,
        automatic
    );
    // Every other paragraph reads back as it was.
    let (a, b) = (paragraphs(&doc.blocks), paragraphs(&back.blocks));
    assert_eq!(a.len(), b.len());
    for (a, b) in a.iter().zip(&b) {
        assert_eq!((a.text(), a.own), (b.text(), b.own));
    }
}

#[test]
fn a_paragraph_whose_own_looks_differ_from_its_templates_writes_them() {
    let (path, mut doc, source) = opened("differ");
    let i = block(&doc, "Start, end, and hanging");
    let mine = ParaOverrides {
        space_before: Some(60),
        space_after: None,
        line: Some(LineSpacing::Exact(300)),
        indent_left: Some(360),
        indent_right: Some(0),
        indent_first: Some(-200),
        contextual: Some(true),
    };
    // Spacing isn't edited yet: a paragraph pasted from elsewhere stands for one.
    let Block::Paragraph(p) = &mut doc.blocks[i] else {
        panic!("a paragraph");
    };
    p.own = mine;
    p.runs[0].text.push('!');
    let i = block(&doc, "Own space");
    let Block::Paragraph(p) = &mut doc.blocks[i] else {
        panic!("a paragraph");
    };
    p.own = ParaOverrides {
        line: Some(LineSpacing::Auto(1.15)),
        ..ParaOverrides::default()
    };
    p.runs[0].text.push('!');

    let dest = save(&path, "differ", &doc, &source);
    let xml = part(&dest, "word/document.xml");
    for want in [
        "<w:spacing w:before=\"60\" w:line=\"300\" w:lineRule=\"exact\"/>\
         <w:ind w:left=\"360\" w:right=\"0\" w:hanging=\"200\"/><w:contextualSpacing/>",
        "<w:spacing w:line=\"276\" w:lineRule=\"auto\"/>",
    ] {
        assert!(xml.contains(want), "{want} in {xml}");
    }
    assert!(!xml.contains(HANGING), "{xml}");
    let back = docx::read(&dest).unwrap();
    assert_eq!(para(&back, "Start, end").own, mine);
    assert_eq!(
        para(&back, "Own space").own.line,
        Some(LineSpacing::Auto(276.0 / 240.0))
    );
}

#[test]
fn a_paragraph_joining_a_list_leaves_its_own_indents_to_the_level() {
    let (path, mut doc, source) = opened("list");
    let i = block(&doc, "Start, end, and hanging");
    let mut history = History::new();
    let edit = toggle_list(&doc, Container::Top, i..i + 1, true).unwrap();
    history.apply(&mut doc, edit);
    let item = para(&doc, "Start, end, and hanging");
    assert_eq!(item.own, ParaOverrides::default());
    // Its spacing stays.
    let i = block(&doc, "Own space");
    let edit = toggle_list(&doc, Container::Top, i..i + 1, false).unwrap();
    history.apply(&mut doc, edit);
    assert_eq!(para(&doc, "Own space").own.space_before, Some(120));

    let dest = save(&path, "list", &doc, &source);
    let xml = part(&dest, "word/document.xml");
    assert!(!xml.contains(HANGING), "{xml}");
    let back = docx::read(&dest).unwrap();
    assert!(matches!(
        &back.blocks[block(&back, "Start, end, and hanging")],
        Block::ListItem(item) if item.paragraph.own.is_empty()
    ));
    assert_eq!(para(&back, "Own space").own.space_before, Some(120));
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
#[ignore = "needs LibreOffice: cargo test -p docx --test spacing -- --ignored"]
fn libreoffice_reads_every_edited_paragraphs_own_looks_as_before() {
    let (path, mut doc, source) = opened("libreoffice");
    // " Edited" after every paragraph and list item outside the table.
    let mut history = History::new();
    for i in 0..doc.blocks.len() {
        if matches!(doc.blocks[i], Block::Table(_)) {
            continue;
        }
        let at = Pos::new(i, text_len(&doc, Container::Top, i).unwrap());
        let edit = replace(&doc, at, at, " Edited", &document_edit::look(&doc, at)).unwrap();
        history.apply(&mut doc, edit);
    }
    let dest = save(&path, "libreoffice", &doc, &source);
    let (before, after) = (lo_dump(&path, 2184), lo_dump(&dest, 2184));
    let mut want = Vec::new();
    let mut in_table = false;
    for line in before.lines() {
        match line {
            "table" => in_table = true,
            "end" => in_table = false,
            _ => {}
        }
        if !in_table && (line.starts_with("para\t") || line.starts_with("run\t")) {
            let text = line.strip_suffix('"').unwrap();
            want.push(format!("{text} Edited\""));
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
