//! Operating-system integration shared by every app in the suite. The first target is
//! Linux.
//!
//! File dialogs, clipboard, desktop integration, drag and drop, Wayland/X11 differences,
//! system theme, and recent files may move here once two apps need them.

use std::fmt;

/// An operating system the suite runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Linux,
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
            "unsupported operating system {:?}; only Linux is supported",
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
        // DEC-11 SPIKE EXPERIMENT: "windows" let through to see what runs
        "linux" | "windows" => Ok(Platform::Linux),
        _ => Err(Unsupported { os }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_linux_is_supported() {
        assert_eq!(platform_of("linux"), Ok(Platform::Linux));
        let err = platform_of("macos").unwrap_err();
        assert_eq!(err, Unsupported { os: "macos" });
        assert!(err.to_string().contains("\"macos\""));
    }
}
