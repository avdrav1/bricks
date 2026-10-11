//! The SL0 viewer's document column (ADR 0003, ADR 0005): a document drawn into a
//! read-only `GtkTextView`.
//!
//! Paragraph and run looks become text tags, one per distinct look. Tables are embedded
//! `gtk::Grid`s whose cells are text views drawn the same way, pictures embedded
//! `gtk::Picture`s scaled to fit the column, and page breaks and rules separators. Slate is
//! pageless: the text column is as wide as the user's text width setting (narrower when the
//! window is) and centred; the view's margins grow with the window, the tags that indent
//! paragraphs follow them, and the embedded widgets are resized to the column.
//!
//! Markdown and plain text take their looks from the paragraph style (headings larger and
//! bold, quotes indented and italic); a DOCX file's runs already carry the sizes and weights
//! its styles give them, so its headings only get their spacing here.

use crate::settings::TextWidth;
use document_model::{
    Align, Block, CodeBlock, Document, Format, Image, ImageSource, ListKind, Paragraph,
    ParagraphStyle, Raw, Run, Table,
};
use gtk::prelude::*;
use gtk::{gdk, pango};
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::Arc;

/// The smallest margin beside the column, when the window is narrower than the text width
/// (and on each side of a Full column).
const MIN_MARGIN: i32 = 24;
/// The narrowest the column gets, however narrow the window.
const MIN_COLUMN: i32 = 120;
/// How far each list or quote level indents.
const LEVEL_INDENT: i32 = 28;
/// Space below a paragraph, and below a list item followed by another.
const PARAGRAPH_SPACE: i32 = 8;
const ITEM_SPACE: i32 = 3;
/// Space inside a table cell's border, left and right (half that above and below).
const CELL_PADDING: i32 = 6;
/// Heading sizes relative to body text, levels 1 to 6; and the title's.
const HEADING_SCALE: [f64; 6] = [2.0, 1.6, 1.35, 1.2, 1.1, 1.0];
const TITLE_SCALE: f64 = 2.4;
/// Pango's bold weight.
const BOLD: i32 = 700;

/// Decoded pictures, by the address of their bytes in the document.
pub type Textures = HashMap<usize, gdk::Texture>;

/// The key of an embedded picture's bytes in [`Textures`].
pub fn texture_key(bytes: &Arc<[u8]>) -> usize {
    Arc::as_ptr(bytes) as *const u8 as usize
}

/// The text column's width for `setting` in a view `view_width` pixels wide: the setting's
/// width, or the view's less a margin each side when that is narrower (Full always), but
/// never under [`MIN_COLUMN`].
pub fn column_width(setting: TextWidth, view_width: i32) -> i32 {
    let room = view_width - 2 * MIN_MARGIN;
    setting
        .pixels()
        .map_or(room, |px| px.min(room))
        .max(MIN_COLUMN)
}

/// A document's column: the text view, the tags whose indent follows its margin, and the
/// embedded widgets sized by its width.
pub struct Column {
    pub view: gtk::TextView,
    margin_tags: Vec<(gtk::TextTag, i32)>,
    embedded: Vec<Embedded>,
    setting: Cell<TextWidth>,
    /// The view's width at the last [`Column::fit`]; 0 before the first.
    view_width: Cell<i32>,
    /// The width the embedded widgets are sized to.
    width: Cell<i32>,
}

impl Column {
    pub fn new(doc: &Document, textures: &Textures, setting: TextWidth) -> Self {
        let view = text_view("slate-page");
        view.set_top_margin(40);
        view.set_bottom_margin(64);
        let style_looks = !matches!(doc.format, Format::Docx);
        let plain = matches!(doc.format, Format::Text(_));
        let mut r = Renderer::new(&view, textures, style_looks, plain);
        r.render(&doc.blocks);
        let column = Column {
            view,
            margin_tags: r.margin_tags,
            embedded: r.embedded,
            setting: Cell::new(setting),
            view_width: Cell::new(0),
            width: Cell::new(0),
        };
        column.set_margin(MIN_MARGIN);
        column.resize(setting.pixels().unwrap_or(MIN_COLUMN));
        column
    }

    /// Centre the column in a view `view_width` pixels wide.
    pub fn fit(&self, view_width: i32) {
        self.view_width.set(view_width);
        let width = column_width(self.setting.get(), view_width);
        self.set_margin(((view_width - width) / 2).max(MIN_MARGIN));
        self.resize(width);
    }

    /// Follow a new text width setting.
    pub fn set_text_width(&self, setting: TextWidth) {
        self.setting.set(setting);
        match self.view_width.get() {
            0 => self.resize(setting.pixels().unwrap_or(MIN_COLUMN)),
            w => self.fit(w),
        }
    }

    fn set_margin(&self, margin: i32) {
        if self.view.left_margin() == margin {
            return;
        }
        self.view.set_left_margin(margin);
        self.view.set_right_margin(margin);
        for (tag, extra) in &self.margin_tags {
            tag.set_left_margin(margin + extra);
        }
    }

    fn resize(&self, width: i32) {
        if self.width.replace(width) != width {
            resize(&self.embedded, width);
        }
    }
}

/// A widget embedded in a text view, sized by the view's column.
enum Embedded {
    /// A page break's or rule's line: the column's width.
    Line(gtk::Separator),
    /// A picture at `width` x `height`, scaled down to fit.
    Picture(gtk::Picture, f32, f32),
    /// A table of `columns` homogeneous columns filling the column.
    Table { columns: i32, cells: Vec<TableCell> },
}

struct TableCell {
    view: gtk::TextView,
    span: i32,
    embedded: Vec<Embedded>,
}

/// Size `embedded` to a column `width` pixels wide.
fn resize(embedded: &[Embedded], width: i32) {
    for e in embedded {
        match e {
            Embedded::Line(line) => line.set_size_request(width, -1),
            Embedded::Picture(picture, w, h) => {
                let shown = w.min(width as f32).max(1.0);
                let height = (h * shown / w.max(1.0)).max(1.0);
                picture.set_size_request(shown.round() as i32, height.round() as i32);
            }
            Embedded::Table { columns, cells } => {
                let column = (width / columns).max(24);
                for cell in cells {
                    cell.view.set_size_request(column * cell.span, -1);
                    // Inside the cell's padding and its 1px border (main.rs CSS).
                    resize(&cell.embedded, column * cell.span - 2 * CELL_PADDING - 2);
                }
            }
        }
    }
}

/// A read-only, selectable text view that opens links on Ctrl+click.
fn text_view(class: &str) -> gtk::TextView {
    let view = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .wrap_mode(gtk::WrapMode::WordChar)
        .css_classes([class])
        .build();
    follow_links(&view);
    view
}

/// The link under buffer position (`x`, `y`) of `view`'s widget coordinates.
fn link_at(view: &gtk::TextView, x: f64, y: f64) -> Option<String> {
    let (bx, by) = view.window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
    let iter = view.iter_at_location(bx, by)?;
    iter.tags().iter().find_map(|t| {
        t.name()
            .and_then(|n| n.strip_prefix("link:").map(str::to_owned))
    })
}

fn follow_links(view: &gtk::TextView) {
    let click = gtk::GestureClick::new();
    click.connect_released(|gesture, _, x, y| {
        let Some(view) = gesture.widget().and_downcast::<gtk::TextView>() else {
            return;
        };
        if !gesture
            .current_event_state()
            .contains(gdk::ModifierType::CONTROL_MASK)
        {
            return;
        }
        if let Some(url) = link_at(&view, x, y) {
            let window = view.root().and_downcast::<gtk::Window>();
            gtk::UriLauncher::new(&url).launch(
                window.as_ref(),
                gtk::gio::Cancellable::NONE,
                move |result| {
                    if let Err(e) = result {
                        tracing::warn!("cannot open link {url}: {e}");
                    }
                },
            );
        }
    });
    view.add_controller(click);
    let motion = gtk::EventControllerMotion::new();
    motion.connect_motion(|motion, x, y| {
        if let Some(view) = motion.widget().and_downcast::<gtk::TextView>() {
            let cursor = if link_at(&view, x, y).is_some() {
                "pointer"
            } else {
                "text"
            };
            view.set_cursor_from_name(Some(cursor));
        }
    });
    view.add_controller(motion);
}

fn rgba(hex: u32) -> gdk::RGBA {
    gdk::RGBA::new(
        ((hex >> 16) & 0xFF) as f32 / 255.0,
        ((hex >> 8) & 0xFF) as f32 / 255.0,
        (hex & 0xFF) as f32 / 255.0,
        1.0,
    )
}

struct Renderer<'a> {
    view: gtk::TextView,
    buffer: gtk::TextBuffer,
    textures: &'a Textures,
    tags: HashMap<String, gtk::TextTag>,
    /// Tags that indent, with how far beyond the view's margin.
    margin_tags: Vec<(gtk::TextTag, i32)>,
    /// Looks come from paragraph styles (Markdown, plain text), not from the runs (DOCX).
    style_looks: bool,
    /// Plain text: lines, without space between paragraphs.
    plain: bool,
    /// Embedded widgets sized by the column, drawn so far.
    embedded: Vec<Embedded>,
    /// Every run bold: a Markdown table's header row.
    all_bold: bool,
    /// Blocks drawn so far: each after the first starts a new line.
    started: bool,
}

impl<'a> Renderer<'a> {
    fn new(view: &gtk::TextView, textures: &'a Textures, style_looks: bool, plain: bool) -> Self {
        Self {
            view: view.clone(),
            buffer: view.buffer(),
            textures,
            tags: HashMap::new(),
            margin_tags: Vec::new(),
            style_looks,
            plain,
            embedded: Vec::new(),
            all_bold: false,
            started: false,
        }
    }

    /// The tag called `name`, made by `make` the first time.
    fn tag(&mut self, name: &str, make: impl FnOnce(gtk::TextTag) -> gtk::TextTag) -> gtk::TextTag {
        if let Some(t) = self.tags.get(name) {
            return t.clone();
        }
        let tag = make(gtk::TextTag::new(Some(name)));
        self.buffer.tag_table().add(&tag);
        self.tags.insert(name.to_owned(), tag.clone());
        tag
    }

    /// A tag indenting paragraphs `extra` pixels beyond the margin, the first line
    /// `hang` pixels less (for a list marker).
    fn margin(&mut self, extra: i32, hang: i32) -> gtk::TextTag {
        let name = format!("margin {extra} {hang}");
        if let Some(t) = self.tags.get(&name) {
            return t.clone();
        }
        let base = self.view.left_margin();
        let tag = self.tag(&name, |t| {
            t.set_left_margin(base + extra);
            t.set_indent(-hang);
            if hang > 0 {
                // The text after the marker starts where the wrapped lines do.
                let mut tabs = pango::TabArray::new(1, true);
                tabs.set_tab(0, pango::TabAlign::Left, hang);
                t.set_tabs(Some(&tabs));
            }
            t
        });
        self.margin_tags.push((tag.clone(), extra));
        tag
    }

    fn end(&self) -> gtk::TextIter {
        self.buffer.end_iter()
    }

    /// Start a new line unless this is the first block.
    fn next_line(&mut self) {
        if self.started {
            self.buffer.insert(&mut self.end(), "\n");
        }
        self.started = true;
    }

    fn render(&mut self, blocks: &[Block]) {
        for (i, block) in blocks.iter().enumerate() {
            self.block(block, blocks.get(i + 1));
        }
    }

    /// Draw `block`; `next` is the block after it, if any.
    fn block(&mut self, block: &Block, next: Option<&Block>) {
        match block {
            Block::Paragraph(p) => self.paragraph(p, None, PARAGRAPH_SPACE),
            Block::ListItem(item) => {
                // A list's items sit close together; the list ends with a paragraph's space.
                let below = if matches!(next, Some(Block::ListItem(_))) {
                    ITEM_SPACE
                } else {
                    PARAGRAPH_SPACE
                };
                self.paragraph(&item.paragraph, Some((&item.kind, item.level)), below)
            }
            Block::CodeBlock(code) => self.code(code),
            Block::Raw(raw) => self.raw(raw),
            Block::Table(table) => {
                let (grid, embedded) = self.table(table);
                self.embed(&grid);
                self.embedded.push(embedded);
            }
            Block::Image(image) => {
                let picture = self.image(image);
                self.embed(&picture);
            }
            Block::PageBreak => {
                let line = gtk::Separator::new(gtk::Orientation::Horizontal);
                line.add_css_class("page-break");
                line.update_property(&[gtk::accessible::Property::Label("Page break")]);
                self.embed(&line);
                self.embedded.push(Embedded::Line(line));
            }
            Block::Rule => {
                let line = gtk::Separator::new(gtk::Orientation::Horizontal);
                line.add_css_class("rule");
                self.embed(&line);
                self.embedded.push(Embedded::Line(line));
            }
        }
    }

    /// A widget on a line of its own.
    fn embed(&mut self, widget: &impl IsA<gtk::Widget>) {
        self.next_line();
        let anchor = self.buffer.create_child_anchor(&mut self.end());
        self.view.add_child_at_anchor(widget, &anchor);
    }

    fn paragraph(&mut self, p: &Paragraph, list: Option<(&ListKind, u8)>, below: i32) {
        self.next_line();
        let mut tags = Vec::new();
        let mut extra = i32::from(p.indent) * LEVEL_INDENT;
        let mut look = None;
        match p.style {
            ParagraphStyle::Heading(n) => {
                let i = usize::from(n.clamp(1, 6)) - 1;
                tags.push(self.tag(&format!("h{n} space"), |t| {
                    t.set_pixels_above_lines([18, 14, 12, 10, 8, 8][i]);
                    t.set_pixels_below_lines(6);
                    t
                }));
                if self.style_looks {
                    look = Some(self.tag(&format!("h{n}"), |t| {
                        t.set_scale(HEADING_SCALE[i]);
                        t.set_weight(BOLD);
                        t
                    }));
                }
            }
            ParagraphStyle::Title => {
                tags.push(self.tag("title space", |t| {
                    t.set_pixels_below_lines(12);
                    t
                }));
                if self.style_looks {
                    look = Some(self.tag("title", |t| {
                        t.set_scale(TITLE_SCALE);
                        t.set_weight(BOLD);
                        t
                    }));
                }
            }
            ParagraphStyle::Quote => {
                extra += LEVEL_INDENT;
                if self.style_looks {
                    look = Some(self.tag("quote", |t| {
                        t.set_style(pango::Style::Italic);
                        t.set_foreground_rgba(Some(&rgba(0x5E5C64)));
                        t
                    }));
                }
            }
            ParagraphStyle::Normal => {}
        }
        if !self.plain {
            tags.push(self.tag(&format!("below {below}"), |t| {
                t.set_pixels_below_lines(below);
                t
            }));
        }
        match p.align {
            Align::Start => {}
            Align::Center => tags.push(self.tag("centre", |t| {
                t.set_justification(gtk::Justification::Center);
                t
            })),
            Align::End => tags.push(self.tag("right", |t| {
                t.set_justification(gtk::Justification::Right);
                t
            })),
            Align::Justify => tags.push(self.tag("justify", |t| {
                t.set_justification(gtk::Justification::Fill);
                t
            })),
        }
        let marker = list.map(|(kind, level)| {
            extra = i32::from(level) * LEVEL_INDENT + LEVEL_INDENT;
            match kind {
                ListKind::Bullet => ["•", "◦", "▪"][usize::from(level) % 3].to_owned(),
                ListKind::Ordered { label, .. } => label.clone(),
                ListKind::Task { checked: true } => "☑".to_owned(),
                ListKind::Task { checked: false } => "☐".to_owned(),
            }
        });
        let hang = if marker.is_some() { LEVEL_INDENT } else { 0 };
        if extra > 0 || hang > 0 {
            tags.push(self.margin(extra, hang));
        }
        if let Some(look) = look {
            tags.push(look);
        }
        if let Some(marker) = marker {
            let mut marker_tags = tags.clone();
            marker_tags.push(self.tag("marker", |t| {
                t.set_foreground_rgba(Some(&rgba(0x3D3846)));
                t
            }));
            let refs: Vec<&gtk::TextTag> = marker_tags.iter().collect();
            self.buffer
                .insert_with_tags(&mut self.end(), &format!("{marker}\t"), &refs);
        }
        for run in &p.runs {
            let mut run_tags = tags.clone();
            self.run_tags(run, &mut run_tags);
            let refs: Vec<&gtk::TextTag> = run_tags.iter().collect();
            self.buffer
                .insert_with_tags(&mut self.end(), &run.text, &refs);
        }
    }

    fn run_tags(&mut self, run: &Run, tags: &mut Vec<gtk::TextTag>) {
        let s = &run.style;
        if s.bold || self.all_bold {
            tags.push(self.tag("b", |t| {
                t.set_weight(BOLD);
                t
            }));
        }
        if s.italic {
            tags.push(self.tag("i", |t| {
                t.set_style(pango::Style::Italic);
                t
            }));
        }
        if s.underline {
            tags.push(self.tag("u", |t| {
                t.set_underline(pango::Underline::Single);
                t
            }));
        }
        if s.strike {
            tags.push(self.tag("s", |t| {
                t.set_strikethrough(true);
                t
            }));
        }
        if s.code {
            tags.push(self.tag("code", |t| {
                t.set_family(Some("monospace"));
                t.set_background_rgba(Some(&rgba(0xEDEDED)));
                t
            }));
        }
        if s.raw {
            tags.push(self.tag("raw run", |t| {
                t.set_family(Some("monospace"));
                t.set_foreground_rgba(Some(&rgba(0x77767B)));
                t
            }));
        }
        if let Some(half) = s.size {
            tags.push(self.tag(&format!("size {half}"), |t| {
                t.set_size_points(f64::from(half) / 2.0);
                t
            }));
        }
        if let Some([r, g, b]) = s.color {
            let hex = u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b);
            tags.push(self.tag(&format!("colour {hex:06x}"), |t| {
                t.set_foreground_rgba(Some(&rgba(hex)));
                t
            }));
        }
        if let Some(url) = &s.link {
            tags.push(self.tag(&format!("link:{url}"), |t| {
                t.set_foreground_rgba(Some(&rgba(0x1A5FB4)));
                t.set_underline(pango::Underline::Single);
                t
            }));
        }
    }

    fn code(&mut self, code: &CodeBlock) {
        self.next_line();
        let margin = self.margin(i32::from(code.indent) * LEVEL_INDENT + 12, 0);
        let look = self.tag("code block", |t| {
            t.set_family(Some("monospace"));
            t.set_paragraph_background_rgba(Some(&rgba(0xF2F2F2)));
            t.set_pixels_below_lines(0);
            t
        });
        let first = self.tag("code block first", |t| {
            t.set_pixels_above_lines(4);
            t
        });
        let last = self.tag("code block last", |t| {
            t.set_pixels_below_lines(PARAGRAPH_SPACE);
            t
        });
        let text = code.text.strip_suffix('\n').unwrap_or(&code.text);
        let text = if text.is_empty() { " " } else { text };
        let start = self.end().offset();
        self.buffer
            .insert_with_tags(&mut self.end(), text, &[&look, &margin]);
        // The block's first line has room above it and its last a paragraph's space below.
        let first_start = self.buffer.iter_at_offset(start);
        let mut first_end = first_start;
        first_end.forward_to_line_end();
        self.buffer.apply_tag(&first, &first_start, &first_end);
        let last_end = self.end();
        let mut last_start = last_end;
        last_start.set_line_offset(0);
        self.buffer.apply_tag(&last, &last_start, &last_end);
    }

    fn raw(&mut self, raw: &Raw) {
        self.next_line();
        let look = self.tag("raw", |t| {
            t.set_family(Some("monospace"));
            t.set_foreground_rgba(Some(&rgba(0x77767B)));
            t.set_pixels_below_lines(8);
            t
        });
        self.buffer
            .insert_with_tags(&mut self.end(), &raw.source, &[&look]);
    }

    /// A table as a grid of cells, each a text view drawn like the column; sized by
    /// [`resize`].
    fn table(&self, table: &Table) -> (gtk::Grid, Embedded) {
        let grid = gtk::Grid::new();
        grid.add_css_class("slate-table");
        grid.set_column_homogeneous(true);
        grid.set_halign(gtk::Align::Start);
        let columns = table
            .rows
            .iter()
            .map(|r| r.cells.iter().map(|c| c.span.max(1) as i32).sum::<i32>())
            .max()
            .unwrap_or(1)
            .max(1);
        let mut cells = Vec::new();
        for (y, row) in table.rows.iter().enumerate() {
            let mut x = 0;
            for cell in &row.cells {
                let span = cell.span.max(1) as i32;
                let view = text_view("slate-cell");
                view.set_left_margin(CELL_PADDING);
                view.set_right_margin(CELL_PADDING);
                view.set_top_margin(CELL_PADDING / 2);
                view.set_bottom_margin(CELL_PADDING / 2);
                let mut r = Renderer::new(&view, self.textures, self.style_looks, false);
                r.all_bold = row.header && self.style_looks;
                r.render(&cell.blocks);
                grid.attach(&view, x, y as i32, span, 1);
                x += span;
                cells.push(TableCell {
                    view,
                    span,
                    embedded: r.embedded,
                });
            }
        }
        (grid, Embedded::Table { columns, cells })
    }

    /// A picture, sized by [`resize`]; its alt text when it can't be shown.
    fn image(&mut self, image: &Image) -> gtk::Widget {
        if let ImageSource::Embedded { bytes, .. } = &image.source {
            if let Some(texture) = self.textures.get(&texture_key(bytes)) {
                let (w, h) = image
                    .size
                    .unwrap_or((texture.width() as f32, texture.height() as f32));
                let picture = gtk::Picture::for_paintable(texture);
                picture.set_can_shrink(true);
                picture.set_content_fit(gtk::ContentFit::Contain);
                picture.set_halign(gtk::Align::Start);
                picture.set_alternative_text(Some(&image.alt));
                self.embedded.push(Embedded::Picture(picture.clone(), w, h));
                return picture.upcast();
            }
        }
        let text = if image.alt.is_empty() {
            "[picture]".to_owned()
        } else {
            format!("[picture: {}]", image.alt)
        };
        let label = gtk::Label::new(Some(&text));
        label.add_css_class("dim-label");
        label.upcast()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_window_gets_the_settings_width() {
        for (setting, px) in [
            (TextWidth::Narrow, 480),
            (TextWidth::Medium, 624),
            (TextWidth::Wide, 816),
        ] {
            assert_eq!(column_width(setting, 1280), px);
            // Exactly room for it and a margin each side.
            assert_eq!(column_width(setting, px + 2 * MIN_MARGIN), px);
        }
    }

    #[test]
    fn full_is_the_window_less_a_margin_each_side() {
        assert_eq!(column_width(TextWidth::Full, 1280), 1232);
        assert_eq!(column_width(TextWidth::Full, 900), 852);
        assert_eq!(column_width(TextWidth::Full, 3840), 3792);
    }

    #[test]
    fn a_narrower_window_narrows_the_column() {
        assert_eq!(
            column_width(TextWidth::Medium, 624 + 2 * MIN_MARGIN - 1),
            623
        );
        assert_eq!(column_width(TextWidth::Wide, 640), 592);
        assert_eq!(column_width(TextWidth::Narrow, 500), 452);
    }

    #[test]
    fn the_column_never_gets_narrower_than_the_minimum() {
        for setting in TextWidth::ALL {
            assert_eq!(
                column_width(setting, MIN_COLUMN + 2 * MIN_MARGIN),
                MIN_COLUMN
            );
            assert_eq!(column_width(setting, 100), MIN_COLUMN);
            assert_eq!(column_width(setting, 0), MIN_COLUMN);
        }
    }
}
