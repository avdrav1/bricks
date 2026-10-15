//! The format bar over the page (SLATE-21, ADR 0008): the paragraph style, font, size,
//! bold, italic, underline, strikethrough, colour, and alignment. It shows the looks at
//! the caret or across the selection ([`Page::looks`]), blank where the selection mixes
//! them; showing them never formats anything. Each change the user makes applies to the
//! selection, or with none to what's typed next (the paragraph's style and alignment to
//! the caret's paragraph), as one undo step, and gives the page the keyboard back.
//! Plain text has no bar. It follows Brick's format bar (STYLE-6).

use crate::keys::{self, Does};
use crate::view::{Command, Page};
use document_edit::{LookPatch, Looks, SizeChange, MAX_SIZE, MIN_SIZE};
use document_model::{Align, Rgb, Style};
use gtk::prelude::*;
use gtk::{gdk, glib, pango};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The largest size a style's name is drawn at in the style list, in points.
const LIST_SIZE: f64 = 16.0;
/// What the colour button shows for automatic text: the page's ink.
const INK: Rgb = [0x1E, 0x1E, 0x1E];

/// A drop-down that shows nothing while the looks it shows are mixed. GTK's drop-down
/// always keeps an item chosen, so the bar draws its button's label itself, and a pick
/// of the item it still holds is caught on the list's row.
struct Choice {
    drop: gtk::DropDown,
    /// The button's label, made by its factory.
    label: RefCell<Option<gtk::Label>>,
    blank: Cell<bool>,
}

impl Choice {
    fn new(drop: gtk::DropDown) -> Choice {
        Choice {
            drop,
            label: RefCell::new(None),
            blank: Cell::new(false),
        }
    }

    /// Choose item `at`, or show nothing.
    fn show(&self, at: Option<u32>) {
        if let Some(at) = at {
            self.drop.set_selected(at);
        }
        self.blank.set(at.is_none());
        if let Some(label) = self.label.borrow().as_ref() {
            label.set_text(&self.text(self.drop.selected_item()));
        }
    }

    /// The button's text for `item`: nothing while blank.
    fn text(&self, item: Option<glib::Object>) -> glib::GString {
        if self.blank.get() {
            glib::GString::default()
        } else {
            item_text(item)
        }
    }
}

/// A string list item's text.
fn item_text(item: Option<glib::Object>) -> glib::GString {
    item.and_downcast::<gtk::StringObject>()
        .map(|s| s.string())
        .unwrap_or_default()
}

/// The label a list item's factory gave it.
fn label_of(item: &gtk::ListItem) -> gtk::Label {
    item.child()
        .and_downcast::<gtk::Label>()
        .expect("the factory gives each item a label")
}

pub struct FormatBar {
    root: gtk::Box,
    /// The style list's styles, in the document's order.
    styles: Vec<Style>,
    style: Choice,
    fonts: gtk::StringList,
    font: Choice,
    size: gtk::SpinButton,
    bold: gtk::ToggleButton,
    italic: gtk::ToggleButton,
    underline: gtk::ToggleButton,
    strike: gtk::ToggleButton,
    color: gtk::ColorDialogButton,
    aligns: [(gtk::ToggleButton, Align); 4],
    page: glib::WeakRef<Page>,
    /// Set while the bar shows the page's looks: control changes then aren't the user's.
    showing: Cell<bool>,
    /// The size field was left empty (a mixed size): GTK shows the old value again, which
    /// isn't a change.
    blank_input: Cell<bool>,
    /// A refresh is queued for the next idle moment.
    queued: Cell<bool>,
}

/// A control's name for screen readers, and its tooltip.
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

fn rgba(rgb: Rgb) -> gdk::RGBA {
    let c = |v: u8| f32::from(v) / 255.0;
    gdk::RGBA::new(c(rgb[0]), c(rgb[1]), c(rgb[2]), 1.0)
}

fn rgb_of(c: &gdk::RGBA) -> Rgb {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    [b(c.red()), b(c.green()), b(c.blue())]
}

/// A size in points as the size field shows it: whole points without a fraction.
fn points(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}

/// A list row's label for `style`: its name in its font, size (capped), bold and italic.
fn style_label(label: &gtk::Label, style: &Style) {
    label.set_text(&style.name);
    let look = &style.run;
    let mut desc = pango::FontDescription::new();
    desc.set_family(&look.font);
    let size = (f64::from(look.size) / 2.0).min(LIST_SIZE);
    desc.set_size((size * f64::from(pango::SCALE)).round() as i32);
    if look.bold {
        desc.set_weight(pango::Weight::Bold);
    }
    if look.italic {
        desc.set_style(pango::Style::Italic);
    }
    let attrs = pango::AttrList::new();
    attrs.insert(pango::AttrFontDesc::new(&desc));
    label.set_attributes(Some(&attrs));
}

impl FormatBar {
    pub fn new(page: &Page) -> Rc<Self> {
        let sheet = page.styles();
        let styles: Vec<Style> = sheet.styles().to_vec();
        let names: Vec<&str> = styles.iter().map(|s| s.name.as_str()).collect();
        let string = || {
            gtk::PropertyExpression::new(
                gtk::StringObject::static_type(),
                None::<gtk::Expression>,
                "string",
            )
        };
        let style = gtk::DropDown::new(Some(gtk::StringList::new(&names)), Some(string()));
        style.set_size_request(150, -1);
        let style_keys = format!(
            "Paragraph style ({} to {})",
            keys::label(Does::Command(Command::Style(0))),
            keys::label(Does::Command(Command::Style(6)))
        );
        named(&style, "Style", &style_keys);
        // A drop-down's accessible name is the item it shows: its description says what
        // it is.
        style.update_property(&[gtk::accessible::Property::Description("Style")]);

        let mut families: Vec<String> = page
            .pango_context()
            .font_map()
            .map(|m| {
                m.list_families()
                    .iter()
                    .map(|f| f.name().to_string())
                    .collect()
            })
            .unwrap_or_default();
        // A font the document names but this machine lacks still shows its name.
        families.extend(page.fonts());
        families.sort_unstable_by_key(|f| f.to_lowercase());
        families.dedup();
        let fonts = gtk::StringList::new(&families.iter().map(String::as_str).collect::<Vec<_>>());
        let font = gtk::DropDown::new(Some(fonts.clone()), Some(string()));
        font.set_enable_search(true);
        font.set_search_match_mode(gtk::StringFilterMatchMode::Substring);
        font.set_size_request(170, -1);
        named(&font, "Font", "Font family (type to search)");
        font.update_property(&[gtk::accessible::Property::Description("Font")]);

        let size =
            gtk::SpinButton::with_range(f64::from(MIN_SIZE) / 2.0, f64::from(MAX_SIZE) / 2.0, 1.0);
        size.set_digits(1);
        size.set_width_chars(4);
        size.set_update_policy(gtk::SpinButtonUpdatePolicy::IfValid);
        let size_keys = format!(
            "Font size ({} and {})",
            keys::label(Does::Command(Command::Size(true))),
            keys::label(Does::Command(Command::Size(false)))
        );
        named(&size, "Size", &size_keys);

        let tip = |text: &str, command: Command| keys::tip(text, Does::Command(command));
        let bold = toggle(
            "format-text-bold-symbolic",
            "Bold",
            &tip("Bold", Command::Bold),
        );
        let italic = toggle(
            "format-text-italic-symbolic",
            "Italic",
            &tip("Italic", Command::Italic),
        );
        let underline = toggle(
            "format-text-underline-symbolic",
            "Underline",
            &tip("Underline", Command::Underline),
        );
        let strike = toggle(
            "format-text-strikethrough-symbolic",
            "Strikethrough",
            "Strikethrough",
        );
        let dialog = gtk::ColorDialog::new();
        dialog.set_with_alpha(false);
        let color = gtk::ColorDialogButton::new(Some(dialog));
        named(&color, "Colour", "Text colour");
        let left = toggle(
            "format-justify-left-symbolic",
            "Align Left",
            &tip("Align left", Command::Align(Align::Start)),
        );
        let center = toggle(
            "format-justify-center-symbolic",
            "Centre",
            &tip("Centre", Command::Align(Align::Center)),
        );
        let right = toggle(
            "format-justify-right-symbolic",
            "Align Right",
            &tip("Align right", Command::Align(Align::End)),
        );
        let justify = toggle(
            "format-justify-fill-symbolic",
            "Justify",
            &tip("Justify", Command::Align(Align::Justify)),
        );
        for b in [&center, &right, &justify] {
            b.set_group(Some(&left));
        }

        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(2)
            .margin_start(6)
            .margin_end(6)
            .margin_top(4)
            .margin_bottom(4)
            .build();
        named(&root, "Format Bar", "Format");
        let bar = Rc::new(Self {
            root,
            styles,
            style: Choice::new(style),
            fonts,
            font: Choice::new(font),
            size,
            bold,
            italic,
            underline,
            strike,
            color,
            aligns: [
                (left, Align::Start),
                (center, Align::Center),
                (right, Align::End),
                (justify, Align::Justify),
            ],
            page: page.downgrade(),
            showing: Cell::new(false),
            blank_input: Cell::new(false),
            queued: Cell::new(false),
        });
        bar.lay_out();
        bar.make_lists();
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
        root.append(&self.style.drop);
        root.append(&sep());
        root.append(&self.font.drop);
        root.append(&self.size);
        root.append(&sep());
        for b in [&self.bold, &self.italic, &self.underline, &self.strike] {
            root.append(b);
        }
        root.append(&sep());
        root.append(&self.color);
        let automatic = gtk::Button::from_icon_name("edit-clear-symbolic");
        automatic.add_css_class("flat");
        named(
            &automatic,
            "Automatic Colour",
            "Text colour back to the style's",
        );
        automatic.connect_clicked({
            let bar = Rc::downgrade(self);
            move |_| {
                if let Some(bar) = bar.upgrade() {
                    bar.format(LookPatch {
                        color: Some(None),
                        ..LookPatch::default()
                    });
                }
            }
        });
        root.append(&automatic);
        root.append(&sep());
        let aligns = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        aligns.add_css_class("linked");
        named(&aligns, "Alignment", "Alignment");
        for (b, _) in &self.aligns {
            aligns.append(b);
        }
        root.append(&aligns);
    }

    /// Run `f` on the page, then give it the keyboard back.
    fn on_page(&self, f: impl FnOnce(&Page)) {
        if let Some(page) = self.page.upgrade() {
            f(&page);
            page.grab_focus();
        }
        // A change that did nothing (the same look again) shows the page's looks again.
        self.refresh();
    }

    fn format(&self, patch: LookPatch) {
        self.on_page(|page| page.format(&patch));
    }

    /// The style list's item `at` picked.
    fn pick_style(&self, at: u32) {
        if let Some(style) = self.styles.get(at as usize) {
            let id = style.id.clone();
            self.on_page(|page| page.set_style(&id));
        }
    }

    fn pick_font(&self, name: &str) {
        self.format(LookPatch {
            font: Some(Some(name.to_owned())),
            ..LookPatch::default()
        });
    }

    /// The drop-downs' factories: each button's label (see [`Choice`]), and the lists'
    /// rows (each style's name in its look), whose clicks pick the item they show even
    /// when the drop-down still holds it.
    fn make_lists(self: &Rc<Self>) {
        type Which = fn(&FormatBar) -> &Choice;
        type Row = fn(&FormatBar, &gtk::ListItem);
        // Each drop-down, how its rows draw, and what picking a row does.
        let lists: [(Which, Row, Row); 2] = [
            (
                |bar| &bar.style,
                |bar, row| {
                    if let Some(style) = bar.styles.get(row.position() as usize) {
                        style_label(&label_of(row), style);
                    }
                },
                |bar, row| bar.pick_style(row.position()),
            ),
            (
                |bar| &bar.font,
                |_, row| label_of(row).set_text(&item_text(row.item())),
                |bar, row| bar.pick_font(&item_text(row.item())),
            ),
        ];
        for (which, draw, pick) in lists {
            let rows = gtk::SignalListItemFactory::new();
            let bar = Rc::downgrade(self);
            rows.connect_setup(move |_, row| {
                let Some(row) = row.downcast_ref::<gtk::ListItem>() else {
                    return;
                };
                let label = gtk::Label::new(None);
                label.set_xalign(0.0);
                let click = gtk::GestureClick::new();
                let (bar, row_ref) = (bar.clone(), row.downgrade());
                click.connect_released(move |_, _, _, _| {
                    let (Some(bar), Some(row)) = (bar.upgrade(), row_ref.upgrade()) else {
                        return;
                    };
                    // A pick that changes the drop-down's item is its own; this is one
                    // that doesn't, while it shows nothing. After the click, so the page
                    // takes the keyboard from the closing list.
                    if which(&bar).blank.get() {
                        glib::idle_add_local_once(move || pick(&bar, &row));
                    }
                });
                label.add_controller(click);
                row.set_child(Some(&label));
            });
            let bar = Rc::downgrade(self);
            rows.connect_bind(move |_, row| {
                if let (Some(bar), Some(row)) = (bar.upgrade(), row.downcast_ref()) {
                    draw(&bar, row);
                }
            });
            let button = gtk::SignalListItemFactory::new();
            let bar = Rc::downgrade(self);
            button.connect_setup(move |_, item| {
                let (Some(bar), Some(item)) = (bar.upgrade(), item.downcast_ref::<gtk::ListItem>())
                else {
                    return;
                };
                let label = gtk::Label::new(None);
                label.set_xalign(0.0);
                label.set_ellipsize(pango::EllipsizeMode::End);
                item.set_child(Some(&label));
                which(&bar).label.replace(Some(label));
            });
            let bar = Rc::downgrade(self);
            button.connect_bind(move |_, item| {
                if let (Some(bar), Some(item)) =
                    (bar.upgrade(), item.downcast_ref::<gtk::ListItem>())
                {
                    label_of(item).set_text(&which(&bar).text(item.item()));
                }
            });
            let drop = &which(self).drop;
            drop.set_list_factory(Some(&rows));
            drop.set_factory(Some(&button));
        }
    }

    /// The handler for a control the user changed; ignored while the bar shows the looks.
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
        if let Some(page) = self.page.upgrade() {
            let bar = Rc::downgrade(self);
            page.connect_looks_changed(move || {
                if let Some(bar) = bar.upgrade() {
                    bar.queue_refresh();
                }
            });
        }
        self.style
            .drop
            .connect_selected_notify(self.on(|bar, d: &gtk::DropDown| {
                bar.pick_style(d.selected());
            }));
        self.font
            .drop
            .connect_selected_notify(self.on(|bar, d: &gtk::DropDown| {
                if let Some(s) = d.selected_item().and_downcast::<gtk::StringObject>() {
                    bar.pick_font(&s.string());
                }
            }));
        let blank = Rc::downgrade(self);
        self.size.connect_input(move |s| {
            if s.text().trim().is_empty() {
                if let Some(bar) = blank.upgrade() {
                    bar.blank_input.set(true);
                }
                return Some(Err(()));
            }
            None
        });
        self.size.connect_output(|s| {
            s.set_text(&points(s.value()));
            glib::Propagation::Stop
        });
        self.size
            .connect_value_changed(self.on(|bar, s: &gtk::SpinButton| {
                if bar.blank_input.replace(false) {
                    // Left empty: still mixed.
                    s.set_text("");
                    return;
                }
                bar.format(LookPatch {
                    size: Some(SizeChange::Set((s.value() * 2.0).round() as u16)),
                    ..LookPatch::default()
                });
            }));
        type Field = fn(&mut LookPatch) -> &mut Option<bool>;
        let fields: [(&gtk::ToggleButton, Field); 4] = [
            (&self.bold, |p| &mut p.bold),
            (&self.italic, |p| &mut p.italic),
            (&self.underline, |p| &mut p.underline),
            (&self.strike, |p| &mut p.strike),
        ];
        for (button, field) in fields {
            button.connect_toggled(self.on(move |bar, b: &gtk::ToggleButton| {
                let mut patch = LookPatch::default();
                *field(&mut patch) = Some(b.is_active());
                bar.format(patch);
            }));
        }
        self.color
            .connect_rgba_notify(self.on(|bar, b: &gtk::ColorDialogButton| {
                bar.format(LookPatch {
                    color: Some(Some(rgb_of(&b.rgba()))),
                    ..LookPatch::default()
                });
            }));
        for (button, align) in &self.aligns {
            let align = *align;
            button.connect_toggled(self.on(move |bar, b: &gtk::ToggleButton| {
                // The one turned off in the group as another turns on isn't a change.
                if b.is_active() {
                    bar.on_page(|page| page.command(Command::Align(align)));
                }
            }));
        }
    }

    /// Show the page's looks at the next idle moment, once however often it's asked.
    fn queue_refresh(self: &Rc<Self>) {
        if self.queued.replace(true) {
            return;
        }
        let bar = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(bar) = bar.upgrade() {
                bar.queued.set(false);
                bar.refresh();
            }
        });
    }

    /// Show the looks at the caret or across the selection; nothing it sets formats
    /// anything.
    pub fn refresh(&self) {
        let looks = self
            .page
            .upgrade()
            .and_then(|p| p.looks())
            .unwrap_or_default();
        self.showing.set(true);
        self.show(&looks);
        self.showing.set(false);
    }

    fn show(&self, looks: &Looks) {
        let style = looks
            .style
            .as_ref()
            .and_then(|id| self.styles.iter().position(|s| s.id == *id));
        self.style.show(style.map(|i| i as u32));
        let font = looks.font.as_ref().map(|name| {
            let n = self.fonts.n_items();
            (0..n)
                .find(|&i| self.fonts.string(i).as_deref() == Some(name.as_str()))
                .unwrap_or_else(|| {
                    self.fonts.append(name);
                    n
                })
        });
        self.font.show(font);
        self.blank_input.set(false);
        match looks.size {
            Some(size) => {
                let pt = f64::from(size) / 2.0;
                self.size.set_value(pt);
                // The same value as before doesn't show it again after a blank.
                self.size.set_text(&points(pt));
            }
            None => self.size.set_text(""),
        }
        for (button, on) in [
            (&self.bold, looks.bold),
            (&self.italic, looks.italic),
            (&self.underline, looks.underline),
            (&self.strike, looks.strike),
        ] {
            button.set_active(on == Some(true));
        }
        let color = match looks.color {
            Some(c) => rgba(c.unwrap_or(INK)),
            // Mixed: no colour.
            None => gdk::RGBA::new(0.0, 0.0, 0.0, 0.0),
        };
        self.color.set_rgba(&color);
        for (button, align) in &self.aligns {
            button.set_active(looks.align == Some(*align));
        }
    }
}
