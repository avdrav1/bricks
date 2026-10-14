//! Application-independent primitives shared by every app in the suite.
//!
//! No UI, OS, or file code lives here, and nothing here depends on another suite crate or
//! on an app. A type belongs here only when at least two apps need the same thing.

use std::fmt;

/// Which application of the suite is running.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AppKind {
    /// Spreadsheet; today a fast CSV editor.
    Brick,
    /// Word processor.
    Slate,
    /// Presentation app (working name).
    Frame,
    /// Raster image editor.
    Etch,
}

impl AppKind {
    /// Every application, in a stable order.
    pub const ALL: [AppKind; 4] = [
        AppKind::Brick,
        AppKind::Slate,
        AppKind::Frame,
        AppKind::Etch,
    ];

    /// The name shown to people: window titles, menus, messages.
    pub fn display_name(self) -> &'static str {
        match self {
            AppKind::Brick => "Brick",
            AppKind::Slate => "Slate",
            AppKind::Frame => "Frame",
            AppKind::Etch => "Etch",
        }
    }
}

impl fmt::Display for AppKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn application_names_are_correct() {
        let names: Vec<_> = AppKind::ALL.iter().map(|a| a.display_name()).collect();
        assert_eq!(names, ["Brick", "Slate", "Frame", "Etch"]);
        assert_eq!(AppKind::Etch.to_string(), "Etch");
    }
}
