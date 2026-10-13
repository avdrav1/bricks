//! The flow's [`Shaper`] on Pango: one `pango::Layout` per leaf, kept in a cache capped by
//! the text it holds (the least recently used go first; heights stay in the flow), styled
//! with the viewer's looks.
//!
//! Markdown and plain text take their paragraph looks from the paragraph style (headings
//! larger and bold, quotes italic and grey, a table's header row bold); a DOCX file's
//! styles give its runs their sizes and colours. Every run is drawn in its style's look
//! with its own overrides on top (weight, slant, underline, strikethrough); the page's
//! font family stays.

use super::{texture_key, Textures};
use document_model::{
    Align, Format, Image, ImageSource, ListKind, Look as RunLook, Paragraph, RunStyle, StyleSheet,
};
use flow::{Content, Hit, LayoutKey, Look, Metrics, Shaper};
use gtk::pango;
use gtk::prelude::*;
use std::collections::HashMap;

/// The cache's cap, in bytes of text laid out: room for every layout of a document of about
/// 300 pages, so a width change re-wraps them in place instead of shaping them again.
const CACHE_BYTES: usize = 2 << 20;
/// Heading sizes relative to body text, levels 1 to 6; and the title's.
const HEADING_SCALE: [f64; 6] = [2.0, 1.6, 1.35, 1.2, 1.1, 1.0];
const TITLE_SCALE: f64 = 2.4;
const MONO: &str = "monospace";
/// Colours as 0xRRGGBB.
const QUOTE: u32 = 0x5E5C64;
const CODE_BACKGROUND: u32 = 0xEDEDED;
const RAW: u32 = 0x77767B;
const LINK: u32 = 0x1A5FB4;
/// A label's text is dimmed to this opacity (GTK's `dim-label`).
const DIM: f64 = 0.55;

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
    /// Paragraph looks scale the page font by the paragraph's kind of style (Markdown,
    /// plain text); a Word document's runs take their sizes from its styles.
    style_looks: bool,
    textures: Textures,
    cache: HashMap<LayoutKey, Entry>,
    cached_bytes: usize,
    clock: u64,
    markers: HashMap<String, pango::Layout>,
    preedit: Option<Preedit>,
}

impl PangoShaper {
    pub fn new(context: pango::Context, format: &Format, textures: Textures) -> Self {
        PangoShaper {
            context,
            style_looks: !matches!(format, Format::Docx),
            textures,
            cache: HashMap::new(),
            cached_bytes: 0,
            clock: 0,
            markers: HashMap::new(),
            preedit: None,
        }
    }

    /// The page's font: the family and size text has before its looks.
    pub fn font(&self) -> pango::FontDescription {
        self.context.font_description().unwrap_or_default()
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

    /// The marker of a list item, laid out.
    pub fn marker(&mut self, item: &document_model::ListItem) -> &pango::Layout {
        let text = match &item.kind {
            ListKind::Bullet => ["•", "◦", "▪"][usize::from(item.level) % 3],
            ListKind::Ordered { label, .. } => label,
            ListKind::Task { checked: true } => "☑",
            ListKind::Task { checked: false } => "☐",
        };
        let context = &self.context;
        self.markers.entry(text.to_owned()).or_insert_with(|| {
            let layout = pango::Layout::new(context);
            layout.set_text(text);
            layout
        })
    }

    fn make(&self, content: Content<'_>) -> pango::Layout {
        let layout = pango::Layout::new(&self.context);
        layout.set_wrap(pango::WrapMode::WordChar);
        let attrs = pango::AttrList::new();
        let text = content.text();
        layout.set_text(&text);
        match content {
            Content::Paragraph {
                paragraph,
                styles,
                header,
                ..
            } => {
                self.paragraph_looks(paragraph, styles, header, &attrs);
                let style = &styles.style_of(&paragraph.style).run;
                let sized = !self.style_looks;
                let mut start = 0;
                for run in &paragraph.runs {
                    let end = start + run.text.len();
                    run_looks(style, &run.style, sized, start as u32, end as u32, &attrs);
                    start = end;
                }
                match paragraph.align {
                    Align::Start => {}
                    Align::Center => layout.set_alignment(pango::Alignment::Center),
                    Align::End => layout.set_alignment(pango::Alignment::Right),
                    Align::Justify => layout.set_justify(true),
                }
            }
            Content::Code(_) => insert(&attrs, pango::AttrString::new_family(MONO), 0, u32::MAX),
            Content::Raw(_) => {
                insert(&attrs, pango::AttrString::new_family(MONO), 0, u32::MAX);
                insert(&attrs, foreground(RAW), 0, u32::MAX);
            }
            Content::PictureLabel(_) => insert(
                &attrs,
                pango::AttrInt::new_foreground_alpha((DIM * 65535.0) as u16),
                0,
                u32::MAX,
            ),
        }
        layout.set_attributes(Some(&attrs));
        layout
    }

    fn paragraph_looks(
        &self,
        p: &Paragraph,
        styles: &StyleSheet,
        header: bool,
        attrs: &pango::AttrList,
    ) {
        if !self.style_looks {
            return;
        }
        let all = |a: pango::Attribute| insert(attrs, a, 0, u32::MAX);
        let style = styles.style_of(&p.style);
        if style.title {
            all(pango::AttrFloat::new_scale(TITLE_SCALE).into());
            all(pango::AttrInt::new_weight(pango::Weight::Bold).into());
        } else if let Some(n) = style.outline {
            all(pango::AttrFloat::new_scale(HEADING_SCALE[usize::from(n.clamp(1, 6)) - 1]).into());
            all(pango::AttrInt::new_weight(pango::Weight::Bold).into());
        } else if style.is_quote() {
            all(pango::AttrInt::new_style(pango::Style::Italic).into());
            all(foreground(QUOTE));
        }
        if header {
            all(pango::AttrInt::new_weight(pango::Weight::Bold).into());
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

fn foreground(hex: u32) -> pango::Attribute {
    let (r, g, b) = channels(hex);
    pango::AttrColor::new_foreground(r, g, b).into()
}

/// A run's looks over bytes `start..end`: its style's look `style` with its overrides `s`
/// on top (as [`StyleSheet::look`]), its size only when `sized` (a Word document's;
/// elsewhere the paragraph's scale sizes it), and Slate's own looks (code, raw source,
/// links).
fn run_looks(
    style: &RunLook,
    s: &RunStyle,
    sized: bool,
    start: u32,
    end: u32,
    attrs: &pango::AttrList,
) {
    let add = |a: pango::Attribute| insert(attrs, a, start, end);
    if s.bold.unwrap_or(style.bold) {
        add(pango::AttrInt::new_weight(pango::Weight::Bold).into());
    }
    if s.italic.unwrap_or(style.italic) {
        add(pango::AttrInt::new_style(pango::Style::Italic).into());
    }
    if s.underline.unwrap_or(style.underline) {
        add(pango::AttrInt::new_underline(pango::Underline::Single).into());
    }
    if s.strike.unwrap_or(style.strike) {
        add(pango::AttrInt::new_strikethrough(true).into());
    }
    if s.code {
        add(pango::AttrString::new_family(MONO).into());
        let (r, g, b) = channels(CODE_BACKGROUND);
        add(pango::AttrColor::new_background(r, g, b).into());
    }
    if s.raw {
        add(pango::AttrString::new_family(MONO).into());
        add(foreground(RAW));
    }
    if sized {
        let half = s.size.unwrap_or(style.size);
        add(pango::AttrSize::new(i32::from(half) * pango::SCALE / 2).into());
    }
    if let Some([r, g, b]) = s.color.or(style.color) {
        add(foreground(
            u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b),
        ));
    }
    if s.link.is_some() {
        add(foreground(LINK));
        add(pango::AttrInt::new_underline(pango::Underline::Single).into());
    }
}

impl Shaper for PangoShaper {
    fn metrics(&mut self, look: Look) -> Metrics {
        let mut font = self.context.font_description().unwrap_or_default();
        let scale = match look {
            Look::Title if self.style_looks => TITLE_SCALE,
            Look::Heading(n) if self.style_looks => HEADING_SCALE[usize::from(n.clamp(1, 6)) - 1],
            _ => 1.0,
        };
        if scale != 1.0 {
            font.set_weight(pango::Weight::Bold);
            font.set_size((f64::from(font.size()) * scale).round() as i32);
        }
        if look == Look::Mono {
            font.set_family(MONO);
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

    fn forget(&mut self, key: LayoutKey) {
        if let Some(entry) = self.cache.remove(&key) {
            self.cached_bytes -= entry.bytes;
        }
        if self.preedit.as_ref().is_some_and(|p| p.key == key) {
            self.preedit = None;
        }
    }
}
