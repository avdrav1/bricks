//! Shapes a save writes whole, from the model (FRAME-12): text boxes, preset shapes,
//! lines, pictures (a placeholder's too), and tables; a text body's autofit; and the
//! extension and content type of the media part a picture brings.

use crate::save::{outline_markup, paragraphs, ph_tag, solid_fill};
use crate::xml::escape;
use presentation_model::{
    Anchor, Autofit, Crop, Fill, Frame, ImageFormat, Insets, PlaceholderSlot, Shape, ShapeKind,
    Table, TextBody, Vertical,
};
use std::io;

/// The namespace prefixes a slide part's shapes are written with, each with its colon:
/// PresentationML's, DrawingML's, and relationships'; `r_decl` declares the last on the
/// element that uses it when the part doesn't (empty when it does).
pub(crate) struct Prefixes {
    pub p: String,
    pub a: String,
    pub r: String,
    pub r_decl: String,
}

const TABLE_URI: &str = "http://schemas.openxmlformats.org/drawingml/2006/table";

/// A picture format's file extension and content type.
pub(crate) fn media_type(format: ImageFormat) -> (&'static str, &'static str) {
    match format {
        ImageFormat::Png => ("png", "image/png"),
        ImageFormat::Jpeg => ("jpeg", "image/jpeg"),
        ImageFormat::Gif => ("gif", "image/gif"),
        ImageFormat::Bmp => ("bmp", "image/bmp"),
        ImageFormat::Tiff => ("tiff", "image/tiff"),
        ImageFormat::Emf => ("emf", "image/x-emf"),
        ImageFormat::Wmf => ("wmf", "image/x-wmf"),
        ImageFormat::Svg => ("svg", "image/svg+xml"),
        ImageFormat::Other => ("bin", "application/octet-stream"),
    }
}

/// The xfrm element `tag` (with its prefix: `a:xfrm`, a graphic frame's `p:xfrm`) for
/// `frame`, its rotation in 60,000ths of a degree.
fn xfrm(tag: &str, a: &str, f: &Frame) -> String {
    let mut attrs = String::new();
    if f.rotation != 0.0 {
        attrs.push_str(&format!(
            r#" rot="{}""#,
            (f.rotation * 60_000.0).round() as i64
        ));
    }
    if f.flip_h {
        attrs.push_str(r#" flipH="1""#);
    }
    if f.flip_v {
        attrs.push_str(r#" flipV="1""#);
    }
    format!(
        r#"<{tag}{attrs}><{a}off x="{}" y="{}"/><{a}ext cx="{}" cy="{}"/></{tag}>"#,
        f.x, f.y, f.cx, f.cy
    )
}

/// The shape's fill and outline as `spPr` children, each when it's set on the shape.
fn looks(s: &Shape, a: &str) -> io::Result<String> {
    let mut out = String::new();
    if s.sources.fill.is_own() {
        match &s.fill {
            Fill::None => out.push_str(&format!("<{a}noFill/>")),
            Fill::Solid(c) => out.push_str(&solid_fill(a, *c)),
            Fill::Picture(_) | Fill::Unsupported { .. } => {}
        }
    }
    if s.sources.outline.is_own() {
        out.push_str(&outline_markup(&format!("<{a}ln/>"), s.outline)?);
    }
    Ok(out)
}

/// A text body's autofit element: `<a:normAutofit>` with its font scale and line
/// spacing reduction in thousandths of a percent (each left out at 100% and 0),
/// `<a:noAutofit/>`, or `<a:spAutoFit/>`; empty when unset.
pub(crate) fn autofit(t: &TextBody, a: &str) -> String {
    match t.autofit {
        Autofit::Unset => String::new(),
        Autofit::Off => format!("<{a}noAutofit/>"),
        Autofit::Grow => format!("<{a}spAutoFit/>"),
        Autofit::Shrink => {
            let scale = (t.font_scale * 100_000.0).round() as i64;
            let reduction = (t.line_reduction * 100_000.0).round() as i64;
            let mut attrs = String::new();
            if scale != 100_000 {
                attrs.push_str(&format!(r#" fontScale="{scale}""#));
            }
            if reduction != 0 {
                attrs.push_str(&format!(r#" lnSpcReduction="{reduction}""#));
            }
            format!("<{a}normAutofit{attrs}/>")
        }
    }
}

/// `<a:bodyPr>` for a text body: its turn, wrapping, insets (when not PowerPoint's
/// defaults), anchoring, and autofit.
fn body_pr(t: &TextBody, a: &str) -> String {
    let mut attrs = String::new();
    match t.vertical {
        Vertical::Down => attrs.push_str(r#" vert="vert""#),
        Vertical::Up => attrs.push_str(r#" vert="vert270""#),
        Vertical::Horizontal => {}
    }
    attrs.push_str(if t.wrap {
        r#" wrap="square""#
    } else {
        r#" wrap="none""#
    });
    let d = Insets::default();
    for (name, value, default) in [
        ("lIns", t.insets.left, d.left),
        ("tIns", t.insets.top, d.top),
        ("rIns", t.insets.right, d.right),
        ("bIns", t.insets.bottom, d.bottom),
    ] {
        if value != default {
            attrs.push_str(&format!(r#" {name}="{value}""#));
        }
    }
    attrs.push_str(match t.anchor {
        Anchor::Top => r#" anchor="t""#,
        Anchor::Middle => r#" anchor="ctr""#,
        Anchor::Bottom => r#" anchor="b""#,
    });
    if t.anchor_centre {
        attrs.push_str(r#" anchorCtr="1""#);
    }
    match autofit(t, a) {
        fit if fit.is_empty() => format!("<{a}bodyPr{attrs}/>"),
        fit => format!("<{a}bodyPr{attrs}>{fit}</{a}bodyPr>"),
    }
}

/// A text body's paragraphs, at least one (an empty body is one empty paragraph).
fn body_paragraphs(t: &TextBody, a: &str) -> io::Result<String> {
    if t.paragraphs.is_empty() {
        return Ok(format!("<{a}p/>"));
    }
    paragraphs(t, a)
}

/// `<p:cNvPr>` for a shape.
fn c_nv_pr(p: &str, s: &Shape) -> String {
    format!(
        r#"<{p}cNvPr id="{}" name="{}"/>"#,
        s.id.unwrap_or_default(),
        escape(&s.name)
    )
}

/// A shape the slide part doesn't have, whole; `rid` is a picture's image relationship.
pub(crate) fn shape(s: &Shape, ns: &Prefixes, rid: &str) -> io::Result<String> {
    match &s.kind {
        ShapeKind::Picture(pic) => picture(s, pic.crop, ns, rid, None),
        ShapeKind::Table(t) => table(s, t, ns),
        ShapeKind::Geometry { preset, adjust } => geometry(s, preset, *adjust, ns),
        ShapeKind::Unsupported { label, .. } => {
            Err(io::Error::other(format!("a new {label} can't be saved")))
        }
    }
}

/// A preset shape (`<p:sp>`, with its text when it has some), or a line
/// (`<p:cxnSp>`). A text box (it grows to fit its text, and has no fill) says so.
fn geometry(s: &Shape, preset: &str, adjust: Option<f64>, ns: &Prefixes) -> io::Result<String> {
    let (p, a) = (ns.p.as_str(), ns.a.as_str());
    let prst = if preset == "custom" { "rect" } else { preset };
    let av = match adjust {
        Some(v) => format!(
            r#"<{a}avLst><{a}gd name="adj" fmla="val {}"/></{a}avLst>"#,
            (v * 100_000.0).round() as i64
        ),
        None => format!("<{a}avLst/>"),
    };
    let sp_pr = format!(
        r#"<{p}spPr>{}<{a}prstGeom prst="{}">{av}</{a}prstGeom>{}</{p}spPr>"#,
        xfrm(&format!("{a}xfrm"), a, &s.frame),
        escape(prst),
        looks(s, a)?
    );
    if prst == "line" {
        return Ok(format!(
            concat!(
                "<{p}cxnSp><{p}nvCxnSpPr>{c}<{p}cNvCxnSpPr/><{p}nvPr/></{p}nvCxnSpPr>",
                "{sp_pr}</{p}cxnSp>"
            ),
            p = p,
            c = c_nv_pr(p, s),
            sp_pr = sp_pr
        ));
    }
    let text_box = s
        .text
        .as_ref()
        .is_some_and(|t| t.autofit == Autofit::Grow && s.fill == Fill::None);
    let c_nv_sp_pr = if text_box {
        format!(r#"<{p}cNvSpPr txBox="1"/>"#)
    } else {
        format!("<{p}cNvSpPr/>")
    };
    let tx_body = match &s.text {
        Some(t) => format!(
            "<{p}txBody>{}<{a}lstStyle/>{}</{p}txBody>",
            body_pr(t, a),
            body_paragraphs(t, a)?
        ),
        None => String::new(),
    };
    Ok(format!(
        "<{p}sp><{p}nvSpPr>{}{c_nv_sp_pr}<{p}nvPr/></{p}nvSpPr>{sp_pr}{tx_body}</{p}sp>",
        c_nv_pr(p, s)
    ))
}

/// A picture (`<p:pic>`) showing the image of relationship `rid`, cropped by `crop`.
/// In a placeholder (`placeholder`: its slot, and whether to write its box rather than
/// take its layout's), it keeps the placeholder's `<p:ph>`.
pub(crate) fn picture(
    s: &Shape,
    crop: Crop,
    ns: &Prefixes,
    rid: &str,
    placeholder: Option<(&PlaceholderSlot, bool)>,
) -> io::Result<String> {
    let (p, a, r) = (ns.p.as_str(), ns.a.as_str(), ns.r.as_str());
    let box_ = xfrm(&format!("{a}xfrm"), a, &s.frame);
    let (locks, ph, sp_pr) = match placeholder {
        Some((slot, own_box)) => {
            let content = format!(
                "{}{}",
                if own_box { box_ } else { String::new() },
                looks(s, a)?
            );
            let sp_pr = if content.is_empty() {
                format!("<{p}spPr/>")
            } else {
                format!("<{p}spPr>{content}</{p}spPr>")
            };
            (r#" noGrp="1" noChangeAspect="1""#, ph_tag(p, slot), sp_pr)
        }
        None => (
            r#" noChangeAspect="1""#,
            String::new(),
            format!(
                r#"<{p}spPr>{box_}<{a}prstGeom prst="rect"><{a}avLst/></{a}prstGeom>{}</{p}spPr>"#,
                looks(s, a)?
            ),
        ),
    };
    let mut src_rect = String::new();
    for (name, v) in [
        ("l", crop.left),
        ("t", crop.top),
        ("r", crop.right),
        ("b", crop.bottom),
    ] {
        let v = (v * 100_000.0).round() as i64;
        if v != 0 {
            src_rect.push_str(&format!(r#" {name}="{v}""#));
        }
    }
    let src_rect = if src_rect.is_empty() {
        src_rect
    } else {
        format!("<{a}srcRect{src_rect}/>")
    };
    Ok(format!(
        concat!(
            "<{p}pic><{p}nvPicPr>{c}<{p}cNvPicPr><{a}picLocks{locks}/></{p}cNvPicPr>",
            "<{p}nvPr>{ph}</{p}nvPr></{p}nvPicPr><{p}blipFill>",
            r#"<{a}blip{decl} {r}embed="{rid}"/>{src}<{a}stretch><{a}fillRect/></{a}stretch>"#,
            "</{p}blipFill>{sp_pr}</{p}pic>"
        ),
        p = p,
        a = a,
        r = r,
        c = c_nv_pr(p, s),
        locks = locks,
        ph = ph,
        decl = ns.r_decl,
        rid = escape(rid),
        src = src_rect,
        sp_pr = sp_pr
    ))
}

/// A table (`<p:graphicFrame>` holding `<a:tbl>`): its grid, rows, and cells with their
/// text, borders, and fills.
fn table(s: &Shape, t: &Table, ns: &Prefixes) -> io::Result<String> {
    let (p, a) = (ns.p.as_str(), ns.a.as_str());
    let grid: String = t
        .columns
        .iter()
        .map(|w| format!(r#"<{a}gridCol w="{w}"/>"#))
        .collect();
    let mut rows = String::new();
    for row in &t.rows {
        rows.push_str(&format!(r#"<{a}tr h="{}">"#, row.height));
        for cell in &row.cells {
            let mut attrs = String::new();
            if cell.col_span > 1 {
                attrs.push_str(&format!(r#" gridSpan="{}""#, cell.col_span));
            }
            if cell.row_span > 1 {
                attrs.push_str(&format!(r#" rowSpan="{}""#, cell.row_span));
            }
            if cell.merged {
                attrs.push_str(r#" hMerge="1""#);
            }
            let text = &cell.text;
            let mut pr = String::new();
            let d = Insets::default();
            for (name, value, default) in [
                ("marL", text.insets.left, d.left),
                ("marR", text.insets.right, d.right),
                ("marT", text.insets.top, d.top),
                ("marB", text.insets.bottom, d.bottom),
            ] {
                if value != default {
                    pr.push_str(&format!(r#" {name}="{value}""#));
                }
            }
            match text.anchor {
                Anchor::Top => {}
                Anchor::Middle => pr.push_str(r#" anchor="ctr""#),
                Anchor::Bottom => pr.push_str(r#" anchor="b""#),
            }
            let b = &cell.borders;
            let mut content = String::new();
            for (name, side) in [
                ("lnL", b.left),
                ("lnR", b.right),
                ("lnT", b.top),
                ("lnB", b.bottom),
            ] {
                content.push_str(&outline_markup(&format!("<{a}{name}/>"), side)?);
            }
            content.push_str(&match cell.fill {
                Some(c) => solid_fill(a, c),
                None => format!("<{a}noFill/>"),
            });
            rows.push_str(&format!(
                concat!(
                    "<{a}tc{attrs}><{a}txBody><{a}bodyPr/><{a}lstStyle/>{paras}</{a}txBody>",
                    "<{a}tcPr{pr}>{content}</{a}tcPr></{a}tc>"
                ),
                a = a,
                attrs = attrs,
                paras = body_paragraphs(text, a)?,
                pr = pr,
                content = content
            ));
        }
        rows.push_str(&format!("</{a}tr>"));
    }
    Ok(format!(
        concat!(
            "<{p}graphicFrame><{p}nvGraphicFramePr>{c}<{p}cNvGraphicFramePr>",
            r#"<{a}graphicFrameLocks noGrp="1"/></{p}cNvGraphicFramePr><{p}nvPr/>"#,
            "</{p}nvGraphicFramePr>{xfrm}<{a}graphic>",
            r#"<{a}graphicData uri="{uri}"><{a}tbl><{a}tblPr firstRow="1" bandRow="1"/>"#,
            "<{a}tblGrid>{grid}</{a}tblGrid>{rows}</{a}tbl></{a}graphicData></{a}graphic>",
            "</{p}graphicFrame>"
        ),
        p = p,
        a = a,
        c = c_nv_pr(p, s),
        xfrm = xfrm(&format!("{p}xfrm"), a, &s.frame),
        uri = TABLE_URI,
        grid = grid,
        rows = rows
    ))
}
