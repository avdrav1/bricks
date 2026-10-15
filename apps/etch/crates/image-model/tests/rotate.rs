//! ETCH-20: Rotate 90° either way and 180°, Flip horizontally and vertically. Four quarter
//! turns and two flips each give back the original bytes; a quarter turn swaps width and
//! height; each undoes, by its inverse, keeping no image.

use image_model::{Color, Document, Flip, Image, Rect, Transform, Turn};

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

const ALL: [Transform; 5] = [
    Transform::Rotate(Turn::Right),
    Transform::Rotate(Turn::Left),
    Transform::Rotate(Turn::Half),
    Transform::Flip(Flip::Horizontal),
    Transform::Flip(Flip::Vertical),
];

/// Sizes odd and even, wide, tall, a single row and column, and one large enough to be
/// split across threads (over 1 MiB).
const SIZES: [(u32, u32); 6] = [(13, 9), (8, 20), (1, 7), (6, 1), (1, 1), (700, 410)];

#[test]
fn four_quarter_turns_two_half_turns_and_two_flips_each_give_back_the_original_bytes() {
    for (w, h) in SIZES {
        let original = image(w, h, pattern);
        for (t, times) in ALL.into_iter().zip([4, 4, 2, 2, 2]) {
            let mut turned = original.clone();
            for _ in 0..times {
                turned = t.apply(&turned).unwrap();
            }
            assert_eq!(turned, original, "{t:?} × {times} on {w}×{h}");
        }
    }
}

#[test]
fn each_moves_every_pixel_where_it_says_and_a_quarter_turn_swaps_width_and_height() {
    for (w, h) in SIZES {
        let original = image(w, h, pattern);
        let at = |x: u32, y: u32| original.pixel(x, y).unwrap();
        let expected = [
            // Turned right (clockwise): the left column becomes the top row.
            image(h, w, |x, y| at(y, h - 1 - x)),
            // Turned left: the top row becomes the left column, read upwards.
            image(h, w, |x, y| at(w - 1 - y, x)),
            image(w, h, |x, y| at(w - 1 - x, h - 1 - y)),
            image(w, h, |x, y| at(w - 1 - x, y)),
            image(w, h, |x, y| at(x, h - 1 - y)),
        ];
        for (t, want) in ALL.into_iter().zip(expected) {
            assert_eq!(t.apply(&original).unwrap(), want, "{t:?} on {w}×{h}");
        }
    }
    let wide = image(13, 9, pattern);
    let turned = Transform::Rotate(Turn::Right).apply(&wide).unwrap();
    assert_eq!((turned.width(), turned.height()), (9, 13));
}

#[test]
fn each_undoes_exactly_by_its_inverse_and_redoes_keeping_no_image() {
    let original = image(13, 9, pattern);
    for t in ALL {
        let mut doc = Document::new(original.clone());
        let done = t.apply(&original).unwrap();
        assert_eq!(
            doc.transform(t),
            Some(Rect::new(0, 0, done.width(), done.height()))
        );
        assert_eq!(doc.image(), &done);
        assert_eq!(doc.history_bytes(), 0, "{t:?} keeps no image");
        assert_eq!(doc.undo(), Some(Rect::new(0, 0, 13, 9)));
        assert_eq!(
            doc.image(),
            &original,
            "{t:?}: undo gives back size and bytes"
        );
        assert_eq!(doc.position(), Some(0));
        doc.redo();
        assert_eq!(doc.image(), &done, "{t:?}: redo turns it again");
        assert_eq!(doc.position(), Some(1));
    }
    // A run of them, with a stroke between, undoes back to the start and redoes to the end.
    let mut doc = Document::new(original.clone());
    doc.transform(Transform::Rotate(Turn::Right));
    doc.transform(Transform::Flip(Flip::Horizontal));
    let pencil = image_model::Pencil {
        color: Color::rgb(1, 2, 3),
        width: 2,
    };
    doc.begin_stroke(pencil, (1.0, 1.0));
    doc.extend_stroke((7.0, 10.0));
    doc.end_stroke();
    doc.transform(Transform::Rotate(Turn::Half));
    doc.transform(Transform::Rotate(Turn::Left));
    let end = doc.image().clone();
    while doc.can_undo() {
        doc.undo();
    }
    assert_eq!(doc.image(), &original);
    while doc.can_redo() {
        doc.redo();
    }
    assert_eq!(doc.image(), &end);
}
