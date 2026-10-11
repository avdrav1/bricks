//! From package parts to the model: slide order, shapes in z-order, placeholder
//! inheritance (slide → layout → master), group transforms, hidden slides, and notes.

use crate::package::{Package, Part};
use crate::xml::Element;
use crate::Error;
use presentation_model::{
    Frame, Layer, PlaceholderKind, Presentation, Shape, ShapeKind, Size, Slide,
};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::LazyLock;

/// PowerPoint's default slide size (10 x 7.5 in) when `presentation.xml` names none.
const DEFAULT_SIZE: Size = Size {
    cx: 9_144_000,
    cy: 6_858_000,
};

pub(crate) struct Reader {
    pkg: Package,
    masters: HashMap<String, Rc<Master>>,
    layouts: HashMap<String, Rc<Layout>>,
}

struct Master {
    part: Rc<Part>,
    placeholders: Vec<Placeholder>,
}

struct Layout {
    part: Rc<Part>,
    placeholders: Vec<Placeholder>,
    master: Rc<Master>,
    /// `showMasterSp`: whether slides on this layout show the master's shapes.
    show_master: bool,
}

/// A placeholder shape on a layout or master, as slides inherit from it.
struct Placeholder {
    /// `<p:ph type>`, `obj` when absent (the schema's default).
    kind: String,
    idx: Option<u32>,
    shape: Element,
}

/// What a shape on the slide being read inherits from.
struct Scope<'a> {
    layer: Layer,
    layout: Option<&'a Layout>,
}

/// A group's transform: its frame on the slide (or in its parent group) and the child
/// coordinate space (`chOff`, `chExt`) that frame shows.
#[derive(Clone, Copy)]
struct Group {
    frame: Frame,
    ch_x: i64,
    ch_y: i64,
    ch_cx: i64,
    ch_cy: i64,
}

impl Reader {
    pub fn new(pkg: Package) -> Self {
        Reader {
            pkg,
            masters: HashMap::new(),
            layouts: HashMap::new(),
        }
    }

    pub fn presentation(mut self) -> Result<Presentation, Error> {
        let main = self
            .pkg
            .rels("")?
            .of_kind("officeDocument")
            .map(|r| r.target.clone())
            .unwrap_or_else(|| "ppt/presentation.xml".to_owned());
        let pres = self.pkg.part(&main)?;
        let size = pres
            .root
            .child("sldSz")
            .and_then(|s| {
                Some(Size {
                    cx: s.num("cx")?,
                    cy: s.num("cy")?,
                })
            })
            .unwrap_or(DEFAULT_SIZE);
        let mut slides = Vec::new();
        for id in pres
            .root
            .path(&["sldIdLst"])
            .into_iter()
            .flat_map(|l| l.all("sldId"))
        {
            let Some(rel) = id.rel_attr("id").and_then(|r| pres.rels.get(r)) else {
                continue;
            };
            let target = rel.target.clone();
            slides.push(self.slide(&target)?);
        }
        Ok(Presentation { size, slides })
    }

    fn slide(&mut self, path: &str) -> Result<Slide, Error> {
        let part = self.pkg.part(path)?;
        let layout = match part.rels.of_kind("slideLayout") {
            Some(r) => {
                let target = r.target.clone();
                Some(self.layout(&target)?)
            }
            None => None,
        };
        let root = &part.root;
        let mut shapes = Vec::new();
        if let Some(layout) = &layout {
            if root.flag("showMasterSp") != Some(false) {
                if layout.show_master {
                    let scope = Scope {
                        layer: Layer::Master,
                        layout: None,
                    };
                    collect(
                        &scope,
                        sp_tree(&layout.master.part.root),
                        &mut Vec::new(),
                        &mut shapes,
                    );
                }
                let scope = Scope {
                    layer: Layer::Layout,
                    layout: None,
                };
                collect(
                    &scope,
                    sp_tree(&layout.part.root),
                    &mut Vec::new(),
                    &mut shapes,
                );
            }
        }
        let scope = Scope {
            layer: Layer::Slide,
            layout: layout.as_deref(),
        };
        collect(&scope, sp_tree(root), &mut Vec::new(), &mut shapes);
        let notes = match part.rels.of_kind("notesSlide") {
            Some(r) => {
                let target = r.target.clone();
                notes_text(&self.pkg.part(&target)?.root)
            }
            None => String::new(),
        };
        Ok(Slide {
            shapes,
            hidden: root.flag("show") == Some(false),
            notes,
        })
    }

    fn layout(&mut self, path: &str) -> Result<Rc<Layout>, Error> {
        if let Some(l) = self.layouts.get(path) {
            return Ok(l.clone());
        }
        let part = self.pkg.part(path)?;
        let master_path = part
            .rels
            .of_kind("slideMaster")
            .map(|r| r.target.clone())
            .ok_or_else(|| Error::damaged(path, "the layout has no master"))?;
        let master = self.master(&master_path)?;
        let layout = Rc::new(Layout {
            placeholders: placeholders(sp_tree(&part.root)),
            show_master: part.root.flag("showMasterSp") != Some(false),
            part,
            master,
        });
        self.layouts.insert(path.to_owned(), layout.clone());
        Ok(layout)
    }

    fn master(&mut self, path: &str) -> Result<Rc<Master>, Error> {
        if let Some(m) = self.masters.get(path) {
            return Ok(m.clone());
        }
        let part = self.pkg.part(path)?;
        let master = Rc::new(Master {
            placeholders: placeholders(sp_tree(&part.root)),
            part,
        });
        self.masters.insert(path.to_owned(), master.clone());
        Ok(master)
    }
}

/// `<p:cSld><p:spTree>`, or an empty element when the part has none.
fn sp_tree(root: &Element) -> &Element {
    static EMPTY: LazyLock<Element> = LazyLock::new(Element::default);
    root.path(&["cSld", "spTree"]).unwrap_or(&EMPTY)
}

/// The placeholders of a layout or master shape tree (groups included).
fn placeholders(tree: &Element) -> Vec<Placeholder> {
    let mut out = Vec::new();
    for e in tree.elements() {
        match e.name.as_str() {
            "grpSp" => out.extend(placeholders(e)),
            _ => {
                if let Some(ph) = ph_of(e) {
                    out.push(Placeholder {
                        kind: ph.attr("type").unwrap_or("obj").to_owned(),
                        idx: ph.num("idx"),
                        shape: e.clone(),
                    });
                }
            }
        }
    }
    out
}

/// A shape's non-visual properties (`nvSpPr`, `nvPicPr`, ...).
fn nv_props(shape: &Element) -> Option<&Element> {
    shape
        .elements()
        .find(|e| e.name.starts_with("nv") && e.name.ends_with("Pr"))
}

/// A shape's `<p:ph>`.
fn ph_of(shape: &Element) -> Option<&Element> {
    nv_props(shape)?.path(&["nvPr", "ph"])
}

/// The family a placeholder type matches in a master: titles, body text, and the
/// footer placeholders by their own type.
fn family(kind: &str) -> &str {
    match kind {
        "title" | "ctrTitle" => "title",
        "dt" | "ftr" | "sldNum" | "hdr" => kind,
        _ => "body",
    }
}

fn placeholder_kind(kind: &str) -> PlaceholderKind {
    match kind {
        "title" => PlaceholderKind::Title,
        "ctrTitle" => PlaceholderKind::CentredTitle,
        "subTitle" => PlaceholderKind::Subtitle,
        "dt" => PlaceholderKind::Date,
        "ftr" => PlaceholderKind::Footer,
        "sldNum" => PlaceholderKind::SlideNumber,
        "hdr" => PlaceholderKind::Header,
        "body" | "obj" => PlaceholderKind::Body,
        _ => PlaceholderKind::Content,
    }
}

/// The layout placeholder a slide placeholder inherits from: same `idx`, else same type,
/// else same family (a slide's `ctrTitle` on a layout's `title`).
fn find<'a>(list: &'a [Placeholder], kind: &str, idx: Option<u32>) -> Option<&'a Placeholder> {
    idx.and_then(|i| list.iter().find(|p| p.idx == Some(i)))
        .or_else(|| list.iter().find(|p| p.kind == kind))
        .or_else(|| list.iter().find(|p| family(&p.kind) == family(kind)))
}

/// The shapes a slide placeholder inherits from, nearest first: its layout's, then that
/// one's master's (found by the layout placeholder's type). A placeholder its layout has
/// no match for (an orphan, such as a title on a Blank layout, often `idx="4294967295"`)
/// inherits no shape, as in LibreOffice: only its master text style.
fn inherited<'a>(scope: &Scope<'a>, ph: &Element) -> Vec<&'a Element> {
    let Some(layout) = scope.layout else {
        return Vec::new();
    };
    let kind = ph.attr("type").unwrap_or("obj");
    let Some(from_layout) = find(&layout.placeholders, kind, ph.num("idx")) else {
        return Vec::new();
    };
    let mut out = vec![&from_layout.shape];
    let master_kind = from_layout.kind.as_str();
    let masters = &layout.master.placeholders;
    if let Some(p) = masters.iter().find(|p| p.kind == master_kind).or_else(|| {
        masters
            .iter()
            .find(|p| family(&p.kind) == family(master_kind))
    }) {
        out.push(&p.shape);
    }
    out
}

/// A shape's `<a:xfrm>`: in `spPr` for shapes and pictures, `grpSpPr` for groups, and
/// directly in a graphic frame.
fn xfrm(shape: &Element) -> Option<&Element> {
    match shape.name.as_str() {
        "graphicFrame" => shape.child("xfrm"),
        "grpSp" => shape.path(&["grpSpPr", "xfrm"]),
        _ => shape.path(&["spPr", "xfrm"]),
    }
}

fn frame_of(x: &Element) -> Frame {
    let off = x.child("off");
    let ext = x.child("ext");
    let n = |e: Option<&Element>, a| e.and_then(|e| e.num::<i64>(a)).unwrap_or(0);
    Frame {
        x: n(off, "x"),
        y: n(off, "y"),
        cx: n(ext, "cx"),
        cy: n(ext, "cy"),
        rotation: normalize(x.num::<f64>("rot").unwrap_or(0.0) / 60_000.0),
        flip_h: x.flag("flipH") == Some(true),
        flip_v: x.flag("flipV") == Some(true),
    }
}

fn normalize(deg: f64) -> f64 {
    let d = deg.rem_euclid(360.0);
    if (360.0 - d).abs() < 1e-9 {
        0.0
    } else {
        d
    }
}

impl Group {
    fn of(x: &Element) -> Group {
        let frame = frame_of(x);
        let off = x.child("chOff");
        let ext = x.child("chExt");
        let n = |e: Option<&Element>, a| e.and_then(|e| e.num::<i64>(a)).unwrap_or(0);
        Group {
            frame,
            ch_x: n(off, "x"),
            ch_y: n(off, "y"),
            ch_cx: n(ext, "cx"),
            ch_cy: n(ext, "cy"),
        }
    }

    /// `f`, given in this group's child space, in the group's parent space: scaled from
    /// the child extent to the group's, mirrored by the group's flips, and rotated with
    /// the group about the group's centre.
    fn apply(&self, f: Frame) -> Frame {
        let g = &self.frame;
        let scale = |ext: i64, ch: i64| if ch == 0 { 1.0 } else { ext as f64 / ch as f64 };
        let (sx, sy) = (scale(g.cx, self.ch_cx), scale(g.cy, self.ch_cy));
        let (cx, cy) = f.centre();
        let mut px = g.x as f64 + (cx - self.ch_x as f64) * sx;
        let mut py = g.y as f64 + (cy - self.ch_y as f64) * sy;
        let (gx, gy) = g.centre();
        if g.flip_h {
            px = 2.0 * gx - px;
        }
        if g.flip_v {
            py = 2.0 * gy - py;
        }
        let (sin, cos) = g.rotation.to_radians().sin_cos();
        let (dx, dy) = (px - gx, py - gy);
        let (px, py) = (gx + dx * cos - dy * sin, gy + dx * sin + dy * cos);
        let (w, h) = (f.cx as f64 * sx, f.cy as f64 * sy);
        let mirrored = g.flip_h != g.flip_v;
        let own = if mirrored { -f.rotation } else { f.rotation };
        Frame {
            x: (px - w / 2.0).round() as i64,
            y: (py - h / 2.0).round() as i64,
            cx: w.round() as i64,
            cy: h.round() as i64,
            rotation: normalize(own + g.rotation),
            flip_h: f.flip_h != g.flip_h,
            flip_v: f.flip_v != g.flip_v,
        }
    }
}

/// Which branch of `<mc:AlternateContent>` to read: a `Choice` that needs only the 2010
/// PowerPoint and DrawingML extensions (whose content is ordinary shapes), else the
/// `Fallback`, which is there for readers that know none.
fn alternate(e: &Element) -> Option<&Element> {
    const KNOWN: [&str; 4] = ["p14", "p15", "a14", "a16"];
    e.all("Choice")
        .find(|c| {
            c.attr("Requires")
                .is_some_and(|r| r.split_whitespace().all(|p| KNOWN.contains(&p)))
        })
        .or_else(|| e.child("Fallback"))
}

/// Read the shapes of `tree` into `out`, in z-order, groups flattened.
fn collect(scope: &Scope<'_>, tree: &Element, groups: &mut Vec<Group>, out: &mut Vec<Shape>) {
    for e in tree.elements() {
        match e.name.as_str() {
            "sp" | "cxnSp" | "pic" | "graphicFrame" => {
                if let Some(shape) = shape(scope, e, groups) {
                    out.push(shape);
                }
            }
            "grpSp" => {
                if let Some(x) = xfrm(e) {
                    groups.push(Group::of(x));
                    collect(scope, e, groups, out);
                    groups.pop();
                } else {
                    collect(scope, e, groups, out);
                }
            }
            "AlternateContent" => {
                if let Some(branch) = alternate(e) {
                    collect(scope, branch, groups, out);
                }
            }
            _ => {}
        }
    }
}

fn shape(scope: &Scope<'_>, e: &Element, groups: &[Group]) -> Option<Shape> {
    let nv = nv_props(e);
    let ph = ph_of(e);
    // Layouts and masters lend their placeholders to slides; they don't show them.
    if ph.is_some() && scope.layer != Layer::Slide {
        return None;
    }
    let parents = ph.map(|ph| inherited(scope, ph)).unwrap_or_default();
    let x = xfrm(e).or_else(|| parents.iter().find_map(|p| xfrm(p)));
    let frame = x.map(frame_of).unwrap_or_default();
    let frame = groups.iter().rev().fold(frame, |f, g| g.apply(f));
    let kind = match e.name.as_str() {
        "pic" => ShapeKind::Picture,
        "graphicFrame" => graphic_kind(e),
        _ => ShapeKind::Geometry {
            preset: std::iter::once(e)
                .chain(parents.iter().copied())
                .find_map(preset)
                .unwrap_or_else(|| "rect".to_owned()),
        },
    };
    Some(Shape {
        name: nv
            .and_then(|n| n.child("cNvPr"))
            .and_then(|c| c.attr("name"))
            .unwrap_or_default()
            .to_owned(),
        frame,
        kind,
        placeholder: ph.map(|p| placeholder_kind(p.attr("type").unwrap_or("obj"))),
        layer: scope.layer,
    })
}

/// A shape's geometry: its preset name, or `custom` for freeform geometry.
fn preset(shape: &Element) -> Option<String> {
    let sp_pr = shape.child("spPr")?;
    if let Some(p) = sp_pr.child("prstGeom") {
        return Some(p.attr("prst").unwrap_or("rect").to_owned());
    }
    sp_pr.child("custGeom").map(|_| "custom".to_owned())
}

fn graphic_kind(frame: &Element) -> ShapeKind {
    let uri = frame
        .path(&["graphic", "graphicData"])
        .and_then(|d| d.attr("uri"))
        .unwrap_or_default();
    let label = match uri.rsplit('/').next().unwrap_or_default() {
        "table" => return ShapeKind::Table,
        "chart" | "chartex" => "Chart",
        "diagram" => "SmartArt",
        "ole" => "Embedded object",
        _ => "Object",
    };
    ShapeKind::Unsupported {
        label: label.to_owned(),
    }
}

/// The text of a notes slide's body placeholder: its paragraphs joined by `\n`.
fn notes_text(root: &Element) -> String {
    let mut paragraphs = Vec::new();
    for sp in sp_tree(root).all("sp") {
        let Some(ph) = ph_of(sp) else { continue };
        if ph.attr("type") != Some("body") {
            continue;
        }
        let Some(body) = sp.child("txBody") else {
            continue;
        };
        for p in body.all("p") {
            let mut text = String::new();
            for r in p.elements() {
                match r.name.as_str() {
                    "r" | "fld" => {
                        if let Some(t) = r.child("t") {
                            text.push_str(&t.text());
                        }
                    }
                    "br" => text.push('\n'),
                    _ => {}
                }
            }
            paragraphs.push(text);
        }
    }
    let joined = paragraphs.join("\n");
    joined.trim_end_matches('\n').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(x: &str) -> Group {
        Group::of(&crate::xml::parse(x.as_bytes()).unwrap())
    }

    #[test]
    fn a_group_scales_and_moves_its_children() {
        let g = group(
            r#"<xfrm><off x="1000" y="2000"/><ext cx="2000" cy="1000"/><chOff x="0" y="0"/><chExt cx="1000" cy="1000"/></xfrm>"#,
        );
        let f = g.apply(Frame {
            x: 500,
            y: 0,
            cx: 500,
            cy: 500,
            ..Frame::default()
        });
        assert_eq!((f.x, f.y, f.cx, f.cy), (2000, 2000, 1000, 500));
    }

    #[test]
    fn a_flipped_group_mirrors_its_children() {
        let g = group(
            r#"<xfrm flipH="1"><off x="0" y="0"/><ext cx="1000" cy="1000"/><chOff x="0" y="0"/><chExt cx="1000" cy="1000"/></xfrm>"#,
        );
        let f = g.apply(Frame {
            x: 0,
            y: 0,
            cx: 200,
            cy: 200,
            rotation: 30.0,
            ..Frame::default()
        });
        assert_eq!((f.x, f.y), (800, 0));
        assert!(f.flip_h);
        assert!((f.rotation - 330.0).abs() < 1e-9);
    }

    #[test]
    fn a_rotated_group_turns_children_about_its_centre() {
        let g = group(
            r#"<xfrm rot="5400000"><off x="0" y="0"/><ext cx="1000" cy="1000"/><chOff x="0" y="0"/><chExt cx="1000" cy="1000"/></xfrm>"#,
        );
        // The top-left quarter turns to the top-right quarter.
        let f = g.apply(Frame {
            x: 0,
            y: 0,
            cx: 500,
            cy: 500,
            ..Frame::default()
        });
        assert_eq!((f.x, f.y), (500, 0));
        assert!((f.rotation - 90.0).abs() < 1e-9);
    }
}
