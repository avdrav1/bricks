//! ETCH-1: new images and pencil strokes.

use image_model::{Color, Document, Image, Pencil, Rect, SizeError, MAX_SIDE};
use std::collections::BTreeSet;

const BLACK: Pencil = Pencil {
    color: Color::BLACK,
    width: 1,
};

fn white(w: u32, h: u32) -> Document {
    Document::new(Image::new(w, h, Color::WHITE).unwrap())
}

/// Every pixel that isn't white.
fn inked(image: &Image) -> BTreeSet<(u32, u32)> {
    let mut out = BTreeSet::new();
    for y in 0..image.height() {
        for x in 0..image.width() {
            if image.pixel(x, y) != Some(Color::WHITE) {
                out.insert((x, y));
            }
        }
    }
    out
}

fn stroke(doc: &mut Document, pencil: Pencil, points: &[(f64, f64)]) -> Option<Rect> {
    let mut changed = doc.begin_stroke(pencil, points[0]);
    for &p in &points[1..] {
        if let Some(r) = doc.extend_stroke(p) {
            changed = Some(changed.map_or(r, |c| c.union(r)));
        }
    }
    doc.end_stroke();
    changed
}

#[test]
fn a_new_image_holds_its_fill_at_every_pixel() {
    let fill = Color::rgba(1, 2, 3, 4);
    let image = Image::new(300, 200, fill).unwrap();
    assert_eq!((image.width(), image.height()), (300, 200));
    assert_eq!(image.data().len(), 300 * 200 * 4);
    assert!(image
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| p == &[1, 2, 3, 4]));
    assert_eq!(image.pixel(300, 0), None);
}

#[test]
fn an_image_needs_a_pixel_and_at_most_max_side_on_each_side() {
    assert_eq!(Image::new(0, 10, Color::WHITE), Err(SizeError::Empty));
    assert_eq!(Image::new(10, 0, Color::WHITE), Err(SizeError::Empty));
    assert_eq!(
        Image::new(MAX_SIDE + 1, 1, Color::WHITE),
        Err(SizeError::TooLarge {
            width: MAX_SIDE + 1,
            height: 1
        })
    );
    assert!(Image::new(MAX_SIDE, 1, Color::WHITE).is_ok());
}

#[test]
fn an_image_from_rgba_holds_those_pixels() {
    let data: Vec<u8> = (0..3 * 2 * 4).map(|v| v as u8).collect();
    let image = Image::from_rgba(3, 2, data.clone()).unwrap();
    assert_eq!(image.data(), &data[..]);
    assert_eq!(image.pixel(2, 1), Some(Color::rgba(20, 21, 22, 23)));
    assert_eq!(Image::from_rgba(0, 2, Vec::new()), Err(SizeError::Empty));
    assert_eq!(
        Image::from_rgba(MAX_SIDE + 1, 1, Vec::new()),
        Err(SizeError::TooLarge {
            width: MAX_SIDE + 1,
            height: 1
        })
    );
}

#[test]
fn a_one_pixel_line_sets_exactly_bresenhams_pixels() {
    let mut doc = white(10, 6);
    // Fractions floor to the pixel they're in.
    let changed = stroke(&mut doc, BLACK, &[(0.2, 0.9), (5.7, 2.1)]);
    let want: BTreeSet<_> = [(0, 0), (1, 0), (2, 1), (3, 1), (4, 2), (5, 2)].into();
    assert_eq!(inked(doc.image()), want);
    assert_eq!(changed, Some(Rect::new(0, 0, 6, 3)));
    assert_eq!(doc.image().pixel(2, 1), Some(Color::BLACK));
}

#[test]
fn a_stroke_joins_every_point_without_gaps() {
    let mut doc = white(20, 20);
    stroke(
        &mut doc,
        BLACK,
        &[(2.0, 2.0), (12.0, 2.0), (12.0, 9.0), (2.0, 19.0)],
    );
    let ink = inked(doc.image());
    let want: BTreeSet<_> = (2..=12)
        .map(|x| (x, 2))
        .chain((2..=9).map(|y| (12, y)))
        .chain((0..=10).map(|i| (12 - i, 9 + i)))
        .collect();
    assert_eq!(ink, want);
}

#[test]
fn a_click_draws_one_pixel_and_reports_it() {
    let mut doc = white(8, 8);
    let changed = stroke(&mut doc, BLACK, &[(3.5, 4.5)]);
    assert_eq!(inked(doc.image()), [(3, 4)].into());
    assert_eq!(changed, Some(Rect::new(3, 4, 1, 1)));
}

#[test]
fn a_wide_pencil_stamps_a_disc() {
    let mut doc = white(21, 21);
    let red = Pencil {
        color: Color::rgb(255, 0, 0),
        width: 5,
    };
    let changed = stroke(&mut doc, red, &[(10.0, 10.0)]);
    let want: BTreeSet<_> = (-2i32..=2)
        .flat_map(|dy| (-2i32..=2).map(move |dx| (dx, dy)))
        .filter(|(dx, dy)| dx * dx + dy * dy <= 6)
        .map(|(dx, dy)| ((10 + dx) as u32, (10 + dy) as u32))
        .collect();
    assert_eq!(want.len(), 21);
    assert_eq!(inked(doc.image()), want);
    assert_eq!(changed, Some(Rect::new(8, 8, 5, 5)));
    assert_eq!(doc.image().pixel(10, 8), Some(Color::rgb(255, 0, 0)));
}

#[test]
fn a_wide_line_is_its_discs_along_the_line() {
    let mut doc = white(30, 12);
    let pencil = Pencil {
        color: Color::BLACK,
        width: 3,
    };
    stroke(&mut doc, pencil, &[(5.0, 5.0), (20.0, 5.0)]);
    // A 3-wide disc is its whole square, so the line is a 3-pixel band.
    let want: BTreeSet<_> = (4..=21)
        .flat_map(|x| (4..=6).map(move |y| (x, y)))
        .collect();
    assert_eq!(inked(doc.image()), want);
}

#[test]
fn strokes_clip_at_the_edges() {
    let mut doc = white(8, 8);
    let changed = stroke(&mut doc, BLACK, &[(-10.0, -10.0), (3.0, 3.0)]);
    assert_eq!(inked(doc.image()), (0..=3).map(|i| (i, i)).collect());
    assert_eq!(changed, Some(Rect::new(0, 0, 4, 4)));

    let wide = Pencil {
        color: Color::BLACK,
        width: 9,
    };
    let changed = stroke(&mut doc, wide, &[(7.0, 7.0), (1e12, f64::NAN)]);
    assert_eq!(changed.map(|r| (r.right(), r.bottom())), Some((8, 8)));
}

#[test]
fn a_stroke_off_the_image_changes_nothing_and_leaves_no_step() {
    let mut doc = white(8, 8);
    assert_eq!(stroke(&mut doc, BLACK, &[(-5.0, 2.0), (-1.0, 9.0)]), None);
    assert!(inked(doc.image()).is_empty());
    assert!(!doc.can_undo());
}

#[test]
fn the_changed_rect_holds_every_changed_pixel() {
    let mut doc = white(64, 64);
    let before = doc.image().clone();
    let pencil = Pencil {
        color: Color::BLACK,
        width: 4,
    };
    let rect = stroke(
        &mut doc,
        pencil,
        &[(10.0, 30.0), (40.0, 12.0), (50.0, 50.0)],
    )
    .unwrap();
    let mut diff = None::<Rect>;
    for y in 0..64 {
        for x in 0..64 {
            if before.pixel(x, y) != doc.image().pixel(x, y) {
                assert!(rect.contains(x, y), "({x}, {y}) changed outside {rect:?}");
                let px = Rect::new(x, y, 1, 1);
                diff = Some(diff.map_or(px, |d| d.union(px)));
            }
        }
    }
    // Tight: no wider than what changed.
    assert_eq!(diff, Some(rect));
}
