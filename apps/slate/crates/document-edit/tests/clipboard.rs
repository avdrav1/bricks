//! Selection and clipboard: copying, deleting, and pasting across paragraphs, list items,
//! and tables, each one undo step giving back the document exactly.

use document_edit::{
    copy, delete, normalize, paste, text, Change, Container, Edit, Fragment, History, Pos,
};
use document_model::{
    read_markdown, Block, Document, Image, ImageFormat, ImageSource, ListKind, Paragraph,
    ParagraphStyle, Raw, RawKind, Run, RunStyle,
};
use std::sync::Arc;

/// 0 paragraph, 1-2 list items, 3 a table (A B / c d), 4-5 paragraphs.
const DOC: &str = "Intro **bold** text\n\n- one\n- two\n\n| A | B |\n|---|---|\n| c | d |\n\nAfter *it* here\n\nLast\n";

fn md(source: &str) -> Document {
    let mut doc = read_markdown(source.as_bytes());
    normalize(&mut doc);
    doc
}

fn doc() -> Document {
    let doc = md(DOC);
    assert!(matches!(doc.blocks[3], Block::Table(_)), "{:?}", doc.blocks);
    assert_eq!(doc.blocks.len(), 6);
    doc
}

fn pos(block: usize, offset: usize) -> Pos {
    Pos::new(block, offset)
}

fn cell(row: usize, cell: usize, offset: usize) -> Pos {
    Pos::in_cell(3, row, cell, 0, offset)
}

fn runs(block: &Block) -> Vec<(String, bool, bool)> {
    let p = match block {
        Block::Paragraph(p) => p,
        Block::ListItem(item) => &item.paragraph,
        other => panic!("no runs: {other:?}"),
    };
    p.runs
        .iter()
        .map(|r| (r.text.clone(), r.style.bold, r.style.italic))
        .collect()
}

fn run(text: &str, bold: bool, italic: bool) -> (String, bool, bool) {
    (text.to_owned(), bold, italic)
}

fn plain(text: &str) -> (String, bool, bool) {
    run(text, false, false)
}

fn txt(doc: &Document, at: Container, block: usize) -> String {
    text(doc, at, block)
        .expect("an editable block")
        .into_owned()
}

fn is_bullet(block: &Block) -> bool {
    matches!(block, Block::ListItem(item) if item.kind == ListKind::Bullet)
}

fn blank() -> Block {
    Block::Paragraph(Paragraph::default())
}

/// Apply the edit `make` makes as one step, check undo gives back the document before
/// (leaving nothing more to undo) and redo the one after; returns the applied change.
fn checked(doc: &mut Document, make: impl FnOnce(&Document) -> Option<Edit>) -> Change {
    let before = doc.clone();
    let mut history = History::new();
    let change = history.apply(doc, make(doc).expect("an edit"));
    let after = doc.clone();
    history.undo(doc).expect("a step to undo");
    assert_eq!(*doc, before);
    assert!(!history.can_undo(), "one step");
    history.redo(doc).expect("a step to redo");
    assert_eq!(*doc, after);
    change
}

fn round_trip(fragment: &Fragment) {
    let json = fragment.to_json();
    assert_eq!(Fragment::from_json(&json).as_ref(), Some(fragment));
}

#[test]
fn copy_within_a_paragraph_keeps_runs_and_looks() {
    let mut doc = md("# Big **bold** end");
    let fragment = copy(&doc, pos(0, 9), pos(0, 2)).expect("a fragment");
    let [Block::Paragraph(p)] = &fragment.blocks[..] else {
        panic!("one paragraph: {fragment:?}");
    };
    assert_eq!(p.style, ParagraphStyle::Heading(1));
    assert_eq!(
        runs(&fragment.blocks[0]),
        [plain("g "), run("bold", true, false), plain(" ")]
    );
    assert_eq!(fragment.plain_text(), "g bold ");
    round_trip(&fragment);
    assert_eq!(copy(&doc, pos(0, 3), pos(0, 3)), None);

    let change = checked(&mut doc, |d| delete(d, pos(0, 9), pos(0, 2)));
    assert_eq!(txt(&doc, Container::Top, 0), "Biend");
    assert_eq!(change.caret, pos(0, 2));
}

#[test]
fn copy_across_paragraphs_and_list_items() {
    let mut doc = doc();
    let fragment = copy(&doc, pos(0, 6), pos(2, 2)).expect("a fragment");
    assert_eq!(fragment.blocks.len(), 3);
    assert!(matches!(fragment.blocks[0], Block::Paragraph(_)));
    assert_eq!(
        runs(&fragment.blocks[0]),
        [run("bold", true, false), plain(" text")]
    );
    assert_eq!(fragment.blocks[1], doc.blocks[1]);
    assert!(is_bullet(&fragment.blocks[2]));
    assert_eq!(runs(&fragment.blocks[2]), [plain("tw")]);
    assert_eq!(fragment.plain_text(), "bold text\none\ntw");
    round_trip(&fragment);

    let change = checked(&mut doc, |d| delete(d, pos(0, 6), pos(2, 2)));
    assert_eq!(doc.blocks.len(), 4);
    assert_eq!(runs(&doc.blocks[0]), [plain("Intro o")]);
    assert!(matches!(doc.blocks[1], Block::Table(_)));
    assert_eq!((change.start, change.removed, change.inserted), (0, 3, 1));
    assert_eq!(change.caret, pos(0, 6));
}

#[test]
fn copy_through_a_table_takes_it_whole() {
    let mut doc = doc();
    let fragment = copy(&doc, pos(2, 1), pos(4, 5)).expect("a fragment");
    assert_eq!(fragment.blocks.len(), 3);
    assert_eq!(runs(&fragment.blocks[0]), [plain("wo")]);
    assert_eq!(fragment.blocks[1], doc.blocks[3]);
    assert_eq!(runs(&fragment.blocks[2]), [plain("After")]);
    assert_eq!(fragment.plain_text(), "wo\nA\nB\nc\nd\nAfter");
    round_trip(&fragment);

    let change = checked(&mut doc, |d| delete(d, pos(4, 5), pos(2, 1)));
    assert_eq!(doc.blocks.len(), 4);
    assert!(is_bullet(&doc.blocks[2]));
    assert_eq!(
        runs(&doc.blocks[2]),
        [plain("t "), run("it", false, true), plain(" here")]
    );
    assert_eq!((change.start, change.removed, change.inserted), (2, 3, 1));
    assert_eq!(change.caret, pos(2, 1));
}

#[test]
fn copy_from_a_cell_out_of_the_table_takes_the_table_whole() {
    let mut doc = doc();
    let fragment = copy(&doc, pos(4, 6), cell(1, 0, 0)).expect("a fragment");
    assert_eq!(fragment.blocks.len(), 3);
    assert_eq!(fragment.blocks[0], blank());
    assert_eq!(fragment.blocks[1], doc.blocks[3]);
    assert_eq!(runs(&fragment.blocks[2]), [plain("After ")]);
    assert_eq!(fragment.plain_text(), "A\nB\nc\nd\nAfter ");
    round_trip(&fragment);

    // Delete keeps the text after the selection, without the table.
    let change = checked(&mut doc, |d| delete(d, cell(1, 0, 0), pos(4, 6)));
    assert_eq!(doc.blocks.len(), 5);
    assert_eq!(
        runs(&doc.blocks[3]),
        [run("it", false, true), plain(" here")]
    );
    assert_eq!(change.at, Container::Top);
    assert_eq!((change.start, change.removed, change.inserted), (3, 2, 1));
    assert_eq!(change.caret, pos(3, 0));
    assert_eq!(change.anchor, None);
}

#[test]
fn copy_into_a_cell_from_before_the_table() {
    let mut doc = doc();
    let fragment = copy(&doc, pos(2, 1), cell(0, 0, 1)).expect("a fragment");
    assert_eq!(fragment.blocks.len(), 3);
    assert_eq!(runs(&fragment.blocks[0]), [plain("wo")]);
    assert_eq!(fragment.blocks[1], doc.blocks[3]);
    assert_eq!(fragment.blocks[2], blank());
    assert_eq!(fragment.plain_text(), "wo\nA\nB\nc\nd");
    round_trip(&fragment);

    // The start block keeps its head and doesn't join what follows the table.
    let change = checked(&mut doc, |d| delete(d, pos(2, 1), cell(0, 0, 1)));
    assert_eq!(doc.blocks.len(), 5);
    assert!(is_bullet(&doc.blocks[2]));
    assert_eq!(runs(&doc.blocks[2]), [plain("t")]);
    assert_eq!(txt(&doc, Container::Top, 3), "After it here");
    assert_eq!((change.start, change.removed, change.inserted), (2, 2, 1));
    assert_eq!(change.caret, pos(2, 1));
}

#[test]
fn copy_between_two_cells_takes_the_table() {
    let mut doc = doc();
    let fragment = copy(&doc, cell(0, 1, 1), cell(1, 0, 1)).expect("a fragment");
    assert_eq!(fragment.blocks, [blank(), doc.blocks[3].clone(), blank()]);
    assert_eq!(fragment.plain_text(), "A\nB\nc\nd");
    round_trip(&fragment);

    // An empty paragraph takes the table's place, for the caret.
    let change = checked(&mut doc, |d| delete(d, cell(1, 0, 1), cell(0, 1, 1)));
    assert_eq!(doc.blocks.len(), 6);
    assert_eq!(doc.blocks[3], blank());
    assert_eq!(change.caret, pos(3, 0));
}

#[test]
fn copy_within_a_cell_stays_in_it() {
    let mut doc = doc();
    let at = Container::Cell {
        table: 3,
        row: 1,
        cell: 1,
    };
    let fragment = copy(&doc, cell(1, 1, 0), cell(1, 1, 1)).expect("a fragment");
    assert_eq!(fragment.plain_text(), "d");
    checked(&mut doc, |d| delete(d, cell(1, 1, 0), cell(1, 1, 1)));
    assert_eq!(txt(&doc, at, 0), "");
}

#[test]
fn json_keeps_pictures_and_raw_source() {
    let fragment = Fragment {
        blocks: vec![
            blank(),
            Block::Image(Image {
                alt: "a cat".to_owned(),
                source: ImageSource::Embedded {
                    format: ImageFormat::Png,
                    bytes: Arc::from(&b"\x89PNG\r\n\x1a\n"[..]),
                },
                size: Some((12.5, 3.0)),
            }),
            Block::Rule,
            Block::PageBreak,
            Block::Raw(Raw {
                kind: RawKind::Html,
                source: "<b>hi</b>\n".to_owned(),
            }),
            blank(),
        ],
    };
    round_trip(&fragment);
    assert_eq!(fragment.plain_text(), "\na cat\n<b>hi</b>\n");
    assert_eq!(Fragment::from_json("not json"), None);
    assert_eq!(Fragment::from_json(r#"{"blocks":[]}"#), None);
    assert_eq!(Fragment::MIME, "application/x-slate-fragment");
}

#[test]
fn paste_into_a_paragraphs_middle_keeps_its_looks_and_joins_the_tail() {
    let mut doc = doc();
    let fragment = copy(&doc, pos(0, 6), pos(2, 2)).expect("a fragment");
    let change = checked(&mut doc, |d| paste(d, pos(5, 2), pos(5, 2), &fragment));
    assert_eq!(doc.blocks.len(), 8);
    assert!(matches!(&doc.blocks[5], Block::Paragraph(p) if p.style == ParagraphStyle::Normal));
    assert_eq!(
        runs(&doc.blocks[5]),
        [plain("La"), run("bold", true, false), plain(" text")]
    );
    assert_eq!(doc.blocks[6], doc.blocks[1]);
    assert!(is_bullet(&doc.blocks[7]));
    assert_eq!(runs(&doc.blocks[7]), [plain("twst")]);
    assert_eq!(change.caret, pos(7, 2));
    assert_eq!(change.anchor, None);

    // One piece goes in inline, in its own looks.
    let bold = copy(&doc, pos(0, 6), pos(0, 10)).expect("a fragment");
    let change = checked(&mut doc, |d| paste(d, pos(4, 0), pos(4, 0), &bold));
    assert_eq!(
        runs(&doc.blocks[4]),
        [
            run("bold", true, false),
            plain("After "),
            run("it", false, true),
            plain(" here")
        ]
    );
    assert_eq!(change.caret, pos(4, 4));
}

#[test]
fn paste_at_top_puts_a_table_in() {
    let mut doc = doc();
    let fragment = copy(&doc, pos(2, 1), pos(4, 5)).expect("a fragment");
    let table = doc.blocks[3].clone();
    let change = checked(&mut doc, |d| paste(d, pos(5, 2), pos(5, 2), &fragment));
    assert_eq!(doc.blocks.len(), 8);
    assert_eq!(runs(&doc.blocks[5]), [plain("Lawo")]);
    assert_eq!(doc.blocks[6], table);
    assert_eq!(runs(&doc.blocks[7]), [plain("Afterst")]);
    assert_eq!(change.caret, pos(7, 5));

    // A fragment starting with a table: its empty paragraph joins the one pasted into.
    let fragment = copy(&doc, cell(0, 0, 0), pos(4, 3)).expect("a fragment");
    let change = checked(&mut doc, |d| paste(d, pos(0, 5), pos(0, 5), &fragment));
    assert_eq!(runs(&doc.blocks[0]), [plain("Intro")]);
    assert_eq!(doc.blocks[1], table);
    assert_eq!(txt(&doc, Container::Top, 2), "Aft bold text");
    assert_eq!(change.caret, pos(2, 3));
}

#[test]
fn paste_into_a_cell_flattens_a_table() {
    let mut doc = doc();
    let fragment = copy(&doc, pos(2, 1), pos(4, 5)).expect("a fragment");
    let at = Container::Cell {
        table: 3,
        row: 1,
        cell: 1,
    };
    let change = checked(&mut doc, |d| {
        paste(d, cell(1, 1, 1), cell(1, 1, 1), &fragment)
    });
    let texts: Vec<String> = (0..6).map(|i| txt(&doc, at, i)).collect();
    assert_eq!(texts, ["dwo", "A", "B", "c", "d", "After"]);
    assert_eq!(
        document_edit::blocks(&doc, at).expect("the cell").len(),
        6,
        "no table in the cell"
    );
    assert_eq!(change.at, at);
    assert_eq!(change.caret, Pos::in_cell(3, 1, 1, 5, 5));
}

#[test]
fn paste_replaces_a_selection() {
    let mut doc = doc();
    let lines = Fragment::from_plain("X\nY", &RunStyle::default());
    let change = checked(&mut doc, |d| paste(d, pos(4, 6), pos(0, 6), &lines));
    assert_eq!(doc.blocks.len(), 3);
    assert_eq!(runs(&doc.blocks[0]), [plain("Intro X")]);
    assert_eq!(
        runs(&doc.blocks[1]),
        [plain("Y"), run("it", false, true), plain(" here")]
    );
    assert_eq!(change.caret, pos(1, 1));

    // A selection out of a cell: the table goes, the paste joins what's left.
    let mut doc = self::doc();
    let z = Fragment::from_plain("Z", &RunStyle::default());
    let change = checked(&mut doc, |d| paste(d, cell(0, 0, 0), pos(4, 6), &z));
    assert_eq!(doc.blocks.len(), 5);
    assert_eq!(
        runs(&doc.blocks[3]),
        [plain("Z"), run("it", false, true), plain(" here")]
    );
    assert_eq!(change.caret, pos(3, 1));
}

#[test]
fn plain_text_pastes_a_paragraph_per_line() {
    let bold = RunStyle {
        bold: true,
        ..RunStyle::default()
    };
    let fragment = Fragment::from_plain("a\r\nb\nc", &bold);
    assert_eq!(fragment.blocks.len(), 3);
    for (block, text) in fragment.blocks.iter().zip(["a", "b", "c"]) {
        let Block::Paragraph(p) = block else {
            panic!("a paragraph: {block:?}");
        };
        assert_eq!(p.style, ParagraphStyle::Normal);
        assert_eq!(
            p.runs,
            [Run {
                text: text.to_owned(),
                style: bold.clone()
            }]
        );
    }
    assert_eq!(fragment.plain_text(), "a\nb\nc");

    let mut doc = md("one\n\ntwo");
    checked(&mut doc, |d| paste(d, pos(0, 3), pos(0, 3), &fragment));
    assert_eq!(doc.blocks.len(), 4);
    assert_eq!(runs(&doc.blocks[0]), [plain("one"), run("a", true, false)]);
    assert_eq!(runs(&doc.blocks[2]), [run("c", true, false)]);
}

#[test]
fn paste_into_a_code_block_is_plain_text() {
    let mut doc = md("```\ncode\n```");
    let fragment = Fragment::from_plain("a\nb", &RunStyle::default());
    let change = checked(&mut doc, |d| paste(d, pos(0, 2), pos(0, 2), &fragment));
    assert_eq!(doc.blocks.len(), 1);
    assert_eq!(txt(&doc, Container::Top, 0), "coa\nbde");
    assert_eq!(change.caret, pos(0, 5));
}

#[test]
fn pasted_numbered_items_renumber() {
    let mut doc = md("1. a\n2. b\n\nEnd");
    let fragment = copy(&doc, pos(0, 1), pos(1, 1)).expect("a fragment");
    checked(&mut doc, |d| paste(d, pos(1, 1), pos(1, 1), &fragment));
    let labels: Vec<u64> = doc.blocks[..3]
        .iter()
        .map(|b| match b {
            Block::ListItem(item) => match item.kind {
                ListKind::Ordered { number, .. } => number,
                ref other => panic!("not numbered: {other:?}"),
            },
            other => panic!("not a list item: {other:?}"),
        })
        .collect();
    assert_eq!(labels, [1, 2, 3]);
}

#[test]
fn cut_then_paste_moves_the_text() {
    let original = doc();
    let mut doc = original.clone();
    let mut history = History::new();
    let fragment = copy(&doc, pos(0, 6), pos(2, 2)).expect("a fragment");
    let cut = delete(&doc, pos(0, 6), pos(2, 2)).expect("an edit");
    history.apply(&mut doc, cut);
    assert_eq!(doc.blocks.len(), 4);
    let put = paste(&doc, pos(3, 4), pos(3, 4), &fragment).expect("an edit");
    history.apply(&mut doc, put);
    assert_eq!(txt(&doc, Container::Top, 0), "Intro o");
    assert_eq!(txt(&doc, Container::Top, 3), "Lastbold text");
    assert_eq!(doc.blocks[4], original.blocks[1]);
    assert_eq!(txt(&doc, Container::Top, 5), "tw");

    history.undo(&mut doc).expect("the paste");
    history.undo(&mut doc).expect("the cut");
    assert_eq!(doc, original);
}
