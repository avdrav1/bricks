//! FRAME-10 acceptance, without the window: a dragged box snaps to the slide's edges and
//! centre, the layout's placeholders, other shapes' edges and centres, and equal spacing,
//! within the tolerance (6 screen pixels, given in EMU), with guides; moving and resizing
//! undo and redo.

use deck_edit::{snap, FrameEdit, Grab, Guide, History, ShapeRef};
use presentation_model::{Frame, Layer, PlaceholderKind, Presentation, Shape, ShapeKind};
use std::path::Path;

const TOL: i64 = 60_000;
const M: i64 = 1_000_000;

fn fixture() -> Presentation {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../pptx/tests/data/fixture.pptx");
    pptx::read(&path).unwrap()
}

fn frame(x: i64, y: i64, cx: i64, cy: i64) -> Frame {
    Frame {
        x,
        y,
        cx,
        cy,
        ..Frame::default()
    }
}

/// The fixture with a slide of its own boxes (no layout to align with), each at `frames`;
/// the first is the one dragged.
fn deck_with(frames: &[Frame]) -> Presentation {
    let mut deck = fixture();
    let template: Shape = deck.slides[2]
        .shapes
        .iter()
        .find(|s| s.layer == Layer::Slide && matches!(s.kind, ShapeKind::Geometry { .. }))
        .cloned()
        .unwrap();
    let slide = &mut deck.slides[2];
    slide.layout = None;
    slide.shapes = frames
        .iter()
        .enumerate()
        .map(|(i, f)| Shape {
            frame: *f,
            id: Some(100 + i as u32),
            text: None,
            ..template.clone()
        })
        .collect();
    deck
}

fn moved(deck: &Presentation, to: Frame) -> deck_edit::Snapped {
    snap(deck, 2, 0, to, Grab::Move, TOL)
}

fn vertical_at(guides: &[Guide], x: i64) -> bool {
    guides.iter().any(|g| g.from.0 == x && g.to.0 == x)
}

fn horizontal_at(guides: &[Guide], y: i64) -> bool {
    guides.iter().any(|g| g.from.1 == y && g.to.1 == y)
}

#[test]
fn boxes_snap_to_the_slides_edges_and_centre() {
    let deck = deck_with(&[frame(0, 0, M, M)]);
    let size = deck.size;
    // Its centre 30,000 EMU right of the slide's: centred, with a guide down the middle.
    let x = size.cx / 2 - M / 2 + 30_000;
    let s = moved(&deck, frame(x, 2 * M + 7, M, M));
    assert_eq!(s.frame.x + M / 2, size.cx / 2);
    assert!(vertical_at(&s.guides, size.cx / 2), "{:?}", s.guides);
    let g = s.guides.iter().find(|g| g.from.0 == size.cx / 2).unwrap();
    assert_eq!(
        (g.from.1, g.to.1),
        (0, size.cy),
        "the guide runs the slide's height"
    );
    // Near the left and bottom edges.
    let s = moved(&deck, frame(-40_000, size.cy - M + 50_000, M, M));
    assert_eq!((s.frame.x, s.frame.y + M), (0, size.cy));
    assert!(vertical_at(&s.guides, 0) && horizontal_at(&s.guides, size.cy));
    // Past the tolerance: where it was dropped, no guides.
    let s = moved(&deck, frame(x + 2 * TOL, 2 * M + 7, M, M));
    assert_eq!(s.frame, frame(x + 2 * TOL, 2 * M + 7, M, M));
    assert!(s.guides.is_empty());
}

#[test]
fn boxes_snap_to_other_shapes_edges_and_centres() {
    let other = frame(2 * M, 3 * M, 2 * M, M);
    let deck = deck_with(&[frame(0, 0, M / 2, M / 2), other]);
    // Its left edge near the other's right edge; its centre near the other's middle.
    let s = moved(
        &deck,
        frame(4 * M + 25_000, 3 * M + M / 4 - 20_000, M / 2, M / 2),
    );
    assert_eq!(s.frame.x, 4 * M);
    assert_eq!(s.frame.y + M / 4, 3 * M + M / 2);
    assert!(vertical_at(&s.guides, 4 * M));
    assert!(horizontal_at(&s.guides, 3 * M + M / 2));
    // Its top near the other's top.
    let s = moved(&deck, frame(6 * M + 333, 3 * M - 45_000, M / 2, M / 2));
    assert_eq!(s.frame.y, 3 * M);
}

#[test]
fn boxes_snap_to_the_layouts_placeholders() {
    let mut deck = fixture();
    // Slide 2's layout has a title and a body; its title, empty, is dragged.
    let slide = 1;
    let layout = &deck.layouts[deck.slides[slide].layout.unwrap()];
    let body = layout
        .slide
        .shapes
        .iter()
        .find(|s| s.placeholder == Some(PlaceholderKind::Body))
        .unwrap()
        .frame;
    let title = deck.slides[slide]
        .shapes
        .iter()
        .position(|s| s.layer == Layer::Slide && s.placeholder == Some(PlaceholderKind::Title))
        .unwrap();
    // The slide's own body placeholder moves out of the way, so only the layout's is there.
    let own_body = deck.slides[slide]
        .shapes
        .iter()
        .position(|s| s.layer == Layer::Slide && s.placeholder == Some(PlaceholderKind::Body))
        .unwrap();
    deck.slides[slide].shapes[own_body].frame = frame(8 * M, 0, 1000, 1000);
    // A third of a box just under it, its other edges and centre near nothing else.
    let at = frame(body.x + 3 * M + 7, body.y + body.cy + 30_000, M, M / 3);
    let s = snap(&deck, slide, title, at, Grab::Move, TOL);
    assert_eq!(s.frame.y, body.y + body.cy, "onto the layout body's bottom");
    assert!(horizontal_at(&s.guides, body.y + body.cy));
}

#[test]
fn a_box_snaps_to_equal_spacing_in_a_row() {
    let a = frame(M, 2 * M, M, M);
    let b = frame(5 * M / 2, 2 * M, M, M);
    let w = 4 * M / 5;
    let deck = deck_with(&[frame(0, 0, w, M), a, b]);
    // After b by the gap between a and b (half an M), give or take.
    let s = moved(&deck, frame(4 * M + 40_000, 2 * M + 123_456, w, M));
    assert_eq!(s.frame.x, 4 * M);
    // Guides mark both gaps at the row's height.
    let mid = s.frame.y + M / 2;
    assert!(s.guides.contains(&Guide {
        from: (2 * M, mid),
        to: (5 * M / 2, mid)
    }));
    assert!(s.guides.contains(&Guide {
        from: (7 * M / 2, mid),
        to: (4 * M, mid)
    }));
    // Before a, and centred between them (a 2 M gap fits a half-M box).
    let s = moved(&deck, frame(-3 * M / 10 + 30_000, 2 * M, w, M));
    assert_eq!(s.frame.x, -3 * M / 10);
    let wide = deck_with(&[frame(0, 0, M / 2, M / 2), a, frame(4 * M, 2 * M, M, M)]);
    let s = snap(
        &wide,
        2,
        0,
        frame(2 * M + 3 * M / 4 - 20_000, 2 * M, M / 2, M / 2),
        Grab::Move,
        TOL,
    );
    assert_eq!(s.frame.x, 2 * M + 3 * M / 4);
}

#[test]
fn resizing_snaps_only_the_edges_held() {
    let deck = deck_with(&[frame(M, M, M, M)]);
    let centre = deck.size.cx / 2;
    let grab = Grab::Resize {
        left: false,
        right: true,
        top: false,
        bottom: false,
    };
    let s = snap(&deck, 2, 0, frame(M, M, centre - M + 35_000, M), grab, TOL);
    assert_eq!((s.frame.x, s.frame.x + s.frame.cx), (M, centre));
    assert!(vertical_at(&s.guides, centre));
    // The left edge, held, snaps to the slide's; the right stays.
    let left = Grab::Resize {
        left: true,
        right: false,
        top: false,
        bottom: false,
    };
    let s = snap(&deck, 2, 0, frame(25_000, M, 2 * M - 25_000, M), left, TOL);
    assert_eq!((s.frame.x, s.frame.x + s.frame.cx), (0, 2 * M));
}

#[test]
fn moves_and_resizes_undo_and_redo() {
    let mut deck = fixture();
    let mut h = History::new();
    let shapes = &deck.slides[2].shapes;
    let rect = shapes
        .iter()
        .position(|s| s.layer == Layer::Slide && s.id.is_some() && !s.grouped)
        .unwrap();
    let start = shapes[rect].frame;
    let s = ShapeRef {
        slide: 2,
        shape: rect,
    };
    let to = Frame {
        x: start.x + M,
        ..start
    };
    let step = FrameEdit::new(&deck, s, to).unwrap();
    h.apply(&mut deck, step);
    let bigger = Frame { cx: 3 * M, ..to };
    let step = FrameEdit::new(&deck, s, bigger).unwrap();
    h.apply(&mut deck, step);
    assert_eq!(deck.slides[2].shapes[rect].frame, bigger);
    assert!(FrameEdit::new(&deck, s, bigger).is_none(), "nothing to do");
    h.undo(&mut deck);
    assert_eq!(deck.slides[2].shapes[rect].frame, to);
    h.undo(&mut deck);
    assert_eq!(deck.slides[2].shapes[rect].frame, start);
    h.redo(&mut deck);
    assert_eq!(deck.slides[2].shapes[rect].frame, to);
    // A layout's or master's shape doesn't move from the slide.
    let behind = deck.slides[0]
        .shapes
        .iter()
        .position(|s| s.layer != Layer::Slide);
    if let Some(i) = behind {
        let s = ShapeRef { slide: 0, shape: i };
        assert!(FrameEdit::new(&deck, s, frame(0, 0, M, M)).is_none());
    }
}
