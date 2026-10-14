//! SLATE-28: list markers from the numbering (ADR 0009). Each Word list item's level
//! gives its marker (`w:lvlText`, Symbol and Wingdings characters mapped to Unicode, a
//! number's label filled in), its alignment and suffix, its run look over the paragraph
//! mark's, and its indents, which go between its style's and its own. Saving an edited
//! item keeps its `w:numPr` and `numbering.xml` byte for byte.
//! `tests/data/lists.docx` is `python3 scripts/lo_docx.py lists`'s.

use document_edit::{normalize, replace, text_len, Container, History, Pos};
use document_model::{
    Block, Document, ListItem, ListKind, ListLevel, MarkerAlign, MarkerSuffix, RunStyle,
};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

fn data() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/lists.docx")
}

/// The list item whose text starts with `start`.
fn item<'a>(doc: &'a Document, start: &str) -> &'a ListItem {
    doc.blocks
        .iter()
        .find_map(|b| match b {
            Block::ListItem(item) if item.paragraph.text().starts_with(start) => Some(item),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no list item starts with {start:?}"))
}

/// The level of the item whose text starts with `start`, and its marker.
fn level<'a>(doc: &'a Document, start: &str) -> (&'a ListLevel, &'a str) {
    let item = item(doc, start);
    let level = doc
        .format
        .list_level(item)
        .expect("a Word list item has a level");
    (level, item.marker(Some(level)))
}

#[test]
fn levels_give_markers_alignments_suffixes_and_indents() {
    let doc = docx::read(&data()).unwrap();
    // Google Docs' bullet, exactly as it writes it: "●", 360 in, hanging 360, no
    // underline, in the paragraph's font.
    let (google, marker) = level(&doc, "Google bullet:");
    assert_eq!(marker, "●");
    assert_eq!(
        (google.indent_left, google.indent_first),
        (Some(360), Some(-360))
    );
    assert_eq!(
        (google.align, google.suffix),
        (MarkerAlign::Left, MarkerSuffix::Tab)
    );
    assert_eq!(
        google.look,
        RunStyle {
            underline: Some(false),
            ..RunStyle::default()
        }
    );
    assert_eq!(level(&doc, "Google's second level").1, "○");
    assert_eq!(level(&doc, "Google's third level").1, "■");
    // Symbol and Wingdings bullets map to Unicode and leave their font; Courier New's
    // "o" stays an "o" in Courier New.
    for (start, want) in [
        ("Word's Symbol bullet", "•"),
        ("Word's Wingdings square", "▪"),
        ("Word's Wingdings arrowhead", "➢"),
    ] {
        let (l, marker) = level(&doc, start);
        assert_eq!((marker, &l.look.font), (want, &None), "{start}");
    }
    let (courier, marker) = level(&doc, "Word's Courier New o");
    assert_eq!(marker, "o");
    assert_eq!(courier.look.font.as_deref(), Some("Courier New"));
    // Numbers: their labels, counted as before.
    for (start, label) in [
        ("Decimal one.", "1."),
        ("Decimal one point one", "1.1."),
        ("Lower letter in", "(a)"),
        ("Lower letter, the second", "(b)"),
        ("Decimal two", "2."),
        ("Upper roman one", "I."),
        ("Upper roman three", "III."),
        ("A number followed by a space", "1)"),
        ("A roman number followed by nothing", "i."),
        ("A level whose text is empty", ""),
        ("A first-line indent", "Step 1:"),
    ] {
        assert_eq!(level(&doc, start).1, label, "{start}");
    }
    assert_eq!(level(&doc, "Upper roman two").0.align, MarkerAlign::Right);
    assert_eq!(
        level(&doc, "A number followed by a space").0.suffix,
        MarkerSuffix::Space
    );
    assert_eq!(
        level(&doc, "A roman number followed by nothing").0.suffix,
        MarkerSuffix::Nothing
    );
    let first = level(&doc, "A first-line indent").0;
    assert_eq!(
        (first.indent_left, first.indent_first),
        (Some(720), Some(0))
    );
}

#[test]
fn indents_go_style_then_level_then_own() {
    let doc = docx::read(&data()).unwrap();
    let indents = |start: &str| {
        let item = item(&doc, start);
        let s = doc
            .styles
            .list_spacing(&item.paragraph, doc.format.list_level(item).unwrap());
        (s.indent_left, s.indent_first)
    };
    assert_eq!(indents("Decimal one."), (720, -360));
    // Its own left indent over the level's, the level's hanging kept.
    assert_eq!(indents("Decimal three"), (1440, -360));
    assert_eq!(indents("Decimal four"), (1080, -720));
    // List Paragraph's left indent (720) under the level's, its spacing kept.
    assert_eq!(indents("Decimal five"), (720, -360));
    let five = item(&doc, "Decimal five");
    assert!(doc.styles.spacing(&five.paragraph).contextual);
}

#[test]
fn a_markers_look_is_its_levels_over_its_paragraph_marks() {
    let doc = docx::read(&data()).unwrap();
    let red = item(&doc, "A bullet whose paragraph mark");
    let mark = &red.numbering.as_ref().unwrap().mark;
    assert_eq!((mark.color, mark.size), (Some([0xC0, 0, 0]), Some(28)));
    let level = doc.format.list_level(red).unwrap();
    let look = doc.styles.marker_look(&red.paragraph, mark, level);
    assert_eq!(
        (look.color, look.size, look.underline, look.font.as_str()),
        (Some([0xC0, 0, 0]), 28, false, "Liberation Sans")
    );
    // The text keeps its own look: the mark's isn't the runs'.
    let run = &red.paragraph.runs[0].style;
    assert_eq!((run.color, run.size), (None, None));
    // Courier New's level font goes over the paragraph's.
    let o = item(&doc, "Word's Courier New o");
    let look = doc.styles.marker_look(
        &o.paragraph,
        &o.numbering.as_ref().unwrap().mark,
        doc.format.list_level(o).unwrap(),
    );
    assert_eq!((look.font.as_str(), look.size), ("Courier New", 22));
}

#[test]
fn items_without_a_fitting_level_take_slates() {
    let doc = docx::read(&data()).unwrap();
    // A numbered item of a bulleted instance (as a kind change leaves it until saved)
    // lays out with Slate's numbered level, as saving gives it a list of Slate's.
    let mut changed = item(&doc, "Google bullet:").clone();
    changed.kind = ListKind::Ordered {
        number: 1,
        label: "1.".into(),
    };
    assert_eq!(
        doc.format.list_level(&changed),
        Some(ListLevel::slate(false, 0))
    );
    // So does an item Slate started.
    changed.numbering = None;
    changed.kind = ListKind::Bullet;
    changed.level = 1;
    assert_eq!(
        doc.format.list_level(&changed),
        Some(ListLevel::slate(true, 1))
    );
}

fn part(path: &Path, name: &str) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut bytes = Vec::new();
    zip.by_name(name).unwrap().read_to_end(&mut bytes).unwrap();
    bytes
}

#[test]
fn an_edited_item_keeps_its_numbering() {
    let dir = std::env::temp_dir().join(format!("docx-lists-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("lists.docx");
    std::fs::copy(data(), &path).unwrap();
    let (mut doc, mut source) = docx::read_source(&path).unwrap();
    normalize(&mut doc);
    source.set_blocks(doc.blocks.clone());

    let i = doc
        .blocks
        .iter()
        .position(|b| matches!(b, Block::ListItem(item) if item.paragraph.text().starts_with("Upper roman two")))
        .unwrap();
    let at = Pos::new(i, text_len(&doc, Container::Top, i).unwrap());
    let edit = replace(&doc, at, at, " Edited", &document_edit::look(&doc, at)).unwrap();
    History::new().apply(&mut doc, edit);

    let dest = dir.join("saved.docx");
    docx::save(&dest, &path, &doc.blocks, &source, &AtomicBool::new(false)).unwrap();
    assert_eq!(
        part(&dest, "word/numbering.xml"),
        part(&path, "word/numbering.xml"),
        "numbering.xml is untouched"
    );
    let xml = String::from_utf8(part(&dest, "word/document.xml")).unwrap();
    let edited = &xml[..xml
        .find("Upper roman two. Edited")
        .expect("the edit is saved")];
    let paragraph = &edited[edited.rfind("<w:p>").unwrap()..];
    assert!(
        paragraph.contains("<w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"4\"/></w:numPr>"),
        "{paragraph}"
    );
    let back = docx::read(&dest).unwrap();
    let (level, marker) = level(&back, "Upper roman two. Edited");
    assert_eq!((marker, level.align), ("II.", MarkerAlign::Right));
    assert_eq!(
        item(&back, "Upper roman three").kind,
        item(&doc, "Upper roman three").kind
    );
}
