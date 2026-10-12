//! FRAME-15, without the window: Ctrl+Enter's order through a slide's text placeholders,
//! and Tab and Shift+Tab moving paragraphs a level, with that level's looks, as one step.

use deck_edit::{next_placeholder, relevel, History, ShapeRef, TextPos, MAX_LEVEL};
use presentation_model::{Layer, Paragraph, PlaceholderKind, Presentation, Slide};
use std::path::Path;

fn read(name: &str) -> Presentation {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../pptx/tests/data")
        .join(name);
    pptx::read(&path).unwrap()
}

/// The placeholders Ctrl+Enter visits on `slide`, from nothing edited to the last.
fn stops(slide: &Slide) -> Vec<usize> {
    let mut at = None;
    let mut seen = Vec::new();
    while let Some(next) = next_placeholder(slide, at) {
        seen.push(next);
        at = Some(next);
    }
    seen
}

#[test]
fn ctrl_enter_goes_through_the_text_placeholders_in_order() {
    let deck = read("layouts.pptx");
    // Slide 4, Two Content: the title, then the left and right content placeholders.
    let slide = &deck.slides[3];
    let own: Vec<usize> = (0..slide.shapes.len())
        .filter(|&i| slide.shapes[i].layer == Layer::Slide)
        .collect();
    assert_eq!(stops(slide), own[..3]);
    assert_eq!(next_placeholder(slide, Some(own[2])), None);
    // A slide of drawn shapes: only its title is a stop.
    let fixture = read("fixture.pptx");
    let drawn = &fixture.slides[2];
    let title = deck_edit::title_of(drawn).unwrap();
    assert_eq!(
        next_placeholder(drawn, Some(title)),
        None,
        "drawn shapes are skipped"
    );
}

#[test]
fn ctrl_enter_skips_footers_and_shapes_without_text() {
    let deck = read("layouts.pptx");
    let mut slide = deck.slides[1].clone();
    let title = deck_edit::title_of(&slide).unwrap();
    let body = deck_edit::body_of(&slide).unwrap();
    let template = slide.shapes[body].clone();
    let mut extra = Vec::new();
    for kind in [
        PlaceholderKind::Date,
        PlaceholderKind::Footer,
        PlaceholderKind::SlideNumber,
        PlaceholderKind::Header,
    ] {
        let mut s = template.clone();
        s.placeholder = Some(kind);
        extra.push(s);
    }
    // A content placeholder without a text body, one from the layout, and one without
    // an id: not the slide's text.
    let mut picture = template.clone();
    picture.placeholder = Some(PlaceholderKind::Content);
    picture.text = None;
    extra.push(picture);
    let mut layout = template.clone();
    layout.layer = Layer::Layout;
    extra.push(layout);
    let mut unsaved = template.clone();
    unsaved.id = None;
    extra.push(unsaved);
    // Between the title and the body, and after the body.
    let n = extra.len();
    let at = title + 1;
    for (i, s) in extra.iter().cloned().enumerate() {
        slide.shapes.insert(at + i, s);
    }
    slide.shapes.extend(extra);
    let body = body + n;
    // A content placeholder with text is one.
    let mut content = template.clone();
    content.placeholder = Some(PlaceholderKind::Content);
    slide.shapes.push(content);
    let content = slide.shapes.len() - 1;
    assert_eq!(stops(&slide), [title, body, content]);
    assert_eq!(next_placeholder(&slide, Some(title + 1)), Some(body));
    assert_eq!(next_placeholder(&slide, Some(content)), None);
}

/// Slide 2's body: Content at level 0, then Level 2 ... Level 9 at levels 1 to 8.
fn body(deck: &Presentation) -> ShapeRef {
    ShapeRef {
        slide: 1,
        shape: deck_edit::body_of(&deck.slides[1]).unwrap(),
    }
}

fn paragraphs(deck: &Presentation, s: ShapeRef) -> &[Paragraph] {
    &deck.slides[s.slide].shapes[s.shape]
        .text
        .as_ref()
        .unwrap()
        .paragraphs
}

fn size(p: &Paragraph) -> f64 {
    p.end.size
}

#[test]
fn tab_at_a_paragraphs_start_moves_it_a_level_with_its_looks() {
    let mut deck = read("layouts.pptx");
    let original = deck.clone();
    let s = body(&deck);
    let at = TextPos::new(0, 0);
    let mut history = History::new();
    let edit = relevel(&deck, s, at, at, true).unwrap();
    history.apply(&mut deck, edit);
    let ps = paragraphs(&deck, s);
    let was = paragraphs(&original, s);
    assert_eq!(ps[0].level, 1);
    assert_eq!(ps[0].text(), "Content");
    // Level 2's looks: its size and margins.
    assert_eq!(size(&ps[0]), size(&was[1]));
    assert_eq!(ps[0].margin_left, was[1].margin_left);
    assert_ne!(size(&ps[0]), size(&was[0]));
    assert_eq!(ps[1..], was[1..]);
    // Shift+Tab takes it back, looks and all.
    let edit = relevel(&deck, s, at, at, false).unwrap();
    history.apply(&mut deck, edit);
    assert_eq!(paragraphs(&deck, s)[0].level, 0);
    assert_eq!(size(&paragraphs(&deck, s)[0]), size(&was[0]));
    history.undo(&mut deck);
    history.undo(&mut deck);
    assert_eq!(deck, original);
}

#[test]
fn tab_over_paragraphs_moves_them_all_as_one_step() {
    let mut deck = read("layouts.pptx");
    let original = deck.clone();
    let s = body(&deck);
    let mut history = History::new();
    // From inside Content to inside Level 3: three paragraphs, either order.
    let edit = relevel(&deck, s, TextPos::new(2, 3), TextPos::new(0, 2), true).unwrap();
    history.apply(&mut deck, edit);
    let levels: Vec<u8> = paragraphs(&deck, s).iter().map(|p| p.level).collect();
    assert_eq!(levels, [1, 2, 3, 3, 4, 5, 6, 7, 8]);
    history.undo(&mut deck);
    assert_eq!(deck, original);
}

#[test]
fn tab_does_nothing_inside_a_paragraph_or_past_the_limits() {
    let deck = read("layouts.pptx");
    let s = body(&deck);
    // A caret inside a paragraph, or a range within one.
    let inside = TextPos::new(0, 2);
    assert!(relevel(&deck, s, inside, inside, true).is_none());
    assert!(relevel(&deck, s, TextPos::new(0, 0), inside, true).is_none());
    // Content is at level 0: no higher.
    let start = TextPos::new(0, 0);
    assert!(relevel(&deck, s, start, start, false).is_none());
    // Level 9 is at the deepest level: no lower.
    let deepest = TextPos::new(8, 0);
    assert_eq!(paragraphs(&deck, s)[8].level, MAX_LEVEL);
    assert!(relevel(&deck, s, deepest, deepest, true).is_none());
    // Over both ends, only the one not at the limit moves.
    let edit = relevel(&deck, s, TextPos::new(7, 0), TextPos::new(8, 2), true).unwrap();
    let mut after = deck.clone();
    History::new().apply(&mut after, edit);
    let levels: Vec<u8> = paragraphs(&after, s)[7..].iter().map(|p| p.level).collect();
    assert_eq!(levels, [8, 8]);
}
