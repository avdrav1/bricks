//! The Mac's light or dark appearance (MAC-6).
//!
//! The appearance is the global `AppleInterfaceStyle` preference: `Dark` in dark mode,
//! absent in light mode. macOS keeps it current when the appearance is Auto too, and posts
//! the distributed notification `AppleInterfaceThemeChangedNotification` on each change.
//! Both are read through CoreFoundation, which every Mac process already loads.

#![allow(unsafe_code)]

use crate::cf::{
    cf_string, CFNotificationCenterAddObserver, CFNotificationCenterGetDistributedCenter,
    CFNotificationCenterRef, CFStringRef, CFTypeRef, DELIVER_IMMEDIATELY,
};
use std::ffi::c_void;
use std::sync::{LazyLock, OnceLock};

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFPreferencesAnyApplication: CFStringRef;
    fn CFPreferencesAppSynchronize(app: CFStringRef) -> u8;
    fn CFPreferencesCopyAppValue(key: CFStringRef, app: CFStringRef) -> CFTypeRef;
    fn CFEqual(a: CFTypeRef, b: CFTypeRef) -> u8;
    fn CFRelease(cf: CFTypeRef);
}

/// Whether the Mac is in dark mode now.
pub fn is_dark() -> bool {
    // CFStrings as addresses: raw pointers aren't `Sync`, CFStrings are immutable.
    static KEY: LazyLock<usize> = LazyLock::new(|| cf_string(c"AppleInterfaceStyle") as usize);
    static DARK: LazyLock<usize> = LazyLock::new(|| cf_string(c"Dark") as usize);
    let (key, dark) = (*KEY as CFStringRef, *DARK as CFStringRef);
    // SAFETY: CoreFoundation calls on live strings; the copied value is released.
    unsafe {
        // Another process (System Settings) changed it: drop this process's cached copy.
        CFPreferencesAppSynchronize(kCFPreferencesAnyApplication);
        let value = CFPreferencesCopyAppValue(key, kCFPreferencesAnyApplication);
        if value.is_null() {
            return false;
        }
        let is = CFEqual(value, dark) != 0;
        CFRelease(value);
        is
    }
}

static ON_CHANGE: OnceLock<fn(bool)> = OnceLock::new();

extern "C" fn changed(
    _center: CFNotificationCenterRef,
    _observer: *mut c_void,
    _name: CFStringRef,
    _object: *const c_void,
    _user_info: CFTypeRef,
) {
    if let Some(on_change) = ON_CHANGE.get() {
        on_change(is_dark());
    }
}

/// Call `on_change` with whether the Mac is dark now and again on every change of
/// appearance, from the main thread's run loop (GTK's main loop runs it). Once per
/// process: later calls do nothing.
pub fn follow(on_change: fn(bool)) {
    if ON_CHANGE.set(on_change).is_err() {
        return;
    }
    on_change(is_dark());
    // SAFETY: the distributed center lives for the process; the name is never released
    // and the callback is a plain function. Delivered at once even while the app is in the
    // background, so a window behind others still changes with the system.
    unsafe {
        CFNotificationCenterAddObserver(
            CFNotificationCenterGetDistributedCenter(),
            std::ptr::null(),
            changed,
            cf_string(c"AppleInterfaceThemeChangedNotification"),
            std::ptr::null(),
            DELIVER_IMMEDIATELY,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_is_what_defaults_reads() {
        // `defaults read -g AppleInterfaceStyle` prints `Dark` in dark mode and fails
        // (no such key) in light mode.
        let out = std::process::Command::new("defaults")
            .args(["read", "-g", "AppleInterfaceStyle"])
            .output()
            .expect("defaults runs");
        let dark = out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "Dark";
        assert_eq!(is_dark(), dark);
    }
}
