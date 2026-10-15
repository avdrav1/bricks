//! The Image menu's transforms on an 8K image (7680×4320). The app runs them off the UI
//! thread on a copy of the image, so what the UI thread pays is the copy (and undo and
//! redo, which swap the whole image in and out); the transform itself is timed to show how
//! long the canvas waits. Release only (`cargo test --release -p image-model -- --ignored
//! --nocapture` prints the numbers).

use image_model::{Color, Document, Image, Rect, Transform};
use std::time::Instant;

/// The UI thread's budget for what it does itself (as ETCH-9's fill).
const UI_MS: f64 = 200.0;

fn base() -> Image {
    Image::new(7680, 4320, Color::rgb(237, 28, 36)).unwrap()
}

/// The worst of five runs of `run`, in ms.
fn worst(name: &str, mut run: impl FnMut()) -> f64 {
    let mut times: Vec<f64> = (0..5)
        .map(|_| {
            let t = Instant::now();
            run();
            t.elapsed().as_secs_f64() * 1e3
        })
        .collect();
    times.sort_by(f64::total_cmp);
    eprintln!(
        "{name}: {:.1} ms median, {:.1} ms worst",
        times[2], times[4]
    );
    times[4]
}

/// Times `transform` on the 8K image off the UI thread, and the UI thread's share: the
/// copy before it, and undo and redo after.
fn time(name: &str, transform: Transform) {
    let image = base();
    let copy = worst(&format!("{name}: copying the image (UI thread)"), || {
        std::hint::black_box(image.clone());
    });
    assert!(copy < UI_MS, "copying took {copy:.1} ms");
    worst(
        &format!("{name}: the transform (off the UI thread)"),
        || {
            std::hint::black_box(transform.apply(&image).unwrap());
        },
    );
    let mut doc = Document::new(image.clone());
    let result = transform.apply(&image).unwrap();
    doc.install(transform, result);
    let swap = worst(&format!("{name}: undo and redo (UI thread)"), || {
        doc.undo();
        doc.redo();
    });
    assert!(swap < 1.0, "undo and redo took {swap:.1} ms");
}

#[test]
#[ignore = "perf: run in release with --ignored"]
fn cropping_8k() {
    time(
        "crop 7680×4320 to 7000×4000",
        Transform::Crop(Rect::new(300, 200, 7000, 4000)),
    );
}

#[test]
#[ignore = "perf: run in release with --ignored"]
fn resizing_8k() {
    use image_model::Resampling::{Nearest, Smooth};
    for (w, h, resampling) in [
        (3840, 2160, Smooth),
        (15360, 8640, Smooth),
        (3840, 2160, Nearest),
        (15360, 8640, Nearest),
    ] {
        time(
            &format!("resize 7680×4320 to {w}×{h} {resampling:?}"),
            Transform::Resize {
                width: w,
                height: h,
                resampling,
            },
        );
    }
}

#[test]
#[ignore = "perf: run in release with --ignored"]
fn resizing_the_canvas_8k() {
    use image_model::{Anchor, Color};
    for (w, h) in [(8000, 5000), (4000, 3000)] {
        time(
            &format!("canvas 7680×4320 to {w}×{h}"),
            Transform::ResizeCanvas {
                width: w,
                height: h,
                anchor: Anchor::Centre,
                fill: Color::TRANSPARENT,
            },
        );
    }
}
