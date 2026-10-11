//! From package parts to the model: slide order, shapes in z-order, placeholder
//! inheritance (slide → layout → master), group transforms, text (through `text`),
//! fills and outlines (through `fill`), pictures, tables (through `table`), SmartArt
//! drawings, backgrounds, hidden slides, and notes.

use crate::color::Palette;
use crate::fill::{self, Looks};
use crate::package::{Package, Part};
use crate::text::{self, FontRef, Fonts, Styles, TextContext};
use crate::xml::Element;
use crate::Error;
use presentation_model::{
    Background, Fill, Frame, Image, Layer, Picture, PlaceholderKind, Presentation, Shape,
    ShapeKind, Size, Slide, TextBody,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::LazyLock;

/// PowerPoint's default slide size (10 x 7.5 in) when `presentation.xml` names none.
const DEFAULT_SIZE: Size = Size {
    cx: 9_144_000,
    cy: 6_858_000,
};

pub(crate) struct Reader {
    pkg: RefCell<Package>,
    masters: HashMap<String, Rc<Master>>,
    layouts: HashMap<String, Rc<Layout>>,
    /// `presentation.xml`'s `defaultTextStyle`.
    default_style: Option<Element>,
    /// `tableStyles.xml`.
    table_styles: Option<Rc<Part>>,
}

struct Master {
    part: Rc<Part>,
    placeholders: Vec<Placeholder>,
    fonts: Fonts,
    /// The theme's colour scheme through the master's colour map.
    palette: Palette,
    /// The theme part: its `fmtScheme` (fill, line, and background styles) and the
    /// relationships of the pictures those use.
    theme: Option<Rc<Part>>,
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

/// What reading the shapes of one part needs.
struct Scope<'a> {
    layer: Layer,
    /// The part the shapes are in (its relationships name pictures and diagrams).
    part: &'a Part,
    pkg: &'a RefCell<Package>,
    /// The layout slide placeholders inherit from (`None` on layouts and masters).
    layout: Option<&'a Layout>,
    master: Option<&'a Master>,
    default_style: Option<&'a Element>,
    table_styles: Option<&'a Element>,
    text: &'a TextContext<'a>,
}

impl<'a> Scope<'a> {
    fn looks(&self) -> Looks<'a> {
        let theme = self.master.and_then(|m| m.theme.as_deref());
        Looks {
            palette: self.text.palette,
            fmt: theme.and_then(|t| t.root.path(&["themeElements", "fmtScheme"])),
            rels: &self.part.rels,
            theme_rels: theme.map(|t| &t.rels),
            pkg: self.pkg,
        }
    }

    /// The same scope over another part (a SmartArt drawing).
    fn on(&self, part: &'a Part) -> Scope<'a> {
        Scope {
            layer: self.layer,
            part,
            pkg: self.pkg,
            layout: self.layout,
            master: self.master,
            default_style: self.default_style,
            table_styles: self.table_styles,
            text: self.text,
        }
    }
}

/// A group's transform: its frame on the slide (or in its parent group) and the child
/// coordinate space (`chOff`, `chExt`) that frame shows; and its fill, for children
/// with `grpFill`.
#[derive(Clone)]
struct Group {
    frame: Frame,
    ch_x: i64,
    ch_y: i64,
    ch_cx: i64,
    ch_cy: i64,
    fill: Fill,
}

impl Reader {
    pub fn new(pkg: Package) -> Self {
        Reader {
            pkg: RefCell::new(pkg),
            masters: HashMap::new(),
            layouts: HashMap::new(),
            default_style: None,
            table_styles: None,
        }
    }

    fn part(&self, name: &str) -> Result<Rc<Part>, Error> {
        self.pkg.borrow_mut().part(name)
    }

    pub fn presentation(mut self) -> Result<Presentation, Error> {
        let main = self
            .pkg
            .borrow_mut()
            .rels("")?
            .of_kind("officeDocument")
            .map(|r| r.target.clone())
            .unwrap_or_else(|| "ppt/presentation.xml".to_owned());
        let pres = self.part(&main)?;
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
        self.default_style = pres.root.child("defaultTextStyle").cloned();
        if let Some(r) = pres.rels.of_kind("tableStyles") {
            self.table_styles = self.part(&r.target).ok();
        }
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
            let number = slides.len() + 1;
            slides.push(self.slide(&rel.target, number)?);
        }
        Ok(Presentation { size, slides })
    }

    fn slide(&mut self, path: &str, number: usize) -> Result<Slide, Error> {
        let part = self.part(path)?;
        let layout = match part.rels.of_kind("slideLayout") {
            Some(r) => Some(self.layout(&r.target)?),
            None => None,
        };
        let root = &part.root;
        let master = layout.as_ref().map(|l| l.master.clone());
        // Colours: the master's map, as the layout and then the slide override it.
        let mut palette = master
            .as_ref()
            .map(|m| m.palette.clone())
            .unwrap_or_default();
        for over in layout
            .as_ref()
            .map(|l| &l.part.root)
            .into_iter()
            .chain([root])
            .filter_map(|r| r.path(&["clrMapOvr", "overrideClrMapping"]))
        {
            palette.remap(over);
        }
        let fonts = master.as_ref().map(|m| m.fonts.clone()).unwrap_or_default();
        let ctx = TextContext {
            palette: &palette,
            fonts: &fonts,
            slide_number: number,
        };
        let mut shapes = Vec::new();
        let scope = |layer, part, layout| Scope {
            layer,
            part,
            pkg: &self.pkg,
            layout,
            master: master.as_deref(),
            default_style: self.default_style.as_ref(),
            table_styles: self.table_styles.as_ref().map(|t| &t.root),
            text: &ctx,
        };
        // The background: the slide's own, else its layout's, else its master's.
        let mut background = Background::default();
        let mut sources: Vec<&Part> = vec![&part];
        if let Some(l) = &layout {
            sources.push(&l.part);
            sources.push(&l.master.part);
        }
        for p in sources {
            if let Some(bg) = p.root.path(&["cSld", "bg"]) {
                if let Some(b) = scope(Layer::Slide, p, None).looks().background(bg) {
                    background = b;
                }
                break;
            }
        }
        if let Some(layout) = &layout {
            if root.flag("showMasterSp") != Some(false) {
                if layout.show_master {
                    let m = &layout.master.part;
                    let s = scope(Layer::Master, m, None);
                    collect(&s, sp_tree(&m.root), &mut Vec::new(), &mut shapes);
                }
                let s = scope(Layer::Layout, &layout.part, None);
                collect(&s, sp_tree(&layout.part.root), &mut Vec::new(), &mut shapes);
            }
        }
        let s = scope(Layer::Slide, &part, layout.as_deref());
        collect(&s, sp_tree(root), &mut Vec::new(), &mut shapes);
        let notes = match part.rels.of_kind("notesSlide") {
            Some(r) => notes_text(&self.part(&r.target)?.root),
            None => String::new(),
        };
        Ok(Slide {
            background,
            shapes,
            hidden: root.flag("show") == Some(false),
            notes,
        })
    }

    fn layout(&mut self, path: &str) -> Result<Rc<Layout>, Error> {
        if let Some(l) = self.layouts.get(path) {
            return Ok(l.clone());
        }
        let part = self.part(path)?;
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
        let part = self.part(path)?;
        let theme = match part.rels.of_kind("theme") {
            Some(r) => Some(self.part(&r.target)?),
            None => None,
        };
        let elements = theme.as_ref().and_then(|t| t.root.child("themeElements"));
        let master = Rc::new(Master {
            placeholders: placeholders(sp_tree(&part.root)),
            fonts: Fonts::of(elements.and_then(|e| e.child("fontScheme"))),
            palette: Palette::new(
                elements.and_then(|e| e.child("clrScheme")),
                part.root.child("clrMap"),
            ),
            theme: theme.clone(),
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
/// inherits nothing, as in LibreOffice.
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
            fill: Fill::None,
        }
    }

    /// A SmartArt drawing's space: its coordinates are offsets from the graphic frame.
    fn translate(frame: Frame) -> Group {
        Group {
            frame: Frame {
                rotation: 0.0,
                flip_h: false,
                flip_v: false,
                ..frame
            },
            ch_x: 0,
            ch_y: 0,
            ch_cx: frame.cx,
            ch_cy: frame.cy,
            fill: Fill::None,
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
            "graphicFrame" if graphic_uri(e).ends_with("/diagram") => {
                smart_art(scope, e, groups, out);
            }
            "sp" | "cxnSp" | "pic" | "graphicFrame" => {
                if let Some(shape) = shape(scope, e, groups) {
                    out.push(shape);
                }
            }
            "grpSp" => {
                let parent_fill = groups.last().map(|g| g.fill.clone()).unwrap_or_default();
                let mut group = xfrm(e).map(Group::of);
                if let Some(g) = &mut group {
                    g.fill = e
                        .child("grpSpPr")
                        .and_then(|p| scope.looks().fill_in(p, None, &parent_fill))
                        .unwrap_or(parent_fill);
                }
                match group {
                    Some(g) => {
                        groups.push(g);
                        collect(scope, e, groups, out);
                        groups.pop();
                    }
                    None => collect(scope, e, groups, out),
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

fn graphic_uri(frame: &Element) -> &str {
    frame
        .path(&["graphic", "graphicData"])
        .and_then(|d| d.attr("uri"))
        .unwrap_or_default()
}

/// A SmartArt graphic frame: the shapes of the drawing PowerPoint saved with it (the
/// diagram data part names the drawing), placed in the frame. Without a drawing it is an
/// unsupported shape.
fn smart_art(scope: &Scope<'_>, e: &Element, groups: &mut Vec<Group>, out: &mut Vec<Shape>) {
    let drawing = (|| {
        let ids = e.path(&["graphic", "graphicData", "relIds"])?;
        let data = scope.part.rels.get(ids.rel_attr("dm")?)?;
        let data = scope.pkg.borrow_mut().part(&data.target).ok()?;
        let rel_id = data
            .root
            .path(&["extLst", "ext", "dataModelExt"])?
            .attr("relId")?;
        let drawing = scope.part.rels.get(rel_id)?;
        scope.pkg.borrow_mut().part(&drawing.target).ok()
    })();
    let Some(drawing) = drawing else {
        if let Some(shape) = shape(scope, e, groups) {
            out.push(shape);
        }
        return;
    };
    let frame = xfrm(e).map(frame_of).unwrap_or_default();
    let tree = drawing.root.child("spTree").unwrap_or(&drawing.root);
    groups.push(Group::translate(frame));
    collect(&scope.on(&drawing), tree, groups, out);
    groups.pop();
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
    let looks = scope.looks();
    let group_fill = groups.last().map(|g| g.fill.clone()).unwrap_or_default();
    let sp_prs: Vec<&Element> = std::iter::once(e)
        .chain(parents.iter().copied())
        .filter_map(|s| s.child("spPr"))
        .collect();
    let style = e.child("style");
    let kind = match e.name.as_str() {
        "pic" => picture(e, &looks),
        "graphicFrame" => graphic(scope, e, &looks),
        _ => {
            let geom = std::iter::once(e)
                .chain(parents.iter().copied())
                .find_map(geometry);
            let (preset, adjust) = geom.unwrap_or_else(|| ("rect".to_owned(), None));
            ShapeKind::Geometry { preset, adjust }
        }
    };
    let (fill, outline) = match e.name.as_str() {
        // A graphic frame's look is its content's.
        "graphicFrame" => (Fill::None, None),
        _ => (
            looks.shape_fill(&sp_prs, style, &group_fill),
            looks.outline(&sp_prs, style),
        ),
    };
    let text = shape_text(scope, e, ph, &parents);
    let c_nv = nv.and_then(|n| n.child("cNvPr"));
    let name = c_nv
        .and_then(|c| c.attr("name"))
        .filter(|n| !n.is_empty())
        .or_else(|| e.attr("modelId"))
        .unwrap_or_default()
        .to_owned();
    Some(Shape {
        name,
        frame,
        kind,
        fill,
        outline,
        placeholder: ph.map(|p| placeholder_kind(p.attr("type").unwrap_or("obj"))),
        layer: scope.layer,
        text,
        // A SmartArt drawing part's text box (`dsp:txXfrm`).
        text_frame: e
            .child("txXfrm")
            .map(|x| groups.iter().rev().fold(frame_of(x), |f, g| g.apply(f))),
    })
}

/// A picture shape: its image, or what it stands for (a video, an EMF picture).
fn picture(e: &Element, looks: &Looks<'_>) -> ShapeKind {
    let blip_fill = e.child("blipFill");
    let image = blip_fill
        .and_then(|b| b.child("blip"))
        .and_then(|b| looks.image(b));
    let nv_pr = nv_props(e).and_then(|n| n.child("nvPr"));
    let media = nv_pr.and_then(|n| {
        n.elements()
            .find_map(|m| match m.name.as_str() {
                "videoFile" | "quickTimeFile" => Some("Video"),
                "audioFile" | "audioCd" | "wavAudioFile" => Some("Audio"),
                _ => None,
            })
            .or_else(|| {
                // PowerPoint 2010's embedded media: `<p14:media>` in an extension.
                n.path(&["extLst", "ext", "media"]).map(|_| "Video")
            })
    });
    match (media, image) {
        (Some(label), image) => ShapeKind::Unsupported {
            label: label.to_owned(),
            preview: image.filter(|i| i.format.is_drawable()),
        },
        (None, Some(image)) if image.format.is_drawable() => ShapeKind::Picture(Picture {
            image,
            crop: blip_fill.map(fill::crop).unwrap_or_default(),
        }),
        (None, Some(image)) => ShapeKind::Unsupported {
            label: format!("Picture ({})", image.format.name()),
            preview: None,
        },
        (None, None) => ShapeKind::Unsupported {
            label: "Picture (missing)".to_owned(),
            preview: None,
        },
    }
}

/// A graphic frame: a table, or a labelled shape for charts, embedded objects (with the
/// preview picture the file keeps for them), and anything else.
fn graphic(scope: &Scope<'_>, e: &Element, looks: &Looks<'_>) -> ShapeKind {
    let Some(data) = e.path(&["graphic", "graphicData"]) else {
        return unsupported("Object", None);
    };
    match graphic_uri(e).rsplit('/').next().unwrap_or_default() {
        "table" => match data.child("tbl") {
            Some(tbl) => {
                let base: Vec<&Element> = scope
                    .default_style
                    .into_iter()
                    .chain(
                        scope
                            .master
                            .and_then(|m| m.part.root.path(&["txStyles", "otherStyle"])),
                    )
                    .collect();
                ShapeKind::Table(crate::table::table(
                    tbl,
                    looks,
                    scope.text,
                    &base,
                    scope.table_styles,
                ))
            }
            None => unsupported("Table", None),
        },
        "chart" | "chartex" => unsupported("Chart", None),
        "ole" => {
            let ole = data.child("oleObj").or_else(|| {
                data.child("AlternateContent")
                    .and_then(|a| a.child("Fallback").or_else(|| a.child("Choice")))
                    .and_then(|b| b.child("oleObj"))
            });
            let preview = ole
                .and_then(|o| o.path(&["pic", "blipFill", "blip"]))
                .and_then(|b| looks.image(b))
                .filter(|i| i.format.is_drawable());
            unsupported("Embedded object", preview)
        }
        "diagram" => unsupported("SmartArt", None),
        _ => unsupported("Object", None),
    }
}

fn unsupported(label: &str, preview: Option<Image>) -> ShapeKind {
    ShapeKind::Unsupported {
        label: label.to_owned(),
        preview,
    }
}

/// A shape's text with its styles: the presentation default, the master text style for
/// its kind, its style's font reference, and the placeholders' and its own list styles.
/// An orphan placeholder (no layout match) reads like a text box, as in LibreOffice.
fn shape_text(
    scope: &Scope<'_>,
    e: &Element,
    ph: Option<&Element>,
    parents: &[&Element],
) -> Option<TextBody> {
    let tx_body = e.child("txBody")?;
    let linked = ph.filter(|_| !parents.is_empty());
    let master_style = match linked.map(|p| family(p.attr("type").unwrap_or("obj"))) {
        Some("title") => "titleStyle",
        Some("body") => "bodyStyle",
        _ => "otherStyle",
    };
    let base = scope
        .default_style
        .into_iter()
        .chain(
            scope
                .master
                .and_then(|m| m.part.root.path(&["txStyles", master_style])),
        )
        .collect();
    // Nearest parent last: the master placeholder's list style, then the layout's.
    let inherited = parents.iter().rev().filter_map(|p| p.child("txBody"));
    let styles = Styles {
        base,
        font_ref: font_ref(e, scope.text),
        shape: inherited
            .clone()
            .chain([tx_body])
            .filter_map(|b| b.child("lstStyle"))
            .collect(),
        body: inherited.filter_map(|b| b.child("bodyPr")).collect(),
    };
    text::body(Some(tx_body), &styles, scope.text)
}

/// The font and colour a shape style's `<a:fontRef idx="minor|major">` gives its text.
fn font_ref(e: &Element, ctx: &TextContext<'_>) -> FontRef {
    let Some(r) = e.path(&["style", "fontRef"]) else {
        return FontRef::default();
    };
    FontRef {
        font: match r.attr("idx") {
            Some("major") => Some(ctx.fonts.major.clone()),
            Some("minor") => Some(ctx.fonts.minor.clone()),
            _ => None,
        },
        color: ctx.palette.first(r, None),
        ..FontRef::default()
    }
}

/// A shape's geometry: its preset name and first adjust value, or `custom` for freeform
/// geometry.
fn geometry(shape: &Element) -> Option<(String, Option<f64>)> {
    let sp_pr = shape.child("spPr")?;
    if let Some(p) = sp_pr.child("prstGeom") {
        let adjust = p
            .path(&["avLst", "gd"])
            .and_then(|g| g.attr("fmla"))
            .and_then(|f| f.strip_prefix("val "))
            .and_then(|v| v.trim().parse::<f64>().ok())
            .map(|v| v / 100_000.0);
        return Some((p.attr("prst").unwrap_or("rect").to_owned(), adjust));
    }
    sp_pr.child("custGeom").map(|_| ("custom".to_owned(), None))
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
