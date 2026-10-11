//! The sheet tabs under the grid (XLSX-1): one toggle button per sheet, the active one
//! pressed. Clicking a tab shows its sheet; Ctrl+Page Down / Up step through them.

use crate::grid_view::GridView;
use data_model::SheetId;
use gtk::{glib, prelude::*};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub struct SheetTabs {
    root: gtk::ScrolledWindow,
    strip: gtk::Box,
    grid: glib::WeakRef<GridView>,
    /// The layout revision the tabs show, so they are rebuilt only when sheets change.
    shown: Cell<Option<u64>>,
    buttons: RefCell<Vec<(SheetId, gtk::ToggleButton)>>,
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
        });
        tabs.refresh();
        tabs
    }

    pub fn root(&self) -> &gtk::ScrolledWindow {
        &self.root
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
        let active = grid.active_sheet();
        for (id, button) in self.buttons.borrow().iter() {
            if *id == active && !button.is_active() {
                button.set_active(true);
            }
        }
    }

    fn rebuild(self: &Rc<Self>, grid: &GridView) {
        while let Some(child) = self.strip.first_child() {
            self.strip.remove(&child);
        }
        let active = grid.active_sheet();
        let mut buttons: Vec<(SheetId, gtk::ToggleButton)> = Vec::new();
        for (id, name) in grid.sheet_list() {
            let button = gtk::ToggleButton::with_label(&name);
            button.add_css_class("flat");
            button.set_focus_on_click(false); // keep the keyboard in the grid
            if let Some((_, first)) = buttons.first() {
                button.set_group(Some(first));
            }
            button.set_active(id == active);
            button.connect_toggled({
                let (tabs, grid) = (Rc::downgrade(self), self.grid.clone());
                move |b| {
                    let (Some(tabs), Some(grid)) = (tabs.upgrade(), grid.upgrade()) else {
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
            self.strip.append(&button);
            buttons.push((id, button));
        }
        self.buttons.replace(buttons);
    }

    fn press(&self, id: SheetId) {
        let button = self
            .buttons
            .borrow()
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, b)| b.clone());
        if let Some(b) = button {
            b.set_active(true);
        }
    }
}
