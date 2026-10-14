//! The layout tree: one node per block, mirroring the document (a table's cells hold
//! nodes of their own), with the viewer's spacing rules, measuring through a [`Shaper`],
//! estimating without one, and walking it to paint and to hit-test.
//!
//! A paragraph's space above and below, its left, right, and first-line indents, and its
//! line spacing are its style's with its own on top ([`StyleSheet::spacing`], ADR 0009),
//! and contextual spacing drops the space between two paragraphs of one style. A list
//! item keeps the viewer's list spacing and its level's indent unless it has its own;
//! code blocks, tables, rules, and page breaks keep the viewer's own spacing.
//!
//! A paragraph's borders and shading ([`StyleSheet::borders`], [`StyleSheet::shading`])
//! frame it as Word does: its top and bottom lines lie their space from the text, and they
//! and their spaces add to its height, inside its space before and after; its left and
//! right lines lie their space outside its indents. Paragraphs bordered together
//! ([`StyleSheet::bordered_together`]) share one box: the top line above the first, the
//! bottom line below the last, the between line above each of the others, and the box
//! (its sides and its shading) runs on through the space between them. Shading fills the
//! box to its lines; without borders, the text's indented lines.

use crate::content::{code_text, floor_boundary, Content};
use crate::{Dash, LayoutKey, LeafId, Located, Look, Metrics, Paint, Position, Shaper, MONOSPACE};
use document_model::{
    Block, Border, BorderStyle, Document, Format, Image, LineSpacing, Paragraph, Rgb, Style,
    StyleSheet,
};
use std::ops::Range;

/// Twips (DOCX's 1/20 pt) per pixel at 96 dpi.
pub const TWIPS_PER_PX: f64 = 15.0;

/// `twips` in whole pixels at 96 dpi.
pub fn px(twips: i64) -> i32 {
    (twips as f64 / TWIPS_PER_PX).round() as i32
}

/// How far each list or quote level indents.
pub const LEVEL_INDENT: i32 = 28;
/// A code block's text starts this far inside its band.
pub const CODE_INSET: i32 = 12;
/// Space inside a table cell's border, left and right; half of it above and below.
pub const CELL_PADDING: i32 = 6;
/// A table cell's border.
pub const CELL_BORDER: i32 = 1;
/// Space above and below a table.
pub const TABLE_MARGIN: i32 = 6;
/// The narrowest a table column gets.
pub const MIN_TABLE_COLUMN: i32 = 24;
/// A page break's height: its dashed line with room above and below.
pub const PAGE_BREAK_HEIGHT: i32 = 38;
/// A rule's height: a 1 px line [`RULE_MARGIN`] below its top, with as much room below.
pub const RULE_HEIGHT: i32 = 25;
/// Room above a rule's line.
pub const RULE_MARGIN: i32 = 12;

const PARAGRAPH_SPACE: i32 = 8;
const ITEM_SPACE: i32 = 3;
const CODE_ABOVE: i32 = 4;

/// The text looks of a document's text boxes, each with its line metrics once a shaper
/// gave them, numbered as they are first met. The first is the document's default style's.
#[derive(Debug, Default)]
pub(crate) struct Looks {
    looks: Vec<Look>,
    metrics: Vec<Metrics>,
}

impl Looks {
    pub fn new(styles: &StyleSheet) -> Self {
        let mut looks = Looks::default();
        let run = &styles.default_style().run;
        looks.id(&run.font, run.size, run.bold);
        looks
    }

    /// The number of the look of `font` at `size` half-points, bold or not.
    fn id(&mut self, font: &str, size: u16, bold: bool) -> u32 {
        let found = self
            .looks
            .iter()
            .position(|l| l.size == size && l.bold == bold && l.font == font);
        let i = found.unwrap_or_else(|| {
            self.looks.push(Look {
                font: font.to_owned(),
                size,
                bold,
            });
            self.looks.len() - 1
        });
        i as u32
    }

    /// Ask `shaper` for the metrics of the looks that have none yet.
    pub fn measure(&mut self, shaper: &mut dyn Shaper) {
        while self.metrics.len() < self.looks.len() {
            let m = shaper.metrics(&self.looks[self.metrics.len()]);
            self.metrics.push(m);
        }
    }

    fn metrics(&self, id: u32) -> Metrics {
        self.metrics[id as usize]
    }

    /// The default style's metrics: an empty table cell's and a picture label's.
    fn body(&self) -> Metrics {
        self.metrics(0)
    }
}

/// Every leaf in document order: where it is in the model and what text it holds.
#[derive(Debug)]
pub(crate) struct Leaf {
    /// The top-level block's index, then (row, cell, block) for each table it is in.
    pub path: Box<[u32]>,
    /// Characters in its text (a picture's alt text); `None` for a leaf without text,
    /// which the accessible text and copies leave out.
    pub chars: Option<u32>,
    /// Its text's length in bytes.
    pub bytes: u32,
    /// Characters per line of its text (split at `\n`), for estimates.
    pub lines: Box<[u32]>,
    /// The width its text was last wrapped at.
    pub wrap: i32,
    /// What the shaper caches its layout under: new whenever the leaf is rebuilt.
    pub key: LayoutKey,
}

#[derive(Debug)]
pub(crate) struct Node {
    pub kind: Kind,
    /// The height at the flow's width: exact when `laid` is that width, else estimated.
    /// Nested nodes only hold exact heights, for their table's `laid` width.
    pub height: i32,
    /// The width the node was last measured at; -1 before.
    pub laid: i32,
    /// The height measured at `laid`.
    pub laid_height: i32,
    /// Its top in its table cell (nested nodes).
    pub y: i32,
}

#[derive(Debug)]
pub(crate) enum Kind {
    Text(TextBox),
    Picture {
        leaf: LeafId,
        /// The size to show it at before fitting, once asked; `None` inside when it can't
        /// be shown (its label is shown instead).
        natural: Option<Option<(f32, f32)>>,
        /// The label's height when it can't be shown.
        label_height: i32,
    },
    PageBreak {
        leaf: LeafId,
        index: usize,
    },
    Rule {
        leaf: LeafId,
    },
    Table(Box<TableBox>),
}

#[derive(Debug)]
pub(crate) struct TextBox {
    pub leaf: LeafId,
    pub above: i32,
    pub below: i32,
    /// The text's left edge from the box's: the paragraph's left indent, less a hanging
    /// first line's.
    pub indent: i32,
    /// The text's right edge from the box's: the paragraph's right indent.
    pub right: i32,
    /// A list item: its marker hangs in the indent's last level.
    pub marker: bool,
    /// A code block: its band starts this far in.
    pub band: Option<i32>,
    /// Its first run's look ([`Looks`]), for estimates.
    pub look: u32,
    /// Its line spacing, for estimates (the shaper lays it out).
    pub line: LineSpacing,
    /// The text's height measured at `laid`.
    pub text_height: i32,
    /// A paragraph's borders and shading, when it has any.
    pub frame: Option<Frame>,
}

/// A paragraph's borders and shading as laid out around its text (ADR 0009). Its space
/// before is `above` less `zone.0`, its space after `below` less `zone.1`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Frame {
    /// The line drawn above the text: its top line, or a group's between line above each
    /// paragraph but the first.
    pub top: Border,
    /// The line drawn below the text: its bottom line, the last of a group's only.
    pub bottom: Border,
    pub left: Border,
    pub right: Border,
    pub fill: Option<Rgb>,
    /// How far the box reaches out from the text, in pixels: above, below, left, right.
    pub zone: (i32, i32, i32, i32),
    /// The box runs on through the space before into the paragraph above's (a group's
    /// paragraphs but the first), and through the space after into the one below's.
    pub open: (bool, bool),
}

#[derive(Debug)]
pub(crate) struct TableBox {
    pub columns: i32,
    pub rows: Vec<RowBox>,
}

#[derive(Debug)]
pub(crate) struct RowBox {
    /// Its top below the table's top margin, and its height, at the table's `laid` width.
    pub y: i32,
    pub height: i32,
    pub cells: Vec<CellBox>,
}

#[derive(Debug)]
pub(crate) struct CellBox {
    pub span: i32,
    /// Its left edge and width in the table, at the table's `laid` width.
    pub x: i32,
    pub width: i32,
    /// The first leaf in or after the cell (one past the last leaf for an empty cell at
    /// the document's end).
    pub first_leaf: LeafId,
    pub nodes: Vec<Node>,
}

impl Node {
    fn new(kind: Kind) -> Self {
        Node {
            kind,
            height: 1,
            laid: -1,
            laid_height: 1,
            y: 0,
        }
    }
}

/// Builds the tree and the leaves of a document, or of a stretch of its top-level blocks.
pub(crate) struct Builder<'a> {
    pub leaves: Vec<Leaf>,
    pub page_breaks: usize,
    /// Top-level blocks holding a page break, in order.
    pub break_tops: Vec<usize>,
    styles: &'a StyleSheet,
    plain: bool,
    /// The document's default tab stop ([`Content::Paragraph`]'s `default_tab`).
    default_tab: Option<u32>,
    /// The first leaf's number: leaves before the stretch built.
    base: u32,
    /// The next layout key.
    pub keys: u64,
}

impl<'a> Builder<'a> {
    pub fn new(doc: &'a Document, base: usize, keys: u64) -> Self {
        Builder {
            leaves: Vec::new(),
            page_breaks: 0,
            break_tops: Vec::new(),
            styles: &doc.styles,
            plain: matches!(doc.format, Format::Text(_)),
            default_tab: doc.format.default_tab(),
            base: base as u32,
            keys,
        }
    }

    /// Nodes for top-level blocks `range` of `all`, their looks numbered in `looks`.
    pub fn tops(&mut self, looks: &mut Looks, all: &[Block], range: Range<usize>) -> Vec<Node> {
        range
            .map(|i| {
                let (prev, next) = neighbours(all, i);
                self.block(looks, &all[i], prev, next, vec![i as u32], false)
            })
            .collect()
    }

    /// Nodes for `blocks`, whose paths start with `prefix`.
    fn blocks(
        &mut self,
        looks: &mut Looks,
        blocks: &[Block],
        prefix: &[u32],
        header: bool,
    ) -> Vec<Node> {
        let mut nodes = Vec::with_capacity(blocks.len());
        for (i, block) in blocks.iter().enumerate() {
            let mut path = prefix.to_vec();
            path.push(i as u32);
            let (prev, next) = neighbours(blocks, i);
            nodes.push(self.block(looks, block, prev, next, path, header));
        }
        nodes
    }

    fn next_leaf(&self) -> LeafId {
        LeafId(self.base + self.leaves.len() as u32)
    }

    fn leaf(&mut self, path: Vec<u32>, text: Option<&str>) -> LeafId {
        let id = self.next_leaf();
        let (chars, bytes, lines) = match text {
            Some(t) => {
                let lines: Box<[u32]> = t.split('\n').map(|l| l.chars().count() as u32).collect();
                let chars = lines.iter().sum::<u32>() + lines.len() as u32 - 1;
                (Some(chars), t.len() as u32, lines)
            }
            None => (None, 0, Box::default()),
        };
        self.leaves.push(Leaf {
            path: path.into_boxed_slice(),
            chars,
            bytes,
            lines,
            wrap: 0,
            key: LayoutKey(self.keys),
        });
        self.keys += 1;
        id
    }

    fn block(
        &mut self,
        looks: &mut Looks,
        block: &Block,
        prev: Option<&Block>,
        next: Option<&Block>,
        path: Vec<u32>,
        header: bool,
    ) -> Node {
        let kind = match block {
            Block::Paragraph(p) => {
                let content = Content::Paragraph {
                    paragraph: p,
                    styles: self.styles,
                    list: None,
                    header,
                    default_tab: self.default_tab,
                };
                let leaf = self.leaf(path, Some(&content.text()));
                let style = self.styles.style_of(&p.style);
                let line = content.line_spacing();
                let mut b = TextBox::paragraph(leaf, looks, p, style, line, header);
                self.place(&mut b, block, prev, next);
                b.indent = content.text_left();
                b.right = content.text_right();
                Kind::Text(b)
            }
            Block::ListItem(item) => {
                let content = Content::Paragraph {
                    paragraph: &item.paragraph,
                    styles: self.styles,
                    list: Some(item),
                    header,
                    default_tab: self.default_tab,
                };
                let leaf = self.leaf(path, Some(&content.text()));
                let style = self.styles.style_of(&item.paragraph.style);
                let line = content.line_spacing();
                let mut b = TextBox::paragraph(leaf, looks, &item.paragraph, style, line, header);
                self.place(&mut b, block, prev, next);
                b.indent = content.text_left();
                b.right = content.text_right();
                b.marker = true;
                Kind::Text(b)
            }
            Block::CodeBlock(code) => {
                let leaf = self.leaf(path, Some(code_text(code)));
                let mut b = TextBox::new(leaf, self.mono(looks));
                let band = i32::from(code.indent) * LEVEL_INDENT;
                b.band = Some(band);
                b.indent = band + CODE_INSET;
                b.above = CODE_ABOVE;
                b.below = PARAGRAPH_SPACE;
                Kind::Text(b)
            }
            Block::Raw(raw) => {
                let leaf = self.leaf(path, Some(&raw.source));
                let mut b = TextBox::new(leaf, self.mono(looks));
                b.below = PARAGRAPH_SPACE;
                Kind::Text(b)
            }
            Block::Image(image) => {
                let alt = (!image.alt.is_empty()).then_some(image.alt.as_str());
                Kind::Picture {
                    leaf: self.leaf(path, alt),
                    natural: None,
                    label_height: 1,
                }
            }
            Block::PageBreak => {
                let index = self.page_breaks;
                self.page_breaks += 1;
                let top = path[0] as usize;
                if self.break_tops.last() != Some(&top) {
                    self.break_tops.push(top);
                }
                Kind::PageBreak {
                    leaf: self.leaf(path, None),
                    index,
                }
            }
            Block::Rule => Kind::Rule {
                leaf: self.leaf(path, None),
            },
            Block::Table(table) => {
                let columns = table
                    .rows
                    .iter()
                    .map(|r| r.cells.iter().map(|c| c.span.max(1) as i32).sum::<i32>())
                    .max()
                    .unwrap_or(1)
                    .max(1);
                let mut rows = Vec::with_capacity(table.rows.len());
                for (r, row) in table.rows.iter().enumerate() {
                    let mut cells = Vec::with_capacity(row.cells.len());
                    for (c, cell) in row.cells.iter().enumerate() {
                        let mut prefix = path.clone();
                        prefix.extend([r as u32, c as u32]);
                        let first_leaf = self.next_leaf();
                        cells.push(CellBox {
                            span: cell.span.max(1) as i32,
                            x: 0,
                            width: 0,
                            first_leaf,
                            nodes: self.blocks(looks, &cell.blocks, &prefix, row.header),
                        });
                    }
                    rows.push(RowBox {
                        y: 0,
                        height: 0,
                        cells,
                    });
                }
                Kind::Table(Box::new(TableBox { columns, rows }))
            }
        };
        Node::new(kind)
    }

    /// The look of code and raw source: monospace at the default style's size.
    fn mono(&self, looks: &mut Looks) -> u32 {
        looks.id(MONOSPACE, self.styles.default_style().run.size, false)
    }

    fn space(&self) -> i32 {
        if self.plain {
            0
        } else {
            PARAGRAPH_SPACE
        }
    }

    /// The space below a list item followed by `next`: a list's items sit close together;
    /// the list ends with a paragraph's space.
    fn item_below(&self, next: Option<&Block>) -> i32 {
        if matches!(next, Some(Block::ListItem(_))) {
            ITEM_SPACE
        } else {
            self.space()
        }
    }

    /// The space above and below paragraph or list item `block`, between `prev` and `next`
    /// (ADR 0009): a paragraph's, its style's with its own on top; a list item's own, else
    /// the viewer's list spacing. With contextual spacing, none against a paragraph or
    /// list item in the same style. Plain text's paragraphs are the file's lines, without
    /// space between. `None` for other blocks.
    fn around(
        &self,
        block: &Block,
        prev: Option<&Block>,
        next: Option<&Block>,
    ) -> Option<(i32, i32)> {
        let (p, list) = match block {
            Block::Paragraph(p) => (p, false),
            Block::ListItem(item) => (&item.paragraph, true),
            _ => return None,
        };
        if self.plain {
            return Some((0, 0));
        }
        let s = self.styles.spacing(p);
        let style = self.styles.style_of(&p.style);
        let joined = |other: Option<&Block>| {
            s.contextual
                && other
                    .and_then(paragraph_of)
                    .is_some_and(|o| std::ptr::eq(self.styles.style_of(&o.style), style))
        };
        let (before, after) = if list {
            (p.own.space_before, p.own.space_after)
        } else {
            (Some(s.space_before), Some(s.space_after))
        };
        let above = match before {
            Some(t) if !joined(prev) => px(i64::from(t)),
            _ => 0,
        };
        let below = match after {
            Some(_) if joined(next) => 0,
            Some(t) => px(i64::from(t)),
            None => self.item_below(next),
        };
        Some((above, below))
    }

    /// Set text box `b`'s space above and below and its frame, for paragraph or list item
    /// `block` between `prev` and `next`: the frame's lines and their spaces go inside the
    /// space before and after.
    fn place(&self, b: &mut TextBox, block: &Block, prev: Option<&Block>, next: Option<&Block>) {
        let (above, below) = self.around(block, prev, next).unwrap_or_default();
        b.frame = self.frame(block, prev, next);
        let (top, bottom) = b.frame.as_ref().map_or((0, 0), |f| (f.zone.0, f.zone.1));
        (b.above, b.below) = (above + top, below + bottom);
    }

    /// The borders and shading of paragraph or list item `block` between `prev` and `next`
    /// (ADR 0009), grouped with the paragraphs it is bordered together with; `None` without
    /// any, in plain text, and for other blocks.
    fn frame(&self, block: &Block, prev: Option<&Block>, next: Option<&Block>) -> Option<Frame> {
        if self.plain {
            return None;
        }
        let p = paragraph_of(block)?;
        let borders = self.styles.borders(p);
        let fill = self.styles.shading(p).fill;
        if !borders.any() && fill.is_none() {
            return None;
        }
        let open_above = prev
            .and_then(paragraph_of)
            .is_some_and(|o| self.styles.bordered_together(o, p));
        let open_below = next
            .and_then(paragraph_of)
            .is_some_and(|o| self.styles.bordered_together(p, o));
        let top = if open_above {
            borders.between
        } else {
            borders.top
        };
        let bottom = if open_below {
            Border::default()
        } else {
            borders.bottom
        };
        Some(Frame {
            zone: (
                reach(&top),
                reach(&bottom),
                reach(&borders.left),
                reach(&borders.right),
            ),
            top,
            bottom,
            left: borders.left,
            right: borders.right,
            fill,
            open: (open_above, open_below),
        })
    }

    /// Give a top-level paragraph's or list item's node the space and frame its neighbours
    /// `prev` and `next` now call for (they were inserted or removed), keeping its
    /// measurement; how much taller it got.
    pub fn respace(
        &self,
        node: &mut Node,
        block: &Block,
        prev: Option<&Block>,
        next: Option<&Block>,
    ) -> i32 {
        let Kind::Text(b) = &mut node.kind else {
            return 0;
        };
        if paragraph_of(block).is_none() {
            return 0;
        }
        let (above, below) = (b.above, b.below);
        self.place(b, block, prev, next);
        let by = b.above - above + b.below - below;
        node.height += by;
        node.laid_height += by;
        by
    }
}

/// The thickness of a border line's strokes in whole pixels at 96 dpi, at least 1: a
/// double line's two strokes each are, with a gap as wide between them.
fn stroke(b: &Border) -> i32 {
    let eighths = match b.style {
        BorderStyle::Thick => 2 * u32::from(b.width),
        _ => u32::from(b.width),
    };
    // An eighth of a point is 1/6 px.
    ((f64::from(eighths) / 6.0).round() as i32).max(1)
}

/// How far border `b`'s strokes and gaps reach, in pixels.
fn strokes(b: &Border) -> i32 {
    match b.style {
        BorderStyle::Double => 3 * stroke(b),
        _ => stroke(b),
    }
}

/// How far border `b` reaches out from the text, its space and its lines, in whole pixels
/// (never less than its strokes); 0 without a line.
fn reach(b: &Border) -> i32 {
    if b.is_drawn() {
        px(i64::from(b.reach())).max(strokes(b))
    } else {
        0
    }
}

/// The blocks before and after block `i` of `blocks`.
pub(crate) fn neighbours(blocks: &[Block], i: usize) -> (Option<&Block>, Option<&Block>) {
    (
        i.checked_sub(1).and_then(|j| blocks.get(j)),
        blocks.get(i + 1),
    )
}

/// The paragraph of a paragraph or list item.
fn paragraph_of(block: &Block) -> Option<&Paragraph> {
    match block {
        Block::Paragraph(p) => Some(p),
        Block::ListItem(item) => Some(&item.paragraph),
        _ => None,
    }
}

/// Move every leaf number in `node` by `by`.
pub(crate) fn shift_leaves(node: &mut Node, by: i64) {
    let shift = |l: &mut LeafId| l.0 = (i64::from(l.0) + by) as u32;
    match &mut node.kind {
        Kind::Text(b) => shift(&mut b.leaf),
        Kind::Picture { leaf, .. } | Kind::PageBreak { leaf, .. } | Kind::Rule { leaf } => {
            shift(leaf)
        }
        Kind::Table(table) => {
            for cell in table.rows.iter_mut().flat_map(|r| &mut r.cells) {
                shift(&mut cell.first_leaf);
                for n in &mut cell.nodes {
                    shift_leaves(n, by);
                }
            }
        }
    }
}

/// Number the page breaks in `tops` in document order; their count, and the top-level
/// blocks holding one.
pub(crate) fn number_breaks(tops: &mut [Node]) -> (usize, Vec<usize>) {
    fn walk(node: &mut Node, count: &mut usize) -> bool {
        match &mut node.kind {
            Kind::PageBreak { index, .. } => {
                *index = *count;
                *count += 1;
                true
            }
            Kind::Table(table) => {
                let mut any = false;
                for cell in table.rows.iter_mut().flat_map(|r| &mut r.cells) {
                    for n in &mut cell.nodes {
                        any |= walk(n, count);
                    }
                }
                any
            }
            _ => false,
        }
    }
    let mut count = 0;
    let mut holding = Vec::new();
    for (i, node) in tops.iter_mut().enumerate() {
        if walk(node, &mut count) {
            holding.push(i);
        }
    }
    (count, holding)
}

/// Where `leaf`'s text sits in `node`, whose box starts at (`x`, `y`) and is `width`
/// wide: its left edge, top, and wrap width; `None` when `leaf` isn't text in it. Inside a
/// table this holds only once the table is measured at `width`.
pub(crate) fn text_place(
    node: &Node,
    leaf: LeafId,
    x: i32,
    y: f64,
    width: i32,
) -> Option<(i32, f64, i32)> {
    match &node.kind {
        Kind::Text(b) if b.leaf == leaf => {
            Some((x + b.indent, y + f64::from(b.above), b.wrap(width)))
        }
        Kind::Table(table) => {
            let top = y + f64::from(TABLE_MARGIN);
            table.rows.iter().find_map(|row| {
                row.cells.iter().find_map(|cell| {
                    let cx = x + cell.x + CELL_BORDER + CELL_PADDING;
                    let inner = cell_inner(cell.width);
                    cell.nodes
                        .iter()
                        .find_map(|n| text_place(n, leaf, cx, top + f64::from(row.y + n.y), inner))
                })
            })
        }
        _ => None,
    }
}

/// Mark `node`, and the nodes inside it down to `leaf`'s, as not measured; false if `leaf`
/// isn't in it.
pub(crate) fn unlay(node: &mut Node, leaf: LeafId) -> bool {
    let found = match &mut node.kind {
        Kind::Text(b) => b.leaf == leaf,
        Kind::Picture { leaf: l, .. } => *l == leaf,
        Kind::Table(table) => table
            .rows
            .iter_mut()
            .flat_map(|r| &mut r.cells)
            .flat_map(|c| &mut c.nodes)
            .any(|n| unlay(n, leaf)),
        _ => false,
    };
    if found {
        node.laid = -1;
    }
    found
}

impl TextBox {
    fn new(leaf: LeafId, look: u32) -> Self {
        TextBox {
            leaf,
            above: 0,
            below: 0,
            indent: 0,
            right: 0,
            marker: false,
            band: None,
            look,
            line: LineSpacing::default(),
            text_height: 1,
            frame: None,
        }
    }

    /// The box of paragraph `p` in `style` (in a table's header row: `header`): its first
    /// run's look, as the shaper draws it, and its line spacing `line`.
    fn paragraph(
        leaf: LeafId,
        looks: &mut Looks,
        p: &Paragraph,
        style: &Style,
        line: LineSpacing,
        header: bool,
    ) -> Self {
        let look = &style.run;
        let id = match p.runs.first().map(|r| &r.style) {
            Some(r) => {
                let font = if r.code || r.raw {
                    MONOSPACE
                } else {
                    r.font.as_deref().unwrap_or(&look.font)
                };
                let bold = r.bold.unwrap_or(look.bold) || header;
                looks.id(font, r.size.unwrap_or(look.size), bold)
            }
            None => looks.id(&look.font, look.size, look.bold || header),
        };
        TextBox {
            line,
            ..TextBox::new(leaf, id)
        }
    }

    /// The width its text wraps at in a box `width` wide: between its indents.
    pub(crate) fn wrap(&self, width: i32) -> i32 {
        (width - self.indent - self.right).max(1)
    }
}

/// What measuring needs besides the node.
pub(crate) struct Cx<'a> {
    pub doc: &'a Document,
    pub leaves: &'a mut [Leaf],
    pub shaper: &'a mut dyn Shaper,
    pub looks: &'a Looks,
}

/// The model block at `path`, and whether it is in a table's header row.
pub(crate) fn block_at<'a>(doc: &'a Document, path: &[u32]) -> (&'a Block, bool) {
    let mut block = &doc.blocks[path[0] as usize];
    let mut header = false;
    for &[r, c, b] in path[1..].as_chunks::<3>().0 {
        let Block::Table(table) = block else {
            unreachable!("a path steps into a table");
        };
        let row = &table.rows[r as usize];
        header = row.header;
        block = &row.cells[c as usize].blocks[b as usize];
    }
    (block, header)
}

/// The leaf at `path` as a shaper lays it out; `None` for a page break or rule, and for a
/// picture (which is shown, or labelled through [`Content::PictureLabel`]).
pub(crate) fn content_at<'a>(doc: &'a Document, path: &[u32]) -> Option<Content<'a>> {
    let default_tab = doc.format.default_tab();
    match block_at(doc, path) {
        (Block::Paragraph(p), header) => Some(Content::Paragraph {
            paragraph: p,
            styles: &doc.styles,
            list: None,
            header,
            default_tab,
        }),
        (Block::ListItem(item), header) => Some(Content::Paragraph {
            paragraph: &item.paragraph,
            styles: &doc.styles,
            list: Some(item),
            header,
            default_tab,
        }),
        (Block::CodeBlock(code), _) => Some(Content::Code(code)),
        (Block::Raw(raw), _) => Some(Content::Raw(raw)),
        _ => None,
    }
}

fn image_at<'a>(doc: &'a Document, path: &[u32]) -> &'a Image {
    match block_at(doc, path).0 {
        Block::Image(image) => image,
        _ => unreachable!("a picture leaf is an image"),
    }
}

/// A picture of natural size `w` x `h` fitted to `width`: its shown size.
fn fit((w, h): (f32, f32), width: i32) -> (i32, i32) {
    let shown = w.min(width as f32).max(1.0);
    let height = (h * shown / w.max(1.0)).max(1.0);
    (shown.round() as i32, height.round() as i32)
}

/// Each column's width and the cells' inner width in a table `width` wide.
fn column_width(columns: i32, width: i32) -> i32 {
    (width / columns).max(MIN_TABLE_COLUMN)
}

fn cell_inner(width: i32) -> i32 {
    (width - 2 * (CELL_PADDING + CELL_BORDER)).max(1)
}

/// Above a cell's first block, and below its last.
const CELL_TOP: i32 = CELL_BORDER + CELL_PADDING / 2;

/// Ask the shaper for every picture's natural size, once.
pub(crate) fn prepare_pictures(
    nodes: &mut [Node],
    doc: &Document,
    leaves: &[Leaf],
    shaper: &mut dyn Shaper,
) {
    for node in nodes {
        match &mut node.kind {
            Kind::Picture { leaf, natural, .. } if natural.is_none() => {
                let image = image_at(doc, &leaves[leaf.index()].path);
                let size = shaper.picture_size(image).map(|s| image.size.unwrap_or(s));
                *natural = Some(size);
            }
            Kind::Table(table) => {
                for cell in table.rows.iter_mut().flat_map(|r| &mut r.cells) {
                    prepare_pictures(&mut cell.nodes, doc, leaves, shaper);
                }
            }
            _ => {}
        }
    }
}

/// Measure `node` at `width` (exactly, through the shaper) unless it already is; its height.
pub(crate) fn measure(node: &mut Node, width: i32, cx: &mut Cx<'_>) -> i32 {
    if node.laid == width {
        return node.laid_height;
    }
    let height = match &mut node.kind {
        Kind::Text(b) => {
            let leaf = &mut cx.leaves[b.leaf.index()];
            let content = content_at(cx.doc, &leaf.path).expect("a text leaf has content");
            let wrap = b.wrap(width);
            leaf.wrap = wrap;
            b.text_height = cx.shaper.shape(leaf.key, content, wrap).max(1);
            b.above + b.text_height + b.below
        }
        Kind::Picture {
            leaf,
            natural,
            label_height,
        } => match natural.flatten() {
            Some(size) => fit(size, width).1,
            None => {
                let l = &mut cx.leaves[leaf.index()];
                l.wrap = width;
                let image = image_at(cx.doc, &l.path);
                *label_height = cx
                    .shaper
                    .shape(l.key, Content::PictureLabel(image), width)
                    .max(1);
                *label_height
            }
        },
        Kind::PageBreak { .. } => PAGE_BREAK_HEIGHT,
        Kind::Rule { .. } => RULE_HEIGHT,
        Kind::Table(table) => {
            let column = column_width(table.columns, width);
            let empty = cx.looks.body().line_height.round() as i32;
            let mut y = 0;
            for row in &mut table.rows {
                let mut x = 0;
                let mut tallest = 0;
                for cell in &mut row.cells {
                    cell.x = x;
                    cell.width = column * cell.span;
                    x += cell.width;
                    let inner = cell_inner(cell.width);
                    let mut h = CELL_TOP;
                    for n in &mut cell.nodes {
                        n.y = h;
                        h += measure(n, inner, cx);
                    }
                    if cell.nodes.is_empty() {
                        h += empty;
                    }
                    tallest = tallest.max(h + CELL_TOP);
                }
                row.y = y;
                row.height = tallest;
                y += tallest;
            }
            2 * TABLE_MARGIN + y
        }
    }
    .max(1);
    node.laid = width;
    node.laid_height = height;
    node.height = height;
    height
}

/// `node`'s estimated height at `width`, from character counts and line metrics: what it
/// shows until it is measured.
pub(crate) fn estimate(node: &Node, width: i32, leaves: &[Leaf], looks: &Looks) -> i32 {
    if node.laid == width {
        return node.laid_height;
    }
    let height = match &node.kind {
        Kind::Text(b) => {
            let m = looks.metrics(b.look);
            let text = estimate_text(&leaves[b.leaf.index()], m, b.line, b.wrap(width));
            b.above + text + b.below
        }
        Kind::Picture { leaf, natural, .. } => match natural.flatten() {
            Some(size) => fit(size, width).1,
            None => {
                let body = looks.body();
                let label = leaves[leaf.index()].chars.unwrap_or(0) + 11;
                let lines = (f64::from(label) * body.char_width / f64::from(width.max(1)))
                    .ceil()
                    .max(1.0);
                (lines * body.line_height).round() as i32
            }
        },
        Kind::PageBreak { .. } => PAGE_BREAK_HEIGHT,
        Kind::Rule { .. } => RULE_HEIGHT,
        Kind::Table(table) => {
            let column = column_width(table.columns, width);
            let empty = looks.body().line_height.round() as i32;
            let rows: i32 = table
                .rows
                .iter()
                .map(|row| {
                    row.cells
                        .iter()
                        .map(|cell| {
                            let inner = cell_inner(column * cell.span);
                            let content: i32 = cell
                                .nodes
                                .iter()
                                .map(|n| estimate(n, inner, leaves, looks))
                                .sum();
                            2 * CELL_TOP
                                + if cell.nodes.is_empty() {
                                    empty
                                } else {
                                    content
                                }
                        })
                        .max()
                        .unwrap_or(0)
                })
                .sum();
            2 * TABLE_MARGIN + rows
        }
    };
    height.max(1)
}

fn estimate_text(leaf: &Leaf, m: Metrics, line: LineSpacing, width: i32) -> i32 {
    let per_line = f64::from(width.max(1)) / m.char_width.max(0.1);
    let lines: f64 = leaf
        .lines
        .iter()
        .map(|&n| (f64::from(n) / per_line).ceil().max(1.0))
        .sum();
    (lines.max(1.0) * line_height(m.line_height, line)).round() as i32
}

/// How far apart lines of a font `natural` px tall are with spacing `line`.
fn line_height(natural: f64, line: LineSpacing) -> f64 {
    match line {
        LineSpacing::Auto(m) => natural * f64::from(m),
        LineSpacing::Exact(t) => f64::from(t) / TWIPS_PER_PX,
        LineSpacing::AtLeast(t) => natural.max(f64::from(t) / TWIPS_PER_PX),
    }
}

/// What painting needs besides the node.
pub(crate) struct PaintCx<'a, 'f> {
    pub doc: &'a Document,
    pub leaves: &'a [Leaf],
    pub selection: Option<(Position, Position)>,
    /// The document's visible band.
    pub top: f64,
    pub bottom: f64,
    pub f: &'f mut dyn FnMut(Paint<'a>),
}

impl PaintCx<'_, '_> {
    /// The bytes of `leaf`'s text the selection covers.
    fn selected(&self, leaf: LeafId) -> Option<Range<usize>> {
        selected_bytes(
            self.selection,
            leaf,
            self.leaves[leaf.index()].bytes as usize,
        )
    }
}

/// The bytes of a leaf `bytes` long that `selection` covers, if any.
pub(crate) fn selected_bytes(
    selection: Option<(Position, Position)>,
    leaf: LeafId,
    bytes: usize,
) -> Option<Range<usize>> {
    let (s, e) = selection?;
    if leaf < s.leaf || leaf > e.leaf {
        return None;
    }
    let start = if leaf == s.leaf {
        s.offset.min(bytes)
    } else {
        0
    };
    let end = if leaf == e.leaf {
        e.offset.min(bytes)
    } else {
        bytes
    };
    (start < end || (s.leaf < leaf && leaf < e.leaf)).then_some(start..end)
}

/// Paint `node`, whose box starts at (`x`, `y`) and is `width` wide.
pub(crate) fn paint<'a>(node: &'a Node, x: i32, y: f64, width: i32, cx: &mut PaintCx<'a, '_>) {
    match &node.kind {
        Kind::Text(b) => {
            if let Some(frame) = &b.frame {
                paint_frame(frame, b, node.height, (x, y, width), &mut *cx.f);
            }
            if let Some(band) = b.band {
                (cx.f)(Paint::Band {
                    x: x + band,
                    y,
                    width: width - band,
                    height: node.height,
                });
            }
            let leaf = &cx.leaves[b.leaf.index()];
            let content = content_at(cx.doc, &leaf.path).expect("text leaf");
            let selected = cx.selected(b.leaf);
            (cx.f)(Paint::Text {
                leaf: b.leaf,
                key: leaf.key,
                content,
                x: x + b.indent,
                y: y + f64::from(b.above),
                width: b.wrap(width),
                marker: b.marker,
                selected,
            });
        }
        Kind::Picture { leaf, natural, .. } => {
            let image = image_at(cx.doc, &cx.leaves[leaf.index()].path);
            let selected = cx.selected(*leaf).is_some();
            match natural.flatten() {
                Some(size) => {
                    let (w, h) = fit(size, width);
                    (cx.f)(Paint::Picture {
                        leaf: *leaf,
                        image,
                        x,
                        y,
                        width: w,
                        height: h,
                        selected,
                    });
                }
                None => (cx.f)(Paint::Text {
                    leaf: *leaf,
                    key: cx.leaves[leaf.index()].key,
                    content: Content::PictureLabel(image),
                    x,
                    y,
                    width: width.max(1),
                    marker: false,
                    selected: selected.then(|| 0..crate::content::picture_label(image).len()),
                }),
            }
        }
        Kind::PageBreak { index, .. } => (cx.f)(Paint::PageBreak {
            index: *index,
            x,
            y,
            width,
            height: PAGE_BREAK_HEIGHT,
        }),
        Kind::Rule { .. } => (cx.f)(Paint::Rule {
            x,
            y: y + f64::from(RULE_MARGIN),
            width,
        }),
        Kind::Table(table) => {
            if node.laid != width {
                return;
            }
            let top = y + f64::from(TABLE_MARGIN);
            for row in &table.rows {
                let ry = top + f64::from(row.y);
                if ry + f64::from(row.height) <= cx.top {
                    continue;
                }
                if ry >= cx.bottom {
                    break;
                }
                for cell in &row.cells {
                    (cx.f)(Paint::Border {
                        x: x + cell.x,
                        y: ry,
                        width: cell.width,
                        height: row.height,
                    });
                    let inner = cell_inner(cell.width);
                    let cx0 = x + cell.x + CELL_BORDER + CELL_PADDING;
                    for n in &cell.nodes {
                        let ny = ry + f64::from(n.y);
                        if ny < cx.bottom && ny + f64::from(n.height) > cx.top {
                            paint(n, cx0, ny, inner, cx);
                        }
                    }
                }
            }
        }
    }
}

/// Paint frame `f` of text box `b` in a node `height` tall, whose box starts at (`x`, `y`)
/// and is `width` wide: its shading, then its lines, each drawn inward from the box's edge.
fn paint_frame<'a>(
    f: &Frame,
    b: &TextBox,
    height: i32,
    (x, y, width): (i32, f64, i32),
    out: &mut dyn FnMut(Paint<'a>),
) {
    let (top, bottom, left, right) = f.zone;
    let (before, after) = (b.above - top, b.below - bottom);
    let x0 = x + b.indent - left;
    let x1 = x + b.indent + b.wrap(width) + right;
    let y0 = if f.open.0 { 0 } else { before };
    let y1 = if f.open.1 { height } else { height - after };
    if let Some(color) = f.fill {
        out(Paint::Fill {
            x: x0,
            y: y + f64::from(y0),
            width: x1 - x0,
            height: y1 - y0,
            color,
        });
    }
    // The strokes of line `l`, as offsets from the box's edge towards the text: a double
    // line's second lies a stroke's width inside its first.
    let offsets = |l: &Border| {
        let t = stroke(l);
        let n = match l.style {
            BorderStyle::None => 0,
            BorderStyle::Double => 2,
            _ => 1,
        };
        (0..n).map(move |i| i * 2 * t)
    };
    let dash = |l: &Border| match l.style {
        BorderStyle::Dotted => Dash::Dotted,
        BorderStyle::Dashed => Dash::Dashed,
        _ => Dash::Solid,
    };
    for (l, inward) in [(&f.top, true), (&f.bottom, false)] {
        let t = stroke(l);
        for o in offsets(l) {
            let sy = if inward {
                before + o
            } else {
                height - after - o - t
            };
            out(Paint::Stroke {
                x: x0,
                y: y + f64::from(sy),
                width: x1 - x0,
                height: t,
                dash: dash(l),
                color: l.color,
            });
        }
    }
    for (l, inward) in [(&f.left, true), (&f.right, false)] {
        let t = stroke(l);
        for o in offsets(l) {
            let sx = if inward { x0 + o } else { x1 - o - t };
            out(Paint::Stroke {
                x: sx,
                y: y + f64::from(y0),
                width: t,
                height: y1 - y0,
                dash: dash(l),
                color: l.color,
            });
        }
    }
}

/// Where (`x`, `y`) in `node`'s box falls, the box `width` wide.
pub(crate) fn hit(
    node: &Node,
    x: f64,
    y: f64,
    width: i32,
    doc: &Document,
    leaves: &[Leaf],
    shaper: &mut dyn Shaper,
) -> Located {
    match &node.kind {
        Kind::Text(b) => {
            let leaf = &leaves[b.leaf.index()];
            let content = content_at(doc, &leaf.path).expect("text leaf");
            let wrap = b.wrap(width);
            let ty = (y - f64::from(b.above)).clamp(0.0, f64::from(b.text_height - 1).max(0.0));
            let h = shaper.hit(leaf.key, content, wrap, x - f64::from(b.indent), ty);
            let text = content.text();
            let index = floor_boundary(&text, h.index);
            let offset = if h.trailing {
                text[index..]
                    .chars()
                    .next()
                    .map_or(index, |c| index + c.len_utf8())
            } else {
                index
            };
            let inside =
                h.inside && y >= f64::from(b.above) && y < f64::from(b.above + b.text_height);
            Located {
                position: Position {
                    leaf: b.leaf,
                    offset,
                },
                over: inside.then_some(Position {
                    leaf: b.leaf,
                    offset: index,
                }),
            }
        }
        Kind::Picture { leaf, natural, .. } => {
            let shown = natural.flatten().map_or(width, |s| fit(s, width).0);
            let end = leaves[leaf.index()].bytes as usize;
            Located {
                position: Position {
                    leaf: *leaf,
                    offset: if x * 2.0 > f64::from(shown) { end } else { 0 },
                },
                over: None,
            }
        }
        Kind::PageBreak { leaf, .. } | Kind::Rule { leaf } => Located {
            position: Position {
                leaf: *leaf,
                offset: 0,
            },
            over: None,
        },
        Kind::Table(table) => {
            let ty = y - f64::from(TABLE_MARGIN);
            let Some(row) = table
                .rows
                .iter()
                .find(|r| ty < f64::from(r.y + r.height))
                .or(table.rows.last())
            else {
                return Located::start_of(after_table(node, leaves));
            };
            let Some(cell) = row
                .cells
                .iter()
                .find(|c| x < f64::from(c.x + c.width))
                .or(row.cells.last())
            else {
                return Located::start_of(after_table(node, leaves));
            };
            let cy = ty - f64::from(row.y);
            let n = cell
                .nodes
                .iter()
                .rev()
                .find(|n| f64::from(n.y) <= cy)
                .or(cell.nodes.first());
            match n {
                Some(n) => hit(
                    n,
                    x - f64::from(cell.x + CELL_BORDER + CELL_PADDING),
                    cy - f64::from(n.y),
                    cell_inner(cell.width),
                    doc,
                    leaves,
                    shaper,
                ),
                None => Located::start_of(clamp_leaf(cell.first_leaf, leaves)),
            }
        }
    }
}

/// The first leaf after a table's leaves (or the table's first leaf for an empty table).
fn after_table(node: &Node, leaves: &[Leaf]) -> Position {
    match &node.kind {
        Kind::Table(t) => clamp_leaf(
            t.rows
                .first()
                .and_then(|r| r.cells.first())
                .map_or(LeafId(leaves.len() as u32), |c| c.first_leaf),
            leaves,
        ),
        _ => unreachable!(),
    }
}

/// `leaf`'s start, or the last leaf's end when `leaf` is past it.
fn clamp_leaf(leaf: LeafId, leaves: &[Leaf]) -> Position {
    if leaf.index() < leaves.len() {
        Position { leaf, offset: 0 }
    } else {
        let last = leaves.len().saturating_sub(1);
        Position {
            leaf: LeafId(last as u32),
            offset: leaves.get(last).map_or(0, |l| l.bytes as usize),
        }
    }
}
