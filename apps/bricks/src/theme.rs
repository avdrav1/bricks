//! Follow the desktop's dark or light preference (APP-6). Desktops publish it as the
//! `org.freedesktop.appearance color-scheme` setting of the settings portal (GNOME 42+,
//! KDE, and wlroots desktops through xdg-desktop-portal-gtk), not always as a dark theme
//! name, and GTK before 4.20 reads only the theme name. So the app reads the setting at
//! startup, follows its changes while running, and asks GTK for the dark or light variant
//! of the theme. Everything else follows: the grid takes its colors from the theme's
//! foreground color. Without a portal nothing changes.

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";
const SETTINGS: &str = "org.freedesktop.portal.Settings";
const NAMESPACE: &str = "org.freedesktop.appearance";
const KEY: &str = "color-scheme";

/// Whether a `color-scheme` value asks for dark: 1 prefers dark, 0 (no preference) and 2
/// (prefers light) get the light variant. `None` for anything else.
pub fn prefers_dark(value: &glib::Variant) -> Option<bool> {
    match value.get::<u32>()? {
        1 => Some(true),
        0 | 2 => Some(false),
        _ => None,
    }
}

fn apply(value: &glib::Variant) {
    let (Some(dark), Some(settings)) = (prefers_dark(value), gtk::Settings::default()) else {
        return;
    };
    // GTK 4.20 replaced the dark-variant switch with `gtk-interface-color-scheme` (and
    // warns when the old one is set). The app builds against GTK 4.14's API, so the new
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
}

thread_local! {
    /// The portal's proxy, kept while the app runs: dropping it would stop the
    /// SettingChanged signals.
    static PROXY: std::cell::RefCell<Option<gio::DBusProxy>> =
        const { std::cell::RefCell::new(None) };
}

/// Read the setting now and follow it from then on. Call once GTK is initialized.
pub fn follow_color_scheme() {
    glib::spawn_future_local(async {
        let proxy = match gio::DBusProxy::for_bus_future(
            gio::BusType::Session,
            gio::DBusProxyFlags::DO_NOT_LOAD_PROPERTIES,
            None,
            PORTAL,
            PATH,
            SETTINGS,
        )
        .await
        {
            Ok(p) => p,
            Err(_) => return, // no session bus
        };
        proxy.connect_local("g-signal", false, |args| {
            let signal = args.get(2)?.get::<String>().ok()?;
            let params = args.get(3)?.get::<glib::Variant>().ok()?;
            if signal == "SettingChanged" {
                let (namespace, key, value) = params.get::<(String, String, glib::Variant)>()?;
                if namespace == NAMESPACE && key == KEY {
                    apply(&value);
                }
            }
            None
        });
        PROXY.with_borrow_mut(|p| *p = Some(proxy.clone()));
        let args = (NAMESPACE, KEY).to_variant();
        let flags = gio::DBusCallFlags::NONE;
        // ReadOne (portal version 2) returns the value; Read (older) wraps it once more.
        let value = match proxy.call_future("ReadOne", Some(&args), flags, 2000).await {
            Ok(reply) => reply.child_value(0).as_variant(),
            Err(_) => match proxy.call_future("Read", Some(&args), flags, 2000).await {
                Ok(reply) => reply
                    .child_value(0)
                    .as_variant()
                    .and_then(|v| v.as_variant()),
                Err(_) => None, // no portal, or no such setting
            },
        };
        if let Some(v) = value {
            apply(&v);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_scheme_values() {
        assert_eq!(prefers_dark(&1u32.to_variant()), Some(true));
        assert_eq!(prefers_dark(&2u32.to_variant()), Some(false));
        assert_eq!(
            prefers_dark(&0u32.to_variant()),
            Some(false),
            "no preference: light"
        );
        assert_eq!(prefers_dark(&7u32.to_variant()), None);
        assert_eq!(prefers_dark(&"dark".to_variant()), None, "not a u32");
    }
}
