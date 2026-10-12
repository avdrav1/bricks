//! The slide list and slide commands (FRAME-7). The sidebar's numbered thumbnails are
//! rebuilt whenever slides come and go. New Slide (Ctrl+M, the header bar's button)
//! adds a slide after the current one on its layout, or on Title and Content after a
//! title slide; the button's menu shows the deck's layouts as thumbnails to pick from.
//! Ctrl+D duplicates the slide, Delete in the list deletes it, and Ctrl+Up and Ctrl+Down
//! or dragging a thumbnail move it; the list's context menu has New, Duplicate, and Delete.
//! Each is one step in the deck's undo history; nothing asks for confirmation.

use crate::deck::Deck;
use crate::slide_view::SlideView;
use crate::window::DeckWindow;
use deck_edit::SlideEdit;
use gtk::prelude::*;
use gtk::{gdk, gio, graphene};
use std::rc::Rc;

/// A thumbnail's width in the slide list.
pub const THUMB_WIDTH: i32 = 176;
/// A layout's thumbnail's width in the New Slide menu.
const LAYOUT_WIDTH: i32 = 144;

/// The list row of slide `index`: its number and thumbnail (dimmed when hidden), and a
/// drag source offering its index, to move it.
fn row(deck: &Rc<Deck>, index: usize) -> (gtk::ListBoxRow, SlideView) {
    let hidden = deck.presentation().slides[index].hidden;
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    content.set_margin_top(6);
    content.set_margin_bottom(6);
    content.set_margin_start(4);
    content.set_margin_end(8);
    let number = gtk::Label::new(Some(&(index + 1).to_string()));
    number.set_valign(gtk::Align::Start);
    number.set_width_chars(3);
    number.set_xalign(1.0);
    let thumb = SlideView::new(deck, index, THUMB_WIDTH);
    let label = if hidden {
        format!("Slide {} (hidden)", index + 1)
    } else {
        format!("Slide {}", index + 1)
    };
    thumb.update_property(&[gtk::accessible::Property::Label(&label)]);
    content.append(&number);
    content.append(&thumb);
    if hidden {
        // Hidden slides show dimmed; the slideshow skips them.
        content.set_opacity(0.4);
        content.set_tooltip_text(Some("Hidden slide"));
    }
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&content));
    let drag = gtk::DragSource::new();
    drag.set_actions(gdk::DragAction::MOVE);
    drag.connect_prepare({
        let row = row.downgrade();
        move |_, _, _| {
            let index = u32::try_from(row.upgrade()?.index()).ok()?;
            Some(gdk::ContentProvider::for_value(&index.to_value()))
        }
    });
    drag.connect_drag_begin({
        let thumb = thumb.downgrade();
        move |drag, _| {
            if let Some(thumb) = thumb.upgrade() {
                drag.set_icon(Some(&gtk::WidgetPaintable::new(Some(&thumb))), 0, 0);
            }
        }
    });
    row.add_controller(drag);
    (row, thumb)
}

/// Fill the slide list with the deck's slides as they are now.
pub fn rebuild(view: &DeckWindow) {
    view.list.remove_all();
    let n = view.count();
    let mut thumbs = Vec::with_capacity(n);
    for i in 0..n {
        let (row, thumb) = row(&view.deck, i);
        view.list.append(&row);
        thumbs.push(thumb);
    }
    view.thumbs.replace(thumbs);
}

/// Whether the keyboard focus is in the slide list.
pub fn has_focus(view: &DeckWindow) -> bool {
    view.list
        .root()
        .and_then(|r| r.focus())
        .is_some_and(|f| f == view.list || f.is_ancestor(&view.list))
}

/// The slide shown, when there is one.
fn current(view: &DeckWindow) -> Option<usize> {
    (view.count() > 0).then(|| view.canvas.index())
}

/// Ctrl+M: a new slide after the current one.
pub fn new_slide(view: &DeckWindow) {
    let edit = SlideEdit::new_slide(&view.deck.presentation(), current(view));
    if let Some(edit) = edit {
        view.apply(edit);
    }
}

/// A new slide on layout `layout` after the current one.
fn on_layout(view: &DeckWindow, layout: usize) {
    let edit = SlideEdit::on_layout(&view.deck.presentation(), current(view), layout);
    if let Some(edit) = edit {
        view.apply(edit);
    }
}

/// Ctrl+D: a copy of the current slide after it.
pub fn duplicate(view: &DeckWindow) {
    let edit = current(view).and_then(|i| SlideEdit::duplicate(&view.deck.presentation(), i));
    if let Some(edit) = edit {
        view.apply(edit);
    }
}

/// Delete: the current slide goes.
pub fn delete(view: &DeckWindow) {
    let edit = current(view).and_then(|i| SlideEdit::delete(&view.deck.presentation(), i));
    if let Some(edit) = edit {
        view.apply(edit);
    }
}

/// Ctrl+Up and Ctrl+Down: the current slide moves `by` places.
pub fn move_by(view: &DeckWindow, by: isize) {
    let Some(from) = current(view) else {
        return;
    };
    let to = from.saturating_add_signed(by).min(view.count() - 1);
    move_to(view, from, to);
}

fn move_to(view: &DeckWindow, from: usize, to: usize) {
    let edit = SlideEdit::move_to(&view.deck.presentation(), from, to);
    if let Some(edit) = edit {
        view.apply(edit);
    }
}

/// Where a thumbnail dropped at height `y` of the list of `n` slides goes: before the row
/// there, or after it in its lower half; the slide from `from` lands at the returned
/// index.
fn drop_index(list: &gtk::ListBox, n: usize, from: usize, y: f64) -> Option<usize> {
    let gap = match list.row_at_y(y as i32) {
        Some(row) => {
            let b = row.compute_bounds(list)?;
            let i = usize::try_from(row.index()).ok()?;
            if y > f64::from(b.y() + b.height() / 2.0) {
                i + 1
            } else {
                i
            }
        }
        // Below the last row.
        None => n,
    };
    Some(if gap > from { gap - 1 } else { gap })
}

/// The slide list's drop target, context menu (parented to `side`, the box the list's
/// scrolled window is in: a list box's children must all be rows), and slide actions,
/// and the header bar's New Slide button with its menu of layouts.
pub fn attach(
    view: &Rc<DeckWindow>,
    window: &gtk::ApplicationWindow,
    bar: &gtk::HeaderBar,
    side: &gtk::Box,
) {
    let list = &view.list;
    let drop = gtk::DropTarget::new(u32::static_type(), gdk::DragAction::MOVE);
    drop.connect_drop({
        let view = Rc::downgrade(view);
        move |_, value, _, y| {
            let (Some(view), Ok(from)) = (view.upgrade(), value.get::<u32>()) else {
                return false;
            };
            let from = from as usize;
            match drop_index(&view.list, view.count(), from, y) {
                Some(to) => {
                    move_to(&view, from, to);
                    true
                }
                None => false,
            }
        }
    });
    list.add_controller(drop);

    let entry = |name: &str, run: fn(&DeckWindow)| {
        let view = Rc::downgrade(view);
        gio::ActionEntry::builder(name)
            .activate(move |_: &gtk::ApplicationWindow, _, _| {
                if let Some(view) = view.upgrade() {
                    run(&view);
                }
            })
            .build()
    };
    window.add_action_entries([
        entry("new-slide", new_slide),
        entry("duplicate-slide", duplicate),
        entry("delete-slide", delete),
    ]);

    // Right-click in the list: the slide clicked, and what can be done with it.
    let menu = gio::Menu::new();
    menu.append(Some("New Slide"), Some("win.new-slide"));
    menu.append(Some("Duplicate Slide"), Some("win.duplicate-slide"));
    menu.append(Some("Delete Slide"), Some("win.delete-slide"));
    let popover = gtk::PopoverMenu::from_model(Some(&menu));
    popover.set_parent(side);
    popover.set_has_arrow(false);
    let click = gtk::GestureClick::new();
    click.set_button(gdk::BUTTON_SECONDARY);
    click.connect_pressed({
        let (view, side) = (Rc::downgrade(view), side.downgrade());
        move |_, _, x, y| {
            let (Some(view), Some(side)) = (view.upgrade(), side.upgrade()) else {
                return;
            };
            if let Some(row) = view.list.row_at_y(y as i32) {
                view.go(row.index() as usize);
            }
            let at = view
                .list
                .compute_point(&side, &graphene::Point::new(x as f32, y as f32))
                .unwrap_or_else(|| graphene::Point::new(x as f32, y as f32));
            let at = gdk::Rectangle::new(at.x() as i32, at.y() as i32, 1, 1);
            popover.set_pointing_to(Some(&at));
            popover.popup();
        }
    });
    list.add_controller(click);

    // The header bar: New Slide, and beside it the menu of layouts.
    let add = gtk::Button::with_label("New Slide");
    add.set_action_name(Some("win.new-slide"));
    add.set_tooltip_text(Some("New Slide (Ctrl+M)"));
    let layouts = gtk::MenuButton::new();
    layouts.set_tooltip_text(Some("New Slide with a layout"));
    layouts.update_property(&[gtk::accessible::Property::Label("Layouts")]);
    layouts.set_popover(Some(&layout_menu(view, &layouts)));
    let group = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    group.add_css_class("linked");
    group.append(&add);
    group.append(&layouts);
    bar.pack_start(&group);
}

/// The New Slide menu: the deck's layouts as thumbnails of the slides they make, named;
/// picking one adds a slide on it after the current one.
fn layout_menu(view: &Rc<DeckWindow>, button: &gtk::MenuButton) -> gtk::Popover {
    let flow = gtk::FlowBox::new();
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_min_children_per_line(2);
    flow.set_max_children_per_line(4);
    flow.set_homogeneous(true);
    let names: Vec<String> = view
        .deck
        .presentation()
        .layouts
        .iter()
        .map(|l| l.name.clone())
        .collect();
    for (i, name) in names.iter().enumerate() {
        let thumb = SlideView::layout(&view.deck, i, LAYOUT_WIDTH);
        let label = gtk::Label::new(Some(name));
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(18);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 4);
        content.append(&thumb);
        content.append(&label);
        let pick = gtk::Button::new();
        pick.add_css_class("flat");
        pick.set_child(Some(&content));
        pick.set_tooltip_text(Some(name));
        pick.update_property(&[gtk::accessible::Property::Label(name)]);
        pick.connect_clicked({
            let (view, button) = (Rc::downgrade(view), button.downgrade());
            move |_| {
                if let Some(button) = button.upgrade() {
                    button.popdown();
                }
                if let Some(view) = view.upgrade() {
                    on_layout(&view, i);
                }
            }
        });
        flow.append(&pick);
    }
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(520)
        .child(&flow)
        .build();
    let popover = gtk::Popover::new();
    popover.set_child(Some(&scrolled));
    popover
}
