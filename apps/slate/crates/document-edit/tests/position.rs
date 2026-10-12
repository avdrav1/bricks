//! `History::position` names the document the history is at, so a window knows whether it
//! differs from the file it last saved: undo back to a position gives its id again, and any
//! new edit gives a new one, typing joined to the step before included.

use document_edit::{look, normalize, replace, History, Pos, HISTORY_STEPS};
use document_model::{read_markdown, Document};

fn doc() -> Document {
    let mut doc = read_markdown(b"Hello\n");
    normalize(&mut doc);
    doc
}

/// Type `s` at the end of the first paragraph.
fn type_at_end(history: &mut History, doc: &mut Document, s: &str) {
    let end = document_edit::text(doc, document_edit::Container::Top, 0)
        .unwrap()
        .len();
    let at = Pos::new(0, end);
    let edit = replace(doc, at, at, s, &look(doc, at)).unwrap();
    history.apply(doc, edit);
}

#[test]
fn undo_and_redo_come_back_to_the_same_positions() {
    let (mut doc, mut history) = (doc(), History::new());
    let opened = history.position();
    type_at_end(&mut history, &mut doc, " world");
    let typed = history.position();
    assert_ne!(typed, opened);
    history.undo(&mut doc);
    assert_eq!(history.position(), opened);
    history.redo(&mut doc);
    assert_eq!(history.position(), typed);
}

#[test]
fn typing_joined_to_a_step_is_a_new_position() {
    let (mut doc, mut history) = (doc(), History::new());
    type_at_end(&mut history, &mut doc, "a");
    let saved = history.position();
    // Typing straight on joins the same undo step, yet the document is another one.
    type_at_end(&mut history, &mut doc, "b");
    assert_ne!(history.position(), saved);
    history.undo(&mut doc);
    assert_eq!(
        document_edit::text(&doc, document_edit::Container::Top, 0).unwrap(),
        "Hello"
    );
    assert_ne!(history.position(), saved);
}

#[test]
fn undoing_everything_past_the_history_limit_is_not_the_opened_document() {
    let (mut doc, mut history) = (doc(), History::new());
    let opened = history.position();
    for _ in 0..HISTORY_STEPS + 5 {
        history.seal();
        type_at_end(&mut history, &mut doc, "x");
    }
    while history.can_undo() {
        history.undo(&mut doc);
    }
    assert_ne!(history.position(), opened);
}
