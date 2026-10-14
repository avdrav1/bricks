//! SLATE-27: a paragraph's borders and shading (ADR 0009). `w:pBdr` and `w:shd` in a
//! paragraph's `w:pPr` (a table cell's too) go over its style's, side by side: a `nil` side
//! removes the style's, an automatic fill its shading; styles merge them along `basedOn`.
//! Word's line styles read as the nearest the model draws, as LibreOffice reads them.
//! Saving keeps them: an edited paragraph keeps its elements byte for byte, both halves of
//! one split by Enter keep them, and a paragraph whose own borders or shading differ from
//! its template's writes the model's; LibreOffice reads every edited paragraph's as before
//! (`-- --ignored`). `tests/data/borders.docx` is `python3 scripts/lo_docx.py borders`'s.

use document_edit::{normalize, replace, split, text_len, Container, History, Pos};
use document_model::{
    Block, Border, BorderOverrides, BorderStyle, Document, ParaOverrides, Paragraph, Rgb, Shading,
};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

fn data() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/borders.docx")
}

/// Every paragraph, table cells' included, in document order.
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
        .position(|b| matches!(b, Block::Paragraph(p) if p.text().starts_with(start)))
        .unwrap()
}

fn line(style: BorderStyle, width: u16, space: u16, color: Option<Rgb>) -> Border {
    Border {
        style,
        width,
        space,
        color,
    }
}

const BLUE: Option<Rgb> = Some([0x2F, 0x54, 0x96]);
const RED: Option<Rgb> = Some([0xC0, 0, 0]);

#[test]
fn borders_and_shading_go_over_the_style() {
    let doc = docx::read(&data()).unwrap();
    let s = &doc.styles;
    let borders = |start: &str| s.borders(para(&doc, start));
    let fill = |start: &str| s.shading(para(&doc, start)).fill;

    // The résumé's title rule, the paragraph's own.
    let title = para(&doc, "Jane Q. Writer");
    assert_eq!(
        title.own.borders,
        BorderOverrides {
            bottom: Some(line(BorderStyle::Single, 4, 1, Some([0; 3]))),
            ..BorderOverrides::default()
        }
    );
    // Google Docs' nil sides and automatic fill: set, but nothing to draw.
    let nil = para(&doc, "Nil borders");
    assert_eq!(nil.own.borders.top, Some(Border::default()));
    assert_eq!(nil.own.shading, Some(Shading { fill: None }));
    assert!(!borders("Nil borders").any());
    assert_eq!(fill("Nil borders"), None);

    let boxed = borders("Boxed on all four sides");
    let auto = line(BorderStyle::Single, 4, 4, None);
    assert_eq!(
        (boxed.top, boxed.left, boxed.bottom, boxed.right),
        (auto, auto, auto, auto)
    );
    assert!(!boxed.between.is_drawn());
    assert_eq!(
        borders("A double rule below").bottom,
        line(BorderStyle::Double, 6, 2, RED)
    );
    assert_eq!(
        borders("A dotted rule above").top,
        line(BorderStyle::Dotted, 12, 1, Some([0, 0xB0, 0x50]))
    );
    let dashed = borders("Dashed left and right");
    assert_eq!(
        dashed.left,
        line(BorderStyle::Dashed, 8, 8, Some([0x70, 0x30, 0xA0]))
    );
    assert_eq!(dashed.left, dashed.right);

    // Bordered together, with a red line between.
    let first = para(&doc, "The first of two boxed");
    let second = para(&doc, "The second of them");
    assert!(s.bordered_together(first, second));
    assert_eq!(
        s.borders(first).between,
        line(BorderStyle::Single, 4, 1, RED)
    );
    assert!(!s.bordered_together(second, para(&doc, "Shaded yellow")));

    assert_eq!(fill("Shaded yellow"), Some([0xFF, 0xE5, 0x99]));
    assert_eq!(
        fill("Solid shading in the pattern's"),
        Some([0xBD, 0xD7, 0xEE])
    );
    assert_eq!(fill("Solid shading in an automatic"), Some([0; 3]));
    assert_eq!(fill("A 25% pattern"), Some([0xBF; 3]));

    // The Boxed style's box and fill, the paragraph setting none.
    let style = para(&doc, "The Boxed style's");
    assert!(style.own.is_empty());
    let side = line(BorderStyle::Single, 8, 4, BLUE);
    let b = s.borders(style);
    assert_eq!((b.top, b.left, b.bottom, b.right), (side, side, side, side));
    assert_eq!(fill("The Boxed style's"), Some([0xDE, 0xEA, 0xF6]));
    assert!(s.bordered_together(style, para(&doc, "A second Boxed")));
    // Its own nil top and none bottom remove the style's; its sides stay; its automatic
    // fill removes the style's.
    let b = borders("Boxed, with its own nil top");
    assert!(!b.top.is_drawn() && !b.bottom.is_drawn());
    assert_eq!((b.left, b.right), (side, side));
    assert_eq!(fill("Boxed, with its own nil top"), None);
    // A style based on Boxed: its own double bottom over Boxed's box and fill.
    let b = borders("Boxed Double");
    assert_eq!(b.bottom, line(BorderStyle::Double, 4, 1, RED));
    assert_eq!((b.top, b.left), (side, side));
    assert_eq!(fill("Boxed Double"), Some([0xDE, 0xEA, 0xF6]));

    // Word's other styles as the nearest drawn.
    for (start, style) in [
        ("A thick rule", BorderStyle::Thick),
        ("A dot-dash rule", BorderStyle::Dashed),
        ("A triple rule", BorderStyle::Double),
        ("A wavy rule", BorderStyle::Single),
    ] {
        assert_eq!(borders(start).bottom.style, style, "{start}");
    }

    let cell = para(&doc, "A cell paragraph");
    assert_eq!(
        cell.own.borders.bottom,
        Some(line(BorderStyle::Single, 8, 1, Some([0x70, 0xAD, 0x47])))
    );
    assert_eq!(fill("A cell paragraph"), Some([0xE2, 0xEF, 0xDA]));
}

/// A file `name` in a directory of test `test`'s own.
fn temp(test: &str, name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join(format!("docx-borders-{}", std::process::id()))
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

/// The borders document copied for test `test`, opened as Slate opens it.
fn opened(test: &str) -> (PathBuf, Document, docx::DocxSource) {
    let path = temp(test, "borders.docx");
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

/// Append " Edited" to top-level paragraph `start`.
fn edit(doc: &mut Document, history: &mut History, start: &str) {
    let i = block(doc, start);
    let at = Pos::new(i, text_len(doc, Container::Top, i).unwrap());
    let edit = replace(doc, at, at, " Edited", &document_edit::look(doc, at)).unwrap();
    history.apply(doc, edit);
}

const RESUME_RULE: &str =
    "<w:pBdr><w:bottom w:color=\"000000\" w:space=\"1\" w:sz=\"4\" w:val=\"single\"/></w:pBdr>";
const NO_BORDERS: &str = "<w:pBdr><w:top w:space=\"0\" w:sz=\"0\" w:val=\"nil\"/><w:left \
                          w:space=\"0\" w:sz=\"0\" w:val=\"nil\"/><w:bottom w:space=\"0\" \
                          w:sz=\"0\" w:val=\"nil\"/><w:right w:space=\"0\" w:sz=\"0\" \
                          w:val=\"nil\"/><w:between w:space=\"0\" w:sz=\"0\" w:val=\"nil\"/>\
                          </w:pBdr><w:shd w:fill=\"auto\" w:val=\"clear\"/>";
const DOT_DASH: &str = "<w:pBdr><w:bottom w:val=\"dotDash\" w:sz=\"8\" w:space=\"1\" \
                        w:color=\"auto\"/></w:pBdr>";
const PATTERN: &str = "<w:shd w:val=\"pct25\" w:color=\"auto\" w:fill=\"FFFFFF\"/>";

#[test]
fn an_edited_paragraph_keeps_its_own_borders_and_shading() {
    let (path, mut doc, source) = opened("edited");
    let original = part(&path, "word/document.xml");
    for markup in [RESUME_RULE, NO_BORDERS, DOT_DASH, PATTERN] {
        assert_eq!(original.matches(markup).count(), 1, "{markup}");
    }
    let rule = para(&doc, "A dot-dash rule").own;

    let mut history = History::new();
    for start in [
        "Jane Q. Writer",
        "Nil borders",
        "A 25% pattern",
        "The Boxed style's",
    ] {
        edit(&mut doc, &mut history, start);
    }
    // Enter after "A dot-": both halves keep the paragraph's own rule, as in Word.
    let i = block(&doc, "A dot-dash rule");
    let edit = split(&doc, Pos::new(i, 6), Pos::new(i, 6)).unwrap();
    history.apply(&mut doc, edit);
    assert_eq!(para(&doc, "A dot-").own, rule);
    assert_eq!(para(&doc, "dash rule").own, rule);

    let dest = save(&path, "edited", &doc, &source);
    let xml = part(&dest, "word/document.xml");
    // Written as they were: Google Docs' attribute order, its nil sides, a style drawn as
    // the nearest, a pattern drawn as its colour.
    for (markup, count) in [
        (RESUME_RULE, 1),
        (NO_BORDERS, 1),
        (DOT_DASH, 2),
        (PATTERN, 1),
    ] {
        assert_eq!(xml.matches(markup).count(), count, "{markup} in {xml}");
    }
    let back = docx::read(&dest).unwrap();
    let (a, b) = (paragraphs(&doc.blocks), paragraphs(&back.blocks));
    assert_eq!(a.len(), b.len());
    for (a, b) in a.iter().zip(&b) {
        assert_eq!((a.text(), a.own), (b.text(), b.own));
        assert_eq!(doc.styles.borders(a), back.styles.borders(b));
        assert_eq!(doc.styles.shading(a), back.styles.shading(b));
    }
}

#[test]
fn a_paragraph_whose_own_borders_differ_from_its_templates_writes_them() {
    let (path, mut doc, source) = opened("differ");
    let mine = ParaOverrides {
        borders: BorderOverrides {
            top: Some(line(BorderStyle::Dashed, 6, 2, None)),
            bottom: Some(Border::default()),
            between: Some(line(BorderStyle::Thick, 4, 0, RED)),
            ..BorderOverrides::default()
        },
        shading: Some(Shading {
            fill: Some([0xFF, 0xE5, 0x99]),
        }),
        ..ParaOverrides::default()
    };
    // Borders aren't edited yet: a paragraph pasted from elsewhere stands for one.
    let i = block(&doc, "Dashed left and right");
    let Block::Paragraph(p) = &mut doc.blocks[i] else {
        panic!("a paragraph");
    };
    let indents = (p.own.indent_left, p.own.indent_right);
    p.own = ParaOverrides {
        indent_left: indents.0,
        indent_right: indents.1,
        ..mine
    };
    p.runs[0].text.push('!');
    // The Boxed paragraph's automatic fill and nil sides dropped: the style's come back.
    let i = block(&doc, "Boxed, with its own nil top");
    let Block::Paragraph(p) = &mut doc.blocks[i] else {
        panic!("a paragraph");
    };
    p.own = ParaOverrides::default();
    p.runs[0].text.push('!');
    // A rule pasted from Markdown is a paragraph with a bottom border.
    doc.blocks.insert(i, Block::Rule);

    let dest = save(&path, "differ", &doc, &source);
    let xml = part(&dest, "word/document.xml");
    for want in [
        // In schema order: borders, shading, then the indents kept as written.
        "<w:pPr><w:pBdr><w:top w:val=\"dashed\" w:sz=\"6\" w:space=\"2\" w:color=\"auto\"/>\
         <w:bottom w:val=\"nil\"/><w:between w:val=\"thick\" w:sz=\"4\" w:space=\"0\" \
         w:color=\"C00000\"/></w:pBdr><w:shd w:val=\"clear\" w:color=\"auto\" \
         w:fill=\"FFE599\"/><w:ind w:left=\"720\" w:right=\"720\"/></w:pPr>",
        "<w:pBdr><w:bottom w:val=\"single\" w:sz=\"6\" w:space=\"1\" w:color=\"auto\"/></w:pBdr>",
    ] {
        assert!(xml.contains(want), "{want} in {xml}");
    }
    let back = docx::read(&dest).unwrap();
    let dashed = para(&back, "Dashed left and right");
    assert_eq!(
        (dashed.own.borders, dashed.own.shading),
        (mine.borders, mine.shading)
    );
    let boxed = para(&back, "Boxed, with its own nil top");
    assert!(boxed.own.is_empty());
    assert!(back.styles.borders(boxed).top.is_drawn());
    // The Boxed paragraph's `w:pPr` is its style's alone, as the two plain ones' are.
    assert_eq!(
        xml.matches("<w:pPr><w:pStyle w:val=\"Boxed\"/></w:pPr>")
            .count(),
        3,
        "{xml}"
    );
    let rule = paragraphs(&back.blocks)
        .into_iter()
        .find(|p| p.text().is_empty())
        .unwrap();
    assert_eq!(
        rule.own.borders.bottom,
        Some(line(BorderStyle::Single, 6, 1, None))
    );
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
#[ignore = "needs LibreOffice: cargo test -p docx --test borders -- --ignored"]
fn libreoffice_reads_every_edited_paragraphs_borders_and_shading_as_before() {
    let (path, mut doc, source) = opened("libreoffice");
    // " Edited" after every paragraph outside the table.
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
    let (before, after) = (lo_dump(&path, 2186), lo_dump(&dest, 2186));
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
