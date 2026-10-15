//! The Image menu: commands that change the whole image, its size too ([`Transform`]), each
//! one undo step run off the UI thread ([`Canvas::transform`]).
//!
//! Keys (the spec gives none): Ctrl+Shift+X crops to the selection (Paint's and
//! Paint.NET's).

use crate::canvas::Canvas;
use gtk::prelude::*;
use gtk::{gdk, gio};

/// The Image menu, each command's key shown beside it (the window's key handler takes the
/// shifted ones, as it does Ctrl+Shift+S).
pub fn menu() -> gio::Menu {
    let menu = gio::Menu::new();
    let item = gio::MenuItem::new(Some("_Crop to Selection"), Some("win.crop"));
    item.set_attribute_value("accel", Some(&"<Control><Shift>x".to_variant()));
    menu.append_item(&item);
    menu
}

/// The window's Image menu actions, acting on `canvas`.
pub fn add_actions(window: &gtk::ApplicationWindow, canvas: &Canvas) {
    let crop = gio::ActionEntry::builder("crop")
        .activate({
            let canvas = canvas.clone();
            move |_: &gtk::ApplicationWindow, _, _| canvas.crop()
        })
        .build();
    window.add_action_entries([crop]);
}

/// Enables the commands that can run now: none while one runs, Crop to Selection only
/// with a selection.
pub fn update(window: &gtk::ApplicationWindow, canvas: &Canvas) {
    let idle = !canvas.busy();
    enable(window, "crop", idle && canvas.selection_size().is_some());
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
