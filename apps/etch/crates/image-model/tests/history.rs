//! ETCH-1: undo and redo of pencil strokes, and what history keeps.

use image_model::{Color, Document, Image, Pencil};

fn pencil(width: u32) -> Pencil {
    Pencil {
        color: Color::BLACK,
        width,
    }
}

fn stroke(doc: &mut Document, width: u32, points: &[(f64, f64)]) {
    doc.begin_stroke(pencil(width), points[0]);
    for &p in &points[1..] {
        doc.extend_stroke(p);
    }
    doc.end_stroke();
}

#[test]
fn undo_restores_bytes_and_redo_gives_back_the_stroke() {
    // Strokes crossing tile edges and each other.
    let mut doc = Document::new(Image::new(300, 200, Color::rgba(10, 20, 30, 128)).unwrap());
    let blank = doc.image().clone();
    stroke(&mut doc, 3, &[(5.0, 5.0), (250.0, 150.0), (60.0, 190.0)]);
    let one = doc.image().clone();
    stroke(&mut doc, 7, &[(290.0, 10.0), (10.0, 120.0)]);
    let two = doc.image().clone();
    assert_ne!(one, blank);
    assert_ne!(two, one);

    let changed = doc.undo().unwrap();
    assert_eq!(doc.image(), &one);
    assert!(changed.x <= 10 && changed.right() >= 291);
    doc.undo();
    assert_eq!(doc.image(), &blank);
    assert!(!doc.can_undo());
    assert_eq!(doc.undo(), None);

    doc.redo();
    assert_eq!(doc.image(), &one);
    doc.redo();
    assert_eq!(doc.image(), &two);
    assert!(!doc.can_redo());
    assert_eq!(doc.redo(), None);
}

#[test]
fn undo_takes_back_only_the_last_stroke() {
    let mut doc = Document::new(Image::new(50, 50, Color::WHITE).unwrap());
    stroke(&mut doc, 1, &[(1.0, 1.0), (40.0, 1.0)]);
    stroke(&mut doc, 1, &[(1.0, 1.0), (1.0, 40.0)]);
    doc.undo();
    assert_eq!(doc.image().pixel(30, 1), Some(Color::BLACK));
    assert_eq!(doc.image().pixel(1, 30), Some(Color::WHITE));
    // The pixel both strokes drew keeps the first's ink.
    assert_eq!(doc.image().pixel(1, 1), Some(Color::BLACK));
}

#[test]
fn a_new_stroke_drops_what_was_undone() {
    let mut doc = Document::new(Image::new(50, 50, Color::WHITE).unwrap());
    stroke(&mut doc, 1, &[(1.0, 1.0), (40.0, 1.0)]);
    doc.undo();
    assert!(doc.can_redo());
    stroke(&mut doc, 1, &[(1.0, 5.0), (40.0, 5.0)]);
    assert!(!doc.can_redo());
    doc.undo();
    assert_eq!(doc.image(), &Image::new(50, 50, Color::WHITE).unwrap());
}

#[test]
fn undo_during_a_stroke_takes_back_that_stroke() {
    let mut doc = Document::new(Image::new(50, 50, Color::WHITE).unwrap());
    doc.begin_stroke(pencil(1), (1.0, 1.0));
    doc.extend_stroke((30.0, 30.0));
    assert!(doc.can_undo());
    doc.undo();
    assert_eq!(doc.image(), &Image::new(50, 50, Color::WHITE).unwrap());
    // The stroke ended with the undo: more motion draws nothing.
    assert_eq!(doc.extend_stroke((40.0, 40.0)), None);
    assert!(doc.can_redo());
}

#[test]
fn a_stroke_keeps_only_the_tiles_it_touched() {
    let mut doc = Document::new(Image::new(4096, 4096, Color::WHITE).unwrap());
    let image_bytes = doc.image().data().len();
    stroke(&mut doc, 1, &[(100.0, 100.0), (160.0, 140.0)]);
    // Four 64-pixel tiles of 16 KiB.
    assert_eq!(doc.history_bytes(), 4 * 64 * 64 * 4);
    assert!(doc.history_bytes() * 100 < image_bytes);
    // Zigzagging over the same four tiles in a second stroke saves each of them once.
    doc.begin_stroke(pencil(1), (100.0, 100.0));
    for i in 0..50 {
        doc.extend_stroke((100.0 + i as f64, 100.0 + (i * 13 % 40) as f64));
    }
    doc.end_stroke();
    assert_eq!(doc.history_bytes(), 2 * 4 * 64 * 64 * 4);
}

#[test]
fn edge_tiles_are_kept_clipped_to_the_image() {
    // 100×70: the tiles at the right and bottom are partial.
    let mut doc = Document::new(Image::new(100, 70, Color::WHITE).unwrap());
    let blank = doc.image().clone();
    stroke(&mut doc, 5, &[(99.0, 69.0), (90.0, 67.0)]);
    assert_eq!(doc.history_bytes(), 36 * 6 * 4);
    doc.undo();
    assert_eq!(doc.image(), &blank);
}
