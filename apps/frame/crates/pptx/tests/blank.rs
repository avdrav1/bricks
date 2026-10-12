//! FRAME-14: Frame's built-in blank deck. It reads as a 16:9 deck with one empty title
//! slide and the five standard layouts; a title typed into it saves into a `.pptx`
//! LibreOffice reads with that title and all five layouts.

use deck_edit::{History, ShapeRef, TextEdit, TextPos};
use presentation_model::{Layer, PlaceholderKind, Presentation};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

const LAYOUTS: [&str; 5] = [
    "Title Slide",
    "Title and Content",
    "Section Header",
    "Two Content",
    "Title Only",
];

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("frame-blank-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// The blank deck, from the copy a window would make of it.
fn blank() -> (pptx::Original, Presentation) {
    let original = pptx::Original::of_bytes(pptx::blank()).unwrap();
    let deck = pptx::read(original.path()).unwrap();
    (original, deck)
}

/// The blank deck with `title` typed into its title.
fn titled(deck: &Presentation, title: &str) -> Presentation {
    let mut deck = deck.clone();
    let shape = deck.slides[0]
        .shapes
        .iter()
        .position(|s| s.placeholder == Some(PlaceholderKind::CentredTitle))
        .unwrap();
    let s = ShapeRef { slide: 0, shape };
    let at = TextPos::default();
    let edit = TextEdit::typing(&deck, s, at, at, title).unwrap();
    History::new().apply(&mut deck, edit);
    deck
}

#[test]
fn the_blank_deck_is_a_title_slide_on_five_layouts() {
    let (_original, deck) = blank();
    assert_eq!(
        (deck.size.cx, deck.size.cy),
        (12_192_000, 6_858_000),
        "16:9"
    );
    let names: Vec<&str> = deck.layouts.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, LAYOUTS);
    assert_eq!(deck.slides.len(), 1);
    let slide = &deck.slides[0];
    assert_eq!(deck.layouts[slide.layout.unwrap()].name, "Title Slide");
    let own: Vec<_> = slide
        .shapes
        .iter()
        .filter(|s| s.layer == Layer::Slide)
        .map(|s| (s.placeholder, s.plain_text()))
        .collect();
    assert_eq!(
        own,
        [
            (Some(PlaceholderKind::CentredTitle), String::new()),
            (Some(PlaceholderKind::Subtitle), String::new())
        ]
    );
    // The Office theme's looks: a 60 pt Calibri Light title, centred, as PowerPoint's
    // blank deck has it; text boxes in 18 pt Calibri.
    let title = &slide
        .shapes
        .iter()
        .find(|s| s.layer == Layer::Slide)
        .unwrap();
    let mark = &title.text.as_ref().unwrap().paragraphs[0].end;
    assert_eq!((mark.font.as_str(), mark.size), ("Calibri Light", 60.0));
    let text_box = deck.layouts[0].text_box.as_ref().unwrap();
    let end = &text_box.paragraphs[0].end;
    assert_eq!((end.font.as_str(), end.size), ("Calibri", 18.0));
}

#[test]
fn a_title_typed_into_the_blank_deck_saves() {
    let (original, read) = blank();
    let deck = titled(&read, "Quarterly review");
    let dest = temp("saved.pptx");
    pptx::save(
        &dest,
        original.path(),
        &read,
        &deck,
        &AtomicBool::new(false),
    )
    .unwrap();
    let back = pptx::read(&dest).unwrap();
    assert_eq!(back.slides.len(), 1);
    assert_eq!(
        back.slides[0].own_shapes().next().unwrap().plain_text(),
        "Quarterly review"
    );
    let names: Vec<&str> = back.layouts.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, LAYOUTS);
}

fn lo(command: &str, file: &Path) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../..");
    let out = std::process::Command::new("python3")
        .arg(root.join("scripts/lo_pptx.py"))
        .args([command, &file.to_string_lossy()])
        .env("LO_PORT", "2175")
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
#[ignore = "needs LibreOffice: cargo test -p pptx --test blank -- --ignored"]
fn libreoffice_reads_the_title_and_the_five_layouts() {
    let (original, read) = blank();
    let deck = titled(&read, "Quarterly review");
    let dest = temp("libreoffice.pptx");
    pptx::save(
        &dest,
        original.path(),
        &read,
        &deck,
        &AtomicBool::new(false),
    )
    .unwrap();
    let dump = lo("dump", &dest);
    // 16:9, to LibreOffice's 1/100 mm (360 EMU).
    let size: Vec<i64> = dump
        .lines()
        .next()
        .unwrap()
        .split('\t')
        .skip(1)
        .map(|v| v.parse().unwrap())
        .collect();
    assert!(
        (size[0] - 12_192_000).abs() <= 360 && (size[1] - 6_858_000).abs() <= 360,
        "{size:?}"
    );
    assert!(
        dump.lines()
            .any(|l| l.starts_with("para\t0\tcenter\tQuarterly review")),
        "{dump}"
    );
    assert_eq!(dump.lines().filter(|l| l.starts_with("slide\t")).count(), 1);
    let layouts: Vec<String> = lo("layouts", &dest)
        .lines()
        .filter_map(|l| l.strip_prefix("layout\t").map(String::from))
        .collect();
    assert_eq!(layouts, LAYOUTS);
}
