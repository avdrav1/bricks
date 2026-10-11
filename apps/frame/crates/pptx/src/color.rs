//! DrawingML colours: `srgbClr`, `schemeClr` through the theme and the colour map,
//! `sysClr`, `prstClr`, `scrgbClr`, `hslClr`, and their transforms (`lumMod`, `lumOff`,
//! `tint`, `shade`, `alpha`, ...).
//!
//! The arithmetic follows LibreOffice's `oox::drawingml::Color` (oox/source/drawingml/
//! color.cxx), the reader the tests compare against: percentages in 1/1000 %, `tint` and
//! `shade` applied to gamma-expanded ("CRGB", gamma 2.3) components, `lum*`, `sat*`, and
//! `hue*` in HSL, integer truncation where LibreOffice truncates.

use crate::xml::Element;
use presentation_model::Color;
use std::collections::HashMap;

const MAX_PERCENT: i32 = 100_000;
const PER_DEGREE: i32 = 60_000;
const MAX_DEGREE: i32 = 360 * PER_DEGREE;
const DEC_GAMMA: f64 = 2.3;
const INC_GAMMA: f64 = 1.0 / DEC_GAMMA;

/// What `schemeClr` names mean on one slide: the theme's colour scheme and the colour map
/// (`bg1` → `lt1`, ...) of the master, as the layout and slide override it.
#[derive(Debug, Clone, Default)]
pub(crate) struct Palette {
    scheme: HashMap<String, Color>,
    map: HashMap<String, String>,
}

impl Palette {
    /// From a theme's `<a:clrScheme>` (or none) and a master's `<p:clrMap>`.
    pub fn new(scheme: Option<&Element>, clr_map: Option<&Element>) -> Palette {
        let mut palette = Palette::default();
        if let Some(scheme) = scheme {
            for slot in scheme.elements() {
                if let Some(c) = slot.elements().next().and_then(|c| palette.color(c, None)) {
                    palette.scheme.insert(slot.name.clone(), c);
                }
            }
        }
        // PowerPoint's default map, for masters without one.
        for (k, v) in [
            ("bg1", "lt1"),
            ("tx1", "dk1"),
            ("bg2", "lt2"),
            ("tx2", "dk2"),
        ] {
            palette.map.insert(k.to_owned(), v.to_owned());
        }
        if let Some(m) = clr_map {
            palette.remap(m);
        }
        palette
    }

    /// Apply an `<a:overrideClrMapping>` (or a `<p:clrMap>`).
    pub fn remap(&mut self, map: &Element) {
        for key in [
            "bg1", "tx1", "bg2", "tx2", "accent1", "accent2", "accent3", "accent4", "accent5",
            "accent6", "hlink", "folHlink",
        ] {
            if let Some(v) = map.attr(key) {
                self.map.insert(key.to_owned(), v.to_owned());
            }
        }
    }

    /// A scheme colour by name (`accent1`, `tx1`, `dk2`, ...), through the map.
    pub fn scheme(&self, name: &str) -> Option<Color> {
        let slot = self.map.get(name).map(String::as_str).unwrap_or(name);
        self.scheme.get(slot).copied()
    }

    /// The colour of the first colour element inside `fill` (a `solidFill`, `buClr`, a
    /// style reference, ...). `ph` stands for `phClr`, the colour a style reference passes.
    pub fn first(&self, fill: &Element, ph: Option<Color>) -> Option<Color> {
        fill.elements().find_map(|c| self.color(c, ph))
    }

    /// A colour element's colour; `None` for elements that aren't colours.
    pub fn color(&self, e: &Element, ph: Option<Color>) -> Option<Color> {
        let mut c = match e.name.as_str() {
            "srgbClr" => Lo::rgb(hex(e.attr("val")?)?),
            "schemeClr" => {
                let name = e.attr("val")?;
                let base = if name == "phClr" {
                    ph.unwrap_or(Color::BLACK)
                } else {
                    self.scheme(name).unwrap_or(Color::BLACK)
                };
                let mut lo = Lo::rgb(base);
                lo.alpha = i32::from(base.alpha) * MAX_PERCENT / 255;
                lo
            }
            "sysClr" => Lo::rgb(e.attr("lastClr").and_then(hex).or_else(
                || match e.attr("val")? {
                    "window" | "highlightText" | "btnHighlight" | "menu" => Some(Color::WHITE),
                    _ => Some(Color::BLACK),
                },
            )?),
            "prstClr" => Lo::rgb(preset(e.attr("val")?)),
            "scrgbClr" => Lo {
                mode: Mode::Crgb,
                c1: e.num("r").unwrap_or(0),
                c2: e.num("g").unwrap_or(0),
                c3: e.num("b").unwrap_or(0),
                alpha: MAX_PERCENT,
            },
            "hslClr" => Lo {
                mode: Mode::Hsl,
                c1: e.num("hue").unwrap_or(0),
                c2: e.num("sat").unwrap_or(0),
                c3: e.num("lum").unwrap_or(0),
                alpha: MAX_PERCENT,
            },
            _ => return None,
        };
        for t in e.elements() {
            c.transform(&t.name, t.num::<i32>("val").unwrap_or(0));
        }
        Some(c.finish())
    }
}

fn hex(s: &str) -> Option<Color> {
    let v = u32::from_str_radix(s.trim(), 16).ok()?;
    if s.trim().len() != 6 {
        return None;
    }
    Some(Color::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

/// LibreOffice's colour state: components in the current mode, alpha in 1/1000 %.
#[derive(Debug)]
struct Lo {
    mode: Mode,
    c1: i32,
    c2: i32,
    c3: i32,
    alpha: i32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    /// 0..255 per component.
    Rgb,
    /// Linear (gamma-expanded) components in 1/1000 %.
    Crgb,
    /// Hue in 1/60000 degree, saturation and luminance in 1/1000 %.
    Hsl,
}

fn limited(v: f64, lo: i32, hi: i32) -> i32 {
    v.clamp(f64::from(lo), f64::from(hi)) as i32
}

fn gamma(comp: i32, g: f64) -> i32 {
    ((f64::from(comp) / f64::from(MAX_PERCENT)).powf(g) * f64::from(MAX_PERCENT) + 0.5) as i32
}

impl Lo {
    fn rgb(c: Color) -> Lo {
        Lo {
            mode: Mode::Rgb,
            c1: i32::from(c.r),
            c2: i32::from(c.g),
            c3: i32::from(c.b),
            alpha: MAX_PERCENT,
        }
    }

    fn transform(&mut self, name: &str, v: i32) {
        match name {
            "alpha" => self.alpha = v.clamp(0, MAX_PERCENT),
            "alphaMod" => {
                self.alpha = limited(
                    f64::from(self.alpha) * f64::from(v) / f64::from(MAX_PERCENT),
                    0,
                    MAX_PERCENT,
                )
            }
            "alphaOff" => self.alpha = (self.alpha + v).clamp(0, MAX_PERCENT),
            "lumMod" => {
                self.go_hsl();
                self.c3 = limited(
                    f64::from(self.c3) * f64::from(v) / f64::from(MAX_PERCENT),
                    0,
                    MAX_PERCENT,
                );
            }
            "lumOff" => {
                self.go_hsl();
                self.c3 = (self.c3 + v).clamp(0, MAX_PERCENT);
            }
            "satMod" => {
                self.go_hsl();
                self.c2 = limited(
                    f64::from(self.c2) * f64::from(v) / f64::from(MAX_PERCENT),
                    0,
                    MAX_PERCENT,
                );
            }
            "satOff" => {
                self.go_hsl();
                self.c2 = (self.c2 + v).clamp(0, MAX_PERCENT);
            }
            "hueMod" => {
                self.go_hsl();
                self.c1 = (f64::from(self.c1) * f64::from(v) / f64::from(MAX_PERCENT)) as i32
                    % MAX_DEGREE;
            }
            "hueOff" => {
                self.go_hsl();
                self.c1 = (self.c1 + v).rem_euclid(MAX_DEGREE);
            }
            "tint" => {
                self.go_crgb();
                if (0..MAX_PERCENT).contains(&v) {
                    let f = f64::from(v) / f64::from(MAX_PERCENT);
                    for c in [&mut self.c1, &mut self.c2, &mut self.c3] {
                        *c = (f64::from(MAX_PERCENT) - f64::from(MAX_PERCENT - *c) * f) as i32;
                    }
                }
            }
            "shade" => {
                self.go_crgb();
                if (0..MAX_PERCENT).contains(&v) {
                    let f = f64::from(v) / f64::from(MAX_PERCENT);
                    for c in [&mut self.c1, &mut self.c2, &mut self.c3] {
                        *c = (f64::from(*c) * f) as i32;
                    }
                }
            }
            "gray" => {
                self.go_rgb();
                let g = (f64::from(self.c1) * 0.299
                    + f64::from(self.c2) * 0.587
                    + f64::from(self.c3) * 0.114
                    + 0.5) as i32;
                (self.c1, self.c2, self.c3) = (g, g, g);
            }
            "inv" => {
                self.go_rgb();
                (self.c1, self.c2, self.c3) = (255 - self.c1, 255 - self.c2, 255 - self.c3);
            }
            "comp" => {
                self.go_hsl();
                self.c1 = (self.c1 + 180 * PER_DEGREE) % MAX_DEGREE;
            }
            _ => {}
        }
    }

    fn go_crgb(&mut self) {
        match self.mode {
            Mode::Crgb => {}
            Mode::Hsl | Mode::Rgb => {
                self.go_rgb();
                self.mode = Mode::Crgb;
                for c in [&mut self.c1, &mut self.c2, &mut self.c3] {
                    *c = gamma(*c * MAX_PERCENT / 255, DEC_GAMMA);
                }
            }
        }
    }

    fn go_rgb(&mut self) {
        match self.mode {
            Mode::Rgb => {}
            Mode::Crgb => {
                self.mode = Mode::Rgb;
                for c in [&mut self.c1, &mut self.c2, &mut self.c3] {
                    *c = (gamma(*c, INC_GAMMA) * 255 / MAX_PERCENT).clamp(0, 255);
                }
            }
            Mode::Hsl => {
                self.mode = Mode::Rgb;
                let (mut r, mut g, mut b);
                if self.c2 == 0 || self.c3 == 0 || self.c3 == MAX_PERCENT {
                    let l = f64::from(self.c3) / f64::from(MAX_PERCENT);
                    (r, g, b) = (l, l, l);
                } else {
                    let hue = f64::from(self.c1) / f64::from(MAX_DEGREE) * 6.0;
                    (r, g, b) = (0.0, 0.0, 0.0);
                    if hue <= 1.0 {
                        (r, g) = (1.0, hue);
                    } else if hue <= 2.0 {
                        (r, g) = (2.0 - hue, 1.0);
                    } else if hue <= 3.0 {
                        (g, b) = (1.0, hue - 2.0);
                    } else if hue <= 4.0 {
                        (g, b) = (4.0 - hue, 1.0);
                    } else if hue <= 5.0 {
                        (r, b) = (hue - 4.0, 1.0);
                    } else {
                        (r, b) = (1.0, 6.0 - hue);
                    }
                    let sat = f64::from(self.c2) / f64::from(MAX_PERCENT);
                    for c in [&mut r, &mut g, &mut b] {
                        *c = (*c - 0.5) * sat + 0.5;
                    }
                    let lum = 2.0 * f64::from(self.c3) / f64::from(MAX_PERCENT) - 1.0;
                    if lum < 0.0 {
                        let shade = lum + 1.0;
                        for c in [&mut r, &mut g, &mut b] {
                            *c *= shade;
                        }
                    } else if lum > 0.0 {
                        let tint = 1.0 - lum;
                        for c in [&mut r, &mut g, &mut b] {
                            *c = 1.0 - (1.0 - *c) * tint;
                        }
                    }
                }
                self.c1 = (r * 255.0 + 0.5) as i32;
                self.c2 = (g * 255.0 + 0.5) as i32;
                self.c3 = (b * 255.0 + 0.5) as i32;
            }
        }
    }

    fn go_hsl(&mut self) {
        if self.mode == Mode::Hsl {
            return;
        }
        self.go_rgb();
        self.mode = Mode::Hsl;
        let r = f64::from(self.c1) / 255.0;
        let g = f64::from(self.c2) / 255.0;
        let b = f64::from(self.c3) / 255.0;
        let min = r.min(g).min(b);
        let max = r.max(g).max(b);
        let d = max - min;
        let pd = f64::from(PER_DEGREE);
        self.c1 = if d == 0.0 {
            0
        } else if max == r {
            (((g - b) / d * 60.0 + 360.0) * pd + 0.5) as i32 % MAX_DEGREE
        } else if max == g {
            (((b - r) / d * 60.0 + 120.0) * pd + 0.5) as i32
        } else {
            (((r - g) / d * 60.0 + 240.0) * pd + 0.5) as i32
        };
        self.c3 = ((min + max) / 2.0 * f64::from(MAX_PERCENT) + 0.5) as i32;
        self.c2 = if self.c3 == 0 || self.c3 == MAX_PERCENT {
            0
        } else if self.c3 <= 50 * 1000 {
            (d / (min + max) * f64::from(MAX_PERCENT) + 0.5) as i32
        } else {
            (d / (2.0 - max - min) * f64::from(MAX_PERCENT) + 0.5) as i32
        };
    }

    fn finish(mut self) -> Color {
        self.go_rgb();
        Color {
            r: self.c1.clamp(0, 255) as u8,
            g: self.c2.clamp(0, 255) as u8,
            b: self.c3.clamp(0, 255) as u8,
            alpha: ((f64::from(self.alpha) * 255.0 / f64::from(MAX_PERCENT)) + 0.5) as u8,
        }
    }
}

/// DrawingML's preset colour names (`ST_PresetColorVal`), the CSS colours.
fn preset(name: &str) -> Color {
    let v: u32 = match name {
        "aliceBlue" => 0xF0F8FF,
        "antiqueWhite" => 0xFAEBD7,
        "aqua" | "cyan" => 0x00FFFF,
        "aquamarine" => 0x7FFFD4,
        "azure" => 0xF0FFFF,
        "beige" => 0xF5F5DC,
        "bisque" => 0xFFE4C4,
        "black" => 0x000000,
        "blanchedAlmond" => 0xFFEBCD,
        "blue" => 0x0000FF,
        "blueViolet" => 0x8A2BE2,
        "brown" => 0xA52A2A,
        "burlyWood" => 0xDEB887,
        "cadetBlue" => 0x5F9EA0,
        "chartreuse" => 0x7FFF00,
        "chocolate" => 0xD2691E,
        "coral" => 0xFF7F50,
        "cornflowerBlue" => 0x6495ED,
        "cornsilk" => 0xFFF8DC,
        "crimson" => 0xDC143C,
        "darkBlue" | "dkBlue" => 0x00008B,
        "darkCyan" | "dkCyan" => 0x008B8B,
        "darkGoldenrod" | "dkGoldenrod" => 0xB8860B,
        "darkGray" | "darkGrey" | "dkGray" | "dkGrey" => 0xA9A9A9,
        "darkGreen" | "dkGreen" => 0x006400,
        "darkKhaki" | "dkKhaki" => 0xBDB76B,
        "darkMagenta" | "dkMagenta" => 0x8B008B,
        "darkOliveGreen" | "dkOliveGreen" => 0x556B2F,
        "darkOrange" | "dkOrange" => 0xFF8C00,
        "darkOrchid" | "dkOrchid" => 0x9932CC,
        "darkRed" | "dkRed" => 0x8B0000,
        "darkSalmon" | "dkSalmon" => 0xE9967A,
        "darkSeaGreen" | "dkSeaGreen" => 0x8FBC8F,
        "darkSlateBlue" | "dkSlateBlue" => 0x483D8B,
        "darkSlateGray" | "darkSlateGrey" | "dkSlateGray" | "dkSlateGrey" => 0x2F4F4F,
        "darkTurquoise" | "dkTurquoise" => 0x00CED1,
        "darkViolet" | "dkViolet" => 0x9400D3,
        "deepPink" => 0xFF1493,
        "deepSkyBlue" => 0x00BFFF,
        "dimGray" | "dimGrey" => 0x696969,
        "dodgerBlue" => 0x1E90FF,
        "firebrick" => 0xB22222,
        "floralWhite" => 0xFFFAF0,
        "forestGreen" => 0x228B22,
        "fuchsia" | "magenta" => 0xFF00FF,
        "gainsboro" => 0xDCDCDC,
        "ghostWhite" => 0xF8F8FF,
        "gold" => 0xFFD700,
        "goldenrod" => 0xDAA520,
        "gray" | "grey" => 0x808080,
        "green" => 0x008000,
        "greenYellow" => 0xADFF2F,
        "honeydew" => 0xF0FFF0,
        "hotPink" => 0xFF69B4,
        "indianRed" => 0xCD5C5C,
        "indigo" => 0x4B0082,
        "ivory" => 0xFFFFF0,
        "khaki" => 0xF0E68C,
        "lavender" => 0xE6E6FA,
        "lavenderBlush" => 0xFFF0F5,
        "lawnGreen" => 0x7CFC00,
        "lemonChiffon" => 0xFFFACD,
        "lightBlue" | "ltBlue" => 0xADD8E6,
        "lightCoral" | "ltCoral" => 0xF08080,
        "lightCyan" | "ltCyan" => 0xE0FFFF,
        "lightGoldenrodYellow" | "ltGoldenrodYellow" => 0xFAFAD2,
        "lightGray" | "lightGrey" | "ltGray" | "ltGrey" => 0xD3D3D3,
        "lightGreen" | "ltGreen" => 0x90EE90,
        "lightPink" | "ltPink" => 0xFFB6C1,
        "lightSalmon" | "ltSalmon" => 0xFFA07A,
        "lightSeaGreen" | "ltSeaGreen" => 0x20B2AA,
        "lightSkyBlue" | "ltSkyBlue" => 0x87CEFA,
        "lightSlateGray" | "lightSlateGrey" | "ltSlateGray" | "ltSlateGrey" => 0x778899,
        "lightSteelBlue" | "ltSteelBlue" => 0xB0C4DE,
        "lightYellow" | "ltYellow" => 0xFFFFE0,
        "lime" => 0x00FF00,
        "limeGreen" => 0x32CD32,
        "linen" => 0xFAF0E6,
        "maroon" => 0x800000,
        "medAquamarine" | "mediumAquamarine" => 0x66CDAA,
        "medBlue" | "mediumBlue" => 0x0000CD,
        "medOrchid" | "mediumOrchid" => 0xBA55D3,
        "medPurple" | "mediumPurple" => 0x9370DB,
        "medSeaGreen" | "mediumSeaGreen" => 0x3CB371,
        "medSlateBlue" | "mediumSlateBlue" => 0x7B68EE,
        "medSpringGreen" | "mediumSpringGreen" => 0x00FA9A,
        "medTurquoise" | "mediumTurquoise" => 0x48D1CC,
        "medVioletRed" | "mediumVioletRed" => 0xC71585,
        "midnightBlue" => 0x191970,
        "mintCream" => 0xF5FFFA,
        "mistyRose" => 0xFFE4E1,
        "moccasin" => 0xFFE4B5,
        "navajoWhite" => 0xFFDEAD,
        "navy" => 0x000080,
        "oldLace" => 0xFDF5E6,
        "olive" => 0x808000,
        "oliveDrab" => 0x6B8E23,
        "orange" => 0xFFA500,
        "orangeRed" => 0xFF4500,
        "orchid" => 0xDA70D6,
        "paleGoldenrod" => 0xEEE8AA,
        "paleGreen" => 0x98FB98,
        "paleTurquoise" => 0xAFEEEE,
        "paleVioletRed" => 0xDB7093,
        "papayaWhip" => 0xFFEFD5,
        "peachPuff" => 0xFFDAB9,
        "peru" => 0xCD853F,
        "pink" => 0xFFC0CB,
        "plum" => 0xDDA0DD,
        "powderBlue" => 0xB0E0E6,
        "purple" => 0x800080,
        "red" => 0xFF0000,
        "rosyBrown" => 0xBC8F8F,
        "royalBlue" => 0x4169E1,
        "saddleBrown" => 0x8B4513,
        "salmon" => 0xFA8072,
        "sandyBrown" => 0xF4A460,
        "seaGreen" => 0x2E8B57,
        "seaShell" => 0xFFF5EE,
        "sienna" => 0xA0522D,
        "silver" => 0xC0C0C0,
        "skyBlue" => 0x87CEEB,
        "slateBlue" => 0x6A5ACD,
        "slateGray" | "slateGrey" => 0x708090,
        "snow" => 0xFFFAFA,
        "springGreen" => 0x00FF7F,
        "steelBlue" => 0x4682B4,
        "tan" => 0xD2B48C,
        "teal" => 0x008080,
        "thistle" => 0xD8BFD8,
        "tomato" => 0xFF6347,
        "turquoise" => 0x40E0D0,
        "violet" => 0xEE82EE,
        "wheat" => 0xF5DEB3,
        "white" => 0xFFFFFF,
        "whiteSmoke" => 0xF5F5F5,
        "yellow" => 0xFFFF00,
        "yellowGreen" => 0x9ACD32,
        _ => 0x000000,
    };
    Color::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::parse;

    /// LibreOffice's default theme ("LibreOffice"), as Impress writes it.
    fn palette() -> Palette {
        let scheme = parse(
            br#"<a:clrScheme xmlns:a="a" name="x">
              <a:dk1><a:srgbClr val="000000"/></a:dk1><a:lt1><a:srgbClr val="FFFFFF"/></a:lt1>
              <a:dk2><a:srgbClr val="44546A"/></a:dk2><a:lt2><a:srgbClr val="E7E6E6"/></a:lt2>
              <a:accent1><a:srgbClr val="18A303"/></a:accent1><a:accent2><a:srgbClr val="0369A3"/></a:accent2>
              <a:accent3><a:srgbClr val="A33E03"/></a:accent3><a:accent4><a:srgbClr val="8E03A3"/></a:accent4>
              <a:accent5><a:srgbClr val="C99C00"/></a:accent5><a:accent6><a:srgbClr val="C9211E"/></a:accent6>
              <a:hlink><a:srgbClr val="0000EE"/></a:hlink><a:folHlink><a:srgbClr val="551A8B"/></a:folHlink>
            </a:clrScheme>"#,
        )
        .unwrap();
        let map =
            parse(br#"<p:clrMap xmlns:p="p" bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2"/>"#).unwrap();
        Palette::new(Some(&scheme), Some(&map))
    }

    fn el(xml: &str) -> Element {
        parse(xml.replacen(" ", " xmlns:a=\"a\" ", 1).as_bytes()).unwrap()
    }

    fn resolve(xml: &str) -> String {
        palette().color(&el(xml), None).unwrap().hex()
    }

    /// Expected values are LibreOffice's (the fixture's dump, apps/frame/crates/pptx/
    /// tests/data/fixture-libreoffice.txt).
    #[test]
    fn transforms_match_libreoffice() {
        assert_eq!(
            resolve(r#"<a:schemeClr val="accent2"><a:tint val="40000"/></a:schemeClr>"#),
            "CCD3E0"
        );
        assert_eq!(
            resolve(r#"<a:schemeClr val="accent1"><a:shade val="50000"/></a:schemeClr>"#),
            "117802"
        );
        assert_eq!(
            resolve(
                r#"<a:schemeClr val="accent6"><a:lumMod val="60000"/><a:lumOff val="40000"/></a:schemeClr>"#
            ),
            "E9706D"
        );
        assert_eq!(
            resolve(r#"<a:schemeClr val="accent5"><a:lumMod val="50000"/></a:schemeClr>"#),
            "654E00"
        );
        assert_eq!(
            resolve(
                r#"<a:schemeClr val="accent1"><a:lumMod val="75000"/><a:lumOff val="0"/></a:schemeClr>"#
            ),
            "127A02"
        );
    }

    #[test]
    fn alpha_is_kept() {
        let c = palette()
            .color(
                &el(r#"<a:schemeClr val="accent4"><a:alpha val="50000"/></a:schemeClr>"#),
                None,
            )
            .unwrap();
        assert_eq!((c.hex().as_str(), c.alpha), ("8E03A3", 128));
    }

    #[test]
    fn scheme_names_go_through_the_colour_map() {
        let mut p = palette();
        assert_eq!(p.scheme("tx1").unwrap().hex(), "000000");
        p.remap(&el(r#"<a:overrideClrMapping bg1="dk1" tx1="lt1"/>"#));
        assert_eq!(p.scheme("tx1").unwrap().hex(), "FFFFFF");
        assert_eq!(p.scheme("bg1").unwrap().hex(), "000000");
        assert_eq!(p.scheme("accent2").unwrap().hex(), "0369A3");
    }

    #[test]
    fn other_colour_kinds() {
        assert_eq!(
            resolve(r#"<a:sysClr val="windowText" lastClr="000000"/>"#),
            "000000"
        );
        assert_eq!(resolve(r#"<a:sysClr val="window"/>"#), "FFFFFF");
        assert_eq!(resolve(r#"<a:prstClr val="teal"/>"#), "008080");
        assert_eq!(resolve(r#"<a:scrgbClr r="100000" g="0" b="0"/>"#), "FF0000");
        assert_eq!(
            resolve(r#"<a:hslClr hue="14400000" sat="100000" lum="50000"/>"#),
            "0000FF"
        );
        assert_eq!(
            palette()
                .color(
                    &el(r#"<a:schemeClr val="phClr"/>"#),
                    Some(Color::rgb(1, 2, 3))
                )
                .unwrap()
                .hex(),
            "010203"
        );
    }
}
