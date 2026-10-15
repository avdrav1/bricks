//! How the apps start (MAC-5): every file reaches an app through GApplication's `open`,
//! whoever names it, and `activate` means no file.
//!
//! - Command line (`brick FILE`, a Linux file manager's `Exec=brick %f`): [`run`] hands
//!   the files to GApplication, which emits `open` with them; with none it emits
//!   `activate`.
//! - macOS (Finder, `open -a`, a file dropped on the Dock icon): Launch Services starts
//!   the app without arguments and sends an Apple Event, which GTK turns into
//!   `g_application_open` in the running process. GTK answers it only for an app with
//!   `HANDLES_OPEN` (`gtkapplication-quartz.c`); without it the spike's opens were dropped
//!   (DEC-11 C3).
//!
//! The apps stay `NON_UNIQUE`: on Linux each launch is its own process, as before; on
//! macOS Launch Services keeps one process per app and GTK delivers its opens inside it,
//! so a second `open -a` reaches the running app without D-Bus.

use gtk::{gio, glib, prelude::*};
use std::ffi::OsString;
use std::path::PathBuf;

/// Run `app` with `files` from the command line: they reach its `open` handler; without
/// any, `activate`. Relative paths are made absolute first, so no file is read as an
/// option.
pub fn run(app: &impl IsA<gio::Application>, files: &[PathBuf]) -> glib::ExitCode {
    #[cfg(target_os = "macos")]
    crate::probe::attach(app.upcast_ref());
    app.run_with_args_os(&args(files))
}

/// The arguments GApplication gets: the program, then each file as an absolute path.
fn args(files: &[PathBuf]) -> Vec<OsString> {
    let program = std::env::args_os().next().unwrap_or_default();
    std::iter::once(program)
        .chain(files.iter().map(|f| {
            std::path::absolute(f)
                .unwrap_or_else(|_| f.clone())
                .into_os_string()
        }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// What `run` makes GApplication emit for `files`: `activate`, or `open` with paths.
    fn emitted(files: &[PathBuf]) -> Vec<String> {
        let app = gio::Application::new(
            None,
            gio::ApplicationFlags::NON_UNIQUE | gio::ApplicationFlags::HANDLES_OPEN,
        );
        let seen = Rc::new(RefCell::new(Vec::new()));
        app.connect_activate({
            let seen = seen.clone();
            move |_| seen.borrow_mut().push("activate".to_owned())
        });
        app.connect_open({
            let seen = seen.clone();
            move |_, files, _| {
                for f in files {
                    let path = f.path().expect("a local file");
                    seen.borrow_mut().push(format!("open {}", path.display()));
                }
            }
        });
        assert_eq!(run(&app, files), glib::ExitCode::SUCCESS);
        seen.take()
    }

    #[test]
    fn files_reach_open_and_none_activates() {
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(emitted(&[]), ["activate"]);
        assert_eq!(
            emitted(&[
                PathBuf::from("a b.csv"),
                PathBuf::from("-x.md"),
                PathBuf::from("/tmp/c.pptx"),
            ]),
            [
                format!("open {}", cwd.join("a b.csv").display()),
                format!("open {}", cwd.join("-x.md").display()),
                "open /tmp/c.pptx".to_owned(),
            ]
        );
    }
}
