//! Frame's menus in macOS's menu bar (MAC-4): File, Edit, Insert, Slide, and View, each
//! item with its key from [`crate::keys`]; Linux has the header bar's buttons instead.
//! The window's actions they run are `crate::window`'s, `crate::slide_list`'s, and
//! `crate::insert`'s.

use crate::keys::{self, Action};
use deck_edit::SHAPES;
use gtk::gio;
use gtk::prelude::*;
use suite_ui::keys::Primary;

/// An item running `action`, showing the key that does `key`.
fn item(label: &str, action: &str, key: Option<Action>) -> gio::MenuItem {
    let item = gio::MenuItem::new(Some(label), Some(action));
    if let Some(accel) = key.and_then(|k| keys::accel_for(Primary::HERE, k)) {
        item.set_attribute_value("accel", Some(&accel.to_variant()));
    }
    item
}

/// A menu of `items`, a section each.
fn menu(sections: &[&[(&str, &str, Option<Action>)]]) -> gio::Menu {
    let menu = gio::Menu::new();
    for items in sections {
        let section = gio::Menu::new();
        for &(label, action, key) in *items {
            section.append_item(&item(label, action, key));
        }
        menu.append_section(None, &section);
    }
    menu
}

/// The menus: File, Edit, Insert, Slide, View.
pub fn menus() -> [(&'static str, gio::Menu); 5] {
    use Action as A;
    let file = menu(&[
        &[
            ("New", "app.new", Some(A::New)),
            ("Open…", "app.open", Some(A::Open)),
        ],
        &[
            ("Close Window", "app.close", Some(A::Close)),
            ("Save", "win.save", Some(A::Save)),
            ("Save As…", "win.save-as", Some(A::SaveAs)),
        ],
    ]);
    let edit = menu(&[&[
        ("Undo", "win.undo", Some(A::Undo)),
        ("Redo", "win.redo", Some(A::Redo)),
    ]]);
    let insert = gio::Menu::new();
    insert.append_item(&item("Text", "win.insert-text", None));
    let shapes = gio::Menu::new();
    for (preset, name) in SHAPES {
        let shape = gio::MenuItem::new(Some(name), None);
        shape.set_action_and_target_value(Some("win.insert-shape"), Some(&preset.to_variant()));
        shapes.append_item(&shape);
    }
    insert.append_submenu(Some("Shape"), &shapes);
    insert.append_item(&item("Picture…", "win.insert-picture", None));
    insert.append_item(&item("Table", "win.insert-table", None));
    let slide = menu(&[
        &[
            ("New Slide", "win.new-slide", Some(A::NewSlide)),
            ("Duplicate Slide", "win.duplicate-slide", Some(A::Duplicate)),
            ("Delete Slide", "win.delete-slide", None),
        ],
        &[
            ("Move Slide Up", "win.move-slide-up", Some(A::MoveUp)),
            ("Move Slide Down", "win.move-slide-down", Some(A::MoveDown)),
        ],
    ]);
    let view = menu(&[
        &[("Outline", "win.outline", Some(A::Outline))],
        &[
            ("Slideshow", "win.slideshow", Some(A::Slideshow)),
            (
                "Slideshow from This Slide",
                "win.slideshow-here",
                Some(A::SlideshowHere),
            ),
        ],
    ]);
    [
        ("File", file),
        ("Edit", edit),
        ("Insert", insert),
        ("Slide", slide),
        ("View", view),
    ]
}

/// The menu bar: the menus and macOS's Window menu.
pub fn bar() -> gio::Menu {
    let menus = menus();
    let refs: Vec<(&str, &gio::Menu)> = menus.iter().map(|(l, m)| (*l, m)).collect();
    suite_ui::chrome::menu_bar(&refs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use suite_ui::chrome::items;

    #[test]
    fn every_item_shows_its_key_from_the_table() {
        let bar = bar();
        let all = items(&bar);
        let accel = |label: &str| {
            all.iter()
                .find(|(l, _, _)| l == label)
                .unwrap_or_else(|| panic!("no {label}"))
                .2
                .clone()
        };
        let p = Primary::HERE;
        assert_eq!(accel("Save"), Some(p.accel("<Primary>s")));
        assert_eq!(accel("Save As…"), Some(p.accel("<Primary><Shift>s")));
        assert_eq!(accel("Undo"), Some(p.accel("<Primary>z")));
        assert_eq!(accel("Redo"), Some(p.accel("<Primary><Shift>z")));
        assert_eq!(accel("Outline"), Some(p.accel("<Primary><Shift>o")));
        assert_eq!(accel("Slideshow"), Some("F5".to_owned()));
        assert_eq!(accel("New Slide"), keys::accel_for(p, Action::NewSlide));
        // Close Window's key is macOS's alone.
        assert_eq!(
            accel("Close Window"),
            (p == Primary::Command).then(|| "<Meta>w".to_owned())
        );
        // Every shape is in Insert → Shape.
        for (_, name) in SHAPES {
            assert!(all
                .iter()
                .any(|(l, a, _)| l == name && a.as_deref() == Some("win.insert-shape")));
        }
        let top: Vec<_> = (0..bar.n_items())
            .filter_map(|i| {
                bar.item_attribute_value(i, "label", Some(gtk::glib::VariantTy::STRING))
                    .and_then(|v| v.get::<String>())
            })
            .collect();
        let mut want = vec!["File", "Edit", "Insert", "Slide", "View"];
        if suite_ui::chrome::MENU_BAR {
            want.push("Window");
        }
        assert_eq!(top, want);
    }
}
