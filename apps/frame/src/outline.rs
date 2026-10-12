//! The outline view (FRAME-13, ADR 0006): the header bar's Outline (Ctrl+Shift+O) shows
//! every slide's title and bullets as one editable outline in place of the canvas and the
//! inspector; the slide list stays. Titles are bold, after their slide's number; bullets
//! are indented by level; a slide's other items (text outside its title and body, shapes,
//! pictures) show as a dim count that can't be edited.
//!
//! The text view never changes its buffer itself: typing, input method commits, pastes,
//! and deletions arrive as the buffer's insert-text and delete-range signals, which are
//! stopped and turned into `deck-edit`'s outline edits ([`deck_edit::typing`],
//! [`deck_edit::erase`]); Enter, Tab, Shift+Tab, Backspace, and Delete are taken before the
//! text view sees them ([`deck_edit::key`]). Each edit is applied through
//! [`DeckWindow::apply`], so the outline and the canvas share one undo history, and the
//! buffer then shows the deck again, a line per outline line, the caret where the edit
//! leaves it. Moving the caret into another slide's lines shows that slide in the slide
//! list and the status bar.

use crate::editing;
use crate::window::DeckWindow;
use deck_edit::{Change, Edit, Line, LineKind, Outline, OutlineKey, OutlinePos};
use gtk::prelude::*;
use gtk::{gdk, glib};
use presentation_model::Presentation;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The outline's left margin, where titles start, in pixels.
const MARGIN: i32 = 16;
/// How far each bullet level is indented, in pixels.
const INDENT: i32 = 28;
/// The deepest bullet level (0 the first).
const LEVELS: u8 = 8;

/// The outline view, the page beside it (the canvas and the inspector), and the header
/// bar's button that switches between them.
pub(crate) struct OutlineView {
    stack: gtk::Stack,
    text: gtk::TextView,
    button: gtk::ToggleButton,
    /// The outline the buffer shows: buffer line `n` is outline line `n`.
    shown: RefCell<Outline>,
    /// Set while the buffer is changed to show the deck, or its caret placed: those
    /// changes go through as they are.
    busy: Cell<bool>,
    /// A deletion the text view asked for, held until the current event is handled: an
    /// input method's commit and a paste delete the selection, then insert, which types
    /// over it as one edit.
    pending: Cell<Option<(OutlinePos, OutlinePos)>>,
    /// Where the caret was (buffer line and character), and whether a check that it's
    /// out of a prefix is due: the text view can leave it in one (Home, a click, Up).
    caret: Cell<(i32, i32)>,
    checking: Cell<bool>,
}

impl OutlineView {
    /// The outline and `slide`, the page it takes the place of.
    pub fn new(slide: &impl IsA<gtk::Widget>) -> OutlineView {
        let buffer = gtk::TextBuffer::new(None);
        buffer.set_enable_undo(false);
        let table = buffer.tag_table();
        let dim = gdk::RGBA::new(0.5, 0.5, 0.5, 1.0);
        table.add(
            &gtk::TextTag::builder()
                .name("title")
                .weight(700)
                .scale(1.25)
                .pixels_above_lines(12)
                .build(),
        );
        table.add(
            &gtk::TextTag::builder()
                .name("prefix")
                .foreground_rgba(&dim)
                .build(),
        );
        table.add(
            &gtk::TextTag::builder()
                .name("others")
                .foreground_rgba(&dim)
                .style(gtk::pango::Style::Italic)
                .left_margin(MARGIN + INDENT)
                .build(),
        );
        for level in 0..=LEVELS {
            table.add(
                &gtk::TextTag::builder()
                    .name(format!("bullet-{level}"))
                    .left_margin(MARGIN + INDENT * (i32::from(level) + 1))
                    .pixels_above_lines(2)
                    .build(),
            );
        }
        let text = gtk::TextView::with_buffer(&buffer);
        text.set_wrap_mode(gtk::WrapMode::WordChar);
        text.set_left_margin(MARGIN);
        text.set_right_margin(MARGIN);
        text.set_top_margin(MARGIN);
        text.set_bottom_margin(MARGIN);
        text.update_property(&[gtk::accessible::Property::Label("Outline")]);
        let scrolled = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&text)
            .hexpand(true)
            .vexpand(true)
            .build();
        let stack = gtk::Stack::new();
        stack.add_named(slide, Some("slide"));
        stack.add_named(&scrolled, Some("outline"));
        stack.set_visible_child_name("slide");
        let button = gtk::ToggleButton::with_label("Outline");
        button.set_tooltip_text(Some("Outline (Ctrl+Shift+O)"));
        button.update_property(&[gtk::accessible::Property::Label("Outline")]);
        OutlineView {
            stack,
            text,
            button,
            shown: RefCell::new(Outline::default()),
            busy: Cell::new(false),
            pending: Cell::new(None),
            caret: Cell::new((0, 0)),
            checking: Cell::new(false),
        }
    }

    /// The outline and the page it replaces, one shown at a time.
    pub fn widget(&self) -> &gtk::Stack {
        &self.stack
    }

    /// The header bar's Outline button.
    pub fn button(&self) -> &gtk::ToggleButton {
        &self.button
    }

    /// Whether the outline is shown.
    pub fn is_shown(&self) -> bool {
        self.button.is_active()
    }

    /// Ctrl+Shift+O: show the outline, or the canvas again.
    pub fn toggle(&self) {
        self.button.set_active(!self.button.is_active());
    }

    /// Whether the keyboard focus is in the outline.
    pub fn has_focus(&self) -> bool {
        self.text
            .root()
            .and_then(|r| r.focus())
            .is_some_and(|f| f == self.text)
    }

    /// Show `deck` in the buffer: only the lines that changed are replaced, so the view
    /// keeps its place.
    fn render(&self, deck: &Presentation) {
        let new = deck_edit::outline(deck);
        let mut shown = self.shown.borrow_mut();
        let old = &shown.lines;
        let lines = &new.lines;
        let head = old.iter().zip(lines).take_while(|(a, b)| a == b).count();
        if head == old.len() && head == lines.len() {
            return;
        }
        let room = old.len().min(lines.len()) - head;
        let tail = old
            .iter()
            .rev()
            .zip(lines.iter().rev())
            .take(room)
            .take_while(|(a, b)| a == b)
            .count();
        let buffer = self.text.buffer();
        self.busy.set(true);
        if tail > 0 {
            // Lines head.. up to the unchanged ones, each with its line end.
            let mut at = line_start(&buffer, head);
            let mut end = line_start(&buffer, old.len() - tail);
            buffer.delete(&mut at, &mut end);
            for line in &lines[head..lines.len() - tail] {
                insert_line(&buffer, &mut at, line);
                insert_end(&buffer, &mut at, line);
            }
        } else if head > 0 {
            // From the end of the last unchanged line to the end.
            let mut at = line_start(&buffer, head - 1);
            if !at.ends_line() {
                at.forward_to_line_end();
            }
            let mut end = buffer.end_iter();
            buffer.delete(&mut at, &mut end);
            for pair in lines[head - 1..].windows(2) {
                insert_end(&buffer, &mut at, &pair[0]);
                insert_line(&buffer, &mut at, &pair[1]);
            }
        } else {
            buffer.set_text("");
            let mut at = buffer.start_iter();
            for (i, line) in lines.iter().enumerate() {
                if i > 0 {
                    insert_end(&buffer, &mut at, &lines[i - 1]);
                }
                insert_line(&buffer, &mut at, line);
            }
        }
        *shown = new;
        self.busy.set(false);
    }

    /// The buffer position of `pos`: after its line's prefix, the offset clamped to the
    /// line's text.
    fn iter_at(&self, pos: OutlinePos) -> Option<gtk::TextIter> {
        let shown = self.shown.borrow();
        let line = shown.lines.get(pos.line)?;
        let mut offset = pos.offset.min(line.text.len());
        while !line.text.is_char_boundary(offset) {
            offset -= 1;
        }
        let chars = prefix(line).chars().count() + line.text[..offset].chars().count();
        self.text
            .buffer()
            .iter_at_line_offset(i32::try_from(pos.line).ok()?, i32::try_from(chars).ok()?)
    }

    /// The outline position of buffer position `iter`: a place in a line's prefix is its
    /// text's start.
    fn pos_at(&self, iter: &gtk::TextIter) -> OutlinePos {
        let shown = self.shown.borrow();
        let n = usize::try_from(iter.line()).unwrap_or(0);
        let Some(line) = shown.lines.get(n) else {
            return OutlinePos::default();
        };
        let chars = usize::try_from(iter.line_offset()).unwrap_or(0);
        let k = chars.saturating_sub(prefix(line).chars().count());
        let offset = line
            .text
            .char_indices()
            .nth(k)
            .map_or(line.text.len(), |(b, _)| b);
        OutlinePos::new(n, offset)
    }

    /// The caret, and the selection's ends in order (both the caret without one).
    fn selection(&self) -> (OutlinePos, OutlinePos, OutlinePos) {
        let buffer = self.text.buffer();
        let caret = self.pos_at(&buffer.iter_at_mark(&buffer.get_insert()));
        match buffer.selection_bounds() {
            Some((a, b)) => (caret, self.pos_at(&a), self.pos_at(&b)),
            None => (caret, caret, caret),
        }
    }

    /// Where buffer line `line`'s text starts, in characters: after its prefix.
    fn text_start(&self, line: i32) -> Option<i32> {
        let shown = self.shown.borrow();
        let l = shown.lines.get(usize::try_from(line).ok()?)?;
        i32::try_from(prefix(l).chars().count()).ok()
    }

    /// The slide of outline line `line`.
    fn slide_of(&self, line: usize) -> Option<usize> {
        self.shown.borrow().lines.get(line).map(|l| l.slide)
    }
}

/// The start of buffer line `n`; the end of the buffer past its last line.
fn line_start(buffer: &gtk::TextBuffer, n: usize) -> gtk::TextIter {
    i32::try_from(n)
        .ok()
        .and_then(|n| buffer.iter_at_line(n))
        .unwrap_or_else(|| buffer.end_iter())
}

/// What shows before a line's text: a title's slide number, a bullet's sign (by level, as
/// PowerPoint's bullets go: •, ◦, ▪, again), or, for a count, all the line shows.
fn prefix(line: &Line) -> String {
    match line.kind {
        LineKind::Title => format!("{}  ", line.slide + 1),
        LineKind::Bullet { level } => ["• ", "◦ ", "▪ "][usize::from(level) % 3].to_owned(),
        LineKind::Others { count: 1 } => "1 other item".to_owned(),
        LineKind::Others { count } => format!("{count} other items"),
    }
}

/// The tag a line's text takes.
fn tag_name(line: &Line) -> String {
    match line.kind {
        LineKind::Title => "title".to_owned(),
        LineKind::Bullet { level } => format!("bullet-{}", level.min(LEVELS)),
        LineKind::Others { .. } => "others".to_owned(),
    }
}

/// Insert `line` at `at`: its prefix, then its text, a line break in it shown as a line
/// separator (the buffer's line ends are the outline's).
fn insert_line(buffer: &gtk::TextBuffer, at: &mut gtk::TextIter, line: &Line) {
    let text: String = line
        .text
        .chars()
        .map(|c| match c {
            '\n' | '\r' | '\u{2029}' => '\u{2028}',
            c => c,
        })
        .collect();
    let start = at.offset();
    buffer.insert(at, &prefix(line));
    let text_start = at.offset();
    buffer.insert(at, &text);
    tag(buffer, at, start, line);
    if let Some(dim) = buffer.tag_table().lookup("prefix") {
        let (a, b) = (
            buffer.iter_at_offset(start),
            buffer.iter_at_offset(text_start),
        );
        buffer.apply_tag(&dim, &a, &b);
    }
}

/// Insert the end of `line` at `at`.
fn insert_end(buffer: &gtk::TextBuffer, at: &mut gtk::TextIter, line: &Line) {
    let start = at.offset();
    buffer.insert(at, "\n");
    tag(buffer, at, start, line);
}

/// Give the text from offset `start` to `at` `line`'s tag and no other: text inserted next
/// to tagged text can take its tags.
fn tag(buffer: &gtk::TextBuffer, at: &mut gtk::TextIter, start: i32, line: &Line) {
    let end = at.offset();
    let from = buffer.iter_at_offset(start);
    buffer.remove_all_tags(&from, at);
    if let Some(tag) = buffer.tag_table().lookup(&tag_name(line)) {
        buffer.apply_tag(&tag, &from, at);
    }
    *at = buffer.iter_at_offset(end);
}

/// Make `view`'s outline edit the deck: its button, its buffer's edits, and its keys.
pub fn attach(view: &Rc<DeckWindow>) {
    let o = &view.outline;
    o.button.connect_toggled({
        let view = Rc::downgrade(view);
        move |b| {
            if let Some(view) = view.upgrade() {
                show(&view, b.is_active());
            }
        }
    });
    let buffer = o.text.buffer();
    buffer.connect_insert_text({
        let view = Rc::downgrade(view);
        move |buffer, iter, text| {
            let Some(view) = view.upgrade() else {
                return;
            };
            if view.outline.busy.get() {
                return;
            }
            buffer.stop_signal_emission_by_name("insert-text");
            let at = view.outline.pos_at(iter);
            let (from, to) = view.outline.pending.take().unwrap_or((at, at));
            type_text(&view, from, to, text);
        }
    });
    buffer.connect_delete_range({
        let weak = Rc::downgrade(view);
        move |buffer, start, end| {
            let Some(view) = weak.upgrade() else {
                return;
            };
            if view.outline.busy.get() {
                return;
            }
            buffer.stop_signal_emission_by_name("delete-range");
            flush(&view);
            let range = (view.outline.pos_at(start), view.outline.pos_at(end));
            view.outline.pending.set(Some(range));
            // Ahead of the events queued: the deletion is done before the next key.
            let weak = weak.clone();
            glib::idle_add_local_full(glib::Priority::HIGH, move || {
                if let Some(view) = weak.upgrade() {
                    flush(&view);
                }
                glib::ControlFlow::Break
            });
        }
    });
    buffer.connect_mark_set({
        let view = Rc::downgrade(view);
        move |buffer, iter, mark| {
            let Some(view) = view.upgrade() else {
                return;
            };
            if view.outline.busy.get() || *mark != buffer.get_insert() {
                return;
            }
            // The caret moved by a click or a key: the next typing is a step of its own,
            // and the slide it's on shows. Once the event is handled, it leaves a prefix.
            view.seal();
            let line = usize::try_from(iter.line()).unwrap_or(0);
            if let Some(slide) = view.outline.slide_of(line) {
                if slide != view.canvas.index() {
                    view.go(slide);
                }
            }
            if !view.outline.checking.replace(true) {
                let weak = Rc::downgrade(&view);
                glib::idle_add_local_full(glib::Priority::HIGH, move || {
                    if let Some(view) = weak.upgrade() {
                        out_of_prefix(&view);
                    }
                    glib::ControlFlow::Break
                });
            }
        }
    });
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed({
        let view = Rc::downgrade(view);
        move |_, key, _, mods| match view.upgrade() {
            Some(view) => key_pressed(&view, key, mods),
            None => glib::Propagation::Proceed,
        }
    });
    o.text.add_controller(keys);
}

/// Show the outline (`on`) or the canvas, on the current slide.
fn show(view: &DeckWindow, on: bool) {
    let o = &view.outline;
    view.seal();
    if !on {
        o.stack.set_visible_child_name("slide");
        view.show_slide_status();
        view.canvas.grab_focus();
        return;
    }
    editing::show(view, None);
    view.canvas.select(None);
    o.render(&view.deck.presentation());
    o.stack.set_visible_child_name("outline");
    o.text.grab_focus();
    to_title(view, view.canvas.index());
}

/// The caret at the end of slide `slide`'s title.
fn to_title(view: &DeckWindow, slide: usize) {
    let at = {
        let shown = view.outline.shown.borrow();
        shown
            .title_line(slide)
            .map(|l| OutlinePos::new(l, shown.lines[l].text.len()))
    };
    if let Some(at) = at {
        place(view, at);
    }
}

/// Slide `slide` is shown: unless the outline has the keys (its caret chose the slide), its
/// caret goes to that slide's title.
pub(crate) fn follow(view: &DeckWindow, slide: usize) {
    let o = &view.outline;
    if !o.is_shown() || o.has_focus() || o.busy.get() {
        return;
    }
    let (caret, _, _) = o.selection();
    if o.slide_of(caret.line) != Some(slide) {
        to_title(view, slide);
    }
}

/// The deck changed (an outline edit, the canvas's, undo, the slide list): the outline
/// shows it, the caret where `change` was, else on the line of its slide it was on.
pub(crate) fn changed(view: &DeckWindow, change: Change) {
    let o = &view.outline;
    o.pending.set(None);
    if !o.is_shown() {
        return;
    }
    // Where the caret was: its slide, its line among the slide's, its offset.
    let (cursor, _, _) = o.selection();
    let was = {
        let shown = o.shown.borrow();
        shown.lines.get(cursor.line).map(|l| {
            let first = shown.lines.iter().position(|m| m.slide == l.slide);
            (l.slide, cursor.line - first.unwrap_or(cursor.line))
        })
    };
    let deck = view.deck.presentation();
    o.render(&deck);
    let at = {
        let shown = o.shown.borrow();
        let slide = change.slide();
        let of_text = match change {
            Change::Text { shape, caret } => deck.slides.get(shape.slide).and_then(|s| {
                if Some(shape.shape) == deck_edit::title_of(s) && caret.paragraph == 0 {
                    shown.title_line(shape.slide)
                } else if Some(shape.shape) == deck_edit::body_of(s) {
                    shown.bullet_line(shape.slide, caret.paragraph)
                } else {
                    None
                }
                .map(|line| OutlinePos::new(line, caret.offset))
            }),
            _ => None,
        };
        of_text.or_else(|| {
            let first = shown.title_line(slide)?;
            let count = shown.lines[first..]
                .iter()
                .take_while(|l| l.slide == slide)
                .count();
            match was {
                Some((s, rel)) if s == slide => {
                    Some(OutlinePos::new(first + rel.min(count - 1), cursor.offset))
                }
                _ => Some(OutlinePos::new(first, 0)),
            }
        })
    };
    drop(deck);
    if let Some(at) = at {
        place(view, at);
    }
}

/// The caret at `pos`, scrolled into view, its slide shown.
fn place(view: &DeckWindow, pos: OutlinePos) {
    let o = &view.outline;
    let Some(iter) = o.iter_at(pos) else {
        return;
    };
    let buffer = o.text.buffer();
    o.busy.set(true);
    buffer.place_cursor(&iter);
    o.busy.set(false);
    o.caret.set((iter.line(), iter.line_offset()));
    o.text.scroll_mark_onscreen(&buffer.get_insert());
    if let Some(slide) = o.slide_of(pos.line) {
        if slide != view.canvas.index() {
            view.go(slide);
        }
    }
}

/// The caret out of a line's prefix, where the text view can leave it (Home, a click, Up
/// and Down): at the line's text, or, moving left from the text's start, at the end of the
/// line before. A selection keeps its other end.
fn out_of_prefix(view: &DeckWindow) {
    let o = &view.outline;
    o.checking.set(false);
    let buffer = o.text.buffer();
    let insert = buffer.get_insert();
    let iter = buffer.iter_at_mark(&insert);
    let (line, at) = (iter.line(), iter.line_offset());
    let was = o.caret.replace((line, at));
    let Some(start) = o.text_start(line) else {
        return;
    };
    if at >= start {
        return;
    }
    let left = was == (line, start) && line > 0;
    let target = match buffer.iter_at_line(line) {
        Some(mut t) if left => {
            t.backward_char();
            t
        }
        _ => match buffer.iter_at_line_offset(line, start) {
            Some(t) => t,
            None => return,
        },
    };
    o.busy.set(true);
    if buffer.has_selection() {
        buffer.move_mark(&insert, &target);
    } else {
        buffer.place_cursor(&target);
    }
    o.busy.set(false);
    o.caret.set((target.line(), target.line_offset()));
    o.text.scroll_mark_onscreen(&insert);
    if let Some(slide) = usize::try_from(target.line())
        .ok()
        .and_then(|l| o.slide_of(l))
    {
        if slide != view.canvas.index() {
            view.go(slide);
        }
    }
}

/// Apply the outline edit `make` builds from the deck and the outline shown: the caret
/// where it leaves it. `None` when there's nothing to do.
fn run(
    view: &DeckWindow,
    make: impl FnOnce(&Presentation, &Outline) -> Option<(Edit, OutlinePos)>,
) -> Option<OutlinePos> {
    let done = make(&view.deck.presentation(), &view.outline.shown.borrow());
    let (edit, caret) = done?;
    view.apply(edit);
    place(view, caret);
    Some(caret)
}

/// Do the deletion the text view asked for, when it wasn't typed over.
fn flush(view: &DeckWindow) {
    if let Some((from, to)) = view.outline.pending.take() {
        run(view, |deck, o| deck_edit::erase(deck, o, from, to));
    }
}

/// Type `text` over `from..to`: each line of it after the first starts a line as Enter
/// does; control characters are dropped.
fn type_text(view: &DeckWindow, from: OutlinePos, to: OutlinePos, text: &str) {
    let (mut from, mut to) = (from, to);
    for (i, part) in text.split('\n').enumerate() {
        if i > 0 {
            let Some(caret) = run(view, |deck, o| {
                deck_edit::key(deck, o, from, OutlineKey::Enter)
            }) else {
                return;
            };
            (from, to) = (caret, caret);
        }
        let part: String = part.chars().filter(|c| !c.is_control()).collect();
        let done = if part.is_empty() {
            if from == to {
                continue;
            }
            run(view, |deck, o| deck_edit::erase(deck, o, from, to))
        } else {
            run(view, |deck, o| deck_edit::typing(deck, o, from, to, &part))
        };
        match done {
            Some(caret) => (from, to) = (caret, caret),
            None => return,
        }
    }
}

fn key_pressed(view: &DeckWindow, key: gdk::Key, mods: gdk::ModifierType) -> glib::Propagation {
    use gdk::Key;
    let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
    let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
    let alt = mods.contains(gdk::ModifierType::ALT_MASK);
    if ctrl || alt {
        return glib::Propagation::Proceed;
    }
    let key = match key {
        Key::Return | Key::KP_Enter | Key::ISO_Enter => OutlineKey::Enter,
        Key::ISO_Left_Tab => OutlineKey::ShiftTab,
        Key::Tab | Key::KP_Tab if shift => OutlineKey::ShiftTab,
        Key::Tab | Key::KP_Tab => OutlineKey::Tab,
        Key::BackSpace => OutlineKey::Backspace,
        Key::Delete | Key::KP_Delete => OutlineKey::Delete,
        _ => return glib::Propagation::Proceed,
    };
    flush(view);
    let (caret, from, to) = view.outline.selection();
    if from != to && matches!(key, OutlineKey::Backspace | OutlineKey::Delete) {
        run(view, |deck, o| deck_edit::erase(deck, o, from, to));
        return glib::Propagation::Stop;
    }
    let at = if from != to && key == OutlineKey::Enter {
        match run(view, |deck, o| deck_edit::erase(deck, o, from, to)) {
            Some(at) => at,
            None => return glib::Propagation::Stop,
        }
    } else {
        caret
    };
    run(view, |deck, o| {
        let text = &o.lines.get(at.line)?.text;
        match key {
            // Within a line, a character goes; at its ends the key joins lines.
            OutlineKey::Backspace if at.offset > 0 => {
                let (b, _) = text.get(..at.offset)?.char_indices().next_back()?;
                deck_edit::erase(deck, o, OutlinePos::new(at.line, b), at)
            }
            OutlineKey::Delete if at.offset < text.len() => {
                let c = text.get(at.offset..)?.chars().next()?;
                let after = OutlinePos::new(at.line, at.offset + c.len_utf8());
                deck_edit::erase(deck, o, at, after)
            }
            _ => deck_edit::key(deck, o, at, key),
        }
    });
    glib::Propagation::Stop
}
