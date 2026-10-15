//! The formula bar and name box over the grid (FML-9). The name box shows the selection
//! (`B2`, `B2:D9`) and goes to a reference typed there (`C5`, `Sheet2!B3`); the formula
//! entry shows the cursor cell's raw text, its formula rather than its result, and Enter
//! sets the cell as one undo step. A note under them says why a formula is refused, or
//! that a result came from the file.

use crate::grid_view::GridView;
use crate::keys::{self, Command};
use gtk::{gdk, glib, prelude::*};
use std::rc::Rc;

pub struct FormulaBar {
    root: gtk::Box,
    name: gtk::Entry,
    input: gtk::Entry,
    note: gtk::Label,
    grid: glib::WeakRef<GridView>,
}

impl FormulaBar {
    pub fn new(grid: &GridView) -> Rc<Self> {
        let name = gtk::Entry::new();
        name.set_width_chars(14);
        name.set_max_width_chars(14);
        name.set_tooltip_text(Some(&format!("Name box ({})", keys::label(Command::GoTo))));
        let fx = gtk::Label::new(Some("ƒx"));
        fx.add_css_class("dim-label");
        let input = gtk::Entry::new();
        input.set_hexpand(true);
        input.set_tooltip_text(Some("Formula bar"));
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.append(&name);
        row.append(&fx);
        row.append(&input);
        let note = gtk::Label::new(None);
        note.set_xalign(0.0);
        note.set_wrap(true);
        note.set_visible(false);
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .margin_start(6)
            .margin_end(6)
            .margin_top(4)
            .margin_bottom(4)
            .build();
        root.append(&row);
        root.append(&note);
        let bar = Rc::new(Self {
            root,
            name,
            input,
            note,
            grid: grid.downgrade(),
        });
        bar.connect();
        bar.refresh();
        bar
    }

    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Ctrl+G / F5: the name box, its text selected, ready for a reference.
    pub fn focus_name(&self) {
        self.name.grab_focus();
        self.name.select_region(0, -1);
    }

    /// A message under the bar: an error (red) or a remark (dim); empty hides it.
    pub fn say(&self, text: &str, error: bool) {
        self.note.set_text(text);
        self.note.set_visible(!text.is_empty());
        if error {
            self.note.remove_css_class("dim-label");
            self.note.add_css_class("error");
        } else {
            self.note.remove_css_class("error");
            self.note.add_css_class("dim-label");
        }
    }

    /// Show the grid's selection and cursor cell; the entries keep what is being typed.
    pub fn refresh(&self) {
        let Some(grid) = self.grid.upgrade() else {
            return;
        };
        if !self.name.has_focus() {
            let text = grid.selection_reference();
            if self.name.text() != text {
                self.name.set_text(&text);
            }
        }
        if !self.input.has_focus() {
            let raw = grid.cell_raw();
            if self.input.text() != raw {
                self.input.set_text(&raw);
            }
            match grid.cell_note() {
                Some(note) => self.say(&note, false),
                None => self.say("", false),
            }
        }
    }

    fn connect(self: &Rc<Self>) {
        let Some(grid) = self.grid.upgrade() else {
            return;
        };
        for signal in ["cursor-changed", "changed"] {
            let bar = Rc::downgrade(self);
            grid.connect_local(signal, false, move |_| {
                if let Some(bar) = bar.upgrade() {
                    bar.refresh();
                }
                None
            });
        }
        let bar = Rc::downgrade(self);
        grid.connect_local("formula-error", false, move |args| {
            let message: String = args[1].get().unwrap_or_default();
            if let Some(bar) = bar.upgrade() {
                bar.say(&message, true);
            }
            None
        });
        self.input.connect_activate({
            let bar = Rc::downgrade(self);
            move |input| {
                let Some(bar) = bar.upgrade() else { return };
                let Some(grid) = bar.grid.upgrade() else {
                    return;
                };
                match grid.set_cell_text(&input.text()) {
                    Ok(()) => {
                        grid.grab_focus();
                        bar.refresh();
                    }
                    Err(why) => bar.say(&why, true),
                }
            }
        });
        self.name.connect_activate({
            let bar = Rc::downgrade(self);
            move |name| {
                let Some(bar) = bar.upgrade() else { return };
                let Some(grid) = bar.grid.upgrade() else {
                    return;
                };
                let result = formula::parse_reference(&name.text())
                    .ok_or("Not a cell reference")
                    .and_then(|r| grid.select_reference(&r));
                match result {
                    Ok(()) => {
                        bar.say("", false);
                        bar.refresh();
                    }
                    Err(why) => bar.say(why, true),
                }
            }
        });
        // Escape: back to the grid, the entries showing the cell again.
        for entry in [&self.input, &self.name] {
            let keys = gtk::EventControllerKey::new();
            keys.connect_key_pressed({
                let bar = Rc::downgrade(self);
                move |_, key, _, _| {
                    if key != gdk::Key::Escape {
                        return glib::Propagation::Proceed;
                    }
                    let Some(bar) = bar.upgrade() else {
                        return glib::Propagation::Proceed;
                    };
                    if let Some(grid) = bar.grid.upgrade() {
                        grid.grab_focus();
                    }
                    bar.refresh();
                    glib::Propagation::Stop
                }
            });
            entry.add_controller(keys);
        }
    }
}
