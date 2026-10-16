//! ETCH-7: saved images read back as they were: PNG exactly, alpha and the colour of fully
//! transparent pixels included; JPEG and BMP composited onto white (BMP exactly, JPEG
//! within its loss). The format follows the file name's extension. A save that fails
//! leaves the old file byte for byte.

use image_io::{encode, has_transparency, on_white, read, save, Format, JPEG_QUALITY};
use image_model::Image;
use std::path::{Path, PathBuf};

/// A fresh directory for one test.
fn dir(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("etch-save-{}-{test}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 96×64: smooth colour gradients (what JPEG keeps well) in four bands of alpha, top to
/// bottom: opaque, half (128), faint (40), and fully transparent with its colour kept.
fn sample() -> Image {
    let (w, h) = (96u32, 64u32);
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let alpha = [255, 128, 40, 0][(y / 16) as usize];
            data.extend_from_slice(&[(x * 255 / 95) as u8, (y * 255 / 63) as u8, 160, alpha]);
        }
    }
    Image::from_rgba(w, h, data).unwrap()
}

/// The files in `dir`, but `keep`.
fn others(dir: &Path, keep: &str) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n != keep)
        .collect()
}

#[test]
fn png_reads_back_with_the_same_pixels_and_alpha() {
    let dir = dir("png");
    let path = dir.join("a.png");
    let image = sample();
    save(&path, &image, Format::Png).unwrap();
    let (back, format) = read(&path).unwrap();
    assert_eq!(format, Format::Png);
    // Every byte: colour, alpha, and the colour under alpha 0.
    assert_eq!(back, image);
    assert_eq!(back.pixel(5, 60).unwrap().a, 0);
    assert_eq!(
        others(&dir, "a.png"),
        Vec::<String>::new(),
        "no temp file left"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn compositing_onto_white_mixes_by_alpha() {
    let image = Image::from_rgba(
        4,
        1,
        vec![
            255, 0, 0, 128, // red, half
            10, 20, 30, 255, // opaque: as it is
            0, 0, 0, 0, // transparent: white
            0, 0, 255, 51, // blue, a fifth
        ],
    )
    .unwrap();
    assert_eq!(
        on_white(&image),
        vec![255, 127, 127, 10, 20, 30, 255, 255, 255, 204, 204, 255]
    );
    assert!(has_transparency(&image));
    let opaque = Image::new(3, 3, image_model::Color::WHITE).unwrap();
    assert!(!has_transparency(&opaque));
}

#[test]
fn bmp_reads_back_composited_onto_white() {
    let dir = dir("bmp");
    let path = dir.join("a.bmp");
    let image = sample();
    save(&path, &image, Format::Bmp).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    // A 24-bit BMP: bits per pixel at offset 28.
    assert_eq!(&bytes[..2], b"BM");
    assert_eq!(u16::from_le_bytes([bytes[28], bytes[29]]), 24);
    let (back, format) = read(&path).unwrap();
    assert_eq!(format, Format::Bmp);
    let want: Vec<u8> = on_white(&image)
        .as_chunks::<3>()
        .0
        .iter()
        .flat_map(|&[r, g, b]| [r, g, b, 255])
        .collect();
    assert_eq!(back.data(), &want[..]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn jpeg_reads_back_composited_onto_white_within_its_loss() {
    let dir = dir("jpeg");
    let path = dir.join("a.jpg");
    let image = sample();
    save(&path, &image, Format::Jpeg).unwrap();
    let (back, format) = read(&path).unwrap();
    assert_eq!(format, Format::Jpeg);
    assert!(!has_transparency(&back));
    let want = on_white(&image);
    let (mut worst, mut total) = (0u8, 0u64);
    for (got, want) in back
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .zip(want.as_chunks::<3>().0)
    {
        for k in 0..3 {
            let off = got[k].abs_diff(want[k]);
            worst = worst.max(off);
            total += u64::from(off);
        }
    }
    let mean = total as f64 / want.len() as f64;
    eprintln!("JPEG at quality {JPEG_QUALITY}: every channel within {worst}, {mean:.2} on average");
    // JPEG's loss on smooth gradients at quality 90: a few levels; compositing wrongly
    // (onto black, or alpha ignored) is over a hundred off in the transparent bands.
    assert!(worst <= 16, "a channel {worst} off");
    assert!(mean <= 2.0, "{mean:.2} off on average");
    // The transparent band is white.
    let p = back.pixel(48, 60).unwrap();
    assert!(p.r >= 245 && p.g >= 245 && p.b >= 245, "{p:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_format_follows_the_extension() {
    for (name, format) in [
        ("a.png", Format::Png),
        ("a.PNG", Format::Png),
        ("a.jpg", Format::Jpeg),
        ("b.JPEG", Format::Jpeg),
        ("c.bmp", Format::Bmp),
    ] {
        assert_eq!(Format::for_path(Path::new(name)), Ok(format), "{name}");
    }
    for name in ["a.gif", "a.webp", "drawing", "a.png.txt"] {
        let e = Format::for_path(Path::new(name)).unwrap_err();
        assert_eq!(
            e.to_string(),
            "Etch saves PNG (.png), JPEG (.jpg or .jpeg), and BMP (.bmp) images: \
             end the name with one of them.",
            "{name}"
        );
    }
    // What each writes, by content.
    let image = sample();
    for format in [Format::Png, Format::Jpeg, Format::Bmp] {
        let bytes = encode(&image, format).unwrap();
        assert_eq!(image_io::format_of(&bytes).unwrap(), format);
    }
}

/// A save that can't replace the file fails and leaves it as it was, with no temp file:
/// on Unix the folder is read-only; on Windows, where anyone may add files to a read-only
/// folder, the file is (WIN-1).
#[test]
fn a_save_that_fails_leaves_the_old_file() {
    let dir = dir("unwritable");
    let path = dir.join("old.png");
    let old = encode(&sample(), Format::Png).unwrap();
    std::fs::write(&path, &old).unwrap();
    #[cfg(unix)]
    let protect = |on: bool| {
        use std::os::unix::fs::PermissionsExt;
        let mode = if on { 0o555 } else { 0o755 };
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(mode)).unwrap();
    };
    #[cfg(not(unix))]
    let protect = |on: bool| {
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_readonly(on);
        std::fs::set_permissions(&path, perms).unwrap();
    };
    protect(true);
    // Root writes into a read-only directory anyway; then there's nothing to test.
    let enforced = cfg!(not(unix)) || std::fs::write(dir.join("probe"), b"").is_err();
    let white = Image::new(96, 64, image_model::Color::WHITE).unwrap();
    let result = save(&path, &white, Format::Png);
    protect(false);
    if enforced {
        let e = result.unwrap_err();
        // "Permission denied" on Unix, "Access is denied" on Windows.
        assert!(e.to_string().contains("denied"), "{e}");
        assert_eq!(
            std::fs::read(&path).unwrap(),
            old,
            "the old file, byte for byte"
        );
        assert_eq!(
            others(&dir, "old.png"),
            Vec::<String>::new(),
            "no temp file left"
        );
    }
    std::fs::remove_dir_all(&dir).unwrap();
}
