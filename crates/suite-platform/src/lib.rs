//! Operating-system integration shared by every app in the suite: Linux, macOS, and
//! Windows (ADR 0011). [`current`] checks the build's platform; [`dirs`] has each app's
//! per-user folders (XDG on Linux, `~/Library` on macOS, `%APPDATA%` and `%LOCALAPPDATA%`
//! on Windows); `appearance` (macOS and Windows) reads the system's light or dark mode and
//! follows its changes; [`console`] lets a Windows GUI program print to the terminal it
//! was started from; [`launch`] says when the app has finished launching (on macOS, after
//! Finder's files have opened); [`bundle`] points GTK at the libraries' files inside a
//! macOS app bundle. Nothing here depends on the UI toolkit, so engine crates may use it
//! too.
//!
//! File dialogs, clipboard, desktop integration, drag and drop, Wayland/X11 differences,
//! and recent files may move here once two apps need them.

#[cfg(any(target_os = "macos", windows))]
pub mod appearance;
pub mod bundle;
#[cfg(target_os = "macos")]
mod cf;
pub mod console;
pub mod dirs;
pub mod launch;

use std::fmt;

/// An operating system the suite runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Linux,
    MacOs,
    Windows,
}

/// The suite was built for an operating system it doesn't support.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unsupported {
    /// `std::env::consts::OS` of the build.
    pub os: &'static str,
}

impl fmt::Display for Unsupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unsupported operating system {:?}; only Linux, macOS, and Windows are supported",
            self.os
        )
    }
}

impl std::error::Error for Unsupported {}

/// The platform this build runs on, or why it can't run.
pub fn current() -> Result<Platform, Unsupported> {
    platform_of(std::env::consts::OS)
}

fn platform_of(os: &'static str) -> Result<Platform, Unsupported> {
    match os {
        "linux" => Ok(Platform::Linux),
        "macos" => Ok(Platform::MacOs),
        "windows" => Ok(Platform::Windows),
        _ => Err(Unsupported { os }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_macos_and_windows_are_supported() {
        assert_eq!(platform_of("linux"), Ok(Platform::Linux));
        assert_eq!(platform_of("macos"), Ok(Platform::MacOs));
        assert_eq!(platform_of("windows"), Ok(Platform::Windows));
        let err = platform_of("freebsd").unwrap_err();
        assert_eq!(err, Unsupported { os: "freebsd" });
        assert!(err.to_string().contains("\"freebsd\""));
    }

    #[test]
    fn this_build_is_supported() {
        let expected = if cfg!(target_os = "macos") {
            Platform::MacOs
        } else if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Linux
        };
        assert_eq!(current(), Ok(expected));
    }
}
