//! FRAME-8 acceptance: saving patches the package the deck was read from. Text edits, a
//! new slide, a duplicate (with its notes), a deleted slide, and a moved slide save and
//! read back as the deck is; parts nothing touched are copied byte for byte; a saved deck
//! saves again. LibreOffice's reading of the saved deck: the ignored tests at the end.
//!
//! FRAME-11: formatting (text looks, alignment, bullets, fills, outlines, backgrounds)
//! and layout changes save and read back as the edited deck, the rest byte for byte.
//!
//! FRAME-12: pictures (filling a placeholder, or centred), text boxes, shapes, lines, and
//! tables inserted, a shape deleted, and text shrunk to fit save and read back as the
//! edited deck, each picture's image a new media part.

mod common;

use deck_edit::{
    BackgroundEdit, Fit, FitEdit, FrameEdit, History, LayoutEdit, LookEdit, ParagraphLook,
    ShapeEdit, ShapeLook, ShapeLookEdit, ShapeRef, SlideEdit, TextEdit, TextLook, TextPos,
};
use presentation_model::{
    Align, Autofit, Background, Color, Dash, Fill, Frame, Image, ImageFormat, Layer, Outline,
    Paragraph, PlaceholderKind, Presentation, RunSources, ShapeKind, Slide, Source,
};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn temp(test: &str, name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("frame-save-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// Every part of the package at `path`, unpacked.
fn parts(path: &Path) -> BTreeMap<String, Vec<u8>> {
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

fn placeholder(slide: &Slide, kind: PlaceholderKind) -> usize {
    slide
        .shapes
        .iter()
        .position(|s| s.layer == Layer::Slide && s.placeholder == Some(kind))
        .unwrap()
}

/// Type `text` at the end of shape `shape` of slide `slide`, a character at a time.
fn type_at_end(deck: &mut Presentation, h: &mut History, slide: usize, shape: usize, text: &str) {
    let s = ShapeRef { slide, shape };
    let body = deck.slides[slide].shapes[shape].text.as_ref().unwrap();
    let mut at = deck_edit::text_end(body);
    for c in text.chars() {
        let edit = if c == '\n' {
            TextEdit::new_paragraph(deck, s, at, at)
        } else {
            TextEdit::typing(deck, s, at, at, &c.to_string())
        };
        at = h.apply(deck, edit.unwrap()).caret().unwrap();
    }
}

/// layouts.pptx edited: typing in slide 1's subtitle (a paragraph added); slide 1's title
/// (its box its layout's) moved right by 1 M; a new slide after it, its title and body
/// typed; slide 10 (with notes) duplicated and the copy's title typed into; Two Content
/// deleted; the picture slide moved to third and its picture made 1 M narrower.
fn edited() -> (Presentation, Presentation) {
    let mut deck = pptx::read(&common::data("layouts.pptx")).unwrap();
    let read = deck.clone();
    let mut h = History::new();
    let sub = placeholder(&deck.slides[0], PlaceholderKind::Subtitle);
    type_at_end(&mut deck, &mut h, 0, sub, " here\nsecond line");
    let add = SlideEdit::new_slide(&deck, Some(0)).unwrap();
    h.apply(&mut deck, add);
    let title = placeholder(&deck.slides[1], PlaceholderKind::Title);
    type_at_end(&mut deck, &mut h, 1, title, "Agenda");
    let body = placeholder(&deck.slides[1], PlaceholderKind::Body);
    type_at_end(&mut deck, &mut h, 1, body, "Point one\nPoint two");
    // Slide 10 has a notes part (empty): the copy gets its own.
    let last = deck.slides.len() - 1;
    let dup = SlideEdit::duplicate(&deck, last).unwrap();
    h.apply(&mut deck, dup);
    let title = placeholder(&deck.slides[last + 1], PlaceholderKind::Title);
    type_at_end(&mut deck, &mut h, last + 1, title, " (copy)");
    let two = deck
        .slides
        .iter()
        .position(|s| {
            s.layout
                .is_some_and(|l| deck.layouts[l].name == "Two Content")
        })
        .unwrap();
    let delete = SlideEdit::delete(&deck, two).unwrap();
    h.apply(&mut deck, delete);
    let picture = deck
        .slides
        .iter()
        .position(|s| {
            s.layout
                .is_some_and(|l| deck.layouts[l].name == "Picture with Caption")
        })
        .unwrap();
    let step = SlideEdit::move_to(&deck, picture, 2).unwrap();
    h.apply(&mut deck, step);
    let title = placeholder(&deck.slides[0], PlaceholderKind::CentredTitle);
    let f = deck.slides[0].shapes[title].frame;
    let s = ShapeRef {
        slide: 0,
        shape: title,
    };
    let step = FrameEdit::new(&deck, s, Frame { x: f.x + M, ..f }).unwrap();
    h.apply(&mut deck, step);
    let pic = deck.slides[2]
        .shapes
        .iter()
        .position(|s| s.layer == Layer::Slide && matches!(s.kind, ShapeKind::Picture(_)))
        .unwrap();
    let f = deck.slides[2].shapes[pic].frame;
    let s = ShapeRef {
        slide: 2,
        shape: pic,
    };
    let step = FrameEdit::new(&deck, s, Frame { cx: f.cx - M, ..f }).unwrap();
    h.apply(&mut deck, step);
    (read, deck)
}

const M: i64 = 1_000_000;

/// Field `n` (tab-separated) of the first `shape` line of kind `kind` in a dump slide.
fn shape_field(lines: &[&str], kind: &str, n: usize) -> i64 {
    let line = lines
        .iter()
        .find(|l| l.starts_with("shape\t") && l.contains(kind))
        .unwrap_or_else(|| panic!("no {kind} in {lines:?}"));
    line.split('\t').nth(n).unwrap().parse().unwrap()
}

/// `s` with the looks an edit set read as set on the run itself, as they read back.
fn own(s: RunSources) -> RunSources {
    let own = |x: Source| if x == Source::Edit { Source::Own } else { x };
    RunSources {
        font: own(s.font),
        size: own(s.size),
        bold: own(s.bold),
        italic: own(s.italic),
        underline: own(s.underline),
        strike: own(s.strike),
        color: own(s.color),
        baseline: own(s.baseline),
        caps: own(s.caps),
    }
}

/// A paragraph as it reads: its looks, and its runs' text and looks (not the markup a
/// save writes them with).
fn reading(p: &Paragraph) -> String {
    let runs: Vec<String> = p
        .runs
        .iter()
        .map(|r| {
            format!(
                "{:?} {} {} {} {} {} {} {:?} {:?}",
                r.text,
                r.font,
                r.size,
                r.bold,
                r.italic,
                r.underline,
                r.color.hex(),
                r.caps,
                own(r.sources)
            )
        })
        .collect();
    format!(
        "{} {:?} {} {} {:?} {:?} | {}",
        p.level,
        p.align,
        p.margin_left,
        p.indent,
        p.bullet.as_ref().map(|b| &b.text),
        p.end.size,
        runs.join(" | ")
    )
}

/// A fill as it reads, pictures by their size.
fn fill_reading(f: &Fill) -> String {
    match f {
        Fill::None => "none".into(),
        Fill::Solid(c) => format!("solid {} {}", c.hex(), c.alpha),
        Fill::Picture(img) => format!("picture {}", img.bytes.len()),
        Fill::Unsupported { label, approx } => format!("{label} {}", approx.hex()),
    }
}

fn background_reading(b: &Background) -> String {
    match b {
        Background::Solid(c) => format!("solid {}", c.hex()),
        Background::Picture(img) => format!("picture {}", img.bytes.len()),
        Background::Other { label, approx } => format!("{label} {}", approx.hex()),
    }
}

/// The deck as it reads: per slide, its layout, background, notes, and each own shape's
/// id, placeholder and slot, fill, outline, and paragraphs.
fn readings(deck: &Presentation) -> Vec<String> {
    deck.slides
        .iter()
        .map(|s| {
            let mut out = format!(
                "layout {:?}, background {}, notes {:?}\n",
                s.layout,
                background_reading(&s.background),
                s.notes
            );
            for sh in s.shapes.iter().filter(|sh| sh.layer == Layer::Slide) {
                out.push_str(&format!(
                    "shape {:?} {:?} {:?} {:?} fill {} outline {:?}\n",
                    sh.id,
                    sh.placeholder,
                    sh.slot,
                    sh.frame,
                    fill_reading(&sh.fill),
                    sh.outline
                ));
                for p in sh.text.iter().flat_map(|t| &t.paragraphs) {
                    out.push_str(&format!("  {}\n", reading(p)));
                }
            }
            out
        })
        .collect()
}

/// `back`, read from a save, reads as `deck`, slide by slide.
fn reads_as(back: &Presentation, deck: &Presentation) {
    assert_eq!(back.slides.len(), deck.slides.len());
    for (i, (a, b)) in readings(back).iter().zip(readings(deck)).enumerate() {
        assert_eq!(a, &b, "slide {}", i + 1);
    }
}

#[test]
fn edits_save_and_everything_else_is_copied() {
    let (read, mut deck) = edited();
    let source = common::data("layouts.pptx");
    let dest = temp("edits", "saved.pptx");
    pptx::save(&dest, &source, &read, &deck, &AtomicBool::new(false)).unwrap();

    // It reads back as the deck is: order, layouts, notes, text, and looks.
    let back = pptx::read(&dest).unwrap();
    reads_as(&back, &deck);
    let texts: Vec<String> = back.slides.iter().map(|s| s.text()).collect();
    assert!(
        texts[0].contains("subtitle here\nsecond line"),
        "{}",
        texts[0]
    );
    assert_eq!(texts[1], "Agenda\nPoint one\nPoint two");
    assert!(texts.last().unwrap().contains(" (copy)"));
    // 10 slides, one added, one duplicated, one deleted.
    assert_eq!(back.slides.len(), 11);

    // Slides keep their parts; the new and the copy get new ones.
    let part = |i: usize| back.slides[i].part.as_deref().unwrap();
    assert_eq!(part(0), "ppt/slides/slide1.xml");
    assert_eq!(part(1), "ppt/slides/slide11.xml");
    assert_eq!(part(10), "ppt/slides/slide12.xml");
    // The picture slide, moved third, keeps its part.
    assert_eq!(part(2), "ppt/slides/slide9.xml");

    // Every part but those the edits change is as it was, byte for byte.
    let (before, after) = (parts(&source), parts(&dest));
    let changed = [
        "[Content_Types].xml",
        "ppt/presentation.xml",
        "ppt/_rels/presentation.xml.rels",
        "ppt/slides/slide1.xml",
        "ppt/slides/slide9.xml",
    ];
    let gone = ["ppt/slides/slide4.xml", "ppt/slides/_rels/slide4.xml.rels"];
    for (name, bytes) in &before {
        if gone.contains(&name.as_str()) {
            assert!(!after.contains_key(name), "{name} is deleted");
        } else if !changed.contains(&name.as_str()) {
            assert_eq!(after.get(name), Some(bytes), "{name} is as it was");
        }
    }
    let new: Vec<&String> = after.keys().filter(|n| !before.contains_key(*n)).collect();
    assert_eq!(
        new,
        [
            "ppt/notesSlides/_rels/notesSlide2.xml.rels",
            "ppt/notesSlides/notesSlide2.xml",
            "ppt/slides/_rels/slide11.xml.rels",
            "ppt/slides/_rels/slide12.xml.rels",
            "ppt/slides/slide11.xml",
            "ppt/slides/slide12.xml",
        ]
    );
    let types = String::from_utf8(after["[Content_Types].xml"].clone()).unwrap();
    assert!(types.contains(r#"PartName="/ppt/slides/slide12.xml""#));
    assert!(types.contains(r#"PartName="/ppt/notesSlides/notesSlide2.xml""#));
    assert!(!types.contains(r#"PartName="/ppt/slides/slide4.xml""#));
    let copy_notes =
        String::from_utf8(after["ppt/notesSlides/_rels/notesSlide2.xml.rels"].clone()).unwrap();
    assert!(
        copy_notes.contains(r#"Target="../slides/slide12.xml""#),
        "{copy_notes}"
    );

    // Edited further, the deck saves again from the package it was read from, as the
    // window does after saving: the second save differs only where the edit is.
    let mut h = History::new();
    let title = placeholder(&deck.slides[1], PlaceholderKind::Title);
    type_at_end(&mut deck, &mut h, 1, title, "!");
    let again = temp("edits", "again.pptx");
    pptx::save(&again, &source, &read, &deck, &AtomicBool::new(false)).unwrap();
    let back = pptx::read(&again).unwrap();
    reads_as(&back, &deck);
    assert_eq!(back.slides[1].text(), "Agenda!\nPoint one\nPoint two");
    let (once, twice) = (parts(&dest), parts(&again));
    assert_eq!(
        once.keys().collect::<Vec<_>>(),
        twice.keys().collect::<Vec<_>>()
    );
    for (name, bytes) in &once {
        if name != "ppt/slides/slide11.xml" {
            assert_eq!(&twice[name], bytes, "{name} is as the first save wrote it");
        }
    }
}

/// The fixture with two sections, as PowerPoint 2010 writes them: slides 1 and 2, then
/// slides 3 to 5.
fn with_sections(dest: &Path) {
    const SECTIONS: &str = concat!(
        r#"<p:extLst><p:ext uri="{521415D9-36F7-43E2-AB2F-B90AF26B5E84}">"#,
        r#"<p14:sectionLst xmlns:p14="http://schemas.microsoft.com/office/powerpoint/2010/main">"#,
        r#"<p14:section name="Opening" id="{1}"><p14:sldIdLst><p14:sldId id="256"/><p14:sldId id="257"/></p14:sldIdLst></p14:section>"#,
        r#"<p14:section name="Body" id="{2}"><p14:sldIdLst><p14:sldId id="258"/><p14:sldId id="259"/><p14:sldId id="260"/></p14:sldIdLst></p14:section>"#,
        r#"</p14:sectionLst></p:ext></p:extLst>"#,
    );
    let mut src =
        zip::ZipArchive::new(std::fs::File::open(common::data("fixture.pptx")).unwrap()).unwrap();
    let mut out = zip::ZipWriter::new(std::fs::File::create(dest).unwrap());
    for i in 0..src.len() {
        let mut f = src.by_index(i).unwrap();
        let name = f.name().to_owned();
        let mut bytes = Vec::new();
        f.read_to_end(&mut bytes).unwrap();
        if name == "ppt/presentation.xml" {
            let text = String::from_utf8(bytes).unwrap();
            bytes = text
                .replace("</p:presentation>", &format!("{SECTIONS}</p:presentation>"))
                .into_bytes();
        }
        out.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut out, &bytes).unwrap();
    }
    out.finish().unwrap();
}

#[test]
fn sections_keep_their_slides() {
    let source = temp("sections", "sections.pptx");
    with_sections(&source);
    let mut deck = pptx::read(&source).unwrap();
    let read = deck.clone();
    let mut h = History::new();
    // Slide 2 deleted; a new slide after slide 3 (the Body's first); slide 1 moved last.
    h.apply(&mut deck, SlideEdit::delete(&read, 1).unwrap());
    let add = SlideEdit::new_slide(&deck, Some(1)).unwrap();
    h.apply(&mut deck, add);
    let step = SlideEdit::move_to(&deck, 0, 4).unwrap();
    h.apply(&mut deck, step);
    let dest = temp("sections", "saved.pptx");
    pptx::save(&dest, &source, &read, &deck, &AtomicBool::new(false)).unwrap();
    let pres = String::from_utf8(parts(&dest)["ppt/presentation.xml"].clone()).unwrap();
    let list = |name: &str| -> String {
        let at = pres.find(&format!(r#"name="{name}""#)).unwrap();
        let rest = &pres[at..];
        rest[..rest.find("</p14:section>").unwrap()].to_owned()
    };
    // Sections stay contiguous: the new slide (id 261) joins Body after slide 3, and slide
    // 1, moved after Body's slides, joins Body; Opening is left empty.
    assert!(
        list("Opening").ends_with(r#"<p14:sldIdLst></p14:sldIdLst>"#),
        "{}",
        list("Opening")
    );
    assert!(
        list("Body").ends_with(concat!(
            r#"<p14:sldIdLst><p14:sldId id="258"/><p14:sldId id="261"/>"#,
            r#"<p14:sldId id="259"/><p14:sldId id="260"/><p14:sldId id="256"/></p14:sldIdLst>"#
        )),
        "{}",
        list("Body")
    );
    assert!(!pres.contains(r#"id="257""#));
    reads_as(&pptx::read(&dest).unwrap(), &deck);
}

#[test]
fn an_unedited_deck_saves_as_it_was() {
    let read = pptx::read(&common::data("fixture.pptx")).unwrap();
    let dest = temp("unedited", "saved.pptx");
    pptx::save(
        &dest,
        &common::data("fixture.pptx"),
        &read,
        &read,
        &AtomicBool::new(false),
    )
    .unwrap();
    let (before, after) = (parts(&common::data("fixture.pptx")), parts(&dest));
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>()
    );
    for (name, bytes) in &before {
        if ![
            "[Content_Types].xml",
            "ppt/presentation.xml",
            "ppt/_rels/presentation.xml.rels",
        ]
        .contains(&name.as_str())
        {
            assert_eq!(&after[name], bytes, "{name}");
        }
    }
    reads_as(&pptx::read(&dest).unwrap(), &read);
}

/// `scripts/lo_pptx.py dump FILE`: LibreOffice's reading of a deck, through a LibreOffice
/// listening on `port` (one per test, so tests run side by side).
fn lo_dump(file: &Path, port: u16) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../..");
    let out = std::process::Command::new("python3")
        .arg(root.join("scripts/lo_pptx.py"))
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

/// A dump's slides, each its lines after its `slide` line.
fn dump_slides(dump: &str) -> Vec<Vec<&str>> {
    let mut out: Vec<Vec<&str>> = Vec::new();
    for line in dump.lines() {
        if line.starts_with("slide\t") {
            out.push(Vec::new());
        } else if let Some(last) = out.last_mut() {
            last.push(line);
        }
    }
    out
}

#[test]
#[ignore = "needs LibreOffice: cargo test -p pptx --test save -- --ignored"]
fn libreoffice_reads_the_saved_deck_as_the_original_plus_the_edits() {
    let (read, deck) = edited();
    let source = common::data("layouts.pptx");
    let dest = temp("libreoffice", "saved.pptx");
    pptx::save(&dest, &source, &read, &deck, &AtomicBool::new(false)).unwrap();
    let back = pptx::read(&dest).unwrap();
    let (before, after) = (lo_dump(&source, 2171), lo_dump(&dest, 2171));
    let (old, new) = (dump_slides(&before), dump_slides(&after));
    assert_eq!(new.len(), deck.slides.len());
    let original = |part: &str| {
        read.slides
            .iter()
            .position(|s| s.part.as_deref() == Some(part))
    };
    for (i, slide) in back.slides.iter().enumerate() {
        let part = slide.part.as_deref().unwrap();
        let at = format!("saved slide {} ({part})", i + 1);
        match (i, original(part)) {
            // Slide 1: the subtitle is typed into and the title moved; everything else as
            // it was.
            (0, Some(j)) => {
                let others = |lines: &[&str]| -> Vec<String> {
                    lines
                        .iter()
                        .filter(|l| !l.starts_with("para\t") && !l.starts_with("run\t"))
                        .filter(|l| !l.contains("TitleTextShape"))
                        .map(|l| l.to_string())
                        .collect()
                };
                assert_eq!(others(&new[i]), others(&old[j]), "{at}");
                assert!(
                    new[i].contains(&"para\t0\tcenter\tsubtitle here"),
                    "{at}: {:?}",
                    new[i]
                );
                assert!(new[i].contains(&"para\t0\tcenter\tsecond line"), "{at}");
                assert!(new[i].contains(&"run\t32\t0\t0\t0\t0\t8B8B8B\tCalibri\tsecond line"));
                let x = |lines: &[&str]| shape_field(lines, "TitleTextShape", 3);
                assert!(
                    (x(&new[i]) - x(&old[j]) - M).abs() <= 720,
                    "{at}: moved 1 M right"
                );
            }
            // The picture slide: its picture 1 M narrower.
            (2, Some(j)) => {
                let others = |lines: &[&str]| -> Vec<String> {
                    lines
                        .iter()
                        .filter(|l| !l.contains("GraphicObjectShape"))
                        .map(|l| l.to_string())
                        .collect()
                };
                assert_eq!(others(&new[i]), others(&old[j]), "{at}");
                let w = |lines: &[&str]| shape_field(lines, "GraphicObjectShape", 5);
                assert!(
                    (w(&old[j]) - w(&new[i]) - M).abs() <= 720,
                    "{at}: 1 M narrower"
                );
            }
            // Every other slide read from the file: as LibreOffice read it before.
            (_, Some(j)) => assert_eq!(new[i], old[j], "{at}"),
            // The new slide: its typed title and bullets.
            (1, None) => {
                assert!(
                    new[i].contains(&"para\t0\tcenter\tAgenda"),
                    "{at}: {:?}",
                    new[i]
                );
                assert!(new[i].contains(&"para\t0\tleft\tPoint one"), "{at}");
                assert!(new[i].contains(&"para\t0\tleft\tPoint two"), "{at}");
            }
            // The copy of slide 10: as slide 10, its title typed into.
            (_, None) => {
                let ten = &old[9];
                let title = "para\t0\tcenter\tHeaders & Footers (copy)";
                assert!(new[i].contains(&title), "{at}: {:?}", new[i]);
                let same = |lines: &[&str]| -> Vec<String> {
                    lines
                        .iter()
                        .filter(|l| !l.contains("Headers & Footers"))
                        .map(|l| l.to_string())
                        .collect()
                };
                assert_eq!(same(&new[i]), same(ten), "{at}");
            }
        }
    }
}

const RED: Color = Color::rgb(0xC0, 0x00, 0x00);
const GREEN: Color = Color::rgb(0x00, 0xB0, 0x50);
const CREAM: Color = Color::rgb(0xFF, 0xF2, 0xCC);
const BLUE: Color = Color::rgb(0x44, 0x72, 0xC4);

fn layout_named(deck: &Presentation, name: &str) -> usize {
    deck.layouts
        .iter()
        .position(|l| l.name == name)
        .unwrap_or_else(|| panic!("no layout {name}"))
}

fn placeholder_ref(deck: &Presentation, slide: usize, kind: PlaceholderKind) -> ShapeRef {
    ShapeRef {
        slide,
        shape: placeholder(&deck.slides[slide], kind),
    }
}

/// layouts.pptx formatted: "sub" of slide 1's subtitle bold, red, 40 pt, and Liberation
/// Serif; slide 2's first body paragraph right-aligned, its bullet off. Slide 1
/// duplicated, the copy (Title Slide) on Title and Content; Section Header emptied of its
/// text and on Title Only (its title moved, its empty text placeholder gone); the Title
/// Only slide on Two Content (two placeholders added); a new Title Slide at the end, its
/// title typed, on Title Only.
fn formatted_layouts() -> (Presentation, Presentation) {
    let mut deck = pptx::read(&common::data("layouts.pptx")).unwrap();
    let read = deck.clone();
    let mut h = History::new();
    let sub = placeholder_ref(&deck, 0, PlaceholderKind::Subtitle);
    let looks = [
        TextLook::Bold(true),
        TextLook::Color(RED),
        TextLook::Size(40.0),
        TextLook::Font("Liberation Serif".into()),
    ];
    for look in &looks {
        let edit = LookEdit::text(&deck, sub, TextPos::new(0, 0), TextPos::new(0, 3), look);
        h.apply(&mut deck, edit.unwrap());
    }
    let body = placeholder_ref(&deck, 1, PlaceholderKind::Body);
    for look in [
        ParagraphLook::Align(Align::Right),
        ParagraphLook::Bullets(false),
    ] {
        let at = TextPos::new(0, 0);
        let edit = LookEdit::paragraphs(&deck, body, at, at, look);
        h.apply(&mut deck, edit.unwrap());
    }
    let dup = SlideEdit::duplicate(&deck, 0).unwrap();
    h.apply(&mut deck, dup);
    let step = LayoutEdit::new(&deck, 1, layout_named(&deck, "Title and Content")).unwrap();
    h.apply(&mut deck, step);
    let text = placeholder_ref(&deck, 3, PlaceholderKind::Body);
    let end = deck_edit::text_end(deck.slides[3].shapes[text.shape].text.as_ref().unwrap());
    let delete = TextEdit::delete(&deck, text, TextPos::new(0, 0), end).unwrap();
    h.apply(&mut deck, delete);
    let title_only = layout_named(&deck, "Title Only");
    let step = LayoutEdit::new(&deck, 3, title_only).unwrap();
    h.apply(&mut deck, step);
    let step = LayoutEdit::new(&deck, 6, layout_named(&deck, "Two Content")).unwrap();
    h.apply(&mut deck, step);
    let last = deck.slides.len() - 1;
    let add = SlideEdit::on_layout(&deck, Some(last), layout_named(&deck, "Title Slide")).unwrap();
    h.apply(&mut deck, add);
    let title = placeholder(&deck.slides[last + 1], PlaceholderKind::CentredTitle);
    type_at_end(&mut deck, &mut h, last + 1, title, "Closing");
    let step = LayoutEdit::new(&deck, last + 1, title_only).unwrap();
    h.apply(&mut deck, step);
    (read, deck)
}

/// fixture.pptx formatted: slide 3's blue rectangle red with a 3 pt dashed green
/// outline, its text box's bullets on; slide 2's background cream.
fn formatted_fixture() -> (Presentation, Presentation) {
    let mut deck = pptx::read(&common::data("fixture.pptx")).unwrap();
    let read = deck.clone();
    let mut h = History::new();
    let shapes = &deck.slides[2].shapes;
    let rect = shapes
        .iter()
        .position(|s| s.layer == Layer::Slide && s.fill == Fill::Solid(BLUE))
        .unwrap();
    let text_box = shapes
        .iter()
        .position(|s| s.plain_text().starts_with("Text box"))
        .unwrap();
    let rect = ShapeRef {
        slide: 2,
        shape: rect,
    };
    let fill = ShapeLookEdit::new(
        &deck,
        rect,
        ShapeLook::Fill(Fill::Solid(Color::rgb(255, 0, 0))),
    );
    h.apply(&mut deck, fill.unwrap());
    let outline = Outline {
        width: 3 * 12_700,
        color: GREEN,
        dash: Dash::Dash,
    };
    let line = ShapeLookEdit::new(&deck, rect, ShapeLook::Outline(Some(outline)));
    h.apply(&mut deck, line.unwrap());
    let text_box = ShapeRef {
        slide: 2,
        shape: text_box,
    };
    let at = TextPos::new(0, 0);
    let bullets = LookEdit::paragraphs(&deck, text_box, at, at, ParagraphLook::Bullets(true));
    h.apply(&mut deck, bullets.unwrap());
    let bg = BackgroundEdit::new(&deck, 1, Background::Solid(CREAM)).unwrap();
    h.apply(&mut deck, bg);
    (read, deck)
}

/// Every part of `source` but `changed` is in `dest` byte for byte.
fn untouched_but(source: &Path, dest: &Path, changed: &[String]) {
    let (before, after) = (parts(source), parts(dest));
    for (name, bytes) in &before {
        if !changed.contains(name) {
            assert_eq!(after.get(name), Some(bytes), "{name} is as it was");
        }
    }
}

/// The parts a save changes: the package's slide list and types, and the parts (and,
/// with `rels`, the relationships) of slides `slides` of `read`.
fn changed_parts(read: &Presentation, slides: &[usize], rels: &[usize]) -> Vec<String> {
    let part = |i: usize| read.slides[i].part.clone().unwrap();
    let mut out: Vec<String> = [
        "[Content_Types].xml",
        "ppt/presentation.xml",
        "ppt/_rels/presentation.xml.rels",
    ]
    .map(String::from)
    .into();
    out.extend(slides.iter().map(|&i| part(i)));
    out.extend(rels.iter().map(|&i| {
        let p = part(i);
        let (dir, file) = p.rsplit_once('/').unwrap();
        format!("{dir}/_rels/{file}.rels")
    }));
    out
}

#[test]
fn formatting_and_layout_changes_save_and_read_back() {
    let (read, deck) = formatted_layouts();
    let source = common::data("layouts.pptx");
    let dest = temp("formatting", "saved.pptx");
    pptx::save(&dest, &source, &read, &deck, &AtomicBool::new(false)).unwrap();
    let back = pptx::read(&dest).unwrap();
    reads_as(&back, &deck);

    // The formatted run, and the paragraph.
    let sub = placeholder(&back.slides[0], PlaceholderKind::Subtitle);
    let runs = &back.slides[0].shapes[sub].text.as_ref().unwrap().paragraphs[0].runs;
    let r = &runs[0];
    assert_eq!(
        (r.text.as_str(), r.bold, r.color, r.size, r.font.as_str()),
        ("sub", true, RED, 40.0, "Liberation Serif")
    );
    assert_eq!(runs[1].text, "title");
    assert!(!runs[1].bold);
    let body = placeholder(&back.slides[2], PlaceholderKind::Body);
    let p = &back.slides[2].shapes[body]
        .text
        .as_ref()
        .unwrap()
        .paragraphs;
    assert_eq!((p[0].align, p[0].bullet.is_none()), (Align::Right, true));
    assert_eq!((p[1].align, p[1].bullet.is_some()), (Align::Left, true));

    // The layouts, by name, and the placeholders on them.
    let layout = |i: usize| back.layouts[back.slides[i].layout.unwrap()].name.as_str();
    let kinds = |i: usize| -> Vec<Option<PlaceholderKind>> {
        back.slides[i].own_shapes().map(|s| s.placeholder).collect()
    };
    use PlaceholderKind::{Body, Title};
    assert_eq!(layout(1), "Title and Content");
    assert_eq!(kinds(1), [Some(Title), Some(Body)]);
    assert_eq!(back.slides[1].text(), "Centered Title\nsubtitle");
    assert_eq!(layout(3), "Title Only");
    assert_eq!(kinds(3), [Some(Title)]);
    assert_eq!(back.slides[3].text(), "Section Title");
    assert_eq!(layout(6), "Two Content");
    assert_eq!(kinds(6), [Some(Body), Some(Body), Some(Title)]);
    assert_eq!(back.slides[6].text(), "Title Only");
    let last = back.slides.len() - 1;
    assert_eq!(layout(last), "Title Only");
    assert_eq!(kinds(last), [Some(Title)]);
    assert_eq!(back.slides[last].text(), "Closing");
    assert_eq!(back.slides.len(), 12);

    // Every other part is as it was; slide 10, moved one on, has its slide number
    // field written.
    untouched_but(
        &source,
        &dest,
        &changed_parts(&read, &[0, 1, 2, 5, 9], &[2, 5]),
    );
}

#[test]
fn shape_looks_and_backgrounds_save_and_read_back() {
    let (read, deck) = formatted_fixture();
    let source = common::data("fixture.pptx");
    let dest = temp("looks", "saved.pptx");
    pptx::save(&dest, &source, &read, &deck, &AtomicBool::new(false)).unwrap();
    let back = pptx::read(&dest).unwrap();
    reads_as(&back, &deck);
    let rect = back.slides[2]
        .shapes
        .iter()
        .find(|s| s.fill == Fill::Solid(Color::rgb(255, 0, 0)))
        .unwrap();
    assert_eq!(
        rect.outline,
        Some(Outline {
            width: 38_100,
            color: GREEN,
            dash: Dash::Dash
        })
    );
    let text_box = back.slides[2]
        .shapes
        .iter()
        .find(|s| s.plain_text().starts_with("Text box"))
        .unwrap();
    let p = &text_box.text.as_ref().unwrap().paragraphs[0];
    assert_eq!(p.bullet.as_ref().map(|b| b.text.as_str()), Some("\u{2022}"));
    assert_eq!(
        (p.margin_left, p.indent),
        (deck_edit::BULLET_MARGIN, -deck_edit::BULLET_MARGIN)
    );
    assert_eq!(back.slides[1].background, Background::Solid(CREAM));
    assert_eq!(back.slides[1].background_source, Source::Own);
    untouched_but(&source, &dest, &changed_parts(&read, &[1, 2], &[]));
}

/// A copy of test deck `name` for test `test`: LibreOffice locks a deck it opens, so tests
/// running side by side dump their own copies.
fn private_copy(test: &str, name: &str) -> PathBuf {
    let copy = temp(test, name);
    std::fs::copy(common::data(name), &copy).unwrap();
    copy
}

#[test]
#[ignore = "needs LibreOffice: cargo test -p pptx --test save -- --ignored"]
fn libreoffice_reads_the_formatting_and_layout_changes() {
    // layouts.pptx: text looks, a paragraph, and layout changes.
    let (read, deck) = formatted_layouts();
    let source = private_copy("libreoffice-formatting", "layouts.pptx");
    let dest = temp("libreoffice-formatting", "saved.pptx");
    pptx::save(&dest, &source, &read, &deck, &AtomicBool::new(false)).unwrap();
    let (before, after) = (lo_dump(&source, 2172), lo_dump(&dest, 2172));
    let (old, new) = (dump_slides(&before), dump_slides(&after));
    assert_eq!(new.len(), deck.slides.len());
    let text_free = |lines: &[&str]| -> Vec<String> {
        lines
            .iter()
            .filter(|l| !l.starts_with("para\t") && !l.starts_with("run\t"))
            .map(|l| l.to_string())
            .collect()
    };
    let shapes = |lines: &[&str]| -> Vec<String> {
        lines
            .iter()
            .filter(|l| l.starts_with("shape\t"))
            .map(|l| l.split('\t').nth(2).unwrap().to_owned())
            .collect()
    };
    // Slide 1: "sub" formatted, the rest of the subtitle as it was.
    assert_eq!(text_free(&new[0]), text_free(&old[0]));
    let sub = "run\t40\t1\t0\t0\t0\tC00000\tLiberation Serif\tsub";
    assert!(new[0].contains(&sub), "{:?}", new[0]);
    assert!(new[0].contains(&"run\t32\t0\t0\t0\t0\t8B8B8B\tCalibri\ttitle"));
    // Slide 2 (now 3): its first paragraph right-aligned.
    assert_eq!(text_free(&new[2]), text_free(&old[1]));
    assert!(new[2].contains(&"para\t0\tright\tContent"), "{:?}", new[2]);
    let others = |lines: &[&str]| -> Vec<String> {
        lines
            .iter()
            .filter(|l| !l.starts_with("para\t0\t"))
            .map(|l| l.to_string())
            .collect()
    };
    assert_eq!(others(&new[2]), others(&old[1]));
    // The copy of slide 1 on Title and Content: a title and a body with its text.
    assert_eq!(shapes(&new[1]), ["TitleTextShape", "OutlinerShape"]);
    assert!(
        new[1].contains(&"para\t0\tcenter\tCentered Title"),
        "{:?}",
        new[1]
    );
    assert!(new[1].contains(&"para\t0\tleft\tsubtitle"), "{:?}", new[1]);
    assert!(new[1].contains(&sub));
    assert!(new[1].contains(&"run\t32\t0\t0\t0\t0\t000000\tCalibri\ttitle"));
    // Section Header on Title Only: its title alone, as Title Only's.
    assert_eq!(shapes(&new[3]), ["TitleTextShape"]);
    assert!(
        new[3].contains(&"para\t0\tcenter\tSection Title"),
        "{:?}",
        new[3]
    );
    assert!(new[3].contains(&"run\t44\t0\t0\t0\t0\t000000\tCalibri\tSection Title"));
    // Title Only on Two Content: two empty content placeholders, then its title.
    assert_eq!(
        shapes(&new[6]),
        ["OutlinerShape", "OutlinerShape", "TitleTextShape"]
    );
    assert!(
        new[6].contains(&"para\t0\tcenter\tTitle Only"),
        "{:?}",
        new[6]
    );
    // The new slide on Title Only: its title.
    let last = new.len() - 1;
    assert_eq!(shapes(&new[last]), ["TitleTextShape"]);
    assert!(
        new[last].contains(&"para\t0\tcenter\tClosing"),
        "{:?}",
        new[last]
    );
    // Every other slide as LibreOffice read it before.
    for (i, j) in [(4, 3), (5, 4), (7, 6), (8, 7), (9, 8), (10, 9)] {
        assert_eq!(new[i], old[j], "saved slide {}", i + 1);
    }

    // fixture.pptx: a fill, an outline, bullets, and a background.
    let (read, deck) = formatted_fixture();
    let source = private_copy("libreoffice-looks", "fixture.pptx");
    let dest = temp("libreoffice-looks", "saved.pptx");
    pptx::save(&dest, &source, &read, &deck, &AtomicBool::new(false)).unwrap();
    let (before, after) = (lo_dump(&source, 2172), lo_dump(&dest, 2172));
    let (old, new) = (dump_slides(&before), dump_slides(&after));
    assert_eq!(new.len(), old.len());
    // Slide 2: only its background.
    assert_eq!(new[1][0], "background\tsolid FFF2CC 100");
    assert_eq!(new[1][1..], old[1][1..]);
    // Slide 3: the rectangle's fill and line, the rest as it was.
    let rect = old[2]
        .iter()
        .position(|l| l.starts_with("fill\tsolid 4472C4"))
        .unwrap();
    assert_eq!(new[2][rect], "fill\tsolid FF0000 100");
    let line: Vec<&str> = new[2][rect + 1].split([' ', '\t']).collect();
    assert_eq!(
        line[..3],
        ["line", "dash", "00B050"],
        "{}",
        new[2][rect + 1]
    );
    let width: i64 = line[3].parse().unwrap();
    assert!((width - 38_100).abs() <= 360, "{width}");
    for (n, (a, b)) in new[2].iter().zip(&old[2]).enumerate() {
        if n != rect && n != rect + 1 {
            assert_eq!(a, b);
        }
    }
    assert_eq!(new[2].len(), old[2].len());
    for i in [0, 3, 4] {
        assert_eq!(new[i], old[i], "slide {}", i + 1);
    }
}

/// A `w` by `h` PNG: red left half, blue right half (one stored zlib block, so `w` and
/// `h` stay small).
fn png(w: u32, h: u32) -> Vec<u8> {
    fn crc(bytes: &[u8]) -> u32 {
        let mut c = !0u32;
        for &b in bytes {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 == 1 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
        !c
    }
    // Each row: filter type 0, then its pixels.
    let row: Vec<u8> = std::iter::once(0)
        .chain((0..w).flat_map(|x| if x < w / 2 { [255, 0, 0] } else { [0, 0, 255] }))
        .collect();
    let raw = row.repeat(h as usize);
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + u32::from(byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    let len = u16::try_from(raw.len()).unwrap();
    let mut zlib = vec![0x78, 0x01, 1];
    zlib.extend(len.to_le_bytes());
    zlib.extend((!len).to_le_bytes());
    zlib.extend(&raw);
    zlib.extend(((b << 16) | a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8], data: &[u8]| {
        out.extend(u32::try_from(data.len()).unwrap().to_be_bytes());
        let body = [kind, data].concat();
        out.extend(&body);
        out.extend(crc(&body).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend(w.to_be_bytes());
    ihdr.extend(h.to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &zlib);
    chunk(b"IEND", &[]);
    out
}

/// What `inserted` typed into slide 1's subtitle: more than its box holds.
const LONG: &str = " that runs on and on, line after line,\nand on again, well past the bottom";

/// layouts.pptx with things inserted: a new Title and Content slide 2 whose body a 40 x
/// 30 PNG fills; a new Title Only slide 3 with the PNG centred and a 3 x 4 table, its
/// cells typed into (`R1C1`, ...); on slide 1, a text box and an oval typed into, a line,
/// and its subtitle typed long and shrunk to fit (62.5%, lines 20% closer); the
/// original slide 7's second text box deleted; the last slide duplicated, the PNG
/// centred on the copy. The pictures, the table, and the shapes come in as the window
/// inserts them.
fn inserted() -> (Presentation, Presentation) {
    let mut deck = pptx::read(&common::data("layouts.pptx")).unwrap();
    let read = deck.clone();
    let mut h = History::new();
    let image = || Image {
        format: ImageFormat::Png,
        bytes: Arc::from(png(40, 30)),
    };
    let add = SlideEdit::on_layout(&deck, Some(0), layout_named(&deck, "Title and Content"));
    h.apply(&mut deck, add.unwrap());
    let add = SlideEdit::on_layout(&deck, Some(1), layout_named(&deck, "Title Only"));
    h.apply(&mut deck, add.unwrap());
    let last = deck.slides.len() - 1;
    let dup = SlideEdit::duplicate(&deck, last).unwrap();
    h.apply(&mut deck, dup);
    for slide in [1, 2, last + 1] {
        let edit = ShapeEdit::picture(&deck, slide, image(), (40, 30), None).unwrap();
        h.apply(&mut deck, edit);
    }
    let table = ShapeEdit::table(&deck, 2, 3, 4).unwrap();
    let at = table.target().unwrap();
    h.apply(&mut deck, table);
    let ShapeKind::Table(t) = &mut deck.slides[2].shapes[at.shape].kind else {
        panic!("no table");
    };
    for (r, row) in t.rows.iter_mut().enumerate() {
        for (c, cell) in row.cells.iter_mut().enumerate() {
            let p = &mut cell.text.paragraphs[0];
            p.runs = vec![p.end.with_text(format!("R{}C{}", r + 1, c + 1))];
        }
    }
    let text_box = ShapeEdit::text_box(&deck, 0, "").unwrap();
    let at = text_box.target().unwrap();
    h.apply(&mut deck, text_box);
    type_at_end(&mut deck, &mut h, 0, at.shape, "Text box text");
    let oval = ShapeEdit::shape(&deck, 0, "ellipse").unwrap();
    let at = oval.target().unwrap();
    h.apply(&mut deck, oval);
    type_at_end(&mut deck, &mut h, 0, at.shape, "Oval text");
    let line = ShapeEdit::shape(&deck, 0, "line").unwrap();
    h.apply(&mut deck, line);
    let sub = placeholder_ref(&deck, 0, PlaceholderKind::Subtitle);
    type_at_end(&mut deck, &mut h, 0, sub.shape, LONG);
    let fit = Fit {
        font_scale: 0.625,
        line_reduction: 0.2,
    };
    let fit = FitEdit::new(&deck, sub, fit).unwrap();
    h.amend(&mut deck, fit);
    let seven = deck
        .slides
        .iter()
        .position(|s| s.part == read.slides[6].part)
        .unwrap();
    let shape = deck.slides[seven]
        .shapes
        .iter()
        .position(|s| s.name == "TextBox 4")
        .unwrap();
    let remove = ShapeEdit::remove(
        &deck,
        ShapeRef {
            slide: seven,
            shape,
        },
    )
    .unwrap();
    h.apply(&mut deck, remove);
    (read, deck)
}

/// The own shape of `slide` named `name`.
fn named<'a>(slide: &'a Slide, name: &str) -> &'a presentation_model::Shape {
    slide
        .own_shapes()
        .find(|s| s.name.starts_with(name))
        .unwrap_or_else(|| panic!("no {name}"))
}

#[test]
fn inserts_save_and_read_back() {
    let (read, deck) = inserted();
    let source = common::data("layouts.pptx");
    let dest = temp("inserts", "saved.pptx");
    pptx::save(&dest, &source, &read, &deck, &AtomicBool::new(false)).unwrap();
    let back = pptx::read(&dest).unwrap();
    reads_as(&back, &deck);
    let png = png(40, 30);
    let picture = |s: &presentation_model::Shape| match &s.kind {
        ShapeKind::Picture(p) => p.clone(),
        other => panic!("{} is {other:?}", s.name),
    };

    // Slide 2: the PNG fills the body placeholder, its top and bottom cropped to its shape.
    let (now, again) = (&deck.slides[1], &back.slides[1]);
    let body = placeholder(again, PlaceholderKind::Body);
    let (b, was) = (&again.shapes[body], &now.shapes[body]);
    assert_eq!((b.id, &b.slot, b.frame), (was.id, &was.slot, was.frame));
    let (p, q) = (picture(b), picture(was));
    assert_eq!(&*p.image.bytes, &png[..]);
    assert!(p.crop.top > 0.05 && p.crop.left == 0.0, "{:?}", p.crop);
    for (x, y) in [
        (p.crop.top, q.crop.top),
        (p.crop.bottom, q.crop.bottom),
        (p.crop.left, q.crop.left),
        (p.crop.right, q.crop.right),
    ] {
        assert!((x - y).abs() < 1e-5, "{:?} vs {:?}", p.crop, q.crop);
    }

    // Slide 3: the PNG centred at its size, and the table.
    let (now, again) = (&deck.slides[2], &back.slides[2]);
    let pic = named(again, "Picture");
    assert_eq!(&*picture(pic).image.bytes, &png[..]);
    assert_eq!((pic.frame.cx, pic.frame.cy), (40 * 9_525, 30 * 9_525));
    assert_eq!(pic.frame, named(now, "Picture").frame);
    let table = named(again, "Table");
    let ShapeKind::Table(t) = &table.kind else {
        panic!("{:?}", table.kind)
    };
    assert_eq!((t.rows.len(), t.columns.len()), (3, 4));
    let cell = |r: usize, c: usize| &t.rows[r].cells[c];
    assert_eq!(cell(1, 2).text.plain_text(), "R2C3");
    assert_eq!(
        [cell(0, 0).fill, cell(1, 0).fill, cell(2, 3).fill],
        [
            Some(deck_edit::ACCENT),
            Some(deck_edit::BAND_1),
            Some(deck_edit::BAND_2)
        ]
    );
    let white = Some(Outline {
        width: 12_700,
        color: Color::WHITE,
        dash: Dash::Solid,
    });
    let b = cell(2, 1).borders;
    assert_eq!([b.top, b.bottom, b.left, b.right], [white; 4]);
    let header = &cell(0, 1).text.paragraphs[0].runs[0];
    assert_eq!(
        (header.text.as_str(), header.bold, header.color),
        ("R1C2", true, Color::WHITE)
    );
    assert!(!cell(1, 1).text.paragraphs[0].runs[0].bold);

    // Slide 1: the text box, the oval, and the line, in front of what was there; the
    // subtitle shrunk.
    let again = &back.slides[0];
    let names: Vec<&str> = again.own_shapes().map(|s| s.name.as_str()).collect();
    assert_eq!(names.len(), 5, "{names:?}");
    assert!(names[2].starts_with("TextBox"), "{names:?}");
    assert!(names[3].starts_with("Oval"), "{names:?}");
    assert!(names[4].starts_with("Line"), "{names:?}");
    let text_box = named(again, "TextBox");
    let body = text_box.text.as_ref().unwrap();
    assert_eq!(text_box.plain_text(), "Text box text");
    assert_eq!((body.autofit, body.wrap), (Autofit::Grow, true));
    assert_eq!(text_box.fill, Fill::None);
    let oval = named(again, "Oval");
    assert!(matches!(&oval.kind, ShapeKind::Geometry { preset, .. } if preset == "ellipse"));
    assert_eq!(oval.fill, Fill::Solid(deck_edit::ACCENT));
    assert_eq!(oval.plain_text(), "Oval text");
    let run = &oval.text.as_ref().unwrap().paragraphs[0].runs[0];
    assert_eq!(run.color, Color::WHITE);
    let line = named(again, "Line");
    assert!(matches!(&line.kind, ShapeKind::Geometry { preset, .. } if preset == "line"));
    assert_eq!((line.text.is_none(), line.frame.cy), (true, 0));
    assert_eq!(line.outline.map(|o| o.color), Some(deck_edit::ACCENT));
    let sub = &again.shapes[placeholder(again, PlaceholderKind::Subtitle)];
    let body = sub.text.as_ref().unwrap();
    assert_eq!(
        (body.autofit, body.font_scale, body.line_reduction),
        (Autofit::Shrink, 0.625, 0.2)
    );
    assert!(sub.plain_text().ends_with("well past the bottom"));

    // The original slide 7: its second text box gone.
    let seven = back
        .slides
        .iter()
        .position(|s| s.part == read.slides[6].part)
        .unwrap();
    let names: Vec<&str> = back.slides[seven]
        .own_shapes()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(names, ["Title 1", "TextBox 3"]);

    // The copy of the last slide: its shapes, and the PNG.
    let copy = back.slides.last().unwrap();
    assert_eq!(&*picture(named(copy, "Picture")).image.bytes, &png[..]);
    assert_eq!(
        copy.own_shapes().count(),
        read.slides[9].own_shapes().count() + 1
    );

    // The images are new media parts, their type the package's now; slide 1 says how
    // its subtitle shrank; every other part is as it was (the last slide, now two on, has
    // its slide number field written).
    let saved = parts(&dest);
    let media: Vec<&String> = saved
        .keys()
        .filter(|n| n.starts_with("ppt/media/") && n.ends_with(".png"))
        .collect();
    assert_eq!(media.len(), 3, "{media:?}");
    assert!(media.iter().all(|m| saved[*m] == png));
    let types = String::from_utf8(saved["[Content_Types].xml"].clone()).unwrap();
    assert!(types.contains(r#"<Default Extension="png" ContentType="image/png"/>"#));
    let slide1 = String::from_utf8(saved["ppt/slides/slide1.xml"].clone()).unwrap();
    assert!(
        slide1.contains(r#"<a:normAutofit fontScale="62500" lnSpcReduction="20000"/>"#),
        "{slide1}"
    );
    untouched_but(&source, &dest, &changed_parts(&read, &[0, 6, 9], &[]));

    // A saved deck saves again, from the file it was saved to.
    let again = temp("inserts", "again.pptx");
    pptx::save(&again, &dest, &back, &back, &AtomicBool::new(false)).unwrap();
    reads_as(&pptx::read(&again).unwrap(), &deck);
}

/// The lines of shape `name` in a dump slide: its `shape` line and those after it, up to
/// the next shape.
fn shape_lines<'a>(lines: &[&'a str], name: &str) -> Vec<&'a str> {
    let start = lines
        .iter()
        .position(|l| l.starts_with("shape\t") && l.split('\t').nth(10) == Some(name))
        .unwrap_or_else(|| panic!("no {name} in {lines:?}"));
    let len = lines[start + 1..]
        .iter()
        .position(|l| l.starts_with("shape\t"))
        .map_or(lines.len() - start, |n| n + 1);
    lines[start..start + len].to_vec()
}

#[test]
#[ignore = "needs LibreOffice: cargo test -p pptx --test save -- --ignored"]
fn libreoffice_reads_the_inserts() {
    let (read, deck) = inserted();
    let source = private_copy("libreoffice-inserts", "layouts.pptx");
    let dest = temp("libreoffice-inserts", "saved.pptx");
    pptx::save(&dest, &source, &read, &deck, &AtomicBool::new(false)).unwrap();
    let (before, after) = (lo_dump(&source, 2173), lo_dump(&dest, 2173));
    let (old, new) = (dump_slides(&before), dump_slides(&after));
    assert_eq!(new.len(), deck.slides.len());
    let png = "picture\timage/png\t40x30";
    let frame = |lines: &[&str], kind: &str| -> [i64; 4] {
        [3, 4, 5, 6].map(|n| shape_field(lines, kind, n))
    };
    let near = |a: [i64; 4], b: [i64; 4]| a.iter().zip(&b).all(|(x, y)| (x - y).abs() <= 720);

    // Slide 2: the PNG at the body placeholder's frame (the original slide 2 has one of
    // Title and Content's too).
    assert!(new[1].contains(&png), "{:?}", new[1]);
    let (pic, body) = (
        frame(&new[1], "GraphicObjectShape"),
        frame(&old[1], "OutlinerShape"),
    );
    assert!(near(pic, body), "{pic:?} vs {body:?}");

    // Slide 3: the PNG centred, and the table with its fills, borders, and text.
    assert!(new[2].contains(&png), "{:?}", new[2]);
    let f = named(&deck.slides[2], "Picture").frame;
    let pic = frame(&new[2], "GraphicObjectShape");
    assert!(near(pic, [f.x, f.y, f.cx, f.cy]), "{pic:?} vs {f:?}");
    let cells: Vec<&str> = new[2]
        .iter()
        .copied()
        .filter(|l| l.starts_with("cell\t"))
        .collect();
    assert_eq!(cells.len(), 12, "{:?}", new[2]);
    // Each border white, 1 pt: LibreOffice gives each of the two cells a border lies
    // between half its width (to its 1/100 mm).
    let white = |l: &&str| {
        let borders = l.split('\t').nth(4).unwrap_or_default().split(' ');
        borders.map(|b| b.split_once(':')).all(|b| {
            b.is_some_and(|(c, w)| {
                c == "FFFFFF" && (w.parse::<i64>().unwrap() - 6_350).abs() <= 360
            })
        })
    };
    assert!(cells.iter().all(white), "{cells:?}");
    assert!(
        cells[0].starts_with("cell\t0\t0\tsolid 4472C4 100"),
        "{cells:?}"
    );
    assert!(
        cells[4].starts_with("cell\t1\t0\tsolid CFD5EA 100"),
        "{cells:?}"
    );
    assert!(
        cells[8].starts_with("cell\t2\t0\tsolid E9EBF5 100"),
        "{cells:?}"
    );
    assert!(new[2].contains(&"para\t0\tleft\tR2C3"), "{:?}", new[2]);
    let header = new[2]
        .iter()
        .find(|l| l.starts_with("run\t") && l.ends_with("\tR1C1"))
        .unwrap();
    let header: Vec<&str> = header.split('\t').collect();
    assert_eq!((header[2], header[6]), ("1", "FFFFFF"), "{header:?}");

    // Slide 1: what was there (the subtitle aside), then the text box, the oval, and the
    // line.
    let text_free = |lines: &[&str]| -> Vec<String> {
        lines
            .iter()
            .filter(|l| !l.starts_with("para\t") && !l.starts_with("run\t"))
            .map(|l| l.to_string())
            .collect()
    };
    let (was, is) = (text_free(&old[0]), text_free(&new[0]));
    assert_eq!(is[..was.len()], was[..], "{is:?}");
    let shape = |prefix: &str| {
        let s = named(&deck.slides[0], prefix);
        shape_lines(&new[0], &s.name)
    };
    let text_box = shape("TextBox");
    assert!(
        text_box.contains(&"para\t0\tleft\tText box text"),
        "{text_box:?}"
    );
    assert!(text_box.contains(&"fill\tnone"), "{text_box:?}");
    let oval = shape("Oval");
    assert!(oval.contains(&"fill\tsolid 4472C4 100"), "{oval:?}");
    assert!(oval.contains(&"para\t0\tcenter\tOval text"), "{oval:?}");
    let run = oval.iter().find(|l| l.starts_with("run\t")).unwrap();
    assert!(run.contains("\tFFFFFF\t"), "{run}");
    let line = shape("Line");
    assert!(
        line[1..]
            .iter()
            .any(|l| l.starts_with("line\tsolid 4472C4")),
        "{line:?}"
    );
    assert!(
        new[0].contains(&"para\t0\tcenter\tand on again, well past the bottom"),
        "{:?}",
        new[0]
    );
    let saved = parts(&dest);
    let slide1 = String::from_utf8(saved["ppt/slides/slide1.xml"].clone()).unwrap();
    assert!(slide1.contains(r#"fontScale="62500" lnSpcReduction="20000""#));

    // The original slide 7 without its second text box.
    let mut without = old[6].clone();
    let gone = shape_lines(&old[6], "TextBox 4");
    let at = without.iter().position(|l| *l == gone[0]).unwrap();
    without.drain(at..at + gone.len());
    assert_eq!(new[8], without);

    // The copy of the last slide: as it, then the PNG.
    let copy = &new[new.len() - 1];
    assert_eq!(copy[..old[9].len()], old[9][..], "{copy:?}");
    assert!(copy[old[9].len()..].contains(&png), "{copy:?}");

    // Every other slide as LibreOffice read it before.
    for j in [1, 2, 3, 4, 5, 7, 8, 9] {
        assert_eq!(new[j + 2], old[j], "saved slide {}", j + 3);
    }
}
