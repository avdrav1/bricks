//! LibreOffice's reading of the test decks (`scripts/lo_pptx.py dump`), parsed.

#![allow(dead_code)]

use presentation_model::{PlaceholderKind, Shape, Slide};

use std::path::PathBuf;

pub fn data(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

#[derive(Debug, Default)]
pub struct Dump {
    pub size: (i64, i64),
    pub slides: Vec<DSlide>,
}

#[derive(Debug, Default)]
pub struct DSlide {
    pub hidden: bool,
    pub background: String,
    /// Every shape, groups included, in dump order.
    pub shapes: Vec<DShape>,
    pub notes: String,
}

impl DSlide {
    /// Shapes that aren't groups: what Frame's model holds once groups are flattened.
    pub fn leaves(&self) -> impl Iterator<Item = &DShape> {
        self.shapes.iter().filter(|s| s.kind != "GroupShape")
    }
}

#[derive(Debug, Default)]
pub struct DShape {
    pub depth: usize,
    pub kind: String,
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
    pub rotation: f64,
    pub flip_h: bool,
    pub flip_v: bool,
    pub name: String,
    pub fill: String,
    pub line: String,
    pub picture: String,
    pub paragraphs: Vec<DPara>,
    pub cells: Vec<DCell>,
}

#[derive(Debug, Default)]
pub struct DCell {
    pub row: usize,
    pub col: usize,
    pub fill: String,
    /// Top, bottom, left, right: `RRGGBB:WIDTH` or `-`.
    pub borders: Vec<String>,
    pub paragraphs: Vec<DPara>,
}

#[derive(Debug, Default)]
pub struct DPara {
    pub level: u32,
    pub align: String,
    pub text: String,
    pub runs: Vec<DRun>,
}

#[derive(Debug, Default)]
pub struct DRun {
    pub size: f64,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub color: String,
    pub font: String,
    pub text: String,
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(o) => out.push(o),
            None => {}
        }
    }
    out
}

pub fn dump(name: &str) -> Dump {
    let path = data(&format!("{name}-libreoffice.txt"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    let mut d = Dump::default();
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let slide = d.slides.last_mut();
        match f[0] {
            "size" => d.size = (f[1].parse().unwrap(), f[2].parse().unwrap()),
            "slide" => d.slides.push(DSlide {
                hidden: f[2] == "hidden=1",
                ..DSlide::default()
            }),
            "background" => slide.unwrap().background = f[1].to_owned(),
            "notes" => slide.unwrap().notes = unescape(f[1]),
            "shape" => slide.unwrap().shapes.push(DShape {
                depth: f[1].parse().unwrap(),
                kind: f[2].to_owned(),
                x: f[3].parse().unwrap(),
                y: f[4].parse().unwrap(),
                w: f[5].parse().unwrap(),
                h: f[6].parse().unwrap(),
                rotation: f[7].parse().unwrap(),
                flip_h: f[8] == "1",
                flip_v: f[9] == "1",
                name: unescape(f.get(10).copied().unwrap_or_default()),
                ..DShape::default()
            }),
            "fill" => slide.unwrap().shapes.last_mut().unwrap().fill = f[1].to_owned(),
            "line" => slide.unwrap().shapes.last_mut().unwrap().line = f[1].to_owned(),
            "picture" => {
                slide.unwrap().shapes.last_mut().unwrap().picture = format!("{} {}", f[1], f[2])
            }
            "cell" => slide.unwrap().shapes.last_mut().unwrap().cells.push(DCell {
                row: f[1].parse().unwrap(),
                col: f[2].parse().unwrap(),
                fill: f[3].to_owned(),
                borders: f[4].split(' ').map(str::to_owned).collect(),
                paragraphs: Vec::new(),
            }),
            "para" => {
                let shape = slide.unwrap().shapes.last_mut().unwrap();
                let para = DPara {
                    level: f[1].parse().unwrap(),
                    align: f[2].to_owned(),
                    text: unescape(f[3]),
                    runs: Vec::new(),
                };
                match shape.cells.last_mut() {
                    Some(cell) => cell.paragraphs.push(para),
                    None => shape.paragraphs.push(para),
                }
            }
            "run" => {
                let shape = slide.unwrap().shapes.last_mut().unwrap();
                let paras = match shape.cells.last_mut() {
                    Some(cell) => &mut cell.paragraphs,
                    None => &mut shape.paragraphs,
                };
                paras.last_mut().unwrap().runs.push(DRun {
                    size: f[1].parse().unwrap(),
                    bold: f[2] == "1",
                    italic: f[3] == "1",
                    underline: f[4] != "0",
                    strike: f[5] != "0",
                    color: f[6].to_owned(),
                    font: f[7].to_owned(),
                    text: unescape(f[8]),
                });
            }
            other => panic!("{path:?}: unknown line {other:?}"),
        }
    }
    d
}

/// The decks with LibreOffice dumps: the LibreOffice-built fixture and PowerPoint-authored
/// samples from Apache POI (tests/data/README.md).
pub const DECKS: [&str; 4] = ["fixture", "shapes", "layouts", "SampleShow"];

/// The slide's own shapes paired with LibreOffice's, in z-order, names equal.
///
/// Normalization: LibreOffice turns a slide's footer, slide-number, and (field) date
/// placeholders into its own header-and-footer settings instead of shapes, so those of
/// ours LibreOffice has no shape for are skipped. Every other shape must pair up.
pub fn pairs<'a>(slide: &'a Slide, lo: &'a DSlide, at: &str) -> Vec<(&'a Shape, &'a DShape)> {
    let theirs: Vec<&DShape> = lo.leaves().collect();
    let mut out = Vec::new();
    let mut k = 0;
    for shape in slide.own_shapes() {
        let footer = matches!(
            shape.placeholder,
            Some(PlaceholderKind::Footer | PlaceholderKind::SlideNumber | PlaceholderKind::Date)
        );
        match theirs.get(k) {
            Some(lo_shape) if lo_shape.name == shape.name => {
                out.push((shape, *lo_shape));
                k += 1;
            }
            _ if footer => {}
            other => panic!(
                "{at}: shape {:?} where LibreOffice has {:?}",
                shape.name,
                other.map(|s| &s.name)
            ),
        }
    }
    assert_eq!(
        k,
        theirs.len(),
        "{at}: LibreOffice has more shapes: {:?}",
        theirs[k..].iter().map(|s| &s.name).collect::<Vec<_>>()
    );
    out
}
