//! FRAME-7 acceptance, without the window: Ctrl+M adds a slide with the current slide's
//! layout, or Title and Content after a title slide; adding, duplicating, deleting, and
//! moving slides each undo and redo; the slide to show follows each step.

use deck_edit::{next_layout, Change, History, ShapeRef, SlideEdit, TextEdit, TextPos};
use presentation_model::{Layer, PlaceholderKind, Presentation};
use std::path::Path;

fn deck(name: &str) -> Presentation {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../pptx/tests/data")
        .join(name);
    pptx::read(&path).unwrap()
}

fn layout_name(deck: &Presentation, layout: Option<usize>) -> &str {
    &deck.layouts[layout.unwrap()].name
}

#[test]
fn ctrl_m_takes_the_slides_layout_or_title_and_content_after_a_title_slide() {
    let d = deck("layouts.pptx");
    // Slide 1 is a Title Slide, slide 3 a Section Header, slide 10 Title and Content.
    assert_eq!(
        layout_name(&d, next_layout(&d, Some(0))),
        "Title and Content"
    );
    assert_eq!(layout_name(&d, next_layout(&d, Some(2))), "Section Header");
    assert_eq!(
        layout_name(&d, next_layout(&d, Some(9))),
        "Title and Content"
    );
    assert_eq!(layout_name(&d, next_layout(&d, None)), "Title and Content");

    // LibreOffice's decks call Title and Content `tx`.
    let f = deck("fixture.pptx");
    let title = f.layouts[f.slides[0].layout.unwrap()].kind.as_str();
    assert_eq!(title, "title");
    assert_eq!(f.layouts[next_layout(&f, Some(0)).unwrap()].kind, "tx");

    // With seven masters, the Title and Content of the title slide's own master.
    let t = deck("themes.pptx");
    for (i, slide) in t.slides.iter().enumerate() {
        let on = &t.layouts[slide.layout.unwrap()];
        let next = &t.layouts[next_layout(&t, Some(i)).unwrap()];
        if on.kind == "title" {
            assert_eq!(
                (next.kind.as_str(), next.master),
                ("obj", on.master),
                "slide {i}"
            );
        } else {
            assert_eq!(next, on, "slide {i}");
        }
    }
}

#[test]
fn slide_steps_undo_and_redo() {
    let mut d = deck("layouts.pptx");
    let mut history = History::new();
    let mut states = vec![d.clone()];
    let mut shows = Vec::new();

    // Ctrl+M on slide 1: a Title and Content slide after it, shown.
    let add = SlideEdit::new_slide(&d, Some(0)).unwrap();
    let change = history.apply(&mut d, add);
    assert_eq!(change, Change::Slides { show: 1 });
    assert_eq!(d.slides.len(), 11);
    assert_eq!(d.slides[1], d.layouts[1].slide);
    states.push(d.clone());
    shows.push(change.slide());
    // Typing its title, then the rest.
    let title = d.slides[1]
        .shapes
        .iter()
        .position(|s| s.layer == Layer::Slide && s.placeholder == Some(PlaceholderKind::Title))
        .unwrap();
    let at = TextPos::default();
    let s = ShapeRef {
        slide: 1,
        shape: title,
    };
    let typed = TextEdit::typing(&d, s, at, at, "Agenda").unwrap();
    shows.push(history.apply(&mut d, typed).slide());
    states.push(d.clone());
    // Each step built on the deck as the step before leaves it.
    let steps: [fn(&Presentation) -> Option<SlideEdit>; 4] = [
        |d| SlideEdit::duplicate(d, 1),
        |d| SlideEdit::delete(d, 0),
        |d| SlideEdit::move_to(d, 9, 0),
        |d| SlideEdit::on_layout(d, Some(10), 6),
    ];
    for step in steps {
        let step = step(&d).unwrap();
        shows.push(history.apply(&mut d, step).slide());
        states.push(d.clone());
    }
    assert_eq!(shows, [1, 1, 2, 0, 0, 11]);
    assert_eq!(d.slides.len(), 12);
    assert_eq!(
        d.slides[1].text(),
        "Agenda",
        "duplicate, then the first slide deleted"
    );
    assert_eq!(d.slides[11].layout, Some(6));

    let n = states.len() - 1;
    for i in (0..n).rev() {
        history.undo(&mut d).expect("a step to undo");
        assert_eq!(d, states[i], "undo to step {i}");
    }
    assert!(!history.can_undo());
    for (i, state) in states.iter().enumerate().skip(1) {
        history.redo(&mut d).expect("a step to redo");
        assert_eq!(&d, state, "redo to step {i}");
    }
}

#[test]
fn undoing_shows_the_slide_it_changes_back() {
    let mut d = deck("layouts.pptx");
    let mut history = History::new();
    let add = SlideEdit::new_slide(&d, Some(4)).unwrap();
    history.apply(&mut d, add);
    // Undoing an added slide goes back to the slide it followed.
    assert_eq!(history.undo(&mut d), Some(Change::Slides { show: 4 }));
    // Deleting the last slide shows the one before; undoing shows it again.
    let delete = SlideEdit::delete(&d, 9).unwrap();
    history.apply(&mut d, delete);
    assert_eq!(history.undo(&mut d), Some(Change::Slides { show: 9 }));
    assert_eq!(history.redo(&mut d), Some(Change::Slides { show: 8 }));
    // A move shows the slide where it lands, and undone where it was.
    let text = d.slides[2].text();
    let step = SlideEdit::move_to(&d, 2, 5).unwrap();
    history.apply(&mut d, step);
    assert_eq!(d.slides[5].text(), text);
    assert_eq!(history.undo(&mut d), Some(Change::Slides { show: 2 }));
    assert!(SlideEdit::move_to(&d, 3, 3).is_none());
}
