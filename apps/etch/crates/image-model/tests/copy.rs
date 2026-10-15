//! ETCH-7: a save copies the image on the UI thread so editing goes on while the copy is
//! written; at 8K that copy has to be quick. Release only (`cargo test --release -p
//! image-model -- --ignored`).

use image_model::{Color, Image};
use std::time::Instant;

/// The copy's budget: well inside the half second after which a click feels ignored, and
/// a few frames at most.
const BUDGET_MS: f64 = 60.0;

#[test]
#[ignore = "perf: run in release with --ignored"]
fn copying_an_8k_image_is_quick() {
    let image = Image::new(7680, 4320, Color::rgba(10, 20, 30, 200)).unwrap();
    let mut times: Vec<f64> = (0..10)
        .map(|_| {
            let t = Instant::now();
            let copy = image.clone();
            let ms = t.elapsed().as_secs_f64() * 1e3;
            assert_eq!(copy.data().len(), image.data().len());
            drop(copy);
            ms
        })
        .collect();
    times.sort_by(f64::total_cmp);
    let (median, worst) = (times[times.len() / 2], times[times.len() - 1]);
    eprintln!(
        "copying 7680×4320: {median:.1} ms median, {worst:.1} ms worst (budget {BUDGET_MS} ms)"
    );
    assert!(worst <= BUDGET_MS, "{worst:.1} ms");
}
