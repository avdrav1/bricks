//! FRAME-3 acceptance: pictures carry their bytes; fills, outlines, and backgrounds match
//! LibreOffice's reading; group children land where LibreOffice reads them (also in
//! `frames.rs`, over every deck); table cells carry text, fills, and borders; unsupported
//! content becomes a labelled shape.
//!
//! Normalizations: LibreOffice reports border widths of table cells on its own scale (a
//! 1 pt PowerPoint border reads as 6120 EMU), so borders compare by presence and colour;
//! outline widths compare within 360 EMU (LibreOffice's 1/100 mm).

mod common;

use common::{dump, DShape, DECKS};
use presentation_model::{
    Background, Fill, ImageFormat, Outline, Presentation, Shape, ShapeKind, Slide,
};
use std::io::Read;

fn read(name: &str) -> Presentation {
    pptx::read(&common::data(&format!("{name}.pptx"))).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// Every pair of the slide's shapes with LibreOffice's, for each deck.
fn each_pair(mut f: impl FnMut(&str, &Slide, &Shape, &DShape)) {
    for name in DECKS {
        let deck = read(name);
        let lo = dump(name);
        for (i, (slide, lo_slide)) in deck.slides.iter().zip(&lo.slides).enumerate() {
            let at = format!("{name} slide {}", i + 1);
            for (shape, lo_shape) in common::pairs(slide, lo_slide, &at) {
                f(
                    &format!("{at} shape {:?}", shape.name),
                    slide,
                    shape,
                    lo_shape,
                );
            }
        }
    }
}

/// `RRGGBB ALPHA` as LibreOffice prints a solid fill (ALPHA 0-100, 100 opaque).
fn solid(c: presentation_model::Color) -> String {
    format!(
        "{} {}",
        c.hex(),
        (f64::from(c.alpha) * 100.0 / 255.0).round()
    )
}

fn fill_matches(fill: &Fill, lo: &str) -> bool {
    match fill {
        Fill::None => lo == "none",
        Fill::Solid(c) => lo == format!("solid {}", solid(*c)),
        Fill::Picture(_) => lo == "bitmap",
        Fill::Unsupported { label, .. } => match label.as_str() {
            "Gradient" => lo == "gradient",
            "Pattern" => lo == "hatch",
            _ => lo == "bitmap",
        },
    }
}

fn outline_matches(outline: &Option<Outline>, lo: &str) -> bool {
    let f: Vec<&str> = lo.split(' ').collect();
    match (outline, f.as_slice()) {
        (None, ["none"]) => true,
        (Some(o), [_, color, width]) => {
            o.color.hex() == *color && (o.width - width.parse::<i64>().unwrap()).abs() <= 360
        }
        _ => false,
    }
}

#[test]
fn fills_and_outlines_match_libreoffice() {
    let mut checked = 0;
    each_pair(|at, _, shape, lo| {
        if !matches!(shape.kind, ShapeKind::Geometry { .. }) || lo.fill.is_empty() {
            return;
        }
        assert!(
            fill_matches(&shape.fill, &lo.fill),
            "{at}: fill {:?} vs LibreOffice {}",
            shape.fill,
            lo.fill
        );
        assert!(
            outline_matches(&shape.outline, &lo.line),
            "{at}: outline {:?} vs LibreOffice {}",
            shape.outline,
            lo.line
        );
        checked += 1;
    });
    eprintln!("{checked} shapes' fills and outlines match LibreOffice");
    assert!(checked > 100, "only {checked}");
}

#[test]
fn backgrounds_match_libreoffice() {
    let mut kinds = std::collections::BTreeSet::new();
    for name in DECKS {
        let deck = read(name);
        for (i, (slide, lo)) in deck.slides.iter().zip(&dump(name).slides).enumerate() {
            let ok = match (&slide.background, lo.background.as_str()) {
                (Background::Solid(c), "none") => c.hex() == "FFFFFF",
                (Background::Solid(c), lo) => lo == format!("solid {}", solid(*c)),
                (Background::Picture(_), lo) => lo == "bitmap",
                (Background::Other { label, .. }, lo) => match label.as_str() {
                    "Gradient" => lo == "gradient",
                    "Pattern" => lo == "hatch",
                    _ => lo == "bitmap",
                },
            };
            assert!(
                ok,
                "{name} slide {}: {:?} vs LibreOffice {}",
                i + 1,
                slide.background,
                lo.background
            );
            kinds.insert(lo.background.split(' ').next().unwrap().to_owned());
        }
    }
    assert!(
        ["solid", "gradient", "bitmap"]
            .iter()
            .all(|k| kinds.contains(*k)),
        "{kinds:?}"
    );
}

/// The pixel size in an image's header.
fn pixel_size(bytes: &[u8], format: ImageFormat) -> (u32, u32) {
    let be = |i: usize| u32::from_be_bytes(bytes[i..i + 4].try_into().unwrap());
    let le16 = |i: usize| u32::from(u16::from_le_bytes([bytes[i], bytes[i + 1]]));
    match format {
        ImageFormat::Png => (be(16), be(20)),
        ImageFormat::Gif => (le16(6), le16(8)),
        ImageFormat::Bmp => (
            u32::from_le_bytes(bytes[18..22].try_into().unwrap()),
            i32::from_le_bytes(bytes[22..26].try_into().unwrap()).unsigned_abs(),
        ),
        ImageFormat::Jpeg => {
            // The first start-of-frame marker holds the size.
            let mut i = 2;
            while i + 9 < bytes.len() {
                let marker = bytes[i + 1];
                let len = usize::from(u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]));
                if (0xC0..=0xCF).contains(&marker) && ![0xC4, 0xC8, 0xCC].contains(&marker) {
                    let h = u32::from(u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]));
                    let w = u32::from(u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]));
                    return (w, h);
                }
                i += 2 + len;
            }
            panic!("no JPEG frame header")
        }
        other => panic!("{other:?}"),
    }
}

fn media_parts(name: &str) -> Vec<Vec<u8>> {
    let file = std::fs::File::open(common::data(&format!("{name}.pptx"))).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let names: Vec<String> = zip
        .file_names()
        .filter(|n| n.starts_with("ppt/media/"))
        .map(str::to_owned)
        .collect();
    names
        .iter()
        .map(|n| {
            let mut out = Vec::new();
            zip.by_name(n).unwrap().read_to_end(&mut out).unwrap();
            out
        })
        .collect()
}

#[test]
fn pictures_carry_their_bytes() {
    let mut pictures = 0;
    for name in DECKS {
        let media = media_parts(name);
        let deck = read(name);
        let lo = dump(name);
        for (i, (slide, lo_slide)) in deck.slides.iter().zip(&lo.slides).enumerate() {
            let at = format!("{name} slide {}", i + 1);
            for (shape, lo_shape) in common::pairs(slide, lo_slide, &at) {
                let ShapeKind::Picture(p) = &shape.kind else {
                    continue;
                };
                let at = format!("{at} {:?}", shape.name);
                assert!(
                    media.iter().any(|m| m[..] == p.image.bytes[..]),
                    "{at}: the bytes aren't a media part's"
                );
                let (mime, size) = lo_shape.picture.split_once(' ').unwrap();
                let want = match p.image.format {
                    ImageFormat::Png => "image/png",
                    ImageFormat::Jpeg => "image/jpeg",
                    ImageFormat::Gif => "image/gif",
                    ImageFormat::Bmp => "image/bmp",
                    other => panic!("{at}: {other:?}"),
                };
                assert_eq!(mime, want, "{at}");
                let (w, h) = pixel_size(&p.image.bytes, p.image.format);
                assert_eq!(size, format!("{w}x{h}"), "{at}: pixel size");
                pictures += 1;
            }
        }
    }
    eprintln!("{pictures} pictures carry their bytes");
    assert!(pictures >= 4, "{pictures} pictures");
}

#[test]
fn table_cells_carry_text_fills_and_borders() {
    let mut cells = 0;
    each_pair(|at, _, shape, lo| {
        let ShapeKind::Table(table) = &shape.kind else {
            return;
        };
        assert_eq!(
            lo.cells.len(),
            table.rows.iter().map(|r| r.cells.len()).sum::<usize>(),
            "{at}: cells"
        );
        for lo_cell in &lo.cells {
            let at = format!("{at} cell {},{}", lo_cell.row, lo_cell.col);
            let cell = &table.rows[lo_cell.row].cells[lo_cell.col];
            common::compare_paragraphs(&at, &cell.text.paragraphs, &lo_cell.paragraphs);
            // A cell covered by a merge isn't drawn; LibreOffice gives it a fill of its
            // own default table look.
            if cell.merged {
                continue;
            }
            let fill = match cell.fill {
                Some(c) => format!("solid {}", solid(c)),
                None => "none".to_owned(),
            };
            assert_eq!(fill, lo_cell.fill, "{at}: fill");
            let b = &cell.borders;
            for (side, ours, theirs) in [
                ("top", b.top, &lo_cell.borders[0]),
                ("bottom", b.bottom, &lo_cell.borders[1]),
                ("left", b.left, &lo_cell.borders[2]),
                ("right", b.right, &lo_cell.borders[3]),
            ] {
                let ours = ours.map(|o| o.color.hex()).unwrap_or_else(|| "-".into());
                let theirs = theirs.split(':').next().unwrap();
                assert_eq!(ours, theirs, "{at}: {side} border");
            }
            cells += 1;
        }
    });
    eprintln!("{cells} table cells match LibreOffice");
    assert!(cells >= 20, "{cells} cells");
}

#[test]
fn unsupported_content_is_a_labelled_shape() {
    let deck = read("bar-chart");
    let lo = dump("bar-chart");
    let chart = common::pairs(&deck.slides[0], &lo.slides[0], "bar-chart")
        .into_iter()
        .find(|(_, lo)| lo.kind == "OLE2Shape")
        .unwrap()
        .0;
    assert!(
        matches!(&chart.kind, ShapeKind::Unsupported { label, preview: None } if label == "Chart")
    );
    assert!(chart.frame.cx > 0 && chart.frame.cy > 0);
}

/// smartart-simple.pptx: the SmartArt draws from the drawing PowerPoint saved with it,
/// one shape per part (four circles with their text), placed in the graphic frame.
#[test]
fn smart_art_draws_from_its_saved_drawing() {
    let deck = read("smartart-simple");
    let parts: Vec<&Shape> = deck.slides[0]
        .own_shapes()
        .filter(|s| s.name.starts_with('{'))
        .collect();
    assert_eq!(parts.len(), 4);
    assert!(parts
        .iter()
        .all(|s| matches!(&s.kind, ShapeKind::Geometry { preset, .. } if preset == "ellipse")));
    let texts: Vec<String> = parts.iter().map(|s| s.plain_text()).collect();
    assert_eq!(texts, ["abc", "def", "ghi", ""]);
}
