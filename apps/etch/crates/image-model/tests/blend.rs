//! ETCH-12: pixels laid over the image (the text tool's rendered text; paste later),
//! source-over, clipped, one undo step.

use image_model::{Color, Document, Image, Rect, TILE};

/// An image whose pixel (x, y) is `f(x, y)`.
fn image(w: u32, h: u32, f: impl Fn(u32, u32) -> Color) -> Image {
    let data = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .flat_map(|(x, y)| f(x, y).to_bytes())
        .collect();
    Image::from_rgba(w, h, data).unwrap()
}

/// A background of every kind of pixel: opaque, see-through, and transparent ones hiding a
/// colour.
fn background(x: u32, y: u32) -> Color {
    match (x + 2 * y) % 4 {
        0 => Color::WHITE,
        1 => Color::rgba(255, 0, 0, 128),
        2 => Color::rgba(0, 255, 0, 0),
        _ => Color::rgb((x * 7 % 256) as u8, (y * 11 % 256) as u8, 90),
    }
}

/// Pixels to lay over it: opaque, see-through, and transparent.
fn pattern(x: u32, y: u32) -> Color {
    match (3 * x + y) % 5 {
        0 => Color::rgb(0, 0, 255),
        1 => Color::rgba(0, 0, 255, 128),
        2 => Color::TRANSPARENT,
        3 => Color::rgba((x * 30 % 256) as u8, 200, (y * 40 % 256) as u8, 7),
        _ => Color::rgba(10, 20, 30, 254),
    }
}

#[test]
fn pixels_blend_source_over_byte_for_byte() {
    let dst = [
        Color::WHITE,
        Color::WHITE,
        Color::rgba(0, 255, 0, 0),
        Color::rgba(255, 0, 0, 128),
        Color::rgba(12, 34, 56, 78),
    ];
    let src = [
        Color::rgb(0, 0, 255),
        Color::rgba(0, 0, 255, 128),
        Color::rgba(0, 0, 255, 128),
        Color::rgba(0, 0, 255, 128),
        Color::rgba(255, 255, 255, 0),
    ];
    let mut doc = Document::new(image(5, 1, |x, _| dst[x as usize]));
    let changed = doc.blend((0, 0), image(5, 1, |x, _| src[x as usize]));
    assert_eq!(changed, Some(Rect::new(0, 0, 5, 1)));
    let want = [
        // Opaque replaces.
        Color::rgb(0, 0, 255),
        // Half blue over white: 255 × (1 − 128/255) = 127 of white left.
        Color::rgb(127, 127, 255),
        // Over a transparent pixel the hidden colour plays no part.
        Color::rgba(0, 0, 255, 128),
        // Over half red: alpha 128/255 + 128/255 × 127/255 = 191.75/255; red 255 × 63.75 /
        // 191.75 = 84.8, blue 255 × 128 / 191.75 = 170.2.
        Color::rgba(85, 0, 170, 192),
        // A transparent pixel leaves the one under it as it was.
        Color::rgba(12, 34, 56, 78),
    ];
    for (x, w) in want.into_iter().enumerate() {
        assert_eq!(doc.image().pixel(x as u32, 0), Some(w), "pixel {x}");
    }
}

#[test]
fn pixels_off_the_image_are_clipped() {
    let (w, h) = (20, 16);
    let pixels = || image(7, 5, pattern);
    for at in [
        (-3, -2),
        (w as i64 - 4, h as i64 - 3),
        (6, -4),
        (-6, 13),
        (5, 5),
    ] {
        let mut doc = Document::new(image(w, h, background));
        let changed = doc.blend(at, pixels());
        // The same blend on an image 10 pixels larger each way, nothing clipped.
        let mut big = Document::new(image(w + 20, h + 20, |x, y| {
            background(x.wrapping_sub(10) % 1000, y.wrapping_sub(10) % 1000)
        }));
        big.blend((at.0 + 10, at.1 + 10), pixels());
        for y in 0..h {
            for x in 0..w {
                assert_eq!(
                    doc.image().pixel(x, y),
                    big.image().pixel(x + 10, y + 10),
                    "at {at:?}: {x},{y}"
                );
            }
        }
        let x0 = at.0.max(0) as u32;
        let y0 = at.1.max(0) as u32;
        let x1 = ((at.0 + 7) as u32).min(w);
        let y1 = ((at.1 + 5) as u32).min(h);
        assert_eq!(
            changed,
            Some(Rect::new(x0, y0, x1 - x0, y1 - y0)),
            "at {at:?}"
        );
    }
    for at in [(-7, 0), (20, 3), (0, -5), (4, 16)] {
        let mut doc = Document::new(image(w, h, background));
        assert_eq!(doc.blend(at, pixels()), None, "at {at:?}");
        assert!(!doc.can_undo(), "at {at:?}: no step");
        assert_eq!(doc.image(), &image(w, h, background));
    }
}

#[test]
fn a_blend_undoes_and_redoes_exactly_keeping_only_its_tiles() {
    let (w, h) = (200, 150);
    let original = image(w, h, background);
    let mut doc = Document::new(original.clone());
    let at = (100, 90);
    let changed = doc.blend(at, image(70, 50, pattern)).unwrap();
    assert_eq!(changed, Rect::new(100, 90, 70, 50));
    let blended = doc.image().clone();
    assert_ne!(blended, original);
    // Columns 64..192 and rows 64..150: four tiles, the lower two cut short by the image.
    let tiles = 2 * TILE * TILE + 2 * TILE * (h - 2 * TILE);
    assert_eq!(doc.history_bytes(), tiles as usize * 4);
    assert_eq!(doc.undo(), Some(Rect::new(TILE, TILE, 2 * TILE, h - TILE)));
    assert_eq!(doc.image(), &original);
    assert!(!doc.can_undo(), "one step");
    assert_eq!(doc.redo(), Some(changed));
    assert_eq!(doc.image(), &blended);
    doc.undo();
    assert_eq!(doc.image(), &original);
}
