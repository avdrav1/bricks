//! The Image menu: commands that change the whole image, its size too ([`Transform`]), each
//! one undo step run off the UI thread ([`Canvas::transform`]).
//!
//! Keys (the spec gives none; Paint.NET's): Ctrl+Shift+X crops to the selection, Ctrl+R
//! resizes the image, Ctrl+Shift+R resizes the canvas, Ctrl+H rotates 90° right, Ctrl+G
//! 90° left, Ctrl+J 180°; the flips have none.

use crate::canvas::Canvas;
use crate::colours;
use gtk::prelude::*;
use gtk::{gdk, gio};
use image_model::{Anchor, Color, Flip, Resampling, Transform, Turn, MAX_SIDE};
use std::cell::Cell;
use std::rc::Rc;

/// The Image menu, each command's key shown beside it (the window's key handler takes the
/// shifted ones, as it does Ctrl+Shift+S; [`set_accels`] the others), in sections: crop,
/// resize, rotate, flip.
pub fn menu() -> gio::Menu {
    let menu = gio::Menu::new();
    let section = |items: &[(&str, &str, Option<&str>)]| {
        let section = gio::Menu::new();
        for &(label, action, shifted) in items {
            let item = gio::MenuItem::new(Some(label), Some(action));
            if let Some(accel) = shifted {
                item.set_attribute_value("accel", Some(&accel.to_variant()));
            }
            section.append_item(&item);
        }
        menu.append_section(None, &section);
    };
    section(&[("_Crop to Selection", "win.crop", Some("<Control><Shift>x"))]);
    section(&[
        ("_Resize Image…", "win.resize-image", None),
        (
            "Resize Ca_nvas…",
            "win.resize-canvas",
            Some("<Control><Shift>r"),
        ),
    ]);
    section(&[
        ("Rotate 90° Righ_t", "win.rotate-right", None),
        ("Rotate 90° _Left", "win.rotate-left", None),
        ("Rotate 1_80°", "win.rotate-half", None),
    ]);
    section(&[
        ("Flip _Horizontal", "win.flip-horizontal", None),
        ("Flip _Vertical", "win.flip-vertical", None),
    ]);
    menu
}

/// The Image menu's accelerators: its unshifted keys.
pub fn set_accels(app: &gtk::Application) {
    for (action, key) in [
        ("win.resize-image", "<Control>r"),
        ("win.rotate-right", "<Control>h"),
        ("win.rotate-left", "<Control>g"),
        ("win.rotate-half", "<Control>j"),
    ] {
        app.set_accels_for_action(action, &[key]);
    }
}

/// The commands that only run a transform, and it.
const TRANSFORMS: [(&str, Transform); 5] = [
    ("rotate-right", Transform::Rotate(Turn::Right)),
    ("rotate-left", Transform::Rotate(Turn::Left)),
    ("rotate-half", Transform::Rotate(Turn::Half)),
    ("flip-horizontal", Transform::Flip(Flip::Horizontal)),
    ("flip-vertical", Transform::Flip(Flip::Vertical)),
];

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
        action("resize-canvas", ask_resize_canvas),
    ]);
    window.add_action_entries(TRANSFORMS.map(|(name, transform)| {
        let canvas = canvas.clone();
        gio::ActionEntry::builder(name)
            .activate(move |_: &gtk::ApplicationWindow, _, _| canvas.transform(transform))
            .build()
    }));
}

/// Enables the commands that can run now: none while one runs, Crop to Selection only
/// with a selection.
pub fn update(window: &gtk::ApplicationWindow, canvas: &Canvas) {
    let idle = !canvas.busy();
    enable(window, "crop", idle && canvas.selection_size().is_some());
    enable(window, "resize-image", idle);
    enable(window, "resize-canvas", idle);
    for (name, _) in TRANSFORMS {
        enable(window, name, idle);
    }
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
        gdk::Key::r | gdk::Key::R => "resize-canvas",
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

/// The anchor's button: its arrow (a dot for the centre) and its name.
fn anchor_label(anchor: Anchor) -> (&'static str, &'static str) {
    match anchor {
        Anchor::TopLeft => ("↖", "Top left"),
        Anchor::Top => ("↑", "Top"),
        Anchor::TopRight => ("↗", "Top right"),
        Anchor::Left => ("←", "Left"),
        Anchor::Centre => ("●", "Centre"),
        Anchor::Right => ("→", "Right"),
        Anchor::BottomLeft => ("↙", "Bottom left"),
        Anchor::Bottom => ("↓", "Bottom"),
        Anchor::BottomRight => ("↘", "Bottom right"),
    }
}

/// Image → Resize Canvas (Ctrl+Shift+R): asks for the canvas's new width and height (the
/// image's at first), where the image goes on it (an anchor of nine, the centre at
/// first: toggle buttons in a 3×3 grid, each an arrow named for screen readers), and
/// what fills the new area (Transparent at first, or the window's background colour),
/// then resizes the canvas: the image's pixels unscaled, cropped where the canvas is
/// smaller.
fn ask_resize_canvas(window: &gtk::ApplicationWindow, canvas: &Canvas) {
    let (w, h) = canvas.image_size();
    let background = crate::window::background_of(Some(window.upcast_ref()));
    let (dialog, grid, ok) = dialog(window, "Resize Canvas", "_Resize", 4);
    let width = side_field(&grid, 0, "_Width", w);
    let height = side_field(&grid, 1, "_Height", h);

    let name = gtk::Label::new(Some("Anchor"));
    name.set_xalign(0.0);
    name.set_valign(gtk::Align::Start);
    grid.attach(&name, 0, 2, 1, 1);
    let anchors = gtk::Grid::builder()
        .row_homogeneous(true)
        .column_homogeneous(true)
        .halign(gtk::Align::Start)
        .build();
    anchors.add_css_class("linked");
    anchors.update_property(&[gtk::accessible::Property::Label("Anchor")]);
    let buttons = Anchor::ALL.map(|a| {
        let (arrow, name) = anchor_label(a);
        // The arrow as a child, not the button's label: a label would name the button
        // for screen readers ("↖") over its own name.
        let button = gtk::ToggleButton::new();
        button.set_child(Some(&gtk::Label::new(Some(arrow))));
        button.update_property(&[gtk::accessible::Property::Label(name)]);
        button.set_tooltip_text(Some(&format!("{name}: keep the image there")));
        button
    });
    for (k, b) in buttons.iter().enumerate() {
        if k > 0 {
            b.set_group(Some(&buttons[0]));
        }
        anchors.attach(b, (k % 3) as i32, (k / 3) as i32, 1, 1);
    }
    buttons[4].set_active(true);
    grid.attach(&anchors, 1, 2, 2, 1);

    let name = gtk::Label::new(Some("New area"));
    name.set_xalign(0.0);
    name.set_valign(gtk::Align::Start);
    grid.attach(&name, 0, 3, 1, 1);
    let choices = gtk::Box::new(gtk::Orientation::Vertical, 4);
    choices.update_property(&[gtk::accessible::Property::Label("New area")]);
    let transparent = gtk::CheckButton::with_mnemonic("_Transparent");
    let filled = gtk::CheckButton::with_mnemonic("Back_ground colour");
    let hex = colours::hex(background);
    filled.set_tooltip_text(Some(&format!("The background colour, {hex}")));
    filled.update_property(&[gtk::accessible::Property::Description(&hex)]);
    filled.set_group(Some(&transparent));
    transparent.set_active(true);
    choices.append(&transparent);
    choices.append(&filled);
    grid.attach(&choices, 1, 3, 2, 1);

    ok.connect_clicked({
        let (dialog, canvas) = (dialog.downgrade(), canvas.clone());
        move |_| {
            width.update();
            height.update();
            let anchor = Anchor::ALL
                .into_iter()
                .zip(&buttons)
                .find(|(_, b)| b.is_active())
                .map_or(Anchor::Centre, |(a, _)| a);
            let fill = match filled.is_active() {
                true => background,
                false => Color::TRANSPARENT,
            };
            if let Some(d) = dialog.upgrade() {
                d.close();
            }
            canvas.transform(Transform::ResizeCanvas {
                width: width.value() as u32,
                height: height.value() as u32,
                anchor,
                fill,
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
