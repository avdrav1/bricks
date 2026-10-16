//! Windows' app mode (WIN-3): Settings → Personalization → Colors → "Choose your app
//! mode" writes the user's registry value
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize\AppsUseLightTheme`,
//! 0 for dark and 1 for light (absent until it's first chosen: light). A thread waits on
//! the registry for changes (`RegNotifyChangeKeyValue`), so a change shows at once, from
//! Settings or from anything else that writes the value.

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegGetValueW, RegNotifyChangeKeyValue, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER,
    KEY_NOTIFY, REG_NOTIFY_CHANGE_LAST_SET, REG_NOTIFY_CHANGE_NAME, RRF_RT_REG_DWORD,
};

/// The key holding the value, under `HKEY_CURRENT_USER`.
const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
/// The value: 0 is dark.
const VALUE: &str = "AppsUseLightTheme";
/// The key watched: `Personalize`'s parent, which every profile has, so a `Personalize`
/// made later is seen too.
const THEMES: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes";

/// `s` as a NUL-terminated UTF-16 string.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Whether the apps are in dark mode now.
pub fn is_dark() -> bool {
    let (key, value) = (wide(PERSONALIZE), wide(VALUE));
    let mut data = 1u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: NUL-terminated names and a DWORD-sized buffer whose size is given.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut data as *mut u32).cast::<c_void>(),
            &mut size,
        )
    };
    status == ERROR_SUCCESS && data == 0
}

static FOLLOWING: AtomicBool = AtomicBool::new(false);

/// Call `on_change` with whether the apps are dark now and again on every change, from a
/// thread of its own (the caller hands it to its main loop). Once per process: later
/// calls do nothing.
pub fn follow(on_change: fn(bool)) {
    if FOLLOWING.swap(true, Ordering::Relaxed) {
        return;
    }
    let dark = is_dark();
    on_change(dark);
    // Without the thread the mode is read at startup only.
    let _ = std::thread::Builder::new()
        .name("appearance".into())
        .spawn(move || watch(dark, on_change));
}

/// Wait for changes under `Themes` and call `on_change` when the mode is no longer `dark`.
fn watch(mut dark: bool, on_change: fn(bool)) {
    let path = wide(THEMES);
    let mut key: HKEY = std::ptr::null_mut();
    // SAFETY: a NUL-terminated name; the key handle is written to `key`.
    let status =
        unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, KEY_NOTIFY, &mut key) };
    if status != ERROR_SUCCESS {
        return;
    }
    loop {
        // SAFETY: an open key; synchronous, so it returns once a value or key under it is
        // set, added, or deleted.
        let status = unsafe {
            RegNotifyChangeKeyValue(
                key,
                1,
                REG_NOTIFY_CHANGE_NAME | REG_NOTIFY_CHANGE_LAST_SET,
                std::ptr::null_mut(),
                0,
            )
        };
        if status != ERROR_SUCCESS {
            break;
        }
        let now = is_dark();
        if now != dark {
            dark = now;
            on_change(now);
        }
    }
    // SAFETY: the key opened above.
    unsafe { RegCloseKey(key) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_is_what_reg_reads() {
        // `reg query` prints `AppsUseLightTheme    REG_DWORD    0x0` in dark mode and
        // fails when the value is missing (light).
        let out = std::process::Command::new("reg")
            .args(["query", &format!(r"HKCU\{PERSONALIZE}"), "/v", VALUE])
            .output()
            .expect("reg runs");
        let text = String::from_utf8_lossy(&out.stdout);
        let dark = out.status.success()
            && text
                .lines()
                .find(|l| l.contains(VALUE))
                .and_then(|l| l.split_whitespace().last())
                .is_some_and(|v| v == "0x0");
        assert_eq!(is_dark(), dark, "{text}");
    }

    #[test]
    fn names_are_nul_terminated_utf16() {
        assert_eq!(wide("Ab"), [0x41, 0x62, 0]);
    }
}
