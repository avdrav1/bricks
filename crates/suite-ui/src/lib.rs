//! User-interface conventions shared by every app in the suite, in GTK 4
//! (docs/adr/0002-gtk4-for-the-suite.md): [`keys`], the primary modifier every shortcut
//! uses (Control on Linux, Command on macOS), and [`im`], input methods for the apps' own
//! text widgets.
//!
//! Menus, the command palette, toolbars, dialogs, themes, and window chrome may move here
//! once two apps need the same one.

pub mod im;
pub mod keys;

use suite_core::AppKind;

/// The line an app prints when it starts: `Brick starting...`.
pub fn startup_message(app: AppKind) -> String {
    format!("{} starting...", app.display_name())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_message_names_the_app() {
        assert_eq!(startup_message(AppKind::Brick), "Brick starting...");
        assert_eq!(startup_message(AppKind::Slate), "Slate starting...");
        assert_eq!(startup_message(AppKind::Frame), "Frame starting...");
    }
}
