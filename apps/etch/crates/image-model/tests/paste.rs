//! ETCH-16: pasting. A clipboard image's rows (padded, as a texture download gives them)
//! become an image exactly; one larger than an image can be keeps its top-left; pasted
//! pixels larger than the image float whole and land clipped as one undo step.

use image_model::{Color, Document, Floating, Image, SizeError, MAX_SIDE};

/// An image whose pixel (x, y) is `f(x, y)`.
fn image(w: u32, h: u32, f: impl Fn(u32, u32) -> Color) -> Image {
    let data = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .flat_map(|(x, y)| f(x, y).to_bytes())
        .collect();
    Image::from_rgba(w, h, data).unwrap()
}

fn pattern(x: u32, y: u32) -> Color {
    let (r, g) = ((x * 13 % 256) as u8, (y * 7 % 256) as u8);
    match (x + y) % 3 {
        0 => Color::rgb(r, g, 200),
        1 => Color::rgba(r, g, 100, 128),
        _ => Color::rgba(r, g, 50, 0),
    }
}

#[test]
fn padded_rows_become_the_image_exactly() {
    let expected = image(5, 3, pattern);
    // Each row padded to 24 bytes, the padding a byte no pixel has.
    let stride = 24;
    let mut data = vec![0xEE; stride * 2 + 20];
    for y in 0..3 {
        let row = &expected.data()[y * 20..y * 20 + 20];
        data[y * stride..y * stride + 20].copy_from_slice(row);
    }
    assert_eq!(Image::from_rows(5, 3, &data, stride).unwrap(), expected);
    // Unpadded rows too.
    assert_eq!(
        Image::from_rows(5, 3, expected.data(), 20).unwrap(),
        expected
    );
    assert_eq!(Image::from_rows(0, 3, &[], 0), Err(SizeError::Empty));
}

#[test]
fn rows_past_the_largest_side_keep_the_top_left() {
    let width = MAX_SIDE + 3;
    let data: Vec<u8> = (0..width * 2 * 4).map(|i| (i % 251) as u8).collect();
    let pasted = Image::from_rows(width, 2, &data, width as usize * 4).unwrap();
    assert_eq!((pasted.width(), pasted.height()), (MAX_SIDE, 2));
    let row = MAX_SIDE as usize * 4;
    assert_eq!(&pasted.data()[..row], &data[..row]);
    assert_eq!(
        &pasted.data()[row..],
        &data[width as usize * 4..width as usize * 4 + row]
    );
}

#[test]
fn a_paste_larger_than_the_image_floats_whole_and_lands_clipped_as_one_step() {
    let original = image(10, 8, |_, _| Color::rgb(255, 255, 255));
    let mut doc = Document::new(original.clone());
    let pasted = image(14, 11, pattern);
    let mut floating = Floating::new(pasted.clone(), (0, 0));
    assert_eq!(floating.size(), (14, 11));
    assert_eq!(floating.source(), None);
    floating.move_by(-2, -1);
    let changed = doc.land(floating).unwrap();
    assert_eq!((changed.width, changed.height), (10, 8));
    // Every pixel the paste's, from where it was moved to, transparent ones too.
    let expected = image(10, 8, |x, y| pasted.pixel(x + 2, y + 1).unwrap());
    assert_eq!(doc.image(), &expected);
    assert_eq!(doc.position(), Some(1));
    doc.undo();
    assert_eq!(doc.image(), &original);
    doc.redo();
    assert_eq!(doc.image(), &expected);
}
