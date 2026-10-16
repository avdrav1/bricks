//! Windows' title bars, dark or light with the app (WIN-3). The system draws them (GTK
//! draws none there: `chrome::init`), light unless a window asks for the dark one with
//! `DWMWA_USE_IMMERSIVE_DARK_MODE`. So every window is told as it's realized, and every
//! window again when the mode changes.

#![allow(unsafe_code)]

use gtk::glib::translate::ToGlibPtr;
use gtk::{gdk, gio, prelude::*};
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use windows_sys::core::BOOL;
use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};

/// The attribute's number before Windows 10 20H1 (build 19041), which took 19.
const DARK_MODE_BEFORE_20H1: u32 = 19;

extern "C" {
    // GDK's Win32 backend (in GTK's DLL, which the gtk4 crates link).
    fn gdk_win32_surface_get_handle(surface: *mut gdk::ffi::GdkSurface) -> *mut c_void;
}

thread_local! {
    /// The mode set last; `None` until the first.
    static DARK: Cell<Option<bool>> = const { Cell::new(None) };
    /// GTK's list of windows, watched for new ones.
    static TOPLEVELS: RefCell<Option<gio::ListModel>> = const { RefCell::new(None) };
}

/// Draw every window's title bar dark or light, and each new window's from now on. On the
/// main thread.
pub(super) fn set_dark(dark: bool) {
    if DARK.replace(Some(dark)).is_none() {
        watch();
    }
    for window in gtk::Window::list_toplevels() {
        if let Ok(window) = window.downcast::<gtk::Window>() {
            paint(&window, dark);
        }
    }
}

/// Each window GTK adds gets its title bar when it's realized, before it shows.
fn watch() {
    let toplevels = gtk::Window::toplevels();
    toplevels.connect_items_changed(|model, position, _removed, added| {
        for i in position..position + added {
            let Some(window) = model.item(i).and_downcast::<gtk::Window>() else {
                continue;
            };
            window.connect_realize(|window| {
                if let Some(dark) = DARK.get() {
                    paint(window, dark);
                }
            });
        }
    });
    TOPLEVELS.set(Some(toplevels));
}

/// Tell the system how to draw `window`'s title bar, if it has its window yet.
fn paint(window: &gtk::Window, dark: bool) {
    let Some(surface) = window.surface() else {
        return;
    };
    if !surface.type_().name().starts_with("GdkWin32") {
        return;
    }
    // SAFETY: a live GDK Win32 surface; GDK owns the window handle it returns.
    let hwnd = unsafe { gdk_win32_surface_get_handle(surface.to_glib_none().0) };
    if hwnd.is_null() {
        return;
    }
    let value = BOOL::from(dark);
    for attribute in [DWMWA_USE_IMMERSIVE_DARK_MODE as u32, DARK_MODE_BEFORE_20H1] {
        // SAFETY: a window of this process and a BOOL of the size given.
        let result = unsafe {
            DwmSetWindowAttribute(
                hwnd,
                attribute,
                (&value as *const BOOL).cast(),
                size_of::<BOOL>() as u32,
            )
        };
        if result >= 0 {
            return;
        }
    }
    tracing::warn!("appearance: the title bar's mode can't be set");
}
