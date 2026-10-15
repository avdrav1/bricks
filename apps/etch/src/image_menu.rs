//! The Image menu: commands that change the whole image, its size too ([`Transform`]), each
//! one undo step run off the UI thread ([`Canvas::transform`]).
//!
//! Keys (the spec gives none; Paint.NET's): Ctrl+Shift+X crops to the selection, Ctrl+R
//! resizes the image.

use crate::canvas::Canvas;
use gtk::prelude::*;
use gtk::{gdk, gio};
use image_model::{Resampling, Transform, MAX_SIDE};
use std::cell::Cell;
use std::rc::Rc;

/// The Image menu, each command's key shown beside it (the window's key handler takes the
/// shifted ones, as it does Ctrl+Shift+S; [`set_accels`] the others).
pub fn menu() -> gio::Menu {
    let menu = gio::Menu::new();
    let item = gio::MenuItem::new(Some("_Crop to Selection"), Some("win.crop"));
    item.set_attribute_value("accel", Some(&"<Control><Shift>x".to_variant()));
    menu.append_item(&item);
    menu.append(Some("_Resize Image…"), Some("win.resize-image"));
    menu
}

/// The Image menu's accelerators: its unshifted keys.
pub fn set_accels(app: &gtk::Application) {
    app.set_accels_for_action("win.resize-image", &["<Control>r"]);
}

/// The window's Image menu actions, acting on `canvas`.
pub fn add_actions(window: &gtk::ApplicationWindow, canvas: &Canvas) {
    let action = |name: &str, f: fn(&gtk::ApplicationWindow, &Canvas)| {
        let canvas = canvas.clone();
        gio::ActionEntry::builder(name)
            .activate(move |window: &gtk::ApplicationWindow, _, _| f(window, &canvas))
            .build()
    };
    window.add_action_entries([
        action("crop", |_, canvas| canvas.crop()),
        action("resize-image", ask_resize_image),
    ]);
}

/// Enables the commands that can run now: none while one runs, Crop to Selection only
/// with a selection.
pub fn update(window: &gtk::ApplicationWindow, canvas: &Canvas) {
    let idle = !canvas.busy();
    enable(window, "crop", idle && canvas.selection_size().is_some());
    enable(window, "resize-image", idle);
}

fn enable(window: &gtk::ApplicationWindow, name: &str, on: bool) {
    if let Some(a) = window
        .lookup_action(name)
        .and_downcast::<gio::SimpleAction>()
    {
        a.set_enabled(on);
    }
}

/// The window's key handler's Image menu keys with Ctrl and Shift held: runs `key`'s
/// command, if it's one of them, and says whether it was.
pub fn shifted_key(window: &gtk::ApplicationWindow, key: gdk::Key) -> bool {
    let action = match key {
        gdk::Key::x | gdk::Key::X => "crop",
        _ => return false,
    };
    ActionGroupExt::activate_action(window, action, None);
    true
}

/// A modal dialog over `parent` titled `title`, laid out as New Image's: a grid for its
/// fields, Cancel, and the default button labelled `run` (returned with the dialog and
/// the grid; the buttons go in `rows`, the grid's row after the fields). Escape closes it.
fn dialog(
    parent: &gtk::ApplicationWindow,
    title: &str,
    run: &str,
    rows: i32,
) -> (gtk::Window, gtk::Grid, gtk::Button) {
    let dialog = gtk::Window::builder()
        .title(title)
        .modal(true)
        .resizable(false)
        .transient_for(parent)
        .build();
    let grid = gtk::Grid::builder()
        .row_spacing(8)
        .column_spacing(8)
        .margin_start(16)
        .margin_end(16)
        .margin_top(16)
        .margin_bottom(16)
        .build();
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    buttons.set_margin_top(8);
    let cancel = gtk::Button::with_mnemonic("_Cancel");
    let ok = gtk::Button::with_mnemonic(run);
    ok.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&ok);
    grid.attach(&buttons, 0, rows, 3, 1);
    dialog.set_child(Some(&grid));
    dialog.set_default_widget(Some(&ok));
    cancel.connect_clicked({
        let dialog = dialog.downgrade();
        move |_| {
            if let Some(d) = dialog.upgrade() {
                d.close();
            }
        }
    });
    let escape = gtk::ShortcutController::new();
    escape.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("Escape"),
        Some(gtk::NamedAction::new("window.close")),
    ));
    dialog.add_controller(escape);
    (dialog, grid, ok)
}

/// A side's field in row `row` of `grid`: its name (`label`, with a mnemonic), a spin
/// button of 1 to [`MAX_SIDE`] holding `value` that activates the default, and "pixels".
fn side_field(grid: &gtk::Grid, row: i32, label: &str, value: u32) -> gtk::SpinButton {
    let name = gtk::Label::with_mnemonic(label);
    name.set_xalign(0.0);
    let spin = gtk::SpinButton::with_range(1.0, MAX_SIDE as f64, 1.0);
    spin.set_value(value as f64);
    spin.set_activates_default(true);
    spin.update_property(&[gtk::accessible::Property::Label(&label.replace('_', ""))]);
    name.set_mnemonic_widget(Some(&spin));
    grid.attach(&name, 0, row, 1, 1);
    grid.attach(&spin, 1, row, 1, 1);
    grid.attach(&gtk::Label::new(Some("pixels")), 2, row, 1, 1);
    spin
}

/// `side` scaled by `to / from`, rounded, kept to 1 to [`MAX_SIDE`].
fn scaled(side: u32, to: f64, from: u32) -> f64 {
    (side as f64 * to / from as f64)
        .round()
        .clamp(1.0, MAX_SIDE as f64)
}

/// Image → Resize Image (Ctrl+R): asks for the new width and height (the image's at
/// first), kept to the image's aspect ratio while Keep aspect ratio is on (at first), and
/// Smooth or Nearest neighbour resampling, then resizes the image to them.
fn ask_resize_image(window: &gtk::ApplicationWindow, canvas: &Canvas) {
    let (w, h) = canvas.image_size();
    let (dialog, grid, ok) = dialog(window, "Resize Image", "_Resize", 4);
    let width = side_field(&grid, 0, "_Width", w);
    let height = side_field(&grid, 1, "_Height", h);
    let keep = gtk::CheckButton::with_mnemonic("_Keep aspect ratio");
    keep.set_active(true);
    grid.attach(&keep, 1, 2, 2, 1);
    // Keeping the ratio: a side changed sets the other from it; `linking` stops the
    // other's change from coming back.
    let linking = Rc::new(Cell::new(false));
    let link = |from: &gtk::SpinButton, to: &gtk::SpinButton, (a, b): (u32, u32)| {
        let (to, keep, linking) = (to.downgrade(), keep.downgrade(), linking.clone());
        from.connect_value_changed(move |from| {
            let (Some(to), Some(keep)) = (to.upgrade(), keep.upgrade()) else {
                return;
            };
            if keep.is_active() && !linking.replace(true) {
                to.set_value(scaled(b, from.value(), a));
                linking.set(false);
            }
        });
    };
    link(&width, &height, (w, h));
    link(&height, &width, (h, w));
    keep.connect_toggled({
        let (width, height) = (width.downgrade(), height.downgrade());
        move |keep| {
            if let (true, Some(width), Some(height)) =
                (keep.is_active(), width.upgrade(), height.upgrade())
            {
                height.set_value(scaled(h, width.value(), w));
            }
        }
    });
    let name = gtk::Label::new(Some("Resampling"));
    name.set_xalign(0.0);
    name.set_valign(gtk::Align::Start);
    grid.attach(&name, 0, 3, 1, 1);
    let choices = gtk::Box::new(gtk::Orientation::Vertical, 4);
    choices.update_property(&[gtk::accessible::Property::Label("Resampling")]);
    let smooth = gtk::CheckButton::with_mnemonic("_Smooth");
    smooth.set_tooltip_text(Some("Averages the pixels: for photos and drawings"));
    let nearest = gtk::CheckButton::with_mnemonic("_Nearest neighbour");
    nearest.set_tooltip_text(Some("Keeps hard pixel edges: for pixel art"));
    nearest.set_group(Some(&smooth));
    smooth.set_active(true);
    choices.append(&smooth);
    choices.append(&nearest);
    grid.attach(&choices, 1, 3, 2, 1);

    ok.connect_clicked({
        let (dialog, canvas) = (dialog.downgrade(), canvas.clone());
        move |_| {
            width.update();
            height.update();
            let resampling = match nearest.is_active() {
                true => Resampling::Nearest,
                false => Resampling::Smooth,
            };
            if let Some(d) = dialog.upgrade() {
                d.close();
            }
            canvas.transform(Transform::Resize {
                width: width.value() as u32,
                height: height.value() as u32,
                resampling,
            });
        }
    });
    dialog.present();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeping_the_aspect_ratio_rounds_and_stays_within_the_limits() {
        // 400×300 to 801 wide: 600.75 high, 601.
        assert_eq!(scaled(300, 801.0, 400), 601.0);
        assert_eq!(scaled(300, 1.0, 400), 1.0);
        assert_eq!(scaled(16384, 16384.0, 2), MAX_SIDE as f64);
    }
}
