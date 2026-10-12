//! Saving a deck by patching the package it was read from (ADR 0006, FRAME-8). Parts no
//! edit touches are copied as compressed bytes. What changes:
//!
//! - an edited shape's paragraphs, in its slide part: written from the model, each run
//!   and paragraph with the markup it was read with (`Run::markup`,
//!   `Paragraph::markup`); everything else in the part stays as written;
//! - `presentation.xml`'s slide list (and its sections, when it has them), its
//!   relationships, and `[Content_Types].xml`, as slides are added, deleted, and moved;
//! - a new slide's part: the slide XML PowerPoint writes for one on its layout, with its
//!   text; a duplicate's: its source's part and relationships, with its own copy of the
//!   source's notes;
//! - a deleted slide's part, relationships, and notes go.
//!
//! The package is written with each part's sizes in its local header (LibreOffice can't
//! read a zip streamed with data descriptors), through `suite_file::save_atomic`.

use crate::package::resolve;
use crate::read::blank_slide;
use crate::xml::{self, escape, Element};
use presentation_model::{Layer, Paragraph, Presentation, Slide, TextBody};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, BufReader, Read, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
/// reads the written file back: its slide count, and the text of every shape the save
/// wrote.
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

/// The shapes a save wrote text into, by slide: their ids.
type Written = Vec<Vec<u32>>;

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
    for (n, ((now, again), ids)) in slides.enumerate() {
        for id in ids {
            let text = |s: &Slide| {
                s.shapes
                    .iter()
                    .find(|sh| sh.id == Some(*id))
                    .map(|sh| sh.plain_text())
            };
            if text(now) != text(again) {
                return Err(format!(
                    "slide {}'s shape {id} reads back differently",
                    n + 1
                ));
            }
        }
    }
    Ok(())
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

/// Replace the `spans` of `text` (any order, not overlapping) with their strings.
fn splice(text: &str, mut spans: Vec<(Range<usize>, String)>) -> String {
    spans.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
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
    let tag = &text[e.span.start + 1..e.span.end];
    let name_end = tag
        .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
        .unwrap_or(tag.len());
    match tag[..name_end].find(':') {
        Some(i) => &tag[..=i],
        None => "",
    }
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
fn paragraphs(body: &TextBody, a: &str) -> String {
    let mut out = String::new();
    for p in &body.paragraphs {
        paragraph(&mut out, p, a);
    }
    out
}

fn paragraph(out: &mut String, p: &Paragraph, a: &str) {
    out.push_str(&format!("<{a}p>"));
    out.push_str(&p.markup);
    for r in &p.runs {
        let props = retag(&r.markup.props, "rPr");
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
    out.push_str(&retag(&p.end.markup.props, "endParaRPr"));
    out.push_str(&format!("</{a}p>"));
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
        let own = e
            .elements()
            .find(|c| c.name.starts_with("nv") && c.name.ends_with("Pr"))
            .and_then(|nv| nv.child("cNvPr"))
            .and_then(|c| c.num::<u32>("id"));
        if own == Some(id) {
            return Some(e);
        }
    }
    None
}

/// The shapes of `now` whose text differs from `base`'s, matched by id: ids and bodies.
fn edits<'a>(base: &Slide, now: &'a Slide) -> Vec<(u32, &'a TextBody)> {
    now.shapes
        .iter()
        .filter(|s| s.layer == Layer::Slide)
        .filter_map(|s| {
            let (id, body) = (s.id?, s.text.as_ref()?);
            let old = base
                .shapes
                .iter()
                .find(|b| b.id == Some(id))
                .and_then(|b| b.text.as_ref());
            (old.map(|o| &o.paragraphs) != Some(&body.paragraphs)).then_some((id, body))
        })
        .collect()
}

/// Slide part `xml` (named `name`) with the `edits` written into it.
fn patch_slide(xml: &str, name: &str, edits: &[(u32, &TextBody)]) -> io::Result<String> {
    if edits.is_empty() {
        return Ok(xml.to_owned());
    }
    let root = parse(xml, name)?;
    let tree = root
        .path(&["cSld", "spTree"])
        .ok_or_else(|| damaged(format!("{name} has no shapes")))?;
    let mut spans = Vec::new();
    for (id, body) in edits {
        let tx = find_shape(tree, *id)
            .and_then(|s| s.child("txBody"))
            .ok_or_else(|| damaged(format!("{name} has no text shape {id}")))?;
        let a = tx.child("bodyPr").map_or("a:", |b| prefix(xml, b));
        let range = match (tx.all("p").next(), tx.all("p").last()) {
            (Some(first), Some(last)) => first.span.start..last.span.end,
            _ => {
                let at = end_tag(xml, tx)?;
                at..at
            }
        };
        spans.push((range, paragraphs(body, a)));
    }
    Ok(splice(xml, spans))
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

/// Part names not taken yet: `{dir}/{stem}{n}.xml`, counting up.
struct Names(HashSet<String>);

impl Names {
    fn next(&mut self, dir: &str, stem: &str) -> String {
        let name = (1..)
            .map(|n| format!("{dir}/{stem}{n}.xml"))
            .find(|name| !self.0.contains(name))
            .expect("a free name");
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

/// What the new package holds: parts replaced (`Some` text) or left out (`None`), and new
/// parts with their content types.
#[derive(Default)]
struct Changes {
    parts: HashMap<String, Option<String>>,
    added: Vec<(String, String)>,
    types_added: Vec<(String, &'static str)>,
    types_removed: HashSet<String>,
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

    for (plan, slide) in plans.iter().zip(&deck.slides) {
        match plan {
            Plan::Keep { part, base } => {
                let ed = edits(base, slide);
                if !ed.is_empty() {
                    let xml = read_text(&mut src, part)?.unwrap_or_default();
                    let patched = patch_slide(&xml, part, &ed)?;
                    changes.parts.insert(part.clone(), Some(patched));
                }
                written.push(ed.iter().map(|(id, _)| *id).collect());
            }
            Plan::Copy { from, part, base } => {
                let ed = edits(base, slide);
                let xml = read_text(&mut src, from)?.unwrap_or_default();
                changes.add(part, patch_slide(&xml, part, &ed)?, Some(SLIDE_TYPE));
                let rels = read_text(&mut src, &rels_name(from))?.unwrap_or_default();
                let rels = copy_rels(&mut src, &rels, from, part, &mut names, &mut changes)?;
                changes.add(&rels_name(part), rels, None);
                written.push(ed.iter().map(|(id, _)| *id).collect());
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
                let ed = edits(base, slide);
                changes.add(part, patch_slide(&blank, part, &ed)?, Some(SLIDE_TYPE));
                let rels = format!(
                    r#"{XML_DECLARATION}<Relationships xmlns="{RELS_NS}">{}</Relationships>"#,
                    relationship("rId1", "slideLayout", dir(part), &layout.part)
                );
                changes.add(&rels_name(part), rels, None);
                written.push(ed.iter().map(|(id, _)| *id).collect());
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
    let new_types: String = changes
        .types_added
        .iter()
        .map(|(part, kind)| {
            format!(
                r#"<Override PartName="/{}" ContentType="{kind}"/>"#,
                escape(part)
            )
        })
        .collect();
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
}
