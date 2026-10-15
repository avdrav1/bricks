//! ETCH-9: the fill bucket, exact colour, 4-connected.

use image_model::{Color, Document, Image, Pencil, Rect, TILE};

const RED: Color = Color::rgb(237, 28, 36);

fn line(doc: &mut Document, color: Color, a: (f64, f64), b: (f64, f64)) {
    doc.begin_stroke(Pencil { color, width: 1 }, a);
    doc.extend_stroke(b);
    doc.end_stroke();
}

#[test]
fn a_fill_stops_at_any_other_colour() {
    let mut doc = Document::new(Image::new(20, 20, Color::WHITE).unwrap());
    // A wall one level off white: still another colour.
    let wall = Color::rgb(254, 255, 255);
    line(&mut doc, wall, (10.0, 0.0), (10.0, 19.0));
    let changed = doc.fill((2.5, 3.5), RED, 0).unwrap();
    assert_eq!(changed, Rect::new(0, 0, 10, 20));
    for y in 0..20 {
        for x in 0..20 {
            let want = match x {
                0..10 => RED,
                10 => wall,
                _ => Color::WHITE,
            };
            assert_eq!(doc.image().pixel(x, y), Some(want), "{x},{y}");
        }
    }
}

#[test]
fn a_fill_goes_only_through_edges_not_corners() {
    let mut doc = Document::new(Image::new(16, 16, Color::WHITE).unwrap());
    // A diagonal of single pixels: corners touch across it, edges don't.
    line(&mut doc, Color::BLACK, (0.0, 0.0), (15.0, 15.0));
    doc.fill((0.0, 15.0), RED, 0);
    for y in 0..16 {
        for x in 0..16 {
            let want = match () {
                _ if x == y => Color::BLACK,
                _ if x < y => RED,
                _ => Color::WHITE,
            };
            assert_eq!(doc.image().pixel(x, y), Some(want), "{x},{y}");
        }
    }
}

#[test]
fn a_fill_reaches_around_corners_and_into_pockets() {
    // A comb: the region winds up and down between its teeth, so rows are revisited.
    let mut doc = Document::new(Image::new(30, 20, Color::WHITE).unwrap());
    for x in (3..30).step_by(4) {
        let (a, b) = if (x / 4) % 2 == 0 {
            (0.0, 16.0)
        } else {
            (3.0, 19.0)
        };
        line(&mut doc, Color::BLACK, (x as f64, a), (x as f64, b));
    }
    doc.fill((0.0, 0.0), RED, 0);
    let image = doc.image();
    for y in 0..20 {
        for x in 0..30 {
            let p = image.pixel(x, y).unwrap();
            assert_ne!(p, Color::WHITE, "{x},{y} was reached");
        }
    }
}

#[test]
fn a_fill_fills_transparent_regions() {
    // Transparent pixels whose hidden colours differ, as an eraser and a PNG leave them,
    // count as one colour; a pixel of alpha 1 doesn't.
    let mut data = Vec::new();
    for y in 0..10u8 {
        for x in 0..10u8 {
            let a = if x == 7 { 1 } else { 0 };
            data.extend_from_slice(&[x * 20, y * 20, 7, a]);
        }
    }
    let mut doc = Document::new(Image::from_rgba(10, 10, data.clone()).unwrap());
    doc.fill((1.0, 1.0), RED, 0).unwrap();
    for y in 0..10 {
        for x in 0..10 {
            let p = doc.image().pixel(x, y).unwrap();
            match x {
                0..7 => assert_eq!(p, RED, "{x},{y}"),
                7 => assert_eq!(p.a, 1),
                _ => assert_eq!(p.a, 0),
            }
        }
    }
    doc.undo();
    assert_eq!(doc.image().data(), &data[..]);
}

#[test]
fn filling_with_the_colour_there_changes_nothing() {
    let mut doc = Document::new(Image::new(10, 10, RED).unwrap());
    assert_eq!(doc.fill((3.0, 3.0), RED, 0), None);
    assert_eq!(doc.fill((-1.0, 3.0), Color::BLACK, 0), None);
    assert_eq!(doc.fill((3.0, 10.0), Color::BLACK, 0), None);
    assert!(!doc.can_undo());
    assert_eq!(doc.position(), Some(0));
}

#[test]
fn a_fill_undoes_keeping_only_the_tiles_it_touched() {
    let mut doc = Document::new(Image::new(1024, 1024, Color::WHITE).unwrap());
    // A box from 100 to 200 each way: its inside spans tiles 1 to 3 each way.
    for (a, b) in [
        ((100.0, 100.0), (200.0, 100.0)),
        ((200.0, 100.0), (200.0, 200.0)),
        ((200.0, 200.0), (100.0, 200.0)),
        ((100.0, 200.0), (100.0, 100.0)),
    ] {
        line(&mut doc, Color::BLACK, a, b);
    }
    let lines = doc.image().clone();
    let kept = doc.history_bytes();
    let changed = doc.fill((150.0, 150.0), RED, 0).unwrap();
    assert_eq!(changed, Rect::new(101, 101, 99, 99));
    let filled = doc.image().clone();
    let tile = (TILE * TILE * 4) as usize;
    // The inside, 101 to 199, overlaps tiles 1 to 3 in each direction: 9 tiles.
    assert_eq!(doc.history_bytes() - kept, 9 * tile);
    assert_eq!(filled.pixel(101, 101), Some(RED));
    assert_eq!(filled.pixel(100, 150), Some(Color::BLACK));
    assert_eq!(filled.pixel(99, 150), Some(Color::WHITE));
    assert_eq!(doc.undo(), Some(Rect::new(64, 64, 192, 192)));
    assert_eq!(doc.image(), &lines);
    doc.redo();
    assert_eq!(doc.image(), &filled);
    // Outside the box: the rest of the image, every tile but the one wholly inside the box
    // (128 to 191 each way).
    let outside = doc
        .fill((5.0, 5.0), Color::rgba(0, 0, 255, 100), 0)
        .unwrap();
    assert_eq!(outside, Rect::new(0, 0, 1024, 1024));
    assert_eq!(doc.history_bytes() - kept, 9 * tile + 255 * tile);
    doc.undo();
    assert_eq!(doc.image(), &filled);
}

#[test]
fn a_tolerance_fills_near_colours_and_zero_is_exact() {
    let mut data = Vec::new();
    for x in 0..10u8 {
        data.extend_from_slice(&[100 + x, 100, 100, 255]);
    }
    let image = Image::from_rgba(10, 1, data).unwrap();
    let mut exact = Document::new(image.clone());
    exact.fill((0.0, 0.0), RED, 0);
    assert_eq!(exact.image().pixel(1, 0), Some(Color::rgb(101, 100, 100)));
    let mut near = Document::new(image);
    // Within 3 of 100: 100 to 103. The colour put down is within it too: each pixel is
    // still filled once.
    near.fill((0.0, 0.0), Color::rgb(102, 100, 100), 3);
    for x in 0..4 {
        assert_eq!(near.image().pixel(x, 0), Some(Color::rgb(102, 100, 100)));
    }
    assert_eq!(near.image().pixel(4, 0), Some(Color::rgb(104, 100, 100)));
}
