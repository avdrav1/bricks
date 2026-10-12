//! FRAME-8 acceptance: saving patches the package the deck was read from. Text edits, a
//! new slide, a duplicate (with its notes), a deleted slide, and a moved slide save and
//! read back as the deck is; parts nothing touched are copied byte for byte; a saved deck
//! saves again. LibreOffice's reading of the saved deck: the ignored test at the end.

mod common;

use deck_edit::{FrameEdit, History, ShapeRef, SlideEdit, TextEdit};
use presentation_model::{
    Frame, Layer, Paragraph, PlaceholderKind, Presentation, ShapeKind, Slide,
};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

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
                r.sources
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

/// The deck as it reads: per slide, its layout, notes, and each own shape's id,
/// placeholder, and paragraphs.
fn readings(deck: &Presentation) -> Vec<String> {
    deck.slides
        .iter()
        .map(|s| {
            let mut out = format!("layout {:?}, notes {:?}\n", s.layout, s.notes);
            for sh in s.shapes.iter().filter(|sh| sh.layer == Layer::Slide) {
                out.push_str(&format!(
                    "shape {:?} {:?} {:?}\n",
                    sh.id, sh.placeholder, sh.frame
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

/// `scripts/lo_pptx.py dump FILE`: LibreOffice's reading of a deck.
fn lo_dump(file: &Path) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../..");
    let out = std::process::Command::new("python3")
        .arg(root.join("scripts/lo_pptx.py"))
        .args(["dump", &file.to_string_lossy()])
        .env("LO_PORT", "2171")
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
    let (before, after) = (lo_dump(&source), lo_dump(&dest));
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
