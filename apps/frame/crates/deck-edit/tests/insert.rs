//! FRAME-12, without the window: pictures fill the selected or first empty picture or
//! content placeholder, else land centred and scaled to fit; text boxes, shapes, and
//! tables come in centred; each insert, and a shrink to fit with the typing that caused
//! it, undoes in one step.

use deck_edit::{
    shrinks, Fit, FitEdit, History, ShapeEdit, ShapeRef, SlideEdit, TextEdit, TextPos, ACCENT,
    BAND_1, EMU_PER_PIXEL,
};
use presentation_model::{
    Autofit, Fill, Image, ImageFormat, Layer, PlaceholderKind, Presentation, ShapeKind,
};
use std::path::Path;
use std::sync::Arc;

fn read(name: &str) -> Presentation {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../pptx/tests/data")
        .join(name);
    pptx::read(&path).unwrap()
}

fn image() -> Image {
    Image {
        format: ImageFormat::Png,
        bytes: Arc::from(&b"\x89PNG not really"[..]),
    }
}

/// layouts.pptx with a new slide 2 on layout `name`.
fn with_slide(name: &str) -> (Presentation, History) {
    let mut deck = read("layouts.pptx");
    let mut h = History::new();
    let layout = deck.layouts.iter().position(|l| l.name == name).unwrap();
    let add = SlideEdit::on_layout(&deck, Some(0), layout).unwrap();
    h.apply(&mut deck, add);
    (deck, h)
}

#[test]
fn a_picture_fills_the_first_empty_content_placeholder() {
    let (mut deck, mut h) = with_slide("Title and Content");
    let before = deck.clone();
    let body = deck.slides[1]
        .shapes
        .iter()
        .position(|s| s.placeholder == Some(PlaceholderKind::Body))
        .unwrap();
    let place = deck.slides[1].shapes[body].clone();
    // Four times as wide as high: cropped left and right to the placeholder's shape.
    let edit = ShapeEdit::picture(&deck, 1, image(), (400, 100), None).unwrap();
    h.apply(&mut deck, edit);
    let s = &deck.slides[1].shapes[body];
    let ShapeKind::Picture(p) = &s.kind else {
        panic!("{:?}", s.kind)
    };
    assert_eq!(
        (s.id, &s.slot, s.frame),
        (place.id, &place.slot, place.frame)
    );
    assert!(s.text.is_none());
    let (pw, ph) = (place.frame.cx as f64, place.frame.cy as f64);
    let kept = 1.0 - p.crop.left - p.crop.right;
    assert!(
        (400.0 * kept / 100.0 - pw / ph).abs() < 1e-9,
        "the placeholder's shape"
    );
    assert_eq!((p.crop.top, p.crop.bottom), (0.0, 0.0));
    assert!((p.crop.left - p.crop.right).abs() < 1e-12, "centred");
    // A second picture: no empty placeholder left, so it lands centred.
    let edit = ShapeEdit::picture(&deck, 1, image(), (200, 100), None).unwrap();
    h.apply(&mut deck, edit);
    let last = deck.slides[1].shapes.last().unwrap();
    let (cx, cy) = (200 * EMU_PER_PIXEL, 100 * EMU_PER_PIXEL);
    assert_eq!(
        (last.frame.x, last.frame.y, last.frame.cx, last.frame.cy),
        ((deck.size.cx - cx) / 2, (deck.size.cy - cy) / 2, cx, cy)
    );
    assert!(last.placeholder.is_none());
    h.undo(&mut deck);
    h.undo(&mut deck);
    assert_eq!(deck, before);
}

#[test]
fn a_picture_fills_the_selected_placeholder() {
    let (deck, _) = with_slide("Two Content");
    let bodies: Vec<usize> = deck.slides[1]
        .shapes
        .iter()
        .enumerate()
        .filter(|(_, s)| s.placeholder == Some(PlaceholderKind::Body))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(bodies.len(), 2);
    let edit = ShapeEdit::picture(&deck, 1, image(), (100, 100), Some(bodies[1])).unwrap();
    assert_eq!(edit.target().map(|s| s.shape), Some(bodies[1]));
    // A title doesn't take a picture: the first body does.
    let title = deck.slides[1]
        .shapes
        .iter()
        .position(|s| s.placeholder == Some(PlaceholderKind::Title))
        .unwrap();
    let edit = ShapeEdit::picture(&deck, 1, image(), (100, 100), Some(title)).unwrap();
    assert_eq!(edit.target().map(|s| s.shape), Some(bodies[0]));
}

#[test]
fn without_a_placeholder_a_big_picture_scales_to_fit_the_slide() {
    let (mut deck, mut h) = with_slide("Title Only");
    let edit = ShapeEdit::picture(&deck, 1, image(), (4000, 1000), None).unwrap();
    h.apply(&mut deck, edit);
    let s = deck.slides[1].shapes.last().unwrap();
    assert_eq!(
        (s.frame.x, s.frame.cx),
        (0, deck.size.cx),
        "as wide as the slide"
    );
    assert_eq!(s.frame.cy, deck.size.cx / 4);
    assert_eq!(s.frame.y, (deck.size.cy - s.frame.cy) / 2);
}

#[test]
fn text_boxes_shapes_and_tables_come_in_centred_and_undo() {
    let (mut deck, mut h) = with_slide("Title Only");
    let before = deck.clone();
    let next = deck.slides[1].next_id;
    let edit = ShapeEdit::text_box(&deck, 1, "Text").unwrap();
    h.apply(&mut deck, edit);
    let edit = ShapeEdit::shape(&deck, 1, "ellipse").unwrap();
    h.apply(&mut deck, edit);
    let edit = ShapeEdit::shape(&deck, 1, "line").unwrap();
    h.apply(&mut deck, edit);
    let edit = ShapeEdit::table(&deck, 1, 3, 4).unwrap();
    h.apply(&mut deck, edit);
    let added = &deck.slides[1].shapes[before.slides[1].shapes.len()..];
    let ids: Vec<u32> = added.iter().map(|s| s.id.unwrap()).collect();
    assert_eq!(ids, [next, next + 1, next + 2, next + 3]);
    for s in added {
        let f = s.frame;
        assert_eq!(
            (f.x * 2 + f.cx, f.y * 2 + f.cy),
            (deck.size.cx, deck.size.cy),
            "{}",
            s.name
        );
        assert_eq!(s.layer, Layer::Slide);
    }
    let text = added[0].text.as_ref().unwrap();
    assert_eq!(
        (added[0].plain_text().as_str(), text.autofit),
        ("Text", Autofit::Grow)
    );
    assert_eq!(
        text.paragraphs[0].runs[0].size, 18.0,
        "the layout's text box look"
    );
    assert_eq!(added[1].fill, Fill::Solid(ACCENT));
    assert!(added[1].text.is_some());
    assert_eq!(
        (&added[2].fill, added[2].text.is_none()),
        (&Fill::None, true)
    );
    let ShapeKind::Table(t) = &added[3].kind else {
        panic!()
    };
    assert_eq!((t.rows.len(), t.columns.len()), (3, 4));
    assert_eq!(t.rows[0].cells[0].fill, Some(ACCENT));
    assert!(t.rows[0].cells[0].text.paragraphs[0].end.bold);
    assert_eq!(t.rows[1].cells[0].fill, Some(BAND_1));
    for _ in 0..4 {
        h.undo(&mut deck);
    }
    assert_eq!(deck.slides[1].shapes, before.slides[1].shapes);
}

#[test]
fn a_shape_deletes_and_comes_back() {
    let mut deck = read("fixture.pptx");
    let before = deck.clone();
    let mut h = History::new();
    let at = ShapeRef { slide: 2, shape: 3 };
    let edit = ShapeEdit::remove(&deck, at).unwrap();
    h.apply(&mut deck, edit);
    assert_eq!(
        deck.slides[2].shapes.len(),
        before.slides[2].shapes.len() - 1
    );
    h.undo(&mut deck);
    assert_eq!(deck, before);
    // The layout's shapes don't delete from the slide.
    let behind = deck.slides[0]
        .shapes
        .iter()
        .position(|s| s.layer != Layer::Slide);
    if let Some(shape) = behind {
        assert!(ShapeEdit::remove(&deck, ShapeRef { slide: 0, shape }).is_none());
    }
}

#[test]
fn a_shrink_to_fit_undoes_with_the_typing_that_caused_it() {
    let mut deck = read("layouts.pptx");
    let before = deck.clone();
    let mut h = History::new();
    let sub = deck.slides[0]
        .shapes
        .iter()
        .position(|s| s.placeholder == Some(PlaceholderKind::Subtitle))
        .unwrap();
    let s = ShapeRef {
        slide: 0,
        shape: sub,
    };
    assert!(shrinks(&deck.slides[0].shapes[sub]), "a placeholder");
    let at = TextPos::new(0, 8);
    let typed = TextEdit::typing(&deck, s, at, at, " more").unwrap();
    h.apply(&mut deck, typed);
    let fit = Fit {
        font_scale: 0.85,
        line_reduction: 0.1,
    };
    let shrink = FitEdit::new(&deck, s, fit).unwrap();
    h.amend(&mut deck, shrink);
    let t = deck.slides[0].shapes[sub].text.as_ref().unwrap();
    assert_eq!(
        (t.font_scale, t.line_reduction, t.autofit),
        (0.85, 0.1, Autofit::Shrink)
    );
    assert!(FitEdit::new(&deck, s, fit).is_none());
    // Typing on joins the step.
    let at = TextPos::new(0, 13);
    let typed = TextEdit::typing(&deck, s, at, at, "r").unwrap();
    h.apply(&mut deck, typed);
    assert_eq!(deck.slides[0].shapes[sub].plain_text(), "subtitle morer");
    h.undo(&mut deck);
    assert_eq!(deck, before);
    h.redo(&mut deck);
    let t = deck.slides[0].shapes[sub].text.as_ref().unwrap();
    assert_eq!(
        (
            t.font_scale,
            deck.slides[0].shapes[sub].plain_text().as_str()
        ),
        (0.85, "subtitle morer")
    );
}

#[test]
fn the_reader_gives_a_text_box_look_and_free_ids() {
    let deck = read("layouts.pptx");
    for l in &deck.layouts {
        let t = l.text_box.as_ref().unwrap();
        assert_eq!(t.paragraphs[0].end.size, 18.0, "{}", l.name);
    }
    let fixture = read("fixture.pptx");
    for s in &fixture.slides {
        assert!(s
            .shapes
            .iter()
            .filter_map(|s| s.id)
            .all(|id| id < s.next_id));
    }
    // A text box doesn't shrink; it grows.
    let text_box = fixture.slides[2]
        .shapes
        .iter()
        .find(|s| s.plain_text().starts_with("Text box"))
        .unwrap();
    assert!(!shrinks(text_box));
}
