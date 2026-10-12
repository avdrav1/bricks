//! The flow's [`Shaper`] on Pango: one `pango::Layout` per leaf, kept in a cache capped by
//! the text it holds (the least recently used go first; heights stay in the flow), styled
//! with the viewer's looks.
//!
//! Markdown and plain text take their looks from the paragraph style (headings larger and
//! bold, quotes italic and grey, a table's header row bold); a DOCX file's runs already carry
//! the sizes and weights its styles give them, so its paragraphs get only the runs' looks.

use super::{texture_key, Textures};
use document_model::{
    Align, Format, Image, ImageSource, ListKind, Paragraph, ParagraphStyle, RunStyle,
};
use flow::{Content, Hit, LeafId, Look, Metrics, Shaper};
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

/// Pango layouts for a document's leaves.
pub struct PangoShaper {
    context: pango::Context,
    /// Looks come from paragraph styles (Markdown, plain text), not from the runs (DOCX).
    style_looks: bool,
    textures: Textures,
    cache: HashMap<LeafId, Entry>,
    cached_bytes: usize,
    clock: u64,
    markers: HashMap<String, pango::Layout>,
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
        }
    }

    /// The decoded picture of `image`, if it has one.
    pub fn texture(&self, image: &Image) -> Option<&gtk::gdk::Texture> {
        match &image.source {
            ImageSource::Embedded { bytes, .. } => self.textures.get(&texture_key(bytes)),
            ImageSource::Link(_) => None,
        }
    }

    /// `leaf`'s layout wrapped at `width`, made (again) if needed.
    pub fn layout(&mut self, leaf: LeafId, content: Content<'_>, width: i32) -> &pango::Layout {
        self.clock += 1;
        let clock = self.clock;
        if !self.cache.contains_key(&leaf) {
            let layout = self.make(content);
            let bytes = layout.text().len().max(1);
            self.cached_bytes += bytes;
            self.cache.insert(
                leaf,
                Entry {
                    layout,
                    width: -1,
                    used: clock,
                    bytes,
                },
            );
            if self.cached_bytes > CACHE_BYTES {
                self.evict(leaf);
            }
        }
        let entry = self.cache.get_mut(&leaf).expect("cached above");
        entry.used = clock;
        if entry.width != width {
            entry.layout.set_width(width * pango::SCALE);
            entry.width = width;
        }
        &entry.layout
    }

    /// Drop the least recently used quarter of the layouts, never `keep`.
    fn evict(&mut self, keep: LeafId) {
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
                paragraph, header, ..
            } => {
                self.paragraph_looks(paragraph, header, &attrs);
                let mut start = 0;
                for run in &paragraph.runs {
                    let end = start + run.text.len();
                    run_looks(&run.style, start as u32, end as u32, &attrs);
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

    fn paragraph_looks(&self, p: &Paragraph, header: bool, attrs: &pango::AttrList) {
        if !self.style_looks {
            return;
        }
        let all = |a: pango::Attribute| insert(attrs, a, 0, u32::MAX);
        match p.style {
            ParagraphStyle::Heading(n) => {
                all(
                    pango::AttrFloat::new_scale(HEADING_SCALE[usize::from(n.clamp(1, 6)) - 1])
                        .into(),
                );
                all(pango::AttrInt::new_weight(pango::Weight::Bold).into());
            }
            ParagraphStyle::Title => {
                all(pango::AttrFloat::new_scale(TITLE_SCALE).into());
                all(pango::AttrInt::new_weight(pango::Weight::Bold).into());
            }
            ParagraphStyle::Quote => {
                all(pango::AttrInt::new_style(pango::Style::Italic).into());
                all(foreground(QUOTE));
            }
            ParagraphStyle::Normal => {}
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

/// A run's looks over bytes `start..end`.
fn run_looks(s: &RunStyle, start: u32, end: u32, attrs: &pango::AttrList) {
    let add = |a: pango::Attribute| insert(attrs, a, start, end);
    if s.bold {
        add(pango::AttrInt::new_weight(pango::Weight::Bold).into());
    }
    if s.italic {
        add(pango::AttrInt::new_style(pango::Style::Italic).into());
    }
    if s.underline {
        add(pango::AttrInt::new_underline(pango::Underline::Single).into());
    }
    if s.strike {
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
    if let Some(half) = s.size {
        add(pango::AttrSize::new(i32::from(half) * pango::SCALE / 2).into());
    }
    if let Some([r, g, b]) = s.color {
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

    fn shape(&mut self, leaf: LeafId, content: Content<'_>, width: i32) -> i32 {
        self.layout(leaf, content, width).pixel_size().1
    }

    fn hit(&mut self, leaf: LeafId, content: Content<'_>, width: i32, x: f64, y: f64) -> Hit {
        let layout = self.layout(leaf, content, width);
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
}
