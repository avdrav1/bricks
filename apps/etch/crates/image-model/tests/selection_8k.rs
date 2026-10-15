//! ETCH-15: lifting, landing, and deleting a selection of a whole 8K image each take under
//! 200 ms (as ETCH-9's fill), so they run on the UI thread. Release only (`cargo test
//! --release -p image-model -- --ignored`).

use image_model::{Color, Document, Image, Rect};
use std::time::Instant;

const BUDGET_MS: f64 = 200.0;
const ALL: Rect = Rect::new(0, 0, 7680, 4320);

/// The worst of five runs of `run` on a fresh document each (made untimed), in ms.
fn worst(name: &str, run: impl Fn(&mut Document)) -> f64 {
    let base = Image::new(ALL.width, ALL.height, Color::rgb(237, 28, 36)).unwrap();
    let mut times: Vec<f64> = (0..5)
        .map(|_| {
            let mut doc = Document::new(base.clone());
            let t = Instant::now();
            run(&mut doc);
            t.elapsed().as_secs_f64() * 1e3
        })
        .collect();
    times.sort_by(f64::total_cmp);
    eprintln!(
        "{name}: {:.1} ms median, {:.1} ms worst (budget {BUDGET_MS} ms)",
        times[2], times[4]
    );
    times[4]
}

#[test]
#[ignore = "perf: run in release with --ignored"]
fn a_whole_8k_selection_moves_and_deletes_quickly() {
    let lift = worst("lift 7680×4320", |doc| {
        doc.lift(ALL).unwrap();
    });
    let land = worst("lift and land 7680×4320 moved 10 px", |doc| {
        let mut floating = doc.lift(ALL).unwrap();
        floating.move_by(10, 10);
        doc.land(floating).unwrap();
    });
    let delete = worst("delete 7680×4320", |doc| {
        doc.clear(ALL).unwrap();
    });
    assert!(lift < BUDGET_MS, "lift: {lift:.1} ms");
    assert!(land < BUDGET_MS, "land: {land:.1} ms");
    assert!(delete < BUDGET_MS, "delete: {delete:.1} ms");
}
