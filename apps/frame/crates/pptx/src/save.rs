//! Saving a deck by patching the package it was read from (ADR 0006, FRAME-8, FRAME-11).
//! Parts no edit touches are copied as compressed bytes. What changes:
//!
//! - an edited shape's paragraphs, in its slide part: written from the model, each run
//!   and paragraph with the markup it was read with (`Run::markup`,
//!   `Paragraph::markup`), the looks an edit set (`Source::Edit`) written into it;
//!   everything else in the part stays as written;
//! - an edited shape's fill and outline, in its `spPr`; a slide's background set by an
//!   edit, as its `<p:bg>`;
//! - a slide moved to another layout: its layout relationship, its placeholders' `<p:ph>`,
//!   its empty placeholders the move dropped, and the new layout's it gained;
//! - `presentation.xml`'s slide list (and its sections, when it has them), its
//!   relationships, and `[Content_Types].xml`, as slides are added, deleted, and moved;
//! - a new slide's part: the slide XML PowerPoint writes for one on its layout, with its
//!   text; a duplicate's: its source's part and relationships, with its own copy of the
//!   source's notes;
//! - a deleted slide's part, relationships, and notes go;
//! - shapes added to a slide (text boxes, preset shapes, lines, pictures, tables): written
//!   whole from the model at their place in its shape tree (`write`), a picture's image
//!   as a new media part with a relationship from the slide; a placeholder a picture
//!   filled becomes a picture that keeps its `<p:ph>`; shapes deleted go;
//! - text shrunk to fit: its body's `<a:normAutofit>`.
//!
//! The package is written with each part's sizes in its local header (LibreOffice can't
//! read a zip streamed with data descriptors), through `suite_file::save_atomic`.

use crate::package::resolve;
use crate::read::blank_slide;
use crate::write::{self as shapes, Prefixes};
use crate::xml::{self, escape, Element};
use presentation_model::{
    Align, Autofit, Background, Bullet, BulletSize, Color, Dash, Fill, Frame, Image, Layer,
    Outline, Paragraph, PlaceholderSlot, Presentation, Run, Shape, ShapeKind, Slide, Source,
    TextBody,
};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{self, BufReader, Read, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use suite_file::{save_atomic, SaveError, WriteSeek};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const SLIDE_TYPE: &str = "application/vnd.openxmlformats-officedocument.presentationml.slide+xml";
const NOTES_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml";
const REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const RELS_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const XML_DECLARATION: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";

/// A private copy of the package a deck was read from, in the cache directory
/// (`$XDG_CACHE_HOME/frame`), removed with it. Every save patches it, so the file may
/// change, move, or be saved over meanwhile, and a deck saved more than once (with undo
/// in between) still finds every part it was read with.
#[derive(Debug)]
pub struct Original {
    copy: PathBuf,
}

impl Original {
    /// Copy the package at `path`.
    pub fn copy_of(path: &Path) -> io::Result<Original> {
        static N: AtomicU64 = AtomicU64::new(0);
        let base = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
            .unwrap_or_else(std::env::temp_dir);
        let dir = base.join("frame");
        std::fs::create_dir_all(&dir)?;
        let copy = dir.join(format!(
            "original-{}-{}.pptx",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::copy(path, &copy)?;
        Ok(Original { copy })
    }

    /// Where the copy is.
    pub fn path(&self) -> &Path {
        &self.copy
    }
}

impl Drop for Original {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.copy);
    }
}

/// Save `deck` at `dest`, atomically, by patching `source`, the package `read` was read
/// from. Slides are matched to the parts they were read from by `Slide::part`. Verification
/// reads the written file back: its slide count; the text, boxes, fills, and outlines of
/// every shape the save wrote; the kind of every shape it wrote whole (a picture's bytes,
/// a table's cells); the text fits it wrote; the shapes it took out; and the backgrounds
/// and layouts it wrote.
pub fn save(
    dest: &Path,
    source: &Path,
    read: &Presentation,
    deck: &Presentation,
    cancel: &AtomicBool,
) -> Result<(), SaveError> {
    let written = RefCell::new(None);
    save_atomic(
        dest,
        cancel,
        |out| {
            *written.borrow_mut() = Some(write(out, source, read, deck)?);
            Ok(())
        },
        |tmp| verify(tmp, deck, written.borrow().as_ref()),
    )
}

/// What a save wrote on one slide: the ids of the shapes whose text, boxes, and fills or
/// outlines it wrote, of those it wrote whole, of those whose text fit it wrote, and of
/// those it took out; whether it wrote the background and the layout.
#[derive(Debug, Default)]
struct Wrote {
    text: Vec<u32>,
    frames: Vec<u32>,
    looks: Vec<u32>,
    shapes: Vec<u32>,
    fits: Vec<u32>,
    removed: Vec<u32>,
    background: bool,
    layout: bool,
}

/// What a save wrote, by slide.
type Written = Vec<Wrote>;

fn verify(tmp: &Path, deck: &Presentation, written: Option<&Written>) -> Result<(), String> {
    let back = crate::read(tmp).map_err(|e| e.to_string())?;
    if back.slides.len() != deck.slides.len() {
        return Err(format!(
            "{} slides, not {}",
            back.slides.len(),
            deck.slides.len()
        ));
    }
    let written = written.ok_or("nothing was written")?;
    let slides = deck.slides.iter().zip(&back.slides).zip(written);
    for (n, ((now, again), wrote)) in slides.enumerate() {
        let shape = |s: &'_ Slide, id: u32| s.shapes.iter().find(|sh| sh.id == Some(id)).cloned();
        let differs = |id: &u32, same: &dyn Fn(&Shape, &Shape) -> bool| match (
            shape(now, *id),
            shape(again, *id),
        ) {
            (Some(a), Some(b)) => !same(&a, &b),
            _ => true,
        };
        let text_differs = wrote
            .text
            .iter()
            .find(|id| differs(id, &|a, b| a.plain_text() == b.plain_text()));
        let frame_differs = wrote
            .frames
            .iter()
            .find(|id| differs(id, &|a, b| a.frame == b.frame));
        let look_differs = wrote
            .looks
            .iter()
            .find(|id| differs(id, &|a, b| a.fill == b.fill && a.outline == b.outline));
        let shape_differs = wrote.shapes.iter().find(|id| differs(id, &reads_back));
        let fit_differs = wrote.fits.iter().find(|id| differs(id, &same_fit));
        let still_there = wrote.removed.iter().find(|id| shape(again, **id).is_some());
        if let Some(id) = text_differs
            .or(frame_differs)
            .or(look_differs)
            .or(shape_differs)
            .or(fit_differs)
            .or(still_there)
        {
            return Err(format!(
                "slide {}'s shape {id} reads back differently",
                n + 1
            ));
        }
        if wrote.background && now.background != again.background {
            return Err(format!(
                "slide {}'s background reads back differently",
                n + 1
            ));
        }
        if wrote.layout && now.layout != again.layout {
            return Err(format!("slide {}'s layout reads back differently", n + 1));
        }
    }
    Ok(())
}

/// Whether `b`, read back, is `a`, a shape the save wrote whole: its kind (a preset's
/// name, a picture's bytes and crop, a table's grid and its cells' text and fills), box,
/// text, and the fill and outline set on it.
fn reads_back(a: &Shape, b: &Shape) -> bool {
    let close = |x: f64, y: f64| (x - y).abs() < 1e-4;
    let kind = match (&a.kind, &b.kind) {
        (ShapeKind::Geometry { preset: p, .. }, ShapeKind::Geometry { preset: q, .. }) => {
            p == q || p == "custom"
        }
        (ShapeKind::Picture(p), ShapeKind::Picture(q)) => {
            let (c, d) = (p.crop, q.crop);
            p.image.bytes == q.image.bytes
                && close(c.left, d.left)
                && close(c.top, d.top)
                && close(c.right, d.right)
                && close(c.bottom, d.bottom)
        }
        (ShapeKind::Table(t), ShapeKind::Table(u)) => {
            t.columns == u.columns
                && t.rows.len() == u.rows.len()
                && t.rows.iter().zip(&u.rows).all(|(r, s)| {
                    r.cells.len() == s.cells.len()
                        && r.cells.iter().zip(&s.cells).all(|(c, d)| {
                            c.text.plain_text() == d.text.plain_text() && c.fill == d.fill
                        })
                })
        }
        _ => false,
    };
    let written = |f: &Fill| matches!(f, Fill::None | Fill::Solid(_));
    let fill = !a.sources.fill.is_own() || !written(&a.fill) || a.fill == b.fill;
    let outline = !a.sources.outline.is_own() || a.outline == b.outline;
    kind && a.frame == b.frame && a.plain_text() == b.plain_text() && fill && outline
}

/// Whether `b`'s text fit, read back, is `a`'s: its autofit, and when it shrinks, its
/// font scale and line spacing reduction.
fn same_fit(a: &Shape, b: &Shape) -> bool {
    match (&a.text, &b.text) {
        (Some(x), Some(y)) => {
            let close = |p: f64, q: f64| (p - q).abs() < 1e-4;
            (x.autofit == Autofit::Unset || x.autofit == y.autofit)
                && (x.autofit != Autofit::Shrink
                    || close(x.font_scale, y.font_scale)
                        && close(x.line_reduction, y.line_reduction))
        }
        _ => false,
    }
}

fn zip_error(e: zip::result::ZipError) -> io::Error {
    match e {
        zip::result::ZipError::Io(e) => e,
        e => io::Error::other(e.to_string()),
    }
}

fn damaged(what: impl std::fmt::Display) -> io::Error {
    io::Error::other(format!("the presentation can't be patched: {what}"))
}

type Src = ZipArchive<BufReader<File>>;

fn read_text(src: &mut Src, name: &str) -> io::Result<Option<String>> {
    let mut entry = match src.by_name(name) {
        Ok(e) => e,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(e) => return Err(zip_error(e)),
    };
    let mut out = String::new();
    entry.read_to_string(&mut out)?;
    Ok(Some(out))
}

fn parse(text: &str, name: &str) -> io::Result<Element> {
    xml::parse(text.as_bytes()).map_err(|e| damaged(format!("{name}: {e}")))
}

/// The directory of part `name` (`ppt` for `ppt/presentation.xml`).
fn dir(name: &str) -> &str {
    name.rsplit_once('/').map_or("", |(d, _)| d)
}

/// The relationships part of part `name`.
fn rels_name(name: &str) -> String {
    match name.rsplit_once('/') {
        Some((d, f)) => format!("{d}/_rels/{f}.rels"),
        None => format!("_rels/{name}.rels"),
    }
}

/// Part `to` relative to the directory `from`: `../slideLayouts/slideLayout2.xml` from
/// `ppt/slides`.
fn relative(from: &str, to: &str) -> String {
    let from: Vec<&str> = from.split('/').filter(|s| !s.is_empty()).collect();
    let to: Vec<&str> = to.split('/').collect();
    let common = from
        .iter()
        .zip(&to)
        .take_while(|(a, b)| a == b)
        .count()
        .min(to.len().saturating_sub(1));
    let mut out: Vec<&str> = vec![".."; from.len() - common];
    out.extend(&to[common..]);
    out.join("/")
}

/// A relationship in a relationships part, as written there.
struct Rel {
    id: String,
    kind: String,
    /// Resolved: a part name, or an external URL.
    target: String,
    external: bool,
    span: Range<usize>,
}

/// The relationships in `text`, the relationships part of `part`.
fn rels_of(text: &str, part: &str) -> io::Result<Vec<Rel>> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let root = parse(text, &rels_name(part))?;
    Ok(root
        .all("Relationship")
        .filter_map(|r| {
            let kind = r.attr("Type")?;
            let external = r.attr("TargetMode") == Some("External");
            let target = r.attr("Target")?;
            Some(Rel {
                id: r.attr("Id")?.to_owned(),
                kind: kind.rsplit('/').next().unwrap_or(kind).to_owned(),
                target: if external {
                    target.to_owned()
                } else {
                    resolve(dir(part), target)
                },
                external,
                span: r.span.clone(),
            })
        })
        .collect())
}

/// `<Relationship …/>` of kind `kind` (the type's last segment) to part `target`, written
/// relative to the directory `from`.
fn relationship(id: &str, kind: &str, from: &str, target: &str) -> String {
    format!(
        r#"<Relationship Id="{}" Type="{REL_NS}/{}" Target="{}"/>"#,
        escape(id),
        escape(kind),
        escape(&relative(from, target))
    )
}

/// Replace the `spans` of `text` (any order, not overlapping) with their strings. Where a
/// span starts where another is inserted (an empty span), the insertion comes first.
fn splice(text: &str, mut spans: Vec<(Range<usize>, String)>) -> String {
    spans.sort_by_key(|(r, _)| std::cmp::Reverse((r.start, r.end)));
    let mut out = text.to_owned();
    for (r, with) in spans {
        out.replace_range(r, &with);
    }
    out
}

/// Where the end tag of element `e` starts (its last `</`), for inserting before it.
fn end_tag(text: &str, e: &Element) -> io::Result<usize> {
    text[e.span.clone()]
        .rfind("</")
        .map(|i| e.span.start + i)
        .ok_or_else(|| damaged("an empty element where content was expected"))
}

/// The span of element `e`'s content, between its start and end tags.
fn inner(text: &str, e: &Element) -> io::Result<Range<usize>> {
    let raw = &text[e.span.clone()];
    let open = raw.find('>').ok_or_else(|| damaged("a broken tag"))? + 1;
    Ok(e.span.start + open..end_tag(text, e)?)
}

/// The namespace prefix element `e` is written with, with its colon (`p:` for
/// `<p:sldIdLst>`); empty without one.
fn prefix<'a>(text: &'a str, e: &Element) -> &'a str {
    tag_prefix(&text[e.span.clone()])
}

/// The namespace prefix of the element whose markup `raw` starts with, with its colon.
fn tag_prefix(raw: &str) -> &str {
    let tag = raw.strip_prefix('<').unwrap_or(raw);
    let name_end = tag
        .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
        .unwrap_or(tag.len());
    match tag[..name_end].find(':') {
        Some(i) => &tag[..=i],
        None => "",
    }
}

/// The prefix part `xml` (its root element `root`) declares for namespace `uri`, with its
/// colon; `fallback` when it declares none.
fn ns_prefix(xml: &str, root: &Element, uri: &str, fallback: &str) -> String {
    let raw = &xml[root.span.clone()];
    let open = &raw[..raw.find('>').unwrap_or(raw.len())];
    let found = [format!("\"{uri}\""), format!("'{uri}'")]
        .iter()
        .find_map(|quoted| open.find(quoted.as_str()))
        .and_then(|at| {
            let before = &open[..at];
            let decl = &before[before.rfind("xmlns")? + "xmlns".len()..];
            let decl = decl.trim_end().strip_suffix('=')?.trim_end();
            Some(match decl.strip_prefix(':') {
                Some(p) => format!("{p}:"),
                None => String::new(),
            })
        });
    found.unwrap_or_else(|| fallback.to_owned())
}

const DRAWINGML_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";

/// The span of attribute `name`'s value, its quotes included, in start tag `tag`.
fn attr_value(tag: &str, name: &str) -> Option<Range<usize>> {
    let b = tag.as_bytes();
    let mut i = tag.find(char::is_whitespace)?;
    loop {
        while b.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        let start = i;
        while b
            .get(i)
            .is_some_and(|c| *c != b'=' && !c.is_ascii_whitespace())
        {
            i += 1;
        }
        if i == start {
            return None;
        }
        let key = &tag[start..i];
        i += tag[i..].find('=')? + 1;
        while b.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        let quote = *b.get(i)? as char;
        let end = i + 1 + tag[i + 1..].find(quote)? + 1;
        if key == name {
            return Some(i..end);
        }
        i = end;
    }
}

/// Element `raw` (its markup as written) with attributes `attrs` set, its children named
/// in `remove` taken out, and `add` (each a child's local name and markup) put in where
/// `order`, the schema's child order (the names that may stand at each place), has them.
/// Everything else stays as written.
fn rebuild(
    raw: &str,
    order: &[&[&str]],
    attrs: &[(&str, String)],
    remove: &[&str],
    add: &[(&str, String)],
) -> io::Result<String> {
    let e = xml::parse(raw.as_bytes()).map_err(damaged)?;
    let open_end = raw.find('>').ok_or_else(|| damaged("a broken tag"))? + 1;
    let empty = raw[..open_end].ends_with("/>");
    let mut tag = raw[..open_end]
        .trim_end_matches('>')
        .trim_end_matches('/')
        .trim_end()
        .to_owned();
    for (name, value) in attrs {
        let quoted = format!("\"{}\"", escape(value));
        match attr_value(&tag, name) {
            Some(r) => tag.replace_range(r, &quoted),
            None => tag.push_str(&format!(" {name}={quoted}")),
        }
    }
    let content_end = if empty {
        open_end
    } else {
        raw.rfind("</").ok_or_else(|| damaged("a broken element"))?
    };
    let rank = |name: &str| {
        order
            .iter()
            .position(|g| g.contains(&name))
            .unwrap_or(order.len())
    };
    let mut pending: Vec<(usize, &str)> = add.iter().map(|(n, m)| (rank(n), m.as_str())).collect();
    pending.sort_by_key(|(r, _)| *r);
    let mut pending = pending.into_iter().peekable();
    let mut content = String::new();
    let mut cursor = open_end;
    for c in e.elements() {
        content.push_str(&raw[cursor..c.span.start]);
        cursor = c.span.end;
        if remove.contains(&c.name.as_str()) {
            continue;
        }
        let r = rank(&c.name);
        while let Some((_, m)) = pending.next_if(|(p, _)| *p < r) {
            content.push_str(m);
        }
        content.push_str(&raw[c.span.clone()]);
    }
    for (_, m) in pending {
        content.push_str(m);
    }
    content.push_str(&raw[cursor..content_end]);
    Ok(if empty && content.is_empty() {
        format!("{tag}/>")
    } else if empty {
        let name = &tag[1..tag.find(char::is_whitespace).unwrap_or(tag.len())];
        format!("{tag}>{content}</{name}>")
    } else {
        format!("{tag}>{content}{}", &raw[content_end..])
    })
}

/// DrawingML's fill elements, which stand at one place in their parents.
const FILLS: &[&str] = &[
    "noFill",
    "solidFill",
    "gradFill",
    "blipFill",
    "pattFill",
    "grpFill",
];

/// The children of run properties (`CT_TextCharacterProperties`), in order.
const RUN_ORDER: &[&[&str]] = &[
    &["ln"],
    FILLS,
    &["effectLst", "effectDag"],
    &["highlight"],
    &["uLnTx", "uLn"],
    &["uFillTx", "uFill"],
    &["latin"],
    &["ea"],
    &["cs"],
    &["sym"],
    &["hlinkClick"],
    &["hlinkMouseOver"],
    &["rtl"],
    &["extLst"],
];

/// A paragraph's bullet elements.
const BULLETS: &[&str] = &[
    "buClrTx",
    "buClr",
    "buSzTx",
    "buSzPct",
    "buSzPts",
    "buFontTx",
    "buFont",
    "buNone",
    "buAutoNum",
    "buChar",
    "buBlip",
];

/// The children of paragraph properties (`CT_TextParagraphProperties`), in order.
const PARAGRAPH_ORDER: &[&[&str]] = &[
    &["lnSpc"],
    &["spcBef"],
    &["spcAft"],
    &["buClrTx", "buClr"],
    &["buSzTx", "buSzPct", "buSzPts"],
    &["buFontTx", "buFont"],
    &["buNone", "buAutoNum", "buChar", "buBlip"],
    &["tabLst"],
    &["defRPr"],
    &["extLst"],
];

/// The children of shape properties (`CT_ShapeProperties`), in order.
const SHAPE_ORDER: &[&[&str]] = &[
    &["xfrm"],
    &["custGeom", "prstGeom"],
    FILLS,
    &["ln"],
    &["effectLst", "effectDag"],
    &["scene3d"],
    &["sp3d"],
    &["extLst"],
];

/// The children of a line (`CT_LineProperties`), in order.
const LINE_ORDER: &[&[&str]] = &[
    FILLS,
    &["prstDash", "custDash"],
    &["round", "bevel", "miter"],
    &["headEnd"],
    &["tailEnd"],
    &["extLst"],
];

/// `<a:srgbClr>` for `c`, with its alpha when it isn't opaque.
fn srgb(a: &str, c: Color) -> String {
    let hex = c.hex();
    if c.alpha == 255 {
        return format!(r#"<{a}srgbClr val="{hex}"/>"#);
    }
    let alpha = (u32::from(c.alpha) * 100_000 + 127) / 255;
    format!(r#"<{a}srgbClr val="{hex}"><{a}alpha val="{alpha}"/></{a}srgbClr>"#)
}

pub(crate) fn solid_fill(a: &str, c: Color) -> String {
    format!("<{a}solidFill>{}</{a}solidFill>", srgb(a, c))
}

fn on_off(v: bool) -> String {
    if v { "1" } else { "0" }.to_owned()
}

/// A run's (or a paragraph mark's) properties element under the name `name`: as read,
/// with the looks an edit set (`Source::Edit`) written on it.
fn run_props(r: &Run, name: &str, a: &str) -> io::Result<String> {
    let s = &r.sources;
    let looks = [
        s.font,
        s.size,
        s.bold,
        s.italic,
        s.underline,
        s.strike,
        s.color,
        s.baseline,
        s.caps,
    ];
    if !looks.contains(&Source::Edit) {
        return Ok(retag(&r.markup.props, name));
    }
    let raw = match &*r.markup.props {
        "" => format!("<{a}{name}/>"),
        props => retag(props, name),
    };
    let a = tag_prefix(&raw);
    let edited = |x: Source| x == Source::Edit;
    let mut attrs = Vec::new();
    if edited(s.size) {
        attrs.push(("sz", ((r.size * 100.0).round() as i64).to_string()));
    }
    if edited(s.bold) {
        attrs.push(("b", on_off(r.bold)));
    }
    if edited(s.italic) {
        attrs.push(("i", on_off(r.italic)));
    }
    if edited(s.underline) {
        attrs.push(("u", if r.underline { "sng" } else { "none" }.to_owned()));
    }
    if edited(s.strike) {
        let v = if r.strike { "sngStrike" } else { "noStrike" };
        attrs.push(("strike", v.to_owned()));
    }
    if edited(s.baseline) {
        attrs.push((
            "baseline",
            ((r.baseline * 1000.0).round() as i64).to_string(),
        ));
    }
    if edited(s.caps) {
        let v = match r.caps {
            presentation_model::Caps::All => "all",
            presentation_model::Caps::Small => "small",
            presentation_model::Caps::None => "none",
        };
        attrs.push(("cap", v.to_owned()));
    }
    let mut remove: Vec<&str> = Vec::new();
    let mut add = Vec::new();
    if edited(s.color) {
        remove.extend(FILLS);
        add.push(("solidFill", solid_fill(a, r.color)));
    }
    if edited(s.font) {
        remove.push("latin");
        add.push((
            "latin",
            format!(r#"<{a}latin typeface="{}"/>"#, escape(&r.font)),
        ));
    }
    rebuild(&raw, RUN_ORDER, &attrs, &remove, &add)
}

/// A bullet's elements, each with its name: `None` is no bullet.
fn bullet_markup(bullet: Option<&Bullet>, a: &str) -> Vec<(&'static str, String)> {
    let Some(b) = bullet else {
        return vec![("buNone", format!("<{a}buNone/>"))];
    };
    let mut out = Vec::new();
    out.push(match b.color {
        Some(c) => ("buClr", format!("<{a}buClr>{}</{a}buClr>", srgb(a, c))),
        None => ("buClrTx", format!("<{a}buClrTx/>")),
    });
    out.push(match b.size {
        BulletSize::FollowText => ("buSzTx", format!("<{a}buSzTx/>")),
        BulletSize::Percent(f) => (
            "buSzPct",
            format!(r#"<{a}buSzPct val="{}"/>"#, (f * 100_000.0).round() as i64),
        ),
        BulletSize::Points(p) => (
            "buSzPts",
            format!(r#"<{a}buSzPts val="{}"/>"#, (p * 100.0).round() as i64),
        ),
    });
    out.push(match &b.font {
        Some(f) => (
            "buFont",
            format!(r#"<{a}buFont typeface="{}"/>"#, escape(f)),
        ),
        None => ("buFontTx", format!("<{a}buFontTx/>")),
    });
    out.push(match &b.autonumber {
        Some(n) => {
            let start = match n.start {
                1 => String::new(),
                s => format!(r#" startAt="{s}""#),
            };
            (
                "buAutoNum",
                format!(r#"<{a}buAutoNum type="{}"{start}/>"#, escape(&n.scheme)),
            )
        }
        None => (
            "buChar",
            format!(r#"<{a}buChar char="{}"/>"#, escape(&b.text)),
        ),
    });
    out
}

/// A paragraph's properties element: as read, with the alignment and bullet an edit set
/// (`Source::Edit`) written on it, and its level when that isn't the one written.
fn paragraph_props(p: &Paragraph, a: &str) -> io::Result<String> {
    let align = p.sources.align == Source::Edit;
    let bullet = p.sources.bullet == Source::Edit;
    // The level the markup says, against the paragraph's (an outline edit changes it).
    let start = p
        .markup
        .find('>')
        .map_or(&*p.markup, |end| &p.markup[..=end]);
    let written = attr_value(start, "lvl")
        .and_then(|r| start[r].parse::<u8>().ok())
        .unwrap_or(0);
    let level = written != p.level;
    if !align && !bullet && !level {
        return Ok(p.markup.to_string());
    }
    let raw = match &*p.markup {
        "" => format!("<{a}pPr/>"),
        markup => markup.to_owned(),
    };
    let a = tag_prefix(&raw);
    let mut attrs = Vec::new();
    if level {
        attrs.push(("lvl", p.level.to_string()));
    }
    if align {
        let code = match p.align {
            Align::Left => "l",
            Align::Centre => "ctr",
            Align::Right => "r",
            Align::Justify => "just",
            Align::Distributed => "dist",
        };
        attrs.push(("algn", code.to_owned()));
    }
    let (remove, add) = if bullet {
        attrs.push(("marL", p.margin_left.to_string()));
        attrs.push(("indent", p.indent.to_string()));
        (BULLETS, bullet_markup(p.bullet.as_ref(), a))
    } else {
        (&[][..], Vec::new())
    };
    rebuild(&raw, PARAGRAPH_ORDER, &attrs, remove, &add)
}

/// The prefix of relationship attributes on element `e` (`r:` of `r:id`).
fn r_prefix<'a>(text: &'a str, e: &Element) -> &'a str {
    let tag = &text[e.span.clone()];
    tag.find(":id=")
        .and_then(|colon| {
            let start = tag[..colon].rfind(char::is_whitespace)? + 1;
            Some(&tag[start..=colon])
        })
        .unwrap_or("r:")
}

/// `raw`, a run properties element as read, under the name `name`: a paragraph mark's
/// `endParaRPr` becomes a typed run's `rPr`, and back.
fn retag(raw: &str, name: &str) -> String {
    let Some(rest) = raw.strip_prefix('<') else {
        return String::new();
    };
    let end = rest
        .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
        .unwrap_or(rest.len());
    let qualified = &rest[..end];
    let (pfx, local) = match qualified.split_once(':') {
        Some((p, l)) => (format!("{p}:"), l),
        None => (String::new(), qualified),
    };
    if local == name {
        return raw.to_owned();
    }
    let mut out = format!("<{pfx}{name}{}", &rest[end..]);
    let close = format!("</{pfx}{local}>");
    if out.ends_with(&close) {
        out.truncate(out.len() - close.len());
        out.push_str(&format!("</{pfx}{name}>"));
    }
    out
}

/// A field's end tag, from its start tag: `<a:fld …>` ends with `</a:fld>`.
fn field_close(start: &str) -> Option<String> {
    let rest = start.strip_prefix('<')?;
    let end = rest.find(|c: char| c.is_whitespace() || c == '>')?;
    Some(format!("</{}>", &rest[..end]))
}

/// A text body's paragraphs as DrawingML, `a` the DrawingML prefix with its colon.
pub(crate) fn paragraphs(body: &TextBody, a: &str) -> io::Result<String> {
    let mut out = String::new();
    for p in &body.paragraphs {
        paragraph(&mut out, p, a)?;
    }
    Ok(out)
}

fn paragraph(out: &mut String, p: &Paragraph, a: &str) -> io::Result<()> {
    out.push_str(&format!("<{a}p>"));
    out.push_str(&paragraph_props(p, a)?);
    for r in &p.runs {
        let props = run_props(r, "rPr", a)?;
        if r.text == "\n" {
            out.push_str(&format!("<{a}br>{props}</{a}br>"));
            continue;
        }
        let t = format!("<{a}t>{}</{a}t>", escape(&r.text));
        match field_close(&r.markup.field) {
            Some(close) => out.push_str(&format!("{}{props}{t}{close}", r.markup.field)),
            None => out.push_str(&format!("<{a}r>{props}{t}</{a}r>")),
        }
    }
    out.push_str(&run_props(&p.end, "endParaRPr", a)?);
    out.push_str(&format!("</{a}p>"));
    Ok(())
}

/// A shape's non-visual properties (`nvSpPr`, `nvPicPr`, ...).
fn nv_of(shape: &Element) -> Option<&Element> {
    shape
        .elements()
        .find(|c| c.name.starts_with("nv") && c.name.ends_with("Pr"))
}

/// A shape element's own id (`cNvPr id`).
fn own_id(e: &Element) -> Option<u32> {
    nv_of(e)
        .and_then(|nv| nv.child("cNvPr"))
        .and_then(|c| c.num::<u32>("id"))
}

/// The shape with id `id` in `tree`, groups searched.
fn find_shape(tree: &Element, id: u32) -> Option<&Element> {
    for e in tree.elements() {
        if e.name == "grpSp" {
            if let Some(found) = find_shape(e, id) {
                return Some(found);
            }
            continue;
        }
        if own_id(e) == Some(id) {
            return Some(e);
        }
    }
    None
}

/// The child of `tree` that is shape `id`, or the group that holds it.
fn top_level(tree: &Element, id: u32) -> Option<&Element> {
    tree.elements().find(|e| match e.name.as_str() {
        "grpSp" => find_shape(e, id).is_some(),
        _ => own_id(e) == Some(id),
    })
}

/// A shape's new fill and outline, by id; `None` for what stays.
type Look<'a> = (u32, Option<&'a Fill>, Option<Option<Outline>>);

/// What changed on a slide, by shape id: text bodies, boxes, fills and outlines, text
/// fits, placeholder slots, shapes gone, placeholders added, shapes inserted, and
/// placeholders a picture filled; the background, and the layout.
#[derive(Default)]
struct Edits<'a> {
    text: Vec<(u32, &'a TextBody)>,
    frames: Vec<(u32, Frame)>,
    looks: Vec<Look<'a>>,
    fits: Vec<(u32, &'a TextBody)>,
    slots: Vec<(u32, &'a PlaceholderSlot)>,
    removed: Vec<u32>,
    added: Vec<&'a Shape>,
    /// Shapes the base doesn't have, each with the id of the base's shape it goes in
    /// front of (`None`: behind everything).
    inserted: Vec<(&'a Shape, Option<u32>)>,
    /// Placeholders that became pictures, each with its box in the base.
    replaced: Vec<(&'a Shape, Frame)>,
    background: Option<Color>,
    /// The layout the slide moved to.
    layout: Option<usize>,
    /// The relationship each picture written shows its image through, by shape id.
    media: HashMap<u32, String>,
}

impl<'a> Edits<'a> {
    fn is_empty(&self) -> bool {
        self.text.is_empty()
            && self.frames.is_empty()
            && self.looks.is_empty()
            && self.fits.is_empty()
            && !self.has_structure()
    }

    /// Whether shapes come or go, placeholders change slots, or the background changes.
    fn has_structure(&self) -> bool {
        !self.slots.is_empty()
            || !self.removed.is_empty()
            || !self.added.is_empty()
            || !self.inserted.is_empty()
            || !self.replaced.is_empty()
            || self.background.is_some()
    }

    /// The shapes written whole: those inserted, placeholders filled with a picture, and
    /// placeholders added as pictures.
    fn whole(&self) -> impl Iterator<Item = &'a Shape> + '_ {
        let added = self
            .added
            .iter()
            .copied()
            .filter(|s| matches!(s.kind, ShapeKind::Picture(_)));
        self.inserted
            .iter()
            .map(|(s, _)| *s)
            .chain(self.replaced.iter().map(|(s, _)| *s))
            .chain(added)
    }

    /// The pictures written, by shape id: each needs its image in the package.
    fn pictures(&self) -> Vec<(u32, &'a Image)> {
        self.whole()
            .filter_map(|s| match &s.kind {
                ShapeKind::Picture(p) => Some((s.id?, &p.image)),
                _ => None,
            })
            .collect()
    }

    /// What writing these edits wrote, given the shapes whose looks it wrote.
    fn wrote(&self, looks: Vec<u32>) -> Wrote {
        Wrote {
            text: self.text.iter().map(|(id, _)| *id).collect(),
            frames: self.frames.iter().map(|(id, _)| *id).collect(),
            looks,
            shapes: self.whole().filter_map(|s| s.id).collect(),
            fits: self.fits.iter().map(|(id, _)| *id).collect(),
            removed: self.removed.clone(),
            background: self.background.is_some(),
            layout: self.layout.is_some(),
        }
    }
}

/// What differs between `now` and `base`, its shapes matched by id. Only the slide's own
/// shapes outside groups have boxes a save writes; fills, outlines, and backgrounds are
/// written when an edit set them (`Source::Edit`) to what the base doesn't have on itself.
/// Shapes the base doesn't have (but placeholders a layout change added) are written
/// whole, as are placeholders that became pictures. `template` is the new slide of the
/// layout `now` moved to, when it moved: placeholders the base doesn't have are compared
/// with its.
fn edits<'a>(base: &'a Slide, now: &'a Slide, template: Option<&'a Slide>) -> Edits<'a> {
    let mut out = Edits::default();
    // The last shape so far the base has: where the next one inserted goes.
    let mut behind = None;
    for s in now.shapes.iter().filter(|s| s.layer == Layer::Slide) {
        let Some(id) = s.id else { continue };
        let old = match base.shapes.iter().find(|b| b.id == Some(id)) {
            Some(o) => {
                behind = Some(id);
                let filled = matches!(o.kind, ShapeKind::Geometry { .. })
                    && matches!(s.kind, ShapeKind::Picture(_));
                if filled && !s.grouped {
                    out.replaced.push((s, o.frame));
                    continue;
                }
                if let Some(slot) = s.slot.as_ref().filter(|slot| o.slot.as_ref() != Some(slot)) {
                    out.slots.push((id, slot));
                }
                Some(o)
            }
            None => {
                if s.slot.is_some() && !s.grouped {
                    out.added.push(s);
                } else if !s.grouped {
                    out.inserted.push((s, behind));
                    continue;
                }
                template.and_then(|t| {
                    t.shapes
                        .iter()
                        .find(|t| t.layer == Layer::Slide && t.slot.is_some() && t.slot == s.slot)
                })
            }
        };
        if let Some(body) = &s.text {
            let old_body = old.and_then(|o| o.text.as_ref());
            if old_body.map(|o| &o.paragraphs) != Some(&body.paragraphs) {
                out.text.push((id, body));
            }
            let fit = |t: &TextBody| (t.font_scale, t.line_reduction, t.autofit);
            if old_body.map_or((1.0, 0.0, Autofit::Unset), fit) != fit(body) {
                out.fits.push((id, body));
            }
        }
        if !s.grouped && old.map(|o| o.frame) != Some(s.frame) {
            out.frames.push((id, s.frame));
        }
        let fill = (s.sources.fill == Source::Edit
            && matches!(s.fill, Fill::None | Fill::Solid(_))
            && old.is_none_or(|o| o.fill != s.fill || !o.sources.fill.is_own()))
        .then_some(&s.fill);
        let outline = (s.sources.outline == Source::Edit
            && old.is_none_or(|o| o.outline != s.outline || !o.sources.outline.is_own()))
        .then_some(s.outline);
        if fill.is_some() || outline.is_some() {
            out.looks.push((id, fill, outline));
        }
    }
    out.removed = base
        .shapes
        .iter()
        .filter(|b| b.layer == Layer::Slide)
        .filter_map(|b| b.id)
        .filter(|id| !now.shapes.iter().any(|s| s.id == Some(*id)))
        .collect();
    if let Background::Solid(c) = now.background {
        if now.background_source == Source::Edit
            && (now.background != base.background || !base.background_source.is_own())
        {
            out.background = Some(c);
        }
    }
    out.layout = now.layout.filter(|_| now.layout != base.layout);
    out
}

/// Slide part `xml` (named `name`) with the `edits` written into it, and the ids of the
/// shapes whose looks it wrote. Shapes come and go first, so text and boxes find them;
/// fills and outlines last, as they rewrite the shape properties boxes are written in.
fn patch_slide(xml: &str, name: &str, edits: &Edits<'_>) -> io::Result<(String, Vec<u32>)> {
    let mut xml = xml.to_owned();
    if edits.has_structure() {
        xml = patch_structure(&xml, name, edits)?;
    }
    if !edits.text.is_empty() || !edits.frames.is_empty() || !edits.fits.is_empty() {
        xml = patch_content(&xml, name, edits)?;
    }
    let mut looks = Vec::new();
    if !edits.looks.is_empty() {
        (xml, looks) = patch_looks(&xml, name, edits)?;
    }
    Ok((xml, looks))
}

/// A slide part's root and its shape tree.
fn shape_tree<'a>(root: &'a Element, name: &str) -> io::Result<&'a Element> {
    root.path(&["cSld", "spTree"])
        .ok_or_else(|| damaged(format!("{name} has no shapes")))
}

/// `<p:ph>` for `slot`: its type (none for `obj`) and index.
pub(crate) fn ph_tag(p: &str, slot: &PlaceholderSlot) -> String {
    let kind = match &*slot.kind {
        "obj" => String::new(),
        k => format!(r#" type="{}""#, escape(k)),
    };
    let idx = slot
        .idx
        .map(|i| format!(r#" idx="{i}""#))
        .unwrap_or_default();
    format!("<{p}ph{kind}{idx}/>")
}

/// Shapes gone, placeholders added, shapes inserted, placeholders a picture filled,
/// placeholder slots, and the background.
fn patch_structure(xml: &str, name: &str, edits: &Edits<'_>) -> io::Result<String> {
    let root = parse(xml, name)?;
    let tree = shape_tree(&root, name)?;
    let p = prefix(xml, tree);
    let a = ns_prefix(xml, &root, DRAWINGML_NS, "a:");
    let r = ns_prefix(xml, &root, REL_NS, "");
    let ns = Prefixes {
        p: p.to_owned(),
        a: a.clone(),
        r_decl: match r.as_str() {
            "" => format!(r#" xmlns:r="{REL_NS}""#),
            _ => String::new(),
        },
        r: match r.as_str() {
            "" => "r:".to_owned(),
            _ => r,
        },
    };
    let rid = |s: &Shape| {
        s.id.and_then(|id| edits.media.get(&id))
            .map_or("", String::as_str)
    };
    let mut spans = Vec::new();
    for id in &edits.removed {
        if let Some(s) = find_shape(tree, *id) {
            spans.push((s.span.clone(), String::new()));
        }
    }
    for (s, base_frame) in &edits.replaced {
        let id = s.id.unwrap_or_default();
        let ShapeKind::Picture(pic) = &s.kind else {
            continue;
        };
        let old =
            find_shape(tree, id).ok_or_else(|| damaged(format!("{name} has no shape {id}")))?;
        let own_box = old.child("spPr").and_then(|p| p.child("xfrm")).is_some();
        let placeholder = s
            .slot
            .as_ref()
            .map(|slot| (slot, own_box || s.frame != *base_frame));
        let with = shapes::picture(s, pic.crop, &ns, rid(s), placeholder)?;
        spans.push((old.span.clone(), with));
    }
    for (id, slot) in &edits.slots {
        let nv_pr = find_shape(tree, *id)
            .and_then(nv_of)
            .and_then(|nv| nv.child("nvPr"))
            .ok_or_else(|| damaged(format!("{name} has no shape {id}")))?;
        match nv_pr.child("ph") {
            Some(ph) => spans.push((ph.span.clone(), ph_tag(prefix(xml, ph), slot))),
            None => {
                let raw = &xml[nv_pr.span.clone()];
                let ph = ph_tag(prefix(xml, nv_pr), slot);
                let with = rebuild(raw, &[&["ph"]], &[], &[], &[("ph", ph)])?;
                spans.push((nv_pr.span.clone(), with));
            }
        }
    }
    // What goes in where: placeholders added at the back, each inserted shape in front of
    // the base's shape it follows; shapes inserted at one place, in their order.
    let start = match tree.child("grpSpPr").or_else(|| tree.child("nvGrpSpPr")) {
        Some(e) => e.span.end,
        None => inner(xml, tree)?.start,
    };
    let mut inserts: BTreeMap<usize, String> = BTreeMap::new();
    for s in &edits.added {
        let markup = match (&s.kind, &s.slot) {
            (ShapeKind::Picture(pic), Some(slot)) => {
                shapes::picture(s, pic.crop, &ns, rid(s), Some((slot, false)))?
            }
            _ => {
                let ph = s
                    .slot
                    .as_ref()
                    .map(|slot| ph_tag(p, slot))
                    .unwrap_or_default();
                format!(
                    concat!(
                        r#"<{p}sp><{p}nvSpPr><{p}cNvPr id="{id}" name="{name}"/><{p}cNvSpPr>"#,
                        r#"<{a}spLocks noGrp="1"/></{p}cNvSpPr><{p}nvPr>{ph}</{p}nvPr></{p}nvSpPr>"#,
                        r#"<{p}spPr/><{p}txBody><{a}bodyPr/><{a}lstStyle/><{a}p/></{p}txBody></{p}sp>"#,
                    ),
                    p = p,
                    a = a,
                    id = s.id.unwrap_or_default(),
                    name = escape(&s.name),
                    ph = ph,
                )
            }
        };
        inserts.entry(start).or_default().push_str(&markup);
    }
    for (s, behind) in &edits.inserted {
        let at = behind
            .and_then(|id| top_level(tree, id))
            .map_or(start, |e| e.span.end);
        let markup = shapes::shape(s, &ns, rid(s))?;
        inserts.entry(at).or_default().push_str(&markup);
    }
    spans.extend(inserts.into_iter().map(|(at, markup)| (at..at, markup)));
    if let Some(c) = edits.background {
        let c_sld = root
            .child("cSld")
            .ok_or_else(|| damaged(format!("{name} has no shapes")))?;
        let pc = prefix(xml, c_sld);
        let bg = format!(
            "<{pc}bg><{pc}bgPr>{}<{a}effectLst/></{pc}bgPr></{pc}bg>",
            solid_fill(&a, c)
        );
        match c_sld.child("bg") {
            Some(old) => spans.push((old.span.clone(), bg)),
            None => {
                let at = inner(xml, c_sld)?.start;
                spans.push((at..at, bg));
            }
        }
    }
    Ok(splice(xml, spans))
}

/// The children of a text body's properties (`CT_TextBodyProperties`), in order.
const BODY_ORDER: &[&[&str]] = &[
    &["prstTxWarp"],
    AUTOFITS,
    &["scene3d"],
    &["sp3d"],
    &["flatTx"],
    &["extLst"],
];

/// A text body's autofit elements, which stand at one place in its properties.
const AUTOFITS: &[&str] = &["noAutofit", "normAutofit", "spAutoFit"];

/// Text, text fits, and boxes.
fn patch_content(xml: &str, name: &str, edits: &Edits<'_>) -> io::Result<String> {
    let root = parse(xml, name)?;
    let tree = shape_tree(&root, name)?;
    let fallback = ns_prefix(xml, &root, DRAWINGML_NS, "a:");
    let text_of = |id: u32| {
        find_shape(tree, id)
            .and_then(|s| s.child("txBody"))
            .ok_or_else(|| damaged(format!("{name} has no text shape {id}")))
    };
    let mut spans = Vec::new();
    for (id, body) in &edits.text {
        let tx = text_of(*id)?;
        let a = tx.child("bodyPr").map_or("a:", |b| prefix(xml, b));
        let range = match (tx.all("p").next(), tx.all("p").last()) {
            (Some(first), Some(last)) => first.span.start..last.span.end,
            _ => {
                let at = end_tag(xml, tx)?;
                at..at
            }
        };
        spans.push((range, paragraphs(body, a)?));
    }
    for (id, body) in &edits.fits {
        let tx = text_of(*id)?;
        match tx.child("bodyPr") {
            Some(b) => {
                let fit = shapes::autofit(body, prefix(xml, b));
                let add: Vec<(&str, String)> = match fit.is_empty() {
                    true => Vec::new(),
                    false => vec![("normAutofit", fit)],
                };
                let with = rebuild(&xml[b.span.clone()], BODY_ORDER, &[], AUTOFITS, &add)?;
                spans.push((b.span.clone(), with));
            }
            None => {
                let a = fallback.as_str();
                let at = inner(xml, tx)?.start;
                let fit = shapes::autofit(body, a);
                spans.push((at..at, format!("<{a}bodyPr>{fit}</{a}bodyPr>")));
            }
        }
    }
    for (id, frame) in &edits.frames {
        let shape =
            find_shape(tree, *id).ok_or_else(|| damaged(format!("{name} has no shape {id}")))?;
        spans.extend(frame_spans(xml, shape, frame)?);
    }
    Ok(splice(xml, spans))
}

/// Line element `raw` (`<a:ln>`, a table cell's `<a:lnL>`, ...) as written, with the
/// width, colour, and dash of `outline`, or no line for `None`.
pub(crate) fn outline_markup(raw: &str, outline: Option<Outline>) -> io::Result<String> {
    let la = tag_prefix(raw).to_owned();
    match outline {
        Some(o) => {
            let dash = match o.dash {
                Dash::Solid => "solid",
                Dash::Dot => "sysDot",
                Dash::Dash => "dash",
                Dash::DashDot => "dashDot",
                Dash::LongDash => "lgDash",
            };
            let mut gone = FILLS.to_vec();
            gone.extend(["prstDash", "custDash"]);
            rebuild(
                raw,
                LINE_ORDER,
                &[("w", o.width.to_string())],
                &gone,
                &[
                    ("solidFill", solid_fill(&la, o.color)),
                    ("prstDash", format!(r#"<{la}prstDash val="{dash}"/>"#)),
                ],
            )
        }
        None => rebuild(
            raw,
            LINE_ORDER,
            &[],
            FILLS,
            &[("noFill", format!("<{la}noFill/>"))],
        ),
    }
}

/// Fills and outlines, in each shape's `spPr`; graphic frames, which have none, are left
/// as they are. Gives the ids of the shapes written.
fn patch_looks(xml: &str, name: &str, edits: &Edits<'_>) -> io::Result<(String, Vec<u32>)> {
    let root = parse(xml, name)?;
    let tree = shape_tree(&root, name)?;
    let fallback = ns_prefix(xml, &root, DRAWINGML_NS, "a:");
    let mut spans = Vec::new();
    let mut ids = Vec::new();
    for (id, fill, outline) in &edits.looks {
        let shape =
            find_shape(tree, *id).ok_or_else(|| damaged(format!("{name} has no shape {id}")))?;
        let Some(sp_pr) = shape.child("spPr") else {
            continue;
        };
        let a = sp_pr
            .elements()
            .next()
            .map_or(fallback.as_str(), |c| prefix(xml, c));
        let mut remove: Vec<&str> = Vec::new();
        let mut add = Vec::new();
        if let Some(fill) = fill {
            remove.extend(FILLS);
            add.push(match fill {
                Fill::Solid(c) => ("solidFill", solid_fill(a, *c)),
                _ => ("noFill", format!("<{a}noFill/>")),
            });
        }
        if let Some(outline) = outline {
            let old = sp_pr.child("ln");
            let raw = old.map_or_else(|| format!("<{a}ln/>"), |l| xml[l.span.clone()].to_owned());
            remove.push("ln");
            add.push(("ln", outline_markup(&raw, *outline)?));
        }
        let with = rebuild(&xml[sp_pr.span.clone()], SHAPE_ORDER, &[], &remove, &add)?;
        spans.push((sp_pr.span.clone(), with));
        ids.push(*id);
    }
    Ok((splice(xml, spans), ids))
}

/// Where `shape`'s box is written (`<a:off>` and `<a:ext>` in its `xfrm`), rewritten for
/// `frame`; a shape without an `xfrm` of its own (a placeholder that took its layout's)
/// gets one, first in its `spPr`. Rotation and flips stay as written.
fn frame_spans(
    xml: &str,
    shape: &Element,
    frame: &Frame,
) -> io::Result<Vec<(Range<usize>, String)>> {
    let (props, xfrm) = if shape.name == "graphicFrame" {
        (None, shape.child("xfrm"))
    } else {
        let props = shape
            .child("spPr")
            .ok_or_else(|| damaged("a shape without shape properties"))?;
        (Some(props), props.child("xfrm"))
    };
    let off = |a: &str| format!(r#"<{a}off x="{}" y="{}"/>"#, frame.x, frame.y);
    let ext = |a: &str| format!(r#"<{a}ext cx="{}" cy="{}"/>"#, frame.cx, frame.cy);
    if let Some(x) = xfrm {
        let a = x.child("off").map_or("a:", |o| prefix(xml, o));
        return match (x.child("off"), x.child("ext")) {
            (Some(o), Some(e)) => Ok(vec![(o.span.clone(), off(a)), (e.span.clone(), ext(a))]),
            _ => {
                let p = prefix(xml, x);
                let open = &xml[x.span.start..x.span.end];
                let attrs_end = open.find('>').unwrap_or(open.len());
                let tag = open[..attrs_end].trim_end_matches('/');
                Ok(vec![(
                    x.span.clone(),
                    format!("{tag}>{}{}</{p}xfrm>", off(a), ext(a)),
                )])
            }
        };
    }
    let props = props.ok_or_else(|| damaged("a graphic frame without its frame"))?;
    let p = prefix(xml, props);
    let new = format!("<a:xfrm>{}{}</a:xfrm>", off("a:"), ext("a:"));
    let raw = &xml[props.span.clone()];
    if raw.ends_with("/>") {
        return Ok(vec![(
            props.span.clone(),
            format!("<{p}spPr>{new}</{p}spPr>"),
        )]);
    }
    let at = props.span.start + raw.find('>').ok_or_else(|| damaged("a broken tag"))? + 1;
    Ok(vec![(at..at, new)])
}

/// What becomes of each of the deck's slides.
enum Plan<'a> {
    /// Its part, patched with its edits.
    Keep { part: String, base: &'a Slide },
    /// A new part copied from `from` (a duplicate), patched with its edits.
    Copy {
        from: String,
        part: String,
        base: &'a Slide,
    },
    /// A new part from layout `layout`'s new slide, with its text.
    New { layout: usize, part: String },
}

/// Part names not taken yet: `{dir}/{stem}{n}.{ext}`, counting up, `n` unused with any
/// extension.
struct Names(HashSet<String>);

impl Names {
    fn next(&mut self, dir: &str, stem: &str) -> String {
        self.next_with(dir, stem, "xml")
    }

    fn next_with(&mut self, dir: &str, stem: &str, ext: &str) -> String {
        let n = (1..)
            .find(|n| {
                let taken = format!("{dir}/{stem}{n}.");
                !self.0.iter().any(|name| name.starts_with(&taken))
            })
            .expect("a free name");
        let name = format!("{dir}/{stem}{n}.{ext}");
        self.0.insert(name.clone());
        name
    }
}

/// The next `rId{n}` not among `ids`.
fn next_rid(ids: &mut HashSet<String>) -> String {
    let id = (1..)
        .map(|n| format!("rId{n}"))
        .find(|id| !ids.contains(id))
        .expect("a free id");
    ids.insert(id.clone());
    id
}

/// What the new package holds: parts replaced (`Some` text) or left out (`None`), new
/// parts with their content types, and new media parts with the content types of their
/// extensions.
#[derive(Default)]
struct Changes {
    parts: HashMap<String, Option<String>>,
    added: Vec<(String, String)>,
    types_added: Vec<(String, &'static str)>,
    types_removed: HashSet<String>,
    media: Vec<(String, Arc<[u8]>)>,
    defaults: Vec<(&'static str, &'static str)>,
}

impl Changes {
    fn remove(&mut self, part: &str) {
        self.parts.insert(part.to_owned(), None);
    }

    fn add(&mut self, part: &str, text: String, kind: Option<&'static str>) {
        self.added.push((part.to_owned(), text));
        if let Some(kind) = kind {
            self.types_added.push((part.to_owned(), kind));
        }
    }
}

/// Slide `part`'s relationships `rels` (as read; empty without) with one to a new media
/// part for each picture `edits` writes, whose ids go in `edits.media`.
fn add_media(
    rels: &str,
    part: &str,
    edits: &mut Edits<'_>,
    media_dir: &str,
    names: &mut Names,
    changes: &mut Changes,
) -> io::Result<String> {
    let pictures = edits.pictures();
    if pictures.is_empty() {
        return Ok(rels.to_owned());
    }
    let mut ids: HashSet<String> = rels_of(rels, part)?.into_iter().map(|r| r.id).collect();
    let mut new = Vec::with_capacity(pictures.len());
    for (id, image) in pictures {
        let (ext, kind) = shapes::media_type(image.format);
        let media = names.next_with(media_dir, "image", ext);
        let rid = next_rid(&mut ids);
        new.push(relationship(&rid, "image", dir(part), &media));
        changes.media.push((media, image.bytes.clone()));
        if !changes.defaults.iter().any(|(e, _)| *e == ext) {
            changes.defaults.push((ext, kind));
        }
        edits.media.insert(id, rid);
    }
    add_relationships(rels, part, &new)
}

fn write(
    out: &mut (dyn WriteSeek + Send),
    source: &Path,
    read: &Presentation,
    deck: &Presentation,
) -> io::Result<Written> {
    let mut src = ZipArchive::new(BufReader::new(File::open(source)?)).map_err(zip_error)?;
    let entries: Vec<String> = src.file_names().map(str::to_owned).collect();
    let mut names = Names(entries.iter().cloned().collect());

    let package_rels = read_text(&mut src, "_rels/.rels")?.unwrap_or_default();
    let main = rels_of(&package_rels, "")?
        .into_iter()
        .find(|r| r.kind == "officeDocument" && !r.external)
        .map_or_else(|| "ppt/presentation.xml".to_owned(), |r| r.target);
    let main_dir = dir(&main).to_owned();
    let pres_xml = read_text(&mut src, &main)?.ok_or_else(|| damaged(format!("no {main}")))?;
    let pres = parse(&pres_xml, &main)?;
    let pres_rels_name = rels_name(&main);
    let pres_rels_xml = read_text(&mut src, &pres_rels_name)?.unwrap_or_default();
    let pres_rels = rels_of(&pres_rels_xml, &main)?;
    let mut rids: HashSet<String> = pres_rels.iter().map(|r| r.id.clone()).collect();

    // The slide list as read: each slide's `<p:sldId>` and id, by part.
    let list = pres.child("sldIdLst");
    let mut sld_ids: HashMap<String, (Range<usize>, u32)> = HashMap::new();
    let mut max_id = 255u32;
    for s in list.into_iter().flat_map(|l| l.all("sldId")) {
        let id = s.num::<u32>("id").unwrap_or(0);
        max_id = max_id.max(id);
        let rel = s
            .rel_attr("id")
            .and_then(|rid| pres_rels.iter().find(|r| r.id == rid));
        if let Some(rel) = rel {
            sld_ids.insert(rel.target.clone(), (s.span.clone(), id));
        }
    }
    let slides_dir = sld_ids
        .keys()
        .next()
        .map_or_else(|| format!("{main_dir}/slides"), |p| dir(p).to_owned());

    // What becomes of each slide: the first slide on a part keeps it, a later one (a
    // duplicate) gets a copy, a slide without one is new.
    let base_by_part: HashMap<&str, &Slide> = read
        .slides
        .iter()
        .filter_map(|s| Some((s.part.as_deref()?, s)))
        .collect();
    let mut used: HashSet<&str> = HashSet::new();
    let mut plans = Vec::with_capacity(deck.slides.len());
    for slide in &deck.slides {
        let from = slide
            .part
            .as_deref()
            .filter(|p| base_by_part.contains_key(p) && sld_ids.contains_key(*p));
        plans.push(match from {
            Some(p) if used.insert(p) => Plan::Keep {
                part: p.to_owned(),
                base: base_by_part[p],
            },
            Some(p) => Plan::Copy {
                from: p.to_owned(),
                part: names.next(&slides_dir, "slide"),
                base: base_by_part[p],
            },
            None => Plan::New {
                layout: slide
                    .layout
                    .filter(|&l| l < deck.layouts.len())
                    .ok_or_else(|| damaged("a new slide without a layout"))?,
                part: names.next(&slides_dir, "slide"),
            },
        });
    }

    let mut changes = Changes::default();
    let mut written: Written = Vec::with_capacity(deck.slides.len());

    // Deleted slides: their parts, relationships, and notes go.
    let deleted: Vec<String> = sld_ids
        .keys()
        .filter(|p| !used.contains(p.as_str()))
        .cloned()
        .collect();
    for part in &deleted {
        changes.remove(part);
        changes.remove(&rels_name(part));
        changes.types_removed.insert(part.clone());
        let rels = read_text(&mut src, &rels_name(part))?.unwrap_or_default();
        for r in rels_of(&rels, part)? {
            if r.kind == "notesSlide" && !r.external {
                changes.remove(&r.target);
                changes.remove(&rels_name(&r.target));
                changes.types_removed.insert(r.target);
            }
        }
    }

    // The new slide of the layout a slide moved to.
    let moved_to = |base: &Slide, slide: &Slide| {
        slide
            .layout
            .filter(|_| slide.layout != base.layout)
            .and_then(|l| deck.layouts.get(l))
            .map(|l| &l.slide)
    };
    let media_dir = format!("{main_dir}/media");
    for (plan, slide) in plans.iter().zip(&deck.slides) {
        match plan {
            Plan::Keep { part, base } => {
                let mut ed = edits(base, slide, moved_to(base, slide));
                if ed.layout.is_some() || !ed.pictures().is_empty() {
                    let name = rels_name(part);
                    let rels = read_text(&mut src, &name)?;
                    let old = rels.as_deref().unwrap_or("");
                    let mut text =
                        add_media(old, part, &mut ed, &media_dir, &mut names, &mut changes)?;
                    if let Some(l) = ed.layout {
                        text = retarget_layout(&text, part, &deck.layouts[l].part)?;
                    }
                    if rels.is_some() {
                        changes.parts.insert(name, Some(text));
                    } else {
                        changes.add(&name, text, None);
                    }
                }
                let mut looks = Vec::new();
                if !ed.is_empty() {
                    let xml = read_text(&mut src, part)?.unwrap_or_default();
                    let patched;
                    (patched, looks) = patch_slide(&xml, part, &ed)?;
                    changes.parts.insert(part.clone(), Some(patched));
                }
                written.push(ed.wrote(looks));
            }
            Plan::Copy { from, part, base } => {
                let mut ed = edits(base, slide, moved_to(base, slide));
                let rels = read_text(&mut src, &rels_name(from))?.unwrap_or_default();
                let rels = copy_rels(&mut src, &rels, from, part, &mut names, &mut changes)?;
                let mut rels =
                    add_media(&rels, part, &mut ed, &media_dir, &mut names, &mut changes)?;
                if let Some(l) = ed.layout {
                    rels = retarget_layout(&rels, part, &deck.layouts[l].part)?;
                }
                changes.add(&rels_name(part), rels, None);
                let xml = read_text(&mut src, from)?.unwrap_or_default();
                let (patched, looks) = patch_slide(&xml, part, &ed)?;
                changes.add(part, patched, Some(SLIDE_TYPE));
                written.push(ed.wrote(looks));
            }
            Plan::New { layout, part } => {
                let layout = &deck.layouts[*layout];
                let layout_xml = read_text(&mut src, &layout.part)?
                    .ok_or_else(|| damaged(format!("no {}", layout.part)))?;
                let blank = blank_slide(&parse(&layout_xml, &layout.part)?);
                let base = &read
                    .layouts
                    .iter()
                    .find(|l| l.part == layout.part)
                    .unwrap_or(layout)
                    .slide;
                let mut ed = edits(base, slide, None);
                let rels = format!(
                    r#"{XML_DECLARATION}<Relationships xmlns="{RELS_NS}">{}</Relationships>"#,
                    relationship("rId1", "slideLayout", dir(part), &layout.part)
                );
                let rels = add_media(&rels, part, &mut ed, &media_dir, &mut names, &mut changes)?;
                changes.add(&rels_name(part), rels, None);
                let (patched, looks) = patch_slide(&blank, part, &ed)?;
                changes.add(part, patched, Some(SLIDE_TYPE));
                written.push(ed.wrote(looks));
            }
        }
    }

    // presentation.xml: the slide list, with ids for new slides; and its sections.
    let p = list.map_or("p:", |l| prefix(&pres_xml, l));
    let r = list
        .and_then(|l| l.elements().next())
        .map_or("r:", |s| r_prefix(&pres_xml, s));
    let mut new_rels = String::new();
    let mut slide_list = String::new();
    let mut slide_ids = Vec::with_capacity(plans.len());
    for plan in &plans {
        match plan {
            Plan::Keep { part, .. } => {
                let (span, id) = &sld_ids[part];
                slide_list.push_str(&pres_xml[span.clone()]);
                slide_ids.push(*id);
            }
            Plan::Copy { part, .. } | Plan::New { part, .. } => {
                max_id += 1;
                let rid = next_rid(&mut rids);
                slide_list.push_str(&format!(r#"<{p}sldId id="{max_id}" {r}id="{rid}"/>"#));
                new_rels.push_str(&relationship(&rid, "slide", &main_dir, part));
                slide_ids.push(max_id);
            }
        }
    }
    let whole_list = format!("<{p}sldIdLst>{slide_list}</{p}sldIdLst>");
    let mut pres_spans = Vec::new();
    match list {
        Some(l) if pres_xml[l.span.clone()].ends_with("/>") => {
            pres_spans.push((l.span.clone(), whole_list))
        }
        Some(l) => pres_spans.push((inner(&pres_xml, l)?, slide_list)),
        None => {
            let at = pres
                .child("sldSz")
                .map(|s| s.span.start)
                .ok_or_else(|| damaged(format!("{main} has no slide size")))?;
            pres_spans.push((at..at, whole_list));
        }
    }
    pres_spans.extend(sections(&pres_xml, &pres, &plans, &slide_ids, &sld_ids));
    changes
        .parts
        .insert(main.clone(), Some(splice(&pres_xml, pres_spans)));

    // Its relationships: deleted slides' go, new slides' come.
    let mut rel_spans: Vec<(Range<usize>, String)> = pres_rels
        .iter()
        .filter(|r| r.kind == "slide" && !r.external && deleted.contains(&r.target))
        .map(|r| (r.span.clone(), String::new()))
        .collect();
    if !new_rels.is_empty() {
        let root = parse(&pres_rels_xml, &pres_rels_name)?;
        let at = end_tag(&pres_rels_xml, &root)?;
        rel_spans.push((at..at, new_rels));
    }
    let patched_rels = splice(&pres_rels_xml, rel_spans);
    changes.parts.insert(pres_rels_name, Some(patched_rels));

    // Content types.
    let types_xml = read_text(&mut src, "[Content_Types].xml")?
        .ok_or_else(|| damaged("no [Content_Types].xml"))?;
    let types = parse(&types_xml, "[Content_Types].xml")?;
    let mut type_spans: Vec<(Range<usize>, String)> = types
        .all("Override")
        .filter(|o| {
            o.attr("PartName")
                .is_some_and(|n| changes.types_removed.contains(n.trim_start_matches('/')))
        })
        .map(|o| (o.span.clone(), String::new()))
        .collect();
    // A media part's type is its extension's, which the package may not have yet.
    let has_default = |ext: &str| {
        types.all("Default").any(|d| {
            d.attr("Extension")
                .is_some_and(|e| e.eq_ignore_ascii_case(ext))
        })
    };
    let defaults = changes
        .defaults
        .iter()
        .filter(|(ext, _)| !has_default(ext))
        .map(|(ext, kind)| format!(r#"<Default Extension="{ext}" ContentType="{kind}"/>"#));
    let overrides = changes.types_added.iter().map(|(part, kind)| {
        format!(
            r#"<Override PartName="/{}" ContentType="{kind}"/>"#,
            escape(part)
        )
    });
    let new_types: String = defaults.chain(overrides).collect();
    let at = end_tag(&types_xml, &types)?;
    type_spans.push((at..at, new_types));
    let patched_types = splice(&types_xml, type_spans);
    changes
        .parts
        .insert("[Content_Types].xml".to_owned(), Some(patched_types));

    // The package: every part as it was but those changed, then the new ones.
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut zip = ZipWriter::new(out);
    for (i, name) in entries.iter().enumerate() {
        match changes.parts.get(name) {
            Some(None) => {}
            Some(Some(text)) => {
                zip.start_file(name, options).map_err(zip_error)?;
                zip.write_all(text.as_bytes())?;
            }
            None => {
                let file = src.by_index_raw(i).map_err(zip_error)?;
                zip.raw_copy_file(file).map_err(zip_error)?;
            }
        }
    }
    for (name, text) in &changes.added {
        zip.start_file(name, options).map_err(zip_error)?;
        zip.write_all(text.as_bytes())?;
    }
    for (name, bytes) in &changes.media {
        zip.start_file(name, options).map_err(zip_error)?;
        zip.write_all(bytes)?;
    }
    zip.finish().map_err(zip_error)?;
    Ok(written)
}

/// The sections of `presentation.xml` (`p14:sectionLst`), each listing its slides' ids, in
/// order and without gaps, as PowerPoint needs them. The most slides that are still in
/// their sections' order keep their sections; every other slide (new, duplicated, or
/// moved out of order) joins the section of the slide before it, or after it at the
/// start. Deleted slides leave theirs.
fn sections(
    xml: &str,
    pres: &Element,
    plans: &[Plan<'_>],
    ids: &[u32],
    sld_ids: &HashMap<String, (Range<usize>, u32)>,
) -> Vec<(Range<usize>, String)> {
    let Some(list) = pres
        .child("extLst")
        .into_iter()
        .flat_map(|x| x.all("ext"))
        .find_map(|ext| ext.child("sectionLst"))
    else {
        return Vec::new();
    };
    let sections: Vec<&Element> = list.all("section").collect();
    let mut section_of: HashMap<u32, usize> = HashMap::new();
    for (i, s) in sections.iter().enumerate() {
        for id in s.child("sldIdLst").into_iter().flat_map(|l| l.all("sldId")) {
            if let Some(n) = id.num::<u32>("id") {
                section_of.insert(n, i);
            }
        }
    }
    // Each slide's section as read; `None` for new slides.
    let was: Vec<Option<usize>> = plans
        .iter()
        .map(|plan| match plan {
            Plan::Keep { part, .. } => sld_ids
                .get(part)
                .and_then(|(_, old)| section_of.get(old))
                .copied(),
            _ => None,
        })
        .collect();
    let kept = in_order(&was);
    let mut section: Vec<Option<usize>> = (0..was.len())
        .map(|i| if kept[i] { was[i] } else { None })
        .collect();
    let first = section.iter().flatten().next().copied().unwrap_or(0);
    let mut current = first;
    for s in &mut section {
        match s {
            Some(n) => current = *n,
            None => *s = Some(current),
        }
    }
    let mut members: Vec<Vec<u32>> = vec![Vec::new(); sections.len()];
    for (s, &id) in section.iter().zip(ids) {
        if let Some(m) = s.and_then(|s| members.get_mut(s)) {
            m.push(id);
        }
    }
    let mut out = Vec::new();
    for (s, ids) in sections.iter().zip(members) {
        let Some(l) = s.child("sldIdLst") else {
            continue;
        };
        let p = prefix(xml, l);
        let entries: String = ids
            .iter()
            .map(|id| format!(r#"<{p}sldId id="{id}"/>"#))
            .collect();
        out.push((
            l.span.clone(),
            format!("<{p}sldIdLst>{entries}</{p}sldIdLst>"),
        ));
    }
    out
}

/// Which of `sections` (each slide's section as read, `None` for new ones) to keep: the
/// longest run of slides, in order but not necessarily side by side, whose sections never
/// go back.
fn in_order(sections: &[Option<usize>]) -> Vec<bool> {
    let n = sections.len();
    // The longest such run ending at each slide, and the slide before it there.
    let mut len = vec![0usize; n];
    let mut prev = vec![None; n];
    for i in 0..n {
        let Some(s) = sections[i] else { continue };
        len[i] = 1;
        for j in 0..i {
            if sections[j].is_some_and(|t| t <= s) && len[j] + 1 > len[i] {
                len[i] = len[j] + 1;
                prev[i] = Some(j);
            }
        }
    }
    let mut keep = vec![false; n];
    let mut at = (0..n)
        .filter(|&i| len[i] > 0)
        .max_by_key(|&i| (len[i], std::cmp::Reverse(i)));
    while let Some(i) = at {
        keep[i] = true;
        at = prev[i];
    }
    keep
}

/// Slide `part`'s relationships (`text`) with its layout relationship pointing at the
/// part `layout`; one is added when it has none.
fn retarget_layout(text: &str, part: &str, layout: &str) -> io::Result<String> {
    let rels = rels_of(text, part)?;
    if let Some(r) = rels.iter().find(|r| r.kind == "slideLayout" && !r.external) {
        let with = relationship(&r.id, "slideLayout", dir(part), layout);
        return Ok(splice(text, vec![(r.span.clone(), with)]));
    }
    let mut ids: HashSet<String> = rels.iter().map(|r| r.id.clone()).collect();
    let rel = relationship(&next_rid(&mut ids), "slideLayout", dir(part), layout);
    add_relationships(text, part, &[rel])
}

/// Part `part`'s relationships (`text`, empty when it has none) with `rels` added.
fn add_relationships(text: &str, part: &str, rels: &[String]) -> io::Result<String> {
    if text.trim().is_empty() {
        return Ok(format!(
            r#"{XML_DECLARATION}<Relationships xmlns="{RELS_NS}">{}</Relationships>"#,
            rels.concat()
        ));
    }
    let root = parse(text, &rels_name(part))?;
    let add: Vec<(&str, String)> = rels.iter().map(|r| ("Relationship", r.clone())).collect();
    let with = rebuild(&text[root.span.clone()], &[], &[], &[], &add)?;
    Ok(splice(text, vec![(root.span.clone(), with)]))
}

/// A duplicate's relationships: its source's (`text`, of part `from`), with its own copy
/// of the source's notes, which points back at `part`.
fn copy_rels(
    src: &mut Src,
    text: &str,
    from: &str,
    part: &str,
    names: &mut Names,
    changes: &mut Changes,
) -> io::Result<String> {
    if text.trim().is_empty() {
        return Ok(format!(
            r#"{XML_DECLARATION}<Relationships xmlns="{RELS_NS}"/>"#
        ));
    }
    let mut spans = Vec::new();
    for r in rels_of(text, from)? {
        if r.external {
            continue;
        }
        if r.kind == "notesSlide" {
            let notes = names.next(dir(&r.target), "notesSlide");
            let notes_xml = read_text(src, &r.target)?.unwrap_or_default();
            changes.add(&notes, notes_xml, Some(NOTES_TYPE));
            let notes_rels = read_text(src, &rels_name(&r.target))?.unwrap_or_default();
            let back: Vec<(Range<usize>, String)> = rels_of(&notes_rels, &r.target)?
                .into_iter()
                .filter(|nr| nr.kind == "slide" && !nr.external)
                .map(|nr| (nr.span, relationship(&nr.id, "slide", dir(&notes), part)))
                .collect();
            changes.add(&rels_name(&notes), splice(&notes_rels, back), None);
            spans.push((r.span, relationship(&r.id, "notesSlide", dir(part), &notes)));
        } else if dir(from) != dir(part) {
            // Targets are relative to the slide: in the same directory they stay right.
            spans.push((r.span, relationship(&r.id, &r.kind, dir(part), &r.target)));
        }
    }
    Ok(splice(text, spans))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_longest_run_of_slides_in_section_order_keeps_its_sections() {
        let (o, b) = (Some(0), Some(1));
        // A Body slide moved to the front: the rest keep theirs.
        assert_eq!(in_order(&[b, o, o, b, b]), [false, true, true, true, true]);
        // An Opening slide moved to the end, a new slide in between.
        assert_eq!(
            in_order(&[o, b, None, b, o]),
            [true, true, false, true, false]
        );
        assert_eq!(in_order(&[None, None]), [false, false]);
    }

    #[test]
    fn relative_targets() {
        assert_eq!(
            relative("ppt/slides", "ppt/slideLayouts/slideLayout2.xml"),
            "../slideLayouts/slideLayout2.xml"
        );
        assert_eq!(
            relative("ppt", "ppt/slides/slide3.xml"),
            "slides/slide3.xml"
        );
        assert_eq!(
            relative("ppt/slides", "ppt/slides/slide1.xml"),
            "slide1.xml"
        );
    }

    #[test]
    fn run_properties_take_the_name_they_are_written_with() {
        assert_eq!(
            retag(r#"<a:endParaRPr lang="en-US" dirty="0"/>"#, "rPr"),
            r#"<a:rPr lang="en-US" dirty="0"/>"#
        );
        assert_eq!(
            retag(
                r#"<a:rPr b="1"><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill></a:rPr>"#,
                "endParaRPr"
            ),
            r#"<a:endParaRPr b="1"><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill></a:endParaRPr>"#
        );
        assert_eq!(retag("", "rPr"), "");
        assert_eq!(
            field_close(r#"<a:fld id="{1}" type="slidenum">"#).as_deref(),
            Some("</a:fld>")
        );
    }

    #[test]
    fn rebuilt_elements_keep_their_schema_order() {
        // Attributes set in place or added; a fill replaced, a font put after the fill
        // and before the hyperlink.
        let raw = r#"<a:rPr lang="en-US" b='0'><a:noFill/><a:hlinkClick r:id="rId2"/></a:rPr>"#;
        let add = [
            ("latin", r#"<a:latin typeface="Arial"/>"#.to_owned()),
            ("solidFill", solid_fill("a:", Color::rgb(255, 0, 0))),
        ];
        let attrs = [("b", "1".to_owned()), ("sz", "4000".to_owned())];
        assert_eq!(
            rebuild(raw, RUN_ORDER, &attrs, FILLS, &add).unwrap(),
            concat!(
                r#"<a:rPr lang="en-US" b="1" sz="4000"><a:solidFill><a:srgbClr val="FF0000"/>"#,
                r#"</a:solidFill><a:latin typeface="Arial"/><a:hlinkClick r:id="rId2"/></a:rPr>"#
            )
        );
        // An empty element opens for its children, and stays empty without any.
        let ln = [("ln", "<a:ln/>".to_owned())];
        assert_eq!(
            rebuild("<p:spPr/>", SHAPE_ORDER, &[], &[], &ln).unwrap(),
            "<p:spPr><a:ln/></p:spPr>"
        );
        assert_eq!(
            rebuild(
                "<a:pPr/>",
                PARAGRAPH_ORDER,
                &[("algn", "r".into())],
                &[],
                &[]
            )
            .unwrap(),
            r#"<a:pPr algn="r"/>"#
        );
        // Alpha below opaque is written in thousandths of a percent.
        let half = Color {
            alpha: 128,
            ..Color::BLACK
        };
        assert_eq!(
            srgb("a:", half),
            r#"<a:srgbClr val="000000"><a:alpha val="50196"/></a:srgbClr>"#
        );
    }
}
