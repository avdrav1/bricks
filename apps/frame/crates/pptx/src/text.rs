//! Text bodies: paragraphs and runs with every property resolved through the list-style
//! chain PowerPoint uses, lowest priority first:
//!
//! 1. the presentation's `defaultTextStyle`;
//! 2. the master's `txStyles`: `titleStyle` for titles, `bodyStyle` for body, subtitle,
//!    and content placeholders, `otherStyle` for everything else;
//! 3. the shape style's `fontRef` (font and colour), for shapes that have one;
//! 4. the master placeholder's, then the layout placeholder's, then the shape's own
//!    `lstStyle`;
//! 5. the paragraph's `pPr` and the run's `rPr`.
//!
//! Each style holds `defPPr` (all levels) and `lvl1pPr`..`lvl9pPr`; each of those may carry
//! a `defRPr` with the level's run defaults. `bodyPr` (anchoring, insets, wrap, autofit)
//! inherits master placeholder → layout placeholder → shape.

use crate::color::Palette;
use crate::xml::Element;
use presentation_model::{
    Align, Anchor, Bullet, Caps, Color, Insets, Paragraph, Run, Spacing, TextBody, Vertical,
};

/// PowerPoint's text size when nothing sets one.
const DEFAULT_SIZE: f64 = 18.0;

/// The theme's fonts (`+mj-lt`, `+mn-lt`, `+mj-ea`, ...).
#[derive(Debug, Clone, Default)]
pub(crate) struct Fonts {
    pub major: String,
    pub minor: String,
    pub major_ea: String,
    pub minor_ea: String,
}

impl Fonts {
    /// From a theme's `<a:fontScheme>`.
    pub fn of(scheme: Option<&Element>) -> Fonts {
        let face = |kind: &str, script: &str| {
            scheme
                .and_then(|s| s.path(&[kind, script]))
                .and_then(|e| e.attr("typeface"))
                .unwrap_or_default()
                .to_owned()
        };
        Fonts {
            major: face("majorFont", "latin"),
            minor: face("minorFont", "latin"),
            major_ea: face("majorFont", "ea"),
            minor_ea: face("minorFont", "ea"),
        }
    }

    /// A typeface with theme references resolved.
    pub fn resolve(&self, face: &str) -> String {
        let pick = |s: &str, fallback: &str| {
            if s.is_empty() {
                fallback.to_owned()
            } else {
                s.to_owned()
            }
        };
        match face {
            "+mj-lt" | "+mj-cs" => self.major.clone(),
            "+mn-lt" | "+mn-cs" => self.minor.clone(),
            "+mj-ea" => pick(&self.major_ea, &self.major),
            "+mn-ea" => pick(&self.minor_ea, &self.minor),
            f => f.to_owned(),
        }
    }
}

/// What text resolution needs from the slide.
pub(crate) struct TextContext<'a> {
    pub palette: &'a Palette,
    pub fonts: &'a Fonts,
    /// 1-based, for slide-number fields.
    pub slide_number: usize,
}

/// The run look a shape style's `<a:fontRef>` sets.
#[derive(Debug, Clone, Default)]
pub(crate) struct FontRef {
    pub font: Option<String>,
    pub color: Option<Color>,
    /// Table styles' `tcTxStyle b`/`i`.
    pub bold: Option<bool>,
    pub italic: Option<bool>,
}

/// A shape's text styles: `base` (default text style and master text style), its style
/// reference, and `shape` (placeholder list styles and its own), lowest priority first.
pub(crate) struct Styles<'a> {
    pub base: Vec<&'a Element>,
    pub font_ref: FontRef,
    pub shape: Vec<&'a Element>,
    /// `bodyPr` elements, lowest priority first.
    pub body: Vec<&'a Element>,
}

#[derive(Debug, Clone, Default)]
struct RunProps {
    font: Option<String>,
    size: Option<f64>,
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    color: Option<Color>,
    baseline: Option<f64>,
    caps: Option<Caps>,
}

#[derive(Debug, Clone, PartialEq)]
enum BulletKind {
    Off,
    Char(String),
    Number { scheme: String, start: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum BulletSize {
    FollowText,
    Percent(f64),
    Points(f64),
}

#[derive(Debug, Clone, Default)]
struct ParaProps {
    align: Option<Align>,
    margin_left: Option<i64>,
    indent: Option<i64>,
    line_spacing: Option<Spacing>,
    space_before: Option<Spacing>,
    space_after: Option<Spacing>,
    bullet: Option<BulletKind>,
    /// `Some(None)`: the text's font (`buFontTx`).
    bullet_font: Option<Option<String>>,
    bullet_color: Option<Option<Color>>,
    bullet_size: Option<BulletSize>,
    run: RunProps,
}

fn on(v: &str) -> bool {
    matches!(v, "1" | "true")
}

impl RunProps {
    fn apply(&mut self, rpr: &Element, ctx: &TextContext<'_>) {
        if let Some(sz) = rpr.num::<f64>("sz") {
            self.size = Some(sz / 100.0);
        }
        if let Some(b) = rpr.attr("b") {
            self.bold = Some(on(b));
        }
        if let Some(i) = rpr.attr("i") {
            self.italic = Some(on(i));
        }
        if let Some(u) = rpr.attr("u") {
            self.underline = Some(u != "none");
        }
        if let Some(s) = rpr.attr("strike") {
            self.strike = Some(s != "noStrike");
        }
        if let Some(b) = rpr.num::<f64>("baseline") {
            self.baseline = Some(b / 1000.0);
        }
        if let Some(c) = rpr.attr("cap") {
            self.caps = Some(match c {
                "all" => Caps::All,
                "small" => Caps::Small,
                _ => Caps::None,
            });
        }
        for e in rpr.elements() {
            match e.name.as_str() {
                "solidFill" => {
                    if let Some(c) = ctx.palette.first(e, None) {
                        self.color = Some(c);
                    }
                }
                // Gradient text shows as its first stop's colour.
                "gradFill" => {
                    if let Some(c) = e
                        .path(&["gsLst", "gs"])
                        .and_then(|gs| ctx.palette.first(gs, None))
                    {
                        self.color = Some(c);
                    }
                }
                "noFill" => {
                    self.color = Some(Color {
                        alpha: 0,
                        ..Color::BLACK
                    })
                }
                "latin" => {
                    if let Some(face) = e.attr("typeface").filter(|f| !f.is_empty()) {
                        self.font = Some(ctx.fonts.resolve(face));
                    }
                }
                _ => {}
            }
        }
    }
}

fn spacing(e: &Element) -> Option<Spacing> {
    if let Some(p) = e.child("spcPct") {
        return Some(Spacing::Lines(p.num::<f64>("val")? / 100_000.0));
    }
    let p = e.child("spcPts")?;
    Some(Spacing::Points(p.num::<f64>("val")? / 100.0))
}

impl ParaProps {
    fn apply(&mut self, ppr: &Element, ctx: &TextContext<'_>) {
        if let Some(a) = ppr.attr("algn") {
            self.align = Some(match a {
                "ctr" => Align::Centre,
                "r" => Align::Right,
                "just" | "justLow" => Align::Justify,
                "dist" | "thaiDist" => Align::Distributed,
                _ => Align::Left,
            });
        }
        if let Some(m) = ppr.num("marL") {
            self.margin_left = Some(m);
        }
        if let Some(i) = ppr.num("indent") {
            self.indent = Some(i);
        }
        for e in ppr.elements() {
            match e.name.as_str() {
                "lnSpc" => self.line_spacing = spacing(e).or(self.line_spacing),
                "spcBef" => self.space_before = spacing(e).or(self.space_before),
                "spcAft" => self.space_after = spacing(e).or(self.space_after),
                "buNone" => self.bullet = Some(BulletKind::Off),
                "buChar" => {
                    self.bullet = Some(BulletKind::Char(
                        e.attr("char").unwrap_or("\u{2022}").to_owned(),
                    ))
                }
                "buAutoNum" => {
                    self.bullet = Some(BulletKind::Number {
                        scheme: e.attr("type").unwrap_or("arabicPeriod").to_owned(),
                        start: e.num("startAt").unwrap_or(1),
                    })
                }
                "buBlip" => self.bullet = Some(BulletKind::Char("\u{2022}".into())),
                "buFontTx" => self.bullet_font = Some(None),
                "buFont" => {
                    self.bullet_font = Some(e.attr("typeface").map(|f| ctx.fonts.resolve(f)))
                }
                "buClrTx" => self.bullet_color = Some(None),
                "buClr" => self.bullet_color = Some(ctx.palette.first(e, None)),
                "buSzTx" => self.bullet_size = Some(BulletSize::FollowText),
                "buSzPct" => {
                    if let Some(v) = e.num::<f64>("val") {
                        self.bullet_size = Some(BulletSize::Percent(v / 100_000.0));
                    }
                }
                "buSzPts" => {
                    if let Some(v) = e.num::<f64>("val") {
                        self.bullet_size = Some(BulletSize::Points(v / 100.0));
                    }
                }
                "defRPr" => self.run.apply(e, ctx),
                _ => {}
            }
        }
    }
}

/// `lvl1pPr`..`lvl9pPr` names.
const LEVELS: [&str; 9] = [
    "lvl1pPr", "lvl2pPr", "lvl3pPr", "lvl4pPr", "lvl5pPr", "lvl6pPr", "lvl7pPr", "lvl8pPr",
    "lvl9pPr",
];

impl Styles<'_> {
    /// The paragraph properties a level gets from the styles.
    fn level(&self, level: usize, ctx: &TextContext<'_>) -> ParaProps {
        let mut props = ParaProps::default();
        let apply = |style: &Element, props: &mut ParaProps| {
            if let Some(d) = style.child("defPPr") {
                props.apply(d, ctx);
            }
            if let Some(l) = style.child(LEVELS[level]) {
                props.apply(l, ctx);
            }
        };
        for s in &self.base {
            apply(s, &mut props);
        }
        if let Some(f) = &self.font_ref.font {
            props.run.font = Some(f.clone());
        }
        if let Some(c) = self.font_ref.color {
            props.run.color = Some(c);
        }
        if let Some(b) = self.font_ref.bold {
            props.run.bold = Some(b);
        }
        if let Some(i) = self.font_ref.italic {
            props.run.italic = Some(i);
        }
        for s in &self.shape {
            apply(s, &mut props);
        }
        props
    }
}

/// Read a `txBody` with its styles. `None` when there's no text body.
pub(crate) fn body(
    tx_body: Option<&Element>,
    styles: &Styles<'_>,
    ctx: &TextContext<'_>,
) -> Option<TextBody> {
    let tx_body = tx_body?;
    let mut text = TextBody {
        paragraphs: Vec::new(),
        anchor: Anchor::Top,
        anchor_centre: false,
        insets: Insets::default(),
        wrap: true,
        font_scale: 1.0,
        line_reduction: 0.0,
        vertical: Vertical::Horizontal,
    };
    let own_body = tx_body.child("bodyPr");
    for b in styles.body.iter().copied().chain(own_body) {
        body_props(&mut text, b);
    }
    // Autofit's scale is the slide's own measurement; inherited bodies don't carry one.
    if let Some(fit) = own_body.and_then(|b| b.child("normAutofit")) {
        text.font_scale = fit.num::<f64>("fontScale").unwrap_or(100_000.0) / 100_000.0;
        text.line_reduction = fit.num::<f64>("lnSpcReduction").unwrap_or(0.0) / 100_000.0;
    }
    let mut counters: [Option<(String, u32)>; 9] = Default::default();
    for p in tx_body.all("p") {
        let ppr = p.child("pPr");
        let level = ppr.and_then(|p| p.num::<usize>("lvl")).unwrap_or(0).min(8);
        let mut props = styles.level(level, ctx);
        if let Some(ppr) = ppr {
            props.apply(ppr, ctx);
        }
        let mut runs = Vec::new();
        let mut end = props.run.clone();
        for r in p.elements() {
            let mut rp = props.run.clone();
            let rpr = r.child("rPr");
            if let Some(rpr) = rpr {
                rp.apply(rpr, ctx);
            }
            let text = match r.name.as_str() {
                "r" => r.child("t").map(|t| t.text()).unwrap_or_default(),
                "br" => "\n".to_owned(),
                "fld" if r.attr("type") == Some("slidenum") => ctx.slide_number.to_string(),
                "fld" => r.child("t").map(|t| t.text()).unwrap_or_default(),
                "endParaRPr" => {
                    end.apply(r, ctx);
                    continue;
                }
                _ => continue,
            };
            // A hyperlink shows in the theme's hyperlink colour, underlined, unless the run
            // sets its own colour.
            let link = rpr.is_some_and(|r| r.child("hlinkClick").is_some());
            let own_color = rpr.is_some_and(|r| r.child("solidFill").is_some());
            if link && !own_color {
                rp.color = ctx.palette.scheme("hlink").or(rp.color);
                rp.underline = Some(true);
            }
            runs.push(run(text, &rp, ctx));
        }
        let has_text = runs.iter().any(|r| !r.text.trim().is_empty());
        let bullet = bullet(&props, &runs, level, has_text, &mut counters);
        text.paragraphs.push(Paragraph {
            level: level as u8,
            align: props.align.unwrap_or_default(),
            margin_left: props.margin_left.unwrap_or(0),
            indent: props.indent.unwrap_or(0),
            line_spacing: props.line_spacing.unwrap_or(Spacing::Lines(1.0)),
            space_before: props.space_before.unwrap_or(Spacing::Points(0.0)),
            space_after: props.space_after.unwrap_or(Spacing::Points(0.0)),
            bullet,
            runs,
            end_size: end.size.unwrap_or(DEFAULT_SIZE),
        });
    }
    Some(text)
}

fn body_props(text: &mut TextBody, b: &Element) {
    if let Some(a) = b.attr("anchor") {
        text.anchor = match a {
            "ctr" => Anchor::Middle,
            "b" => Anchor::Bottom,
            _ => Anchor::Top,
        };
    }
    if let Some(c) = b.flag("anchorCtr") {
        text.anchor_centre = c;
    }
    let insets = &mut text.insets;
    for (name, slot) in [
        ("lIns", &mut insets.left),
        ("tIns", &mut insets.top),
        ("rIns", &mut insets.right),
        ("bIns", &mut insets.bottom),
    ] {
        if let Some(v) = b.num(name) {
            *slot = v;
        }
    }
    if let Some(w) = b.attr("wrap") {
        text.wrap = w != "none";
    }
    if let Some(v) = b.attr("vert") {
        text.vertical = match v {
            "vert" | "eaVert" | "wordArtVertRtl" => Vertical::Down,
            "vert270" => Vertical::Up,
            _ => Vertical::Horizontal,
        };
    }
}

fn run(text: String, p: &RunProps, ctx: &TextContext<'_>) -> Run {
    Run {
        text,
        font: p.font.clone().unwrap_or_else(|| ctx.fonts.minor.clone()),
        size: p.size.unwrap_or(DEFAULT_SIZE),
        bold: p.bold.unwrap_or(false),
        italic: p.italic.unwrap_or(false),
        underline: p.underline.unwrap_or(false),
        strike: p.strike.unwrap_or(false),
        color: p.color.unwrap_or(Color::BLACK),
        baseline: p.baseline.unwrap_or(0.0),
        caps: p.caps.unwrap_or_default(),
    }
}

fn bullet(
    props: &ParaProps,
    runs: &[Run],
    level: usize,
    has_text: bool,
    counters: &mut [Option<(String, u32)>; 9],
) -> Option<Bullet> {
    // Empty paragraphs show no bullet and don't break numbering.
    if !has_text {
        return None;
    }
    for c in counters.iter_mut().skip(level + 1) {
        *c = None;
    }
    let first = runs.iter().find(|r| !r.text.is_empty() && r.text != "\n")?;
    let text = match props.bullet.clone().unwrap_or(BulletKind::Off) {
        BulletKind::Off => {
            counters[level] = None;
            return None;
        }
        BulletKind::Char(c) => {
            counters[level] = None;
            symbol(&c, props.bullet_font.clone().flatten().as_deref())
        }
        BulletKind::Number { scheme, start } => {
            let n = match &counters[level] {
                Some((s, n)) if *s == scheme => *n,
                _ => start,
            };
            counters[level] = Some((scheme.clone(), n + 1));
            number(&scheme, n)
        }
    };
    let font = match props.bullet_font.clone().flatten() {
        // Symbol fonts were mapped to Unicode above: draw them in the text's font.
        Some(f) if !is_symbol_font(&f) => f,
        _ => first.font.clone(),
    };
    let size = match props.bullet_size.unwrap_or(BulletSize::FollowText) {
        BulletSize::FollowText => first.size,
        BulletSize::Percent(p) => first.size * p,
        BulletSize::Points(p) => p,
    };
    Some(Bullet {
        text,
        font,
        size,
        color: props.bullet_color.flatten().unwrap_or(first.color),
    })
}

fn is_symbol_font(f: &str) -> bool {
    let f = f.to_ascii_lowercase();
    f.starts_with("wingdings") || f == "symbol" || f == "webdings"
}

/// A bullet character as Unicode: Wingdings and Symbol put their glyphs on Latin-1 code
/// points, which other fonts show as letters.
fn symbol(c: &str, font: Option<&str>) -> String {
    let Some(font) = font.filter(|f| is_symbol_font(f)) else {
        return c.to_owned();
    };
    let ch = c.chars().next().unwrap_or('\u{2022}');
    let mapped = if font.eq_ignore_ascii_case("symbol") {
        // Symbol's glyphs sit on ASCII and Latin-1 (or the same in U+F020-F0FF): the few
        // that bullets use, the rest as the character itself.
        let ch = match u32::from(ch) {
            v @ 0xF020..=0xF0FF => char::from_u32(v - 0xF000).unwrap_or(ch),
            _ => ch,
        };
        match ch {
            '\u{b7}' => '\u{2022}',
            '\u{a8}' => '\u{2663}',
            '-' => '\u{2212}',
            _ => ch,
        }
    } else {
        match ch {
            'l' | '\u{f06c}' => '\u{25cf}',
            'n' | '\u{f06e}' => '\u{25a0}',
            'q' | '\u{f071}' => '\u{274f}',
            'v' | '\u{f076}' => '\u{2756}',
            'u' | '\u{f075}' => '\u{25c6}',
            '\u{a7}' | '\u{f0a7}' => '\u{25aa}',
            '\u{d8}' | '\u{f0d8}' => '\u{27a2}',
            '\u{fc}' | '\u{f0fc}' => '\u{2714}',
            '\u{e0}' | '\u{f0e0}' => '\u{2192}',
            'Ü' | '\u{f0dc}' => '\u{27a8}',
            'o' | '\u{f06f}' => '\u{25a1}',
            's' | '\u{f073}' => '\u{25c6}',
            't' | '\u{f074}' => '\u{25c6}',
            _ => '\u{2022}',
        }
    };
    mapped.to_string()
}

/// An autonumber's text: `arabicPeriod` 3 is `3.`, `romanLcParenR` 4 is `iv)`,
/// `alphaUcParenBoth` 2 is `(B)`.
pub(crate) fn number(scheme: &str, n: u32) -> String {
    let (digits, rest) = if let Some(r) = scheme.strip_prefix("arabic") {
        (n.to_string(), r)
    } else if let Some(r) = scheme.strip_prefix("romanUc") {
        (roman(n), r)
    } else if let Some(r) = scheme.strip_prefix("romanLc") {
        (roman(n).to_lowercase(), r)
    } else if let Some(r) = scheme.strip_prefix("alphaUc") {
        (alpha(n), r)
    } else if let Some(r) = scheme.strip_prefix("alphaLc") {
        (alpha(n).to_lowercase(), r)
    } else {
        (n.to_string(), "Period")
    };
    match rest {
        "ParenR" => format!("{digits})"),
        "ParenBoth" => format!("({digits})"),
        "Plain" => digits,
        "Minus" => format!("- {digits} -"),
        _ => format!("{digits}."),
    }
}

fn roman(mut n: u32) -> String {
    const TABLE: [(u32, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for (v, s) in TABLE {
        while n >= v {
            out.push_str(s);
            n -= v;
        }
    }
    out
}

/// A, B, ..., Z, AA, BB, ... as PowerPoint counts letters.
fn alpha(n: u32) -> String {
    let n = n.max(1) - 1;
    let letter = char::from(b'A' + (n % 26) as u8);
    std::iter::repeat_n(letter, (n / 26 + 1) as usize).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::parse;

    #[test]
    fn autonumbers() {
        assert_eq!(number("arabicPeriod", 3), "3.");
        assert_eq!(number("romanLcParenR", 4), "iv)");
        assert_eq!(number("alphaUcParenBoth", 2), "(B)");
        assert_eq!(number("alphaLcPeriod", 27), "aa.");
        assert_eq!(number("romanUcPeriod", 1994), "MCMXCIV.");
        assert_eq!(number("arabicPlain", 7), "7");
    }

    fn read(body: &str) -> TextBody {
        let fonts = Fonts {
            major: "Major".into(),
            minor: "Minor".into(),
            ..Fonts::default()
        };
        let palette = Palette::default();
        let ctx = TextContext {
            palette: &palette,
            fonts: &fonts,
            slide_number: 7,
        };
        let el = parse(body.as_bytes()).unwrap();
        let styles = Styles {
            base: Vec::new(),
            font_ref: FontRef::default(),
            shape: Vec::new(),
            body: Vec::new(),
        };
        self::body(Some(&el), &styles, &ctx).unwrap()
    }

    #[test]
    fn numbering_counts_per_level_and_restarts() {
        let t = read(
            r#"<p:txBody><a:bodyPr/>
            <a:p><a:pPr><a:buAutoNum type="arabicPeriod"/></a:pPr><a:r><a:t>one</a:t></a:r></a:p>
            <a:p><a:pPr lvl="1"><a:buAutoNum type="alphaLcParenR"/></a:pPr><a:r><a:t>a</a:t></a:r></a:p>
            <a:p><a:pPr lvl="1"><a:buAutoNum type="alphaLcParenR"/></a:pPr><a:r><a:t>b</a:t></a:r></a:p>
            <a:p><a:pPr><a:buAutoNum type="arabicPeriod"/></a:pPr></a:p>
            <a:p><a:pPr><a:buAutoNum type="arabicPeriod"/></a:pPr><a:r><a:t>two</a:t></a:r></a:p>
            <a:p><a:pPr lvl="1"><a:buAutoNum type="alphaLcParenR"/></a:pPr><a:r><a:t>a again</a:t></a:r></a:p>
            <a:p><a:pPr><a:buAutoNum type="romanUcPeriod" startAt="4"/></a:pPr><a:r><a:t>four</a:t></a:r></a:p>
            </p:txBody>"#,
        );
        let marks: Vec<Option<String>> = t
            .paragraphs
            .iter()
            .map(|p| p.bullet.as_ref().map(|b| b.text.clone()))
            .collect();
        let want = [
            Some("1."),
            Some("a)"),
            Some("b)"),
            None,
            Some("2."),
            Some("a)"),
            Some("IV."),
        ];
        assert_eq!(marks, want.map(|m| m.map(str::to_owned)));
    }

    #[test]
    fn bullets_take_size_colour_and_symbol_fonts() {
        let t = read(
            r#"<p:txBody><a:bodyPr/>
            <a:p><a:pPr><a:buClr><a:srgbClr val="FF0000"/></a:buClr><a:buSzPct val="50000"/><a:buFont typeface="Wingdings"/><a:buChar char="&#167;"/></a:pPr>
              <a:r><a:rPr sz="2400"><a:latin typeface="+mj-lt"/></a:rPr><a:t>x</a:t></a:r></a:p>
            <a:p><a:pPr><a:buNone/></a:pPr><a:r><a:t>none</a:t></a:r></a:p>
            <a:p><a:pPr><a:buFont typeface="Symbol"/><a:buChar char="&#xF02D;"/></a:pPr><a:r><a:t>minus</a:t></a:r></a:p>
            <a:p><a:pPr><a:buFont typeface="Symbol"/><a:buChar char="&#xF0B7;"/></a:pPr><a:r><a:t>dot</a:t></a:r></a:p>
            </p:txBody>"#,
        );
        let b = t.paragraphs[0].bullet.as_ref().unwrap();
        assert_eq!(b.text, "\u{25aa}");
        assert_eq!(b.font, "Major");
        assert_eq!(b.size, 12.0);
        assert_eq!(b.color.hex(), "FF0000");
        assert!(t.paragraphs[1].bullet.is_none());
        // Symbol's private-use code points (as LibreOffice writes them) map too.
        let marks: Vec<&str> = t.paragraphs[2..]
            .iter()
            .map(|p| p.bullet.as_ref().unwrap().text.as_str())
            .collect();
        assert_eq!(marks, ["\u{2212}", "\u{2022}"]);
    }

    #[test]
    fn runs_breaks_and_fields() {
        let t = read(
            r#"<p:txBody><a:bodyPr anchor="ctr" lIns="0" wrap="none"><a:normAutofit fontScale="62500" lnSpcReduction="20000"/></a:bodyPr>
            <a:p><a:r><a:rPr b="1" i="1" u="sng" strike="sngStrike" baseline="30000" cap="all"/><a:t>A</a:t></a:r><a:br/><a:fld type="slidenum"><a:t>‹#›</a:t></a:fld></a:p>
            </p:txBody>"#,
        );
        assert_eq!(t.anchor, Anchor::Middle);
        assert_eq!(t.insets.left, 0);
        assert!(!t.wrap);
        assert_eq!((t.font_scale, t.line_reduction), (0.625, 0.2));
        let p = &t.paragraphs[0];
        assert_eq!(p.text(), "A\n7");
        let a = &p.runs[0];
        assert!(a.bold && a.italic && a.underline && a.strike);
        assert_eq!(
            (a.baseline, a.caps, a.font.as_str()),
            (30.0, Caps::All, "Minor")
        );
    }
}
