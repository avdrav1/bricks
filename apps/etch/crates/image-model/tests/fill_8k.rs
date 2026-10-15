//! ETCH-9: filling an 8K image takes under 200 ms, history included, so the fill runs on
//! the UI thread. Release only (`cargo test --release -p image-model -- --ignored`).

use image_model::{Color, Document, Image, Pencil};
use std::time::Instant;

const BUDGET_MS: f64 = 200.0;
const W: u32 = 7680;
const H: u32 = 4320;

/// The worst of five fills on a fresh document each: `prepare` draws, then the fill at `at`
/// is timed.
fn worst(name: &str, prepare: impl Fn(&mut Document), at: (f64, f64)) -> f64 {
    let base = {
        let mut doc = Document::new(Image::new(W, H, Color::WHITE).unwrap());
        prepare(&mut doc);
        doc.image().clone()
    };
    let mut redo = Vec::new();
    let mut times: Vec<f64> = (0..5)
        .map(|_| {
            let mut doc = Document::new(base.clone());
            let t = Instant::now();
            let changed = doc.fill(at, Color::rgb(237, 28, 36), 0);
            let ms = t.elapsed().as_secs_f64() * 1e3;
            assert!(changed.is_some());
            // Redo replays the fill without saving tiles: the fill's own share.
            doc.undo();
            let t = Instant::now();
            doc.redo();
            redo.push(t.elapsed().as_secs_f64() * 1e3);
            ms
        })
        .collect();
    times.sort_by(f64::total_cmp);
    redo.sort_by(f64::total_cmp);
    let (median, worst) = (times[2], times[4]);
    eprintln!(
        "{name}: {median:.1} ms median, {worst:.1} ms worst (budget {BUDGET_MS} ms); redo {:.1} ms median",
        redo[2]
    );
    worst
}

#[test]
#[ignore = "perf: run in release with --ignored"]
fn filling_an_8k_image_is_quick() {
    // The whole blank image: every pixel and every tile.
    let blank = worst("blank 7680×4320", |_| {}, (10.0, 10.0));
    // A region bounded by drawn lines: a zigzag across the image and a frame, so the fill
    // winds through rows a span at a time on both sides of each line.
    let lines = |doc: &mut Document| {
        let pen = Pencil {
            color: Color::BLACK,
            width: 3,
        };
        let mut points = vec![(0.0, 100.0)];
        for i in 1..=24 {
            let y = if i % 2 == 0 { 100.0 } else { 4200.0 };
            points.push((i as f64 * 320.0, y));
        }
        doc.begin_stroke(pen, points[0]);
        for &p in &points[1..] {
            doc.extend_stroke(p);
        }
        doc.end_stroke();
        doc.begin_stroke(pen, (40.0, 40.0));
        for p in [
            (7640.0, 40.0),
            (7640.0, 4280.0),
            (40.0, 4280.0),
            (40.0, 40.0),
        ] {
            doc.extend_stroke(p);
        }
        doc.end_stroke();
    };
    let bounded = worst("bounded by lines", lines, (3000.0, 2000.0));
    assert!(blank < BUDGET_MS, "blank: {blank:.1} ms");
    assert!(bounded < BUDGET_MS, "bounded: {bounded:.1} ms");
}
