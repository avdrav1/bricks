//! The renderer GTK draws windows with (MAC-5). Call [`init`] in `main` before GTK starts
//! and before any thread does.
//!
//! Linux: GTK chooses (GL, Vulkan, or cairo, by what the machine has); nothing changes.
//!
//! macOS: cairo, unless `GSK_RENDERER` names a renderer. GTK's macOS renderer is GL,
//! through Apple's deprecated OpenGL 4.1 (Homebrew builds no Vulkan, and GTK has no Metal
//! renderer). On GitHub's macOS runners GL draws every window black, on macOS 14 and 26
//! alike, while cairo draws them right (DEC-11 C2; MAC-5's runs). Nobody has tried GL on a
//! real Apple Silicon Mac for the suite yet, so the apps take the renderer seen to work.
//! `GSK_RENDERER=gl` tries GL on a Mac; if it draws there, set [`MAC_DEFAULT`] to `None`
//! to let GTK choose again.

/// The renderer the apps ask for on macOS when `GSK_RENDERER` is unset; `None` leaves the
/// choice to GTK.
pub const MAC_DEFAULT: Option<&str> = Some("cairo");

/// Set `GSK_RENDERER` to the platform's default unless the environment already sets it.
pub fn init() {
    let set = std::env::var_os("GSK_RENDERER").is_some();
    if let Some(renderer) = chosen(std::env::consts::OS, set) {
        // Before any other thread runs (the caller's contract), so nothing reads the
        // environment meanwhile.
        std::env::set_var("GSK_RENDERER", renderer);
    }
}

/// The renderer to ask for on `os` when `GSK_RENDERER` is `set` or not.
fn chosen(os: &str, set: bool) -> Option<&'static str> {
    match os {
        "macos" if !set => MAC_DEFAULT,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cairo_on_macos_unless_the_environment_chooses() {
        assert_eq!(chosen("macos", false), Some("cairo"));
        assert_eq!(chosen("macos", true), None);
        assert_eq!(chosen("linux", false), None);
        assert_eq!(chosen("linux", true), None);
    }
}
