//! The renderer GTK draws windows with (MAC-5, WIN-4). Call [`init`] in `main` before GTK
//! starts and before any thread does.
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
//!
//! Windows: GTK chooses ([`WINDOWS_DEFAULT`] is `None`). MSYS2's GTK tries GL through WGL,
//! then through EGL, and draws with cairo where neither works. On GitHub's GPU-less runner
//! that's every time: WGL has only Windows' GL 1.1 and MSYS2 ships no libEGL, so GTK logs
//! "Failed to realize renderer 'GskGLRenderer' … libEGL not available" and draws every
//! window right with cairo, `GSK_RENDERER=gl` and `=vulkan` included (WIN-4's runs). The
//! black windows of DEC-11 came from Fedora's GTK 4.21 snapshot, whose EGL surface failed,
//! not from the GTK the apps ship. On a machine with a GPU driver GTK draws with GL through
//! WGL; if that draws black on real hardware, set [`WINDOWS_DEFAULT`] to `Some("cairo")`.

/// The renderer the apps ask for on macOS when `GSK_RENDERER` is unset; `None` leaves the
/// choice to GTK.
pub const MAC_DEFAULT: Option<&str> = Some("cairo");

/// The renderer the apps ask for on Windows when `GSK_RENDERER` is unset; `None` leaves
/// the choice to GTK.
pub const WINDOWS_DEFAULT: Option<&str> = None;

/// Set `GSK_RENDERER` to the platform's default unless the environment already sets it.
pub fn init() {
    let set = std::env::var_os("GSK_RENDERER").is_some();
    if let Some(renderer) = chosen(std::env::consts::OS, set) {
        // Before any other thread runs (the caller's contract), so nothing reads the
        // environment meanwhile. GTK reads it with `g_getenv`, which on Windows reads the
        // process's environment, where `set_var` writes.
        std::env::set_var("GSK_RENDERER", renderer);
    }
}

/// The renderer to ask for on `os` when `GSK_RENDERER` is `set` or not.
fn chosen(os: &str, set: bool) -> Option<&'static str> {
    match os {
        "macos" if !set => MAC_DEFAULT,
        "windows" if !set => WINDOWS_DEFAULT,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cairo_on_macos_and_gtks_choice_elsewhere_unless_the_environment_chooses() {
        assert_eq!(chosen("macos", false), Some("cairo"));
        assert_eq!(chosen("macos", true), None);
        assert_eq!(chosen("windows", false), None);
        assert_eq!(chosen("windows", true), None);
        assert_eq!(chosen("linux", false), None);
        assert_eq!(chosen("linux", true), None);
    }
}
