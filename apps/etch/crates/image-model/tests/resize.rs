//! ETCH-18: Resize Image. Nearest neighbour doubles each pixel exactly; smooth matches
//! ImageMagick's Triangle filter within 1 per channel (references made by
//! `tests/data/resize/make_references.sh`), weighting colour by alpha so hidden colours
//! don't bleed; undo restores the size and every pixel.

use image_model::{Color, Document, Image, Resampling, SizeError, Transform, MAX_SIDE};

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

fn resize(width: u32, height: u32, resampling: Resampling) -> Transform {
    Transform::Resize {
        width,
        height,
        resampling,
    }
}

#[test]
fn nearest_doubles_each_pixel_exactly() {
    let original = image(13, 9, pattern);
    let doubled = resize(26, 18, Resampling::Nearest)
        .apply(&original)
        .unwrap();
    assert_eq!((doubled.width(), doubled.height()), (26, 18));
    for y in 0..18 {
        for x in 0..26 {
            assert_eq!(
                doubled.pixel(x, y),
                original.pixel(x / 2, y / 2),
                "pixel ({x}, {y}) is ({}, {})'s",
                x / 2,
                y / 2
            );
        }
    }
    // Three times, and back down to the original from the doubled image.
    let tripled = resize(39, 27, Resampling::Nearest)
        .apply(&original)
        .unwrap();
    for y in 0..27 {
        for x in 0..39 {
            assert_eq!(tripled.pixel(x, y), original.pixel(x / 3, y / 3));
        }
    }
    let halved = resize(13, 9, Resampling::Nearest).apply(&doubled).unwrap();
    assert_eq!(halved, original);
}

/// A raw RGBA8 fixture of `tests/data/resize`.
fn fixture(name: &str, width: u32, height: u32) -> Image {
    let path = format!("{}/tests/data/resize/{name}", env!("CARGO_MANIFEST_DIR"));
    Image::from_rgba(width, height, std::fs::read(path).unwrap()).unwrap()
}

/// The largest difference of any channel between `a` and `b` (of one size), with the
/// pixel where it is.
fn largest_difference(a: &Image, b: &Image) -> (u8, (u32, u32)) {
    assert_eq!((a.width(), a.height()), (b.width(), b.height()));
    let mut worst = (0, (0, 0));
    for y in 0..a.height() {
        for x in 0..a.width() {
            let (p, q) = (a.pixel(x, y).unwrap(), b.pixel(x, y).unwrap());
            let d = p
                .to_bytes()
                .iter()
                .zip(q.to_bytes())
                .map(|(&u, v)| u.abs_diff(v))
                .max()
                .unwrap();
            if d > worst.0 {
                worst = (d, (x, y));
            }
        }
    }
    worst
}

#[test]
fn smooth_matches_imagemagicks_triangle_filter_within_1_per_channel() {
    let source = fixture("source.rgba", 48, 36);
    for (name, w, h) in [
        ("down.rgba", 19, 13),
        ("up.rgba", 125, 97),
        ("wide.rgba", 100, 20),
    ] {
        let ours = resize(w, h, Resampling::Smooth).apply(&source).unwrap();
        let reference = fixture(name, w, h);
        let (d, at) = largest_difference(&ours, &reference);
        eprintln!("{name}: at most {d} levels from ImageMagick's");
        assert!(
            d <= 1,
            "{name}: {d} levels off at {at:?}: {:?}, ImageMagick's {:?}",
            ours.pixel(at.0, at.1),
            reference.pixel(at.0, at.1)
        );
    }
}

#[test]
fn smooth_never_bleeds_a_transparent_pixels_hidden_colour() {
    // Opaque red beside transparent green: shrunk to one pixel, red at half alpha, not
    // a red-green mix.
    let pair = image(2, 1, |x, _| match x {
        0 => Color::rgb(255, 0, 0),
        _ => Color::rgba(0, 255, 0, 0),
    });
    let one = resize(1, 1, Resampling::Smooth).apply(&pair).unwrap();
    assert_eq!(one.pixel(0, 0), Some(Color::rgba(255, 0, 0, 128)));
    // Enlarged, the pixels between fade out still red.
    let wide = resize(8, 1, Resampling::Smooth).apply(&pair).unwrap();
    for x in 0..8 {
        let p = wide.pixel(x, 0).unwrap().to_bytes();
        assert!(
            p[3] == 0 || (p[0], p[1], p[2]) == (255, 0, 0),
            "pixel {x} is {p:?}"
        );
    }
    assert_eq!(wide.pixel(0, 0), Some(Color::rgb(255, 0, 0)));
    assert_eq!(wide.pixel(7, 0).unwrap().to_bytes()[3], 0);
}

#[test]
fn smooth_keeps_a_single_colour_exactly_and_resizing_to_the_same_size_changes_nothing() {
    let flat = Image::new(37, 23, Color::rgba(12, 200, 99, 77)).unwrap();
    for (w, h) in [(100, 9), (5, 60), (1, 1), (370, 230)] {
        let out = resize(w, h, Resampling::Smooth).apply(&flat).unwrap();
        assert!(
            (0..h).all(|y| (0..w).all(|x| out.pixel(x, y) == Some(Color::rgba(12, 200, 99, 77)))),
            "{w}×{h}"
        );
    }
    // The same size is no step (smooth would give transparent pixels' hidden colour as 0).
    let original = image(30, 20, pattern);
    assert_eq!(
        resize(30, 20, Resampling::Nearest)
            .apply(&original)
            .unwrap(),
        original
    );
    for resampling in [Resampling::Nearest, Resampling::Smooth] {
        let mut doc = Document::new(original.clone());
        assert_eq!(doc.transform(resize(30, 20, resampling)), None);
        assert_eq!(doc.position(), Some(0));
    }
}

#[test]
fn resizing_is_one_step_and_undo_restores_the_size_and_every_pixel() {
    let original = image(64, 40, pattern);
    for resampling in [Resampling::Nearest, Resampling::Smooth] {
        let mut doc = Document::new(original.clone());
        let resized = resize(150, 17, resampling).apply(&original).unwrap();
        assert_eq!(
            doc.transform(resize(150, 17, resampling)),
            Some(image_model::Rect::new(0, 0, 150, 17))
        );
        assert_eq!(doc.image(), &resized);
        assert_eq!(doc.position(), Some(1));
        doc.undo();
        assert_eq!(
            doc.image(),
            &original,
            "{resampling:?}: undo restores every byte"
        );
        doc.redo();
        assert_eq!(
            doc.image(),
            &resized,
            "{resampling:?}: redo gives the same bytes"
        );
    }
}

#[test]
fn a_size_past_the_limits_is_refused() {
    let original = image(4, 4, pattern);
    assert_eq!(
        resize(0, 4, Resampling::Smooth).apply(&original),
        Err(SizeError::Empty)
    );
    assert_eq!(
        resize(MAX_SIDE + 1, 4, Resampling::Nearest).apply(&original),
        Err(SizeError::TooLarge {
            width: MAX_SIDE + 1,
            height: 4
        })
    );
    let mut doc = Document::new(original.clone());
    assert_eq!(doc.transform(resize(0, 4, Resampling::Smooth)), None);
    assert_eq!(doc.image(), &original);
}
