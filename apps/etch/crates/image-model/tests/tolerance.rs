//! ETCH-14: the fill's tolerance. A pixel fills when no channel, alpha included, differs
//! from the clicked pixel's by more than the tolerance; with a fully transparent pixel only
//! alpha counts.

use image_model::{Color, Document, Image};

const RED: Color = Color::rgb(237, 28, 36);
const SEED: [u8; 4] = [100, 100, 100, 200];

/// Two rows: the top all `SEED` (it joins everything), the bottom `variants`, one a column,
/// each touching the top row only.
fn image(seed: [u8; 4], variants: &[[u8; 4]]) -> Image {
    let mut data = Vec::new();
    for _ in variants {
        data.extend_from_slice(&seed);
    }
    for v in variants {
        data.extend_from_slice(v);
    }
    Image::from_rgba(variants.len() as u32, 2, data).unwrap()
}

/// `SEED` with channel `c` moved by `delta`.
fn off(c: usize, delta: i16) -> [u8; 4] {
    let mut p = SEED;
    p[c] = (p[c] as i16 + delta) as u8;
    p
}

/// Fills from the top-left at `tolerance`; which bottom pixels were filled.
fn filled(seed: [u8; 4], variants: &[[u8; 4]], tolerance: u8) -> Vec<bool> {
    let mut doc = Document::new(image(seed, variants));
    doc.fill((0.0, 0.0), RED, tolerance).unwrap();
    for x in 0..variants.len() as u32 {
        assert_eq!(doc.image().pixel(x, 0), Some(RED), "top row {x}");
    }
    (0..variants.len() as u32)
        .map(|x| doc.image().pixel(x, 1) == Some(RED))
        .collect()
}

#[test]
fn a_colour_at_the_tolerance_fills_and_one_level_beyond_does_not() {
    for tolerance in [1u8, 10, 54] {
        let t = tolerance as i16;
        for c in 0..4 {
            let variants = [off(c, t), off(c, -t), off(c, t + 1), off(c, -t - 1)];
            assert_eq!(
                filled(SEED, &variants, tolerance),
                [true, true, false, false],
                "tolerance {tolerance} channel {c}"
            );
        }
        // Every channel at the tolerance at once is still within it: the largest
        // difference counts, not their sum.
        let all = [
            100 + tolerance,
            100 - tolerance,
            100 + tolerance,
            200 - tolerance,
        ];
        assert_eq!(filled(SEED, &[all], tolerance), [true]);
    }
}

#[test]
fn tolerance_0_is_an_exact_match() {
    let variants: Vec<_> = (0..4).flat_map(|c| [off(c, 1), off(c, -1)]).collect();
    assert_eq!(filled(SEED, &variants, 0), vec![false; 8]);
    assert_eq!(filled(SEED, &[SEED], 0), [true]);
}

#[test]
fn tolerance_255_fills_everything_connected() {
    let variants = [
        [0, 0, 0, 0],
        [255, 255, 255, 255],
        [0, 255, 0, 1],
        [9, 9, 9, 99],
    ];
    assert_eq!(filled(SEED, &variants, 255), [true; 4]);
}

#[test]
fn with_a_fully_transparent_pixel_only_alpha_counts() {
    // Transparent green clicked: transparent pixels hiding any colour, and pixels of any
    // colour up to the tolerance's alpha, fill; one more alpha doesn't.
    let clear = [0, 255, 0, 0];
    let variants = [
        [255, 0, 255, 0],
        [255, 0, 0, 5],
        [0, 0, 0, 6],
        [0, 255, 0, 6],
    ];
    assert_eq!(filled(clear, &variants, 5), [true, true, false, false]);
    assert_eq!(filled(clear, &variants, 0), [true, false, false, false]);
    // The other way round: a nearly transparent pixel clicked takes transparent ones.
    let faint = [255, 0, 0, 5];
    let variants = [[0, 0, 255, 0], [255, 0, 0, 10], [255, 0, 0, 11]];
    assert_eq!(filled(faint, &variants, 5), [true, true, false]);
}

#[test]
fn a_tolerance_fill_undoes_and_redoes_exactly() {
    let variants: Vec<_> = (0..4).flat_map(|c| [off(c, 7), off(c, -9)]).collect();
    let mut doc = Document::new(image(SEED, &variants));
    let before = doc.image().clone();
    doc.fill((0.0, 0.0), RED, 8).unwrap();
    let after = doc.image().clone();
    doc.undo();
    assert_eq!(doc.image(), &before);
    doc.redo();
    assert_eq!(doc.image(), &after);
}
