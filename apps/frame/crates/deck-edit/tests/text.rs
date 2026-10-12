//! FRAME-6 acceptance, without the window: typing in a text box or a placeholder changes
//! it, an inherited placeholder keeps its layout's style, and undo and redo restore each
//! step. On the test decks of the `pptx` crate: PowerPoint's `layouts.pptx`, whose
//! placeholders inherit their text style, and LibreOffice's fixture
//! (`scripts/lo_pptx.py fixture`), whose runs set their own.

use deck_edit::{after, before, text_end, History, ShapeRef, TextEdit, TextPos};
use presentation_model::{
    AutoNumber, Bullet, BulletSize, Layer, PlaceholderKind, Presentation, Run, Source, TextBody,
};
use std::path::Path;

fn deck(name: &str) -> Presentation {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../pptx/tests/data")
        .join(name);
    pptx::read(&path).unwrap()
}

fn fixture() -> Presentation {
    deck("fixture.pptx")
}

/// The first of the slide's own shapes with text that `pick` accepts.
fn shape(
    deck: &Presentation,
    slide: usize,
    pick: impl Fn(&presentation_model::Shape) -> bool,
) -> ShapeRef {
    let shapes = &deck.slides[slide].shapes;
    let shape = shapes
        .iter()
        .position(|s| s.layer == Layer::Slide && s.text.is_some() && pick(s))
        .expect("a shape");
    ShapeRef { slide, shape }
}

/// The fixture's text box on slide 3: "Text box: bold blue and theme in Liberation Serif",
/// in four looks.
fn text_box(deck: &Presentation) -> ShapeRef {
    shape(deck, 2, |s| {
        s.placeholder.is_none() && !s.plain_text().is_empty()
    })
}

fn body(deck: &Presentation, s: ShapeRef) -> &TextBody {
    deck.slides[s.slide].shapes[s.shape].text.as_ref().unwrap()
}

/// Type `text` a character at a time at `at`, as the window does; returns the caret.
fn type_text(
    deck: &mut Presentation,
    history: &mut History,
    s: ShapeRef,
    mut at: TextPos,
    text: &str,
) -> TextPos {
    for c in text.chars() {
        let edit = TextEdit::typing(deck, s, at, at, &c.to_string()).unwrap();
        at = history.apply(deck, edit).caret().unwrap();
    }
    at
}

fn inherited(run: &Run) -> bool {
    let s = run.sources;
    [s.font, s.size, s.bold, s.italic, s.color]
        .iter()
        .all(|&s| s != Source::Own)
}

#[test]
fn typing_in_a_placeholder_keeps_its_layouts_style() {
    let mut deck = deck("layouts.pptx");
    let mut history = History::new();
    // Slide 1's subtitle sets no look of its own: its colour comes from the Title Slide
    // layout (75% tint of the text colour), its size from the master's body style.
    let sub = shape(&deck, 0, |s| {
        s.placeholder == Some(PlaceholderKind::Subtitle)
    });
    let frame = deck.slides[0].shapes[sub.shape].frame;
    let original = body(&deck, sub).paragraphs[0].runs[0].clone();
    assert_eq!(original.text, "subtitle");
    assert!(inherited(&original), "{:?}", original.sources);
    assert_eq!(
        (original.sources.color, original.sources.size),
        (Source::Layout, Source::Master)
    );

    let end = text_end(body(&deck, sub));
    type_text(&mut deck, &mut history, sub, end, " here");
    let p = &body(&deck, sub).paragraphs[0];
    assert_eq!(p.text(), "subtitle here");
    assert_eq!(p.runs.len(), 1, "typed text joins the run it follows");
    assert!(p.runs[0].same_look(&original), "{:?}", p.runs[0]);
    assert_eq!(deck.slides[0].shapes[sub.shape].frame, frame);
    assert!(deck.slides[0].text().contains("subtitle here"));

    // Emptied, the placeholder types in its paragraph mark's look, still inherited.
    let all = text_end(body(&deck, sub));
    let clear = TextEdit::delete(&deck, sub, TextPos::default(), all).unwrap();
    history.apply(&mut deck, clear);
    assert!(body(&deck, sub).is_empty());
    type_text(&mut deck, &mut history, sub, TextPos::default(), "New");
    let p = &body(&deck, sub).paragraphs[0];
    assert_eq!(p.text(), "New");
    assert!(p.runs[0].same_look(&p.end));
    assert!(p.runs[0].same_look(&original), "{:?}", p.runs[0]);
}

#[test]
fn typing_in_a_text_box_takes_the_look_before_the_caret() {
    let mut deck = fixture();
    let mut history = History::new();
    let tb = text_box(&deck);
    let looks: Vec<Run> = body(&deck, tb).paragraphs[0].runs.clone();
    assert_eq!(looks[0].text, "Text box: ");
    assert_eq!(looks[1].text, "bold blue");
    // After "Text box: ", at the start of "bold blue": the plain look before the caret.
    type_text(&mut deck, &mut history, tb, TextPos::new(0, 10), "very ");
    // After "bold blue": bold and blue.
    type_text(&mut deck, &mut history, tb, TextPos::new(0, 24), "!");
    let runs = &body(&deck, tb).paragraphs[0].runs;
    let texts: Vec<&str> = runs.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(
        texts,
        [
            "Text box: very ",
            "bold blue!",
            " and theme",
            " in Liberation Serif"
        ]
    );
    assert!(runs[0].same_look(&looks[0]));
    assert!(runs[1].same_look(&looks[1]));
    assert!(runs[1].bold && runs[1].color.hex() == "0070C0");
}

#[test]
fn undo_and_redo_restore_each_step() {
    let mut deck = fixture();
    let mut history = History::new();
    let tb = text_box(&deck);
    let mut states = vec![body(&deck, tb).clone()];
    let mut step = |deck: &Presentation| states.push(body(deck, tb).clone());

    // Typing joins one step per word: "Hello " and "world".
    let end = text_end(body(&deck, tb));
    let at = type_text(&mut deck, &mut history, tb, end, "Hello ");
    step(&deck);
    let at = type_text(&mut deck, &mut history, tb, at, "world");
    step(&deck);
    // Enter between them, then a line break.
    let mid = TextPos::new(0, at.offset - "world".len());
    let enter = TextEdit::new_paragraph(&deck, tb, mid, mid).unwrap();
    let at = history.apply(&mut deck, enter).caret().unwrap();
    assert_eq!(at, TextPos::new(1, 0));
    step(&deck);
    let br = TextEdit::line_break(&deck, tb, at, at).unwrap();
    assert_eq!(
        history.apply(&mut deck, br).caret(),
        Some(TextPos::new(1, 1))
    );
    step(&deck);
    // Backspace twice: the line break, then the paragraph end (the paragraphs merge).
    for at in [TextPos::new(1, 1), TextPos::new(1, 0)] {
        let from = before(body(&deck, tb), at).unwrap();
        let delete = TextEdit::delete(&deck, tb, from, at).unwrap();
        history.apply(&mut deck, delete);
        step(&deck);
    }
    assert_eq!(body(&deck, tb).paragraphs.len(), 1);
    // A selection from inside "Text box" to the end, typed over.
    let end = text_end(body(&deck, tb));
    let x = TextEdit::typing(&deck, tb, TextPos::new(0, 4), end, "X").unwrap();
    history.apply(&mut deck, x);
    step(&deck);
    assert_eq!(body(&deck, tb).plain_text(), "TextX");

    let n = states.len() - 1;
    assert_eq!(n, 7);
    for i in (0..n).rev() {
        history.undo(&mut deck).expect("a step to undo");
        assert_eq!(body(&deck, tb), &states[i], "undo to step {i}");
    }
    assert!(!history.can_undo());
    for (i, state) in states.iter().enumerate().skip(1) {
        history.redo(&mut deck).expect("a step to redo");
        assert_eq!(body(&deck, tb), state, "redo to step {i}");
    }
    assert!(!history.can_redo());
    assert!(after(body(&deck, tb), text_end(body(&deck, tb))).is_none());
}

#[test]
fn new_paragraphs_take_their_numbers() {
    let mut deck = fixture();
    let mut history = History::new();
    // Slide 2's body, numbered at every level.
    let list = shape(&deck, 1, |s| s.placeholder == Some(PlaceholderKind::Body));
    let text = deck.slides[1].shapes[list.shape].text.as_mut().unwrap();
    for p in &mut text.paragraphs {
        p.bullet = Some(Bullet {
            text: String::new(),
            autonumber: Some(AutoNumber {
                scheme: "arabicPeriod".into(),
                start: 1,
            }),
            font: None,
            size: BulletSize::FollowText,
            color: None,
        });
    }
    text.number_bullets();
    let marks = |deck: &Presentation| -> Vec<Option<String>> {
        body(deck, list)
            .paragraphs
            .iter()
            .map(|p| p.shown_bullet().map(|b| b.text.to_owned()))
            .collect()
    };
    let first = marks(&deck);
    let s = |m: &str| Some(m.to_owned());
    assert_eq!(&first[..4], [s("1."), s("1."), s("1."), s("2.")]);

    // Enter after "First level": an empty paragraph shows no number until typed into.
    let end = TextPos::new(0, "First level".len());
    let enter = TextEdit::new_paragraph(&deck, list, end, end).unwrap();
    let at = history.apply(&mut deck, enter).caret().unwrap();
    assert_eq!(marks(&deck)[1], None);
    type_text(&mut deck, &mut history, list, at, "Inserted");
    let now = marks(&deck);
    assert_eq!(&now[..5], [s("1."), s("2."), s("1."), s("1."), s("3.")]);
    let inserted = &body(&deck, list).paragraphs[1];
    assert_eq!(inserted.level, 0);
    assert!(inserted.runs[0].same_look(&body(&deck, list).paragraphs[0].runs[0]));

    while history.undo(&mut deck).is_some() {}
    assert_eq!(marks(&deck), first);
}
