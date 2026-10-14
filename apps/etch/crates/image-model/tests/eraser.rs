//! ETCH-3: the eraser erases to transparency, and undoes.

use image_model::{Color, Document, Image, Pencil};

fn erase(doc: &mut Document, width: u32, points: &[(f64, f64)]) {
    doc.begin_stroke(Pencil::eraser(width), points[0]);
    for &p in &points[1..] {
        doc.extend_stroke(p);
    }
    doc.end_stroke();
}

#[test]
fn erasing_makes_exactly_the_strokes_pixels_transparent() {
    let red = Color::rgb(200, 10, 10);
    let mut doc = Document::new(Image::new(40, 20, red).unwrap());
    // A 5-wide eraser from (5, 10) to (30, 10): the union of radius-2.5 discs centred on
    // each pixel of the line.
    erase(&mut doc, 5, &[(5.5, 10.5), (30.5, 10.5)]);
    for y in 0..20i32 {
        for x in 0..40i32 {
            let inside = (5..=30).any(|cx| (x - cx).pow(2) + (y - 10).pow(2) <= 6);
            let want = if inside { Color::TRANSPARENT } else { red };
            assert_eq!(
                doc.image().pixel(x as u32, y as u32),
                Some(want),
                "({x}, {y})"
            );
        }
    }
}

#[test]
fn erasing_undoes_and_redoes() {
    let mut doc = Document::new(Image::new(64, 64, Color::WHITE).unwrap());
    doc.begin_stroke(
        Pencil {
            color: Color::BLACK,
            width: 3,
        },
        (10.0, 10.0),
    );
    doc.extend_stroke((50.0, 50.0));
    doc.end_stroke();
    let drawn = doc.image().clone();
    erase(&mut doc, 9, &[(5.0, 30.0), (60.0, 30.0)]);
    let erased = doc.image().clone();
    assert_eq!(erased.pixel(30, 30), Some(Color::TRANSPARENT));
    assert_eq!(erased.pixel(10, 10), Some(Color::BLACK));
    doc.undo();
    assert_eq!(doc.image(), &drawn);
    doc.redo();
    assert_eq!(doc.image(), &erased);
}
