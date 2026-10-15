//! User-interface conventions shared by every app in the suite.
//!
//! [`fonts`] chooses how text finds its fonts and adds the bundled ones on macOS;
//! [`appearance`] switches the theme between light and dark. Menus, the command palette,
//! toolbars, dialogs, window chrome, and shortcut handling may move here once two apps
//! need the same one. The suite's toolkit is GTK 4 (docs/adr/0002-gtk4-for-the-suite.md).

pub mod appearance;
pub mod fonts;

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
