//! ETCH-17: Crop to Selection. The image becomes the selection's bounds and its pixels;
//! undo restores its size and pixels exactly, and redo crops again.

use image_model::{Color, Document, Image, Rect, Transform};

/// An image whose pixel (x, y) is `f(x, y)`.
fn image(w: u32, h: u32, f: impl Fn(u32, u32) -> Color) -> Image {
    let data = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .flat_map(|(x, y)| f(x, y).to_bytes())
        .collect();
    Image::from_rgba(w, h, data).unwrap()
}

/// Every pixel different, opaque, see-through, and transparent ones hiding a colour among
/// them.
fn pattern(x: u32, y: u32) -> Color {
    let (r, g) = ((x * 13 % 256) as u8, (y * 7 % 256) as u8);
    match (x + 3 * y) % 4 {
        0 => Color::rgb(r, g, 200),
        1 => Color::rgba(r, g, 100, 128),
        2 => Color::rgba(r, g, 50, 0),
        _ => Color::rgba(r, g, 0, 254),
    }
}

#[test]
fn cropping_makes_the_image_the_selections_bounds_and_pixels_and_undo_restores_it() {
    let original = image(150, 90, pattern);
    let mut doc = Document::new(original.clone());
    let rect = Rect::new(37, 11, 70, 65);
    assert_eq!(
        doc.transform(Transform::Crop(rect)),
        Some(Rect::new(0, 0, 70, 65))
    );
    let cropped = doc.image().clone();
    assert_eq!((cropped.width(), cropped.height()), (70, 65));
    // Every pixel is the one it was over, byte for byte (transparent ones' colour too).
    assert_eq!(cropped.data(), original.copy_rect(rect));
    assert_eq!(
        cropped.pixel(0, 0),
        original.pixel(37, 11),
        "the selection's top-left is the image's"
    );
    assert_eq!(doc.position(), Some(1));

    assert_eq!(doc.undo(), Some(Rect::new(0, 0, 150, 90)));
    assert_eq!(
        doc.image(),
        &original,
        "undo restores the size and every byte"
    );
    assert_eq!(doc.position(), Some(0));
    assert_eq!(doc.redo(), Some(Rect::new(0, 0, 70, 65)));
    assert_eq!(doc.image(), &cropped, "redo crops again");
    assert_eq!(doc.position(), Some(1));
}

#[test]
fn a_crop_keeps_the_image_before_it_once_and_steps_before_and_after_it_undo_exactly() {
    let original = image(130, 100, pattern);
    let mut doc = Document::new(original.clone());
    doc.fill((5.0, 5.0), Color::rgb(1, 2, 3), 255);
    let filled = doc.image().clone();
    doc.transform(Transform::Crop(Rect::new(64, 0, 66, 70)));
    // The whole image from before the crop, plus the fill's tiles.
    assert!(doc.history_bytes() >= (130 * 100 * 4) as usize);
    let cropped = doc.image().clone();
    let pencil = image_model::Pencil {
        color: Color::rgb(9, 9, 9),
        width: 3,
    };
    doc.begin_stroke(pencil, (2.0, 2.0));
    doc.extend_stroke((60.0, 60.0));
    doc.end_stroke();
    let drawn = doc.image().clone();
    doc.undo();
    assert_eq!(
        doc.image(),
        &cropped,
        "a stroke after a crop undoes on the cropped image"
    );
    doc.undo();
    assert_eq!(doc.image(), &filled);
    doc.undo();
    assert_eq!(doc.image(), &original);
    doc.redo();
    doc.redo();
    doc.redo();
    assert_eq!(
        doc.image(),
        &drawn,
        "redo goes through the crop to the stroke on it"
    );
}

#[test]
fn a_crop_partly_off_the_image_crops_to_the_part_on_it_and_a_whole_or_empty_one_makes_no_step() {
    let original = image(40, 30, pattern);
    let mut doc = Document::new(original.clone());
    assert_eq!(
        doc.transform(Transform::Crop(Rect::new(0, 0, 40, 30))),
        None
    );
    assert_eq!(
        doc.transform(Transform::Crop(Rect::new(50, 0, 10, 10))),
        None
    );
    assert_eq!(doc.position(), Some(0));
    assert_eq!(
        doc.transform(Transform::Crop(Rect::new(30, 20, 20, 20))),
        Some(Rect::new(0, 0, 10, 10))
    );
    assert_eq!(
        doc.image().data(),
        original.copy_rect(Rect::new(30, 20, 10, 10))
    );
    // A transform computed elsewhere (off the UI thread) installs the same.
    let mut other = Document::new(original.clone());
    let crop = Transform::Crop(Rect::new(30, 20, 20, 20));
    let result = crop.apply(other.image()).unwrap();
    assert_eq!(other.install(crop, result), Some(Rect::new(0, 0, 10, 10)));
    assert_eq!(other.image(), doc.image());
    other.undo();
    assert_eq!(other.image(), &original);
}

#[test]
fn a_one_pixel_crop_keeps_that_pixel() {
    let original = image(9, 7, pattern);
    let mut doc = Document::new(original.clone());
    doc.transform(Transform::Crop(Rect::new(8, 6, 1, 1)));
    assert_eq!((doc.image().width(), doc.image().height()), (1, 1));
    assert_eq!(doc.image().pixel(0, 0), original.pixel(8, 6));
    doc.undo();
    assert_eq!(doc.image(), &original);
}
