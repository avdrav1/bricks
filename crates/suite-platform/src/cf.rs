//! The CoreFoundation calls `appearance` and `launch` share: strings and notification
//! centers. CoreFoundation is loaded in every Mac process.

#![allow(unsafe_code)]

use std::ffi::{c_char, c_void};

pub type CFTypeRef = *const c_void;
pub type CFStringRef = *const c_void;
pub type CFNotificationCenterRef = *const c_void;
pub type CFNotificationCallback = extern "C" fn(
    center: CFNotificationCenterRef,
    observer: *mut c_void,
    name: CFStringRef,
    object: *const c_void,
    user_info: CFTypeRef,
);

const UTF8: u32 = 0x0800_0100; // kCFStringEncodingUTF8
/// CFNotificationSuspensionBehaviorDeliverImmediately.
pub const DELIVER_IMMEDIATELY: isize = 4;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringCreateWithCString(
        alloc: *const c_void,
        s: *const c_char,
        encoding: u32,
    ) -> CFStringRef;
    pub fn CFNotificationCenterGetDistributedCenter() -> CFNotificationCenterRef;
    pub fn CFNotificationCenterGetLocalCenter() -> CFNotificationCenterRef;
    pub fn CFNotificationCenterAddObserver(
        center: CFNotificationCenterRef,
        observer: *const c_void,
        callback: CFNotificationCallback,
        name: CFStringRef,
        object: *const c_void,
        suspension: isize,
    );
}

/// A CFString that lives as long as the process: a constant, made once.
pub fn cf_string(s: &'static std::ffi::CStr) -> CFStringRef {
    // SAFETY: a NUL-terminated UTF-8 string; the result is owned and never released.
    unsafe { CFStringCreateWithCString(std::ptr::null(), s.as_ptr(), UTF8) }
}
