//! The system's light or dark mode, on macOS (MAC-6) and Windows (WIN-3). The apps don't
//! rely on GTK to follow it (on macOS it doesn't: DEC-11 C10), so each app reads it when
//! it starts and on every change and sets its theme from it
//! (`suite_ui::appearance::set_dark`). Linux has no counterpart here: GTK, or Brick's
//! portal reader, follows the desktop's preference there.
//!
//! [`is_dark`] says whether the system is dark now; [`follow`] calls a function with it at
//! once and again on every change.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{follow, is_dark};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{follow, is_dark};
