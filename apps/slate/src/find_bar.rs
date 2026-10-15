//! The find bar (SLATE-16), above a document's page: Ctrl+F (Edit → Find…) opens it with
//! the text to find, Ctrl+H (Edit → Replace…) with a replacement row too.
//!
//! The page searches as the text changes, off the UI thread, and highlights every match.
//! Enter and the down arrow select the next match, Shift+Enter and the up arrow the one
//! before, wrapping round; the bar counts them ("3 of 12"). Match Case makes case matter.
//! Replace replaces the selected match and selects the next (the first press selects the
//! current match); Replace All replaces every match as one undoable step. Escape or the
//! close button hides the bar, takes the highlights away, and goes back to the page.

use crate::view::Page;
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use suite_ui::keys::Primary;

/// Put the find bar above `page` in `content` (the window's column of page and status), and
/// give `window` its `find` and `replace` actions.
pub fn attach(window: &gtk::ApplicationWindow, content: &gtk::Box, page: &Page) {
    let find = gtk::SearchEntry::builder()
        .placeholder_text("Find")
        .hexpand(true)
        .search_delay(0)
        .build();
    find.update_property(&[gtk::accessible::Property::Label("Find")]);
    let case = gtk::CheckButton::with_label("Match Case");
    let count = gtk::Label::builder()
        .width_chars(12)
        .xalign(0.0)
        .css_classes(["dim-label", "numeric"])
        .build();
    let previous = button("go-up-symbolic", "Previous Match (Shift+Enter)");
    let next = button("go-down-symbolic", "Next Match (Enter)");
    let close = button("window-close-symbolic", "Close (Escape)");
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    for w in [
        find.upcast_ref::<gtk::Widget>(),
        case.upcast_ref(),
        count.upcast_ref(),
        previous.upcast_ref(),
        next.upcast_ref(),
        close.upcast_ref(),
    ] {
        row.append(w);
    }

    let with = gtk::Entry::builder()
        .placeholder_text("Replace with")
        .hexpand(true)
        .build();
    with.update_property(&[gtk::accessible::Property::Label("Replace with")]);
    let replace = gtk::Button::with_label("Replace");
    let replace_all = gtk::Button::with_label("Replace All");
    let replace_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    replace_row.append(&with);
    replace_row.append(&replace);
    replace_row.append(&replace_all);

    let rows = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .margin_start(12)
        .margin_end(12)
        .margin_top(6)
        .margin_bottom(6)
        .build();
    rows.append(&row);
    rows.append(&replace_row);
    let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
    frame.append(&rows);
    frame.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    let bar = gtk::Revealer::builder()
        .child(&frame)
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .build();
    content.prepend(&bar);

    // Search as the text or Match Case changes.
    let search = {
        let (page, find, case) = (page.downgrade(), find.clone(), case.clone());
        move || {
            if let Some(page) = page.upgrade() {
                page.set_find(Some((find.text().to_string(), case.is_active())));
            }
        }
    };
    find.connect_search_changed({
        let search = search.clone();
        move |_| search()
    });
    case.connect_toggled({
        let search = search.clone();
        move |_| search()
    });
    page.connect_found({
        let (count, find, bar) = (count.clone(), find.clone(), bar.downgrade());
        move |current, total, more| {
            let plus = if more { "+" } else { "" };
            let open = bar.upgrade().is_some_and(|b| b.reveals_child());
            let text = match (current, total) {
                _ if !open || find.text().is_empty() => String::new(),
                (_, 0) => "No matches".to_owned(),
                (Some(i), n) => format!("{} of {n}{plus}", i + 1),
                (None, n) => format!("{n}{plus} matches"),
            };
            count.set_text(&text);
        }
    });

    let step = |forward: bool| {
        let page = page.downgrade();
        move || {
            if let Some(page) = page.upgrade() {
                page.find_step(forward);
            }
        }
    };
    find.connect_activate({
        let next = step(true);
        move |_| next()
    });
    find.connect_next_match({
        let next = step(true);
        move |_| next()
    });
    find.connect_previous_match({
        let back = step(false);
        move |_| back()
    });
    next.connect_clicked({
        let next = step(true);
        move |_| next()
    });
    previous.connect_clicked({
        let back = step(false);
        move |_| back()
    });
    // Shift+Enter and the arrows, before the entry takes them.
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed({
        let (next, back) = (step(true), step(false));
        move |_, key, _, mods| {
            use gdk::Key as K;
            match key {
                K::Return | K::KP_Enter if mods.contains(gdk::ModifierType::SHIFT_MASK) => back(),
                K::Up | K::KP_Up => back(),
                K::Down | K::KP_Down => next(),
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        }
    });
    find.add_controller(keys);

    let replace_one = {
        let (page, with) = (page.downgrade(), with.clone());
        move || {
            if let Some(page) = page.upgrade() {
                page.replace_found(&with.text());
            }
        }
    };
    replace.connect_clicked({
        let replace_one = replace_one.clone();
        move |_| replace_one()
    });
    with.connect_activate(move |_| replace_one());
    replace_all.connect_clicked({
        let (page, with) = (page.downgrade(), with.clone());
        move |_| {
            if let Some(page) = page.upgrade() {
                page.replace_all_found(&with.text());
            }
        }
    });

    // Closing: no highlights, back to the page.
    let hide = {
        let (bar, page) = (bar.downgrade(), page.downgrade());
        move || {
            if let (Some(bar), Some(page)) = (bar.upgrade(), page.upgrade()) {
                bar.set_reveal_child(false);
                page.set_find(None);
                page.grab_focus();
            }
        }
    };
    // Escape anywhere in the bar closes it; Find and Replace's keys switch rows there too.
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed({
        let hide = hide.clone();
        move |keys, key, _, mods| {
            if key == gdk::Key::Escape {
                hide();
                return glib::Propagation::Stop;
            }
            let Some(action) = crate::keys::bar_key(Primary::HERE, key, mods) else {
                return glib::Propagation::Proceed;
            };
            if let Some(w) = keys.widget() {
                let _ = w.activate_action(action, None);
            }
            glib::Propagation::Stop
        }
    });
    frame.add_controller(keys);
    close.connect_clicked(move |_| hide());

    // Ctrl+F shows the find row, Ctrl+H the replace row too; either finds again.
    let open = move |replacing: bool| {
        let (bar, find, replace_row, search) = (
            bar.clone(),
            find.clone(),
            replace_row.clone(),
            search.clone(),
        );
        move |_: &gtk::ApplicationWindow, _: &gio::SimpleAction, _: Option<&glib::Variant>| {
            replace_row.set_visible(replacing);
            bar.set_reveal_child(true);
            find.grab_focus();
            find.select_region(0, -1);
            search();
        }
    };
    window.add_action_entries([
        gio::ActionEntry::builder("find")
            .activate(open(false))
            .build(),
        gio::ActionEntry::builder("replace")
            .activate(open(true))
            .build(),
    ]);
}

fn button(icon: &str, tooltip: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(icon);
    b.set_tooltip_text(Some(tooltip));
    b.update_property(&[gtk::accessible::Property::Label(tooltip)]);
    b
}
