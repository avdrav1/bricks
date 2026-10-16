//! When the app has finished launching (MAC-5), so an app opens its window for no file
//! only if the system had no file for it.
//!
//! macOS: a file opened from Finder or `open -a` arrives after GApplication's `activate`.
//! Launch Services starts the app without arguments, GTK emits `activate`, and only then
//! does its main loop handle the Apple Event that opens the file. Cocoa posts
//! `NSApplicationDidFinishLaunchingNotification` right after that event, or at once
//! without one: on the runner (MAC-5's runs) it came 50 ms after `activate`, after the
//! file's `open`, when Launch Services had a file, and a few milliseconds after `activate`
//! without one, from `open -a` or a terminal alike. [`when_finished`] waits for it.
//!
//! Linux: a launch's files come with it (`open` instead of `activate`), so nothing waits.

/// Run `f` once the app has finished launching: on macOS after the launch's own files have
/// opened; elsewhere now. Call it on the main thread, from `activate` at the latest: GTK
/// emits `activate` before its main loop runs, and the notification comes from there.
pub fn when_finished(f: Box<dyn FnOnce()>) {
    #[cfg(target_os = "macos")]
    mac::when_finished(f);
    #[cfg(not(target_os = "macos"))]
    f();
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod mac {
    use crate::cf::{
        cf_string, CFNotificationCenterAddObserver, CFNotificationCenterGetLocalCenter,
        CFNotificationCenterRef, CFStringRef, CFTypeRef, DELIVER_IMMEDIATELY,
    };
    use std::cell::{Cell, RefCell};
    use std::ffi::c_void;

    /// What waits for the launch to finish.
    type Waiting = Vec<Box<dyn FnOnce()>>;

    thread_local! {
        static FINISHED: Cell<bool> = const { Cell::new(false) };
        /// `None` until the first wait observes the notification.
        static WAITING: RefCell<Option<Waiting>> = const { RefCell::new(None) };
    }

    extern "C" fn finished(
        _center: CFNotificationCenterRef,
        _observer: *mut c_void,
        _name: CFStringRef,
        _object: *const c_void,
        _user_info: CFTypeRef,
    ) {
        FINISHED.set(true);
        for f in WAITING.take().unwrap_or_default() {
            f();
        }
    }

    pub fn when_finished(f: Box<dyn FnOnce()>) {
        if FINISHED.get() {
            return f();
        }
        let observing = WAITING.with_borrow_mut(|waiting| {
            let observing = waiting.is_some();
            waiting.get_or_insert_with(Vec::new).push(f);
            observing
        });
        if !observing {
            // SAFETY: the local center lives for the process; the name is never released
            // and the callback is a plain function. AppKit posts the notification on the
            // main thread, which the waiting closures belong to.
            unsafe {
                CFNotificationCenterAddObserver(
                    CFNotificationCenterGetLocalCenter(),
                    std::ptr::null(),
                    finished,
                    cf_string(c"NSApplicationDidFinishLaunchingNotification"),
                    std::ptr::null(),
                    DELIVER_IMMEDIATELY,
                );
            }
        }
    }
}

#[cfg(all(test, not(target_os = "macos")))]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn linux_runs_it_at_once() {
        let ran = Rc::new(Cell::new(false));
        when_finished(Box::new({
            let ran = ran.clone();
            move || ran.set(true)
        }));
        assert!(ran.get());
    }
}
