//! The format bar over the formula bar (STYLE-6): font, size, bold, italic, underline,
//! strikethrough, text and fill colours, borders, alignment, wrap, number format, merge,
//! and clear formatting. Each control formats the selection (whole rows or columns when
//! selected so) as one undo step through [`GridView::format_selection`]. The bar shows
//! the cursor cell's format as the cursor moves and the data changes; showing it never
//! formats anything.

use crate::grid_view::{FontToggle, GridView};
use data_model::{BorderPreset, Color, HAlign, StylePatch, Underline, VAlign};
use gtk::{gdk, glib, prelude::*};
use std::cell::Cell;
use std::rc::Rc;

/// The number formats the bar offers, by name and code; "Custom…" asks for a code.
const NUMBER_FORMATS: [(&str, &str); 10] = [
    ("General", "General"),
    ("Number", "#,##0.00"),
    ("Currency", "[$$-409]#,##0.00"),
    (
        "Accounting",
        r#"_($* #,##0.00_);_($* (#,##0.00);_($* "-"??_);_(@_)"#,
    ),
    ("Percent", "0.00%"),
    ("Scientific", "0.00E+00"),
    ("Date", "yyyy-mm-dd"),
    ("Time", "h:mm:ss"),
    ("Date time", "yyyy-mm-dd hh:mm:ss"),
    ("Text", "@"),
];

/// The number format list's last entry.
const CUSTOM: u32 = NUMBER_FORMATS.len() as u32;

const BORDERS: [(&str, BorderPreset); 8] = [
    ("No Border", BorderPreset::None),
    ("All Borders", BorderPreset::All),
    ("Outside Borders", BorderPreset::Outside),
    ("Thick Outside Borders", BorderPreset::ThickOutside),
    ("Top Border", BorderPreset::Top),
    ("Bottom Border", BorderPreset::Bottom),
    ("Left Border", BorderPreset::Left),
    ("Right Border", BorderPreset::Right),
];

const VERTICAL: [(&str, VAlign); 3] = [
    ("Top", VAlign::Top),
    ("Middle", VAlign::Center),
    ("Bottom", VAlign::Bottom),
];

pub struct FormatBar {
    root: gtk::Box,
    fonts: gtk::StringList,
    font: gtk::DropDown,
    size: gtk::SpinButton,
    bold: gtk::ToggleButton,
    italic: gtk::ToggleButton,
    underline: gtk::ToggleButton,
    strike: gtk::ToggleButton,
    color: gtk::ColorDialogButton,
    fill: gtk::ColorDialogButton,
    left: gtk::ToggleButton,
    center: gtk::ToggleButton,
    right: gtk::ToggleButton,
    wrap: gtk::ToggleButton,
    number: gtk::DropDown,
    custom: gtk::Popover,
    custom_entry: gtk::Entry,
    custom_note: gtk::Label,
    grid: glib::WeakRef<GridView>,
    /// Set while the bar shows the cursor cell's format: control changes then aren't
    /// the user's.
    showing: Cell<bool>,
}

/// A control's name for screen readers and its tooltip with the shortcut, if any.
fn named(widget: &(impl IsA<gtk::Widget> + IsA<gtk::Accessible>), name: &str, tooltip: &str) {
    widget.set_tooltip_text(Some(tooltip));
    widget.update_property(&[gtk::accessible::Property::Label(name)]);
}

fn toggle(icon: &str, name: &str, tooltip: &str) -> gtk::ToggleButton {
    let b = gtk::ToggleButton::new();
    b.set_icon_name(icon);
    b.add_css_class("flat");
    named(&b, name, tooltip);
    b
}

fn rgba(rgb: u32) -> gdk::RGBA {
    let c = |shift: u32| ((rgb >> shift) & 0xFF) as f32 / 255.0;
    gdk::RGBA::new(c(16), c(8), c(0), 1.0)
}

fn rgb_of(c: &gdk::RGBA) -> u32 {
    let b = |v: f32| ((v.clamp(0.0, 1.0) * 255.0).round() as u32) & 0xFF;
    (b(c.red()) << 16) | (b(c.green()) << 8) | b(c.blue())
}

/// A menu button whose popover lists `items` as flat buttons.
fn menu<T: Copy + 'static>(
    label: &str,
    tooltip: &str,
    items: &[(&str, T)],
    pick: impl Fn(T) + 'static,
) -> gtk::MenuButton {
    let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let popover = gtk::Popover::new();
    popover.set_child(Some(&list));
    let pick = Rc::new(pick);
    for &(text, value) in items {
        let item = gtk::Button::with_label(text);
        item.add_css_class("flat");
        if let Some(l) = item.child().and_downcast::<gtk::Label>() {
            l.set_xalign(0.0);
        }
        item.connect_clicked({
            let (pick, popover) = (pick.clone(), popover.downgrade());
            move |_| {
                if let Some(p) = popover.upgrade() {
                    p.popdown();
                }
                pick(value);
            }
        });
        list.append(&item);
    }
    let button = gtk::MenuButton::builder()
        .label(label)
        .popover(&popover)
        .build();
    button.add_css_class("flat");
    named(&button, label, tooltip);
    button
}

impl FormatBar {
    pub fn new(grid: &GridView) -> Rc<Self> {
        let mut families: Vec<String> = grid
            .pango_context()
            .font_map()
            .map(|m| {
                m.list_families()
                    .iter()
                    .map(|f| f.name().to_string())
                    .collect()
            })
            .unwrap_or_default();
        families.sort_unstable_by_key(|f| f.to_lowercase());
        families.dedup();
        let fonts = gtk::StringList::new(&families.iter().map(String::as_str).collect::<Vec<_>>());
        let expression = gtk::PropertyExpression::new(
            gtk::StringObject::static_type(),
            None::<gtk::Expression>,
            "string",
        );
        let font = gtk::DropDown::new(Some(fonts.clone()), Some(expression));
        font.set_enable_search(true);
        font.set_size_request(170, -1);
        named(&font, "Font", "Font");
        let size = gtk::SpinButton::with_range(1.0, 409.0, 1.0);
        size.set_digits(1);
        size.set_width_chars(4);
        named(&size, "Font Size", "Font size");
        let bold = toggle("format-text-bold-symbolic", "Bold", "Bold (Ctrl+B)");
        let italic = toggle("format-text-italic-symbolic", "Italic", "Italic (Ctrl+I)");
        let underline = toggle(
            "format-text-underline-symbolic",
            "Underline",
            "Underline (Ctrl+U)",
        );
        let strike = toggle(
            "format-text-strikethrough-symbolic",
            "Strikethrough",
            "Strikethrough (Ctrl+5)",
        );
        let colors = || {
            let dialog = gtk::ColorDialog::new();
            dialog.set_with_alpha(false);
            gtk::ColorDialogButton::new(Some(dialog))
        };
        let color = colors();
        named(&color, "Text Colour", "Text colour");
        let fill = colors();
        named(&fill, "Fill Colour", "Fill colour");
        let left = toggle("format-justify-left-symbolic", "Align Left", "Align left");
        let center = toggle("format-justify-center-symbolic", "Centre", "Centre");
        let right = toggle(
            "format-justify-right-symbolic",
            "Align Right",
            "Align right",
        );
        let wrap = gtk::ToggleButton::with_label("Wrap");
        wrap.add_css_class("flat");
        named(&wrap, "Wrap Text", "Wrap text");
        let mut names: Vec<&str> = NUMBER_FORMATS.iter().map(|(n, _)| *n).collect();
        names.push("Custom…");
        let number = gtk::DropDown::from_strings(&names);
        named(&number, "Number Format", "Number format (Ctrl+1: custom)");
        let custom_entry = gtk::Entry::new();
        custom_entry.set_width_chars(28);
        named(
            &custom_entry,
            "Custom Number Format",
            "An Excel number format code",
        );
        let custom_note = gtk::Label::new(None);
        custom_note.add_css_class("error");
        custom_note.set_visible(false);
        let custom_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        custom_box.append(&gtk::Label::new(Some("Number format code")));
        custom_box.append(&custom_entry);
        custom_box.append(&custom_note);
        let custom = gtk::Popover::new();
        custom.set_child(Some(&custom_box));
        custom.set_parent(&number);

        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(2)
            .margin_start(6)
            .margin_end(6)
            .margin_top(4)
            .build();
        let bar = Rc::new(Self {
            root,
            fonts,
            font,
            size,
            bold,
            italic,
            underline,
            strike,
            color,
            fill,
            left,
            center,
            right,
            wrap,
            number,
            custom,
            custom_entry,
            custom_note,
            grid: grid.downgrade(),
            showing: Cell::new(false),
        });
        bar.lay_out();
        bar.connect();
        bar.refresh();
        bar
    }

    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    fn lay_out(self: &Rc<Self>) {
        let sep = || gtk::Separator::new(gtk::Orientation::Vertical);
        let root = &self.root;
        root.append(&self.font);
        root.append(&self.size);
        root.append(&sep());
        for b in [&self.bold, &self.italic, &self.underline, &self.strike] {
            root.append(b);
        }
        root.append(&sep());
        root.append(&self.color);
        root.append(&self.fill);
        let bar = Rc::downgrade(self);
        root.append(&menu("Borders", "Borders", &BORDERS, move |preset| {
            if let Some(bar) = bar.upgrade() {
                bar.apply(StylePatch {
                    border: Some(preset),
                    ..StylePatch::default()
                });
            }
        }));
        root.append(&sep());
        for b in [&self.left, &self.center, &self.right] {
            root.append(b);
        }
        let bar = Rc::downgrade(self);
        root.append(&menu(
            "Vertical",
            "Vertical alignment",
            &VERTICAL,
            move |v| {
                if let Some(bar) = bar.upgrade() {
                    bar.apply(StylePatch {
                        vertical: Some(v),
                        ..StylePatch::default()
                    });
                }
            },
        ));
        root.append(&self.wrap);
        root.append(&sep());
        root.append(&self.number);
        root.append(&sep());
        let grid_button = |label: &str, tooltip: &str, run: fn(&GridView)| {
            let b = gtk::Button::with_label(label);
            b.add_css_class("flat");
            named(&b, label, tooltip);
            let grid = self.grid.clone();
            b.connect_clicked(move |_| {
                if let Some(g) = grid.upgrade() {
                    run(&g);
                    g.grab_focus();
                }
            });
            b
        };
        root.append(&grid_button(
            "Merge",
            "Merge cells",
            GridView::merge_selection,
        ));
        root.append(&grid_button(
            "Unmerge",
            "Unmerge cells",
            GridView::unmerge_selection,
        ));
        let clear = gtk::Button::from_icon_name("edit-clear-all-symbolic");
        clear.add_css_class("flat");
        named(&clear, "Clear Formatting", "Clear formatting");
        clear.connect_clicked({
            let bar = Rc::downgrade(self);
            move |_| {
                if let Some(bar) = bar.upgrade() {
                    bar.apply(StylePatch {
                        clear: true,
                        ..StylePatch::default()
                    });
                }
            }
        });
        root.append(&clear);
    }

    /// Format the selection with `patch`, then give the grid the keyboard back.
    fn apply(&self, patch: StylePatch) {
        if let Some(g) = self.grid.upgrade() {
            g.format_selection(&patch);
            g.grab_focus();
        }
    }

    /// The handler for a control the user changed; ignored while the bar shows a cell.
    fn on<W: 'static>(self: &Rc<Self>, run: impl Fn(&Self, &W) + 'static) -> impl Fn(&W) + 'static {
        let bar = Rc::downgrade(self);
        move |w| {
            if let Some(bar) = bar.upgrade() {
                if !bar.showing.get() {
                    run(&bar, w);
                }
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
        self.font
            .connect_selected_notify(self.on(|bar, d: &gtk::DropDown| {
                let Some(name) = d
                    .selected_item()
                    .and_downcast::<gtk::StringObject>()
                    .map(|s| s.string())
                else {
                    return;
                };
                bar.apply(StylePatch {
                    font_name: Some(name.as_str().into()),
                    ..StylePatch::default()
                });
            }));
        self.size
            .connect_value_changed(self.on(|bar, s: &gtk::SpinButton| {
                bar.apply(StylePatch {
                    font_size: Some((s.value() * 100.0).round() as u32),
                    ..StylePatch::default()
                });
            }));
        for (button, which) in [
            (&self.bold, FontToggle::Bold),
            (&self.italic, FontToggle::Italic),
            (&self.underline, FontToggle::Underline),
            (&self.strike, FontToggle::Strike),
        ] {
            button.connect_toggled(self.on(move |bar, b: &gtk::ToggleButton| {
                let on = b.is_active();
                let patch = match which {
                    FontToggle::Bold => StylePatch {
                        bold: Some(on),
                        ..StylePatch::default()
                    },
                    FontToggle::Italic => StylePatch {
                        italic: Some(on),
                        ..StylePatch::default()
                    },
                    FontToggle::Underline => StylePatch {
                        underline: Some(if on {
                            Underline::Single
                        } else {
                            Underline::None
                        }),
                        ..StylePatch::default()
                    },
                    FontToggle::Strike => StylePatch {
                        strike: Some(on),
                        ..StylePatch::default()
                    },
                };
                bar.apply(patch);
            }));
        }
        self.color
            .connect_rgba_notify(self.on(|bar, b: &gtk::ColorDialogButton| {
                bar.apply(StylePatch {
                    font_color: Some(Color::Rgb(rgb_of(&b.rgba()))),
                    ..StylePatch::default()
                });
            }));
        self.fill
            .connect_rgba_notify(self.on(|bar, b: &gtk::ColorDialogButton| {
                bar.apply(StylePatch {
                    fill: Some(Some(Color::Rgb(rgb_of(&b.rgba())))),
                    ..StylePatch::default()
                });
            }));
        for (button, align) in [
            (&self.left, HAlign::Left),
            (&self.center, HAlign::Center),
            (&self.right, HAlign::Right),
        ] {
            // Pressing the alignment a cell has takes it back to General.
            button.connect_toggled(self.on(move |bar, b: &gtk::ToggleButton| {
                bar.apply(StylePatch {
                    horizontal: Some(if b.is_active() {
                        align
                    } else {
                        HAlign::General
                    }),
                    ..StylePatch::default()
                });
            }));
        }
        self.wrap
            .connect_toggled(self.on(|bar, b: &gtk::ToggleButton| {
                bar.apply(StylePatch {
                    wrap: Some(b.is_active()),
                    ..StylePatch::default()
                });
            }));
        self.number
            .connect_selected_notify(self.on(|bar, d: &gtk::DropDown| {
                match NUMBER_FORMATS.get(d.selected() as usize) {
                    Some((_, code)) => bar.apply(StylePatch {
                        num_fmt: Some((*code).into()),
                        ..StylePatch::default()
                    }),
                    None => bar.open_custom(),
                }
            }));
        self.custom_entry.connect_activate({
            let bar = Rc::downgrade(self);
            move |entry| {
                let Some(bar) = bar.upgrade() else { return };
                let code = entry.text().to_string();
                let ok = code.eq_ignore_ascii_case("General")
                    || formula::NumberFormat::parse(&code).is_ok();
                if !ok {
                    bar.custom_note
                        .set_text("Excel doesn't accept this number format code");
                    bar.custom_note.set_visible(true);
                    return;
                }
                bar.custom.popdown();
                bar.apply(StylePatch {
                    num_fmt: Some(code.into()),
                    ..StylePatch::default()
                });
            }
        });
        // Closed without a code: the list shows the cell's format again.
        self.custom.connect_closed({
            let bar = Rc::downgrade(self);
            move |_| {
                if let Some(bar) = bar.upgrade() {
                    bar.refresh();
                }
            }
        });
    }

    /// Ctrl+1, or "Custom…": a popover to type a number format code in, the cursor
    /// cell's code to start from.
    pub fn open_custom(&self) {
        if let Some(g) = self.grid.upgrade() {
            g.finish_editing();
            self.custom_entry.set_text(&g.cursor_format().code);
        }
        self.custom_note.set_visible(false);
        self.custom.popup();
        self.custom_entry.grab_focus();
        self.custom_entry.select_region(0, -1);
    }

    /// Show the cursor cell's format; nothing it sets formats anything.
    pub fn refresh(&self) {
        let Some(grid) = self.grid.upgrade() else {
            return;
        };
        let f = grid.cursor_format();
        self.showing.set(true);
        let n = self.fonts.n_items();
        let at = (0..n).find(|&i| self.fonts.string(i).as_deref() == Some(&*f.font.name));
        let at = at.unwrap_or_else(|| {
            // A font the file names but this machine lacks still shows its name.
            self.fonts.append(&f.font.name);
            n
        });
        self.font.set_selected(at);
        self.size.set_value(f64::from(f.font.size) / 100.0);
        self.bold.set_active(f.font.bold);
        self.italic.set_active(f.font.italic);
        self.underline
            .set_active(f.font.underline != Underline::None);
        self.strike.set_active(f.font.strike);
        self.color.set_rgba(&rgba(f.color.unwrap_or(0x000000)));
        self.fill.set_rgba(&rgba(f.fill.unwrap_or(0xFFFFFF)));
        let h = f.xf.align.horizontal;
        self.left.set_active(h == HAlign::Left);
        self.center
            .set_active(matches!(h, HAlign::Center | HAlign::CenterContinuous));
        self.right.set_active(h == HAlign::Right);
        self.wrap.set_active(f.xf.align.wrap);
        let number = NUMBER_FORMATS
            .iter()
            .position(|(_, code)| *code == f.code)
            .map_or(CUSTOM, |i| i as u32);
        self.number.set_selected(number);
        self.showing.set(false);
    }
}
