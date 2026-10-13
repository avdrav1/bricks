//! The format bar's commands (SLATE-21): run looks over a selection and on the look typed
//! next, paragraph alignment, underline and strikethrough toggles, and the looks the bar
//! shows at the caret and across a selection.

use document_edit::{
    format, formatted, look, looks, normalize, set_align, toggle, toggled, Container, Edit, Flag,
    History, LookPatch, Looks, Pos, SizeChange,
};
use document_model::{read_markdown, Align, Block, Document, Paragraph, RunStyle};
use Container::Top;

fn md(source: &str) -> Document {
    let mut doc = read_markdown(source.as_bytes());
    normalize(&mut doc);
    doc
}

fn pos(block: usize, offset: usize) -> Pos {
    Pos::new(block, offset)
}

fn para(doc: &Document, block: usize) -> &Paragraph {
    match &doc.blocks[block] {
        Block::Paragraph(p) => p,
        Block::ListItem(item) => &item.paragraph,
        other => panic!("no runs: {other:?}"),
    }
}

/// Each run's text and direct formatting.
fn runs(doc: &Document, block: usize) -> Vec<(String, RunStyle)> {
    para(doc, block)
        .runs
        .iter()
        .map(|r| (r.text.clone(), r.style.clone()))
        .collect()
}

/// Apply `edit` as one step, check that one undo gives back the document exactly and redo
/// the edited one, and leave it applied.
fn step(doc: &mut Document, make: impl FnOnce(&Document) -> Option<Edit>) -> document_edit::Change {
    let before = doc.clone();
    let mut history = History::new();
    let change = history.apply(doc, make(doc).expect("an edit"));
    let after = doc.clone();
    history.undo(doc);
    assert_eq!(*doc, before, "one undo gives back the document");
    history.redo(doc);
    assert_eq!(*doc, after, "redo gives back the edit");
    change
}

#[test]
fn a_patch_sets_looks_on_the_selected_text_only() {
    let mut doc = md("Alpha beta gamma");
    let patch = LookPatch {
        font: Some(Some("Liberation Mono".into())),
        size: Some(SizeChange::Set(40)),
        underline: Some(true),
        color: Some(Some([0xC0, 0, 0])),
        ..LookPatch::default()
    };
    let change = step(&mut doc, |d| format(d, pos(0, 6), pos(0, 10), &patch));
    // The word stays selected.
    assert_eq!((change.anchor, change.caret), (Some(pos(0, 6)), pos(0, 10)));
    let r = runs(&doc, 0);
    assert_eq!(r.len(), 3);
    assert_eq!((r[0].0.as_str(), &r[0].1), ("Alpha ", &RunStyle::default()));
    assert_eq!(r[1].0, "beta");
    let s = &r[1].1;
    assert_eq!(s.font.as_deref(), Some("Liberation Mono"));
    assert_eq!(
        (s.size, s.underline, s.color),
        (Some(40), Some(true), Some([0xC0, 0, 0]))
    );
    assert_eq!((s.bold, s.italic, s.strike), (None, None, None));
    let shown = doc.styles.look(&para(&doc, 0).style, s);
    assert_eq!((shown.font.as_str(), shown.size), ("Liberation Mono", 40));

    // Already so: no edit. Nothing selected: no edit.
    assert!(format(&doc, pos(0, 6), pos(0, 10), &patch).is_none());
    assert!(format(&doc, pos(0, 6), pos(0, 6), &patch).is_none());

    // The font and colour cleared go back to the style's; the runs join again.
    let clear = LookPatch {
        font: Some(None),
        size: Some(SizeChange::Clear),
        underline: Some(false),
        color: Some(None),
        ..LookPatch::default()
    };
    step(&mut doc, |d| format(d, pos(0, 0), pos(0, 16), &clear));
    let s = &runs(&doc, 0)[0].1;
    assert_eq!((s.font.as_ref(), s.size, s.color), (None, None, None));
    assert_eq!(s.underline, Some(false));
    assert_eq!(runs(&doc, 0).len(), 1);
}

#[test]
fn size_steps_each_runs_size_and_clamps() {
    // A heading (Heading 1: 44 half-points) and body text (22) stepped together.
    let mut doc = md("# Head\n\nbody");
    let up = LookPatch {
        size: Some(SizeChange::Step(2)),
        ..LookPatch::default()
    };
    step(&mut doc, |d| format(d, pos(0, 0), pos(1, 4), &up));
    assert_eq!(runs(&doc, 0)[0].1.size, Some(46));
    assert_eq!(runs(&doc, 1)[0].1.size, Some(24));
    let down = LookPatch {
        size: Some(SizeChange::Step(-2000)),
        ..LookPatch::default()
    };
    step(&mut doc, |d| format(d, pos(1, 0), pos(1, 4), &down));
    assert_eq!(runs(&doc, 1)[0].1.size, Some(document_edit::MIN_SIZE));
    let set = LookPatch {
        size: Some(SizeChange::Set(5000)),
        ..LookPatch::default()
    };
    step(&mut doc, |d| format(d, pos(1, 0), pos(1, 4), &set));
    assert_eq!(runs(&doc, 1)[0].1.size, Some(document_edit::MAX_SIZE));
}

#[test]
fn a_patch_with_no_selection_sets_the_look_typed_next() {
    let doc = md("# Head");
    let at = pos(0, 4);
    let typing = look(&doc, at);
    let patch = LookPatch {
        size: Some(SizeChange::Step(2)),
        strike: Some(true),
        ..LookPatch::default()
    };
    let next = formatted(&doc, at, &typing, &patch);
    assert_eq!((next.size, next.strike), (Some(46), Some(true)));
    // Code isn't touched by run looks.
    let code = md("```\ncode\n```");
    assert!(format(&code, pos(0, 0), pos(0, 4), &patch).is_none());
}

#[test]
fn underline_and_strikethrough_toggle() {
    let mut doc = md("plain text");
    step(&mut doc, |d| {
        toggle(d, pos(0, 0), pos(0, 5), Flag::Underline)
    });
    assert_eq!(runs(&doc, 0)[0].1.underline, Some(true));
    // All underlined: off again.
    step(&mut doc, |d| {
        toggle(d, pos(0, 0), pos(0, 5), Flag::Underline)
    });
    assert_eq!(runs(&doc, 0)[0].1.underline, Some(false));
    step(&mut doc, |d| toggle(d, pos(0, 0), pos(0, 10), Flag::Strike));
    assert!(runs(&doc, 0).iter().all(|r| r.1.strike == Some(true)));
    let typed = toggled(&doc, pos(0, 3), &look(&doc, pos(0, 3)), Flag::Strike);
    assert_eq!(typed.strike, Some(false));
}

#[test]
fn alignment_covers_the_paragraphs_touched() {
    let mut doc = md("one\n\n- two\n\n---\n\nthree");
    let change = step(&mut doc, |d| set_align(d, Top, 0..4, Align::Center));
    assert_eq!(para(&doc, 0).align, Align::Center);
    assert_eq!(para(&doc, 1).align, Align::Center);
    assert!(matches!(doc.blocks[2], Block::Rule));
    assert_eq!(para(&doc, 3).align, Align::Center);
    assert_eq!(para(&doc, 1).runs[0].text, "two");
    assert_eq!((change.start, change.removed), (0, 4));
    // Already so: no edit.
    assert!(set_align(&doc, Top, 0..2, Align::Center).is_none());
    step(&mut doc, |d| set_align(d, Top, 3..4, Align::Justify));
    assert_eq!(para(&doc, 3).align, Align::Justify);
}

#[test]
fn the_bar_shows_the_caret_looks() {
    let doc = md("# Head\n\nplain **bold**");
    let at = pos(0, 2);
    let l = looks(&doc, at, at, None);
    assert_eq!(
        l,
        Looks {
            style: Some("Heading1".into()),
            font: Some("Liberation Sans".into()),
            size: Some(44),
            bold: Some(true),
            italic: Some(false),
            underline: Some(false),
            strike: Some(false),
            color: Some(None),
            align: Some(Align::Start),
        }
    );
    // After the bold word: bold, in Normal.
    let at = pos(1, 10);
    let l = looks(&doc, at, at, None);
    assert_eq!(
        (l.style.as_deref(), l.bold, l.size),
        (Some("Normal"), Some(true), Some(22))
    );
    // The look typed next, when Ctrl+B or the bar changed it.
    let typing = RunStyle {
        italic: Some(true),
        size: Some(30),
        ..RunStyle::default()
    };
    let l = looks(&doc, at, at, Some(&typing));
    assert_eq!(
        (l.bold, l.italic, l.size),
        (Some(false), Some(true), Some(30))
    );
}

#[test]
fn the_bar_shows_mixed_looks_as_none() {
    let doc = md("# Head\n\nplain **bold**");
    // Plain and bold, both Normal.
    let l = looks(&doc, pos(1, 2), pos(1, 8), None);
    assert_eq!((l.bold, l.italic), (None, Some(false)));
    assert_eq!((l.size, l.style.as_deref()), (Some(22), Some("Normal")));
    // Just the bold word.
    let l = looks(&doc, pos(1, 6), pos(1, 10), None);
    assert_eq!(l.bold, Some(true));
    // The heading and the body: styles and sizes differ, bold mixed, the font the same.
    let l = looks(&doc, pos(0, 0), pos(1, 3), None);
    assert_eq!((l.style, l.size, l.bold), (None, None, None));
    assert_eq!(l.font.as_deref(), Some("Liberation Sans"));
    assert_eq!(l.align, Some(Align::Start));
    // Alignment mixed.
    let mut doc = doc;
    step(&mut doc, |d| set_align(d, Top, 1..2, Align::End));
    assert_eq!(looks(&doc, pos(0, 0), pos(1, 3), None).align, None);
    assert_eq!(
        looks(&doc, pos(1, 1), pos(1, 1), None).align,
        Some(Align::End)
    );
    // A caret outside text shows nothing.
    let code = md("```\ncode\n```");
    assert_eq!(looks(&code, pos(0, 1), pos(0, 1), None), Looks::default());
}
