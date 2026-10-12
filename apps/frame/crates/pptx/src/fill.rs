//! Fills, outlines, and backgrounds: `spPr` fills and `<a:ln>`, through placeholder
//! inheritance and the shape style's `fillRef`/`lnRef` into the theme's format scheme
//! (`fillStyleLst`, `lnStyleLst`, `bgFillStyleLst`), with `phClr` standing for the
//! reference's colour.

use crate::color::Palette;
use crate::package::{Package, Rels};
use crate::xml::Element;
use presentation_model::{
    Background, Color, Crop, Dash, Fill, Image, ImageFormat, Outline, Source,
};
use std::cell::RefCell;

/// What resolving fills on one part needs: colours, the theme's format scheme, and the
/// relationships pictures are named through (the part's own; the theme's for the
/// theme's fill styles).
pub(crate) struct Looks<'a> {
    pub palette: &'a Palette,
    pub fmt: Option<&'a Element>,
    pub rels: &'a Rels,
    pub theme_rels: Option<&'a Rels>,
    pub pkg: &'a RefCell<Package>,
}

/// One layer of an outline: what an `<a:ln>` sets.
#[derive(Default)]
struct LineLayer {
    width: Option<i64>,
    /// `Some(None)`: `noFill`.
    color: Option<Option<Color>>,
    dash: Option<Dash>,
}

impl Looks<'_> {
    /// The picture a `blip` (`<a:blip r:embed>`) names, with its format.
    pub fn image(&self, blip: &Element) -> Option<Image> {
        let rel = self.rels.get(blip.rel_attr("embed")?)?;
        if rel.external {
            return None;
        }
        let bytes = self.pkg.borrow_mut().media(&rel.target).ok()?;
        Some(Image {
            format: ImageFormat::of(&bytes),
            bytes,
        })
    }

    /// The fill a fill element (`solidFill`, `noFill`, `gradFill`, `blipFill`,
    /// `pattFill`, `grpFill`) gives; `None` for elements that aren't fills.
    pub fn fill(&self, e: &Element, ph: Option<Color>, group: &Fill) -> Option<Fill> {
        Some(match e.name.as_str() {
            "noFill" => Fill::None,
            "solidFill" => match self.palette.first(e, ph) {
                Some(c) => Fill::Solid(c),
                None => Fill::None,
            },
            "gradFill" => Fill::Unsupported {
                label: "Gradient".into(),
                approx: gradient_colour(self.palette, e, ph),
            },
            "pattFill" => Fill::Unsupported {
                label: "Pattern".into(),
                approx: e
                    .child("fgClr")
                    .and_then(|c| self.palette.first(c, ph))
                    .unwrap_or(Color::BLACK),
            },
            "blipFill" => match e.child("blip").and_then(|b| self.image(b)) {
                Some(img) if img.format.is_drawable() => Fill::Picture(img),
                Some(img) => Fill::Unsupported {
                    label: format!("Picture ({})", img.format.name()),
                    approx: Color::rgb(0xD9, 0xD9, 0xD9),
                },
                None => Fill::None,
            },
            "grpFill" => group.clone(),
            _ => return None,
        })
    }

    /// The first fill among `parent`'s children.
    pub fn fill_in(&self, parent: &Element, ph: Option<Color>, group: &Fill) -> Option<Fill> {
        parent.elements().find_map(|e| self.fill(e, ph, group))
    }

    /// The fill a style reference (`fillRef`, `bgRef`) names: `idx` 0 is none, 1-999
    /// the theme's `fillStyleLst`, 1001 and up its `bgFillStyleLst`.
    pub fn style_fill(&self, r: &Element) -> Option<Fill> {
        let idx: usize = r.num("idx")?;
        let ph = self.palette.first(r, None);
        let (list, i) = match idx {
            0 => return Some(Fill::None),
            1..=999 => ("fillStyleLst", idx - 1),
            _ => ("bgFillStyleLst", idx.checked_sub(1001)?),
        };
        let style = self.fmt?.child(list)?.elements().nth(i)?;
        // A theme fill's pictures are the theme part's.
        let theme = Looks {
            rels: self.theme_rels.unwrap_or(self.rels),
            ..*self
        };
        theme.fill(style, ph, &Fill::None)
    }

    /// A shape's fill and where it's set: its own `spPr`, then its placeholders' (nearest
    /// first; each `spPr` with where it's from), then its style's `fillRef`; none
    /// otherwise.
    pub fn shape_fill(
        &self,
        sp_prs: &[(&Element, Source)],
        style: Option<&Element>,
        group: &Fill,
    ) -> (Fill, Source) {
        sp_prs
            .iter()
            .find_map(|(s, from)| Some((self.fill_in(s, None, group)?, *from)))
            .or_else(|| {
                style
                    .and_then(|s| s.child("fillRef"))
                    .and_then(|r| self.style_fill(r))
                    .map(|f| (f, Source::Presentation))
            })
            .unwrap_or((Fill::None, Source::Presentation))
    }

    fn line_layer(&self, ln: &Element, ph: Option<Color>) -> LineLayer {
        let mut layer = LineLayer {
            width: ln.num("w"),
            ..LineLayer::default()
        };
        for e in ln.elements() {
            match e.name.as_str() {
                "noFill" => layer.color = Some(None),
                "solidFill" => layer.color = Some(self.palette.first(e, ph)),
                "gradFill" => layer.color = Some(Some(gradient_colour(self.palette, e, ph))),
                "pattFill" => {
                    layer.color = Some(e.child("fgClr").and_then(|c| self.palette.first(c, ph)))
                }
                "prstDash" => layer.dash = Some(dash(e.attr("val").unwrap_or("solid"))),
                _ => {}
            }
        }
        layer
    }

    /// A shape's outline from its style's `lnRef`, then its placeholders' `<a:ln>`
    /// (farthest first), then its own: each sets what it names. No colour anywhere, or
    /// `noFill`, is no outline. Where it's set is the nearest `<a:ln>`'s.
    pub fn outline(
        &self,
        sp_prs: &[(&Element, Source)],
        style: Option<&Element>,
    ) -> (Option<Outline>, Source) {
        let mut layers = Vec::new();
        if let Some(r) = style.and_then(|s| s.child("lnRef")) {
            let idx: usize = r.num("idx").unwrap_or(0);
            if idx > 0 {
                let ph = self.palette.first(r, None);
                if let Some(ln) = self
                    .fmt
                    .and_then(|f| f.child("lnStyleLst"))
                    .and_then(|l| l.elements().nth(idx - 1))
                {
                    layers.push(self.line_layer(ln, ph));
                }
            }
        }
        let mut from = Source::Presentation;
        for (sp_pr, source) in sp_prs.iter().rev() {
            if let Some(ln) = sp_pr.child("ln") {
                layers.push(self.line_layer(ln, None));
                from = *source;
            }
        }
        (merge(&layers), from)
    }

    /// A table border's `<a:ln>` (or a style's `lnRef`).
    pub fn border(&self, ln: &Element) -> Option<Outline> {
        merge(&[self.line_layer(ln, None)])
    }

    /// A background: `<p:bgPr>` (a fill) or `<p:bgRef>` (a theme background style).
    pub fn background(&self, bg: &Element) -> Option<Background> {
        let fill = if let Some(pr) = bg.child("bgPr") {
            self.fill_in(pr, None, &Fill::None)?
        } else {
            self.style_fill(bg.child("bgRef")?)?
        };
        // A picture background that tiles isn't stretched: show what it stands for.
        let tiled = bg.path(&["bgPr", "blipFill", "tile"]).is_some();
        Some(match fill {
            Fill::None => Background::Solid(Color::WHITE),
            Fill::Solid(c) => Background::Solid(c),
            Fill::Picture(img) if !tiled => Background::Picture(img),
            Fill::Picture(_) => Background::Other {
                label: "Tiled picture".into(),
                approx: Color::rgb(0xD9, 0xD9, 0xD9),
            },
            Fill::Unsupported { label, approx } => Background::Other { label, approx },
        })
    }
}

fn merge(layers: &[LineLayer]) -> Option<Outline> {
    let mut width = 0;
    let mut color = None;
    let mut dash = Dash::Solid;
    for l in layers {
        if let Some(w) = l.width {
            width = w;
        }
        if let Some(c) = l.color {
            color = Some(c);
        }
        if let Some(d) = l.dash {
            dash = d;
        }
    }
    Some(Outline {
        width,
        color: color.flatten()?,
        dash,
    })
}

fn dash(val: &str) -> Dash {
    match val {
        "dot" | "sysDot" => Dash::Dot,
        "dash" | "sysDash" => Dash::Dash,
        "dashDot" | "sysDashDot" | "lgDashDot" | "lgDashDotDot" | "sysDashDotDot" => Dash::DashDot,
        "lgDash" => Dash::LongDash,
        _ => Dash::Solid,
    }
}

/// A gradient's stand-in colour: its middle stop's (the average of the two around the
/// middle when the count is even).
fn gradient_colour(palette: &Palette, grad: &Element, ph: Option<Color>) -> Color {
    let stops: Vec<Color> = grad
        .child("gsLst")
        .into_iter()
        .flat_map(|l| l.all("gs"))
        .filter_map(|gs| palette.first(gs, ph))
        .collect();
    match stops.len() {
        0 => Color::rgb(0xD9, 0xD9, 0xD9),
        n if n % 2 == 1 => stops[n / 2],
        n => {
            let (a, b) = (stops[n / 2 - 1], stops[n / 2]);
            let mid = |x: u8, y: u8| ((u16::from(x) + u16::from(y)) / 2) as u8;
            Color {
                r: mid(a.r, b.r),
                g: mid(a.g, b.g),
                b: mid(a.b, b.b),
                alpha: mid(a.alpha, b.alpha),
            }
        }
    }
}

/// A `blipFill`'s `srcRect` crop.
pub(crate) fn crop(blip_fill: &Element) -> Crop {
    let Some(r) = blip_fill.child("srcRect") else {
        return Crop::default();
    };
    let f = |a| r.num::<f64>(a).unwrap_or(0.0) / 100_000.0;
    Crop {
        left: f("l"),
        top: f("t"),
        right: f("r"),
        bottom: f("b"),
    }
}
