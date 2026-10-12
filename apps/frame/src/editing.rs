//! Editing slide text on the canvas (FRAME-6, ADR 0006). A click in a text box or a
//! placeholder puts the caret there (Shift+click selects to it); a click on a shape
//! without text selects it ([`crate::arrange`] moves it), and typing into a selected
//! shape with a text body starts at its end. Then typing, Backspace, Delete, Enter (a new
//! paragraph with the same looks), Shift+Enter (a line break), the arrows, Home and End
//! (Ctrl for the whole text's), Shift with any of them to select, Ctrl+A to select all,
//! and Escape to stop. Page Up and Page Down stop and change slides.
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

use crate::slide_list;
use crate::slide_view::Editing;
use crate::window::DeckWindow;
use deck_edit::{ShapeRef, TextEdit, TextPos};
use gtk::prelude::*;
use gtk::{gdk, glib};
use presentation_model::Presentation;
use std::rc::Rc;

/// Make `view`'s canvas edit text: clicks, keys, and the input method.
pub fn attach(view: &Rc<DeckWindow>) {
    let canvas = &view.canvas;
    canvas.set_editable(true);
    canvas.set_focusable(true);
    let im = &view.im;
    im.set_client_widget(Some(canvas));
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
    let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
    let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
    let alt = mods.contains(gdk::ModifierType::ALT_MASK);
    let enter = matches!(key, Key::Return | Key::KP_Enter | Key::ISO_Enter);
    if enter && ctrl && !alt {
        next_placeholder(view);
        return glib::Propagation::Stop;
    }
    let Some(e) = view.canvas.editing() else {
        return glib::Propagation::Proceed;
    };
    match key {
        Key::Escape => {
            view.seal();
            show(view, None);
        }
        Key::BackSpace | Key::Delete | Key::KP_Delete => {
            let forward = key != Key::BackSpace;
            edit(view, |deck, s, from, to| {
                if from != to {
                    return TextEdit::delete(deck, s, from, to);
                }
                let body = deck.slides[s.slide].shapes[s.shape].text.as_ref()?;
                if forward {
                    TextEdit::delete(deck, s, from, deck_edit::after(body, from)?)
                } else {
                    TextEdit::delete(deck, s, deck_edit::before(body, from)?, from)
                }
            });
        }
        // Tab and Shift+Tab are the text's: a level down or up, else nothing.
        Key::Tab | Key::KP_Tab | Key::ISO_Left_Tab if !ctrl && !alt => {
            relevel(view, e, !shift && key != Key::ISO_Left_Tab);
        }
        Key::Return | Key::KP_Enter | Key::ISO_Enter => edit(view, |deck, s, from, to| {
            if shift {
                TextEdit::line_break(deck, s, from, to)
            } else {
                TextEdit::new_paragraph(deck, s, from, to)
            }
        }),
        Key::Left
        | Key::KP_Left
        | Key::Right
        | Key::KP_Right
        | Key::Up
        | Key::KP_Up
        | Key::Down
        | Key::KP_Down
        | Key::Home
        | Key::KP_Home
        | Key::End
        | Key::KP_End => move_caret(view, e, key, shift, ctrl),
        Key::a | Key::A if ctrl && !alt => {
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
        _ if ctrl || alt => return glib::Propagation::Proceed,
        // A key the input method left alone that still types a character.
        _ => match key.to_unicode().filter(|c| !c.is_control()) {
            Some(c) => type_text(view, c.encode_utf8(&mut [0; 4])),
            None => return glib::Propagation::Proceed,
        },
    }
    glib::Propagation::Stop
}

fn text_end(view: &DeckWindow, shape: usize) -> TextPos {
    let deck = view.deck.presentation();
    deck.slides
        .get(view.canvas.index())
        .and_then(|s| s.shapes.get(shape))
        .and_then(|s| s.text.as_ref())
        .map(deck_edit::text_end)
        .unwrap_or_default()
}

/// Move the caret by `key`; with Shift, the selection's far end.
fn move_caret(view: &DeckWindow, e: Editing, key: gdk::Key, shift: bool, ctrl: bool) {
    use gdk::Key;
    let Some(placed) = view.canvas.placed_text(e.shape) else {
        return;
    };
    let (from, to) = e.range();
    let mut goal_x = None;
    let pos = match key {
        Key::Left | Key::KP_Left if !shift && from != to => from,
        Key::Right | Key::KP_Right if !shift && from != to => to,
        Key::Left | Key::KP_Left => placed.step(e.caret, false).unwrap_or(e.caret),
        Key::Right | Key::KP_Right => placed.step(e.caret, true).unwrap_or(e.caret),
        Key::Up | Key::KP_Up | Key::Down | Key::KP_Down => {
            let up = matches!(key, Key::Up | Key::KP_Up);
            let x = e.goal_x.unwrap_or_else(|| placed.caret(e.caret).x());
            goal_x = Some(x);
            match placed.vertical(e.caret, up, x) {
                Some(pos) => pos,
                // Past the first or last line: to the start or the end.
                None if up => TextPos::default(),
                None => text_end(view, e.shape),
            }
        }
        Key::Home | Key::KP_Home if ctrl => TextPos::default(),
        Key::End | Key::KP_End if ctrl => text_end(view, e.shape),
        Key::Home | Key::KP_Home => placed.line_ends(e.caret).0,
        _ => placed.line_ends(e.caret).1,
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
