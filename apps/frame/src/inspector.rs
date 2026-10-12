//! The inspector (FRAME-11; spec FR1, "No ribbon, one inspector"): one sidebar beside the
//! canvas whose rows follow the selection. With nothing selected it shows the slide's
//! layout and background; for text being edited, or a selected shape with text, the font,
//! size, bold, italic, underline, colour, alignment, and bullets of the text at the
//! selection's start; for a selected shape its fill, outline, and geometry, a picture's
//! format and size, a table's rows and columns. A value the selection inherits says where
//! it comes from (its layout, master, or the presentation).
//!
//! Each change applies at once as a `deck-edit` command in the window's undo history:
//! text looks to the text selected, or all the shape's text when none is; paragraph looks
//! to the paragraphs the selection touches. Colours come from a palette of swatches. The
//! rows follow the selection, the slide, and the deck (undo and redo too) without taking
//! the keyboard focus; a colour, font, or layout picked, or a size entered, gives it back
//! to the canvas, so typing goes on.

use crate::deck::Deck;
use crate::slide_view::SlideView;
use crate::window::DeckWindow;
use deck_edit::{
    styleable, BackgroundEdit, Edit, LayoutEdit, LookEdit, ParagraphLook, ShapeLook, ShapeLookEdit,
    ShapeRef, TextLook, TextPos,
};
use gtk::prelude::*;
use gtk::{glib, pango};
use presentation_model::{
    Align, Background, Color, Dash, Fill, Outline, ParagraphSources, RunSources, Shape, ShapeKind,
    Source,
};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

const EMU_PER_POINT: f64 = 12_700.0;
/// The sidebar's width, in pixels.
const WIDTH: i32 = 240;
/// A layout's thumbnail's width in the layout list, in pixels.
const LAYOUT_THUMB_WIDTH: i32 = 64;
/// The outline a shape without one gets when an outline colour is picked: 1 point, solid.
const OUTLINE_WIDTH: i64 = 12_700;

/// The colours the swatch popovers offer, with the names they're announced by.
const PALETTE: [(&str, Color); 20] = [
    ("Black", Color::rgb(0x00, 0x00, 0x00)),
    ("Dark grey", Color::rgb(0x40, 0x40, 0x40)),
    ("Grey", Color::rgb(0x80, 0x80, 0x80)),
    ("Light grey", Color::rgb(0xBF, 0xBF, 0xBF)),
    ("White", Color::rgb(0xFF, 0xFF, 0xFF)),
    ("Red", Color::rgb(0xC0, 0x00, 0x00)),
    ("Bright red", Color::rgb(0xFF, 0x00, 0x00)),
    ("Orange", Color::rgb(0xED, 0x7D, 0x31)),
    ("Gold", Color::rgb(0xFF, 0xC0, 0x00)),
    ("Yellow", Color::rgb(0xFF, 0xFF, 0x00)),
    ("Light green", Color::rgb(0x92, 0xD0, 0x50)),
    ("Green", Color::rgb(0x00, 0xB0, 0x50)),
    ("Teal", Color::rgb(0x00, 0x80, 0x80)),
    ("Light blue", Color::rgb(0x00, 0xB0, 0xF0)),
    ("Blue", Color::rgb(0x00, 0x70, 0xC0)),
    ("Steel blue", Color::rgb(0x44, 0x72, 0xC4)),
    ("Navy", Color::rgb(0x00, 0x20, 0x60)),
    ("Purple", Color::rgb(0x70, 0x30, 0xA0)),
    ("Pink", Color::rgb(0xFF, 0x66, 0xCC)),
    ("Brown", Color::rgb(0x83, 0x3C, 0x0B)),
];

/// What the rows show: the slide's (nothing selected), the selected text's, the selected
/// shape's. All `None`: no slides.
#[derive(Debug, Clone, PartialEq, Default)]
struct State {
    slide: Option<SlideState>,
    text: Option<TextState>,
    shape: Option<ShapeState>,
}

#[derive(Debug, Clone, PartialEq)]
struct SlideState {
    layout: Option<usize>,
    background: Look,
    background_from: Source,
}

/// A colour row's value: the colour its swatch shows (`None`: no colour) and what it
/// says.
#[derive(Debug, Clone, PartialEq)]
struct Look {
    color: Option<Color>,
    text: String,
}

#[derive(Debug, Clone, PartialEq)]
struct TextState {
    font: String,
    size: f64,
    bold: bool,
    italic: bool,
    underline: bool,
    color: Color,
    sources: RunSources,
    align: Align,
    bullets: bool,
    paragraph: ParagraphSources,
}

#[derive(Debug, Clone, PartialEq)]
struct ShapeState {
    /// The section's heading: Shape, Picture, or Table.
    heading: &'static str,
    fill: Option<(Look, Source)>,
    outline: Option<(Option<Outline>, Source)>,
    /// Read-only rows, name and value: the geometry, a picture's format and size, a
    /// table's rows and columns.
    facts: Vec<(&'static str, String)>,
}

/// A colour row's button: the colour it shows, and a popover of the palette to pick
/// from.
struct Swatch {
    button: gtk::MenuButton,
    color: Rc<Cell<Option<Color>>>,
    chip: gtk::DrawingArea,
    text: gtk::Label,
}

impl Swatch {
    /// A swatch announced as `name`; `none` names the choice of no colour, when there is
    /// one. Picking a colour calls `pick` and gives the focus back to the canvas.
    fn new(
        view: &Weak<DeckWindow>,
        name: &str,
        none: Option<&str>,
        pick: impl Fn(&DeckWindow, Option<Color>) + 'static,
    ) -> Swatch {
        let color = Rc::new(Cell::new(None));
        let chip = chip(color.clone());
        let text = gtk::Label::new(None);
        text.set_xalign(0.0);
        text.set_hexpand(true);
        text.set_ellipsize(pango::EllipsizeMode::End);
        let face = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        face.append(&chip);
        face.append(&text);
        let button = gtk::MenuButton::new();
        button.set_child(Some(&face));
        button.set_hexpand(true);
        button.set_tooltip_text(Some(name));
        button.update_property(&[gtk::accessible::Property::Label(name)]);

        let popover = gtk::Popover::new();
        let pick: Rc<dyn Fn(Option<Color>)> = {
            let view = view.clone();
            Rc::new(move |c| {
                if let Some(view) = view.upgrade() {
                    pick(&view, c);
                    back_to_canvas(&view);
                }
            })
        };
        let choose = |b: &gtk::Button, c: Option<Color>| {
            let (pick, popover) = (pick.clone(), popover.downgrade());
            b.connect_clicked(move |_| {
                if let Some(p) = popover.upgrade() {
                    p.popdown();
                }
                pick(c);
            });
        };
        let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
        if let Some(none) = none {
            let b = gtk::Button::with_label(none);
            b.add_css_class("flat");
            b.update_property(&[gtk::accessible::Property::Label(none)]);
            choose(&b, None);
            content.append(&b);
        }
        let grid = gtk::Grid::new();
        grid.set_row_spacing(2);
        grid.set_column_spacing(2);
        for (i, (name, c)) in PALETTE.iter().enumerate() {
            let label = format!("{name} {}", c.hex());
            let b = gtk::Button::new();
            b.add_css_class("flat");
            b.set_child(Some(&self::chip(Rc::new(Cell::new(Some(*c))))));
            b.set_tooltip_text(Some(&label));
            b.update_property(&[gtk::accessible::Property::Label(&label)]);
            choose(&b, Some(*c));
            grid.attach(&b, (i % 5) as i32, (i / 5) as i32, 1, 1);
        }
        content.append(&grid);
        popover.set_child(Some(&content));
        button.set_popover(Some(&popover));
        Swatch {
            button,
            color,
            chip,
            text,
        }
    }

    fn set(&self, look: &Look) {
        self.color.set(look.color);
        self.chip.queue_draw();
        self.text.set_text(&look.text);
    }
}

/// A small square of colour `color` (`None`: white, struck through).
fn chip(color: Rc<Cell<Option<Color>>>) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(18);
    area.set_content_height(18);
    area.set_valign(gtk::Align::Center);
    area.set_draw_func(move |_, cr, w, h| {
        let (w, h) = (f64::from(w), f64::from(h));
        let channel = |v: u8| f64::from(v) / 255.0;
        cr.rectangle(0.5, 0.5, w - 1.0, h - 1.0);
        match color.get() {
            Some(c) => cr.set_source_rgb(channel(c.r), channel(c.g), channel(c.b)),
            None => cr.set_source_rgb(1.0, 1.0, 1.0),
        }
        cr.fill_preserve().ok();
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.4);
        cr.set_line_width(1.0);
        cr.stroke().ok();
        if color.get().is_none() {
            cr.move_to(w - 1.0, 1.0);
            cr.line_to(1.0, h - 1.0);
            cr.set_source_rgb(0.8, 0.1, 0.1);
            cr.stroke().ok();
        }
    });
    area
}

/// The sidebar and its rows.
pub struct Inspector {
    root: gtk::ScrolledWindow,
    view: Weak<DeckWindow>,
    /// A refresh is queued.
    pending: Cell<bool>,
    /// The rows are being set to the selection's values: their handlers do nothing.
    updating: Cell<bool>,
    shown: RefCell<Option<State>>,
    nothing: gtk::Label,
    slide: gtk::Box,
    layout: gtk::DropDown,
    background: Swatch,
    background_from: gtk::Label,
    text: gtk::Box,
    fonts: gtk::StringList,
    font: gtk::DropDown,
    font_from: gtk::Label,
    size: gtk::SpinButton,
    size_from: gtk::Label,
    bold: gtk::ToggleButton,
    italic: gtk::ToggleButton,
    underline: gtk::ToggleButton,
    style_from: gtk::Label,
    color: Swatch,
    color_from: gtk::Label,
    align: [(Align, gtk::ToggleButton); 4],
    bullets: gtk::ToggleButton,
    paragraph_from: gtk::Label,
    shape: gtk::Box,
    shape_heading: gtk::Label,
    fill_row: gtk::Box,
    fill: Swatch,
    fill_from: gtk::Label,
    outline_row: gtk::Box,
    outline: Swatch,
    outline_width: gtk::SpinButton,
    outline_from: gtk::Label,
    facts: [(gtk::Box, gtk::Label, gtk::Label); 2],
}

/// A handler that calls `f` with the window, unless the rows are being set.
fn on<W>(view: &Weak<DeckWindow>, f: impl Fn(&DeckWindow, &W) + 'static) -> impl Fn(&W) {
    let view = view.clone();
    move |w| {
        if let Some(view) = view.upgrade() {
            if !view.inspector.updating.get() {
                f(&view, w);
            }
        }
    }
}

/// A section: a heading over rows.
fn section(title: &str) -> (gtk::Box, gtk::Label) {
    let heading = gtk::Label::new(Some(title));
    heading.set_xalign(0.0);
    heading.add_css_class("heading");
    let section = gtk::Box::new(gtk::Orientation::Vertical, 10);
    section.append(&heading);
    (section, heading)
}

/// A row of `section`: its name over `control`, and under it a caption saying where an
/// inherited value comes from.
fn row(section: &gtk::Box, name: &str, control: &impl IsA<gtk::Widget>) -> (gtk::Box, gtk::Label) {
    let label = gtk::Label::new(Some(name));
    label.set_xalign(0.0);
    let caption = gtk::Label::new(None);
    caption.set_xalign(0.0);
    caption.add_css_class("caption");
    caption.add_css_class("dim-label");
    caption.set_visible(false);
    let row = gtk::Box::new(gtk::Orientation::Vertical, 4);
    row.append(&label);
    row.append(control);
    row.append(&caption);
    section.append(&row);
    (row, caption)
}

/// A read-only row of `section`: a name and its value side by side.
fn fact(section: &gtk::Box) -> (gtk::Box, gtk::Label, gtk::Label) {
    let name = gtk::Label::new(None);
    name.set_xalign(0.0);
    let value = gtk::Label::new(None);
    value.set_xalign(1.0);
    value.set_hexpand(true);
    value.set_ellipsize(pango::EllipsizeMode::End);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.append(&name);
    row.append(&value);
    section.append(&row);
    (row, name, value)
}

/// A toggle button announced as `name`, showing `markup` or, without it, icon `icon`. A
/// click doesn't take the focus from the canvas.
fn toggle(name: &str, markup: Option<&str>, icon: &str) -> gtk::ToggleButton {
    let b = gtk::ToggleButton::new();
    match markup {
        Some(m) => {
            let label = gtk::Label::new(None);
            label.set_markup(m);
            b.set_child(Some(&label));
        }
        None => b.set_icon_name(icon),
    }
    b.set_focus_on_click(false);
    b.set_tooltip_text(Some(name));
    b.update_property(&[gtk::accessible::Property::Label(name)]);
    b
}

/// Buttons side by side, joined.
fn linked(buttons: &[&gtk::ToggleButton]) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    b.add_css_class("linked");
    for button in buttons {
        b.append(*button);
    }
    b
}

/// A drop-down list of `model`'s strings, its button's text ellipsized, its list's items
/// from `items`. It's labelled and described as `name` (GTK announces a drop-down by the
/// item it shows, and `name` as its description).
fn drop_down(
    model: &gtk::StringList,
    name: &str,
    search: bool,
    items: &gtk::SignalListItemFactory,
) -> gtk::DropDown {
    let expression = gtk::PropertyExpression::new(
        gtk::StringObject::static_type(),
        None::<&gtk::Expression>,
        "string",
    );
    let d = gtk::DropDown::new(Some(model.clone()), Some(expression));
    d.set_factory(Some(&labels(true)));
    d.set_list_factory(Some(items));
    d.set_enable_search(search);
    d.set_search_match_mode(gtk::StringFilterMatchMode::Substring);
    d.set_hexpand(true);
    d.set_tooltip_text(Some(name));
    d.update_property(&[
        gtk::accessible::Property::Label(name),
        gtk::accessible::Property::Description(name),
    ]);
    d
}

/// List items that show their string as a label, ellipsized or not.
fn labels(ellipsize: bool) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(move |_, item| {
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        if ellipsize {
            label.set_ellipsize(pango::EllipsizeMode::End);
        }
        if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
            item.set_child(Some(&label));
        }
    });
    factory.connect_bind(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let text = item.item().and_downcast::<gtk::StringObject>();
        if let (Some(label), Some(text)) = (item.child().and_downcast::<gtk::Label>(), text) {
            label.set_text(&text.string());
        }
    });
    factory
}

/// The layout list's items: a thumbnail of the slide each of `deck`'s layouts makes (as
/// the New Slide menu shows them), and the layout's name.
fn layout_items(deck: &Rc<Deck>) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        content.append(&label);
        if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
            item.set_child(Some(&content));
        }
    });
    let deck = deck.clone();
    factory.connect_bind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let Some(content) = item.child().and_downcast::<gtk::Box>() else {
            return;
        };
        // A recycled item: its old thumbnail goes.
        if let Some(old) = content.first_child().and_downcast::<SlideView>() {
            content.remove(&old);
        }
        let thumb = SlideView::layout(&deck, item.position() as usize, LAYOUT_THUMB_WIDTH);
        content.prepend(&thumb);
        let text = item.item().and_downcast::<gtk::StringObject>();
        let label = content.last_child().and_downcast::<gtk::Label>();
        if let (Some(label), Some(text)) = (label, text) {
            label.set_text(&text.string());
        }
    });
    factory
}

/// The font families installed, sorted, for the font list.
fn font_families(widget: &impl IsA<gtk::Widget>) -> gtk::StringList {
    let mut names: Vec<String> = widget
        .pango_context()
        .list_families()
        .iter()
        .map(|f| f.name().to_string())
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    gtk::StringList::new(&names)
}

impl Inspector {
    /// The inspector of `view`'s window, its layout list from `deck`.
    pub fn new(deck: &Rc<Deck>, view: Weak<DeckWindow>) -> Inspector {
        let v = &view;
        let nothing = gtk::Label::new(Some("No slides"));
        nothing.add_css_class("dim-label");

        let (slide, _) = section("Slide");
        let names: Vec<String> = deck
            .presentation()
            .layouts
            .iter()
            .map(|l| l.name.clone())
            .collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let layout = drop_down(
            &gtk::StringList::new(&names),
            "Slide layout",
            false,
            &layout_items(deck),
        );
        layout.connect_selected_notify(on(v, |view, d: &gtk::DropDown| {
            if let Ok(layout) = usize::try_from(d.selected()) {
                set_layout(view, layout);
                back_to_canvas(view);
            }
        }));
        row(&slide, "Layout", &layout);
        let background = Swatch::new(v, "Background colour", None, |view, c| {
            if let Some(c) = c {
                set_background(view, c);
            }
        });
        let (_, background_from) = row(&slide, "Background", &background.button);

        let (text, _) = section("Text");
        let fonts = font_families(&nothing);
        let font = drop_down(&fonts, "Font family", true, &labels(false));
        font.connect_selected_notify(on(v, |view, d: &gtk::DropDown| {
            if let Some(name) = d.selected_item().and_downcast::<gtk::StringObject>() {
                set_text(view, TextLook::Font(name.string().to_string()));
                back_to_canvas(view);
            }
        }));
        let (_, font_from) = row(&text, "Font", &font);
        let size = gtk::SpinButton::with_range(1.0, 400.0, 1.0);
        size.set_digits(1);
        size.set_hexpand(true);
        size.update_property(&[gtk::accessible::Property::Label("Font size")]);
        size.connect_value_changed(on(v, |view, s: &gtk::SpinButton| {
            set_text(view, TextLook::Size(s.value()));
        }));
        size.connect_activate(on(v, |view, _: &gtk::SpinButton| back_to_canvas(view)));
        let (_, size_from) = row(&text, "Size", &size);
        let bold = toggle("Bold", Some("<b>B</b>"), "");
        let italic = toggle("Italic", Some("<i>I</i>"), "");
        let underline = toggle("Underline", Some("<u>U</u>"), "");
        bold.connect_toggled(on(v, |view, b: &gtk::ToggleButton| {
            set_text(view, TextLook::Bold(b.is_active()))
        }));
        italic.connect_toggled(on(v, |view, b: &gtk::ToggleButton| {
            set_text(view, TextLook::Italic(b.is_active()))
        }));
        underline.connect_toggled(on(v, |view, b: &gtk::ToggleButton| {
            set_text(view, TextLook::Underline(b.is_active()))
        }));
        let (_, style_from) = row(&text, "Style", &linked(&[&bold, &italic, &underline]));
        let color = Swatch::new(v, "Text colour", None, |view, c| {
            if let Some(c) = c {
                set_text(view, TextLook::Color(c));
            }
        });
        let (_, color_from) = row(&text, "Colour", &color.button);
        let align = [
            (
                Align::Left,
                toggle("Align left", None, "format-justify-left-symbolic"),
            ),
            (
                Align::Centre,
                toggle("Align centre", None, "format-justify-center-symbolic"),
            ),
            (
                Align::Right,
                toggle("Align right", None, "format-justify-right-symbolic"),
            ),
            (
                Align::Justify,
                toggle("Justify", None, "format-justify-fill-symbolic"),
            ),
        ];
        for (_, b) in &align[1..] {
            b.set_group(Some(&align[0].1));
        }
        for (a, b) in &align {
            let a = *a;
            b.connect_toggled(on(v, move |view, b: &gtk::ToggleButton| {
                if b.is_active() {
                    set_paragraphs(view, ParagraphLook::Align(a));
                }
            }));
        }
        let bullets = toggle("Bullets", None, "view-list-bullet-symbolic");
        bullets.connect_toggled(on(v, |view, b: &gtk::ToggleButton| {
            set_paragraphs(view, ParagraphLook::Bullets(b.is_active()))
        }));
        let paragraph = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let aligns: Vec<&gtk::ToggleButton> = align.iter().map(|(_, b)| b).collect();
        paragraph.append(&linked(&aligns));
        paragraph.append(&bullets);
        let (_, paragraph_from) = row(&text, "Paragraph", &paragraph);

        let (shape, shape_heading) = section("Shape");
        let fill = Swatch::new(v, "Fill colour", Some("No fill"), |view, c| {
            let fill = c.map_or(Fill::None, Fill::Solid);
            set_shape(view, |_| Some(ShapeLook::Fill(fill)));
        });
        let (fill_row, fill_from) = row(&shape, "Fill", &fill.button);
        let outline = Swatch::new(v, "Outline colour", Some("No outline"), |view, c| {
            set_shape(view, |s| {
                Some(ShapeLook::Outline(c.map(|color| Outline {
                    color,
                    ..s.outline.unwrap_or(Outline {
                        width: OUTLINE_WIDTH,
                        color,
                        dash: Dash::Solid,
                    })
                })))
            });
        });
        let outline_width = gtk::SpinButton::with_range(0.0, 20.0, 0.25);
        outline_width.set_digits(2);
        outline_width.set_width_chars(4);
        outline_width.set_tooltip_text(Some("Outline width, points"));
        outline_width.update_property(&[gtk::accessible::Property::Label("Outline width")]);
        outline_width.connect_value_changed(on(v, |view, s: &gtk::SpinButton| {
            let width = (s.value() * EMU_PER_POINT).round() as i64;
            set_shape(view, |shape| {
                let o = shape.outline?;
                Some(ShapeLook::Outline(Some(Outline { width, ..o })))
            });
        }));
        outline_width.connect_activate(on(v, |view, _: &gtk::SpinButton| back_to_canvas(view)));
        let lines = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        lines.append(&outline.button);
        lines.append(&outline_width);
        let (outline_row, outline_from) = row(&shape, "Outline", &lines);
        let facts = [fact(&shape), fact(&shape)];

        let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
        for m in [
            gtk::Widget::set_margin_top,
            gtk::Widget::set_margin_bottom,
            gtk::Widget::set_margin_start,
            gtk::Widget::set_margin_end,
        ] {
            m(content.upcast_ref(), 12);
        }
        for part in [
            nothing.upcast_ref::<gtk::Widget>(),
            slide.upcast_ref(),
            text.upcast_ref(),
            shape.upcast_ref(),
        ] {
            part.set_visible(false);
            content.append(part);
        }
        let root = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .width_request(WIDTH)
            .child(&content)
            .build();
        Inspector {
            root,
            view,
            pending: Cell::new(false),
            updating: Cell::new(false),
            shown: RefCell::new(None),
            nothing,
            slide,
            layout,
            background,
            background_from,
            text,
            fonts,
            font,
            font_from,
            size,
            size_from,
            bold,
            italic,
            underline,
            style_from,
            color,
            color_from,
            align,
            bullets,
            paragraph_from,
            shape,
            shape_heading,
            fill_row,
            fill,
            fill_from,
            outline_row,
            outline,
            outline_width,
            outline_from,
            facts,
        }
    }

    /// The sidebar.
    pub fn widget(&self) -> &gtk::ScrolledWindow {
        &self.root
    }

    /// Whether the keyboard focus is in the inspector (its popovers too).
    pub fn has_focus(&self) -> bool {
        self.root
            .root()
            .and_then(|r| r.focus())
            .is_some_and(|f| f.is_ancestor(&self.root))
    }

    /// Show the selection's values once the current event is handled; many calls make one
    /// refresh.
    pub fn refresh(&self) {
        if self.pending.replace(true) {
            return;
        }
        let view = self.view.clone();
        glib::idle_add_local_once(move || {
            if let Some(view) = view.upgrade() {
                view.inspector.pending.set(false);
                view.inspector.update(&view);
            }
        });
    }

    fn update(&self, view: &DeckWindow) {
        let state = state(view);
        if self.shown.borrow().as_ref() == Some(&state) {
            return;
        }
        self.updating.set(true);
        self.show(&state);
        self.updating.set(false);
        self.shown.replace(Some(state));
    }

    /// Set the rows to `state`: which show, and their values. Only values change; the
    /// focus stays where it is.
    fn show(&self, state: &State) {
        self.nothing.set_visible(*state == State::default());
        self.slide.set_visible(state.slide.is_some());
        if let Some(s) = &state.slide {
            let layout = s.layout.and_then(|l| u32::try_from(l).ok());
            self.layout
                .set_selected(layout.unwrap_or(gtk::INVALID_LIST_POSITION));
            self.background.set(&s.background);
            show_from(&self.background_from, s.background_from);
        }
        self.text.set_visible(state.text.is_some());
        if let Some(t) = &state.text {
            let n = self.fonts.n_items();
            let font = (0..n)
                .find(|&i| self.fonts.string(i).is_some_and(|f| f == t.font.as_str()))
                .unwrap_or_else(|| {
                    self.fonts.append(&t.font);
                    n
                });
            self.font.set_selected(font);
            show_from(&self.font_from, t.sources.font);
            self.size.set_value(t.size);
            show_from(&self.size_from, t.sources.size);
            self.bold.set_active(t.bold);
            self.italic.set_active(t.italic);
            self.underline.set_active(t.underline);
            let s = &t.sources;
            show_from(
                &self.style_from,
                inherited(&[s.bold, s.italic, s.underline]),
            );
            self.color.set(&Look {
                color: Some(t.color),
                text: t.color.hex(),
            });
            show_from(&self.color_from, s.color);
            for (a, b) in &self.align {
                b.set_active(*a == t.align);
            }
            self.bullets.set_active(t.bullets);
            let p = &t.paragraph;
            show_from(&self.paragraph_from, inherited(&[p.align, p.bullet]));
        }
        self.shape.set_visible(state.shape.is_some());
        if let Some(s) = &state.shape {
            self.shape_heading.set_text(s.heading);
            self.fill_row.set_visible(s.fill.is_some());
            if let Some((look, from)) = &s.fill {
                self.fill.set(look);
                show_from(&self.fill_from, *from);
            }
            self.outline_row.set_visible(s.outline.is_some());
            if let Some((outline, from)) = &s.outline {
                self.outline.set(&match outline {
                    Some(o) => Look {
                        color: Some(o.color),
                        text: o.color.hex(),
                    },
                    None => Look {
                        color: None,
                        text: "No outline".into(),
                    },
                });
                self.outline_width
                    .set_value(outline.map_or(0.0, |o| o.width as f64 / EMU_PER_POINT));
                self.outline_width.set_sensitive(outline.is_some());
                show_from(&self.outline_from, *from);
            }
            for (i, (row, name, value)) in self.facts.iter().enumerate() {
                let fact = s.facts.get(i);
                row.set_visible(fact.is_some());
                if let Some((n, v)) = fact {
                    name.set_text(n);
                    value.set_text(v);
                }
            }
        }
    }
}

/// Make the inspector follow the selection on `view`'s canvas, and show it.
pub fn attach(view: &Rc<DeckWindow>) {
    view.canvas.connect_selection_changed({
        let view = Rc::downgrade(view);
        move || {
            if let Some(view) = view.upgrade() {
                view.inspector.refresh();
            }
        }
    });
    view.inspector.refresh();
}

/// Where a row's values come from when none is the selection's own (the first one's);
/// else `Source::Own`.
fn inherited(sources: &[Source]) -> Source {
    match sources.first() {
        Some(&first) if sources.iter().all(|s| !s.is_own()) => first,
        _ => Source::Own,
    }
}

/// Caption `label` with where an inherited value comes from; hide it for one set on the
/// selection itself.
fn show_from(label: &gtk::Label, source: Source) {
    let text = match source {
        Source::Layout => "From the layout",
        Source::Master => "From the master",
        Source::Presentation => "From the presentation",
        Source::Own | Source::Edit => "",
    };
    label.set_text(text);
    label.set_visible(!text.is_empty());
}

/// What the rows show for `view`'s selection: the slide's rows when no shape that takes
/// formatting is selected; else the text's, for a shape with text, and the shape's.
fn state(view: &DeckWindow) -> State {
    let deck = view.deck.presentation();
    let Some(slide) = deck.slides.get(view.canvas.index()) else {
        return State::default();
    };
    let selected = view
        .canvas
        .selected()
        .and_then(|i| Some((i, slide.shapes.get(i)?)))
        .filter(|(_, s)| styleable(s));
    let Some((i, shape)) = selected else {
        return State {
            slide: Some(SlideState {
                layout: slide.layout,
                background: background_look(&slide.background),
                background_from: slide.background_source,
            }),
            ..State::default()
        };
    };
    let text = shape.text.as_ref().and_then(|body| {
        let (from, to) = match view.canvas.editing() {
            Some(e) if e.shape == i => e.range(),
            _ => (TextPos::default(), deck_edit::text_end(body)),
        };
        let run = deck_edit::look(body, from, to)?;
        let p = body.paragraphs.get(from.paragraph)?;
        Some(TextState {
            font: run.font.clone(),
            size: run.size,
            bold: run.bold,
            italic: run.italic,
            underline: run.underline,
            color: run.color,
            sources: run.sources,
            align: p.align,
            bullets: p.bullet.is_some(),
            paragraph: p.sources,
        })
    });
    State {
        slide: None,
        text,
        shape: Some(shape_state(&view.deck, shape)),
    }
}

fn background_look(background: &Background) -> Look {
    match background {
        Background::Solid(c) => Look {
            color: Some(*c),
            text: c.hex(),
        },
        Background::Picture(_) => Look {
            color: None,
            text: "Picture".into(),
        },
        Background::Other { label, approx } => Look {
            color: Some(*approx),
            text: label.clone(),
        },
    }
}

fn fill_look(fill: &Fill) -> Look {
    match fill {
        Fill::None => Look {
            color: None,
            text: "No fill".into(),
        },
        Fill::Solid(c) => Look {
            color: Some(*c),
            text: c.hex(),
        },
        Fill::Picture(_) => Look {
            color: None,
            text: "Picture".into(),
        },
        Fill::Unsupported { label, approx } => Look {
            color: Some(*approx),
            text: label.clone(),
        },
    }
}

fn shape_state(deck: &Deck, shape: &Shape) -> ShapeState {
    let outline = Some((shape.outline, shape.sources.outline));
    match &shape.kind {
        ShapeKind::Geometry { preset, .. } => ShapeState {
            heading: "Shape",
            fill: Some((fill_look(&shape.fill), shape.sources.fill)),
            outline,
            facts: vec![("Geometry", preset.clone())],
        },
        ShapeKind::Picture(p) => {
            let pixels = deck.texture(&p.image).map_or_else(
                || "Not shown".to_owned(),
                |t| format!("{} × {}", t.width(), t.height()),
            );
            ShapeState {
                heading: "Picture",
                fill: None,
                outline,
                facts: vec![
                    ("Format", p.image.format.name().to_owned()),
                    ("Pixels", pixels),
                ],
            }
        }
        ShapeKind::Table(t) => ShapeState {
            heading: "Table",
            fill: None,
            outline: None,
            facts: vec![
                ("Rows", t.rows.len().to_string()),
                ("Columns", t.columns.len().to_string()),
            ],
        },
        ShapeKind::Unsupported { label, .. } => ShapeState {
            heading: "Shape",
            fill: None,
            outline: None,
            facts: vec![("Content", label.clone())],
        },
    }
}

/// Give the keyboard focus back to the canvas once the current event is handled (a
/// popover that closes gives it to its button first).
fn back_to_canvas(view: &DeckWindow) {
    let canvas = view.canvas.clone();
    glib::idle_add_local_once(move || {
        canvas.grab_focus();
    });
}

/// Apply `edit`, when there is one, as a step of its own. Without one (nothing changes),
/// the rows go back to the selection's values: a toggle clicked shows what the text has.
fn apply(view: &DeckWindow, edit: Option<impl Into<Edit>>) {
    match edit {
        Some(edit) => {
            view.seal();
            view.apply(edit);
        }
        None => {
            view.inspector.shown.replace(None);
            view.inspector.refresh();
        }
    }
}

/// The selected shape's text and the stretch of it a look applies to: the text selected;
/// with none, all of it for a text look (`paragraphs` false) or the caret's paragraph for
/// a paragraph look; all of it for a shape selected but not edited.
fn text_range(view: &DeckWindow, paragraphs: bool) -> Option<(ShapeRef, TextPos, TextPos)> {
    let shape = ShapeRef {
        slide: view.canvas.index(),
        shape: view.canvas.selected()?,
    };
    let deck = view.deck.presentation();
    let s = deck.slides.get(shape.slide)?.shapes.get(shape.shape)?;
    let body = s.text.as_ref().filter(|_| styleable(s))?;
    let whole = (TextPos::default(), deck_edit::text_end(body));
    let (from, to) = match view.canvas.editing() {
        Some(e) if e.shape == shape.shape && (paragraphs || e.anchor != e.caret) => e.range(),
        _ => whole,
    };
    Some((shape, from, to))
}

fn set_text(view: &DeckWindow, look: TextLook) {
    let edit = text_range(view, false)
        .and_then(|(s, from, to)| LookEdit::text(&view.deck.presentation(), s, from, to, &look));
    apply(view, edit);
}

fn set_paragraphs(view: &DeckWindow, look: ParagraphLook) {
    let edit = text_range(view, true).and_then(|(s, from, to)| {
        LookEdit::paragraphs(&view.deck.presentation(), s, from, to, look)
    });
    apply(view, edit);
}

/// Give the selected shape the look `look` makes from it.
fn set_shape(view: &DeckWindow, look: impl FnOnce(&Shape) -> Option<ShapeLook>) {
    let Some(shape) = view.canvas.selected() else {
        return;
    };
    let s = ShapeRef {
        slide: view.canvas.index(),
        shape,
    };
    let edit = {
        let deck = view.deck.presentation();
        deck.slides
            .get(s.slide)
            .and_then(|slide| slide.shapes.get(shape))
            .and_then(look)
            .and_then(|l| ShapeLookEdit::new(&deck, s, l))
    };
    apply(view, edit);
}

fn set_layout(view: &DeckWindow, layout: usize) {
    let edit = LayoutEdit::new(&view.deck.presentation(), view.canvas.index(), layout);
    apply(view, edit);
}

fn set_background(view: &DeckWindow, color: Color) {
    let edit = BackgroundEdit::new(
        &view.deck.presentation(),
        view.canvas.index(),
        Background::Solid(color),
    );
    apply(view, edit);
}
