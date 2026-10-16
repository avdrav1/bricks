//! The terminal an app was started from (WIN-3). On Windows the apps are GUI-subsystem
//! programs, so no console window opens with them, and they have no console of their own:
//! [`attach`] attaches to the console of the process that started them, if it has one
//! (`cmd`, PowerShell, Windows Terminal), so `--version`, usage errors, and logs print
//! there. Output redirected to a file or a pipe stays where it was sent. The shell doesn't
//! wait for a GUI program, so what it prints may follow the next prompt.
//!
//! Elsewhere [`attach`] does nothing: a program has its terminal already.

/// Print to the terminal the app was started from. Call first in `main`, before anything
/// prints.
pub fn attach() {
    #[cfg(windows)]
    windows::attach();
}

#[cfg(windows)]
mod windows {
    #![allow(unsafe_code)]

    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{
        AttachConsole, GetStdHandle, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE,
        STD_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };

    const STREAMS: [STD_HANDLE; 3] = [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE];

    pub(super) fn attach() {
        // SAFETY: handle queries and replacements of this process's own standard handles.
        unsafe {
            // A stream the starter redirected comes as a handle; keep it.
            let given = STREAMS.map(|stream| GetStdHandle(stream));
            if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
                // No console to attach to (started from Explorer), or one already.
                return;
            }
            for (stream, handle) in STREAMS.into_iter().zip(given) {
                if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
                    SetStdHandle(stream, handle);
                }
            }
        }
    }
}
