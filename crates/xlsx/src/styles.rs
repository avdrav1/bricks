//! Reading `xl/styles.xml` and the theme's colour scheme into a [`StylePool`] (STYLE-1,
//! ADR 0009). Records keep the file's indices.

use crate::xml::{attr, flag, num, on};
use data_model::{
    Align, Baseline, Border, Color, Edge, FileCounts, Fill, Font, HAlign, LineStyle, Pattern,
    StylePool, Tint, Underline, VAlign, Xf, DEFAULT_THEME,
};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::collections::BTreeMap;
use std::io;

/// The pool of a styles part and a theme part (either may be missing: defaults then).
pub(crate) fn read_pool(styles: Option<&str>, theme: Option<&str>) -> io::Result<StylePool> {
    let theme = match theme {
        Some(xml) => read_theme(xml)?,
        None => DEFAULT_THEME,
    };
    let Some(xml) = styles else {
        return Ok(StylePool {
            theme,
            ..StylePool::default()
        });
    };
    let mut pool = read_styles(xml)?;
    pool.theme = theme;
    Ok(pool)
}

/// A font as the parser starts one: what the file doesn't say is off or unset.
fn blank_font() -> Font {
    Font {
        name: "Calibri".into(),
        size: 1100,
        color: Color::Auto,
        family: None,
        scheme: None,
        ..Font::default()
    }
}

fn read_styles(xml: &str) -> io::Result<StylePool> {
    let mut fonts = Vec::new();
    let mut fills = Vec::new();
    let mut borders = Vec::new();
    let mut xfs = Vec::new();
    let mut num_fmts = BTreeMap::new();
    let mut indexed = Vec::new();
    let mut stack: Vec<Vec<u8>> = Vec::new();
    let mut reader = Reader::from_str(xml);
    loop {
        let (e, empty) = match reader.read_event().map_err(io::Error::other)? {
            Event::Start(e) => (e, false),
            Event::Empty(e) => (e, true),
            Event::End(_) => {
                stack.pop();
                continue;
            }
            Event::Eof => break,
            _ => continue,
        };
        let name = e.local_name().as_ref().to_vec();
        let section = stack.get(1).map(Vec::as_slice);
        let depth = stack.len();
        match (section, depth, name.as_slice()) {
            (Some(b"numFmts"), 2, b"numFmt") => {
                if let (Some(id), Some(code)) =
                    (num::<u32>(&e, b"numFmtId"), attr(&e, b"formatCode"))
                {
                    num_fmts.insert(id, code.into());
                }
            }
            (Some(b"fonts"), 2, b"font") => fonts.push(blank_font()),
            (Some(b"fonts"), 3, child) => {
                if let Some(font) = fonts.last_mut() {
                    font_child(font, child, &e);
                }
            }
            (Some(b"fills"), 2, b"fill") => fills.push(Fill::default()),
            (Some(b"fills"), 3, b"patternFill") => {
                let pattern = match attr(&e, b"patternType").as_deref() {
                    None | Some("none") => Pattern::None,
                    Some("solid") => Pattern::Solid,
                    Some(other) => Pattern::Other(other.into()),
                };
                if let Some(f) = fills.last_mut() {
                    *f = Fill::Pattern {
                        pattern,
                        fg: Color::Auto,
                        bg: Color::Auto,
                    };
                }
            }
            (Some(b"fills"), 4, b"fgColor" | b"bgColor") => {
                if let Some(Fill::Pattern { fg, bg, .. }) = fills.last_mut() {
                    *if name == b"fgColor" { fg } else { bg } = color(&e);
                }
            }
            (Some(b"fills"), 3, b"gradientFill") => {
                if let Some(f) = fills.last_mut() {
                    *f = Fill::Gradient { first: Color::Auto };
                }
            }
            (Some(b"fills"), 5, b"color") => {
                // The first stop of a gradient stands for it.
                if let Some(Fill::Gradient { first }) = fills.last_mut() {
                    if *first == Color::Auto {
                        *first = color(&e);
                    }
                }
            }
            (Some(b"borders"), 2, b"border") => borders.push(Border {
                diagonal_up: flag(&e, b"diagonalUp").unwrap_or(false),
                diagonal_down: flag(&e, b"diagonalDown").unwrap_or(false),
                ..Border::default()
            }),
            (Some(b"borders"), 3, side) => {
                if let Some(edge) = borders.last_mut().and_then(|b| edge_of(b, side)) {
                    edge.style = attr(&e, b"style")
                        .and_then(|s| LineStyle::parse(&s))
                        .unwrap_or_default();
                }
            }
            (Some(b"borders"), 4, b"color") => {
                let side = stack[3].clone();
                if let Some(edge) = borders.last_mut().and_then(|b| edge_of(b, &side)) {
                    edge.color = color(&e);
                }
            }
            (Some(b"cellXfs"), 2, b"xf") => xfs.push(Xf {
                num_fmt: num(&e, b"numFmtId").unwrap_or(0),
                font: num(&e, b"fontId").unwrap_or(0),
                fill: num(&e, b"fillId").unwrap_or(0),
                border: num(&e, b"borderId").unwrap_or(0),
                xf_id: num(&e, b"xfId").unwrap_or(0),
                quote_prefix: flag(&e, b"quotePrefix").unwrap_or(false),
                locked: true,
                ..Xf::default()
            }),
            (Some(b"cellXfs"), 3, b"alignment") => {
                if let Some(xf) = xfs.last_mut() {
                    xf.align = alignment(&e);
                }
            }
            (Some(b"cellXfs"), 3, b"protection") => {
                if let Some(xf) = xfs.last_mut() {
                    xf.locked = flag(&e, b"locked").unwrap_or(true);
                    xf.hidden = flag(&e, b"hidden").unwrap_or(false);
                }
            }
            (Some(b"colors"), 3, b"rgbColor") if stack[2] == b"indexedColors" => {
                if let Some(Color::Rgb(rgb)) = attr(&e, b"rgb").map(|v| rgb_color(&v)) {
                    indexed.push(rgb);
                }
            }
            _ => {}
        }
        if !empty {
            stack.push(name);
        }
    }
    if fonts.is_empty() {
        fonts.push(Font::default());
    }
    if xfs.is_empty() {
        xfs.push(Xf {
            locked: true,
            ..Xf::default()
        });
    }
    let from_file = FileCounts {
        fonts: fonts.len() as u32,
        fills: fills.len() as u32,
        borders: borders.len() as u32,
        xfs: xfs.len() as u32,
    };
    Ok(StylePool {
        fonts,
        fills,
        borders,
        num_fmts,
        xfs,
        theme: DEFAULT_THEME,
        indexed: (!indexed.is_empty()).then_some(indexed),
        from_file,
    })
}

fn font_child(font: &mut Font, child: &[u8], e: &BytesStart<'_>) {
    let val = || attr(e, b"val");
    match child {
        b"b" => font.bold = on(e),
        b"i" => font.italic = on(e),
        b"strike" => font.strike = on(e),
        b"u" => {
            font.underline = match val().as_deref() {
                None | Some("single") => Underline::Single,
                Some("double") => Underline::Double,
                Some("singleAccounting") => Underline::SingleAccounting,
                Some("doubleAccounting") => Underline::DoubleAccounting,
                Some(_) => Underline::None,
            }
        }
        b"sz" => {
            if let Some(pt) = val().and_then(|v| v.trim().parse::<f64>().ok()) {
                font.size = (pt * 100.0).round().max(1.0) as u32;
            }
        }
        b"color" => font.color = color(e),
        b"name" | b"rFont" => {
            if let Some(v) = val() {
                font.name = v.into();
            }
        }
        b"family" => font.family = num(e, b"val"),
        b"charset" => font.charset = num(e, b"val"),
        b"scheme" => font.scheme = val().map(Into::into),
        b"vertAlign" => {
            font.baseline = match val().as_deref() {
                Some("superscript") => Some(Baseline::Superscript),
                Some("subscript") => Some(Baseline::Subscript),
                _ => None,
            }
        }
        _ => {}
    }
}

fn edge_of<'a>(b: &'a mut Border, side: &[u8]) -> Option<&'a mut Edge> {
    Some(match side {
        b"left" | b"start" => &mut b.left,
        b"right" | b"end" => &mut b.right,
        b"top" => &mut b.top,
        b"bottom" => &mut b.bottom,
        b"diagonal" => &mut b.diagonal,
        _ => return None,
    })
}

fn alignment(e: &BytesStart<'_>) -> Align {
    let horizontal = match attr(e, b"horizontal").as_deref() {
        Some("left") => HAlign::Left,
        Some("center") => HAlign::Center,
        Some("right") => HAlign::Right,
        Some("fill") => HAlign::Fill,
        Some("justify") => HAlign::Justify,
        Some("centerContinuous") => HAlign::CenterContinuous,
        Some("distributed") => HAlign::Distributed,
        _ => HAlign::General,
    };
    let vertical = match attr(e, b"vertical").as_deref() {
        Some("top") => VAlign::Top,
        Some("center") => VAlign::Center,
        Some("justify") => VAlign::Justify,
        Some("distributed") => VAlign::Distributed,
        _ => VAlign::Bottom,
    };
    Align {
        horizontal,
        vertical,
        wrap: flag(e, b"wrapText").unwrap_or(false),
        shrink: flag(e, b"shrinkToFit").unwrap_or(false),
        indent: num(e, b"indent").unwrap_or(0),
        rotation: num(e, b"textRotation").unwrap_or(0),
    }
}

/// A colour element (`<color rgb="FF112233"/>`, `theme="4" tint="0.4"`, `indexed="8"`,
/// `auto="1"`).
pub(crate) fn color(e: &BytesStart<'_>) -> Color {
    if flag(e, b"auto") == Some(true) {
        return Color::Auto;
    }
    if let Some(rgb) = attr(e, b"rgb") {
        return rgb_color(&rgb);
    }
    if let Some(index) = num::<u8>(e, b"theme") {
        return Color::Theme {
            index,
            tint: Tint::new(num(e, b"tint").unwrap_or(0.0)),
        };
    }
    if let Some(n) = num::<u8>(e, b"indexed") {
        return Color::Indexed(n);
    }
    Color::Auto
}

/// `AARRGGBB` or `RRGGBB` hex; alpha is ignored, as Excel does.
fn rgb_color(hex: &str) -> Color {
    let hex = hex.trim();
    let hex = hex.get(hex.len().saturating_sub(6)..).unwrap_or("");
    u32::from_str_radix(hex, 16).map_or(Color::Auto, Color::Rgb)
}

/// A theme part's colour scheme: dk1, lt1, dk2, lt2, accent1–6, hlink, folHlink.
fn read_theme(xml: &str) -> io::Result<[u32; 12]> {
    const SLOTS: [&[u8]; 12] = [
        b"dk1",
        b"lt1",
        b"dk2",
        b"lt2",
        b"accent1",
        b"accent2",
        b"accent3",
        b"accent4",
        b"accent5",
        b"accent6",
        b"hlink",
        b"folHlink",
    ];
    let mut theme = DEFAULT_THEME;
    let mut slot: Option<usize> = None;
    let mut in_scheme = false;
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event().map_err(io::Error::other)? {
            Event::Start(e) | Event::Empty(e) => {
                let name = e.local_name();
                let name = name.as_ref();
                if name == b"clrScheme" {
                    in_scheme = true;
                } else if in_scheme {
                    if let Some(i) = SLOTS.iter().position(|s| *s == name) {
                        slot = Some(i);
                    } else if let Some(i) = slot {
                        let value = match name {
                            b"srgbClr" => attr(&e, b"val"),
                            b"sysClr" => attr(&e, b"lastClr"),
                            _ => None,
                        };
                        if let Some(Color::Rgb(rgb)) = value.map(|v| rgb_color(&v)) {
                            theme[i] = rgb;
                        }
                    }
                }
            }
            Event::End(e) if e.local_name().as_ref() == b"clrScheme" => in_scheme = false,
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(theme)
}

#[cfg(test)]
mod tests {
    use super::*;

    const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<numFmts count="1"><numFmt numFmtId="164" formatCode="_(* #,##0_);_(* \(#,##0\);_(* &quot;-&quot;??_);_(@_)"/></numFmts>
<fonts count="2"><font><sz val="11"/><color theme="1"/><name val="Calibri"/><family val="2"/><scheme val="minor"/></font>
<font><b/><i val="0"/><u val="double"/><sz val="16"/><color rgb="FFC00000"/><name val="Arial"/><vertAlign val="superscript"/></font></fonts>
<fills count="3"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill>
<fill><patternFill patternType="solid"><fgColor theme="4" tint="0.39997558519241921"/><bgColor indexed="64"/></patternFill></fill></fills>
<borders count="2"><border><left/><right/><top/><bottom/><diagonal/></border>
<border diagonalUp="1"><left style="double"><color indexed="10"/></left><right style="hair"/><top/><bottom style="mediumDashDot"><color auto="1"/></bottom><diagonal style="thin"/></border></borders>
<cellStyleXfs count="1"><xf numFmtId="9" fontId="1" fillId="2" borderId="1"/></cellStyleXfs>
<cellXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/>
<xf numFmtId="164" fontId="1" fillId="2" borderId="1" xfId="0" applyFont="1" quotePrefix="1"><alignment horizontal="centerContinuous" vertical="top" wrapText="1" indent="2" textRotation="135"/><protection locked="0" hidden="1"/></xf></cellXfs>
<dxfs count="1"><dxf><font><b/></font><fill><patternFill><bgColor rgb="FFFF0000"/></patternFill></fill></dxf></dxfs>
<colors><indexedColors><rgbColor rgb="FF000001"/><rgbColor rgb="FF000002"/></indexedColors></colors>
</styleSheet>"#;

    #[test]
    fn styles_read_with_the_files_indices() {
        let pool = read_pool(Some(STYLES), None).unwrap();
        assert_eq!(
            pool.num_fmts[&164].as_ref(),
            r#"_(* #,##0_);_(* \(#,##0\);_(* "-"??_);_(@_)"#
        );
        assert_eq!(pool.fonts.len(), 2, "dxf fonts are not cell fonts");
        let f = &pool.fonts[1];
        assert!(f.bold && !f.italic);
        assert_eq!(
            (f.underline, f.size, &*f.name),
            (Underline::Double, 1600, "Arial")
        );
        assert_eq!(
            (f.color, f.baseline),
            (Color::Rgb(0xC00000), Some(Baseline::Superscript))
        );
        assert_eq!(pool.fonts[0].scheme.as_deref(), Some("minor"));
        assert_eq!(pool.fills.len(), 3);
        assert_eq!(
            pool.fills[2],
            Fill::Pattern {
                pattern: Pattern::Solid,
                fg: Color::Theme {
                    index: 4,
                    tint: Tint::new(0.399_975_585_192_419_2)
                },
                bg: Color::Indexed(64),
            }
        );
        let b = &pool.borders[1];
        assert!(b.diagonal_up && !b.diagonal_down);
        assert_eq!(
            b.left,
            Edge {
                style: LineStyle::Double,
                color: Color::Indexed(10)
            }
        );
        assert_eq!(b.right.style, LineStyle::Hair);
        assert_eq!(b.top.style, LineStyle::None);
        assert_eq!(
            b.bottom,
            Edge {
                style: LineStyle::MediumDashDot,
                color: Color::Auto
            }
        );
        assert_eq!(pool.xfs.len(), 2, "cellStyleXfs are not cell formats");
        let xf = &pool.xfs[1];
        assert_eq!((xf.num_fmt, xf.font, xf.fill, xf.border), (164, 1, 2, 1));
        assert!(xf.quote_prefix && !xf.locked && xf.hidden);
        assert_eq!(
            xf.align,
            Align {
                horizontal: HAlign::CenterContinuous,
                vertical: VAlign::Top,
                wrap: true,
                shrink: false,
                indent: 2,
                rotation: 135,
            }
        );
        assert!(pool.xfs[0].locked, "locked unless the file says otherwise");
        assert_eq!(pool.indexed.as_deref(), Some(&[1, 2][..]));
        assert_eq!(
            pool.from_file,
            FileCounts {
                fonts: 2,
                fills: 3,
                borders: 2,
                xfs: 2
            }
        );
    }

    #[test]
    fn theme_colours_come_from_the_scheme() {
        let theme = r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:themeElements>
<a:clrScheme name="x"><a:dk1><a:sysClr val="windowText" lastClr="111111"/></a:dk1><a:lt1><a:srgbClr val="FEFEFE"/></a:lt1>
<a:accent1><a:srgbClr val="18A303"/></a:accent1></a:clrScheme>
<a:fontScheme name="f"><a:majorFont><a:latin typeface="Calibri Light"/></a:majorFont></a:fontScheme></a:themeElements></a:theme>"#;
        let pool = read_pool(None, Some(theme)).unwrap();
        assert_eq!(pool.theme[0], 0x111111);
        assert_eq!(pool.theme[1], 0xFEFEFE);
        assert_eq!(pool.theme[4], 0x18A303);
        assert_eq!(
            pool.theme[5], DEFAULT_THEME[5],
            "missing slots keep Office's colours"
        );
    }
}
