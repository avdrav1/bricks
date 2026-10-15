//! Operating-system integration shared by every app in the suite: Linux and macOS
//! (ADR 0011). [`current`] checks the build's platform; [`dirs`] has each app's per-user
//! folders (XDG on Linux, `~/Library` on macOS); `appearance` (macOS only) reads the
//! Mac's light or dark appearance and follows its changes. Nothing here depends on the UI
//! toolkit, so engine crates may use it too.
//!
//! File dialogs, clipboard, desktop integration, drag and drop, Wayland/X11 differences,
//! and recent files may move here once two apps need them.

#[cfg(target_os = "macos")]
pub mod appearance;
pub mod dirs;

use std::fmt;

/// An operating system the suite runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Linux,
    MacOs,
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
            "unsupported operating system {:?}; only Linux and macOS are supported",
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
        _ => Err(Unsupported { os }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_and_macos_are_supported() {
        assert_eq!(platform_of("linux"), Ok(Platform::Linux));
        assert_eq!(platform_of("macos"), Ok(Platform::MacOs));
        let err = platform_of("windows").unwrap_err();
        assert_eq!(err, Unsupported { os: "windows" });
        assert!(err.to_string().contains("\"windows\""));
    }

    #[test]
    fn this_build_is_supported() {
        let expected = if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Linux
        };
        assert_eq!(current(), Ok(expected));
    }
}
