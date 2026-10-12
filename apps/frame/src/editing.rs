//! Editing slide text on the canvas (FRAME-6, ADR 0006). A click in a text box or a
//! placeholder puts the caret there (Shift+click selects to it); then typing, Backspace,
//! Delete, Enter (a new paragraph with the same looks), Shift+Enter (a line break), the
//! arrows, Home and End (Ctrl for the whole text's), Shift with any of them to select,
//! Ctrl+A to select all, and Escape to stop. Page Up and Page Down stop and change slides.
//!
//! Every change is a `deck-edit` command in the window's one undo history: Ctrl+Z undoes,
//! Ctrl+Shift+Z and Ctrl+Y redo, a word of typing at a time.

use crate::slide_view::Editing;
use crate::window::DeckWindow;
use deck_edit::{Change, ShapeRef, TextEdit, TextPos};
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
    let click = gtk::GestureClick::new();
    click.connect_pressed({
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
fn show(view: &DeckWindow, editing: Option<Editing>) {
    view.canvas.set_editing(editing);
    if let Some(area) = view.canvas.caret_area() {
        view.im.set_cursor_location(&area);
    }
}

fn clicked(view: &DeckWindow, x: f64, y: f64, shift: bool) {
    view.canvas.grab_focus();
    view.im.reset();
    view.history.borrow_mut().seal();
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
    let Some(edit) = make(&view.deck.presentation(), shape, from, to) else {
        return;
    };
    let change = view
        .history
        .borrow_mut()
        .apply(&mut view.deck.presentation_mut(), edit);
    changed(view, change, true);
}

/// Show what `change` changed: its slide, redrawn with its thumbnail and accessible
/// description, and, when `caret`, the caret where it leaves the text.
fn changed(view: &DeckWindow, change: Change, caret: bool) {
    let slide = change.shape.slide;
    if view.canvas.index() != slide {
        view.go(slide);
    }
    view.describe(slide);
    if let Some(thumb) = view.thumbs.get(slide) {
        thumb.queue_draw();
    }
    show(
        view,
        caret.then(|| Editing::at(change.shape.shape, change.caret)),
    );
}

/// Ctrl+Z: undo the last step, showing where it was.
pub fn undo(view: &DeckWindow) {
    let editing = view.canvas.editing().is_some();
    let change = view
        .history
        .borrow_mut()
        .undo(&mut view.deck.presentation_mut());
    if let Some(change) = change {
        changed(view, change, editing);
    }
}

/// Ctrl+Shift+Z, Ctrl+Y: redo the last step undone.
pub fn redo(view: &DeckWindow) {
    let editing = view.canvas.editing().is_some();
    let change = view
        .history
        .borrow_mut()
        .redo(&mut view.deck.presentation_mut());
    if let Some(change) = change {
        changed(view, change, editing);
    }
}

/// Text from the input method or a key: one line, no control characters.
fn type_text(view: &DeckWindow, text: &str) {
    let text: String = text.chars().filter(|c| !c.is_control()).collect();
    if !text.is_empty() {
        edit(view, |deck, s, from, to| {
            TextEdit::typing(deck, s, from, to, &text)
        });
    }
}

fn key_pressed(view: &DeckWindow, key: gdk::Key, mods: gdk::ModifierType) -> glib::Propagation {
    use gdk::Key;
    let Some(e) = view.canvas.editing() else {
        return glib::Propagation::Proceed;
    };
    let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
    let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
    let alt = mods.contains(gdk::ModifierType::ALT_MASK);
    match key {
        Key::Escape => {
            view.history.borrow_mut().seal();
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
            view.history.borrow_mut().seal();
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
            view.history.borrow_mut().seal();
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
    view.history.borrow_mut().seal();
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
