//! The flow's layout, scroll anchoring, hit-testing, and selection, with a fake shaper:
//! every character is 10 px wide, every line 20 px tall (or as its paragraph's line
//! spacing says), and text wraps at any character.

use document_model::{
    Block, Border, BorderOverrides, BorderStyle, Borders, Document, DocxInfo, Format, Image,
    ImageFormat, ImageSource, LineSpacing, ListLevel, ListNumbering, MarkerAlign, MarkerSuffix,
    Numbering, ParaLook, ParaOverrides, ParaSpacing, Paragraph, Rgb, Run, RunStyle, Shading, Style,
    StyleSheet, TabAlign, TabLeader, TabStop, Table, TableCell, TableRow,
};
use flow::{
    Content, Dash, Flow, Hit, LayoutKey, LeafId, Look, Marker, MarkerWidth, Metrics, Paint,
    Position, Shaper, Splice, Tab, Tabs, BOTTOM_MARGIN, CELL_BORDER, CELL_PADDING, LEVEL_INDENT,
    PAGE_BREAK_HEIGHT, TABLE_MARGIN, TOP_MARGIN,
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
    /// The tabs each layout was measured with.
    tabs: std::collections::HashMap<LayoutKey, Option<Tabs>>,
    /// Every marker measured.
    markers: Vec<String>,
}

/// A marker's width as the fake measures it: [`CHAR`] a character, and a space's.
fn marker_width(marker: &Marker<'_>) -> MarkerWidth {
    MarkerWidth {
        marker: marker.text.chars().count() as f64 * CHAR,
        space: CHAR,
    }
}

/// Where `content` goes with its marker measured as the fake measures it.
fn place_of(content: &Content<'_>) -> flow::Place {
    let widths = match content.marker() {
        Some(m) if content.measures_marker() => marker_width(&m),
        _ => MarkerWidth::default(),
    };
    content.place(widths)
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
        self.tabs.insert(key, content.tabs(place_of(&content).left));
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

    fn marker_width(&mut self, marker: &Marker<'_>) -> MarkerWidth {
        self.markers.push(marker.text.to_owned());
        marker_width(marker)
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

/// A paragraph look of `spacing`.
fn spaced(spacing: ParaSpacing) -> ParaLook {
    ParaLook {
        spacing,
        ..ParaLook::default()
    }
}

fn styled(style: &str, text: &str) -> Paragraph {
    let mut p = Paragraph::new(style);
    p.push(text, &RunStyle::default());
    p
}

fn flow_of(styles: StyleSheet, blocks: Vec<Block>, width: i32, shaper: &mut Fake) -> Flow {
    flow_with(
        Format::Docx(Default::default()),
        styles,
        blocks,
        width,
        shaper,
    )
}

fn flow_with(
    format: Format,
    styles: StyleSheet,
    blocks: Vec<Block>,
    width: i32,
    shaper: &mut Fake,
) -> Flow {
    let mut flow = Flow::new(Document {
        blocks,
        styles,
        format,
    });
    flow.set_width(width, shaper);
    flow.layout_slice(shaper, &mut || true);
    flow
}

#[test]
fn a_paragraphs_space_comes_from_its_style_and_adds_to_its_neighbours() {
    // `S` has 20 px above and 10 px below (300 and 150 twips); Normal 8 px below.
    let styles = sheet(spaced(ParaSpacing {
        space_before: 300,
        space_after: 150,
        ..ParaSpacing::default()
    }));
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
        let styles = sheet(spaced(ParaSpacing {
            indent_left: 600,
            indent_first: first,
            ..ParaSpacing::default()
        }));
        let mut nested = styled("S", "nested");
        nested.indent = 1;
        let item = Block::ListItem(document_model::ListItem {
            kind: document_model::ListKind::Bullet,
            level: 0,
            paragraph: styled("S", "item"),
            numbering: None,
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
                painted.push((x, width, place_of(&content).first));
            }
        });
        let hang = (first / 15).min(0);
        assert_eq!(
            painted,
            vec![
                (x, 200 - x, first / 15),
                // A deeper quote's own levels add to the style's indent.
                (x + LEVEL_INDENT, 200 - x - LEVEL_INDENT, first / 15),
                // A Word list item's level (Slate's own: 720 twips in, hanging 360) sets
                // its indents over its style's: its text at 48 px, the marker's tab going
                // to the hanging indent's stop.
                (48, 200 - 48, 0),
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
        let styles = sheet(spaced(ParaSpacing {
            line,
            ..ParaSpacing::default()
        }));
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

fn tab(x: f64, align: TabAlign, leader: TabLeader) -> Tab {
    Tab { x, align, leader }
}

fn left_tab(x: f64) -> Tab {
    tab(x, TabAlign::Left, TabLeader::None)
}

fn stop(position: i32, align: TabAlign, leader: TabLeader) -> TabStop {
    TabStop {
        position,
        align,
        leader,
    }
}

/// Every text leaf's tabs as painted, after checking the shaper measured it with the same.
fn painted_tabs(flow: &Flow, fake: &Fake) -> Vec<Option<Tabs>> {
    let mut out = Vec::new();
    flow.paint(0.0, 1e9, &mut |p| {
        if let Paint::Text { key, content, .. } = p {
            let tabs = content.tabs(place_of(&content).left);
            assert_eq!(fake.tabs.get(&key), Some(&tabs), "measured as painted");
            out.push(tabs);
        }
    });
    out
}

#[test]
fn a_word_paragraphs_tabs_go_to_its_stops_then_the_default_stops() {
    // `S` is indented 40 px (600 twips), with a stop left of that (dropped) and one at
    // 1440 that the paragraph clears; the paragraph adds a centre stop and a right one
    // with a dot leader. The default stop is 360 twips (24 px).
    let styles = sheet(ParaLook {
        spacing: ParaSpacing {
            indent_left: 600,
            ..ParaSpacing::default()
        },
        tabs: vec![
            stop(300, TabAlign::Left, TabLeader::None),
            stop(1440, TabAlign::Left, TabLeader::None),
        ],
        ..ParaLook::default()
    });
    let mut own = styled("S", "Name\tCentre\tEnd");
    own.tabs = vec![
        stop(1440, TabAlign::Clear, TabLeader::None),
        stop(5400, TabAlign::Center, TabLeader::None),
        stop(10800, TabAlign::Right, TabLeader::Dot),
    ];
    let mut decimal = styled("Normal", "1.5\t2.25");
    decimal.tabs = vec![stop(3000, TabAlign::Decimal, TabLeader::None)];
    let blocks = vec![
        Block::Paragraph(styled("Normal", "a\tb")),
        Block::Paragraph(own),
        Block::Paragraph(decimal),
        // A table's cell: stops from the cell's text edge.
        table(vec![vec![cell(vec![Block::Paragraph(styled(
            "Normal", "c\td",
        ))])]]),
    ];
    let mut fake = Fake::default();
    let mut flow = Flow::new(Document {
        blocks,
        styles,
        format: Format::Docx(DocxInfo {
            default_tab: 360,
            ..DocxInfo::default()
        }),
    });
    flow.set_width(800, &mut fake);
    flow.layout_slice(&mut fake, &mut || true);
    let defaults = |from: f64| Some(Tabs::Stops(vec![left_tab(from), left_tab(from + 24.0)]));
    assert_eq!(
        painted_tabs(&flow, &fake),
        vec![
            defaults(24.0),
            Some(Tabs::Stops(vec![
                tab(360.0 - 40.0, TabAlign::Center, TabLeader::None),
                tab(720.0 - 40.0, TabAlign::Right, TabLeader::Dot),
                // The first default stop past 10800 twips is 11160 (744 px).
                left_tab(744.0 - 40.0),
                left_tab(768.0 - 40.0),
            ])),
            Some(Tabs::Stops(vec![
                tab(200.0, TabAlign::Decimal, TabLeader::None),
                left_tab(216.0),
                left_tab(240.0),
            ])),
            defaults(24.0),
        ]
    );
}

#[test]
fn default_stops_start_past_a_paragraphs_indent() {
    // 50 px in (750 twips): the default stops right of it are at 1440 and 2160 twips.
    let styles = sheet(spaced(ParaSpacing {
        indent_left: 750,
        ..ParaSpacing::default()
    }));
    let item = Block::ListItem(document_model::ListItem {
        kind: document_model::ListKind::Bullet,
        level: 0,
        paragraph: styled("Normal", "item\tx"),
        numbering: None,
    });
    let mut fake = Fake::default();
    let flow = flow_of(
        styles,
        vec![Block::Paragraph(styled("S", "a\tb")), item],
        400,
        &mut fake,
    );
    assert_eq!(
        painted_tabs(&flow, &fake),
        vec![
            Some(Tabs::Stops(vec![
                left_tab(96.0 - 50.0),
                left_tab(144.0 - 50.0)
            ])),
            // A Word list item's text starts at its level's left indent (Slate's own,
            // 720 twips), the hanging indent's implicit stop there not right of it.
            Some(Tabs::Stops(vec![
                left_tab(96.0 - 48.0),
                left_tab(144.0 - 48.0)
            ])),
        ]
    );
}

#[test]
fn a_hanging_indent_is_a_tab_stop() {
    // 96 px in (1440 twips), hanging 96: the first line starts at the edge, and its tab
    // goes to the hanging indent, Word's implicit stop; the default stops follow it.
    let styles = sheet(spaced(ParaSpacing {
        indent_left: 1440,
        indent_first: -1440,
        ..ParaSpacing::default()
    }));
    let mut fake = Fake::default();
    let blocks = vec![Block::Paragraph(styled("S", "Term\tDefinition"))];
    let flow = flow_of(styles, blocks, 400, &mut fake);
    assert_eq!(
        painted_tabs(&flow, &fake),
        vec![Some(Tabs::Stops(vec![
            left_tab(96.0),
            left_tab(144.0),
            left_tab(192.0)
        ]))]
    );
}

#[test]
fn markdown_and_text_tabs_are_four_spaces_and_code_keeps_the_shapers() {
    for format in [
        document_model::read_markdown(b"x\n").format,
        document_model::read_text(b"x\n").format,
    ] {
        let blocks = vec![
            para("a\tb"),
            Block::CodeBlock(document_model::CodeBlock {
                language: None,
                text: "\tcode\n".into(),
                indent: 0,
            }),
        ];
        let mut fake = Fake::default();
        let mut flow = Flow::new(Document {
            blocks,
            styles: StyleSheet::builtin(),
            format: format.clone(),
        });
        flow.set_width(400, &mut fake);
        flow.layout_slice(&mut fake, &mut || true);
        assert_eq!(
            painted_tabs(&flow, &fake),
            vec![Some(Tabs::Spaces(flow::TAB_SPACES)), None],
            "{format:?}"
        );
        assert_eq!(flow::TAB_SPACES, 4);
    }
}

/// Every text painted: its x, y, and width.
fn boxes(flow: &Flow) -> Vec<(i32, f64, i32)> {
    let mut out = Vec::new();
    flow.paint(0.0, 1e9, &mut |p| {
        if let Paint::Text { x, y, width, .. } = p {
            out.push((x, y, width));
        }
    });
    out
}

/// Every list item's marker as painted: its left edge.
fn markers(flow: &Flow) -> Vec<f64> {
    let mut out = Vec::new();
    flow.paint(0.0, 1e9, &mut |p| {
        if let Paint::Text {
            marker: Some(m), ..
        } = p
        {
            out.push(m);
        }
    });
    out
}

/// `f` of every text painted.
fn each_text<T>(flow: &Flow, f: impl Fn(&Content<'_>) -> T) -> Vec<T> {
    let mut out = Vec::new();
    flow.paint(0.0, 1e9, &mut |p| {
        if let Paint::Text { content, .. } = p {
            out.push(f(&content));
        }
    });
    out
}

/// A Word document's format whose numbering instance 1 has `levels`, level 0 first.
fn numbered(levels: Vec<ListLevel>) -> Format {
    let mut numbering = Numbering::default();
    for (l, def) in levels.into_iter().enumerate() {
        numbering.insert(1, l as u8, def);
    }
    Format::Docx(DocxInfo {
        numbering: Arc::new(numbering),
        ..DocxInfo::default()
    })
}

/// A Word list item of instance 1 at `level`: bulleted, or numbered with `label`.
fn word_item(level: u8, label: Option<&str>, text: &str) -> Block {
    Block::ListItem(document_model::ListItem {
        kind: match label {
            Some(label) => document_model::ListKind::Ordered {
                number: 1,
                label: label.into(),
            },
            None => document_model::ListKind::Bullet,
        },
        level,
        paragraph: styled("Normal", text),
        numbering: Some(ListNumbering {
            num: 1,
            mark: RunStyle::default(),
        }),
    })
}

/// A list level of `format` and `text`, left and first-line indents in twips, aligned and
/// followed as given.
fn level(
    format: &str,
    text: &str,
    (left, first): (i32, i32),
    align: MarkerAlign,
    suffix: MarkerSuffix,
) -> ListLevel {
    ListLevel {
        format: format.into(),
        text: text.into(),
        align,
        suffix,
        indent_left: Some(left),
        indent_first: Some(first),
        ..ListLevel::default()
    }
}

#[test]
fn word_list_markers_and_text_go_where_word_puts_them() {
    use MarkerAlign::{Center, Left, Right};
    use MarkerSuffix::{Nothing, Space, Tab};
    let mut stopped = level("decimal", "%3.", (1440, -360), Left, Tab);
    stopped.tabs = vec![
        stop(1800, TabAlign::Left, TabLeader::None),
        stop(2700, TabAlign::Left, TabLeader::None),
    ];
    let format = numbered(vec![
        // Google Docs' bullet: 360 in, hanging 360.
        level("bullet", "●", (360, -360), Left, Tab),
        level("bullet", "●", (720, -360), Left, Tab),
        level("decimal", "%2.", (1440, -360), Left, Tab),
        stopped,
        level("decimal", "%4)", (1440, -720), Left, Space),
        level("decimal", "%5)", (1440, -720), Left, Nothing),
        level("upperRoman", "%6.", (1080, -360), Right, Tab),
        level("lowerRoman", "%7", (1080, -360), Center, Tab),
        level("bullet", "", (720, -360), Left, Tab),
    ]);
    let blocks = vec![
        word_item(0, None, "google"),
        // A 10 px bullet at 24 px: its tab goes to the hanging indent's stop, 48 px.
        word_item(1, None, "fits"),
        // An 80 px label from 72 px ends past the hanging indent (96 px): its tab goes on
        // to the next default stop (48 px apart), 192 px; the text's other lines at 96.
        word_item(2, Some("Step 10:"), "wide"),
        // ... or to its level's next stop past it: 2700 twips, not 1800 (under it).
        word_item(3, Some("Step 10:"), "stops"),
        // A space after a 20 px label from 48 px: the text at 78, left of the left
        // indent (96), the other lines there.
        word_item(4, Some("1)"), "space"),
        // Nothing after it: the text at 68.
        word_item(5, Some("1)"), "nothing"),
        // Right-justified: the 40 px label ends at the first line's start, 48 px.
        word_item(6, Some("III."), "right"),
        // Centred: the 20 px label centres on 48 px.
        word_item(7, Some("ii"), "centre"),
        // No text: no marker drawn, but its tab still goes to the hanging indent.
        word_item(8, None, "empty"),
    ];
    let mut fake = Fake::default();
    let flow = flow_with(format, StyleSheet::builtin(), blocks, 400, &mut fake);
    let lefts: Vec<i32> = boxes(&flow).iter().map(|b| b.0).collect();
    assert_eq!(lefts, vec![24, 48, 96, 96, 78, 68, 72, 72, 48]);
    let widths: Vec<i32> = boxes(&flow).iter().map(|b| b.2).collect();
    assert_eq!(widths, lefts.iter().map(|l| 400 - l).collect::<Vec<_>>());
    // The first line's start from the box (Pango's indent), so the text starts at 24,
    // 48, 192, 180, 78, 68, 72, 72, and 48 px.
    let firsts = each_text(&flow, |c| place_of(c).first);
    assert_eq!(firsts, vec![0, 0, 96, 84, -18, -28, 0, 0, 0]);
    assert_eq!(
        markers(&flow),
        vec![0.0, 24.0, 72.0, 72.0, 48.0, 48.0, 8.0, 38.0, 24.0]
    );
    assert_eq!(
        fake.markers,
        ["●", "●", "Step 10:", "Step 10:", "1)", "1)", "III.", "ii", ""]
    );
    // Hit-testing ignores the marker: a click on a hanging item's marker is at its
    // text's start.
    let place = flow.hit(&mut fake, 5.0, f64::from(TOP_MARGIN) + 5.0);
    assert_eq!(place.map(|l| l.position.offset), Some(0));
}

fn own(style: &str, text: &str, own: ParaOverrides) -> Block {
    let mut p = styled(style, text);
    p.own = own;
    Block::Paragraph(p)
}

#[test]
fn a_paragraphs_own_space_indents_and_line_spacing_go_over_its_styles() {
    // `S`: 20 px before, 10 after (300 and 150 twips), 40 px in (600).
    let styles = sheet(spaced(ParaSpacing {
        space_before: 300,
        space_after: 150,
        indent_left: 600,
        ..ParaSpacing::default()
    }));
    let blocks = vec![
        Block::Paragraph(styled("S", "x")),
        // No space before, 30 px after, 60 px in with a 10 px hanging first line, and 20
        // px in from the right.
        own(
            "S",
            &"w".repeat(30),
            ParaOverrides {
                space_before: Some(0),
                space_after: Some(450),
                indent_left: Some(900),
                indent_right: Some(300),
                indent_first: Some(-150),
                ..ParaOverrides::default()
            },
        ),
        // Normal (8 px after) with exact 40 px lines (600 twips).
        own(
            "Normal",
            &"w".repeat(25),
            ParaOverrides {
                line: Some(LineSpacing::Exact(600)),
                ..ParaOverrides::default()
            },
        ),
    ];
    let mut fake = Fake::default();
    let mut flow = Flow::new(Document {
        blocks,
        styles,
        format: Format::Docx(Default::default()),
    });
    flow.set_width(200, &mut fake);
    let estimated = flow.content_height();
    flow.layout_slice(&mut fake, &mut || true);
    let top = f64::from(TOP_MARGIN);
    // The first: 20 above, one 20 px line, 10 below. The second starts right under it,
    // at 60 - 10 px, 200 - 50 - 20 px wide: 13 characters a line, three lines.
    // The third: 30 px under that, two 40 px lines of 20 characters.
    assert_eq!(
        boxes(&flow),
        vec![
            (40, top + 20.0, 160),
            (50, top + 50.0, 130),
            (0, top + 140.0, 200)
        ]
    );
    assert_eq!(flow.wrap_width(LeafId::from_index(1)), 130);
    assert_eq!(
        flow.content_height(),
        top + 140.0 + 80.0 + 8.0 + f64::from(BOTTOM_MARGIN)
    );
    assert_eq!(estimated, flow.content_height(), "estimates agree");
    let mut first = Vec::new();
    flow.paint(0.0, 1e9, &mut |p| {
        if let Paint::Text { content, .. } = p {
            first.push((place_of(&content).first, content.line_spacing()));
        }
    });
    assert_eq!(
        first,
        vec![
            (0, LineSpacing::Auto(1.0)),
            (-10, LineSpacing::Auto(1.0)),
            (0, LineSpacing::Exact(600))
        ]
    );
}

#[test]
fn a_markdown_list_items_own_space_and_left_indent_go_over_the_lists() {
    let item = |text: &str, own: ParaOverrides| {
        let mut paragraph = styled("Normal", text);
        paragraph.own = own;
        Block::ListItem(document_model::ListItem {
            kind: document_model::ListKind::Bullet,
            level: 0,
            paragraph,
            numbering: None,
        })
    };
    let blocks = vec![
        item("a", ParaOverrides::default()),
        // 10 px before, 20 after, 72 px in (1080 twips).
        item(
            "b",
            ParaOverrides {
                space_before: Some(150),
                space_after: Some(300),
                indent_left: Some(1080),
                indent_right: Some(150),
                ..ParaOverrides::default()
            },
        ),
        // In at the edge: its marker keeps its room.
        item(
            "c",
            ParaOverrides {
                indent_left: Some(0),
                ..ParaOverrides::default()
            },
        ),
    ];
    let markdown = document_model::read_markdown(b"x\n").format;
    let mut fake = Fake::default();
    let flow = flow_with(markdown, StyleSheet::builtin(), blocks, 200, &mut fake);
    let top = f64::from(TOP_MARGIN);
    let item_space = 3.0;
    let l = LEVEL_INDENT;
    assert_eq!(
        boxes(&flow),
        vec![
            (l, top, 200 - l),
            (72, top + 20.0 + item_space + 10.0, 200 - 72 - 10),
            (l, top + 20.0 + item_space + 10.0 + 20.0 + 20.0, 200 - l),
        ]
    );
    // The viewer's markers hang a level's indent left of the text, unmeasured.
    assert_eq!(markers(&flow), vec![0.0, 72.0 - 28.0, 0.0]);
    assert!(fake.markers.is_empty());
}

#[test]
fn a_word_list_items_own_space_and_indents_go_over_its_levels() {
    let item = |text: &str, own: ParaOverrides| {
        let mut paragraph = styled("Normal", text);
        paragraph.own = own;
        Block::ListItem(document_model::ListItem {
            kind: document_model::ListKind::Bullet,
            level: 0,
            paragraph,
            numbering: None,
        })
    };
    let blocks = vec![
        item("a", ParaOverrides::default()),
        // 10 px before, 20 after, 72 px in (1080 twips), the level's hanging 24 px.
        item(
            "b",
            ParaOverrides {
                space_before: Some(150),
                space_after: Some(300),
                indent_left: Some(1080),
                indent_right: Some(150),
                ..ParaOverrides::default()
            },
        ),
        // In at the edge: the first line starts there too, its marker at 0 and its
        // text past it at the first default stop (48 px).
        item(
            "c",
            ParaOverrides {
                indent_left: Some(0),
                ..ParaOverrides::default()
            },
        ),
        // Its own first line: 30 px in from 72 px, its marker there and its text at the
        // next default stop.
        item(
            "d",
            ParaOverrides {
                indent_left: Some(1080),
                indent_first: Some(450),
                ..ParaOverrides::default()
            },
        ),
    ];
    let mut fake = Fake::default();
    let flow = flow_of(StyleSheet::builtin(), blocks, 200, &mut fake);
    let top = f64::from(TOP_MARGIN);
    // Items space as their style does (Normal: 8 px after), not as a viewer list.
    let after = 8.0;
    assert_eq!(
        boxes(&flow),
        vec![
            (48, top, 200 - 48),
            (72, top + 20.0 + after + 10.0, 200 - 72 - 10),
            (0, top + 20.0 + after + 10.0 + 20.0 + 20.0, 200),
            (
                72,
                top + 20.0 + after + 10.0 + 20.0 + 20.0 + 20.0 + after,
                200 - 72
            ),
        ]
    );
    assert_eq!(markers(&flow), vec![24.0, 48.0, 0.0, 102.0]);
    let firsts = each_text(&flow, |c| place_of(c).first);
    assert_eq!(firsts, vec![0, 0, 48, 144 - 72]);
    assert_eq!(fake.markers, vec!["•"; 4]);
}

#[test]
fn a_cell_paragraphs_own_indents_narrow_it_in_its_cell() {
    let blocks = vec![table(vec![vec![cell(vec![
        Block::Paragraph(styled("Normal", "plain")),
        own(
            "Normal",
            "indented",
            ParaOverrides {
                space_before: Some(150),
                indent_left: Some(300),
                indent_right: Some(150),
                ..ParaOverrides::default()
            },
        ),
    ])]])];
    let flow = flow_of(StyleSheet::builtin(), blocks, 300, &mut Fake::default());
    let b = boxes(&flow);
    assert_eq!(b.len(), 2);
    let ((x0, y0, w0), (x1, y1, w1)) = (b[0], b[1]);
    assert_eq!((x1 - x0, w0 - w1), (20, 30));
    // Normal's 8 px after the first, then the second's own 10 px before.
    assert_eq!(y1 - y0, f64::from(LINE) + 8.0 + 10.0);
}

#[test]
fn contextual_spacing_drops_the_space_between_paragraphs_of_one_style() {
    // `T`: 20 px before, 10 after, contextual.
    let styles = sheet(spaced(ParaSpacing {
        space_before: 300,
        space_after: 150,
        contextual: true,
        ..ParaSpacing::default()
    }));
    let off = ParaOverrides {
        contextual: Some(false),
        ..ParaOverrides::default()
    };
    let blocks = vec![
        Block::Paragraph(styled("S", "a")),
        Block::Paragraph(styled("S", "b")),
        Block::Paragraph(styled("Normal", "c")),
        own("S", "d", off),
        Block::Paragraph(styled("S", "e")),
    ];
    let mut fake = Fake::default();
    let mut flow = flow_of(styles.clone(), blocks, 200, &mut fake);
    let top = f64::from(TOP_MARGIN);
    let ys =
        |flow: &Flow| -> Vec<f64> { boxes(flow).into_iter().map(|(_, y, _)| y - top).collect() };
    // a and b touch; c follows b's 10 px; d (contextual off) keeps its 20 px before and
    // its 10 after; e drops its own before against d, as it's in the same style.
    assert_eq!(ys(&flow), vec![20.0, 40.0, 70.0, 118.0, 148.0]);

    // A Normal paragraph between a and b gives both their space back; removing it takes
    // it away again. Each matches a flow made from scratch.
    flow.edit(&mut fake, |doc| {
        doc.blocks
            .insert(1, Block::Paragraph(styled("Normal", "n")));
        (
            (),
            Splice {
                start: 1,
                removed: 0,
                inserted: 1,
            },
        )
    });
    flow.layout_slice(&mut fake, &mut || true);
    assert_eq!(ys(&flow), vec![20.0, 50.0, 98.0, 128.0, 176.0, 206.0]);
    let fresh = flow_of(
        styles.clone(),
        flow.document().blocks.clone(),
        200,
        &mut Fake::default(),
    );
    assert_eq!(flow.content_height(), fresh.content_height());
    flow.edit(&mut fake, |doc| {
        doc.blocks.remove(1);
        (
            (),
            Splice {
                start: 1,
                removed: 1,
                inserted: 0,
            },
        )
    });
    flow.layout_slice(&mut fake, &mut || true);
    assert_eq!(ys(&flow), vec![20.0, 40.0, 70.0, 118.0, 148.0]);
    let fresh = flow_of(
        styles,
        flow.document().blocks.clone(),
        200,
        &mut Fake::default(),
    );
    assert_eq!(flow.content_height(), fresh.content_height());
}

/// A paragraph's shading or border stroke as painted: (x, y, width, height), and its dash
/// and colour (a fill's colour, solid).
type Drawn = (i32, f64, i32, i32, Dash, Option<Rgb>);

/// Every fill and stroke painted, in order.
fn frames(flow: &Flow) -> Vec<Drawn> {
    let mut out = Vec::new();
    flow.paint(0.0, 1e9, &mut |p| match p {
        Paint::Fill {
            x,
            y,
            width,
            height,
            color,
        } => out.push((x, y, width, height, Dash::Solid, Some(color))),
        Paint::Stroke {
            x,
            y,
            width,
            height,
            dash,
            color,
        } => out.push((x, y, width, height, dash, color)),
        _ => {}
    });
    out
}

fn line(style: BorderStyle, width: u16, space: u16, color: Option<Rgb>) -> Border {
    Border {
        style,
        width,
        space,
        color,
    }
}

fn bordered(text: &str, borders: BorderOverrides) -> Block {
    own(
        "Normal",
        text,
        ParaOverrides {
            borders,
            ..ParaOverrides::default()
        },
    )
}

#[test]
fn a_paragraphs_border_lines_and_spaces_add_to_its_height() {
    let red = Some([0xC0, 0, 0]);
    let single = |space| Some(line(BorderStyle::Single, 8, space, None));
    let blocks = vec![
        // The résumé's title rule: half a point, 1 pt below (10 + 20 twips: 2 px).
        bordered(
            "x",
            BorderOverrides {
                bottom: Some(line(BorderStyle::Single, 4, 1, Some([0; 3]))),
                ..BorderOverrides::default()
            },
        ),
        // A box of 1 pt lines 4 pt out (20 + 80 twips: 7 px), 1 px strokes.
        bordered(
            "y",
            BorderOverrides {
                top: single(4),
                bottom: single(4),
                left: single(4),
                right: single(4),
                ..BorderOverrides::default()
            },
        ),
        // A red double rule of 0.75 pt lines 2 pt below (45 + 40 twips: 6 px): two 1 px
        // strokes with a 1 px gap, at the zone's bottom.
        bordered(
            "z",
            BorderOverrides {
                bottom: Some(line(BorderStyle::Double, 6, 2, red)),
                ..BorderOverrides::default()
            },
        ),
        // A dotted 1.5 pt rule 1 pt above (30 + 20 twips: 3 px), 2 px thick.
        bordered(
            "w",
            BorderOverrides {
                top: Some(line(BorderStyle::Dotted, 12, 1, None)),
                ..BorderOverrides::default()
            },
        ),
        para("v"),
    ];
    let mut fake = Fake::default();
    let mut flow = Flow::new(doc(blocks));
    flow.set_width(200, &mut fake);
    let estimated = flow.content_height();
    flow.layout_slice(&mut fake, &mut || true);
    let top = f64::from(TOP_MARGIN);
    // Each paragraph: its space before (0), its top zone, a 20 px line, its bottom zone,
    // its 8 px after.
    let (a, b, c, d) = (top, top + 30.0, top + 72.0, top + 106.0);
    let ys: Vec<f64> = boxes(&flow).into_iter().map(|(_, y, _)| y).collect();
    assert_eq!(ys, vec![a, b + 7.0, c, d + 3.0, d + 31.0]);
    assert_eq!(
        flow.content_height(),
        d + 31.0 + 20.0 + 8.0 + f64::from(BOTTOM_MARGIN)
    );
    assert_eq!(estimated, flow.content_height(), "estimates agree");
    let black = Some([0; 3]);
    assert_eq!(
        frames(&flow),
        vec![
            // The rule: 1 px under the text's 20 px line, across the column.
            (0, a + 21.0, 200, 1, Dash::Solid, black),
            // The box: its lines 7 px outside the text, the sides from top to bottom.
            (-7, b, 214, 1, Dash::Solid, None),
            (-7, b + 33.0, 214, 1, Dash::Solid, None),
            (-7, b, 1, 34, Dash::Solid, None),
            (206, b, 1, 34, Dash::Solid, None),
            // The double rule's outer stroke at the zone's bottom, its inner one 2 px up.
            (0, c + 25.0, 200, 1, Dash::Solid, red),
            (0, c + 23.0, 200, 1, Dash::Solid, red),
            (0, d, 200, 2, Dash::Dotted, None),
        ]
    );
}

#[test]
fn paragraphs_bordered_together_share_one_box_with_lines_between() {
    let blue = Some([0x2F, 0x54, 0x96]);
    let fill = Some([0xDE, 0xEA, 0xF6]);
    // `S`: 10 px after (150 twips), a box of 1 pt lines 1 pt out (3 px), a red half-point
    // line between, 1 pt out (2 px), and a fill.
    let side = line(BorderStyle::Single, 8, 1, blue);
    let red = Some([0xC0, 0, 0]);
    let styles = sheet(ParaLook {
        spacing: ParaSpacing {
            space_after: 150,
            ..ParaSpacing::default()
        },
        borders: Borders {
            top: side,
            bottom: side,
            left: side,
            right: side,
            between: line(BorderStyle::Single, 4, 1, red),
        },
        shading: Shading { fill },
        ..ParaLook::default()
    });
    let blocks: Vec<Block> = ["a", "b", "c"]
        .iter()
        .map(|t| Block::Paragraph(styled("S", t)))
        .chain([para("d")])
        .collect();
    let mut fake = Fake::default();
    let mut flow = flow_of(styles.clone(), blocks, 200, &mut fake);
    let top = f64::from(TOP_MARGIN);
    // a: the top zone (3), its line, 10 after. b: the between zone (2), its line, 10
    // after. c: the between zone, its line, the bottom zone (3), 10 after.
    let (a, b, c, d) = (top, top + 33.0, top + 65.0, top + 100.0);
    let ys: Vec<f64> = boxes(&flow).into_iter().map(|(_, y, _)| y).collect();
    assert_eq!(ys, vec![a + 3.0, b + 2.0, c + 2.0, d]);
    let (x, w) = (-3, 206);
    let together = vec![
        // a: the fill and sides run on through its space after; the top line only.
        (x, a, w, 33, Dash::Solid, fill),
        (x, a, w, 1, Dash::Solid, blue),
        (x, a, 1, 33, Dash::Solid, blue),
        (202, a, 1, 33, Dash::Solid, blue),
        // b: open above and below; the line between at its top.
        (x, b, w, 32, Dash::Solid, fill),
        (x, b, w, 1, Dash::Solid, red),
        (x, b, 1, 32, Dash::Solid, blue),
        (202, b, 1, 32, Dash::Solid, blue),
        // c: the line between, and the bottom line above its space after.
        (x, c, w, 25, Dash::Solid, fill),
        (x, c, w, 1, Dash::Solid, red),
        (x, c + 24.0, w, 1, Dash::Solid, blue),
        (x, c, 1, 25, Dash::Solid, blue),
        (202, c, 1, 25, Dash::Solid, blue),
    ];
    assert_eq!(frames(&flow), together);

    // A Normal paragraph after a ends the group there: a gets its bottom line, b its top
    // line. Each matches a flow made from scratch, and removing it joins them again.
    let insert = |flow: &mut Flow, fake: &mut Fake| {
        flow.edit(fake, |doc| {
            doc.blocks.insert(1, para("n"));
            (
                (),
                Splice {
                    start: 1,
                    removed: 0,
                    inserted: 1,
                },
            )
        });
        flow.layout_slice(fake, &mut || true);
    };
    insert(&mut flow, &mut fake);
    let fresh = flow_of(
        styles.clone(),
        flow.document().blocks.clone(),
        200,
        &mut Fake::default(),
    );
    assert_eq!(frames(&flow), frames(&fresh));
    assert_eq!(flow.content_height(), fresh.content_height());
    let first: Vec<Drawn> = frames(&flow).into_iter().take(5).collect();
    assert_eq!(
        first,
        vec![
            (x, a, w, 26, Dash::Solid, fill),
            (x, a, w, 1, Dash::Solid, blue),
            (x, a + 25.0, w, 1, Dash::Solid, blue),
            (x, a, 1, 26, Dash::Solid, blue),
            (202, a, 1, 26, Dash::Solid, blue),
        ]
    );
    flow.edit(&mut fake, |doc| {
        doc.blocks.remove(1);
        (
            (),
            Splice {
                start: 1,
                removed: 1,
                inserted: 0,
            },
        )
    });
    flow.layout_slice(&mut fake, &mut || true);
    assert_eq!(frames(&flow), together);
}

#[test]
fn shading_fills_the_texts_indented_box_and_nil_borders_draw_nothing() {
    let yellow = Some([0xFF, 0xE5, 0x99]);
    let blocks = vec![
        // 40 px in, 20 px from the right, two exact 40 px lines.
        own(
            "Normal",
            &"w".repeat(20),
            ParaOverrides {
                line: Some(LineSpacing::Exact(600)),
                indent_left: Some(600),
                indent_right: Some(300),
                shading: Some(Shading { fill: yellow }),
                ..ParaOverrides::default()
            },
        ),
        // Google Docs' paragraphs: nil sides and an automatic fill.
        own(
            "Normal",
            "plain",
            ParaOverrides {
                borders: BorderOverrides {
                    top: Some(Border::default()),
                    bottom: Some(Border::default()),
                    left: Some(Border::default()),
                    right: Some(Border::default()),
                    between: Some(Border::default()),
                },
                shading: Some(Shading { fill: None }),
                ..ParaOverrides::default()
            },
        ),
        para("end"),
    ];
    let flow = flow_of(StyleSheet::builtin(), blocks, 200, &mut Fake::default());
    let top = f64::from(TOP_MARGIN);
    assert_eq!(frames(&flow), vec![(40, top, 140, 80, Dash::Solid, yellow)]);
    let ys: Vec<f64> = boxes(&flow).into_iter().map(|(_, y, _)| y).collect();
    assert_eq!(ys, vec![top, top + 88.0, top + 116.0]);
}

#[test]
fn dots_and_dashes_split_a_stroke_along_its_length() {
    let pieces = |dash: Dash, length, thickness| -> Vec<(i32, i32)> {
        dash.pieces(length, thickness).collect()
    };
    assert_eq!(pieces(Dash::Solid, 50, 1), vec![(0, 50)]);
    assert_eq!(pieces(Dash::Dotted, 10, 2), vec![(0, 2), (4, 2), (8, 2)]);
    assert_eq!(pieces(Dash::Dashed, 14, 1), vec![(0, 3), (5, 3), (10, 3)]);
    // The last piece stops at the stroke's end.
    assert_eq!(pieces(Dash::Dashed, 12, 1), vec![(0, 3), (5, 3), (10, 2)]);
}
