//! FRAME-1 acceptance: slide count, order, size, and every shape's frame match
//! LibreOffice's reading of the same decks (`scripts/lo_pptx.py dump`, committed next to
//! the decks), and placeholders without a frame take their layout's or master's.
//!
//! Tolerance: LibreOffice keeps lengths in 1/100 mm (360 EMU) and the dump multiplies
//! them back, so the dump is exact only to that unit. Text frames that grow with their
//! text (`<a:spAutoFit/>`) also come back one unit larger on each side, centred on the same
//! point. A frame matches when each of x, y, width, and height is within 2 units (720 EMU)
//! of LibreOffice's; rotation within 0.05 degree (LibreOffice keeps 1/100 degree).

mod common;

use common::{dump, DShape, DECKS};
use presentation_model::{Frame, Layer, Shape, ShapeKind};

const TOLERANCE: i64 = 720;

fn read(name: &str) -> presentation_model::Presentation {
    pptx::read(&common::data(&format!("{name}.pptx"))).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// Whether LibreOffice's shape kind is what Frame read.
fn same_kind(model: &ShapeKind, lo: &str) -> bool {
    match model {
        ShapeKind::Picture => lo == "GraphicObjectShape",
        ShapeKind::Table => lo == "TableShape",
        ShapeKind::Unsupported { label } if label == "Chart" => lo == "OLE2Shape",
        ShapeKind::Unsupported { .. } => !matches!(lo, "GraphicObjectShape" | "TableShape"),
        ShapeKind::Geometry { .. } => !matches!(
            lo,
            "GraphicObjectShape" | "TableShape" | "OLE2Shape" | "GroupShape"
        ),
    }
}

fn rotation_matches(a: f64, b: f64) -> bool {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d) <= 0.05
}

fn frame_matches(f: &Frame, lo: &DShape) -> bool {
    (f.x - lo.x).abs() <= TOLERANCE
        && (f.y - lo.y).abs() <= TOLERANCE
        && (f.cx - lo.w).abs() <= TOLERANCE
        && (f.cy - lo.h).abs() <= TOLERANCE
        && rotation_matches(f.rotation, lo.rotation)
}

#[test]
fn slides_and_frames_match_libreoffice() {
    let (mut checked, mut worst) = (0, 0);
    for name in DECKS {
        let deck = read(name);
        let lo = dump(name);
        assert!(
            (deck.size.cx - lo.size.0).abs() <= 360 && (deck.size.cy - lo.size.1).abs() <= 360,
            "{name}: slide size {:?} vs LibreOffice {:?}",
            deck.size,
            lo.size
        );
        assert_eq!(deck.slides.len(), lo.slides.len(), "{name}: slide count");
        for (i, (slide, lo_slide)) in deck.slides.iter().zip(&lo.slides).enumerate() {
            let at = format!("{name} slide {}", i + 1);
            assert_eq!(slide.hidden, lo_slide.hidden, "{at}: hidden");
            let pairs = common::pairs(slide, lo_slide, &at);
            for (shape, lo_shape) in pairs {
                let at = format!("{at} shape {:?}", shape.name);
                assert!(
                    same_kind(&shape.kind, &lo_shape.kind),
                    "{at}: {:?} vs LibreOffice {}",
                    shape.kind,
                    lo_shape.kind
                );
                assert!(
                    frame_matches(&shape.frame, lo_shape),
                    "{at}: frame {:?} vs LibreOffice {:?}",
                    shape.frame,
                    (
                        lo_shape.x,
                        lo_shape.y,
                        lo_shape.w,
                        lo_shape.h,
                        lo_shape.rotation
                    )
                );
                checked += 1;
                worst = worst
                    .max((shape.frame.x - lo_shape.x).abs())
                    .max((shape.frame.y - lo_shape.y).abs())
                    .max((shape.frame.cx - lo_shape.w).abs())
                    .max((shape.frame.cy - lo_shape.h).abs());
            }
        }
    }
    assert!(checked > 60, "only {checked} shapes compared");
    eprintln!("{checked} shapes; largest difference from LibreOffice {worst} EMU");
}

#[test]
fn notes_match_libreoffice() {
    for name in DECKS {
        let deck = read(name);
        for (i, (slide, lo)) in deck.slides.iter().zip(&dump(name).slides).enumerate() {
            assert_eq!(slide.notes, lo.notes, "{name} slide {} notes", i + 1);
        }
    }
}

/// layouts.pptx's placeholders have empty `<p:spPr/>`: they sit where the layout puts
/// them, or the master where the layout has no frame either. The fixture's second slide
/// lost its title's `<a:xfrm>` (scripts/lo_pptx.py patches it out).
#[test]
fn placeholders_without_a_frame_take_their_layouts_or_masters() {
    let deck = read("layouts");
    let lo = dump("layouts");
    let mut inherited = 0;
    for (i, (slide, lo_slide)) in deck.slides.iter().zip(&lo.slides).enumerate() {
        for (shape, lo_shape) in common::pairs(slide, lo_slide, &format!("slide {}", i + 1)) {
            if shape.placeholder.is_some() {
                assert!(frame_matches(&shape.frame, lo_shape), "{}", shape.name);
                // Orphans (a title on a Blank layout) have no frame, there and here.
                if lo_shape.w > 0 {
                    assert!(shape.frame.cx > 0 && shape.frame.cy > 0, "{}", shape.name);
                    inherited += 1;
                }
            }
        }
    }
    assert!(inherited >= 15, "{inherited} placeholders");
    let fixture = read("fixture");
    let title = fixture.slides[1].own_shapes().next().unwrap();
    assert_eq!(title.layer, Layer::Slide);
    assert!(title.frame.cx > 0, "the title takes the layout's frame");
    assert!(frame_matches(
        &title.frame,
        &dump("fixture").slides[1].shapes[0]
    ));
}

/// The fixture's fourth slide has a group scaled 1.5 times, moved, and rotated by 30
/// degrees: its children land where LibreOffice puts them.
#[test]
fn group_children_take_the_group_transform() {
    let deck = read("fixture");
    let lo = dump("fixture");
    let lo_children: Vec<&DShape> = lo.slides[3]
        .shapes
        .iter()
        .filter(|s| s.depth == 1)
        .collect();
    assert_eq!(lo_children.len(), 2);
    let ours: Vec<&Shape> = deck.slides[3]
        .own_shapes()
        .filter(|s| (s.frame.rotation - 30.0).abs() < 1e-6)
        .collect();
    assert_eq!(ours.len(), 2);
    for (shape, lo_shape) in ours.iter().zip(lo_children) {
        assert!(
            frame_matches(&shape.frame, lo_shape),
            "{:?} vs {lo_shape:?}",
            shape.frame
        );
        // Scaled 1.5 times from 1440000 x 1080000.
        assert_eq!((shape.frame.cx, shape.frame.cy), (2_160_000, 1_620_000));
    }
}
