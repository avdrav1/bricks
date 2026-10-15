//! Editing slide text on the canvas (FRAME-6, ADR 0006). A click in a text box or a
//! placeholder puts the caret there (Shift+click selects to it); a click on a shape
//! without text selects it ([`crate::arrange`] moves it), and typing into a selected
//! shape with a text body starts at its end. Then typing, Backspace, Delete, Enter (a new
//! paragraph with the same looks), Shift+Enter (a line break), the arrows, Home and End
//! (Ctrl for the whole text's), Shift with any of them to select, Ctrl+A to select all,
//! and Escape to stop. Page Up and Page Down stop and change slides. On macOS, as its
//! text has them (MAC-3, [`crate::keys`]): ⌥←/⌥→ go a word, ⌘←/⌘→ to the line's start
//! and end, ⌘↑/⌘↓ to the text's; ⌥⌫ and ⌥⌦ delete a word, ⌘⌫ to the line's start.
//!
//! The keyboard flow (FRAME-15), as PowerPoint's: a new deck and a slide New Slide adds
//! start with the caret in the title ([`edit_title`]). Ctrl+Enter puts the caret at the
//! end of the slide's next text placeholder ([`deck_edit::next_placeholder`]; with
//! nothing edited, the first), and after the last adds a slide as New Slide does. Tab and
//! Shift+Tab at the start of a paragraph, or over several, move the paragraphs a level
//! down or up ([`deck_edit::relevel`]); otherwise they do nothing, and while text is
//! edited they never take the focus off the canvas.
//!
//! Every change is a `deck-edit` command in the window's one undo history: Ctrl+Z undoes,
//! Ctrl+Shift+Z and Ctrl+Y redo, a word of typing at a time.

use crate::keys::{self, Action, Caret, Erase, Via};
use crate::slide_list;
use crate::slide_view::Editing;
use crate::window::DeckWindow;
use deck_edit::{ShapeRef, TextEdit, TextPos};
use gtk::prelude::*;
use gtk::{gdk, glib};
use presentation_model::{Presentation, TextBody};
use std::rc::Rc;
use suite_ui::keys::Primary;

/// Make `view`'s canvas edit text: clicks, keys, and the input method.
pub fn attach(view: &Rc<DeckWindow>) {
    let canvas = &view.canvas;
    canvas.set_editable(true);
    canvas.set_focusable(true);
    let im = &view.im;
    suite_ui::im::attach(im, canvas);
    im.connect_commit({
        let view = Rc::downgrade(view);
        move |_, text| {
            if let Some(view) = view.upgrade() {
                type_text(&view, text);
            }
        }
    });
    let keys = gtk::EventControllerKey::new();
    keys.set_im_context(Some(im));
    keys.connect_key_pressed({
        let view = Rc::downgrade(view);
        move |_, key, _, mods| match view.upgrade() {
            Some(view) => key_pressed(&view, key, mods),
            None => glib::Propagation::Proceed,
        }
    });
    canvas.add_controller(keys);
    let focus = gtk::EventControllerFocus::new();
    focus.connect_enter({
        let im = im.clone();
        move |_| im.focus_in()
    });
    focus.connect_leave({
        let im = im.clone();
        move |_| im.focus_out()
    });
    canvas.add_controller(focus);
    // On release: a press that becomes a drag (crate::arrange) claims it, and no caret
    // moves.
    let click = gtk::GestureClick::new();
    click.set_button(gdk::BUTTON_PRIMARY);
    click.connect_released({
        let view = Rc::downgrade(view);
        move |gesture, _, x, y| {
            if let Some(view) = view.upgrade() {
                let shift = gesture
                    .current_event_state()
                    .contains(gdk::ModifierType::SHIFT_MASK);
                clicked(&view, x, y, shift);
            }
        }
    });
    canvas.add_controller(click);
}

/// Show `editing` on the canvas (`None`: not editing), with the input method's caret.
pub(crate) fn show(view: &DeckWindow, editing: Option<Editing>) {
    view.canvas.set_editing(editing);
    if let Some(area) = view.canvas.caret_area() {
        view.im.set_cursor_location(&area);
    }
}

/// Edit shape `shape` of the slide shown as a click in it would: the canvas focused (and
/// again once the event is handled: a popover that closes gives the focus to its button
/// first) and the caret at the end of its text.
pub(crate) fn start(view: &DeckWindow, shape: usize) {
    view.canvas.grab_focus();
    view.im.reset();
    view.seal();
    show(view, Some(Editing::at(shape, text_end(view, shape))));
    let canvas = view.canvas.clone();
    glib::idle_add_local_once(move || {
        if canvas.editing().is_some_and(|e| e.shape == shape) {
            canvas.grab_focus();
        }
    });
}

/// A new deck's and a new slide's first stop: the caret in the title placeholder of the
/// slide shown, when it has one, unless the outline is shown.
pub(crate) fn edit_title(view: &DeckWindow) {
    if view.outline.is_shown() {
        return;
    }
    let title = view
        .deck
        .presentation()
        .slides
        .get(view.canvas.index())
        .and_then(deck_edit::title_of);
    if let Some(title) = title {
        start(view, title);
    }
}

/// Ctrl+Enter: the caret at the end of the slide's next text placeholder after the shape
/// edited (none edited: the first), or, after the last, a new slide as New Slide adds.
fn next_placeholder(view: &DeckWindow) {
    let after = view.canvas.editing().map(|e| e.shape);
    let next = view
        .deck
        .presentation()
        .slides
        .get(view.canvas.index())
        .and_then(|s| deck_edit::next_placeholder(s, after));
    match next {
        Some(shape) => start(view, shape),
        None => {
            view.seal();
            slide_list::new_slide(view);
        }
    }
}

/// Tab (`down`) or Shift+Tab while `e` is edited: its paragraphs a level down or up, as
/// one step, the selection kept.
fn relevel(view: &DeckWindow, e: Editing, down: bool) {
    let shape = ShapeRef {
        slide: view.canvas.index(),
        shape: e.shape,
    };
    let (from, to) = (e.anchor, e.caret);
    let edit = deck_edit::relevel(&view.deck.presentation(), shape, from, to, down);
    if let Some(edit) = edit {
        view.seal();
        view.apply(edit);
        view.seal();
        show(view, Some(e));
    }
}

/// A click at (`x`, `y`): the caret into text there, a shape without text selected, or,
/// on the slide's empty space, nothing selected.
fn clicked(view: &DeckWindow, x: f64, y: f64, shift: bool) {
    view.canvas.grab_focus();
    view.im.reset();
    view.seal();
    let editing = match (view.canvas.text_at(x, y), view.canvas.editing()) {
        (Some((shape, pos)), Some(e)) if shift && e.shape == shape => Some(Editing {
            caret: pos,
            goal_x: None,
            ..e
        }),
        (Some((shape, pos)), _) => Some(Editing::at(shape, pos)),
        (None, _) => None,
    };
    show(view, editing);
    if editing.is_none() {
        let shape = view.canvas.shape_at(x, y, 0.0).filter(|&i| {
            let deck = view.deck.presentation();
            deck.slides
                .get(view.canvas.index())
                .and_then(|s| s.shapes.get(i))
                .is_some_and(deck_edit::movable)
        });
        view.canvas.select(shape);
    }
}

/// Apply the edit `make` builds for the selection of the text being edited.
fn edit(
    view: &DeckWindow,
    make: impl FnOnce(&Presentation, ShapeRef, TextPos, TextPos) -> Option<TextEdit>,
) {
    let Some(e) = view.canvas.editing() else {
        return;
    };
    let shape = ShapeRef {
        slide: view.canvas.index(),
        shape: e.shape,
    };
    let (from, to) = e.range();
    // Built before applying: the deck is borrowed to build it.
    let edit = make(&view.deck.presentation(), shape, from, to);
    if let Some(edit) = edit {
        view.apply(edit);
    }
}

/// Text from the input method or a key: one line, no control characters. A shape
/// selected but not edited takes it at the end of its text.
fn type_text(view: &DeckWindow, text: &str) {
    let text: String = text.chars().filter(|c| !c.is_control()).collect();
    if text.is_empty() {
        return;
    }
    if view.canvas.editing().is_none() {
        let Some(shape) = view.canvas.selected().filter(|&s| has_text(view, s)) else {
            return;
        };
        view.seal();
        show(view, Some(Editing::at(shape, text_end(view, shape))));
    }
    edit(view, |deck, s, from, to| {
        TextEdit::typing(deck, s, from, to, &text)
    });
}

/// Whether shape `shape` of the slide shown has a text body a save can write.
fn has_text(view: &DeckWindow, shape: usize) -> bool {
    let deck = view.deck.presentation();
    deck.slides
        .get(view.canvas.index())
        .and_then(|s| s.shapes.get(shape))
        .is_some_and(|s| s.text.is_some() && s.id.is_some())
}

/// The canvas's keys: Ctrl+Enter whether or not text is edited, the rest only while it is.
fn key_pressed(view: &DeckWindow, key: gdk::Key, mods: gdk::ModifierType) -> glib::Propagation {
    use gdk::Key;
    let p = Primary::HERE;
    let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
    let action = keys::find(p, Via::Canvas, key, mods);
    if action == Some(Action::NextPlaceholder) {
        next_placeholder(view);
        return glib::Propagation::Stop;
    }
    let Some(e) = view.canvas.editing() else {
        return glib::Propagation::Proceed;
    };
    let modified = keys::modified(p, mods);
    if let Some(how) = keys::erase(p, key, mods) {
        erase(view, e, how);
        return glib::Propagation::Stop;
    }
    if let Some(to) = keys::caret(p, key, mods) {
        move_caret(view, e, to, shift);
        return glib::Propagation::Stop;
    }
    match key {
        Key::Escape => {
            view.seal();
            show(view, None);
        }
        // Tab and Shift+Tab are the text's: a level down or up, else nothing.
        Key::Tab | Key::KP_Tab | Key::ISO_Left_Tab if !modified => {
            relevel(view, e, !shift && key != Key::ISO_Left_Tab);
        }
        Key::Return | Key::KP_Enter | Key::ISO_Enter => edit(view, |deck, s, from, to| {
            if shift {
                TextEdit::line_break(deck, s, from, to)
            } else {
                TextEdit::new_paragraph(deck, s, from, to)
            }
        }),
        _ if action == Some(Action::SelectAll) => {
            let end = text_end(view, e.shape);
            view.seal();
            show(
                view,
                Some(Editing {
                    anchor: TextPos::default(),
                    caret: end,
                    goal_x: None,
                    ..e
                }),
            );
        }
        Key::Page_Up | Key::KP_Page_Up | Key::Page_Down | Key::KP_Page_Down => {
            view.seal();
            show(view, None);
            let down = matches!(key, Key::Page_Down | Key::KP_Page_Down);
            view.step(if down { 1 } else { -1 });
        }
        _ if modified => return glib::Propagation::Proceed,
        // A key the input method left alone that still types a character.
        _ => match key.to_unicode().filter(|c| !c.is_control()) {
            Some(c) => type_text(view, c.encode_utf8(&mut [0; 4])),
            None => return glib::Propagation::Proceed,
        },
    }
    glib::Propagation::Stop
}

/// Backspace or Delete while `e` is edited: the selection, else what `how` reaches from
/// the caret.
fn erase(view: &DeckWindow, e: Editing, how: Erase) {
    // Where a word or the line's start is: it takes the text as laid out.
    let reach = match how {
        Erase::Char { .. } => None,
        Erase::Word { forward } => word(view, e, forward),
        Erase::LineStart => view
            .canvas
            .placed_text(e.shape)
            .map(|placed| placed.line_ends(e.caret).0)
            .filter(|&start| start != e.caret),
    };
    let forward = matches!(
        how,
        Erase::Char { forward: true } | Erase::Word { forward: true }
    );
    edit(view, |deck, s, from, to| {
        if from != to {
            return TextEdit::delete(deck, s, from, to);
        }
        let body = deck.slides[s.slide].shapes[s.shape].text.as_ref()?;
        match (reach, forward) {
            (Some(end), true) => TextEdit::delete(deck, s, from, end),
            (Some(start), false) => TextEdit::delete(deck, s, start, from),
            (None, true) => TextEdit::delete(deck, s, from, deck_edit::after(body, from)?),
            (None, false) => TextEdit::delete(deck, s, deck_edit::before(body, from)?, from),
        }
    });
}

/// `f` of the text body of shape `shape` of the slide shown.
fn text_body<T>(view: &DeckWindow, shape: usize, f: impl FnOnce(&TextBody) -> T) -> Option<T> {
    let deck = view.deck.presentation();
    let body = deck
        .slides
        .get(view.canvas.index())?
        .shapes
        .get(shape)?
        .text
        .as_ref()?;
    Some(f(body))
}

/// The start of the word before `e`'s caret, or the end of the word after it.
fn word(view: &DeckWindow, e: Editing, forward: bool) -> Option<TextPos> {
    text_body(view, e.shape, |body| {
        let texts: Vec<String> = body.paragraphs.iter().map(|p| p.text()).collect();
        word_edge(&texts, e.caret, forward)
    })
    .flatten()
}

/// Where ⌥← (`forward` false) or ⌥→ goes from `pos` in the paragraphs `texts`: the start
/// of the word before it or the end of the word after it, past the space and the
/// paragraph ends between; at the text's start or end when there's no word. A word is
/// letters, digits, and `_`. `None` when that's `pos`.
fn word_edge(texts: &[String], pos: TextPos, forward: bool) -> Option<TextPos> {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut at = pos;
    let mut in_word = false;
    while let Some(text) = texts.get(at.paragraph) {
        let c = if forward {
            text.get(at.offset..).and_then(|t| t.chars().next())
        } else {
            text.get(..at.offset).and_then(|t| t.chars().next_back())
        };
        at = match c {
            Some(c) => {
                let word = is_word(c);
                if in_word && !word {
                    break;
                }
                in_word |= word;
                let n = c.len_utf8();
                let offset = if forward {
                    at.offset + n
                } else {
                    at.offset - n
                };
                TextPos::new(at.paragraph, offset)
            }
            // A paragraph's end ends a word, else the next paragraph's text follows.
            None if in_word => break,
            None if forward && at.paragraph + 1 < texts.len() => TextPos::new(at.paragraph + 1, 0),
            None if !forward && at.paragraph > 0 => {
                TextPos::new(at.paragraph - 1, texts[at.paragraph - 1].len())
            }
            None => break,
        };
    }
    (at != pos).then_some(at)
}

fn text_end(view: &DeckWindow, shape: usize) -> TextPos {
    text_body(view, shape, deck_edit::text_end).unwrap_or_default()
}

/// Move the caret `to` a place; with Shift, the selection's far end.
fn move_caret(view: &DeckWindow, e: Editing, to: Caret, shift: bool) {
    let Some(placed) = view.canvas.placed_text(e.shape) else {
        return;
    };
    let (start, end) = e.range();
    let mut goal_x = None;
    let pos = match to {
        Caret::Char { forward: false } if !shift && start != end => start,
        Caret::Char { forward: true } if !shift && start != end => end,
        Caret::Char { forward } => placed.step(e.caret, forward).unwrap_or(e.caret),
        Caret::Word { forward } => word(view, e, forward).unwrap_or(e.caret),
        Caret::Vertical { up } => {
            let x = e.goal_x.unwrap_or_else(|| placed.caret(e.caret).x());
            goal_x = Some(x);
            match placed.vertical(e.caret, up, x) {
                Some(pos) => pos,
                // Past the first or last line: to the start or the end.
                None if up => TextPos::default(),
                None => text_end(view, e.shape),
            }
        }
        Caret::Text { end: false } => TextPos::default(),
        Caret::Text { end: true } => text_end(view, e.shape),
        Caret::Line { end: false } => placed.line_ends(e.caret).0,
        Caret::Line { end: true } => placed.line_ends(e.caret).1,
    };
    view.seal();
    show(
        view,
        Some(Editing {
            shape: e.shape,
            anchor: if shift { e.anchor } else { pos },
            caret: pos,
            goal_x,
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(paras: &[&str]) -> Vec<String> {
        paras.iter().map(|p| (*p).to_owned()).collect()
    }

    #[test]
    fn option_arrows_go_a_word() {
        let t = texts(&["Hello, wide world", "  next_one é"]);
        let at = |p, o| TextPos::new(p, o);
        // ⌥→: the end of the word after, past spaces and punctuation.
        assert_eq!(word_edge(&t, at(0, 0), true), Some(at(0, 5)));
        assert_eq!(word_edge(&t, at(0, 5), true), Some(at(0, 11)));
        assert_eq!(word_edge(&t, at(0, 8), true), Some(at(0, 11)));
        // Past the paragraph's end into the next one's first word.
        assert_eq!(word_edge(&t, at(0, 17), true), Some(at(1, 10)));
        assert_eq!(word_edge(&t, at(1, 10), true), Some(at(1, 13)));
        assert_eq!(word_edge(&t, at(1, 13), true), None);
        // ⌥←: the start of the word before.
        assert_eq!(word_edge(&t, at(0, 17), false), Some(at(0, 12)));
        assert_eq!(word_edge(&t, at(0, 14), false), Some(at(0, 12)));
        assert_eq!(word_edge(&t, at(1, 2), false), Some(at(0, 12)));
        assert_eq!(word_edge(&t, at(0, 3), false), Some(at(0, 0)));
        assert_eq!(word_edge(&t, at(0, 0), false), None);
        // No word left: the text's end.
        let t = texts(&["word", " .. "]);
        assert_eq!(word_edge(&t, at(0, 4), true), Some(at(1, 4)));
    }
}
