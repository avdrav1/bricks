//! File → Open Recent (ETCH-22): the last ten images opened or saved, newest first, across
//! runs. The list is the suite's ([`suite_file::recent`], as Brick's), kept in `recent` in
//! Etch's state folder ([`suite_platform::dirs`]: `~/.local/state/etch/recent` on Linux).
//! The submenu shows each image by its name and its folder; it's read again whenever a
//! window becomes active (another `etch` may have changed the list) and after every
//! change. Reading and writing the list run off the UI thread.

use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use suite_file::recent::{self, Recent};

thread_local! {
    /// The Open Recent submenu: an item a file.
    static MENU: gio::Menu = gio::Menu::new();
    /// The files it shows, so an unchanged list leaves it be.
    static SHOWN: RefCell<Option<Vec<PathBuf>>> = const { RefCell::new(None) };
}

/// `recent` in Etch's state folder, as Brick's.
fn store() -> PathBuf {
    suite_platform::dirs::state_dir(crate::APP).join("recent")
}

/// The Open Recent submenu, filled once the list is read.
pub fn menu() -> gio::Menu {
    refresh();
    MENU.with(Clone::clone)
}

/// Read the list again and show it.
pub fn refresh() {
    update(|_| Ok(()));
}

/// `path` was opened or saved: it goes to the top of the list, once.
pub fn add(path: &Path) {
    let path = path.to_owned();
    update(move |r| r.add(&path));
}

/// `path` can't be opened: it leaves the list.
pub fn remove(path: &Path) {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    update(move |r| r.remove(&path));
}

/// Runs `change` on the list off the UI thread, then shows the list. The app runs until
/// it's written; a failure costs only the list.
fn update(change: impl FnOnce(&mut Recent) -> std::io::Result<()> + Send + 'static) {
    let hold = gio::Application::default().map(|app| app.hold());
    glib::spawn_future_local(async move {
        let _hold = hold;
        let files = gio::spawn_blocking(move || {
            let mut list = Recent::load_from(store());
            if let Err(e) = change(&mut list) {
                tracing::warn!("cannot update the recent files: {e}");
            }
            list.files().to_vec()
        })
        .await;
        if let Ok(files) = files {
            show(files);
        }
    });
}

fn show(files: Vec<PathBuf>) {
    if SHOWN.with_borrow(|shown| shown.as_ref() == Some(&files)) {
        return;
    }
    MENU.with(|menu| {
        menu.remove_all();
        for file in &files {
            // A path that isn't UTF-8 can't travel in the action's string.
            let Some(path) = file.to_str() else { continue };
            let item = gio::MenuItem::new(Some(&label(file)), None);
            item.set_action_and_target_value(Some("app.open-recent"), Some(&path.to_variant()));
            menu.append_item(&item);
        }
        if menu.n_items() == 0 {
            // No such action: the item shows greyed out.
            menu.append(Some("No recent images"), Some("app.no-recent"));
        }
    });
    SHOWN.set(Some(files));
}

/// The item's label: the file's name, then its folder with the home folder as `~`, each
/// underscore doubled (a single one would mark a mnemonic).
fn label(file: &Path) -> String {
    let (name, dir) = recent::label(file);
    format!("{name}  ({dir})").replace('_', "__")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_show_the_name_and_folder_keeping_underscores() {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        assert_eq!(
            label(&home.join("Pictures/cat.png")),
            "cat.png  (~/Pictures)"
        );
        assert_eq!(label(Path::new("/tmp/my_shot.png")), "my__shot.png  (/tmp)");
    }

    #[test]
    fn the_list_lives_in_etchs_state_folder() {
        let folder = if cfg!(target_os = "macos") {
            "Library/Application Support/dev.cronkite.Etch/recent"
        } else {
            "etch/recent"
        };
        assert!(store().ends_with(folder), "{}", store().display());
    }
}
