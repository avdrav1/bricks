//! The flow's layout, scroll anchoring, hit-testing, and selection, with a fake shaper:
//! every character is 10 px wide, every line 20 px tall, and text wraps at any character.

use document_model::{
    Block, Document, Format, Image, ImageFormat, ImageSource, Paragraph, ParagraphStyle, Run,
    RunStyle, Table, TableCell, TableRow,
};
use flow::{
    Content, Flow, Hit, LeafId, Look, Metrics, Paint, Position, Shaper, BOTTOM_MARGIN, CELL_BORDER,
    CELL_PADDING, PAGE_BREAK_HEIGHT, TABLE_MARGIN, TOP_MARGIN,
};
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
    fn metrics(&mut self, _look: Look) -> Metrics {
        Metrics {
            char_width: self.estimated_char.unwrap_or(CHAR),
            line_height: f64::from(LINE),
        }
    }

    fn shape(&mut self, _leaf: LeafId, content: Content<'_>, width: i32) -> i32 {
        self.shaped += 1;
        lines(&content.text(), width).len() as i32 * LINE
    }

    fn hit(&mut self, _leaf: LeafId, content: Content<'_>, width: i32, x: f64, y: f64) -> Hit {
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
}

fn para(text: &str) -> Block {
    let mut p = Paragraph::new(ParagraphStyle::Normal);
    p.push(text, &RunStyle::default());
    Block::Paragraph(p)
}

fn doc(blocks: Vec<Block>) -> Document {
    Document {
        blocks,
        format: Format::Docx,
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
    let mut p = Paragraph::new(ParagraphStyle::Normal);
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
