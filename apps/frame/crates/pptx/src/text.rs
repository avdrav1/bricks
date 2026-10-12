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
//!
//! Each run records where each of its looks is set (`Run::sources`, ADR 0006): the shape
//! itself (steps 3 to 5 on the shape), its layout's or master's placeholder (step 4), the
//! master's text styles (step 2), or the presentation (step 1 and PowerPoint's defaults),
//! so an edit keeps inherited looks inherited. Runs and paragraphs also keep their markup
//! as written (`Run::markup`, `Paragraph::markup`), for a save to write back. Neighbouring
//! runs that look the same and are written the same merge.

use crate::color::Palette;
use crate::xml::Element;
use presentation_model::{
    Align, Anchor, AutoNumber, Bullet, BulletSize, Caps, Color, Insets, Paragraph, Run, RunMarkup,
    RunSources, Source, Spacing, TextBody, Vertical,
};
use std::sync::Arc;

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
    /// The shape's own style, or a table style (the presentation's).
    pub source: Source,
}

/// A shape's text styles: `base` (default text style and master text style), its style
/// reference, and `shape` (placeholder list styles and its own), lowest priority first,
/// each with where it's set.
pub(crate) struct Styles<'a> {
    pub base: Vec<(&'a Element, Source)>,
    pub font_ref: FontRef,
    pub shape: Vec<(&'a Element, Source)>,
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
    /// Where each of the above was last set.
    from: RunSources,
}

#[derive(Debug, Clone, PartialEq)]
enum BulletKind {
    Off,
    Char(String),
    Number { scheme: String, start: u32 },
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
    /// Apply `rpr`, set at `source`.
    fn apply(&mut self, rpr: &Element, ctx: &TextContext<'_>, source: Source) {
        let from = &mut self.from;
        if let Some(sz) = rpr.num::<f64>("sz") {
            self.size = Some(sz / 100.0);
            from.size = source;
        }
        if let Some(b) = rpr.attr("b") {
            self.bold = Some(on(b));
            from.bold = source;
        }
        if let Some(i) = rpr.attr("i") {
            self.italic = Some(on(i));
            from.italic = source;
        }
        if let Some(u) = rpr.attr("u") {
            self.underline = Some(u != "none");
            from.underline = source;
        }
        if let Some(s) = rpr.attr("strike") {
            self.strike = Some(s != "noStrike");
            from.strike = source;
        }
        if let Some(b) = rpr.num::<f64>("baseline") {
            self.baseline = Some(b / 1000.0);
            from.baseline = source;
        }
        if let Some(c) = rpr.attr("cap") {
            self.caps = Some(match c {
                "all" => Caps::All,
                "small" => Caps::Small,
                _ => Caps::None,
            });
            from.caps = source;
        }
        for e in rpr.elements() {
            let color = match e.name.as_str() {
                "solidFill" => ctx.palette.first(e, None),
                // Gradient text shows as its first stop's colour.
                "gradFill" => e
                    .path(&["gsLst", "gs"])
                    .and_then(|gs| ctx.palette.first(gs, None)),
                "noFill" => Some(Color {
                    alpha: 0,
                    ..Color::BLACK
                }),
                "latin" => {
                    if let Some(face) = e.attr("typeface").filter(|f| !f.is_empty()) {
                        self.font = Some(ctx.fonts.resolve(face));
                        from.font = source;
                    }
                    None
                }
                _ => None,
            };
            if let Some(c) = color {
                self.color = Some(c);
                from.color = source;
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
    /// Apply `ppr`, set at `source`.
    fn apply(&mut self, ppr: &Element, ctx: &TextContext<'_>, source: Source) {
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
                "defRPr" => self.run.apply(e, ctx, source),
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
        let apply = |(style, source): &(&Element, Source), props: &mut ParaProps| {
            if let Some(d) = style.child("defPPr") {
                props.apply(d, ctx, *source);
            }
            if let Some(l) = style.child(LEVELS[level]) {
                props.apply(l, ctx, *source);
            }
        };
        for s in &self.base {
            apply(s, &mut props);
        }
        let (fr, run) = (&self.font_ref, &mut props.run);
        if let Some(f) = &fr.font {
            run.font = Some(f.clone());
            run.from.font = fr.source;
        }
        if let Some(c) = fr.color {
            run.color = Some(c);
            run.from.color = fr.source;
        }
        if let Some(b) = fr.bold {
            run.bold = Some(b);
            run.from.bold = fr.source;
        }
        if let Some(i) = fr.italic {
            run.italic = Some(i);
            run.from.italic = fr.source;
        }
        for s in &self.shape {
            apply(s, &mut props);
        }
        props
    }
}

/// An element as written in `source`, the bytes of the part it's in.
fn raw(source: &[u8], e: &Element) -> Arc<str> {
    let bytes = source.get(e.span.clone()).unwrap_or_default();
    Arc::from(String::from_utf8_lossy(bytes))
}

/// A field's start tag as written: up to its first child, or its empty tag opened.
fn start_tag(source: &[u8], e: &Element) -> Arc<str> {
    let end = e.elements().next().map_or(e.span.end, |c| c.span.start);
    let bytes = source.get(e.span.start..end).unwrap_or_default();
    let tag = String::from_utf8_lossy(bytes);
    match tag.trim_end().strip_suffix("/>") {
        Some(open) => Arc::from(format!("{open}>")),
        None => Arc::from(tag),
    }
}

/// Read a `txBody` with its styles. `None` when there's no text body. `source` is the
/// bytes of the part it's in: runs and paragraphs keep their markup as written.
pub(crate) fn body(
    tx_body: Option<&Element>,
    source: &[u8],
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
    for p in tx_body.all("p") {
        let ppr = p.child("pPr");
        let level = ppr.and_then(|p| p.num::<usize>("lvl")).unwrap_or(0).min(8);
        let mut props = styles.level(level, ctx);
        if let Some(ppr) = ppr {
            props.apply(ppr, ctx, Source::Own);
        }
        let mut runs = Vec::new();
        let mut end = props.run.clone();
        let mut end_markup = Arc::<str>::from("");
        for r in p.elements() {
            let mut rp = props.run.clone();
            let rpr = r.child("rPr");
            if let Some(rpr) = rpr {
                rp.apply(rpr, ctx, Source::Own);
            }
            let text = match r.name.as_str() {
                "r" => r.child("t").map(|t| t.text()).unwrap_or_default(),
                "br" => "\n".to_owned(),
                "fld" if r.attr("type") == Some("slidenum") => ctx.slide_number.to_string(),
                "fld" => r.child("t").map(|t| t.text()).unwrap_or_default(),
                "endParaRPr" => {
                    end.apply(r, ctx, Source::Own);
                    end_markup = raw(source, r);
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
                rp.from.color = Source::Own;
                rp.from.underline = Source::Own;
            }
            let mut run = run(text, &rp, ctx);
            run.markup = RunMarkup {
                field: if r.name == "fld" {
                    start_tag(source, r)
                } else {
                    Arc::from("")
                },
                props: rpr.map_or_else(|| Arc::from(""), |e| raw(source, e)),
            };
            run.slide_number = r.name == "fld" && r.attr("type") == Some("slidenum");
            runs.push(run);
        }
        let mut paragraph = Paragraph {
            level: level as u8,
            align: props.align.unwrap_or_default(),
            margin_left: props.margin_left.unwrap_or(0),
            indent: props.indent.unwrap_or(0),
            line_spacing: props.line_spacing.unwrap_or(Spacing::Lines(1.0)),
            space_before: props.space_before.unwrap_or(Spacing::Points(0.0)),
            space_after: props.space_after.unwrap_or(Spacing::Points(0.0)),
            bullet: bullet(&props),
            runs,
            end: Run {
                markup: RunMarkup {
                    field: Arc::from(""),
                    props: end_markup,
                },
                ..run(String::new(), &end, ctx)
            },
            markup: ppr.map_or_else(|| Arc::from(""), |e| raw(source, e)),
        };
        paragraph.merge_runs();
        text.paragraphs.push(paragraph);
    }
    text.number_bullets();
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
        sources: p.from,
        markup: RunMarkup::default(),
        slide_number: false,
    }
}

/// The paragraph's bullet; numbers are counted once the body is read.
fn bullet(props: &ParaProps) -> Option<Bullet> {
    let font = props.bullet_font.clone().flatten();
    let (text, autonumber) = match props.bullet.clone().unwrap_or(BulletKind::Off) {
        BulletKind::Off => return None,
        BulletKind::Char(c) => (symbol(&c, font.as_deref()), None),
        BulletKind::Number { scheme, start } => (String::new(), Some(AutoNumber { scheme, start })),
    };
    Some(Bullet {
        text,
        autonumber,
        // Symbol fonts were mapped to Unicode above: draw them in the text's font.
        font: font.filter(|f| !is_symbol_font(f)),
        size: props.bullet_size.unwrap_or(BulletSize::FollowText),
        color: props.bullet_color.flatten(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::parse;

    fn read_with(body: &str, styles: &[(&str, Source)]) -> TextBody {
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
        let styles: Vec<(Element, Source)> = styles
            .iter()
            .map(|(s, source)| (parse(s.as_bytes()).unwrap(), *source))
            .collect();
        let styles = Styles {
            base: Vec::new(),
            font_ref: FontRef::default(),
            shape: styles.iter().map(|(e, s)| (e, *s)).collect(),
            body: Vec::new(),
        };
        self::body(Some(&el), body.as_bytes(), &styles, &ctx).unwrap()
    }

    fn read(body: &str) -> TextBody {
        read_with(body, &[])
    }

    #[test]
    fn sources_say_where_each_look_is_set() {
        let master = r#"<a:lstStyle><a:lvl1pPr><a:defRPr sz="4400" b="1"><a:solidFill><a:srgbClr val="112233"/></a:solidFill></a:defRPr></a:lvl1pPr></a:lstStyle>"#;
        let layout = r#"<a:lstStyle><a:lvl1pPr><a:defRPr sz="3200"/></a:lvl1pPr></a:lstStyle>"#;
        let t = read_with(
            r#"<p:txBody><a:bodyPr/>
            <a:p><a:r><a:rPr lang="en-US"/><a:t>inherited</a:t></a:r><a:r><a:rPr i="1"/><a:t> own</a:t></a:r><a:endParaRPr b="0"/></a:p>
            </p:txBody>"#,
            &[(master, Source::Master), (layout, Source::Layout)],
        );
        let p = &t.paragraphs[0];
        let (a, b) = (&p.runs[0], &p.runs[1]);
        assert_eq!(
            (a.size, a.bold, a.color.hex()),
            (32.0, true, "112233".into())
        );
        assert_eq!(
            (a.sources.size, a.sources.bold, a.sources.color),
            (Source::Layout, Source::Master, Source::Master)
        );
        assert_eq!(
            (a.sources.italic, a.sources.font),
            (Source::Presentation, Source::Presentation)
        );
        assert_eq!((b.italic, b.sources.italic), (true, Source::Own));
        assert_eq!(b.sources.size, Source::Layout);
        // The paragraph mark keeps its own looks apart from its runs'.
        assert_eq!((p.end.bold, p.end.sources.bold), (false, Source::Own));
        assert_eq!((p.end.size, p.end.sources.size), (32.0, Source::Layout));
    }

    #[test]
    fn runs_that_look_and_are_written_the_same_merge() {
        let t = read(
            r#"<p:txBody><a:bodyPr/>
            <a:p><a:r><a:rPr lang="en-US"/><a:t>one </a:t></a:r><a:r><a:rPr lang="en-US"/><a:t>run</a:t></a:r><a:r><a:rPr lang="de-DE"/><a:t> Lauf</a:t></a:r><a:r><a:t></a:t></a:r><a:br/><a:r><a:rPr b="1"/><a:t>bold</a:t></a:r></a:p>
            </p:txBody>"#,
        );
        let runs: Vec<&str> = t.paragraphs[0]
            .runs
            .iter()
            .map(|r| r.text.as_str())
            .collect();
        assert_eq!(runs, ["one run", " Lauf", "\n", "bold"]);
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
            .map(|p| p.shown_bullet().map(|b| b.text.to_owned()))
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
        let b = t.paragraphs[0].shown_bullet().unwrap();
        assert_eq!(b.text, "\u{25aa}");
        assert_eq!(b.font, "Major");
        assert_eq!(b.size, 12.0);
        assert_eq!(b.color.hex(), "FF0000");
        assert!(t.paragraphs[1].shown_bullet().is_none());
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
        // Markup as written, for a save: the run's properties, the field's start tag.
        assert_eq!(
            &*a.markup.props,
            r#"<a:rPr b="1" i="1" u="sng" strike="sngStrike" baseline="30000" cap="all"/>"#
        );
        assert_eq!(&*a.markup.field, "");
        assert_eq!(&*p.runs[2].markup.field, r#"<a:fld type="slidenum">"#);
    }
}
