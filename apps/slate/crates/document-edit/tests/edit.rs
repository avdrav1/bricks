//! Slate's edits on small documents and on the Markdown fixture: typing, Enter, Backspace,
//! Delete, bold and italic, paragraph styles, lists, and undo and redo giving back the
//! document exactly.

use document_edit::{
    blocks, indent, join_backward, join_forward, look, normalize, replace, set_style, split, text,
    text_len, toggle, toggle_list, Change, Container, Edit, Flag, History, Pos,
};
use document_model::{read_markdown, Block, Document, ListKind, ParagraphStyle, RunStyle, Table};
use Container::Top;

fn md(source: &str) -> Document {
    let mut doc = read_markdown(source.as_bytes());
    normalize(&mut doc);
    doc
}

fn fixture() -> Document {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../document-model/tests/data/fixture.md"
    );
    let mut doc = read_markdown(&std::fs::read(path).expect("the fixture"));
    normalize(&mut doc);
    doc
}

fn pos(block: usize, offset: usize) -> Pos {
    Pos::new(block, offset)
}

fn txt(doc: &Document, block: usize) -> String {
    text(doc, Top, block)
        .expect("an editable block")
        .into_owned()
}

/// The first block whose text starts with `prefix`.
fn find(doc: &Document, prefix: &str) -> usize {
    (0..doc.blocks.len())
        .find(|&i| text(doc, Top, i).is_some_and(|t| t.starts_with(prefix)))
        .unwrap_or_else(|| panic!("no block starting {prefix:?}"))
}

fn end(doc: &Document, block: usize) -> Pos {
    pos(block, text_len(doc, Top, block).expect("an editable block"))
}

fn style(doc: &Document, block: usize) -> ParagraphStyle {
    match &doc.blocks[block] {
        Block::Paragraph(p) => p.style,
        other => panic!("not a paragraph: {other:?}"),
    }
}

fn item(doc: &Document, block: usize) -> (&ListKind, u8) {
    match &doc.blocks[block] {
        Block::ListItem(item) => (&item.kind, item.level),
        other => panic!("not a list item: {other:?}"),
    }
}

fn label(doc: &Document, block: usize) -> (u64, String) {
    match item(doc, block).0 {
        ListKind::Ordered { number, label } => (*number, label.clone()),
        other => panic!("not numbered: {other:?}"),
    }
}

fn runs(doc: &Document, block: usize) -> Vec<(String, bool, bool)> {
    let p = match &doc.blocks[block] {
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

/// Make an edit from the document and apply it.
fn act(
    history: &mut History,
    doc: &mut Document,
    make: impl FnOnce(&Document) -> Option<Edit>,
) -> Change {
    let edit = make(doc).expect("an edit");
    history.apply(doc, edit)
}

/// Make and apply an edit, check undo gives back the document before and redo the one
/// after.
fn apply_checked(doc: &mut Document, make: impl FnOnce(&Document) -> Option<Edit>) {
    let before = doc.clone();
    let mut history = History::new();
    act(&mut history, doc, make);
    let after = doc.clone();
    history.undo(doc).expect("a step to undo");
    assert_eq!(*doc, before);
    history.redo(doc).expect("a step to redo");
    assert_eq!(*doc, after);
}

#[test]
fn typing_takes_the_look_of_the_character_before() {
    let mut doc = md("plain **bold** end");
    let mut history = History::new();

    // Inside the bold run.
    let at = pos(0, 8);
    assert!(look(&doc, at).bold);
    act(&mut history, &mut doc, |d| {
        replace(d, at, at, "X", &look(d, at))
    });
    assert_eq!(
        runs(&doc, 0),
        [
            run("plain ", false, false),
            run("boXld", true, false),
            run(" end", false, false)
        ]
    );

    // At the start of the bold run the character before is plain.
    let at = pos(0, 6);
    assert!(!look(&doc, at).bold);
    history.seal();
    act(&mut history, &mut doc, |d| {
        replace(d, at, at, "Y", &look(d, at))
    });
    assert_eq!(runs(&doc, 0)[0], run("plain Y", false, false));

    // At the end of the bold run the character before is bold.
    let at = pos(0, 12);
    assert_eq!(&txt(&doc, 0)[..12], "plain YboXld");
    assert!(look(&doc, at).bold);
    history.seal();
    act(&mut history, &mut doc, |d| {
        replace(d, at, at, "Z", &look(d, at))
    });
    assert_eq!(runs(&doc, 0)[1], run("boXldZ", true, false));

    // At a paragraph's start: its first character's look.
    let doc = md("**bold** plain");
    assert!(look(&doc, pos(0, 0)).bold);
    // Links don't carry over to typed text.
    let doc = md("[link](https://example.com)");
    assert_eq!(look(&doc, pos(0, 4)).link, None);
    assert_eq!(look(&md(""), pos(0, 0)), RunStyle::default());
}

#[test]
fn replacing_across_paragraphs_keeps_the_first_blocks_looks() {
    let mut doc = md("# Head one\n\nsecond *para* here");
    let before = doc.clone();
    let mut history = History::new();
    let edit = replace(&doc, pos(1, 7), pos(0, 5), "X", &look(&doc, pos(0, 5))).unwrap();
    let change = history.apply(&mut doc, edit);
    assert_eq!(doc.blocks.len(), 1);
    assert_eq!(style(&doc, 0), ParagraphStyle::Heading(1));
    assert_eq!(
        runs(&doc, 0),
        [
            run("Head X", false, false),
            run("para", false, true),
            run(" here", false, false)
        ]
    );
    assert_eq!((change.start, change.removed, change.inserted), (0, 2, 1));
    assert_eq!(change.caret, pos(0, 6));
    history.undo(&mut doc);
    assert_eq!(doc, before);

    // Deleting a selection is replacing it with nothing.
    let mut doc = md("one two");
    let mut history = History::new();
    act(&mut history, &mut doc, |d| {
        replace(d, pos(0, 3), pos(0, 7), "", &RunStyle::default())
    });
    assert_eq!(txt(&doc, 0), "one");
    assert!(replace(&doc, pos(0, 1), pos(0, 1), "", &RunStyle::default()).is_none());
}

#[test]
fn enter_after_a_heading_gives_a_normal_paragraph() {
    let mut doc = md("# Title\n\nbody");
    let mut history = History::new();
    let change = act(&mut history, &mut doc, |d| split(d, pos(0, 5), pos(0, 5)));
    assert_eq!(change.caret, pos(1, 0));
    assert_eq!(style(&doc, 0), ParagraphStyle::Heading(1));
    assert_eq!(style(&doc, 1), ParagraphStyle::Normal);
    assert_eq!(txt(&doc, 1), "");

    // In the middle both halves stay headings.
    let mut doc = md("# Title\n\nbody");
    apply_checked(&mut doc, |d| split(d, pos(0, 2), pos(0, 2)));
    assert_eq!((txt(&doc, 0), txt(&doc, 1)), ("Ti".into(), "tle".into()));
    assert_eq!(style(&doc, 1), ParagraphStyle::Heading(1));

    // Paragraphs keep their style.
    let mut doc = md("> quoted");
    apply_checked(&mut doc, |d| split(d, pos(0, 6), pos(0, 6)));
    assert_eq!(style(&doc, 1), ParagraphStyle::Quote);
}

#[test]
fn enter_in_a_numbered_list_renumbers_the_items_after() {
    let mut doc = md("1. one\n2. two\n3. three");
    let before = doc.clone();
    let mut history = History::new();
    let change = act(&mut history, &mut doc, |d| split(d, pos(0, 3), pos(0, 3)));
    assert_eq!(doc.blocks.len(), 4);
    let labels: Vec<_> = (0..4).map(|i| label(&doc, i)).collect();
    assert_eq!(
        labels,
        [
            (1, "1.".into()),
            (2, "2.".into()),
            (3, "3.".into()),
            (4, "4.".into())
        ]
    );
    assert_eq!(txt(&doc, 1), "");
    assert_eq!(txt(&doc, 2), "two");
    // The relabelled items are inside the change.
    assert_eq!((change.start, change.removed, change.inserted), (0, 3, 4));
    assert_eq!(change.caret, pos(1, 0));
    history.undo(&mut doc);
    assert_eq!(doc, before);

    // A list starting at seven keeps counting from seven, labelled like its first item.
    let mut doc = fixture();
    let seven = find(&doc, "Seven starts");
    apply_checked(&mut doc, |d| split(d, pos(seven, 5), pos(seven, 5)));
    assert_eq!(label(&doc, seven), (7, "7.".into()));
    assert_eq!(label(&doc, seven + 1), (8, "8.".into()));
    assert_eq!(label(&doc, seven + 2), (9, "9.".into()));
    assert_eq!(txt(&doc, seven + 2), "Eight follows");

    // Labels keep their punctuation.
    let mut doc = md("3. a\n4. b");
    for (i, l) in ["3)", "4)"].into_iter().enumerate() {
        if let Block::ListItem(item) = &mut doc.blocks[i] {
            if let ListKind::Ordered { label, .. } = &mut item.kind {
                *label = l.into();
            }
        }
    }
    apply_checked(&mut doc, |d| split(d, pos(0, 1), pos(0, 1)));
    assert_eq!(label(&doc, 1), (4, "4)".into()));
    assert_eq!(label(&doc, 2), (5, "5)".into()));

    // A new task is open.
    let mut doc = md("- [x] done");
    apply_checked(&mut doc, |d| split(d, pos(0, 4), pos(0, 4)));
    assert_eq!(item(&doc, 1).0, &ListKind::Task { checked: false });

    // Enter in code is a line break.
    let mut doc = md("```\nab\n```");
    apply_checked(&mut doc, |d| split(d, pos(0, 1), pos(0, 1)));
    assert_eq!(doc.blocks.len(), 1);
    assert_eq!(txt(&doc, 0), "a\nb");
}

#[test]
fn enter_on_an_empty_item_leaves_the_list() {
    let mut doc = md("- a\n- b\n- c");
    let mut history = History::new();
    act(&mut history, &mut doc, |d| {
        replace(d, pos(1, 0), pos(1, 1), "", &RunStyle::default())
    });
    let before = doc.clone();
    let change = act(&mut history, &mut doc, |d| split(d, pos(1, 0), pos(1, 0)));
    assert_eq!(doc.blocks.len(), 3);
    assert_eq!(style(&doc, 1), ParagraphStyle::Normal);
    assert_eq!(change.caret, pos(1, 0));
    assert_eq!(change.anchor, None);
    history.undo(&mut doc);
    assert_eq!(doc, before);
}

#[test]
fn backspace_at_a_blocks_start() {
    // An item leaves its list, keeping its text; numbers after it start over.
    let mut doc = md("1. a\n2. b\n3. c");
    let mut history = History::new();
    let change = act(&mut history, &mut doc, |d| join_backward(d, pos(1, 0)));
    assert_eq!(style(&doc, 1), ParagraphStyle::Normal);
    assert_eq!(txt(&doc, 1), "b");
    assert_eq!(change.caret, pos(1, 0));
    assert_eq!(label(&doc, 2), (3, "3.".into()));

    // A paragraph joins the one before.
    let mut doc = md("one\n\n*two*");
    let before = doc.clone();
    let mut history = History::new();
    let change = act(&mut history, &mut doc, |d| join_backward(d, pos(1, 0)));
    assert_eq!(doc.blocks.len(), 1);
    assert_eq!(
        runs(&doc, 0),
        [run("one", false, false), run("two", false, true)]
    );
    assert_eq!(change.caret, pos(0, 3));
    history.undo(&mut doc);
    assert_eq!(doc, before);

    // A paragraph joins the list item before it.
    let mut doc = md("- one\n\ntwo");
    apply_checked(&mut doc, |d| join_backward(d, pos(1, 0)));
    assert_eq!(txt(&doc, 0), "onetwo");
    assert!(matches!(doc.blocks[0], Block::ListItem(_)));

    // Nothing before, not at the start, after code, or in code.
    let doc = md("one\n\n```\ncode\n```\n\ntwo");
    assert!(join_backward(&doc, pos(0, 0)).is_none());
    assert!(join_backward(&doc, pos(0, 1)).is_none());
    assert!(join_backward(&doc, pos(1, 0)).is_none());
    assert!(join_backward(&doc, pos(2, 0)).is_none());
}

#[test]
fn delete_at_a_blocks_end_joins_the_next() {
    let mut doc = md("**one**\n\ntwo");
    let before = doc.clone();
    let mut history = History::new();
    let change = act(&mut history, &mut doc, |d| join_forward(d, pos(0, 3)));
    assert_eq!(
        runs(&doc, 0),
        [run("one", true, false), run("two", false, false)]
    );
    assert_eq!(change.caret, pos(0, 3));
    history.undo(&mut doc);
    assert_eq!(doc, before);

    // An item after a paragraph joins it and the numbers after it follow on.
    let mut doc = md("1. a\n2. b\n3. c");
    apply_checked(&mut doc, |d| join_forward(d, pos(0, 1)));
    assert_eq!(txt(&doc, 0), "ab");
    assert_eq!(label(&doc, 1), (2, "2.".into()));

    let doc = md("one\n\n```\ncode\n```");
    assert!(join_forward(&doc, pos(0, 1)).is_none());
    assert!(join_forward(&doc, pos(0, 3)).is_none());
    assert!(join_forward(&doc, pos(1, 4)).is_none());
}

#[test]
fn bold_and_italic_set_and_clear() {
    let mut doc = md("plain **bold** end");
    let mut history = History::new();
    let all = (pos(0, 0), pos(0, 14));
    let change = act(&mut history, &mut doc, |d| {
        toggle(d, all.1, all.0, Flag::Bold)
    });
    assert_eq!(runs(&doc, 0), [run("plain bold end", true, false)]);
    assert_eq!((change.anchor, change.caret), (Some(all.0), all.1));
    act(&mut history, &mut doc, |d| {
        toggle(d, all.0, all.1, Flag::Bold)
    });
    assert_eq!(runs(&doc, 0), [run("plain bold end", false, false)]);
    act(&mut history, &mut doc, |d| {
        toggle(d, pos(0, 6), pos(0, 10), Flag::Italic)
    });
    assert_eq!(
        runs(&doc, 0),
        [
            run("plain ", false, false),
            run("bold", false, true),
            run(" end", false, false)
        ]
    );
    history.undo(&mut doc);
    history.undo(&mut doc);
    history.undo(&mut doc);
    assert_eq!(doc, md("plain **bold** end"));

    // Across paragraphs.
    let mut doc = md("one\n\n- two");
    apply_checked(&mut doc, |d| toggle(d, pos(0, 1), pos(1, 2), Flag::Italic));
    assert_eq!(
        runs(&doc, 0),
        [run("o", false, false), run("ne", false, true)]
    );
    assert_eq!(
        runs(&doc, 1),
        [run("tw", false, true), run("o", false, false)]
    );

    // Nothing selected, or only code.
    assert!(toggle(&doc, pos(0, 1), pos(0, 1), Flag::Bold).is_none());
    let code = md("```\ncode\n```");
    assert!(toggle(&code, pos(0, 0), pos(0, 4), Flag::Bold).is_none());
}

#[test]
fn paragraph_styles() {
    let mut doc = md("a\n\n- b\n\n---");
    let mut history = History::new();
    let change = act(&mut history, &mut doc, |d| {
        set_style(d, Top, 0..3, ParagraphStyle::Heading(2))
    });
    assert_eq!(style(&doc, 0), ParagraphStyle::Heading(2));
    assert_eq!(style(&doc, 1), ParagraphStyle::Heading(2));
    assert_eq!(txt(&doc, 1), "b");
    assert!(matches!(doc.blocks[2], Block::Rule));
    assert_eq!((change.anchor, change.caret), (Some(pos(0, 0)), pos(1, 1)));
    apply_checked(&mut doc, |d| {
        set_style(d, Top, 0..2, ParagraphStyle::Normal)
    });
    assert_eq!(style(&doc, 0), ParagraphStyle::Normal);
    assert!(set_style(&doc, Top, 0..3, ParagraphStyle::Normal).is_none());
    history.undo(&mut doc);
}

#[test]
fn lists_on_and_off() {
    let mut doc = md("a\n\n# b");
    let before = doc.clone();
    let mut history = History::new();
    act(&mut history, &mut doc, |d| toggle_list(d, Top, 0..2, true));
    assert_eq!(label(&doc, 0), (1, "1.".into()));
    assert_eq!(label(&doc, 1), (2, "2.".into()));
    assert_eq!(item(&doc, 1).1, 0);
    act(&mut history, &mut doc, |d| toggle_list(d, Top, 0..2, true));
    assert_eq!(style(&doc, 0), ParagraphStyle::Normal);
    assert_eq!(style(&doc, 1), ParagraphStyle::Normal);
    assert_eq!(txt(&doc, 1), "b");
    history.undo(&mut doc);
    history.undo(&mut doc);
    assert_eq!(doc, before);

    // Bullets over a numbered list make it bulleted; again turns them off.
    let mut doc = md("1. a\n2. b");
    apply_checked(&mut doc, |d| toggle_list(d, Top, 0..2, false));
    assert_eq!(item(&doc, 0).0, &ListKind::Bullet);
    apply_checked(&mut doc, |d| toggle_list(d, Top, 0..2, false));
    assert!(matches!(doc.blocks[0], Block::Paragraph(_)));

    // A paragraph after a numbered list joins its count.
    let mut doc = md("1. a\n2. b\n\nc");
    apply_checked(&mut doc, |d| toggle_list(d, Top, 2..3, true));
    assert_eq!(label(&doc, 2), (3, "3.".into()));
    assert!(toggle_list(&md("---"), Top, 0..1, true).is_none());
}

#[test]
fn indenting_list_items() {
    let mut doc = md("1. a\n2. b\n3. c");
    let mut history = History::new();
    act(&mut history, &mut doc, |d| indent(d, Top, 1..2, 1));
    assert_eq!(item(&doc, 1).1, 1);
    assert_eq!(label(&doc, 1), (1, "1.".into()));
    assert_eq!(label(&doc, 2), (2, "2.".into()));
    act(&mut history, &mut doc, |d| indent(d, Top, 1..2, -1));
    assert_eq!(item(&doc, 1).1, 0);
    assert_eq!(label(&doc, 1), (2, "2.".into()));
    assert_eq!(label(&doc, 2), (3, "3.".into()));
    history.undo(&mut doc);
    history.undo(&mut doc);
    assert_eq!(doc, md("1. a\n2. b\n3. c"));

    // Levels stay within 0..=8; paragraphs don't indent.
    assert!(indent(&doc, Top, 0..1, -1).is_none());
    apply_checked(&mut doc, |d| indent(d, Top, 0..1, 20));
    assert_eq!(item(&doc, 0).1, 8);
    assert!(indent(&doc, Top, 0..1, 1).is_none());
    assert!(indent(&md("a"), Top, 0..1, 1).is_none());
}

/// An edit made from the document as it is.
type MakeEdit = Box<dyn Fn(&Document) -> Option<Edit>>;

#[test]
fn every_undo_and_redo_gives_back_the_fixture_exactly() {
    let mut doc = fixture();
    let mut history = History::new();
    let mut states = vec![doc.clone()];
    let steps: Vec<MakeEdit> = vec![
        Box::new(|d| {
            let at = end(d, find(d, "The last paragraph"));
            replace(d, at, at, " More", &look(d, at))
        }),
        Box::new(|d| {
            let at = end(d, find(d, "Second level heading"));
            split(d, at, at)
        }),
        Box::new(|d| {
            let at = end(d, find(d, "First number"));
            split(d, at, at)
        }),
        Box::new(|d| join_backward(d, pos(find(d, "Back to the first"), 0))),
        Box::new(|d| {
            let a = find(d, "A paragraph with");
            toggle(d, pos(a, 2), pos(a + 1, 5), Flag::Bold)
        }),
        Box::new(|d| {
            let a = find(d, "A paragraph with");
            set_style(d, Top, a..a + 1, ParagraphStyle::Heading(3))
        }),
        Box::new(|d| {
            let a = find(d, "A list that starts");
            toggle_list(d, Top, a..a + 1, true)
        }),
        Box::new(|d| {
            let a = find(d, "Second number");
            indent(d, Top, a..a + 1, 1)
        }),
        Box::new(|d| {
            let (a, b) = (find(d, "Text with inline"), find(d, "The last paragraph"));
            replace(d, pos(a, 4), pos(b, 4), "Z", &RunStyle::default())
        }),
        Box::new(|d| join_forward(d, end(d, find(d, "Fourth level heading")))),
        Box::new(|d| {
            let a = find(d, "fn main");
            split(d, pos(a, 11), pos(a, 11))
        }),
        Box::new(|d| {
            let a = find(d, "fn main");
            replace(d, pos(a, 0), pos(a, 2), "pub fn", &RunStyle::default())
        }),
        Box::new(|d| {
            let (a, b) = (find(d, "First bullet"), find(d, "An open task"));
            replace(d, pos(a, 5), pos(b, 3), "", &RunStyle::default())
        }),
        Box::new(|d| join_backward(d, pos(find(d, "Firstopen task"), 0))),
    ];
    for (i, step) in steps.iter().enumerate() {
        let edit = step(&doc).unwrap_or_else(|| panic!("step {i} makes an edit"));
        history.seal();
        history.apply(&mut doc, edit);
        assert_ne!(
            doc,
            *states.last().unwrap(),
            "step {i} changes the document"
        );
        states.push(doc.clone());
    }
    // The Enter after "First number" renumbered the items after it.
    let first = find(&states[3], "First number");
    assert_eq!(label(&states[3], first + 1), (2, "2.".into()));
    assert_eq!(label(&states[3], first + 2), (3, "3.".into()));
    assert_eq!(label(&states[3], first + 3), (1, "1.".into()));
    assert_eq!(label(&states[3], first + 4), (4, "4.".into()));

    for i in (0..steps.len()).rev() {
        history.undo(&mut doc).expect("a step to undo");
        assert_eq!(doc, states[i], "undoing step {i}");
    }
    assert!(!history.can_undo());
    for (i, state) in states.iter().enumerate().skip(1) {
        history.redo(&mut doc).expect("a step to redo");
        assert_eq!(doc, *state, "redoing step {}", i - 1);
    }
    assert!(!history.can_redo());
}

#[test]
fn typing_joins_a_word_at_a_time() {
    let mut doc = md("x");
    let before = doc.clone();
    let mut history = History::new();
    for (i, c) in "hello world".char_indices() {
        let at = pos(0, 1 + i);
        let edit = replace(&doc, at, at, &c.to_string(), &look(&doc, at)).unwrap();
        history.apply(&mut doc, edit);
    }
    assert_eq!(txt(&doc, 0), "xhello world");
    let mut steps = 0;
    while history.undo(&mut doc).is_some() {
        steps += 1;
    }
    assert_eq!(steps, 2);
    assert_eq!(doc, before);

    // Sealing between characters keeps them apart.
    let mut doc = md("x");
    let mut history = History::new();
    for i in 0..3 {
        history.seal();
        let at = pos(0, 1 + i);
        act(&mut history, &mut doc, |d| {
            replace(d, at, at, "a", &look(d, at))
        });
    }
    let mut steps = 0;
    while history.undo(&mut doc).is_some() {
        steps += 1;
    }
    assert_eq!(steps, 3);
}

/// A paragraph, a 2x2 table (a header row and a body row) whose cells hold a paragraph
/// each, and a paragraph.
const TABLE: &str = "before\n\n| h1 | h2 |\n| --- | --- |\n| a | b |\n\nafter";

fn cell(table: usize, row: usize, cell: usize) -> Container {
    Container::Cell { table, row, cell }
}

fn cell_text(doc: &Document, at: Container, block: usize) -> String {
    text(doc, at, block)
        .expect("an editable block")
        .into_owned()
}

fn table(doc: &Document, block: usize) -> &Table {
    match &doc.blocks[block] {
        Block::Table(t) => t,
        other => panic!("not a table: {other:?}"),
    }
}

/// The table's shape: each row's cell count.
fn shape(doc: &Document, block: usize) -> Vec<usize> {
    table(doc, block)
        .rows
        .iter()
        .map(|r| r.cells.len())
        .collect()
}

#[test]
fn a_cell_is_a_container() {
    let doc = md(TABLE);
    assert!(matches!(doc.blocks[1], Block::Table(_)));
    assert_eq!(shape(&doc, 1), [2, 2]);
    assert_eq!(blocks(&doc, cell(1, 1, 0)).map(<[Block]>::len), Some(1));
    assert_eq!(cell_text(&doc, cell(1, 0, 1), 0), "h2");
    assert_eq!(cell_text(&doc, cell(1, 1, 0), 0), "a");
    assert_eq!(blocks(&doc, Top).map(<[Block]>::len), Some(3));
    // Not a table, or no such row or cell.
    assert!(blocks(&doc, cell(0, 0, 0)).is_none());
    assert!(blocks(&doc, cell(1, 2, 0)).is_none());
    assert!(blocks(&doc, cell(1, 0, 2)).is_none());
    assert!(text_len(&doc, cell(1, 1, 0), 1).is_none());
    assert_eq!(text_len(&doc, cell(1, 1, 0), 0), Some(1));
}

#[test]
fn typing_and_backspace_in_a_cell_edit_only_that_cell() {
    let mut doc = md(TABLE);
    let before = doc.clone();
    let c = cell(1, 1, 0);
    let mut history = History::new();
    let at = Pos::in_cell(1, 1, 0, 0, 1);
    let change = act(&mut history, &mut doc, |d| {
        replace(d, at, at, "xe", &look(d, at))
    });
    assert_eq!(cell_text(&doc, c, 0), "axe");
    assert_eq!(change.at, c);
    assert_eq!((change.start, change.removed, change.inserted), (0, 1, 1));
    assert_eq!(change.caret, Pos::in_cell(1, 1, 0, 0, 3));

    // Only that cell changed.
    let mut expected = before.clone();
    if let Block::Table(t) = &mut expected.blocks[1] {
        t.rows[1].cells[0] = table(&doc, 1).rows[1].cells[0].clone();
    }
    assert_eq!(doc, expected);

    // Backspace over the "x".
    history.seal();
    let change = act(&mut history, &mut doc, |d| {
        let (from, to) = (Pos::in_cell(1, 1, 0, 0, 1), Pos::in_cell(1, 1, 0, 0, 2));
        replace(d, to, from, "", &RunStyle::default())
    });
    assert_eq!(cell_text(&doc, c, 0), "ae");
    assert_eq!(change.caret, Pos::in_cell(1, 1, 0, 0, 1));
    assert_eq!(txt(&doc, 0), "before");
    assert_eq!(txt(&doc, 2), "after");
    assert_eq!(cell_text(&doc, cell(1, 1, 1), 0), "b");

    // Backspace at a cell's start doesn't join the cell before or the text before the
    // table; Delete at its end doesn't join the cell after.
    assert!(join_backward(&doc, Pos::in_cell(1, 1, 0, 0, 0)).is_none());
    assert!(join_backward(&doc, Pos::in_cell(1, 0, 0, 0, 0)).is_none());
    assert!(join_forward(&doc, Pos::in_cell(1, 1, 0, 0, 2)).is_none());
    assert!(join_forward(&doc, Pos::in_cell(1, 1, 1, 0, 1)).is_none());
    // The paragraph after the table doesn't join it either.
    assert!(join_backward(&doc, pos(2, 0)).is_none());

    history.undo(&mut doc);
    history.undo(&mut doc);
    assert_eq!(doc, before);
}

#[test]
fn enter_in_a_cell_adds_a_paragraph_in_the_cell() {
    let mut doc = md(TABLE);
    let before = doc.clone();
    let c = cell(1, 0, 1);
    let mut history = History::new();
    let at = Pos::in_cell(1, 0, 1, 0, 1);
    let change = act(&mut history, &mut doc, |d| split(d, at, at));
    assert_eq!(change.at, c);
    assert_eq!(change.caret, Pos::in_cell(1, 0, 1, 1, 0));
    assert_eq!(blocks(&doc, c).map(<[Block]>::len), Some(2));
    assert_eq!(
        (cell_text(&doc, c, 0), cell_text(&doc, c, 1)),
        ("h".into(), "2".into())
    );
    assert_eq!(shape(&doc, 1), [2, 2]);
    assert_eq!(doc.blocks.len(), 3);

    // Backspace at the cell's second paragraph joins the first; at its first, nothing.
    history.seal();
    act(&mut history, &mut doc, |d| {
        join_backward(d, Pos::in_cell(1, 0, 1, 1, 0))
    });
    assert_eq!(blocks(&doc, c).map(<[Block]>::len), Some(1));
    assert_eq!(cell_text(&doc, c, 0), "h2");
    assert!(join_backward(&doc, Pos::in_cell(1, 0, 1, 0, 0)).is_none());

    history.undo(&mut doc);
    history.undo(&mut doc);
    assert_eq!(doc, before);
}

#[test]
fn edits_dont_cross_out_of_a_cell() {
    let doc = md(TABLE);
    let (a, after) = (Pos::in_cell(1, 1, 0, 0, 0), pos(2, 2));
    assert!(replace(&doc, a, after, "x", &RunStyle::default()).is_none());
    assert!(replace(&doc, after, a, "", &RunStyle::default()).is_none());
    assert!(split(&doc, a, after).is_none());
    assert!(toggle(&doc, a, after, Flag::Bold).is_none());
    let b = Pos::in_cell(1, 1, 1, 0, 1);
    assert!(replace(&doc, a, b, "", &RunStyle::default()).is_none());
    // Within one cell it works.
    assert!(replace(
        &doc,
        a,
        Pos::in_cell(1, 1, 0, 0, 1),
        "",
        &RunStyle::default()
    )
    .is_some());
}

#[test]
fn places_order_as_in_the_document() {
    let before = pos(0, 3);
    let after = pos(2, 0);
    let first = Pos::in_cell(1, 0, 0, 0, 2);
    let later = Pos::in_cell(1, 0, 1, 0, 0);
    let next_row = Pos::in_cell(1, 1, 0, 0, 0);
    let second_block = Pos::in_cell(1, 0, 0, 1, 0);
    assert!(before < first && first < after);
    assert!(before < next_row && next_row < after);
    assert!(first < second_block && second_block < later && later < next_row);
    let mut all = [after, next_row, later, before, second_block, first];
    all.sort();
    assert_eq!(all, [before, first, second_block, later, next_row, after]);
    assert_eq!(Pos::default(), pos(0, 0));
}

#[test]
fn every_undo_and_redo_gives_back_a_document_with_a_table_exactly() {
    let mut doc = md(TABLE);
    let mut history = History::new();
    let mut states = vec![doc.clone()];
    let steps: Vec<MakeEdit> = vec![
        // Type in a cell.
        Box::new(|d| {
            let at = Pos::in_cell(1, 1, 0, 0, 1);
            replace(d, at, at, "pple", &look(d, at))
        }),
        // Type at the top level.
        Box::new(|d| {
            let at = end(d, 0);
            replace(d, at, at, " the table", &look(d, at))
        }),
        // Enter in a cell, twice.
        Box::new(|d| {
            let at = Pos::in_cell(1, 1, 0, 0, 5);
            split(d, at, at)
        }),
        Box::new(|d| {
            let at = Pos::in_cell(1, 1, 0, 1, 0);
            replace(d, at, at, "pear", &look(d, at))
        }),
        // A numbered list in the cell, then Enter in it renumbers within the cell.
        Box::new(|d| toggle_list(d, cell(1, 1, 0), 0..2, true)),
        Box::new(|d| {
            let at = Pos::in_cell(1, 1, 0, 0, 2);
            split(d, at, at)
        }),
        // Bold across the cell's blocks.
        Box::new(|d| {
            toggle(
                d,
                Pos::in_cell(1, 1, 0, 0, 1),
                Pos::in_cell(1, 1, 0, 2, 2),
                Flag::Bold,
            )
        }),
        // A heading in another cell.
        Box::new(|d| set_style(d, cell(1, 0, 0), 0..1, ParagraphStyle::Heading(2))),
        // Enter at the top level after the table.
        Box::new(|d| split(d, pos(2, 2), pos(2, 2))),
        // Delete across the cell's blocks.
        Box::new(|d| {
            replace(
                d,
                Pos::in_cell(1, 1, 0, 1, 1),
                Pos::in_cell(1, 1, 0, 2, 3),
                "",
                &RunStyle::default(),
            )
        }),
        // Indent and join in the cell.
        Box::new(|d| indent(d, cell(1, 1, 0), 1..2, 1)),
        Box::new(|d| join_forward(d, Pos::in_cell(1, 1, 0, 0, 2))),
        // Delete across the table at the top level.
        Box::new(|d| replace(d, pos(0, 2), pos(2, 1), "X", &RunStyle::default())),
    ];
    for (i, step) in steps.iter().enumerate() {
        let edit = step(&doc).unwrap_or_else(|| panic!("step {i} makes an edit"));
        history.seal();
        history.apply(&mut doc, edit);
        assert_ne!(
            doc,
            *states.last().unwrap(),
            "step {i} changes the document"
        );
        states.push(doc.clone());
    }
    // The Enter in the numbered cell list renumbered the item after it.
    let c = cell(1, 1, 0);
    let numbers: Vec<_> = blocks(&states[6], c)
        .unwrap()
        .iter()
        .map(|b| match b {
            Block::ListItem(item) => match &item.kind {
                ListKind::Ordered { number, .. } => *number,
                other => panic!("not numbered: {other:?}"),
            },
            other => panic!("not a list item: {other:?}"),
        })
        .collect();
    assert_eq!(numbers, [1, 2, 3]);
    assert_eq!(shape(&states[12], 1), [2, 2]);
    assert_eq!(states[12].blocks.len(), 4);

    for i in (0..steps.len()).rev() {
        history.undo(&mut doc).expect("a step to undo");
        assert_eq!(doc, states[i], "undoing step {i}");
    }
    assert!(!history.can_undo());
    for (i, state) in states.iter().enumerate().skip(1) {
        history.redo(&mut doc).expect("a step to redo");
        assert_eq!(doc, *state, "redoing step {}", i - 1);
    }
    assert!(!history.can_redo());
}

#[test]
fn editing_the_docx_fixtures_table() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docx/tests/data/fixture.docx"
    );
    let mut doc = docx::read(std::path::Path::new(path)).expect("the fixture");
    normalize(&mut doc);
    let t = doc
        .blocks
        .iter()
        .position(|b| matches!(b, Block::Table(_)))
        .expect("a table");
    let rows = shape(&doc, t);
    // The first cell whose first block is a paragraph with text.
    let (row, col) = (0..rows.len())
        .flat_map(|r| (0..rows[r]).map(move |c| (r, c)))
        .find(|&(r, c)| {
            matches!(
                blocks(&doc, cell(t, r, c)).and_then(|b| b.first()),
                Some(Block::Paragraph(_))
            ) && text_len(&doc, cell(t, r, c), 0).is_some_and(|n| n > 0)
        })
        .expect("a cell with text");
    let c = cell(t, row, col);
    let len = text_len(&doc, c, 0).unwrap();
    let start = Pos::in_cell(t, row, col, 0, 0);
    let end = Pos::in_cell(t, row, col, 0, len);
    let original = cell_text(&doc, c, 0);
    let cells = blocks(&doc, c).unwrap().len();

    let mut history = History::new();
    let mut states = vec![doc.clone()];
    let steps: Vec<MakeEdit> = vec![
        Box::new(move |d| replace(d, end, end, "!", &look(d, end))),
        Box::new(move |d| split(d, start, start)),
        Box::new(move |d| {
            let at = Pos::in_cell(t, row, col, 1, 0);
            replace(d, at, at, ">", &look(d, at))
        }),
        Box::new(move |d| join_backward(d, Pos::in_cell(t, row, col, 1, 0))),
        Box::new(move |d| toggle(d, start, Pos::in_cell(t, row, col, 0, 1), Flag::Italic)),
        Box::new(|d| {
            let at = pos(find(d, ""), 0);
            replace(d, at, at, "Top ", &RunStyle::default())
        }),
    ];
    for (i, step) in steps.iter().enumerate() {
        let edit = step(&doc).unwrap_or_else(|| panic!("step {i} makes an edit"));
        history.seal();
        history.apply(&mut doc, edit);
        assert_ne!(
            doc,
            *states.last().unwrap(),
            "step {i} changes the document"
        );
        states.push(doc.clone());
    }
    assert_eq!(cell_text(&doc, c, 0), format!(">{original}!"));
    assert_eq!(blocks(&doc, c).unwrap().len(), cells);
    assert_eq!(shape(&doc, t), rows);
    assert!(join_backward(&doc, start).is_none());
    for i in (0..steps.len()).rev() {
        history.undo(&mut doc).expect("a step to undo");
        assert_eq!(doc, states[i], "undoing step {i}");
    }
    for (i, state) in states.iter().enumerate().skip(1) {
        history.redo(&mut doc).expect("a step to redo");
        assert_eq!(doc, *state, "redoing step {}", i - 1);
    }
}
