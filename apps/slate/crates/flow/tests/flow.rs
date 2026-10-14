//! The flow's layout, scroll anchoring, hit-testing, and selection, with a fake shaper:
//! every character is 10 px wide, every line 20 px tall (or as its paragraph's line
//! spacing says), and text wraps at any character.

use document_model::{
    Block, Document, Format, Image, ImageFormat, ImageSource, LineSpacing, ParaLook, Paragraph,
    Run, RunStyle, Style, StyleSheet, Table, TableCell, TableRow,
};
use flow::{
    Content, Flow, Hit, LayoutKey, LeafId, Look, Metrics, Paint, Position, Shaper, Splice,
    BOTTOM_MARGIN, CELL_BORDER, CELL_PADDING, LEVEL_INDENT, PAGE_BREAK_HEIGHT, TABLE_MARGIN,
    TOP_MARGIN,
};
use std::collections::HashSet;
use std::sync::Arc;

const CHAR: f64 = 10.0;
const LINE: i32 = 20;
/// Paragraphs have this much space below them (Markdown and DOCX).
const SPACE: i32 = 8;

#[derive(Default)]
struct Fake {
    /// Every `shape` call.
    shaped: usize,
    /// What estimates assume a character's width is (the truth is [`CHAR`]).
    estimated_char: Option<f64>,
    /// Layouts the flow said are gone.
    forgotten: HashSet<LayoutKey>,
    /// Lines a layout has beyond its text's, as an input method's preedit adds.
    extra: std::collections::HashMap<LayoutKey, i32>,
    /// Every look the flow asked metrics for, in order.
    asked: Vec<Look>,
}

/// Lines of `text` wrapped `width` px wide: (byte start, byte end) of each.
fn lines(text: &str, width: i32) -> Vec<(usize, usize)> {
    let per = ((f64::from(width) / CHAR) as usize).max(1);
    let mut out = Vec::new();
    let mut start = 0;
    for hard in text.split('\n') {
        let chars: Vec<usize> = hard.char_indices().map(|(i, _)| start + i).collect();
        if chars.is_empty() {
            out.push((start, start));
        }
        for chunk in chars.chunks(per) {
            let end = chunk
                .last()
                .map_or(start, |&i| i + text[i..].chars().next().unwrap().len_utf8());
            out.push((chunk[0], end));
        }
        start += hard.len() + 1;
    }
    out
}

impl Shaper for Fake {
    fn metrics(&mut self, look: &Look) -> Metrics {
        self.asked.push(look.clone());
        Metrics {
            char_width: self.estimated_char.unwrap_or(CHAR),
            line_height: f64::from(LINE),
        }
    }

    fn shape(&mut self, key: LayoutKey, content: Content<'_>, width: i32) -> i32 {
        self.shaped += 1;
        let line = match content.line_spacing() {
            LineSpacing::Auto(m) => f64::from(LINE) * f64::from(m),
            LineSpacing::Exact(t) => f64::from(t) / 15.0,
            LineSpacing::AtLeast(t) => f64::from(LINE).max(f64::from(t) / 15.0),
        };
        let lines =
            lines(&content.text(), width).len() as i32 + self.extra.get(&key).copied().unwrap_or(0);
        (f64::from(lines) * line).round() as i32
    }

    fn hit(&mut self, _key: LayoutKey, content: Content<'_>, width: i32, x: f64, y: f64) -> Hit {
        let text = content.text();
        let lines = lines(&text, width);
        let line = ((y / f64::from(LINE)) as usize).min(lines.len() - 1);
        let (start, end) = lines[line];
        let chars: Vec<usize> = text[start..end]
            .char_indices()
            .map(|(i, _)| start + i)
            .collect();
        let column = (x.max(0.0) / CHAR) as usize;
        match chars.get(column) {
            Some(&index) => Hit {
                index,
                trailing: x / CHAR - column as f64 >= 0.5,
                inside: true,
            },
            None => Hit {
                index: chars.last().copied().unwrap_or(start),
                trailing: !chars.is_empty(),
                inside: false,
            },
        }
    }

    fn picture_size(&mut self, image: &Image) -> Option<(f32, f32)> {
        match &image.source {
            ImageSource::Embedded { .. } => Some((400.0, 300.0)),
            ImageSource::Link(_) => None,
        }
    }

    fn forget(&mut self, key: LayoutKey) {
        self.forgotten.insert(key);
    }
}

fn para(text: &str) -> Block {
    let mut p = Paragraph::new("Normal");
    p.push(text, &RunStyle::default());
    Block::Paragraph(p)
}

fn doc(blocks: Vec<Block>) -> Document {
    Document {
        blocks,
        styles: StyleSheet::builtin(),
        format: Format::Docx(Default::default()),
    }
}

fn cell(blocks: Vec<Block>) -> TableCell {
    TableCell { blocks, span: 1 }
}

fn table(rows: Vec<Vec<TableCell>>) -> Block {
    Block::Table(Table {
        rows: rows
            .into_iter()
            .map(|cells| TableRow {
                cells,
                header: false,
            })
            .collect(),
    })
}

/// A picture the fake shows at 400 x 300.
fn picture(alt: &str) -> Block {
    Block::Image(Image {
        alt: alt.to_owned(),
        source: ImageSource::Embedded {
            format: ImageFormat::Png,
            bytes: Arc::from(&b"\x89PNG"[..]),
        },
        size: None,
    })
}

/// `n` paragraphs, the `i`th `10 + i % 7 * 13` characters long.
fn many(n: usize) -> Vec<Block> {
    (0..n).map(|i| para(&"w".repeat(10 + i % 7 * 13))).collect()
}

fn laid_out(blocks: Vec<Block>, width: i32, shaper: &mut Fake) -> Flow {
    let mut flow = Flow::new(doc(blocks));
    flow.set_width(width, shaper);
    flow.layout_slice(shaper, &mut || true);
    flow
}

/// Every text painted between `top` and `bottom`: its leaf and y.
fn texts(flow: &Flow, top: f64, bottom: f64) -> Vec<(LeafId, f64)> {
    let mut out = Vec::new();
    flow.paint(top, bottom, &mut |p| {
        if let Paint::Text { leaf, y, .. } = p {
            out.push((leaf, y));
        }
    });
    out
}

/// The flow's text, layout keys aside: every leaf's text and the document's height.
fn shape_of(flow: &Flow) -> (Vec<Option<String>>, f64, usize) {
    let texts = (0..flow.leaf_count())
        .map(|i| {
            flow.leaf_text(LeafId::from_index(i))
                .map(|t| t.into_owned())
        })
        .collect();
    (texts, flow.content_height(), flow.char_count())
}

#[test]
fn an_edit_rebuilds_only_its_blocks_and_keeps_the_viewport() {
    let mut fake = Fake::default();
    let mut blocks = many(60);
    blocks.insert(30, table(vec![vec![cell(vec![para("in a cell")])]]));
    let mut flow = laid_out(blocks, 100, &mut fake);
    flow.scroll_to(1500.0);
    flow.layout_viewport(&mut fake, 400.0);
    let shown = |flow: &Flow| -> Vec<(Option<String>, f64)> {
        texts(flow, flow.scroll(), flow.scroll() + 400.0)
            .into_iter()
            .map(|(l, y)| (flow.leaf_text(l).map(|t| t.into_owned()), y - flow.scroll()))
            .collect()
    };
    let before = shown(&flow);
    let edited_key = flow.layout_key(flow.leaf_at(&[2]).unwrap());

    // Typing in block 2, far above the viewport: one block, same counts.
    let shaped = fake.shaped;
    flow.edit(&mut fake, |doc| {
        let Block::Paragraph(p) = &mut doc.blocks[2] else {
            unreachable!()
        };
        p.runs[0].text.push_str(&"x".repeat(25));
        (
            (),
            Splice {
                start: 2,
                removed: 1,
                inserted: 1,
            },
        )
    });
    assert!(fake.forgotten.contains(&edited_key));
    flow.layout_viewport(&mut fake, 400.0);
    assert_eq!(
        shown(&flow),
        before,
        "the viewport shows the same text at the same place"
    );
    flow.layout_slice(&mut fake, &mut || true);
    assert_eq!(
        fake.shaped - shaped,
        1,
        "only the edited block was shaped again"
    );
    assert_eq!(shown(&flow), before);

    // Enter in block 5: one block becomes two; every leaf after it is renumbered.
    flow.edit(&mut fake, |doc| {
        doc.blocks.insert(6, para("a new paragraph"));
        (
            (),
            Splice {
                start: 5,
                removed: 1,
                inserted: 2,
            },
        )
    });
    flow.layout_viewport(&mut fake, 400.0);
    assert_eq!(shown(&flow), before);
    // Joining two paragraphs below the viewport: two blocks become one.
    flow.edit(&mut fake, |doc| {
        doc.blocks.remove(50);
        (
            (),
            Splice {
                start: 49,
                removed: 2,
                inserted: 1,
            },
        )
    });
    flow.layout_slice(&mut fake, &mut || true);
    assert_eq!(shown(&flow), before);

    // The same as a flow made from the edited document.
    let fresh = laid_out(flow.document().blocks.clone(), 100, &mut Fake::default());
    assert_eq!(shape_of(&flow), shape_of(&fresh));
    let cell = (0..fresh.leaf_count())
        .find(|&i| fresh.path(LeafId::from_index(i)).len() > 1)
        .unwrap();
    assert_eq!(
        flow.path(LeafId::from_index(cell)),
        fresh.path(LeafId::from_index(cell))
    );
}

/// A text's left edge, top, and wrap width.
type Place = (i32, f64, i32);

/// Every text leaf's place: what `text_place` says, and where `paint` draws it.
fn places(flow: &mut Flow, fake: &mut Fake) -> Vec<(Place, Place)> {
    let mut painted = Vec::new();
    flow.paint(0.0, f64::MAX, &mut |p| {
        if let Paint::Text {
            leaf, x, y, width, ..
        } = p
        {
            painted.push((leaf, (x, y, width)));
        }
    });
    painted
        .into_iter()
        .map(|(leaf, at)| {
            let p = flow.text_place(leaf, fake).unwrap();
            ((p.x, p.y, p.width), at)
        })
        .collect()
}

#[test]
fn a_cells_growth_relays_out_only_its_table() {
    let mut fake = Fake::default();
    let mut blocks = many(20);
    blocks.insert(
        10,
        table(vec![
            vec![cell(vec![para("a")]), cell(vec![para("b")])],
            vec![cell(vec![para("c")]), cell(vec![para("d"), para("e")])],
        ]),
    );
    let mut flow = laid_out(blocks, 200, &mut fake);
    for (place, painted) in places(&mut flow, &mut fake) {
        assert_eq!(place, painted, "text_place is where paint draws the text");
    }
    let after_table = |flow: &mut Flow, fake: &mut Fake| {
        let leaf = flow.leaf_at(&[11]).unwrap();
        flow.text_place(leaf, fake).unwrap().y
    };
    let before_table = flow
        .text_place(flow.leaf_at(&[9]).unwrap(), &mut fake)
        .unwrap()
        .y;
    let below = after_table(&mut flow, &mut fake);
    let cell_b = flow.leaf_at(&[10, 0, 1, 0]).unwrap();
    let line = flow.text_place(cell_b, &mut fake).unwrap().width / 10;

    // Cell b grows by two lines: only the table is laid out again.
    let shaped = fake.shaped;
    flow.edit(&mut fake, |doc| {
        let Block::Table(t) = &mut doc.blocks[10] else {
            unreachable!()
        };
        let Block::Paragraph(p) = &mut t.rows[0].cells[1].blocks[0] else {
            unreachable!()
        };
        p.runs[0].text.push_str(&"x".repeat(2 * line as usize));
        (
            (),
            Splice {
                start: 10,
                removed: 1,
                inserted: 1,
            },
        )
    });
    flow.layout_slice(&mut fake, &mut || true);
    assert_eq!(
        fake.shaped - shaped,
        5,
        "the table's five cell paragraphs, nothing else"
    );
    assert_eq!(
        flow.text_place(flow.leaf_at(&[9]).unwrap(), &mut fake)
            .unwrap()
            .y,
        before_table
    );
    assert_eq!(
        after_table(&mut flow, &mut fake),
        below + f64::from(2 * LINE)
    );
    for (place, painted) in places(&mut flow, &mut fake) {
        assert_eq!(place, painted);
    }
}

#[test]
fn a_leaf_measured_again_takes_its_new_height_in_place() {
    let mut fake = Fake::default();
    let mut blocks = many(10);
    blocks.insert(
        5,
        table(vec![vec![cell(vec![para("a")]), cell(vec![para("b")])]]),
    );
    let mut flow = laid_out(blocks, 200, &mut fake);
    let height = flow.content_height();
    let y = |flow: &mut Flow, fake: &mut Fake, path: &[u32]| {
        let leaf = flow.leaf_at(path).unwrap();
        flow.text_place(leaf, fake).unwrap().y
    };
    let (before, after) = (y(&mut flow, &mut fake, &[4]), y(&mut flow, &mut fake, &[6]));

    // The shaper lays the top-level paragraph and the cell's text out two lines taller.
    for path in [&[2u32][..], &[5, 0, 1, 0]] {
        let leaf = flow.leaf_at(path).unwrap();
        fake.extra.insert(flow.layout_key(leaf), 2);
        flow.remeasure(leaf);
    }
    assert!(!flow.is_complete());
    flow.layout_slice(&mut fake, &mut || true);
    assert!(flow.is_complete());
    assert_eq!(flow.content_height(), height + f64::from(4 * LINE));
    assert_eq!(y(&mut flow, &mut fake, &[4]), before + f64::from(2 * LINE));
    assert_eq!(y(&mut flow, &mut fake, &[6]), after + f64::from(4 * LINE));
}

#[test]
fn the_height_index_sums_measured_blocks() {
    let mut fake = Fake::default();
    // 100 px wide: 10 characters a line. 25, 10, and 0 characters: 3, 1, and 1 lines.
    let flow = laid_out(
        vec![para(&"a".repeat(25)), para(&"b".repeat(10)), para("")],
        100,
        &mut fake,
    );
    assert!(flow.is_complete());
    let blocks = (3 + 1 + 1) * LINE + 3 * SPACE;
    assert_eq!(
        flow.content_height(),
        f64::from(TOP_MARGIN + blocks + BOTTOM_MARGIN)
    );
    let ys: Vec<f64> = texts(&flow, 0.0, 1e9).iter().map(|t| t.1).collect();
    let top = f64::from(TOP_MARGIN);
    assert_eq!(
        ys,
        [
            top,
            top + f64::from(3 * LINE + SPACE),
            top + f64::from(4 * LINE + 2 * SPACE)
        ]
    );
}

#[test]
fn y_finds_its_block_and_paint_covers_only_the_band() {
    let mut fake = Fake::default();
    let flow = laid_out(many(1000), 100, &mut fake);
    let all = texts(&flow, 0.0, 1e9);
    assert_eq!(all.len(), 1000);
    let (leaf, y) = all[500];
    // A band starting inside block 500 starts painting there and stops at the band's end.
    let band = texts(&flow, y + 5.0, y + 200.0);
    assert_eq!(band[0], (leaf, y));
    assert!(band.iter().all(|&(_, by)| by < y + 200.0));
    let next = all.iter().find(|t| t.1 >= y + 200.0).unwrap();
    assert!(!band.contains(next));
    let hit = flow.hit(&mut fake, 5.0, y + 1.0).unwrap();
    assert_eq!(hit.position.leaf, leaf);
}

#[test]
fn replacing_estimates_never_moves_the_viewport() {
    // Estimates think characters are 8 px wide; they are 10.
    let mut fake = Fake {
        estimated_char: Some(8.0),
        ..Fake::default()
    };
    let mut flow = Flow::new(doc(many(2000)));
    flow.set_width(200, &mut fake);
    assert!(!flow.is_complete());
    let estimated = flow.content_height();
    flow.scroll_to(estimated / 2.0);
    flow.layout_viewport(&mut fake, 600.0);
    let shown = texts(&flow, flow.scroll(), flow.scroll() + 600.0);
    let on_screen: Vec<(LeafId, f64)> =
        shown.iter().map(|&(l, y)| (l, y - flow.scroll())).collect();
    let before = flow.scroll();

    let progress = flow.layout_slice(&mut fake, &mut || true);
    assert!(progress.complete);
    assert!(progress.measured > 1900, "{progress:?}");
    // Blocks above grew, so the scroll value moved, but the viewport shows the same.
    assert!(
        flow.scroll() > before + 100.0,
        "{} vs {before}",
        flow.scroll()
    );
    assert!(flow.content_height() > estimated);
    let after: Vec<(LeafId, f64)> = texts(&flow, flow.scroll(), flow.scroll() + 600.0)
        .iter()
        .map(|&(l, y)| (l, y - flow.scroll()))
        .collect();
    assert_eq!(after, on_screen);
}

#[test]
fn layout_slices_stop_when_time_is_up_and_resume() {
    let mut fake = Fake::default();
    let mut flow = Flow::new(doc(many(300)));
    flow.set_width(200, &mut fake);
    let mut budget = 100;
    let first = flow.layout_slice(&mut fake, &mut || {
        budget -= 1;
        budget >= 0
    });
    assert_eq!(first.measured, 100);
    assert!(!first.complete);
    let rest = flow.layout_slice(&mut fake, &mut || true);
    assert_eq!(rest.measured, 200);
    assert!(rest.complete);
}

#[test]
fn a_width_change_keeps_the_top_block_and_reuses_heights_measured_at_the_width() {
    let mut fake = Fake::default();
    let mut flow = laid_out(many(500), 400, &mut fake);
    let wide = flow.content_height();
    flow.scroll_to(wide / 3.0);
    flow.layout_viewport(&mut fake, 500.0);
    let top = texts(&flow, flow.scroll(), flow.scroll() + 1.0)[0].0;

    // Narrower: estimated until measured, the viewport still on the same block.
    flow.set_width(100, &mut fake);
    assert!(!flow.is_complete());
    let shaped = fake.shaped;
    flow.layout_viewport(&mut fake, 500.0);
    let visible = fake.shaped - shaped;
    assert!((1..40).contains(&visible), "{visible}");
    assert_eq!(texts(&flow, flow.scroll(), flow.scroll() + 1.0)[0].0, top);

    // Back before the narrow pass finished: only the blocks measured at 100 need measuring.
    flow.set_width(400, &mut fake);
    let shaped = fake.shaped;
    let progress = flow.layout_slice(&mut fake, &mut || true);
    assert_eq!(progress.measured, visible);
    assert_eq!(fake.shaped - shaped, visible);
    assert_eq!(flow.content_height(), wide);
    assert_eq!(texts(&flow, flow.scroll(), flow.scroll() + 1.0)[0].0, top);

    flow.set_width(100, &mut fake);
    flow.layout_slice(&mut fake, &mut || true);
    assert!(flow.content_height() > 2.0 * wide);
}

#[test]
fn a_table_row_is_as_tall_as_its_tallest_cell() {
    let mut fake = Fake::default();
    // 2 columns of 100 px: cells 86 px inside their padding and border: 8 characters a line.
    let nested = table(vec![vec![cell(vec![para("x")])]]);
    let flow = laid_out(
        vec![table(vec![
            vec![cell(vec![para(&"a".repeat(20))]), cell(vec![para("b")])],
            vec![cell(vec![]), cell(vec![para("c"), nested])],
        ])],
        200,
        &mut fake,
    );
    let pad = 2 * (CELL_BORDER + CELL_PADDING / 2);
    let row1 = pad + 3 * LINE + SPACE;
    let nested_table = 2 * TABLE_MARGIN + pad + LINE + SPACE;
    let row2 = pad + LINE + SPACE + nested_table;
    assert_eq!(
        flow.content_height(),
        f64::from(TOP_MARGIN + 2 * TABLE_MARGIN + row1 + row2 + BOTTOM_MARGIN)
    );
    let mut borders = Vec::new();
    flow.paint(0.0, 1e9, &mut |p| {
        if let Paint::Border {
            x, width, height, ..
        } = p
        {
            borders.push((x, width, height));
        }
    });
    assert_eq!(
        borders,
        [
            (0, 100, row1),
            (100, 100, row1),
            (0, 100, row2),
            (100, 100, row2),
            (100 + CELL_BORDER + CELL_PADDING, 86, pad + LINE + SPACE),
        ]
    );
}

#[test]
fn pictures_fit_the_column_and_page_breaks_have_their_height() {
    let mut fake = Fake::default();
    let flow = laid_out(vec![picture("A cat"), Block::PageBreak], 200, &mut fake);
    // 400 x 300 scaled to 200 wide.
    assert_eq!(
        flow.content_height(),
        f64::from(TOP_MARGIN + 150 + PAGE_BREAK_HEIGHT + BOTTOM_MARGIN)
    );
    let mut breaks = Vec::new();
    flow.page_breaks(&mut |p| breaks.push(format!("{p:?}")));
    assert_eq!(breaks.len(), 1);
}

#[test]
fn a_point_hits_the_character_under_it() {
    let mut fake = Fake::default();
    let flow = laid_out(
        vec![
            para(&("abcdefghij".repeat(2) + "abcde")),
            table(vec![vec![
                cell(vec![para("left")]),
                cell(vec![para("right")]),
            ]]),
        ],
        100,
        &mut fake,
    );
    let top = f64::from(TOP_MARGIN);
    // Second line, fourth character's leading half: before "n" (byte 13).
    let hit = flow.hit(&mut fake, 32.0, top + 25.0).unwrap();
    assert_eq!(
        hit.position,
        Position {
            leaf: LeafId::from_index(0),
            offset: 13
        }
    );
    assert_eq!(hit.over.map(|p| p.offset), Some(13));
    // Its trailing half: after it.
    assert_eq!(
        flow.hit(&mut fake, 37.0, top + 25.0)
            .unwrap()
            .position
            .offset,
        14
    );
    // Past the end of a line: its end, not over text.
    let beyond = flow.hit(&mut fake, 99.0, top + 50.0).unwrap();
    assert_eq!(beyond.position.offset, 25);
    assert_eq!(beyond.over, None);
    // The table's second cell, second character of "right".
    let table_top = top + f64::from(3 * LINE + SPACE + TABLE_MARGIN);
    let x = f64::from(50 + CELL_BORDER + CELL_PADDING) + 12.0;
    let in_cell = flow.hit(&mut fake, x, table_top + 10.0).unwrap();
    assert_eq!(flow.path(in_cell.position.leaf), [1, 0, 1, 0]);
    assert_eq!(in_cell.position.offset, 1);
    // Below the document: its end.
    let end = flow.hit(&mut fake, 0.0, 1e6).unwrap();
    assert_eq!(
        end.position,
        Position {
            leaf: LeafId::from_index(2),
            offset: 5
        }
    );
}

#[test]
fn the_selection_copies_as_plain_text_in_document_order() {
    let mut fake = Fake::default();
    let mut flow = laid_out(
        vec![
            para("First paragraph"),
            table(vec![
                vec![cell(vec![para("a1")]), cell(vec![para("b1")])],
                vec![cell(vec![para("a2")]), cell(vec![])],
            ]),
            picture("A cat"),
            Block::PageBreak,
            para("Last"),
        ],
        300,
        &mut fake,
    );
    let at = |leaf, offset| Position {
        leaf: LeafId::from_index(leaf),
        offset,
    };
    // Dragged backwards from "La|st" to "First |paragraph".
    flow.select(at(6, 2), at(0, 6));
    assert_eq!(flow.selected_text(), "paragraph\na1\nb1\na2\nA cat\nLa");
    flow.select_all();
    let all = flow.selected_text();
    assert_eq!(all, "First paragraph\na1\nb1\na2\nA cat\nLast");
    // The accessible text is the same text.
    assert_eq!(flow.char_count(), all.chars().count());
    assert_eq!(flow.text(0, usize::MAX), all);
    assert_eq!(flow.text(6, 18), "paragraph\na1");
    let unit = flow.unit_at(17).unwrap();
    assert_eq!(
        (unit.leaf, unit.start, unit.chars),
        (LeafId::from_index(1), 16, 2)
    );
    assert_eq!(flow.char_offset(at(4, 1)), 26);
    flow.clear_selection();
    assert_eq!(flow.selected_text(), "");
}

#[test]
fn links_are_found_by_position() {
    let mut p = Paragraph::new("Normal");
    p.push("see ", &RunStyle::default());
    p.runs.push(Run {
        text: "here".into(),
        style: RunStyle {
            link: Some("https://example.com".into()),
            ..RunStyle::default()
        },
    });
    let flow = laid_out(vec![Block::Paragraph(p)], 300, &mut Fake::default());
    let at = |offset| Position {
        leaf: LeafId::from_index(0),
        offset,
    };
    assert_eq!(flow.link_at(at(2)), None);
    assert_eq!(flow.link_at(at(5)), Some("https://example.com"));
}

/// The built-in Normal (8 px below) and a style `S` with paragraph look `para`.
fn sheet(para: ParaLook) -> StyleSheet {
    let normal = StyleSheet::builtin().default_style().clone();
    let s = Style {
        id: "S".into(),
        name: "S".into(),
        para,
        ..normal.clone()
    };
    StyleSheet::new(vec![normal, s], "Normal")
}

fn styled(style: &str, text: &str) -> Paragraph {
    let mut p = Paragraph::new(style);
    p.push(text, &RunStyle::default());
    p
}

fn flow_of(styles: StyleSheet, blocks: Vec<Block>, width: i32, shaper: &mut Fake) -> Flow {
    let mut flow = Flow::new(Document {
        blocks,
        styles,
        format: Format::Docx(Default::default()),
    });
    flow.set_width(width, shaper);
    flow.layout_slice(shaper, &mut || true);
    flow
}

#[test]
fn a_paragraphs_space_comes_from_its_style_and_adds_to_its_neighbours() {
    // `S` has 20 px above and 10 px below (300 and 150 twips); Normal 8 px below.
    let styles = sheet(ParaLook {
        space_before: 300,
        space_after: 150,
        ..ParaLook::default()
    });
    let blocks = ["Normal", "S", "Normal"]
        .iter()
        .map(|s| Block::Paragraph(styled(s, "x")))
        .collect();
    let flow = flow_of(styles, blocks, 100, &mut Fake::default());
    let ys: Vec<f64> = texts(&flow, 0.0, 1e9).into_iter().map(|(_, y)| y).collect();
    let top = f64::from(TOP_MARGIN);
    let line = f64::from(LINE);
    // The 8 px after the first and the 20 px before the second add up, as in Word.
    let second = top + line + 8.0 + 20.0;
    assert_eq!(ys, vec![top, second, second + line + 10.0]);
    assert_eq!(
        flow.content_height(),
        second + line + 10.0 + line + 8.0 + f64::from(BOTTOM_MARGIN)
    );
}

#[test]
fn a_paragraphs_left_and_first_line_indents_place_and_narrow_its_text() {
    // 40 px in from the left (600 twips); the first line 20 px further in (300 twips) or
    // hanging 20 px out (-300 twips), which moves the text's box to the first line.
    for (first, x) in [(0, 40), (300, 40), (-300, 20)] {
        let styles = sheet(ParaLook {
            indent_left: 600,
            indent_first: first,
            ..ParaLook::default()
        });
        let mut nested = styled("S", "nested");
        nested.indent = 1;
        let item = Block::ListItem(document_model::ListItem {
            kind: document_model::ListKind::Bullet,
            level: 0,
            paragraph: styled("S", "item"),
        });
        let blocks = vec![
            Block::Paragraph(styled("S", &"w".repeat(30))),
            Block::Paragraph(nested),
            item,
        ];
        let flow = flow_of(styles, blocks, 200, &mut Fake::default());
        let mut painted = Vec::new();
        flow.paint(0.0, 1e9, &mut |p| {
            if let Paint::Text {
                x, width, content, ..
            } = p
            {
                painted.push((x, width, content.first_indent()));
            }
        });
        let hang = (first / 15).min(0);
        assert_eq!(
            painted,
            vec![
                (x, 200 - x, first / 15),
                // A deeper quote's own levels add to the style's indent.
                (x + LEVEL_INDENT, 200 - x - LEVEL_INDENT, first / 15),
                // A list item's marker sets its indent, not its style.
                (LEVEL_INDENT, 200 - LEVEL_INDENT, 0),
            ],
            "first line {first} twips (hanging {hang})"
        );
        assert_eq!(flow.wrap_width(LeafId::from_index(0)), 200 - x);
    }
}

#[test]
fn exact_and_least_line_spacing_set_each_lines_height() {
    // 25 characters 100 px wide: 3 lines, each 20 px in the font.
    let cases = [
        (LineSpacing::Exact(360), 72),
        (LineSpacing::Exact(150), 30),
        (LineSpacing::AtLeast(360), 72),
        (LineSpacing::AtLeast(150), 60),
        (LineSpacing::Auto(1.5), 90),
    ];
    for (line, height) in cases {
        let styles = sheet(ParaLook {
            line,
            ..ParaLook::default()
        });
        let blocks = vec![Block::Paragraph(styled("S", &"w".repeat(25)))];
        let mut fake = Fake::default();
        let mut flow = Flow::new(Document {
            blocks,
            styles,
            format: Format::Docx(Default::default()),
        });
        flow.set_width(100, &mut fake);
        let estimated = flow.content_height();
        flow.layout_slice(&mut fake, &mut || true);
        let margins = f64::from(TOP_MARGIN + BOTTOM_MARGIN);
        assert_eq!(
            flow.content_height() - margins,
            f64::from(height),
            "{line:?}"
        );
        assert_eq!(estimated, flow.content_height(), "{line:?} estimated");
    }
}

#[test]
fn estimates_ask_each_font_size_and_weight_once() {
    let mono = RunStyle {
        font: Some("Liberation Mono".into()),
        ..RunStyle::default()
    };
    let mut other = Paragraph::new("Normal");
    other.push("in another font", &mono);
    let blocks = vec![
        para("body"),
        Block::Paragraph(styled("Heading1", "heading")),
        Block::Paragraph(other),
        para("body again"),
        Block::Paragraph(styled("Heading1", "another heading")),
        Block::CodeBlock(document_model::CodeBlock {
            text: "code\n".into(),
            ..Default::default()
        }),
    ];
    let mut fake = Fake::default();
    let mut flow = flow_of(StyleSheet::builtin(), blocks, 300, &mut fake);
    let look = |font: &str, size, bold| Look {
        font: font.into(),
        size,
        bold,
    };
    assert_eq!(
        fake.asked,
        vec![
            look("Liberation Sans", 22, false),
            look("Liberation Sans", 44, true),
            look("Liberation Mono", 22, false),
            look("monospace", 22, false),
        ]
    );
    // An edit asks only for a look the flow hasn't met.
    flow.edit(&mut fake, |doc| {
        doc.blocks.push(Block::Paragraph(styled("Heading2", "new")));
        doc.blocks.push(para("plain"));
        (
            (),
            Splice {
                start: 6,
                removed: 0,
                inserted: 2,
            },
        )
    });
    assert_eq!(fake.asked.len(), 5);
    assert_eq!(fake.asked[4], look("Liberation Sans", 35, true));
}
