//! ETCH-6 acceptance: an 8K PNG shows within 1 s.
//!
//! Makes a 7680×4320 RGB PNG of photo-like content (smooth gradients, soft shapes, and
//! sensor-like noise, so it compresses as a photo does rather than as a flat colour), then
//! times `etch --bench-open FILE` from the process's start to the end of the first frame
//! that drew the image, three runs, and asserts the median. Opens a real window, so it
//! needs a graphical session. Timing is asserted only in optimized builds:
//! `cargo test --release -p etch --test open_8k -- --ignored --nocapture`

use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder};
use std::path::{Path, PathBuf};
use std::process::Command;

const TARGET_MS: f64 = 1000.0;
const RUNS: usize = 3;
const SIZE: (u32, u32) = (7680, 4320);

/// The test image, made once into the target's scratch directory.
fn photo_png() -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("etch-photo-8k.png");
    if path.exists() {
        return path;
    }
    let (w, h) = SIZE;
    let mut rgb = Vec::with_capacity(w as usize * h as usize * 3);
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            // A sky to ground gradient, a sun, and rolling hills.
            let sun = 1.0 - (((fx - 0.7).powi(2) + (fy - 0.3).powi(2)).sqrt() * 6.0).min(1.0);
            let hill = fy > 0.6 + 0.08 * (fx * 9.0).sin() + 0.04 * (fx * 23.0).cos();
            let (mut r, mut g, mut b) = if hill {
                (60.0 + 80.0 * fy, 110.0 + 60.0 * (1.0 - fy), 40.0)
            } else {
                (90.0 + 120.0 * fy, 140.0 + 80.0 * fy, 230.0 - 40.0 * fy)
            };
            r += 120.0 * sun;
            g += 100.0 * sun;
            b -= 60.0 * sun;
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let noise = (seed % 9) as f32 - 4.0;
            for c in [r, g, b] {
                rgb.push((c + noise).clamp(0.0, 255.0) as u8);
            }
        }
    }
    let tmp = path.with_extension("png.part");
    let file = std::io::BufWriter::new(std::fs::File::create(&tmp).unwrap());
    PngEncoder::new_with_quality(file, CompressionType::Default, FilterType::Adaptive)
        .write_image(&rgb, w, h, ExtendedColorType::Rgb8)
        .unwrap();
    std::fs::rename(&tmp, &path).unwrap();
    path
}

fn field(json: &str, key: &str) -> f64 {
    let start = json
        .find(&format!("\"{key}\":"))
        .unwrap_or_else(|| panic!("no {key} in {json}"))
        + key.len()
        + 3;
    let rest = &json[start..];
    rest[..rest.find([',', '}']).unwrap()].parse().unwrap()
}

#[test]
#[ignore = "opens a window; run with --ignored"]
fn an_8k_png_shows_within_1s() {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_none() {
        panic!("needs a graphical session");
    }
    let path = photo_png();
    let mb = std::fs::metadata(&path).unwrap().len() as f64 / 1e6;
    let mut times = Vec::new();
    for _ in 0..RUNS {
        let out = Command::new(env!("CARGO_BIN_EXE_etch"))
            .arg("--bench-open")
            .arg(&path)
            .output()
            .expect("run etch");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success(),
            "etch failed: {stdout}{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let json = stdout.lines().last().expect("a report").to_owned();
        eprintln!("{json}");
        assert!(json.contains(r#""image":[7680,4320]"#), "{json}");
        times.push(field(&json, "first_frame_ms"));
    }
    times.sort_by(f64::total_cmp);
    let median = times[RUNS / 2];
    eprintln!(
        "8K PNG ({mb:.1} MB) shows in {median:.0} ms (median of {RUNS}; target {TARGET_MS:.0} ms)"
    );
    if cfg!(debug_assertions) {
        eprintln!("debug build: timing not asserted");
        return;
    }
    assert!(
        median < TARGET_MS,
        "8K PNG showed in {median:.0} ms, target {TARGET_MS:.0} ms"
    );
}
