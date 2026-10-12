//! The layout tree: one node per block, mirroring the document (a table's cells hold
//! nodes of their own), with the viewer's spacing rules, measuring through a [`Shaper`],
//! estimating without one, and walking it to paint and to hit-test.

use crate::content::{code_text, floor_boundary, Content};
use crate::{LeafId, Located, Look, Metrics, Paint, Position, Shaper};
use document_model::{Block, Document, Format, Image, ParagraphStyle};
use std::ops::Range;

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
const HEADING_ABOVE: [i32; 6] = [18, 14, 12, 10, 8, 8];
const HEADING_BELOW: i32 = 6;
const TITLE_BELOW: i32 = 12;
const CODE_ABOVE: i32 = 4;

/// The looks a shaper reports metrics for, in [`LOOKS`] order.
pub(crate) const LOOKS: [Look; 9] = [
    Look::Body,
    Look::Title,
    Look::Heading(1),
    Look::Heading(2),
    Look::Heading(3),
    Look::Heading(4),
    Look::Heading(5),
    Look::Heading(6),
    Look::Mono,
];

fn look_index(look: Look) -> usize {
    match look {
        Look::Body => 0,
        Look::Title => 1,
        Look::Heading(n) => 1 + usize::from(n.clamp(1, 6)),
        Look::Mono => 8,
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
    pub indent: i32,
    /// A list item: its marker hangs in the indent's last level.
    pub marker: bool,
    /// A code block: its band starts this far in.
    pub band: Option<i32>,
    pub look: Look,
    /// The first run's size relative to the normal size, for estimates.
    pub scale: f64,
    /// The text's height measured at `laid`.
    pub text_height: i32,
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

/// Builds the tree and the leaves of a document.
pub(crate) struct Builder {
    pub leaves: Vec<Leaf>,
    pub page_breaks: usize,
    /// Top-level blocks holding a page break, in order.
    pub break_tops: Vec<usize>,
    plain: bool,
}

impl Builder {
    pub fn new(format: &Format) -> Self {
        Builder {
            leaves: Vec::new(),
            page_breaks: 0,
            break_tops: Vec::new(),
            plain: matches!(format, Format::Text(_)),
        }
    }

    /// Nodes for `blocks`, whose paths start with `prefix`.
    pub fn blocks(&mut self, blocks: &[Block], prefix: &[u32], header: bool) -> Vec<Node> {
        let mut nodes = Vec::with_capacity(blocks.len());
        for (i, block) in blocks.iter().enumerate() {
            let mut path = prefix.to_vec();
            path.push(i as u32);
            nodes.push(self.block(block, blocks.get(i + 1), path, header));
        }
        nodes
    }

    fn leaf(&mut self, path: Vec<u32>, text: Option<&str>) -> LeafId {
        let id = LeafId(self.leaves.len() as u32);
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
        });
        id
    }

    fn block(&mut self, block: &Block, next: Option<&Block>, path: Vec<u32>, header: bool) -> Node {
        let space = if self.plain { 0 } else { PARAGRAPH_SPACE };
        let kind = match block {
            Block::Paragraph(p) => {
                let content = Content::Paragraph {
                    paragraph: p,
                    list: None,
                    header,
                };
                let leaf = self.leaf(path, Some(&content.text()));
                let mut b = TextBox::new(leaf, Look::Body, scale(p));
                b.below = space;
                b.indent = i32::from(p.indent) * LEVEL_INDENT;
                match p.style {
                    ParagraphStyle::Heading(n) => {
                        b.above = HEADING_ABOVE[usize::from(n.clamp(1, 6)) - 1];
                        b.below = HEADING_BELOW;
                        b.look = Look::Heading(n.clamp(1, 6));
                    }
                    ParagraphStyle::Title => {
                        b.below = TITLE_BELOW;
                        b.look = Look::Title;
                    }
                    ParagraphStyle::Quote => b.indent += LEVEL_INDENT,
                    ParagraphStyle::Normal => {}
                }
                Kind::Text(b)
            }
            Block::ListItem(item) => {
                let content = Content::Paragraph {
                    paragraph: &item.paragraph,
                    list: Some(item),
                    header,
                };
                let leaf = self.leaf(path, Some(&content.text()));
                let mut b = TextBox::new(leaf, Look::Body, scale(&item.paragraph));
                // A list's items sit close together; the list ends with a paragraph's space.
                b.below = if matches!(next, Some(Block::ListItem(_))) {
                    ITEM_SPACE
                } else {
                    space
                };
                b.indent = i32::from(item.level) * LEVEL_INDENT + LEVEL_INDENT;
                b.marker = true;
                Kind::Text(b)
            }
            Block::CodeBlock(code) => {
                let leaf = self.leaf(path, Some(code_text(code)));
                let mut b = TextBox::new(leaf, Look::Mono, 1.0);
                let band = i32::from(code.indent) * LEVEL_INDENT;
                b.band = Some(band);
                b.indent = band + CODE_INSET;
                b.above = CODE_ABOVE;
                b.below = PARAGRAPH_SPACE;
                Kind::Text(b)
            }
            Block::Raw(raw) => {
                let leaf = self.leaf(path, Some(&raw.source));
                let mut b = TextBox::new(leaf, Look::Mono, 1.0);
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
                        let first_leaf = LeafId(self.leaves.len() as u32);
                        cells.push(CellBox {
                            span: cell.span.max(1) as i32,
                            x: 0,
                            width: 0,
                            first_leaf,
                            nodes: self.blocks(&cell.blocks, &prefix, row.header),
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
}

/// The first run's size relative to the normal size (11 pt, 22 half-points).
fn scale(p: &document_model::Paragraph) -> f64 {
    p.runs
        .first()
        .and_then(|r| r.style.size)
        .map_or(1.0, |half| f64::from(half) / 22.0)
}

impl TextBox {
    fn new(leaf: LeafId, look: Look, scale: f64) -> Self {
        TextBox {
            leaf,
            above: 0,
            below: 0,
            indent: 0,
            marker: false,
            band: None,
            look,
            scale,
            text_height: 1,
        }
    }
}

/// What measuring needs besides the node.
pub(crate) struct Cx<'a> {
    pub doc: &'a Document,
    pub leaves: &'a mut [Leaf],
    pub shaper: &'a mut dyn Shaper,
    pub metrics: &'a [Metrics; 9],
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
    match block_at(doc, path) {
        (Block::Paragraph(p), header) => Some(Content::Paragraph {
            paragraph: p,
            list: None,
            header,
        }),
        (Block::ListItem(item), header) => Some(Content::Paragraph {
            paragraph: &item.paragraph,
            list: Some(item),
            header,
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
            let wrap = (width - b.indent).max(1);
            leaf.wrap = wrap;
            b.text_height = cx.shaper.shape(b.leaf, content, wrap).max(1);
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
                    .shape(*leaf, Content::PictureLabel(image), width)
                    .max(1);
                *label_height
            }
        },
        Kind::PageBreak { .. } => PAGE_BREAK_HEIGHT,
        Kind::Rule { .. } => RULE_HEIGHT,
        Kind::Table(table) => {
            let column = column_width(table.columns, width);
            let empty = cx.metrics[0].line_height.round() as i32;
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
pub(crate) fn estimate(node: &Node, width: i32, leaves: &[Leaf], metrics: &[Metrics; 9]) -> i32 {
    if node.laid == width {
        return node.laid_height;
    }
    let height = match &node.kind {
        Kind::Text(b) => {
            let m = metrics[look_index(b.look)];
            b.above + estimate_text(&leaves[b.leaf.index()], m, b.scale, width - b.indent) + b.below
        }
        Kind::Picture { leaf, natural, .. } => match natural.flatten() {
            Some(size) => fit(size, width).1,
            None => {
                let label = leaves[leaf.index()].chars.unwrap_or(0) + 11;
                let lines = (f64::from(label) * metrics[0].char_width / f64::from(width.max(1)))
                    .ceil()
                    .max(1.0);
                (lines * metrics[0].line_height).round() as i32
            }
        },
        Kind::PageBreak { .. } => PAGE_BREAK_HEIGHT,
        Kind::Rule { .. } => RULE_HEIGHT,
        Kind::Table(table) => {
            let column = column_width(table.columns, width);
            let empty = metrics[0].line_height.round() as i32;
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
                                .map(|n| estimate(n, inner, leaves, metrics))
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

fn estimate_text(leaf: &Leaf, m: Metrics, scale: f64, width: i32) -> i32 {
    let per_line = f64::from(width.max(1)) / (m.char_width * scale).max(0.1);
    let lines: f64 = leaf
        .lines
        .iter()
        .map(|&n| (f64::from(n) / per_line).ceil().max(1.0))
        .sum();
    (lines.max(1.0) * m.line_height * scale).round() as i32
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
            if let Some(band) = b.band {
                (cx.f)(Paint::Band {
                    x: x + band,
                    y,
                    width: width - band,
                    height: node.height,
                });
            }
            let content = content_at(cx.doc, &cx.leaves[b.leaf.index()].path).expect("text leaf");
            let selected = cx.selected(b.leaf);
            (cx.f)(Paint::Text {
                leaf: b.leaf,
                content,
                x: x + b.indent,
                y: y + f64::from(b.above),
                width: (width - b.indent).max(1),
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
            let wrap = (width - b.indent).max(1);
            let ty = (y - f64::from(b.above)).clamp(0.0, f64::from(b.text_height - 1).max(0.0));
            let h = shaper.hit(b.leaf, content, wrap, x - f64::from(b.indent), ty);
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
