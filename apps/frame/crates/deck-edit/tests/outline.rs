//! FRAME-13, without the window: the outline is the slides' titles and body paragraphs;
//! typing an outline makes Title and Content slides, Enter after a title starts a slide,
//! Tab and Shift+Tab change levels and turn titles into bullets and back, other text is
//! never touched, and every outline edit is one step of the deck's undo history.

use deck_edit::{
    erase, key, outline, typing, Edit, History, LineKind, Outline, OutlineKey, OutlinePos,
};
use presentation_model::{Layer, Presentation, ShapeKind};
use std::path::Path;

fn read(name: &str) -> Presentation {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../pptx/tests/data")
        .join(name);
    pptx::read(&path).unwrap()
}

/// The outline as text: a title per slide, bullets indented two spaces a level and
/// marked `-`, counts in brackets.
fn shown(o: &Outline) -> Vec<String> {
    o.lines
        .iter()
        .map(|l| match l.kind {
            LineKind::Title => format!("{}: {}", l.slide + 1, l.text),
            LineKind::Bullet { level } => {
                format!("{}- {}", "  ".repeat(level as usize + 1), l.text)
            }
            LineKind::Others { count } => format!("  [{count} other]"),
        })
        .collect()
}

/// An outline session on a deck: edits through the outline into one history.
struct Session {
    deck: Presentation,
    history: History,
    caret: OutlinePos,
}

impl Session {
    fn new(deck: Presentation) -> Session {
        Session {
            deck,
            history: History::new(),
            caret: OutlinePos::default(),
        }
    }

    fn outline(&self) -> Outline {
        outline(&self.deck)
    }

    fn at(&mut self, line: usize, offset: usize) -> &mut Session {
        self.caret = OutlinePos::new(line, offset);
        self.history.seal();
        self
    }

    /// The caret at the end of the line whose text is `text`.
    fn at_end_of(&mut self, text: &str) -> &mut Session {
        let o = self.outline();
        let line = o.lines.iter().position(|l| l.text == text).unwrap();
        self.at(line, text.len())
    }

    fn apply(&mut self, done: Option<(Edit, OutlinePos)>) -> bool {
        match done {
            Some((edit, caret)) => {
                self.history.apply(&mut self.deck, edit);
                self.caret = caret;
                true
            }
            None => false,
        }
    }

    fn type_text(&mut self, text: &str) -> &mut Session {
        let o = self.outline();
        let done = typing(&self.deck, &o, self.caret, self.caret, text);
        assert!(self.apply(done), "typing {text:?} at {:?}", self.caret);
        self
    }

    fn press(&mut self, k: OutlineKey) -> bool {
        let o = self.outline();
        let done = key(&self.deck, &o, self.caret, k);
        self.apply(done)
    }

    fn undo(&mut self) {
        self.history.undo(&mut self.deck);
    }
}

fn layout_of(deck: &Presentation, slide: usize) -> &str {
    &deck.layouts[deck.slides[slide].layout.unwrap()].name
}

#[test]
fn the_outline_is_titles_bullets_and_counts() {
    let deck = read("layouts.pptx");
    let o = shown(&outline(&deck));
    assert_eq!(o[..2], ["1: Centered Title", "  - subtitle"]);
    assert_eq!(o[2..5], ["2: Title", "  - Content", "    - Level 2"]);
    let fixture = read("fixture.pptx");
    let o = shown(&outline(&fixture));
    let shapes = o.iter().position(|l| l == "3: Shapes").unwrap();
    assert_eq!(o[shapes + 1], "  [8 other]", "{o:#?}");
}

#[test]
fn typing_an_outline_makes_its_slides() {
    let mut s = Session::new(read("layouts.pptx"));
    let before = s.deck.clone();
    let n = s.deck.slides.len();
    // After slide 1's subtitle: a new slide's title, then bullets by Enter and Tab.
    s.at_end_of("Centered Title");
    assert!(s.press(OutlineKey::Enter));
    assert_eq!(s.deck.slides.len(), n + 1);
    assert_eq!(layout_of(&s.deck, 1), "Title and Content");
    s.type_text("Highlights");
    assert!(s.press(OutlineKey::Enter), "another slide");
    s.type_text("Growth");
    assert!(s.press(OutlineKey::Tab), "a bullet of the slide before");
    assert!(s.press(OutlineKey::Enter));
    s.type_text("Margins");
    assert!(s.press(OutlineKey::Enter));
    assert!(s.press(OutlineKey::Tab), "a second-level bullet");
    s.type_text("Up 3%");
    let o = shown(&s.outline());
    let at = o.iter().position(|l| l == "2: Highlights").unwrap();
    assert_eq!(
        o[at..at + 4],
        ["2: Highlights", "  - Growth", "  - Margins", "    - Up 3%"]
    );
    assert_eq!(s.deck.slides.len(), n + 1, "the slide Tab took is gone");
    // The bullets take the body's looks at their levels.
    let body = deck_edit::body_of(&s.deck.slides[1]).unwrap();
    let ps = &s.deck.slides[1].shapes[body]
        .text
        .as_ref()
        .unwrap()
        .paragraphs;
    assert_eq!((ps[0].runs[0].size, ps[2].runs[0].size), (32.0, 28.0));
    // Shift+Tab: back up a level, then a slide of its own.
    s.at_end_of("Up 3%");
    assert!(s.press(OutlineKey::ShiftTab));
    assert!(s.press(OutlineKey::ShiftTab));
    let o = shown(&s.outline());
    assert!(o.contains(&"3: Up 3%".to_owned()), "{o:#?}");
    assert_eq!(layout_of(&s.deck, 2), "Title and Content");
    // Every step undoes, back to the deck as read.
    for _ in 0..20 {
        s.undo();
    }
    assert_eq!(s.deck, before);
}

#[test]
fn ctrl_enter_starts_an_empty_slide_after_the_carets() {
    let mut s = Session::new(read("layouts.pptx"));
    let before = s.deck.clone();
    let n = s.deck.slides.len();
    // In the middle of a bullet: the new slide comes after its slide; no text moves.
    s.at_end_of("Content");
    s.caret.offset = 3;
    assert!(s.press(OutlineKey::NewSlide));
    assert_eq!(s.deck.slides.len(), n + 1);
    assert_eq!(layout_of(&s.deck, 2), "Title and Content");
    let o = shown(&s.outline());
    let at = o.iter().position(|l| l == "2: Title").unwrap();
    assert_eq!(o[at + 1..at + 3], ["  - Content", "    - Level 2"]);
    assert_eq!(o[s.caret.line], "3: ", "the caret in the new, empty title");
    s.type_text("Next steps");
    assert!(shown(&s.outline()).contains(&"3: Next steps".to_owned()));
    // From a title slide's subtitle: Title and Content, not another title slide.
    s.at_end_of("subtitle");
    assert!(s.press(OutlineKey::NewSlide));
    assert_eq!(layout_of(&s.deck, 1), "Title and Content");
    s.undo();
    s.undo();
    s.undo();
    assert_eq!(s.deck, before, "each a step of its own");
}

#[test]
fn shift_tab_on_a_bullet_takes_the_bullets_after_it_along() {
    let mut s = Session::new(read("layouts.pptx"));
    // Slide 2's body: Content, Level 2, Level 3, ... Level 9.
    let o = s.outline();
    let content = o.lines.iter().position(|l| l.text == "Content").unwrap();
    s.at(content, 0);
    // Content is the first bullet: a new slide titled Content, with the rest below it.
    assert!(s.press(OutlineKey::ShiftTab));
    let o = shown(&s.outline());
    assert_eq!(o[2], "2: Title");
    assert_eq!(o[3..5], ["3: Content", "    - Level 2"], "{o:#?}");
    // Slide 2 keeps its title and an empty body.
    let body = deck_edit::body_of(&s.deck.slides[1]).unwrap();
    assert!(s.deck.slides[1].shapes[body]
        .text
        .as_ref()
        .unwrap()
        .is_empty());
    s.undo();
    assert_eq!(s.deck, read("layouts.pptx"));
}

#[test]
fn other_text_is_never_touched() {
    let mut s = Session::new(read("fixture.pptx"));
    let o = s.outline();
    let shapes = o.title_line(2).unwrap();
    let before: Vec<_> = s.deck.slides[2]
        .shapes
        .iter()
        .filter(|sh| sh.layer == Layer::Slide && sh.placeholder.is_none())
        .cloned()
        .collect();
    // A slide holding other items doesn't join the one before.
    s.at(shapes, 0);
    assert!(!s.press(OutlineKey::Tab));
    // Typing into its title and Enter (a new slide after it) leave its other shapes be.
    s.at(shapes, "Shapes".len());
    s.type_text("!");
    assert!(s.press(OutlineKey::Enter));
    s.type_text("Next");
    let after: Vec<_> = s.deck.slides[2]
        .shapes
        .iter()
        .filter(|sh| sh.layer == Layer::Slide && sh.placeholder.is_none())
        .cloned()
        .collect();
    assert_eq!(after, before);
    assert!(s.deck.slides[3]
        .shapes
        .iter()
        .all(|sh| !matches!(sh.kind, ShapeKind::Picture(_))));
    // Count lines aren't editable.
    let o = s.outline();
    let count = o
        .lines
        .iter()
        .position(|l| matches!(l.kind, LineKind::Others { .. }))
        .unwrap();
    let at = OutlinePos::new(count, 0);
    assert!(typing(&s.deck, &o, at, at, "x").is_none());
}

#[test]
fn backspace_joins_bullets_and_drops_empty_slides() {
    let mut s = Session::new(read("layouts.pptx"));
    let before = s.deck.clone();
    let o = s.outline();
    let level2 = o.lines.iter().position(|l| l.text == "Level 2").unwrap();
    s.at(level2, 0);
    assert!(s.press(OutlineKey::Backspace));
    assert!(shown(&s.outline()).contains(&"  - ContentLevel 2".to_owned()));
    // An empty slide made by Enter goes with Backspace.
    let n = s.deck.slides.len();
    s.at_end_of("Centered Title");
    assert!(s.press(OutlineKey::Enter));
    assert_eq!(s.deck.slides.len(), n + 1);
    assert!(s.press(OutlineKey::Backspace));
    assert_eq!(s.deck.slides.len(), n);
    // A selection within one body deletes; across a title and its bullets it doesn't.
    let o = s.outline();
    let title = o.title_line(1).unwrap();
    let from = OutlinePos::new(title, 0);
    let to = OutlinePos::new(title + 1, 2);
    assert!(erase(&s.deck, &o, from, to).is_none());
    s.undo();
    s.undo();
    s.undo();
    assert_eq!(s.deck, before);
}
