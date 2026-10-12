//! Slate's pageless flow (ADR 0005, ADR 0007): the layout state of a [`Document`] shown as
//! one continuous column, toolkit-free.
//!
//! The flow owns the document and lays it out at a column width. Every block that isn't a
//! table is a *leaf*, numbered in document order ([`LeafId`]), wherever it is: at the top
//! level or in a table's cell, however deep. Text is measured through a [`Shaper`], which
//! the widget implements with Pango; the flow itself never shapes text, so its tests run
//! without a display.
//!
//! - **Heights.** Each top-level block's height sits in a Fenwick tree, so y ↔ block takes
//!   O(log n). Blocks start with estimated heights (character counts over the shaper's
//!   per-look line metrics) and are measured exactly when they come into view
//!   ([`Flow::layout_viewport`]) or by time-sliced passes ([`Flow::layout_slice`]) until the
//!   whole document is exact at the width. A width change keeps the heights of blocks
//!   measured at that width before and estimates the rest.
//! - **Scroll anchoring.** The scroll position is a block and an offset into it, so replacing
//!   estimates above the viewport changes the scroll value, never what the viewport shows.
//! - **Tables** have homogeneous columns (the widest row's spans), cells as wide as their
//!   spans, rows as tall as their tallest cell; cells hold blocks laid out the same way.
//! - **Pictures** show at their natural size, scaled down to fit their column.
//! - **Positions** are model coordinates: a leaf (whose path in the document
//!   [`Flow::path`] gives) and a byte offset into its text. [`Flow::hit`] turns a point into
//!   one; the selection is a pair of them; copies and the accessible text are the leaves'
//!   texts in document order joined by `\n`, tables' cells and pictures' alt text included.

mod content;
mod fenwick;
mod tree;

pub use content::Content;
pub use tree::{
    CELL_BORDER, CELL_PADDING, CODE_INSET, LEVEL_INDENT, PAGE_BREAK_HEIGHT, RULE_HEIGHT,
    RULE_MARGIN, TABLE_MARGIN,
};

use document_model::{Block, Document, Image};
use fenwick::Fenwick;
use std::borrow::Cow;
use std::ops::Range;
use tree::{Builder, Cx, Leaf, Node, LOOKS};

/// Space above the document's first block.
pub const TOP_MARGIN: i32 = 40;
/// Space below its last block.
pub const BOTTOM_MARGIN: i32 = 64;

/// A leaf: a block that isn't a table, at any depth, numbered in document order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LeafId(u32);

impl LeafId {
    /// Its number in document order.
    pub fn index(self) -> usize {
        self.0 as usize
    }

    /// The leaf numbered `index` in document order.
    pub fn from_index(index: usize) -> Self {
        LeafId(index as u32)
    }
}

/// A place in the document: a leaf and a byte offset into its text (a picture's alt text:
/// 0 before the picture, its length after).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Position {
    pub leaf: LeafId,
    pub offset: usize,
}

/// A text look whose line metrics estimates use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Body,
    Title,
    /// Heading level 1 to 6.
    Heading(u8),
    /// Code and raw source.
    Mono,
}

/// A look's average character width and line height, in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    pub char_width: f64,
    pub line_height: f64,
}

/// Where a point falls in shaped text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    /// The byte offset of the character nearest the point.
    pub index: usize,
    /// The point is in the character's trailing half: the caret position is after it.
    pub trailing: bool,
    /// The point is over the text itself, not beside or below a line.
    pub inside: bool,
}

/// Lays out text for the flow; the widget implements it with Pango.
pub trait Shaper {
    /// The average character width and line height of `look` at the normal size.
    fn metrics(&mut self, look: Look) -> Metrics;
    /// Lay out `content` (leaf `leaf`) wrapped at `width` pixels; its height in pixels.
    fn shape(&mut self, leaf: LeafId, content: Content<'_>, width: i32) -> i32;
    /// Where (`x`, `y`), relative to the text's top left, falls in `content` wrapped at
    /// `width`.
    fn hit(&mut self, leaf: LeafId, content: Content<'_>, width: i32, x: f64, y: f64) -> Hit;
    /// A picture's size in pixels when it can be shown; the flow prefers the size the file
    /// gives ([`Image::size`]).
    fn picture_size(&mut self, image: &Image) -> Option<(f32, f32)>;
}

/// Something to draw. `x` is relative to the column's left edge and `y` is in document
/// coordinates (0 is the top of the document, [`TOP_MARGIN`] above the first block).
#[derive(Debug, Clone)]
pub enum Paint<'a> {
    /// Text laid out `width` wide with its top left at (`x`, `y`); a list item's marker
    /// hangs [`LEVEL_INDENT`] to its left. `selected`: the bytes the selection covers.
    Text {
        leaf: LeafId,
        content: Content<'a>,
        x: i32,
        y: f64,
        width: i32,
        marker: bool,
        selected: Option<Range<usize>>,
    },
    /// A code block's background.
    Band {
        x: i32,
        y: f64,
        width: i32,
        height: i32,
    },
    /// A picture scaled to `width` x `height`.
    Picture {
        leaf: LeafId,
        image: &'a Image,
        x: i32,
        y: f64,
        width: i32,
        height: i32,
        selected: bool,
    },
    /// The `index`th page break in the document.
    PageBreak {
        index: usize,
        x: i32,
        y: f64,
        width: i32,
        height: i32,
    },
    /// A rule's 1 px line.
    Rule { x: i32, y: f64, width: i32 },
    /// A table cell's border.
    Border {
        x: i32,
        y: f64,
        width: i32,
        height: i32,
    },
}

/// What a point hits: the nearest caret position, and the character under the point when
/// it is over text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Located {
    pub position: Position,
    pub over: Option<Position>,
}

impl Located {
    fn start_of(position: Position) -> Self {
        Located {
            position,
            over: None,
        }
    }
}

/// How far a layout pass got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// Blocks measured by this pass.
    pub measured: usize,
    /// Every block is measured at the current width.
    pub complete: bool,
}

/// A unit of the accessible text: a leaf with text, and where its text starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unit {
    pub leaf: LeafId,
    /// The character offset of its first character.
    pub start: usize,
    /// Its length in characters.
    pub chars: usize,
    /// It is the document's last; the others end in a `\n`.
    pub last: bool,
}

/// The scroll position: a top-level block and how far into it the viewport's top is.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Anchor {
    block: usize,
    /// Pixels below the block's top; negative only for the first block (the top margin).
    offset: f64,
}

/// A document's layout in a pageless column.
pub struct Flow {
    doc: Document,
    leaves: Vec<Leaf>,
    tops: Vec<Node>,
    /// Top-level blocks' heights at the width.
    heights: Fenwick,
    /// Per leaf: its characters and a `\n` for a leaf with text, else 0.
    chars: Fenwick,
    width: i32,
    /// Top-level blocks measured at the width.
    exact: usize,
    /// Where the next layout slice starts.
    next: usize,
    anchor: Anchor,
    metrics: Option<[Metrics; 9]>,
    page_breaks: usize,
    break_tops: Vec<usize>,
    /// The selection's anchor and focus.
    selection: Option<(Position, Position)>,
}

impl Flow {
    /// The flow of `doc`, not laid out yet: [`Flow::set_width`] lays it out.
    pub fn new(doc: Document) -> Self {
        let mut builder = Builder::new(&doc.format);
        let tops = builder.blocks(&doc.blocks, &[], false);
        let chars = Fenwick::build(
            builder
                .leaves
                .iter()
                .map(|l| l.chars.map_or(0, |c| i64::from(c) + 1)),
        );
        Flow {
            heights: Fenwick::build(tops.iter().map(|n| i64::from(n.height))),
            doc,
            leaves: builder.leaves,
            tops,
            chars,
            width: 0,
            exact: 0,
            next: 0,
            anchor: Anchor {
                block: 0,
                offset: -f64::from(TOP_MARGIN),
            },
            metrics: None,
            page_breaks: builder.page_breaks,
            break_tops: builder.break_tops,
            selection: None,
        }
    }

    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// The column width laid out for; 0 before [`Flow::set_width`].
    pub fn width(&self) -> i32 {
        self.width
    }

    /// How many page breaks the document has.
    pub fn page_break_count(&self) -> usize {
        self.page_breaks
    }

    /// Lay the document out `width` pixels wide: blocks measured at that width before keep
    /// their heights, the rest are estimated until measured. The viewport keeps showing the
    /// same block, at the same fraction of it.
    pub fn set_width(&mut self, width: i32, shaper: &mut dyn Shaper) {
        let width = width.max(1);
        if width == self.width {
            return;
        }
        let metrics = *self
            .metrics
            .get_or_insert_with(|| LOOKS.map(|look| shaper.metrics(look)));
        tree::prepare_pictures(&mut self.tops, &self.doc, &self.leaves, shaper);
        let old = self
            .tops
            .get(self.anchor.block)
            .map_or(1, |n| n.height.max(1));
        self.width = width;
        self.exact = 0;
        for node in &mut self.tops {
            node.height = tree::estimate(node, width, &self.leaves, &metrics);
            if node.laid == width {
                self.exact += 1;
            }
        }
        self.heights = Fenwick::build(self.tops.iter().map(|n| i64::from(n.height)));
        if let Some(node) = self.tops.get(self.anchor.block) {
            if self.anchor.offset > 0.0 {
                self.anchor.offset *= f64::from(node.height) / f64::from(old);
            }
        }
        self.next = self.anchor.block;
    }

    /// Every block is measured at the width.
    pub fn is_complete(&self) -> bool {
        self.exact == self.tops.len()
    }

    /// The document's height: its blocks and the margins above and below.
    pub fn content_height(&self) -> f64 {
        (i64::from(TOP_MARGIN) + self.heights.total() + i64::from(BOTTOM_MARGIN)) as f64
    }

    /// The document y of the viewport's top.
    pub fn scroll(&self) -> f64 {
        f64::from(TOP_MARGIN) + self.heights.prefix(self.anchor.block) as f64 + self.anchor.offset
    }

    /// Scroll the viewport's top to document y `y`.
    pub fn scroll_to(&mut self, y: f64) {
        self.anchor = self.anchor_at(y);
    }

    fn anchor_at(&self, y: f64) -> Anchor {
        let y = y.max(0.0) - f64::from(TOP_MARGIN);
        if y < 0.0 || self.tops.is_empty() {
            return Anchor {
                block: 0,
                offset: y,
            };
        }
        let block = self.heights.find(y.floor() as i64);
        Anchor {
            block,
            offset: y - self.heights.prefix(block) as f64,
        }
    }

    /// The document y of top-level block `i`'s top.
    fn top_of(&self, i: usize) -> f64 {
        f64::from(TOP_MARGIN) + self.heights.prefix(i) as f64
    }

    /// Measure top-level block `i` at the width; true if it wasn't.
    fn ensure(&mut self, i: usize, shaper: &mut dyn Shaper) -> bool {
        let node = &mut self.tops[i];
        if node.laid == self.width {
            return false;
        }
        let Some(metrics) = &self.metrics else {
            return false;
        };
        let before = node.height;
        let mut cx = Cx {
            doc: &self.doc,
            leaves: &mut self.leaves,
            shaper,
            metrics,
        };
        let after = tree::measure(node, self.width, &mut cx);
        if after != before {
            self.heights.add(i, i64::from(after - before));
        }
        self.exact += 1;
        true
    }

    /// Measure the blocks a viewport `page` pixels tall shows, keeping its top on the same
    /// block and offset, and keep the scroll position inside the document. True if any
    /// height changed.
    pub fn layout_viewport(&mut self, shaper: &mut dyn Shaper, page: f64) -> bool {
        if self.tops.is_empty() || self.width == 0 {
            return false;
        }
        let mut changed = false;
        // Clamping at the end can move the anchor up onto blocks still estimated.
        for _ in 0..8 {
            let max = (self.content_height() - page).max(0.0);
            if self.scroll() > max {
                self.anchor = self.anchor_at(max);
            }
            let bottom = self.scroll() + page;
            let mut measured = false;
            let mut i = self.anchor.block;
            while i < self.tops.len() && self.top_of(i) < bottom {
                measured |= self.ensure(i, shaper);
                i += 1;
            }
            let h = f64::from(self.tops[self.anchor.block].height);
            if self.anchor.offset >= h {
                self.anchor.offset = (h - 1.0).max(0.0);
            }
            changed |= measured;
            if !measured {
                break;
            }
        }
        changed
    }

    /// Measure blocks not yet measured at the width, in order from the viewport on, while
    /// `more` says there is time. The viewport keeps showing what it shows.
    pub fn layout_slice(
        &mut self,
        shaper: &mut dyn Shaper,
        more: &mut dyn FnMut() -> bool,
    ) -> Progress {
        let n = self.tops.len();
        let mut measured = 0;
        while self.exact < n && self.width > 0 && more() {
            if self.next >= n {
                self.next = 0;
            }
            let i = self.next;
            self.next += 1;
            if self.ensure(i, shaper) {
                measured += 1;
            }
        }
        Progress {
            measured,
            complete: self.is_complete(),
        }
    }

    /// Call `f` with what the document shows between document y `top` and `bottom`, in
    /// document order.
    pub fn paint<'a>(&'a self, top: f64, bottom: f64, f: &mut dyn FnMut(Paint<'a>)) {
        if self.tops.is_empty() {
            return;
        }
        let mut i = self
            .heights
            .find((top - f64::from(TOP_MARGIN)).max(0.0).floor() as i64);
        let mut y = self.top_of(i);
        let mut cx = tree::PaintCx {
            doc: &self.doc,
            leaves: &self.leaves,
            selection: self.selection(),
            top,
            bottom,
            f,
        };
        while i < self.tops.len() && y < bottom {
            tree::paint(&self.tops[i], 0, y, self.width, &mut cx);
            y += f64::from(self.tops[i].height);
            i += 1;
        }
    }

    /// Call `f` with every page break, wherever it is.
    pub fn page_breaks<'a>(&'a self, f: &mut dyn FnMut(Paint<'a>)) {
        let mut only_breaks = |p: Paint<'a>| {
            if let Paint::PageBreak { .. } = p {
                f(p)
            }
        };
        let mut cx = tree::PaintCx {
            doc: &self.doc,
            leaves: &self.leaves,
            selection: None,
            top: f64::NEG_INFINITY,
            bottom: f64::INFINITY,
            f: &mut only_breaks,
        };
        for &i in &self.break_tops {
            tree::paint(&self.tops[i], 0, self.top_of(i), self.width, &mut cx);
        }
    }

    /// What the point (`x`, `y`) hits: `x` from the column's left edge, `y` in document
    /// coordinates. `None` for an empty document.
    pub fn hit(&self, shaper: &mut dyn Shaper, x: f64, y: f64) -> Option<Located> {
        if self.leaves.is_empty() {
            return None;
        }
        if y < f64::from(TOP_MARGIN) {
            return Some(Located::start_of(Position {
                leaf: LeafId(0),
                offset: 0,
            }));
        }
        let i = self
            .heights
            .find((y - f64::from(TOP_MARGIN)).floor() as i64);
        let top = self.top_of(i);
        if y >= top + f64::from(self.tops[i].height) {
            // Below the last block.
            return Some(Located::start_of(self.end()));
        }
        Some(tree::hit(
            &self.tops[i],
            x,
            y - top,
            self.width,
            &self.doc,
            &self.leaves,
            shaper,
        ))
    }

    /// The link at `position`'s character, if it is in one.
    pub fn link_at(&self, position: Position) -> Option<&str> {
        let Some(Content::Paragraph { paragraph, .. }) = self.content(position.leaf) else {
            return None;
        };
        let mut start = 0;
        for run in &paragraph.runs {
            let end = start + run.text.len();
            if position.offset < end {
                return run.style.link.as_deref();
            }
            start = end;
        }
        None
    }

    /// `leaf`'s text as a shaper lays it out; `None` for a picture, page break, or rule.
    pub fn content(&self, leaf: LeafId) -> Option<Content<'_>> {
        tree::content_at(&self.doc, &self.leaves.get(leaf.index())?.path)
    }

    /// Where `leaf` is in the document: the top-level block's index, then the row, cell,
    /// and block index for each table it is in.
    pub fn path(&self, leaf: LeafId) -> &[u32] {
        &self.leaves[leaf.index()].path
    }

    /// The model block `leaf` is.
    pub fn block(&self, leaf: LeafId) -> &Block {
        tree::block_at(&self.doc, &self.leaves[leaf.index()].path).0
    }

    /// The width `leaf`'s text was last wrapped at.
    pub fn wrap_width(&self, leaf: LeafId) -> i32 {
        self.leaves[leaf.index()].wrap
    }

    /// The leaves' count.
    pub fn leaf_count(&self) -> usize {
        self.leaves.len()
    }

    /// `leaf`'s text in copies and the accessible text: a picture's alt text; `None` for a
    /// leaf without text.
    pub fn leaf_text(&self, leaf: LeafId) -> Option<Cow<'_, str>> {
        let l = self.leaves.get(leaf.index())?;
        l.chars?;
        match self.content(leaf) {
            Some(content) => Some(content.text()),
            None => match self.block(leaf) {
                Block::Image(image) => Some(Cow::Borrowed(&image.alt)),
                _ => None,
            },
        }
    }

    fn end(&self) -> Position {
        let last = self.leaves.len() - 1;
        Position {
            leaf: LeafId(last as u32),
            offset: self.leaves[last].bytes as usize,
        }
    }

    // ---- selection ----

    /// Select from `anchor` to `focus` (either order).
    pub fn select(&mut self, anchor: Position, focus: Position) {
        self.selection = Some((anchor, focus));
    }

    /// Select the whole document.
    pub fn select_all(&mut self) {
        if !self.leaves.is_empty() {
            self.selection = Some((
                Position {
                    leaf: LeafId(0),
                    offset: 0,
                },
                self.end(),
            ));
        }
    }

    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    /// The selection's start and end, in document order; `None` when nothing is selected.
    pub fn selection(&self) -> Option<(Position, Position)> {
        let (a, b) = self.selection?;
        let (s, e) = if a <= b { (a, b) } else { (b, a) };
        (s != e).then_some((s, e))
    }

    /// The anchor of the selection as made, `None` without one.
    pub fn selection_anchor(&self) -> Option<Position> {
        self.selection.map(|(a, _)| a)
    }

    /// The selected text, as plain text: each leaf's selected part, joined by `\n`.
    pub fn selected_text(&self) -> String {
        let Some((s, e)) = self.selection() else {
            return String::new();
        };
        let mut out = String::new();
        let mut first = true;
        for i in s.leaf.index()..=e.leaf.index() {
            let leaf = LeafId(i as u32);
            let Some(text) = self.leaf_text(leaf) else {
                continue;
            };
            let a = if leaf == s.leaf {
                content::floor_boundary(&text, s.offset)
            } else {
                0
            };
            let b = if leaf == e.leaf {
                content::floor_boundary(&text, e.offset)
            } else {
                text.len()
            };
            if !first {
                out.push('\n');
            }
            first = false;
            out.push_str(&text[a..b.max(a)]);
        }
        out
    }

    // ---- accessible text: the leaves' texts joined by `\n`, offsets in characters ----

    /// The accessible text's length in characters.
    pub fn char_count(&self) -> usize {
        (self.chars.total() as usize).saturating_sub(1)
    }

    /// The unit of the accessible text holding character `offset` (the last one past the
    /// end); `None` for a document without text.
    pub fn unit_at(&self, offset: usize) -> Option<Unit> {
        let total = self.char_count();
        if self.chars.total() == 0 {
            return None;
        }
        let mut i = self.chars.find(offset.min(total) as i64);
        while self.leaves[i].chars.is_none() {
            i -= 1;
        }
        let start = self.chars.prefix(i) as usize;
        let chars = self.leaves[i].chars.unwrap_or(0) as usize;
        Some(Unit {
            leaf: LeafId(i as u32),
            start,
            chars,
            last: start + chars >= total,
        })
    }

    /// The accessible text from character `start` to `end` (clamped to the text).
    pub fn text(&self, start: usize, end: usize) -> String {
        let end = end.min(self.char_count());
        let mut out = String::new();
        if start >= end {
            return out;
        }
        let Some(unit) = self.unit_at(start) else {
            return out;
        };
        let mut left = end - start;
        let mut skip = start - unit.start;
        for i in unit.leaf.index()..self.leaves.len() {
            if left == 0 {
                break;
            }
            let Some(text) = self.leaf_text(LeafId(i as u32)) else {
                continue;
            };
            let chars = self.leaves[i].chars.unwrap_or(0) as usize;
            if skip <= chars {
                for c in text.chars().skip(skip).take(left) {
                    out.push(c);
                    left -= 1;
                }
                skip = 0;
            } else {
                skip -= chars + 1;
                continue;
            }
            if left > 0 {
                out.push('\n');
                left -= 1;
            }
        }
        out
    }

    /// `position` as a character offset in the accessible text; a leaf without text is
    /// where the next unit starts.
    pub fn char_offset(&self, position: Position) -> usize {
        let i = position.leaf.index().min(self.leaves.len());
        let start = self.chars.prefix(i) as usize;
        let within = match self.leaf_text(position.leaf) {
            Some(text) => text[..content::floor_boundary(&text, position.offset)]
                .chars()
                .count(),
            None => 0,
        };
        (start + within).min(self.char_count())
    }
}
