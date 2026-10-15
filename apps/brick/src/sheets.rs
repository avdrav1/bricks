//! The sheet tabs under the grid (XLSX-1, SHEET-1): one toggle button per sheet, the
//! active one pressed, then an Add Sheet button. Clicking a tab shows its sheet; Ctrl+Page
//! Down / Up step through them (⌃Page Down / Up on macOS too; `keys.rs`). Right-clicking a
//! tab offers Rename…, Move Left, Move Right, and Delete; double-clicking it, or F2 on it,
//! renames it in place.

use crate::grid_view::GridView;
use crate::keys::{self, Command};
use data_model::SheetId;
use gtk::{gdk, gio, glib, prelude::*};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub struct SheetTabs {
    root: gtk::ScrolledWindow,
    strip: gtk::Box,
    grid: glib::WeakRef<GridView>,
    /// The layout revision the tabs show, so they are rebuilt only when sheets change.
    shown: Cell<Option<u64>>,
    buttons: RefCell<Vec<(SheetId, gtk::ToggleButton)>>,
    /// The tab a right-click menu was opened on, for its actions.
    target: Cell<Option<SheetId>>,
    menu: RefCell<Option<gtk::PopoverMenu>>,
    rename: RefCell<Option<gtk::Popover>>,
    actions: gio::SimpleActionGroup,
}

impl SheetTabs {
    pub fn new(grid: &GridView) -> Rc<Self> {
        let strip = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        strip.set_margin_start(6);
        strip.set_margin_end(6);
        let root = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .child(&strip)
            .build();
        root.set_hexpand(true);
        let tabs = Rc::new(Self {
            root,
            strip,
            grid: grid.downgrade(),
            shown: Cell::new(None),
            buttons: RefCell::new(Vec::new()),
            target: Cell::new(None),
            menu: RefCell::new(None),
            rename: RefCell::new(None),
            actions: gio::SimpleActionGroup::new(),
        });
        tabs.add_actions();
        tabs.root.insert_action_group("sheet", Some(&tabs.actions));
        tabs.refresh();
        tabs
    }

    pub fn root(&self) -> &gtk::ScrolledWindow {
        &self.root
    }

    fn add_actions(self: &Rc<Self>) {
        type Run = fn(&Rc<SheetTabs>, &GridView, SheetId);
        let entries: [(&str, Run); 4] = [
            ("rename", |tabs, _, id| tabs.start_rename(id)),
            ("move-left", |_, grid, id| {
                if let Some(at) = position(grid, id) {
                    grid.move_sheet(id, at.saturating_sub(1));
                }
            }),
            ("move-right", |_, grid, id| {
                if let Some(at) = position(grid, id) {
                    grid.move_sheet(id, at + 1);
                }
            }),
            ("delete", |_, grid, id| {
                grid.delete_sheet(id);
            }),
        ];
        for (name, run) in entries {
            let action = gio::SimpleAction::new(name, None);
            action.connect_activate({
                let tabs = Rc::downgrade(self);
                move |_, _| {
                    let Some(tabs) = tabs.upgrade() else { return };
                    let (Some(grid), Some(id)) = (tabs.grid.upgrade(), tabs.target.get()) else {
                        return;
                    };
                    run(&tabs, &grid, id);
                    tabs.refresh();
                }
            });
            self.actions.add_action(&action);
        }
    }

    /// Show the grid's sheets: rebuilt when sheets were added, removed, renamed, or moved;
    /// the active one pressed.
    pub fn refresh(self: &Rc<Self>) {
        let Some(grid) = self.grid.upgrade() else {
            return;
        };
        let revision = grid.layout_revision();
        if self.shown.get() != Some(revision) {
            self.shown.set(Some(revision));
            self.rebuild(&grid);
        }
        self.press(grid.active_sheet());
    }

    fn rebuild(self: &Rc<Self>, grid: &GridView) {
        // Popovers belong to the old buttons.
        for popover in [
            self.rename.take().map(|p| p.upcast::<gtk::Widget>()),
            self.menu.take().map(|m| m.upcast::<gtk::Widget>()),
        ]
        .into_iter()
        .flatten()
        {
            popover.unparent();
        }
        while let Some(child) = self.strip.first_child() {
            self.strip.remove(&child);
        }
        let active = grid.active_sheet();
        let mut buttons: Vec<(SheetId, gtk::ToggleButton)> = Vec::new();
        for (id, name) in grid.sheet_list() {
            let button = self.tab(id, &name);
            if let Some((_, first)) = buttons.first() {
                button.set_group(Some(first));
            }
            button.set_active(id == active);
            self.strip.append(&button);
            buttons.push((id, button));
        }
        let add = gtk::Button::from_icon_name("list-add-symbolic");
        add.add_css_class("flat");
        add.set_tooltip_text(Some(&format!(
            "Add Sheet ({})",
            keys::label(Command::NewSheet)
        )));
        add.set_focus_on_click(false);
        add.connect_clicked({
            let tabs = Rc::downgrade(self);
            move |_| {
                let Some(tabs) = tabs.upgrade() else { return };
                if let Some(grid) = tabs.grid.upgrade() {
                    grid.add_sheet();
                    grid.grab_focus();
                }
                tabs.refresh();
            }
        });
        self.strip.append(&add);
        self.buttons.replace(buttons);
    }

    /// One sheet's tab: click shows it, double-click or F2 renames it, right-click opens
    /// its menu.
    fn tab(self: &Rc<Self>, id: SheetId, name: &str) -> gtk::ToggleButton {
        let button = gtk::ToggleButton::with_label(name);
        button.add_css_class("flat");
        button.set_focus_on_click(false); // keep the keyboard in the grid
        button.connect_toggled({
            let tabs = Rc::downgrade(self);
            move |b| {
                let Some(tabs) = tabs.upgrade() else { return };
                let Some(grid) = tabs.grid.upgrade() else {
                    return;
                };
                if !b.is_active() || grid.active_sheet() == id {
                    return;
                }
                if grid.switch_sheet(id) {
                    grid.grab_focus();
                } else {
                    // Refused (a job runs): the current sheet's tab stays pressed.
                    tabs.press(grid.active_sheet());
                }
            }
        });
        let double = gtk::GestureClick::builder().button(1).build();
        double.connect_pressed({
            let tabs = Rc::downgrade(self);
            move |_, n, _, _| {
                if let (2, Some(tabs)) = (n, tabs.upgrade()) {
                    tabs.start_rename(id);
                }
            }
        });
        button.add_controller(double);
        let context = gtk::GestureClick::builder().button(3).build();
        context.connect_pressed({
            let tabs = Rc::downgrade(self);
            move |gesture, _, x, y| {
                let Some(tabs) = tabs.upgrade() else { return };
                if let Some(widget) = gesture.widget() {
                    tabs.open_menu(id, &widget, x, y);
                }
            }
        });
        button.add_controller(context);
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed({
            let tabs = Rc::downgrade(self);
            move |_, key, _, _| {
                if key != gdk::Key::F2 {
                    return glib::Propagation::Proceed;
                }
                if let Some(tabs) = tabs.upgrade() {
                    tabs.start_rename(id);
                }
                glib::Propagation::Stop
            }
        });
        button.add_controller(keys);
        button
    }

    fn button(&self, id: SheetId) -> Option<gtk::ToggleButton> {
        self.buttons
            .borrow()
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, b)| b.clone())
    }

    fn press(&self, id: SheetId) {
        if let Some(b) = self.button(id).filter(|b| !b.is_active()) {
            b.set_active(true);
        }
    }

    /// The tab's menu, its actions enabled as they apply: no Delete for the only sheet, no
    /// Move past either end.
    fn open_menu(&self, id: SheetId, widget: &gtk::Widget, x: f64, y: f64) {
        let Some(grid) = self.grid.upgrade() else {
            return;
        };
        self.target.set(Some(id));
        let (count, at) = (grid.sheet_count(), position(&grid, id).unwrap_or(0));
        for (name, on) in [
            ("rename", true),
            ("delete", count > 1),
            ("move-left", at > 0),
            ("move-right", at + 1 < count),
        ] {
            if let Some(a) = self.actions.lookup_action(name) {
                a.downcast::<gio::SimpleAction>()
                    .expect("our actions are simple")
                    .set_enabled(on);
            }
        }
        if let Some(old) = self.menu.take() {
            old.unparent();
        }
        let model = gio::Menu::new();
        model.append(Some("Rename…"), Some("sheet.rename"));
        model.append(Some("Move Left"), Some("sheet.move-left"));
        model.append(Some("Move Right"), Some("sheet.move-right"));
        model.append(Some("Delete"), Some("sheet.delete"));
        let menu = gtk::PopoverMenu::from_model(Some(&model));
        menu.set_has_arrow(false);
        menu.set_parent(widget);
        menu.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        menu.popup();
        self.menu.replace(Some(menu));
    }

    /// Rename sheet `id` in a popover on its tab: Enter applies (a name Excel refuses
    /// says why and stays open), Escape closes.
    fn start_rename(self: &Rc<Self>, id: SheetId) {
        let Some(button) = self.button(id) else {
            return;
        };
        if let Some(old) = self.rename.take() {
            old.unparent();
        }
        let entry = gtk::Entry::new();
        entry.set_text(&button.label().unwrap_or_default());
        let error = gtk::Label::new(None);
        error.add_css_class("error");
        error.set_wrap(true);
        error.set_visible(false);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
        content.append(&entry);
        content.append(&error);
        let popover = gtk::Popover::builder().child(&content).build();
        popover.set_parent(&button);
        entry.connect_activate({
            let (tabs, popover, error) = (Rc::downgrade(self), popover.downgrade(), error.clone());
            move |entry| {
                let Some(tabs) = tabs.upgrade() else { return };
                let Some(grid) = tabs.grid.upgrade() else {
                    return;
                };
                match grid.rename_sheet(id, &entry.text()) {
                    Ok(()) => {
                        if let Some(p) = popover.upgrade() {
                            p.popdown();
                        }
                        grid.grab_focus();
                        tabs.refresh();
                    }
                    Err(why) => {
                        error.set_text(why);
                        error.set_visible(true);
                    }
                }
            }
        });
        popover.popup();
        entry.grab_focus();
        entry.select_region(0, -1);
        self.rename.replace(Some(popover));
    }
}

fn position(grid: &GridView, id: SheetId) -> Option<usize> {
    grid.sheet_list().iter().position(|(i, _)| *i == id)
}
