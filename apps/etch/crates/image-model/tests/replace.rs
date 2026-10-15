//! ETCH-25: replacing the whole image (a crash journal's pixels put back over the file's).
//! One undo step whatever the sizes: undo gives back the image before it exactly, its size
//! too, and redo the replacement.

use image_model::{Color, Document, Image, Pencil, Rect};

/// An image whose pixel (x, y) is `f(x, y)`.
fn image(w: u32, h: u32, f: impl Fn(u32, u32) -> Color) -> Image {
    let data = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .flat_map(|(x, y)| f(x, y).to_bytes())
        .collect();
    Image::from_rgba(w, h, data).unwrap()
}

/// Every pixel different, opaque, see-through, and transparent ones hiding a colour.
fn pattern(x: u32, y: u32) -> Color {
    let (r, g) = ((x * 13 % 256) as u8, (y * 7 % 256) as u8);
    match (x + 3 * y) % 3 {
        0 => Color::rgb(r, g, 200),
        1 => Color::rgba(r, g, 100, 128),
        _ => Color::rgba(r, g, 50, 0),
    }
}

#[test]
fn replacing_the_image_is_one_step_that_undoes_and_redoes_exactly() {
    let file = image(150, 90, pattern);
    let mut doc = Document::new(file.clone());
    let journal = image(150, 90, |x, y| pattern(y, x));
    assert_eq!(doc.replace(journal.clone()), Rect::new(0, 0, 150, 90));
    assert_eq!(doc.image(), &journal);
    assert_eq!(doc.position(), Some(1));
    assert!(doc.can_undo() && !doc.can_redo());

    assert_eq!(doc.undo(), Some(Rect::new(0, 0, 150, 90)));
    assert_eq!(doc.image(), &file, "one undo gives back the file's pixels");
    assert_eq!(doc.position(), Some(0));
    assert!(!doc.can_undo());
    assert_eq!(doc.redo(), Some(Rect::new(0, 0, 150, 90)));
    assert_eq!(doc.image(), &journal, "redo puts the journal's pixels back");
    assert_eq!(doc.position(), Some(1));
}

#[test]
fn a_replacement_of_another_size_undoes_to_the_old_size() {
    let file = image(130, 100, pattern);
    let mut doc = Document::new(file.clone());
    let resized = image(64, 200, |x, y| pattern(x + 5, y));
    assert_eq!(doc.replace(resized.clone()), Rect::new(0, 0, 64, 200));
    // The image before it, kept whole.
    assert_eq!(doc.history_bytes(), 130 * 100 * 4);
    // Steps after it undo back onto it.
    let pencil = Pencil {
        color: Color::rgb(9, 9, 9),
        width: 3,
    };
    doc.begin_stroke(pencil, (2.0, 2.0));
    doc.extend_stroke((60.0, 190.0));
    doc.end_stroke();
    let drawn = doc.image().clone();
    doc.undo();
    assert_eq!(doc.image(), &resized);
    doc.undo();
    assert_eq!(doc.image(), &file, "the old size and every byte");
    doc.redo();
    assert_eq!(doc.image(), &resized);
    doc.redo();
    assert_eq!(doc.image(), &drawn);
}

#[test]
fn replacing_with_the_same_pixels_is_still_a_step_and_drops_the_redo() {
    let file = image(40, 30, pattern);
    let mut doc = Document::new(file.clone());
    doc.fill((1.0, 1.0), Color::rgb(1, 2, 3), 255);
    doc.undo();
    assert!(doc.can_redo());
    doc.replace(file.clone());
    assert_eq!(
        doc.position(),
        Some(2),
        "a new step, numbered after the fill"
    );
    assert!(!doc.can_redo());
    doc.undo();
    assert_eq!(doc.image(), &file);
    assert_eq!(doc.position(), Some(0));
}
