//! ETCH-19: Resize Canvas. Each of the nine anchors places the old pixels where it says,
//! the new area transparent or the background colour; shrinking crops per the anchor;
//! undo restores the size and every pixel.

use image_model::{Anchor, Color, Document, Image, Rect, Transform};

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

fn canvas(width: u32, height: u32, anchor: Anchor, fill: Color) -> Transform {
    Transform::ResizeCanvas {
        width,
        height,
        anchor,
        fill,
    }
}

/// What an `old`-sized image `original` becomes on a `w` × `h` canvas with its top-left
/// pixel at `(dx, dy)`: its pixels where they land, `fill` elsewhere.
fn placed(original: &Image, (w, h): (u32, u32), (dx, dy): (i64, i64), fill: Color) -> Image {
    image(w, h, |x, y| {
        let (sx, sy) = (x as i64 - dx, y as i64 - dy);
        match sx >= 0 && sy >= 0 {
            true => original.pixel(sx as u32, sy as u32).unwrap_or(fill),
            false => fill,
        }
    })
}

#[test]
fn each_anchor_places_the_old_pixels_where_it_says_when_enlarging() {
    let original = image(10, 7, pattern);
    let fill = Color::rgb(63, 72, 204);
    // 10×7 on 15×12: 5 more across, 5 more down; centred, the odd pixel goes right and
    // down (2 left, 3 right).
    let expected = [
        (Anchor::TopLeft, (0, 0)),
        (Anchor::Top, (2, 0)),
        (Anchor::TopRight, (5, 0)),
        (Anchor::Left, (0, 2)),
        (Anchor::Centre, (2, 2)),
        (Anchor::Right, (5, 2)),
        (Anchor::BottomLeft, (0, 5)),
        (Anchor::Bottom, (2, 5)),
        (Anchor::BottomRight, (5, 5)),
    ];
    assert_eq!(expected.map(|(a, _)| a), Anchor::ALL);
    for (anchor, at) in expected {
        assert_eq!(anchor.offset((10, 7), (15, 12)), at, "{anchor:?}");
        for fill in [Color::TRANSPARENT, fill] {
            let mut doc = Document::new(original.clone());
            assert_eq!(
                doc.transform(canvas(15, 12, anchor, fill)),
                Some(Rect::new(0, 0, 15, 12))
            );
            assert_eq!(
                doc.image(),
                &placed(&original, (15, 12), at, fill),
                "{anchor:?} with {fill:?}"
            );
            doc.undo();
            assert_eq!(doc.image(), &original, "{anchor:?}: undo restores");
            doc.redo();
            assert_eq!(doc.image(), &placed(&original, (15, 12), at, fill));
        }
    }
}

#[test]
fn shrinking_crops_per_the_anchor() {
    let original = image(10, 7, pattern);
    // 10×7 on 7×4: 3 fewer each way; centred, 1 off the left and top, 2 off the right
    // and bottom.
    let expected = [
        (Anchor::TopLeft, (0, 0)),
        (Anchor::Top, (-1, 0)),
        (Anchor::TopRight, (-3, 0)),
        (Anchor::Left, (0, -1)),
        (Anchor::Centre, (-1, -1)),
        (Anchor::Right, (-3, -1)),
        (Anchor::BottomLeft, (0, -3)),
        (Anchor::Bottom, (-1, -3)),
        (Anchor::BottomRight, (-3, -3)),
    ];
    for (anchor, at) in expected {
        assert_eq!(anchor.offset((10, 7), (7, 4)), at, "{anchor:?}");
        let mut doc = Document::new(original.clone());
        doc.transform(canvas(7, 4, anchor, Color::WHITE));
        let shrunk = placed(&original, (7, 4), at, Color::WHITE);
        assert_eq!(doc.image(), &shrunk, "{anchor:?}");
        // Every pixel is the original's: nothing new shows.
        let r = Rect::new((-at.0) as u32, (-at.1) as u32, 7, 4);
        assert_eq!(doc.image().data(), original.copy_rect(r));
        doc.undo();
        assert_eq!(doc.image(), &original, "{anchor:?}: undo restores");
    }
}

#[test]
fn wider_and_shorter_at_once_and_the_same_size_makes_no_step() {
    let original = image(10, 7, pattern);
    let mut doc = Document::new(original.clone());
    doc.transform(canvas(14, 5, Anchor::BottomRight, Color::TRANSPARENT));
    assert_eq!(
        doc.image(),
        &placed(&original, (14, 5), (4, -2), Color::TRANSPARENT)
    );
    doc.undo();
    assert_eq!(doc.image(), &original);
    for anchor in Anchor::ALL {
        assert_eq!(doc.transform(canvas(10, 7, anchor, Color::WHITE)), None);
    }
    assert_eq!(doc.position(), Some(0));
    assert!(canvas(0, 7, Anchor::Centre, Color::WHITE)
        .apply(&original)
        .is_err());
}
