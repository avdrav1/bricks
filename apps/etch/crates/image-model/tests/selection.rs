//! ETCH-15: the rectangular selection. Moving it lifts its pixels, leaves transparency,
//! lands them exactly (clipped at the edges) as one undo step; Delete clears to
//! transparency; a drag spans the pixels between its points.

use image_model::{spanning, Color, Document, Floating, Image, Rect, TILE};

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

/// What the image is after `rect` of `original` moved by `(dx, dy)`: the moved pixels
/// where they land (on the image), transparency where they left and nothing landed, the
/// rest as it was.
fn moved(original: &Image, rect: Rect, (dx, dy): (i64, i64)) -> Image {
    let (w, h) = (original.width(), original.height());
    image(w, h, |x, y| {
        let (sx, sy) = (x as i64 - dx, y as i64 - dy);
        if sx >= 0 && sy >= 0 && rect.contains(sx as u32, sy as u32) {
            original.pixel(sx as u32, sy as u32).unwrap()
        } else if rect.contains(x, y) {
            Color::TRANSPARENT
        } else {
            original.pixel(x, y).unwrap()
        }
    })
}

#[test]
fn lifting_leaves_the_image_and_landing_leaves_transparency_and_places_the_bytes_exactly() {
    let original = image(20, 16, pattern);
    let mut doc = Document::new(original.clone());
    let rect = Rect::new(3, 2, 7, 5);
    let mut floating = doc.lift(rect).unwrap();
    assert_eq!(floating.source(), Some(rect));
    assert_eq!((floating.at(), floating.size()), ((3, 2), (7, 5)));
    assert_eq!(floating.pixels().data(), original.copy_rect(rect));
    // Lifting changes nothing until the pixels land.
    assert_eq!(doc.image(), &original);
    assert_eq!(doc.position(), Some(0));
    // Moved twice (two drags) before landing: one step.
    floating.move_by(4, 1);
    floating.move_by(2, 3);
    assert_eq!(floating.at(), (9, 6));
    assert!(floating.contains((9, 6)) && floating.contains((15, 10)));
    assert!(!floating.contains((16, 10)) && !floating.contains((9, 5)));
    let changed = doc.land(floating);
    // The source and where they landed.
    assert_eq!(changed, Some(Rect::new(3, 2, 13, 9)));
    assert_eq!(doc.image(), &moved(&original, rect, (6, 4)));
    // Alpha 0 where they were and didn't land.
    for (x, y) in [(3, 2), (9, 2), (3, 6), (8, 5)] {
        assert_eq!(
            doc.image().pixel(x, y),
            Some(Color::TRANSPARENT),
            "({x}, {y})"
        );
    }
    assert_eq!(doc.position(), Some(1));
}

#[test]
fn the_whole_move_undoes_in_one_step_and_redoes_exactly() {
    // A 30×20 rectangle in tile (0, 0) moved into tile (2, 1): history keeps those two
    // tiles, not the image.
    let original = image(200, 150, pattern);
    let mut doc = Document::new(original.clone());
    let rect = Rect::new(10, 10, 30, 20);
    let mut floating = doc.lift(rect).unwrap();
    floating.move_to((140, 80));
    doc.land(floating).unwrap();
    let landed = doc.image().clone();
    assert_eq!(landed, moved(&original, rect, (130, 70)));
    assert_eq!(doc.history_bytes(), 2 * (TILE * TILE * 4) as usize);

    let changed = doc.undo().unwrap();
    assert_eq!(doc.image(), &original);
    assert!(changed.contains(10, 10) && changed.contains(169, 99));
    assert!(!doc.can_undo());
    assert_eq!(doc.position(), Some(0));
    doc.redo().unwrap();
    assert_eq!(doc.image(), &landed);
    doc.undo();
    assert_eq!(doc.image(), &original);
}

#[test]
fn moving_partly_off_the_image_clips() {
    let (w, h) = (20, 16);
    let original = image(w, h, pattern);
    let rect = Rect::new(4, 3, 8, 6);
    // Off each edge and corner, and wholly off: what's off the image is lost.
    for d in [
        (-7, 0),
        (0, -5),
        (12, 2),
        (-2, 11),
        (-10, -8),
        (14, 12),
        (30, 0),
        (-4, -40),
    ] {
        let mut doc = Document::new(original.clone());
        let mut floating = doc.lift(rect).unwrap();
        floating.move_by(d.0, d.1);
        assert!(doc.land(floating).is_some(), "{d:?}");
        assert_eq!(doc.image(), &moved(&original, rect, d), "{d:?}");
        doc.undo();
        assert_eq!(doc.image(), &original, "{d:?}");
    }
    // A rectangle partly off the image lifts the part on it.
    let doc = Document::new(original.clone());
    let floating = doc.lift(Rect::new(15, 12, 10, 10)).unwrap();
    assert_eq!(floating.source(), Some(Rect::new(15, 12, 5, 4)));
    assert_eq!(doc.lift(Rect::new(20, 0, 5, 5)), None);
}

#[test]
fn landing_where_they_were_lifted_from_makes_no_step() {
    let original = image(20, 16, pattern);
    let mut doc = Document::new(original.clone());
    let mut floating = doc.lift(Rect::new(2, 2, 5, 5)).unwrap();
    floating.move_by(3, 0);
    floating.move_by(-3, 0);
    assert_eq!(doc.land(floating), None);
    assert_eq!((doc.image(), doc.can_undo()), (&original, false));
}

#[test]
fn pasted_pixels_land_replacing_what_is_there() {
    // Pixels from elsewhere (ETCH-16's paste) clear nothing; they replace the pixels they
    // land on, transparent ones included.
    let original = image(20, 16, pattern);
    let mut doc = Document::new(original.clone());
    let pixels = image(4, 3, |x, _| match x {
        0 => Color::TRANSPARENT,
        1 => Color::rgba(0, 0, 255, 128),
        _ => Color::rgb(255, 0, 0),
    });
    let floating = Floating::new(pixels.clone(), (18, 14));
    assert_eq!(floating.source(), None);
    assert_eq!(doc.land(floating), Some(Rect::new(18, 14, 2, 2)));
    for y in 0..original.height() {
        for x in 0..original.width() {
            let want = match (x >= 18 && y >= 14).then(|| pixels.pixel(x - 18, y - 14)) {
                Some(p) => p.unwrap(),
                None => original.pixel(x, y).unwrap(),
            };
            assert_eq!(doc.image().pixel(x, y), Some(want), "({x}, {y})");
        }
    }
    doc.undo();
    assert_eq!(doc.image(), &original);
    // Wholly off the image: no step.
    assert_eq!(doc.land(Floating::new(pixels, (20, 0))), None);
    assert!(!doc.can_undo());
}

#[test]
fn delete_clears_to_transparency_and_undoes() {
    let original = image(100, 80, pattern);
    let mut doc = Document::new(original.clone());
    // Partly off the image: the part on it.
    let changed = doc.clear(Rect::new(60, 50, 50, 50));
    assert_eq!(changed, Some(Rect::new(60, 50, 40, 30)));
    let cleared = image(100, 80, |x, y| match x >= 60 && y >= 50 {
        true => Color::TRANSPARENT,
        false => pattern(x, y),
    });
    assert_eq!(doc.image(), &cleared);
    assert_eq!(doc.position(), Some(1));
    // Clearing what's transparent already, or off the image, makes no step.
    assert_eq!(doc.clear(Rect::new(70, 60, 10, 10)), None);
    assert_eq!(doc.clear(Rect::new(100, 0, 10, 10)), None);
    assert_eq!(doc.position(), Some(1));
    doc.undo();
    assert_eq!(doc.image(), &original);
    doc.redo();
    assert_eq!(doc.image(), &cleared);
}

#[test]
fn a_drag_spans_the_pixels_between_its_points() {
    let bounds = Rect::new(0, 0, 100, 80);
    // Edges at the nearest pixel edges, either way round.
    let want = Some(Rect::new(2, 1, 9, 3));
    assert_eq!(spanning((2.4, 3.6), (10.6, 1.2), bounds), want);
    assert_eq!(spanning((10.6, 1.2), (2.4, 3.6), bounds), want);
    assert_eq!(spanning((10.6, 3.6), (2.4, 1.2), bounds), want);
    // A drag across 200 pixels selects 200, wherever between pixels it starts.
    let wide = Rect::new(0, 0, 1000, 1000);
    assert_eq!(
        spanning((50.5, 50.5), (250.5, 170.5), wide),
        Some(Rect::new(51, 51, 200, 120))
    );
    assert_eq!(
        spanning((50.2, 50.2), (250.2, 170.2), wide),
        Some(Rect::new(50, 50, 200, 120))
    );
    // Clipped to the image.
    assert_eq!(
        spanning((-20.0, 70.0), (30.0, 500.0), bounds),
        Some(Rect::new(0, 70, 30, 10))
    );
    // A click (or a drag along a line) selects nothing, and nor does a drag off the image.
    assert_eq!(spanning((5.2, 5.2), (5.4, 30.0), bounds), None);
    assert_eq!(spanning((120.0, 5.0), (150.0, 30.0), bounds), None);
}
