//! ETCH-6: each fixture decodes to the pixels ImageMagick decodes from it (its `.rgba`
//! reference; `tests/data/make_fixtures.sh` made both), PNG with its alpha, JPEG and BMP
//! opaque; damaged and unsupported files give the reason.
//!
//! PNG and BMP match byte for byte. JPEG decoding isn't exact: the standard lets decoders
//! round their inverse DCTs, colour conversion, and chroma upsampling differently, and
//! `image`'s zune-jpeg and ImageMagick's libjpeg-turbo do. So a JPEG matches within a few
//! levels a channel (2 unsubsampled, 5 with 2×2 chroma, measured) and on average within
//! half a level; a wrong decode (another upsampling, a colour shift) is tens off.

use image_io::{decode, read, Format, OpenError};
use image_model::{Image, MAX_SIDE};
use std::path::{Path, PathBuf};

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

/// Reads fixture `name` and checks every pixel against its reference: each channel within
/// `tolerance` of it, and on average within half a level.
fn matches_reference(name: &str, size: (u32, u32), tolerance: u8) -> Image {
    let image = read(&data(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!((image.width(), image.height()), size, "{name}");
    let reference = std::fs::read(data(&format!("{name}.rgba"))).unwrap();
    assert_eq!(reference.len(), image.data().len(), "{name}");
    let (mut worst, mut total) = (0, 0u64);
    for (i, (got, want)) in image
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .zip(reference.as_chunks::<4>().0)
        .enumerate()
    {
        let (x, y) = (i as u32 % size.0, i as u32 / size.0);
        let off = got.iter().zip(want).map(|(g, w)| g.abs_diff(*w));
        total += off.clone().map(u64::from).sum::<u64>();
        let off = off.max().unwrap_or(0);
        worst = worst.max(off);
        assert!(
            off <= tolerance,
            "{name}: pixel ({x}, {y}) is {got:?}, reference {want:?}"
        );
    }
    let mean = total as f64 / reference.len() as f64;
    eprintln!("{name}: every channel within {worst} of the reference, {mean:.3} on average");
    assert!(mean <= 0.5, "{name}: {mean:.3} off on average");
    image
}

fn alphas(image: &Image) -> Vec<u8> {
    image
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| p[3])
        .collect()
}

#[test]
fn a_png_keeps_its_pixels_and_alpha() {
    let image = matches_reference("alpha.png", (16, 12), 0);
    let a = alphas(&image);
    assert!(a.contains(&0), "fully transparent pixels");
    assert!(a.contains(&255), "opaque pixels");
    assert!(a.iter().any(|&v| v > 0 && v < 255), "partial alpha");
    // Fully transparent pixels keep their colour too (straight alpha).
    let x0 = image.pixel(3, 11).unwrap();
    assert_eq!(x0.a, 0);
    assert_ne!((x0.r, x0.g, x0.b), (0, 0, 0));
}

#[test]
fn a_jpeg_opens_opaque_with_its_pixels() {
    for (name, tolerance) in [("photo.jpg", 2), ("photo420.jpg", 5)] {
        let image = matches_reference(name, (20, 16), tolerance);
        assert!(alphas(&image).iter().all(|&a| a == 255));
    }
}

#[test]
fn a_24_bit_bmp_opens_opaque_with_its_pixels() {
    let image = matches_reference("rgb24.bmp", (13, 7), 0);
    assert!(alphas(&image).iter().all(|&a| a == 255));
}

#[test]
fn a_32_bit_bmp_with_alpha_opens_opaque_with_its_colours() {
    let image = matches_reference("alpha32.bmp", (10, 8), 0);
    assert!(alphas(&image).iter().all(|&a| a == 255));
}

#[test]
fn a_truncated_file_is_damaged() {
    for (name, format) in [
        ("alpha.png", Format::Png),
        ("photo.jpg", Format::Jpeg),
        ("rgb24.bmp", Format::Bmp),
    ] {
        let bytes = std::fs::read(data(name)).unwrap();
        let cut = &bytes[..bytes.len() / 2];
        match decode(cut) {
            Err(e @ OpenError::Damaged { format: f, .. }) => {
                assert_eq!(f, format, "{name}");
                let reason = e.to_string();
                assert!(
                    reason.starts_with(&format!("It's a damaged or incomplete {}", f.name())),
                    "{name}: {reason}"
                );
            }
            other => panic!("{name} cut in half: {other:?}"),
        }
    }
}

#[test]
fn a_damaged_png_says_so() {
    let mut bytes = std::fs::read(data("alpha.png")).unwrap();
    // Spoil the compressed pixels: the bytes after the image data chunk's type.
    let idat = bytes.windows(4).position(|w| w == b"IDAT").unwrap() + 4;
    for b in &mut bytes[idat..idat + 16] {
        *b ^= 0x5a;
    }
    let e = decode(&bytes).unwrap_err();
    assert!(
        matches!(
            e,
            OpenError::Damaged {
                format: Format::Png,
                ..
            }
        ),
        "{e:?}"
    );
}

#[test]
fn a_gif_is_unsupported_by_its_content() {
    let e = read(&data("tiny.gif")).unwrap_err();
    assert!(matches!(e, OpenError::Unsupported(Some("GIF"))), "{e:?}");
    assert_eq!(
        e.to_string(),
        "It's a GIF image. Etch opens PNG, JPEG, and BMP images."
    );
}

#[test]
fn text_named_png_is_not_an_image() {
    let dir = std::env::temp_dir().join(format!("etch-image-io-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("notes.png");
    std::fs::write(&path, "shopping: eggs, milk\n").unwrap();
    let e = read(&path).unwrap_err();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(matches!(e, OpenError::Unsupported(None)), "{e:?}");
    assert_eq!(
        e.to_string(),
        "It isn't an image Etch can open: PNG, JPEG, or BMP."
    );
}

#[test]
fn a_gif_named_png_is_still_a_gif() {
    let bytes = std::fs::read(data("tiny.gif")).unwrap();
    assert!(matches!(
        decode(&bytes),
        Err(OpenError::Unsupported(Some("GIF")))
    ));
}

#[test]
fn an_empty_or_missing_file_says_why() {
    assert!(matches!(decode(&[]), Err(OpenError::Empty)));
    let e = read(&data("no-such-file.png")).unwrap_err();
    assert!(matches!(e, OpenError::Read(_)), "{e:?}");
    assert_eq!(e.to_string(), "The file doesn't exist.");
}

#[test]
fn an_image_over_max_side_is_refused_before_decoding() {
    // A valid PNG header claiming MAX_SIDE + 1 by 1 pixels, then an empty image data chunk.
    let chunk = |png: &mut Vec<u8>, kind: &[u8], data: &[u8]| {
        let body = [kind, data].concat();
        png.extend((data.len() as u32).to_be_bytes());
        png.extend(&body);
        png.extend(crc32(&body).to_be_bytes());
    };
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = (MAX_SIDE + 1).to_be_bytes().to_vec();
    ihdr.extend(1u32.to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]);
    chunk(&mut png, b"IHDR", &ihdr);
    chunk(&mut png, b"IDAT", &[]);
    let e = decode(&png).unwrap_err();
    assert!(
        matches!(e, OpenError::TooLarge { width, height: 1 } if width == MAX_SIDE + 1),
        "{e:?}"
    );
    assert_eq!(
        e.to_string(),
        "It's 16385×1 pixels; Etch opens images up to 16384 pixels a side."
    );
}

/// PNG's CRC-32 (ISO 3309), bit by bit.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}
