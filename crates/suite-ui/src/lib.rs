//! User-interface conventions shared by every app in the suite.
//!
//! Menus, the command palette, toolbars, dialogs, themes, window chrome, and shortcut
//! handling may move here once two apps need the same one. The GUI toolkit is not chosen
//! yet (see docs/architecture/overview.md), so nothing here depends on one.

use suite_core::AppKind;

/// The line an app prints when it starts: `Bricks starting...`.
pub fn startup_message(app: AppKind) -> String {
    format!("{} starting...", app.display_name())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_message_names_the_app() {
        assert_eq!(startup_message(AppKind::Bricks), "Bricks starting...");
        assert_eq!(startup_message(AppKind::Slate), "Slate starting...");
        assert_eq!(startup_message(AppKind::Frame), "Frame starting...");
    }
}
