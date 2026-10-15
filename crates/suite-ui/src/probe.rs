//! PROBE (MAC-5 experiment, not for merge): log the app's launch notifications.

#![allow(unsafe_code)]

use std::ffi::{c_char, c_void};

type CFStringRef = *const c_void;
type CFNotificationCenterRef = *const c_void;

#[cfg_attr(target_os = "macos", link(name = "CoreFoundation", kind = "framework"))]
extern "C" {
    fn CFNotificationCenterGetLocalCenter() -> CFNotificationCenterRef;
    fn CFNotificationCenterAddObserver(
        center: CFNotificationCenterRef,
        observer: *const c_void,
        callback: extern "C" fn(
            CFNotificationCenterRef,
            *mut c_void,
            CFStringRef,
            *const c_void,
            *const c_void,
        ),
        name: CFStringRef,
        object: *const c_void,
        suspension: isize,
    );
    fn CFStringGetCString(s: CFStringRef, buf: *mut c_char, size: isize, encoding: u32) -> u8;
}

/// Wall-clock milliseconds, so the apps' and GTK's lines line up.
pub fn ms() -> u128 {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    now.map_or(0, |d| d.as_millis() % 1_000_000)
}

pub fn note(what: &str) {
    eprintln!("PROBE {} ms: {what}", ms());
}

extern "C" fn seen(
    _c: CFNotificationCenterRef,
    _o: *mut c_void,
    name: CFStringRef,
    _obj: *const c_void,
    _info: *const c_void,
) {
    let mut buf = [0 as c_char; 256];
    // SAFETY: a live CFString and a buffer of the given size.
    let ok = unsafe { CFStringGetCString(name, buf.as_mut_ptr(), 256, 0x0800_0100) } != 0;
    if ok {
        // SAFETY: NUL-terminated by CFStringGetCString.
        let s = unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }.to_string_lossy();
        if s.contains("Launch") || s.contains("Finish") || s.contains("Activ") {
            eprintln!("PROBE {} ms: notification {s}", ms());
        }
    }
}

pub fn observe() {
    ms();
    // SAFETY: the local center lives for the process; a NULL name observes every one.
    unsafe {
        CFNotificationCenterAddObserver(
            CFNotificationCenterGetLocalCenter(),
            std::ptr::null(),
            seen,
            std::ptr::null(),
            std::ptr::null(),
            4,
        );
    }
}

fn windows(app: &gtk::gio::Application) -> usize {
    use gtk::prelude::*;
    app.downcast_ref::<gtk::Application>()
        .map_or(0, |a| a.windows().len())
}

pub fn attach(app: &gtk::gio::Application) {
    use gtk::{glib, prelude::*};
    use std::time::Duration;
    observe();
    note("before run");
    app.connect_startup(|app| note(&format!("startup done, windows {}", windows(app))));
    app.connect_activate(|app| {
        note(&format!("activate, windows after handler {}", windows(app)));
        let a = app.clone();
        glib::idle_add_local_full(glib::Priority::DEFAULT_IDLE, move || {
            note(&format!("idle after activate, windows {}", windows(&a)));
            glib::ControlFlow::Break
        });
        for t in [0u64, 20, 100, 300, 1000, 3000] {
            let a = app.clone();
            glib::timeout_add_local_once(Duration::from_millis(t), move || {
                note(&format!("{t} ms after activate, windows {}", windows(&a)));
            });
        }
    });
    app.connect_open(|app, files, hint| {
        let names: Vec<String> = files
            .iter()
            .map(|f| {
                f.path()
                    .map_or_else(|| f.uri().to_string(), |p| p.display().to_string())
            })
            .collect();
        note(&format!(
            "open {names:?} hint {hint:?}, windows after handler {}",
            windows(app)
        ));
    });
    app.connect_shutdown(|_| note("shutdown"));
}
