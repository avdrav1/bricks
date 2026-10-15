//! ETCH-8: the brush, hard and soft.

use image_model::{Brush, Color, Document, Image, TILE};

const RED: Color = Color::rgb(237, 28, 36);

fn brush(color: Color, width: u32, soft: bool) -> Brush {
    Brush { color, width, soft }
}

fn stroke(doc: &mut Document, b: Brush, points: &[(f64, f64)]) {
    doc.begin_stroke(b, points[0]);
    for &p in &points[1..] {
        doc.extend_stroke(p);
    }
    doc.end_stroke();
}

/// The soft edge's coverage at `d` pixels from the centre of a disc of radius `r`: a
/// smoothstep from 255 at the centre to 0 at the rim.
fn soft(d: f64, r: f64) -> u8 {
    if d > r {
        return 0;
    }
    let t = d / r;
    (255.0 * (1.0 - t * t * (3.0 - 2.0 * t))).round() as u8
}

/// `src` at `cov` over `dst`, straight alpha, source-over.
fn over(dst: Color, src: Color, cov: u8) -> Color {
    let sa = src.a as f64 / 255.0 * cov as f64 / 255.0;
    let da = dst.a as f64 / 255.0 * (1.0 - sa);
    let oa = sa + da;
    let mix = |s: u8, d: u8| ((s as f64 * sa + d as f64 * da) / oa).round() as u8;
    Color::rgba(
        mix(src.r, dst.r),
        mix(src.g, dst.g),
        mix(src.b, dst.b),
        (oa * 255.0).round() as u8,
    )
}

#[test]
fn a_hard_brush_fills_its_disc() {
    for width in [1, 2, 8, 9, 20] {
        let mut doc = Document::new(Image::new(64, 64, Color::WHITE).unwrap());
        let changed = doc
            .begin_stroke(brush(RED, width, false), (30.4, 30.9))
            .unwrap();
        doc.end_stroke();
        // The disc's centre: the pixel's centre for odd widths, its corner for even ones.
        let c = if width % 2 == 0 { 31.0 } else { 30.5 };
        let r = width as f64 / 2.0;
        let mut inside = 0;
        for y in 0..64 {
            for x in 0..64 {
                let (dx, dy) = (x as f64 + 0.5 - c, y as f64 + 0.5 - c);
                let want = if dx * dx + dy * dy <= r * r {
                    inside += 1;
                    assert!(
                        changed.contains(x, y),
                        "width {width}: {x},{y} in {changed:?}"
                    );
                    RED
                } else {
                    Color::WHITE
                };
                assert_eq!(
                    doc.image().pixel(x, y),
                    Some(want),
                    "width {width} at {x},{y}"
                );
            }
        }
        assert!(inside >= 1);
    }
}

#[test]
fn a_hard_stroke_is_solid_along_its_path() {
    let mut doc = Document::new(Image::new(100, 40, Color::WHITE).unwrap());
    stroke(
        &mut doc,
        brush(RED, 5, false),
        &[(10.0, 20.0), (90.0, 20.0)],
    );
    for x in 10..=90 {
        for y in 18..=22 {
            assert_eq!(doc.image().pixel(x, y), Some(RED), "{x},{y}");
        }
        assert_eq!(doc.image().pixel(x, 17), Some(Color::WHITE));
        assert_eq!(doc.image().pixel(x, 23), Some(Color::WHITE));
    }
}

#[test]
fn a_soft_brush_falls_off_to_transparent_at_its_edge() {
    let mut doc = Document::new(Image::new(64, 64, Color::TRANSPARENT).unwrap());
    let b = brush(Color::BLACK, 21, true);
    doc.begin_stroke(b, (32.0, 32.0));
    doc.end_stroke();
    let r = 10.5;
    let mut last = 255;
    // Rightwards from the centre: alpha is the coverage, falling to 0 at the rim.
    for x in 32..64 {
        let d = (x - 32) as f64;
        let want = soft(d, r);
        let px = doc.image().pixel(x, 32).unwrap();
        assert_eq!(px.a, want, "alpha at {x}");
        assert!(px.a <= last);
        last = px.a;
        if want > 0 {
            assert_eq!((px.r, px.g, px.b), (0, 0, 0));
        } else {
            assert_eq!(px, Color::TRANSPARENT, "untouched at {x}");
        }
    }
    assert_eq!(doc.image().pixel(32, 32), Some(Color::BLACK));
    assert_eq!(doc.image().pixel(42, 32).unwrap().a, soft(10.0, r));
    assert!(soft(10.0, r) < 3, "nearly nothing just inside the rim");
    // Halfway, half covered.
    assert_eq!(soft(5.25, r), 128);
}

#[test]
fn a_soft_brush_blends_over_whats_there() {
    let under = Color::rgba(0, 0, 255, 128);
    let mut doc = Document::new(Image::new(64, 64, under).unwrap());
    doc.begin_stroke(brush(RED, 21, true), (32.0, 32.0));
    doc.end_stroke();
    for x in 32..44 {
        let d = (x - 32) as f64;
        let want = match soft(d, 10.5) {
            0 => under,
            c => over(under, RED, c),
        };
        assert_eq!(doc.image().pixel(x, 32), Some(want), "at {x}");
    }
    // Partly covered: red and the blue under it, more opaque than the blue alone.
    let mid = doc.image().pixel(38, 32).unwrap();
    assert!(
        mid.r > 0 && mid.b > 0 && mid.a > 128 && mid.a < 255,
        "{mid:?}"
    );
}

#[test]
fn a_slow_stroke_does_not_build_up_within_itself() {
    // Back and forth over the same pixels, as a slow drag reports them, covers no more
    // than one pass: each pixel keeps the most any part of the stroke gave it.
    let b = brush(Color::rgba(237, 28, 36, 128), 15, true);
    let mut once = Document::new(Image::new(80, 40, Color::WHITE).unwrap());
    stroke(&mut once, b, &[(10.0, 20.0), (70.0, 20.0)]);
    let mut slow = Document::new(Image::new(80, 40, Color::WHITE).unwrap());
    let mut points = vec![(10.0, 20.0)];
    for i in 0..30 {
        points.push((11.0 + 2.0 * i as f64, 20.0));
        points.push((10.0 + 2.0 * i as f64, 20.0));
    }
    points.push((70.0, 20.0));
    stroke(&mut slow, b, &points);
    assert_eq!(slow.image(), once.image());
    // A translucent hard stroke crossing itself stays one coat where it crosses.
    let h = brush(Color::rgba(0, 0, 0, 100), 5, false);
    let mut doc = Document::new(Image::new(60, 60, Color::WHITE).unwrap());
    stroke(
        &mut doc,
        h,
        &[(5.0, 30.0), (55.0, 30.0), (30.0, 5.0), (30.0, 55.0)],
    );
    let coat = over(Color::WHITE, h.color, 255);
    assert_eq!(doc.image().pixel(10, 30), Some(coat));
    assert_eq!(doc.image().pixel(30, 30), Some(coat));
    assert_eq!(doc.image().pixel(30, 50), Some(coat));
}

#[test]
fn a_second_stroke_blends_over_the_first() {
    let b = brush(Color::rgba(0, 0, 0, 100), 9, false);
    let mut doc = Document::new(Image::new(40, 40, Color::TRANSPARENT).unwrap());
    stroke(&mut doc, b, &[(20.0, 20.0)]);
    let one = doc.image().pixel(20, 20).unwrap();
    stroke(&mut doc, b, &[(20.0, 20.0)]);
    let two = doc.image().pixel(20, 20).unwrap();
    assert_eq!(one, Color::rgba(0, 0, 0, 100));
    assert_eq!(two, over(one, b.color, 255));
    assert!(two.a > one.a);
}

#[test]
fn each_brush_stroke_undoes_and_redoes_exactly() {
    // A patterned image, so blending depends on what's under each pixel.
    let mut data = Vec::new();
    for y in 0..200u32 {
        for x in 0..300u32 {
            data.extend_from_slice(&[
                (x % 256) as u8,
                (y % 256) as u8,
                ((x ^ y) % 256) as u8,
                (x + y) as u8,
            ]);
        }
    }
    let mut doc = Document::new(Image::from_rgba(300, 200, data).unwrap());
    let blank = doc.image().clone();
    stroke(
        &mut doc,
        brush(RED, 31, true),
        &[(5.0, 5.0), (250.0, 150.0), (60.0, 190.0), (61.0, 189.0)],
    );
    let one = doc.image().clone();
    stroke(
        &mut doc,
        brush(Color::rgba(0, 200, 0, 90), 12, false),
        &[(290.0, 10.0), (10.0, 120.0)],
    );
    let two = doc.image().clone();
    assert_ne!(one, blank);
    assert_ne!(two, one);
    doc.undo();
    assert_eq!(doc.image(), &one);
    doc.undo();
    assert_eq!(doc.image(), &blank);
    doc.redo();
    assert_eq!(
        doc.image(),
        &one,
        "redo replays the soft stroke byte for byte"
    );
    doc.redo();
    assert_eq!(doc.image(), &two, "and the hard one");
}

#[test]
fn a_brush_stroke_keeps_only_the_tiles_it_touched() {
    let mut doc = Document::new(Image::new(1024, 1024, Color::WHITE).unwrap());
    // A 20 px soft dab inside one tile.
    stroke(&mut doc, brush(RED, 20, true), &[(30.0, 30.0)]);
    assert_eq!(doc.history_bytes(), (TILE * TILE * 4) as usize);
}
