//! The flow's [`Shaper`] on Pango: one `pango::Layout` per leaf, kept in a cache capped by
//! the text it holds (the least recently used go first; heights stay in the flow), styled
//! with the document's looks.
//!
//! Every run is drawn in its look: its paragraph style's with its own direct formatting
//! on top ([`StyleSheet::look`](document_model::StyleSheet::look)), family and size
//! included, whatever the format (Markdown and plain text have Slate's built-in sheet).
//! Pango resolves the family through fontconfig, so Calibri draws as Carlito where that
//! is installed; a Word document's fonts go to Pango as their [`Families`] give them
//! (embedded faces' families, fallbacks by class; ADR 0009). A paragraph's style also
//! gives its first-line indent and line spacing (its space and left indent are the
//! flow's), and its tab stops with the paragraph's own ([`Content::tabs`]); Pango draws no
//! leaders, so [`PangoShaper::leaders`] lays them out.
//! Slate's own looks go on top: code and raw source in monospace, links in the link
//! colour, a table's header row bold.

use super::{texture_key, Textures};
use crate::fonts::Families;
use document_model::{
    Align, Image, ImageSource, LineSpacing, Look as RunLook, RunStyle, StyleSheet, TabAlign,
    TabLeader,
};
use flow::{
    Content, Hit, LayoutKey, Look, Marker, MarkerWidth, Metrics, Shaper, Tabs, MONOSPACE,
    TWIPS_PER_PX,
};
use gtk::pango;
use gtk::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;

/// The cache's cap, in bytes of text laid out: room for every layout of a document of about
/// 300 pages, so a width change re-wraps them in place instead of shaping them again.
const CACHE_BYTES: usize = 2 << 20;
/// Colours as 0xRRGGBB.
const CODE_BACKGROUND: u32 = 0xEDEDED;
const RAW: u32 = 0x77767B;
const LINK: u32 = 0x1A5FB4;
/// A label's text is dimmed to this opacity (GTK's `dim-label`).
const DIM: f64 = 0.55;

/// A tab's leader laid out: its characters, where its top left goes in its paragraph's
/// layout (pixels; its baseline on the line's), and its run's colour (`None`: the page's
/// ink).
pub struct Leader {
    pub layout: pango::Layout,
    pub x: f32,
    pub y: f32,
    pub color: Option<u32>,
}

struct Entry {
    layout: pango::Layout,
    /// The width it is wrapped at, in pixels.
    width: i32,
    /// When it was last used.
    used: u64,
    bytes: usize,
}

/// An input method's preedit shown in a leaf's text: the layout made with it, apart from
/// the cache.
struct Preedit {
    key: LayoutKey,
    /// Where it goes in the leaf's text, in bytes.
    at: usize,
    text: String,
    attrs: pango::AttrList,
    layout: Option<Entry>,
}

/// Pango layouts for a document's leaves.
pub struct PangoShaper {
    context: pango::Context,
    /// The document's default style's font: every layout's, before its runs' looks.
    base: pango::FontDescription,
    /// Its family and size (half-points), which runs leave unset in their attributes.
    base_font: String,
    base_size: u16,
    /// What Pango is given for each of the document's font names.
    families: Families,
    textures: Textures,
    cache: HashMap<LayoutKey, Entry>,
    cached_bytes: usize,
    clock: u64,
    /// List markers laid out, by text and look.
    markers: HashMap<(String, RunLook), pango::Layout>,
    /// Markers' widths ([`MarkerWidth`]), by text and look: measured once, as every Word
    /// list item's layout and measure take them.
    widths: RefCell<HashMap<(String, RunLook), MarkerWidth>>,
    preedit: Option<Preedit>,
}

impl PangoShaper {
    pub fn new(
        context: pango::Context,
        styles: &StyleSheet,
        textures: Textures,
        families: Families,
    ) -> Self {
        let run = &styles.default_style().run;
        let mut base = pango::FontDescription::new();
        base.set_family(families.get(&run.font));
        base.set_size(half_points(run.size));
        PangoShaper {
            context,
            base,
            base_font: run.font.clone(),
            base_size: run.size,
            families,
            textures,
            cache: HashMap::new(),
            cached_bytes: 0,
            clock: 0,
            markers: HashMap::new(),
            widths: RefCell::default(),
            preedit: None,
        }
    }

    /// The document's font: the family and size text has before its looks.
    pub fn font(&self) -> pango::FontDescription {
        self.base.clone()
    }

    /// The decoded picture of `image`, if it has one.
    pub fn texture(&self, image: &Image) -> Option<&gtk::gdk::Texture> {
        match &image.source {
            ImageSource::Embedded { bytes, .. } => self.textures.get(&texture_key(bytes)),
            ImageSource::Link(_) => None,
        }
    }

    /// Show `text` with `attrs` (relative to it) underlined at byte `at` of the layout
    /// under `key`, until it is set again; `None` shows the text as it is.
    pub fn set_preedit(&mut self, preedit: Option<(LayoutKey, usize, &str, &pango::AttrList)>) {
        self.preedit = preedit.map(|(key, at, text, attrs)| {
            let attrs = attrs.copy().unwrap_or_default();
            let mut underline: pango::Attribute =
                pango::AttrInt::new_underline(pango::Underline::Single).into();
            underline.set_start_index(0);
            underline.set_end_index(text.len() as u32);
            attrs.insert(underline);
            Preedit {
                key,
                at,
                text: text.to_owned(),
                attrs,
                layout: None,
            }
        });
    }

    /// The layout cached under `key` for `content`, wrapped at `width`, made (again) if
    /// needed; with the preedit in it when the preedit is in this leaf.
    pub fn layout(&mut self, key: LayoutKey, content: Content<'_>, width: i32) -> &pango::Layout {
        if self.preedit.as_ref().is_some_and(|p| p.key == key) {
            return self.preedit_layout(content, width);
        }
        self.clock += 1;
        let clock = self.clock;
        if !self.cache.contains_key(&key) {
            let layout = self.make(content);
            let bytes = layout.text().len().max(1);
            self.cached_bytes += bytes;
            self.cache.insert(
                key,
                Entry {
                    layout,
                    width: -1,
                    used: clock,
                    bytes,
                },
            );
            if self.cached_bytes > CACHE_BYTES {
                self.evict(key);
            }
        }
        let entry = self.cache.get_mut(&key).expect("cached above");
        entry.used = clock;
        if entry.width != width {
            entry.layout.set_width(width * pango::SCALE);
            entry.width = width;
        }
        &entry.layout
    }

    /// The preedit's leaf laid out with the preedit in it.
    fn preedit_layout(&mut self, content: Content<'_>, width: i32) -> &pango::Layout {
        if self.preedit.as_ref().is_some_and(|p| p.layout.is_none()) {
            let layout = self.make(content);
            let p = self.preedit.as_mut().expect("checked above");
            let text = layout.text();
            let mut at = p.at.min(text.len());
            while !text.is_char_boundary(at) {
                at -= 1;
            }
            let mut joined = String::with_capacity(text.len() + p.text.len());
            joined.push_str(&text[..at]);
            joined.push_str(&p.text);
            joined.push_str(&text[at..]);
            let attrs = layout
                .attributes()
                .and_then(|a| a.copy())
                .unwrap_or_default();
            attrs.splice(&p.attrs, at as i32, p.text.len() as i32);
            layout.set_text(&joined);
            layout.set_attributes(Some(&attrs));
            p.layout = Some(Entry {
                layout,
                width: -1,
                used: 0,
                bytes: 0,
            });
        }
        let entry = self
            .preedit
            .as_mut()
            .and_then(|p| p.layout.as_mut())
            .expect("made above");
        if entry.width != width {
            entry.layout.set_width(width * pango::SCALE);
            entry.width = width;
        }
        &entry.layout
    }

    /// Drop the least recently used quarter of the layouts, never `keep`.
    fn evict(&mut self, keep: LayoutKey) {
        let mut used: Vec<u64> = self.cache.values().map(|e| e.used).collect();
        let cut = used.len() / 4;
        let (_, &mut oldest, _) = used.select_nth_unstable(cut);
        let mut freed = 0;
        self.cache.retain(|&id, e| {
            let drop = e.used <= oldest && id != keep;
            if drop {
                freed += e.bytes;
            }
            !drop
        });
        self.cached_bytes -= freed;
    }

    /// List item marker `marker` laid out (kept for drawing it again).
    pub fn marker(&mut self, marker: &Marker<'_>) -> &pango::Layout {
        let key = (marker.text.to_owned(), marker.look.clone());
        if !self.markers.contains_key(&key) {
            let layout = self.marker_layout(marker.text, &marker.look);
            self.markers.insert(key.clone(), layout);
        }
        &self.markers[&key]
    }

    /// `text` laid out in marker look `look`: its font, underline, and strikethrough (its
    /// colour is drawn).
    fn marker_layout(&self, text: &str, look: &RunLook) -> pango::Layout {
        let layout = pango::Layout::new(&self.context);
        layout.set_font_description(Some(&self.font_of(look)));
        let attrs = pango::AttrList::new();
        if look.underline {
            insert(
                &attrs,
                pango::AttrInt::new_underline(pango::Underline::Single),
                0,
                u32::MAX,
            );
        }
        if look.strike {
            insert(&attrs, pango::AttrInt::new_strikethrough(true), 0, u32::MAX);
        }
        layout.set_attributes(Some(&attrs));
        layout.set_text(text);
        layout
    }

    /// The width of `marker` and of a space in its look, in pixels: what places a Word
    /// list item's marker and text ([`Content::place`]).
    fn marker_widths(&self, marker: &Marker<'_>) -> MarkerWidth {
        let key = (marker.text.to_owned(), marker.look.clone());
        if let Some(&widths) = self.widths.borrow().get(&key) {
            return widths;
        }
        let width = |text: &str| {
            let layout = self.marker_layout(text, &marker.look);
            f64::from(layout.size().0) / f64::from(pango::SCALE)
        };
        let widths = MarkerWidth {
            marker: if marker.text.is_empty() {
                0.0
            } else {
                width(marker.text)
            },
            space: width(" "),
        };
        self.widths.borrow_mut().insert(key, widths);
        widths
    }

    /// Where `content`'s text goes ([`Content::place`]), its marker measured.
    fn place(&self, content: &Content<'_>) -> flow::Place {
        let widths = match content
            .measures_marker()
            .then(|| content.marker())
            .flatten()
        {
            Some(m) => self.marker_widths(&m),
            None => MarkerWidth::default(),
        };
        content.place(widths)
    }

    fn make(&self, content: Content<'_>) -> pango::Layout {
        let layout = pango::Layout::new(&self.context);
        layout.set_font_description(Some(&self.base));
        layout.set_wrap(pango::WrapMode::WordChar);
        let attrs = pango::AttrList::new();
        let text = content.text();
        layout.set_text(&text);
        let all = |a: pango::Attribute| insert(&attrs, a, 0, u32::MAX);
        match content {
            Content::Paragraph {
                paragraph,
                styles,
                header,
                ..
            } => {
                // The style's look over the whole paragraph (an empty one's line too), then
                // each run's overrides.
                let style = &styles.style_of(&paragraph.style).run;
                self.style_looks(style, &all);
                if header {
                    all(pango::AttrInt::new_weight(pango::Weight::Bold).into());
                }
                let mut start = 0;
                for run in &paragraph.runs {
                    let end = start + run.text.len();
                    run_looks(
                        style,
                        &run.style,
                        start as u32,
                        end as u32,
                        &attrs,
                        &self.families,
                    );
                    start = end;
                }
                match paragraph.align {
                    Align::Start => {}
                    Align::Center => layout.set_alignment(pango::Alignment::Center),
                    Align::End => layout.set_alignment(pango::Alignment::Right),
                    Align::Justify => layout.set_justify(true),
                }
                line_spacing(&layout, content.line_spacing(), &all);
                let place = self.place(&content);
                if place.first != 0 {
                    layout.set_indent(place.first * pango::SCALE);
                }
                // Only text with a tab needs its stops; the rest stays as cheap to shape.
                if text.contains('\t') {
                    if let Some(tabs) = content.tabs(place.left) {
                        layout.set_tabs(Some(&self.tab_array(&tabs, style)));
                    }
                }
            }
            Content::Code(_) => all(pango::AttrString::new_family(MONOSPACE).into()),
            Content::Raw(_) => {
                all(pango::AttrString::new_family(MONOSPACE).into());
                all(foreground(RAW));
            }
            Content::PictureLabel(_) => {
                all(pango::AttrInt::new_foreground_alpha((DIM * 65535.0) as u16).into())
            }
        }
        layout.set_attributes(Some(&attrs));
        layout
    }

    /// A paragraph style's run look `look`, through `all`: its family and size where they
    /// aren't the document's font's, and what it turns on.
    fn style_looks(&self, look: &RunLook, all: &impl Fn(pango::Attribute)) {
        if look.font != self.base_font {
            all(pango::AttrString::new_family(self.families.get(&look.font)).into());
        }
        if look.size != self.base_size {
            all(pango::AttrSize::new(half_points(look.size)).into());
        }
        if look.bold {
            all(pango::AttrInt::new_weight(pango::Weight::Bold).into());
        }
        if look.italic {
            all(pango::AttrInt::new_style(pango::Style::Italic).into());
        }
        if look.underline {
            all(pango::AttrInt::new_underline(pango::Underline::Single).into());
        }
        if look.strike {
            all(pango::AttrInt::new_strikethrough(true).into());
        }
        if let Some(rgb) = look.color {
            all(foreground(hex(rgb)));
        }
    }

    /// `tabs` as Pango's tab array, in Pango units; spaces measured in `look`'s font.
    fn tab_array(&self, tabs: &Tabs, look: &RunLook) -> pango::TabArray {
        let scale = f64::from(pango::SCALE);
        match tabs {
            Tabs::Spaces(n) => {
                let spaces = pango::Layout::new(&self.context);
                spaces.set_font_description(Some(&self.font_of(look)));
                spaces.set_text(&" ".repeat(*n as usize));
                let width = spaces.size().0.max(1);
                let mut array = pango::TabArray::new(1, false);
                array.set_tab(0, pango::TabAlign::Left, width);
                array
            }
            Tabs::Stops(stops) => {
                let mut array = pango::TabArray::new(stops.len() as i32, false);
                for (i, stop) in stops.iter().enumerate() {
                    let i = i as i32;
                    let align = match stop.align {
                        TabAlign::Left | TabAlign::Clear => pango::TabAlign::Left,
                        TabAlign::Center => pango::TabAlign::Center,
                        TabAlign::Right => pango::TabAlign::Right,
                        TabAlign::Decimal => pango::TabAlign::Decimal,
                    };
                    array.set_tab(i, align, (stop.x * scale).round() as i32);
                    if align == pango::TabAlign::Decimal {
                        array.set_decimal_point(i, '.');
                    }
                }
                array
            }
        }
    }

    /// The font of a run look: its family, size, weight, and style.
    fn font_of(&self, look: &RunLook) -> pango::FontDescription {
        let mut font = self.base.clone();
        font.set_family(self.families.get(&look.font));
        font.set_size(half_points(look.size));
        if look.bold {
            font.set_weight(pango::Weight::Bold);
        }
        if look.italic {
            font.set_style(pango::Style::Italic);
        }
        font
    }

    /// The leaders of `content`'s tabs in its `layout` (ADR 0009): each tab whose stop has
    /// a leader fills its gap, less half a leader character at each end, with the leader
    /// character in its run's font, on its line's baseline.
    pub fn leaders(&self, layout: &pango::Layout, content: Content<'_>) -> Vec<Leader> {
        let Content::Paragraph {
            paragraph, styles, ..
        } = content
        else {
            return Vec::new();
        };
        if !paragraph.runs.iter().any(|r| r.text.contains('\t')) {
            return Vec::new();
        }
        let Some(Tabs::Stops(stops)) = content.tabs(self.place(&content).left) else {
            return Vec::new();
        };
        if stops.iter().all(|s| s.leader == TabLeader::None) {
            return Vec::new();
        }
        let scale = f64::from(pango::SCALE);
        let mut out = Vec::new();
        let mut start = 0;
        for run in &paragraph.runs {
            for (i, _) in run.text.match_indices('\t') {
                let index = (start + i) as i32;
                let rect = layout.index_to_pos(index);
                let (x0, width) = (
                    f64::from(rect.x().min(rect.x() + rect.width())),
                    f64::from(rect.width().abs()),
                );
                // The stop the tab went to: the first at least a unit past where it starts,
                // as Pango picks it.
                let Some(stop) = stops.iter().find(|s| s.x * scale >= x0 + 1.0) else {
                    continue;
                };
                let text = match stop.leader {
                    TabLeader::None => continue,
                    TabLeader::Dot => ".",
                    TabLeader::MiddleDot => "·",
                    TabLeader::Hyphen => "-",
                    TabLeader::Underscore | TabLeader::Heavy => "_",
                };
                let mut look = styles.look(&paragraph.style, &run.style);
                look.bold |= stop.leader == TabLeader::Heavy;
                let leader = pango::Layout::new(&self.context);
                leader.set_font_description(Some(&self.font_of(&look)));
                leader.set_text(text);
                let one = f64::from(leader.size().0.max(1));
                let count = ((width - one) / one).floor();
                if count < 1.0 {
                    continue;
                }
                leader.set_text(&text.repeat(count as usize));
                let (line, _) = layout.index_to_line_x(index, false);
                let mut lines = layout.iter();
                for _ in 0..line {
                    lines.next_line();
                }
                let top = f64::from(lines.baseline()) - f64::from(leader.baseline());
                out.push(Leader {
                    x: ((x0 + width - one / 2.0 - count * one) / scale) as f32,
                    y: (top / scale) as f32,
                    layout: leader,
                    color: look.color.map(hex),
                });
            }
            start += run.text.len();
        }
        out
    }
}

/// `half` half-points in Pango units of points.
fn half_points(half: u16) -> i32 {
    i32::from(half) * pango::SCALE / 2
}

/// `twips` in Pango units of pixels at 96 dpi.
fn twips(twips: u32) -> i32 {
    (f64::from(twips) / TWIPS_PER_PX * f64::from(pango::SCALE)).round() as i32
}

/// Space `layout`'s lines as `line` says: a multiple of single spacing through Pango's line
/// spacing; an exact or least line height through the line-height attributes (`all`), so
/// every line, the first and last too, is that tall.
fn line_spacing(layout: &pango::Layout, line: LineSpacing, all: &impl Fn(pango::Attribute)) {
    match line {
        // Pango's own line height is single spacing (a factor of 1 would take the font's
        // height instead, a pixel less).
        LineSpacing::Auto(m) if (m - 1.0).abs() < 1e-3 => {}
        LineSpacing::Auto(m) => layout.set_line_spacing(m),
        LineSpacing::Exact(t) => all(pango::AttrInt::new_line_height_absolute(twips(t)).into()),
        // The larger of the absolute height and the font's own (a factor of 1).
        LineSpacing::AtLeast(t) => {
            all(pango::AttrFloat::new_line_height(1.0).into());
            all(pango::AttrInt::new_line_height_absolute(twips(t)).into());
        }
    }
}

fn insert(attrs: &pango::AttrList, attr: impl Into<pango::Attribute>, start: u32, end: u32) {
    let mut attr = attr.into();
    attr.set_start_index(start);
    attr.set_end_index(end);
    attrs.insert(attr);
}

fn channels(hex: u32) -> (u16, u16, u16) {
    let c = |shift: u32| ((hex >> shift) & 0xFF) as u16 * 257;
    (c(16), c(8), c(0))
}

/// A colour as the page's hex colours are written.
pub(super) fn hex([r, g, b]: [u8; 3]) -> u32 {
    u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)
}

fn foreground(hex: u32) -> pango::Attribute {
    let (r, g, b) = channels(hex);
    pango::AttrColor::new_foreground(r, g, b).into()
}

/// A run's looks over bytes `start..end`, over its paragraph style's look `style` (set on
/// the whole paragraph): the overrides `s` that differ from it (as
/// [`StyleSheet::look`] puts them on top), and Slate's own looks (code, raw source,
/// links). Font names go to Pango as `families` gives them.
fn run_looks(
    style: &RunLook,
    s: &RunStyle,
    start: u32,
    end: u32,
    attrs: &pango::AttrList,
    families: &Families,
) {
    let add = |a: pango::Attribute| insert(attrs, a, start, end);
    let changed = |set: Option<bool>, styled: bool| set.filter(|&on| on != styled);
    if let Some(font) = s.font.as_deref().filter(|&f| f != style.font) {
        add(pango::AttrString::new_family(families.get(font)).into());
    }
    if let Some(size) = s.size.filter(|&h| h != style.size) {
        add(pango::AttrSize::new(half_points(size)).into());
    }
    if let Some(on) = changed(s.bold, style.bold) {
        let weight = if on {
            pango::Weight::Bold
        } else {
            pango::Weight::Normal
        };
        add(pango::AttrInt::new_weight(weight).into());
    }
    if let Some(on) = changed(s.italic, style.italic) {
        let slant = if on {
            pango::Style::Italic
        } else {
            pango::Style::Normal
        };
        add(pango::AttrInt::new_style(slant).into());
    }
    if let Some(on) = changed(s.underline, style.underline) {
        let line = if on {
            pango::Underline::Single
        } else {
            pango::Underline::None
        };
        add(pango::AttrInt::new_underline(line).into());
    }
    if let Some(on) = changed(s.strike, style.strike) {
        add(pango::AttrInt::new_strikethrough(on).into());
    }
    if s.code {
        add(pango::AttrString::new_family(MONOSPACE).into());
        let (r, g, b) = channels(CODE_BACKGROUND);
        add(pango::AttrColor::new_background(r, g, b).into());
    }
    if s.raw {
        add(pango::AttrString::new_family(MONOSPACE).into());
        add(foreground(RAW));
    }
    if let Some(rgb) = s.color {
        add(foreground(hex(rgb)));
    }
    if s.link.is_some() {
        add(foreground(LINK));
        add(pango::AttrInt::new_underline(pango::Underline::Single).into());
    }
}

impl Shaper for PangoShaper {
    fn metrics(&mut self, look: &Look) -> Metrics {
        let mut font = self.base.clone();
        font.set_family(self.families.get(&look.font));
        font.set_size(half_points(look.size));
        if look.bold {
            font.set_weight(pango::Weight::Bold);
        }
        let m = self.context.metrics(Some(&font), None);
        Metrics {
            char_width: f64::from(m.approximate_char_width()) / f64::from(pango::SCALE),
            line_height: f64::from(m.height()) / f64::from(pango::SCALE),
        }
    }

    fn shape(&mut self, key: LayoutKey, content: Content<'_>, width: i32) -> i32 {
        self.layout(key, content, width).pixel_size().1
    }

    fn hit(&mut self, key: LayoutKey, content: Content<'_>, width: i32, x: f64, y: f64) -> Hit {
        let layout = self.layout(key, content, width);
        let scale = f64::from(pango::SCALE);
        let (inside, index, trailing) =
            layout.xy_to_index((x * scale).round() as i32, (y * scale).round() as i32);
        Hit {
            index: index.max(0) as usize,
            trailing: trailing > 0,
            inside,
        }
    }

    fn picture_size(&mut self, image: &Image) -> Option<(f32, f32)> {
        self.texture(image)
            .map(|t| (t.width() as f32, t.height() as f32))
    }

    fn marker_width(&mut self, marker: &Marker<'_>) -> MarkerWidth {
        self.marker_widths(marker)
    }

    fn forget(&mut self, key: LayoutKey) {
        if let Some(entry) = self.cache.remove(&key) {
            self.cached_bytes -= entry.bytes;
        }
        if self.preedit.as_ref().is_some_and(|p| p.key == key) {
            self.preedit = None;
        }
    }
}
