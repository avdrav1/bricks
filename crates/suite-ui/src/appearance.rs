//! The light or dark look of GTK's theme. Brick follows the desktop's preference on Linux
//! (`theme.rs`, APP-6); every app follows the Mac's appearance through
//! `suite_platform::appearance` on macOS (MAC-6).

use gtk::glib;
use gtk::prelude::*;

/// Ask GTK for the dark (`true`) or light variant of the theme. Safe from any thread: GTK
/// is told on the main context, at once when the caller already runs there.
pub fn set_dark(dark: bool) {
    tracing::info!(
        "appearance: {} (thread {:?}, main context owner {})",
        if dark { "dark" } else { "light" },
        std::thread::current().name(),
        glib::MainContext::default().is_owner()
    );
    glib::MainContext::default().invoke(move || apply(dark));
}

fn apply(dark: bool) {
    let Some(settings) = gtk::Settings::default() else {
        return;
    };
    // GTK 4.20 replaced the dark-variant switch with `gtk-interface-color-scheme` (and
    // warns when the old one is set). The apps build against GTK 4.14's API, so the new
    // setting is found by name at run time.
    let new = glib::Type::from_name("GtkInterfaceColorScheme")
        .and_then(glib::EnumClass::with_type)
        .and_then(|e| e.to_value_by_nick(if dark { "dark" } else { "light" }));
    match new {
        Some(v) => settings.set_property_from_value("gtk-interface-color-scheme", &v),
        None if settings.is_gtk_application_prefer_dark_theme() != dark => {
            settings.set_gtk_application_prefer_dark_theme(dark)
        }
        None => {}
    }
    if new_exists() {
        tracing::info!(
            "appearance applied: {:?}",
            settings.property_value("gtk-interface-color-scheme")
        );
    }
}

fn new_exists() -> bool {
    glib::Type::from_name("GtkInterfaceColorScheme").is_some()
}
