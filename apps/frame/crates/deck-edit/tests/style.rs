//! FRAME-11, without the window: what the inspector changes, each one undoable step:
//! text looks over a range, paragraph looks, a shape's fill and outline, a slide's
//! background and layout. Looks an edit sets are `Source::Edit`, for the save to write.

use deck_edit::{
    BackgroundEdit, History, LayoutEdit, LookEdit, ParagraphLook, ShapeLook, ShapeLookEdit,
    ShapeRef, TextLook, TextPos, BULLET_MARGIN,
};
use presentation_model::{
    Align, Background, Color, Dash, Fill, Layer, Outline, PlaceholderKind, Presentation, Source,
};
use std::path::Path;

fn read(name: &str) -> Presentation {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../pptx/tests/data")
        .join(name);
    pptx::read(&path).unwrap()
}

fn placeholder(deck: &Presentation, slide: usize, kind: PlaceholderKind) -> ShapeRef {
    let shape = deck.slides[slide]
        .shapes
        .iter()
        .position(|s| s.layer == Layer::Slide && s.placeholder == Some(kind))
        .unwrap();
    ShapeRef { slide, shape }
}

fn shape(deck: &Presentation, s: ShapeRef) -> &presentation_model::Shape {
    &deck.slides[s.slide].shapes[s.shape]
}

const RED: Color = Color::rgb(0xC0, 0x00, 0x00);

#[test]
fn a_text_look_covers_the_range_and_undoes() {
    let mut deck = read("layouts.pptx");
    let before = deck.clone();
    let mut h = History::new();
    // "Centered Title": bold on "Title" only.
    let title = placeholder(&deck, 0, PlaceholderKind::CentredTitle);
    let edit = LookEdit::text(
        &deck,
        title,
        TextPos::new(0, 14),
        TextPos::new(0, 9),
        &TextLook::Bold(true),
    )
    .unwrap();
    h.apply(&mut deck, edit);
    let p = &shape(&deck, title).text.as_ref().unwrap().paragraphs[0];
    let runs: Vec<(&str, bool, Source)> = p
        .runs
        .iter()
        .map(|r| (r.text.as_str(), r.bold, r.sources.bold))
        .collect();
    assert_eq!(
        runs,
        [
            ("Centered ", false, Source::Presentation),
            ("Title", true, Source::Edit)
        ]
    );
    assert!(
        !p.end.bold,
        "part of the paragraph: its mark keeps its look"
    );
    // The whole paragraph, red: its mark too; the runs join again.
    let edit = LookEdit::text(
        &deck,
        title,
        TextPos::new(0, 0),
        TextPos::new(0, 14),
        &TextLook::Color(RED),
    )
    .unwrap();
    h.apply(&mut deck, edit);
    let p = &shape(&deck, title).text.as_ref().unwrap().paragraphs[0];
    assert!(p
        .runs
        .iter()
        .all(|r| r.color == RED && r.sources.color == Source::Edit));
    assert_eq!((p.end.color, p.end.sources.color), (RED, Source::Edit));
    for look in [
        TextLook::Font("Liberation Serif".into()),
        TextLook::Size(60.0),
        TextLook::Italic(true),
        TextLook::Underline(true),
    ] {
        let edit =
            LookEdit::text(&deck, title, TextPos::new(0, 0), TextPos::new(0, 8), &look).unwrap();
        h.apply(&mut deck, edit);
    }
    let r = &shape(&deck, title).text.as_ref().unwrap().paragraphs[0].runs[0];
    assert_eq!(
        (
            r.text.as_str(),
            r.font.as_str(),
            r.size,
            r.italic,
            r.underline
        ),
        ("Centered", "Liberation Serif", 60.0, true, true)
    );
    // No range, or the look it has: nothing to do.
    assert!(LookEdit::text(
        &deck,
        title,
        TextPos::new(0, 3),
        TextPos::new(0, 3),
        &TextLook::Bold(true)
    )
    .is_none());
    assert!(LookEdit::text(
        &deck,
        title,
        TextPos::new(0, 0),
        TextPos::new(0, 8),
        &TextLook::Size(60.0)
    )
    .is_none());
    for _ in 0..6 {
        h.undo(&mut deck);
    }
    assert_eq!(deck, before);
}

#[test]
fn paragraph_looks_cover_the_paragraphs_touched() {
    let mut deck = read("layouts.pptx");
    let mut h = History::new();
    // Title and Content's body: three levels of bullets.
    let body = placeholder(&deck, 1, PlaceholderKind::Body);
    let edit = LookEdit::paragraphs(
        &deck,
        body,
        TextPos::new(1, 2),
        TextPos::new(0, 1),
        ParagraphLook::Align(Align::Right),
    )
    .unwrap();
    h.apply(&mut deck, edit);
    let ps = &shape(&deck, body).text.as_ref().unwrap().paragraphs;
    let aligns: Vec<(Align, Source)> = ps.iter().map(|p| (p.align, p.sources.align)).collect();
    assert_eq!(
        aligns[..3],
        [
            (Align::Right, Source::Edit),
            (Align::Right, Source::Edit),
            (Align::Left, Source::Master)
        ]
    );
    let edit = LookEdit::paragraphs(
        &deck,
        body,
        TextPos::new(0, 0),
        TextPos::new(0, 0),
        ParagraphLook::Bullets(false),
    )
    .unwrap();
    h.apply(&mut deck, edit);
    let ps = &shape(&deck, body).text.as_ref().unwrap().paragraphs;
    assert_eq!(
        (ps[0].bullet.is_none(), ps[0].sources.bullet),
        (true, Source::Edit)
    );
    assert!(ps[1].bullet.is_some());
    // Bullets on in a text box without margins: a bullet, hanging in a margin.
    let mut fixture = read("fixture.pptx");
    let text_box = fixture.slides[2]
        .shapes
        .iter()
        .position(|s| s.plain_text().starts_with("Text box"))
        .unwrap();
    let s = ShapeRef {
        slide: 2,
        shape: text_box,
    };
    let edit = LookEdit::paragraphs(
        &fixture,
        s,
        TextPos::new(0, 0),
        TextPos::new(0, 0),
        ParagraphLook::Bullets(true),
    )
    .unwrap();
    h.apply(&mut fixture, edit);
    let p = &shape(&fixture, s).text.as_ref().unwrap().paragraphs[0];
    assert_eq!(p.bullet.as_ref().map(|b| b.text.as_str()), Some("\u{2022}"));
    assert_eq!((p.margin_left, p.indent), (BULLET_MARGIN, -BULLET_MARGIN));
    h.undo(&mut fixture);
    assert!(shape(&fixture, s).text.as_ref().unwrap().paragraphs[0]
        .bullet
        .is_none());
}

#[test]
fn shapes_take_a_fill_and_an_outline() {
    let mut deck = read("fixture.pptx");
    let before = deck.clone();
    let mut h = History::new();
    // Slide 3's blue rectangle.
    let rect = deck.slides[2]
        .shapes
        .iter()
        .position(|s| s.fill == Fill::Solid(Color::rgb(0x44, 0x72, 0xC4)))
        .unwrap();
    let s = ShapeRef {
        slide: 2,
        shape: rect,
    };
    let edit = ShapeLookEdit::new(&deck, s, ShapeLook::Fill(Fill::Solid(RED))).unwrap();
    h.apply(&mut deck, edit);
    let outline = Outline {
        width: 38_100,
        color: RED,
        dash: Dash::Solid,
    };
    let edit = ShapeLookEdit::new(&deck, s, ShapeLook::Outline(Some(outline))).unwrap();
    h.apply(&mut deck, edit);
    let sh = shape(&deck, s);
    assert_eq!(
        (&sh.fill, sh.outline, sh.sources.fill, sh.sources.outline),
        (&Fill::Solid(RED), Some(outline), Source::Edit, Source::Edit)
    );
    assert!(
        ShapeLookEdit::new(&deck, s, ShapeLook::Fill(Fill::Solid(RED))).is_none(),
        "it has that fill"
    );
    // A table's look is its cells': no fill for the frame.
    let table = deck.slides[3]
        .shapes
        .iter()
        .position(|s| matches!(s.kind, presentation_model::ShapeKind::Table(_)))
        .unwrap();
    let t = ShapeRef {
        slide: 3,
        shape: table,
    };
    assert!(ShapeLookEdit::new(&deck, t, ShapeLook::Fill(Fill::Solid(RED))).is_none());
    h.undo(&mut deck);
    h.undo(&mut deck);
    assert_eq!(deck, before);
}

#[test]
fn a_slide_takes_a_background_of_its_own() {
    let mut deck = read("layouts.pptx");
    let mut h = History::new();
    assert_eq!(deck.slides[0].background_source, Source::Master);
    let was = deck.slides[0].background.clone();
    let edit = BackgroundEdit::new(&deck, 0, Background::Solid(RED)).unwrap();
    h.apply(&mut deck, edit);
    assert_eq!(
        (&deck.slides[0].background, deck.slides[0].background_source),
        (&Background::Solid(RED), Source::Edit)
    );
    assert!(BackgroundEdit::new(&deck, 0, Background::Solid(RED)).is_none());
    h.undo(&mut deck);
    assert_eq!(
        (&deck.slides[0].background, deck.slides[0].background_source),
        (&was, Source::Master)
    );
}

#[test]
fn a_layout_change_moves_text_into_the_new_placeholders() {
    let mut deck = read("layouts.pptx");
    let before = deck.clone();
    let mut h = History::new();
    // Slide 1 (Title Slide): "subtitle", its own look bold.
    let sub = placeholder(&deck, 0, PlaceholderKind::Subtitle);
    let edit = LookEdit::text(
        &deck,
        sub,
        TextPos::new(0, 0),
        TextPos::new(0, 8),
        &TextLook::Bold(true),
    )
    .unwrap();
    h.apply(&mut deck, edit);
    let content = deck
        .layouts
        .iter()
        .position(|l| l.name == "Title and Content")
        .unwrap();
    let edit = LayoutEdit::new(&deck, 0, content).unwrap();
    h.apply(&mut deck, edit);
    let slide = &deck.slides[0];
    assert_eq!(slide.layout, Some(content));
    let own: Vec<_> = slide
        .shapes
        .iter()
        .filter(|s| s.layer == Layer::Slide)
        .collect();
    assert_eq!(own.len(), 2, "{own:#?}");
    let title = own
        .iter()
        .find(|s| s.placeholder == Some(PlaceholderKind::Title))
        .unwrap();
    assert_eq!(
        (title.id, title.plain_text().as_str()),
        (Some(2), "Centered Title")
    );
    assert_eq!(title.slot.as_ref().map(|s| &*s.kind), Some("title"));
    // The subtitle's text is the body's: its bullet and alignment, its own bold.
    let body = own
        .iter()
        .find(|s| s.placeholder == Some(PlaceholderKind::Body))
        .unwrap();
    assert_eq!((body.id, body.plain_text().as_str()), (Some(3), "subtitle"));
    let slot = body.slot.as_ref().unwrap();
    assert_eq!((&*slot.kind, slot.idx), ("obj", Some(1)));
    let p = &body.text.as_ref().unwrap().paragraphs[0];
    assert_eq!(p.align, Align::Left);
    assert!(p.bullet.is_some(), "the body's bullet");
    assert_eq!(
        (p.runs[0].bold, p.runs[0].sources.bold),
        (true, Source::Edit)
    );
    let template = deck.layouts[content]
        .slide
        .shapes
        .iter()
        .find(|s| s.placeholder == Some(PlaceholderKind::Body))
        .unwrap();
    assert_eq!(body.frame, template.frame, "the layout's box");
    assert!(LayoutEdit::new(&deck, 0, content).is_none());
    // Title Only: the body's text has nowhere to go and stays where it is.
    let title_only = deck
        .layouts
        .iter()
        .position(|l| l.name == "Title Only")
        .unwrap();
    let edit = LayoutEdit::new(&deck, 0, title_only).unwrap();
    h.apply(&mut deck, edit);
    let texts: Vec<String> = deck.slides[0]
        .shapes
        .iter()
        .filter(|s| s.layer == Layer::Slide)
        .map(|s| s.plain_text())
        .collect();
    assert_eq!(texts, ["Centered Title", "subtitle"]);
    h.undo(&mut deck);
    h.undo(&mut deck);
    h.undo(&mut deck);
    assert_eq!(deck, before);
}

#[test]
fn a_layout_change_drops_empty_placeholders_and_adds_the_new_ones() {
    let mut deck = read("layouts.pptx");
    let layout = |name: &str| deck.layouts.iter().position(|l| l.name == name).unwrap();
    let (title_only, two) = (layout("Title Only"), layout("Two Content"));
    let mut h = History::new();
    let add = deck_edit::SlideEdit::on_layout(&deck, Some(0), title_only).unwrap();
    h.apply(&mut deck, add);
    let edit = LayoutEdit::new(&deck, 1, two).unwrap();
    h.apply(&mut deck, edit);
    let own: Vec<_> = deck.slides[1]
        .shapes
        .iter()
        .filter(|s| s.layer == Layer::Slide)
        .collect();
    let kinds: Vec<_> = own.iter().map(|s| s.placeholder).collect();
    assert_eq!(
        kinds
            .iter()
            .filter(|k| **k == Some(PlaceholderKind::Body))
            .count(),
        2
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|k| **k == Some(PlaceholderKind::Title))
            .count(),
        1
    );
    let mut ids: Vec<u32> = own.iter().filter_map(|s| s.id).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), own.len(), "every shape its own id");
    // And back: Title Only has no body; empty, they go.
    let edit = LayoutEdit::new(&deck, 1, title_only).unwrap();
    h.apply(&mut deck, edit);
    let own = deck.slides[1]
        .shapes
        .iter()
        .filter(|s| s.layer == Layer::Slide)
        .count();
    assert_eq!(own, 1);
}

#[test]
fn the_reader_says_where_paragraph_shape_and_background_looks_are_set() {
    let deck = read("layouts.pptx");
    let sub = placeholder(&deck, 0, PlaceholderKind::Subtitle);
    let p = &shape(&deck, sub).text.as_ref().unwrap().paragraphs[0];
    assert_eq!((p.align, p.sources.align), (Align::Centre, Source::Layout));
    assert_eq!(deck.slides[0].background_source, Source::Master);
    let fixture = read("fixture.pptx");
    let rect = fixture.slides[2]
        .shapes
        .iter()
        .find(|s| s.fill == Fill::Solid(Color::rgb(0x44, 0x72, 0xC4)))
        .unwrap();
    assert_eq!(rect.sources.fill, Source::Own);
    let body = &fixture.slides[1]
        .shapes
        .iter()
        .find(|s| s.placeholder == Some(PlaceholderKind::Body))
        .unwrap();
    let levels = &body.text.as_ref().unwrap().levels;
    assert_eq!(levels.len(), 9, "a look for each level");
}
