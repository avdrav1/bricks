//! The colour row: the foreground and background colours, and the palette. A click on a
//! palette colour makes it the foreground, a right-click the background (docs/specs/etch.md).

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib, graphene};
use image_model::Color;
use std::rc::Rc;

/// Paint's familiar colours, two rows of ten: the darker row, then the lighter.
pub const PALETTE: [(&str, Color); 20] = [
    ("Black", Color::rgb(0x00, 0x00, 0x00)),
    ("Grey", Color::rgb(0x7F, 0x7F, 0x7F)),
    ("Dark Red", Color::rgb(0x88, 0x00, 0x15)),
    ("Red", Color::rgb(0xED, 0x1C, 0x24)),
    ("Orange", Color::rgb(0xFF, 0x7F, 0x27)),
    ("Yellow", Color::rgb(0xFF, 0xF2, 0x00)),
    ("Green", Color::rgb(0x22, 0xB1, 0x4C)),
    ("Turquoise", Color::rgb(0x00, 0xA2, 0xE8)),
    ("Indigo", Color::rgb(0x3F, 0x48, 0xCC)),
    ("Purple", Color::rgb(0xA3, 0x49, 0xA4)),
    ("White", Color::rgb(0xFF, 0xFF, 0xFF)),
    ("Light Grey", Color::rgb(0xC3, 0xC3, 0xC3)),
    ("Brown", Color::rgb(0xB9, 0x7A, 0x57)),
    ("Rose", Color::rgb(0xFF, 0xAE, 0xC9)),
    ("Gold", Color::rgb(0xFF, 0xC9, 0x0E)),
    ("Light Yellow", Color::rgb(0xEF, 0xE4, 0xB0)),
    ("Lime", Color::rgb(0xB5, 0xE6, 0x1D)),
    ("Light Turquoise", Color::rgb(0x99, 0xD9, 0xEA)),
    ("Blue Grey", Color::rgb(0x70, 0x92, 0xBE)),
    ("Lavender", Color::rgb(0xC8, 0xBF, 0xE7)),
];

/// A swatch's side in the palette.
const SWATCH_PX: i32 = 16;
/// The foreground and background squares' side in the indicator.
const SQUARE_PX: f32 = 20.0;
const EDGE: gdk::RGBA = gdk::RGBA::new(0.0, 0.0, 0.0, 0.45);

/// `#RRGGBB`, with `AA` after it when the colour isn't opaque.
pub fn hex(c: Color) -> String {
    if c.a == 255 {
        format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b)
    } else {
        format!("#{:02X}{:02X}{:02X}{:02X}", c.r, c.g, c.b, c.a)
    }
}

/// The colour `text` names in hex: `#RGB` (each digit doubled), `#RRGGBB` (opaque), or
/// `#RRGGBBAA`, in either case, the `#` optional, spaces around ignored. Anything else is
/// `None`.
pub fn parse_hex(text: &str) -> Option<Color> {
    let digits = text.trim();
    let digits = digits.strip_prefix('#').unwrap_or(digits);
    if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let at = |i: usize, n: usize| u8::from_str_radix(&digits[i..i + n], 16).ok();
    match digits.len() {
        3 => Some(Color::rgb(at(0, 1)? * 17, at(1, 1)? * 17, at(2, 1)? * 17)),
        6 => Some(Color::rgb(at(0, 2)?, at(2, 2)?, at(4, 2)?)),
        8 => Some(Color::rgba(at(0, 2)?, at(2, 2)?, at(4, 2)?, at(6, 2)?)),
        _ => None,
    }
}

/// The colour GDK gives (a colour chooser's), each channel to the nearest of 256 levels.
pub fn from_rgba(c: &gdk::RGBA) -> Color {
    let level = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color::rgba(
        level(c.red()),
        level(c.green()),
        level(c.blue()),
        level(c.alpha()),
    )
}

/// The colour as GDK draws it.
pub fn rgba(c: Color) -> gdk::RGBA {
    gdk::RGBA::new(
        c.r as f32 / 255.0,
        c.g as f32 / 255.0,
        c.b as f32 / 255.0,
        c.a as f32 / 255.0,
    )
}

/// A colour in `rect` with a thin edge; a translucent one over a checkerboard.
fn draw_colour(snapshot: &gtk::Snapshot, rect: &graphene::Rect, c: Color) {
    snapshot.append_color(&EDGE, rect);
    let inner = rect.inset_r(1.0, 1.0);
    if c.a < 255 {
        let half = graphene::Size::new(inner.width() / 2.0, inner.height() / 2.0);
        snapshot.append_color(&gdk::RGBA::WHITE, &inner);
        let grey = gdk::RGBA::new(0.8, 0.8, 0.8, 1.0);
        snapshot.append_color(
            &grey,
            &graphene::Rect::new(inner.x(), inner.y(), half.width(), half.height()),
        );
        snapshot.append_color(
            &grey,
            &graphene::Rect::new(
                inner.x() + half.width(),
                inner.y() + half.height(),
                half.width(),
                half.height(),
            ),
        );
    }
    snapshot.append_color(&rgba(c), &inner);
}

/// Palette buttons as small as their swatches.
pub fn install_style() {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let css = gtk::CssProvider::new();
    css.load_from_string("button.swatch { padding: 1px; min-width: 0; min-height: 0; }");
    gtk::style_context_add_provider_for_display(
        &display,
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

/// The palette: a button a colour. `pick(colour, background)` gets a left click's colour
/// with `false` and a right-click's with `true`.
pub fn palette(pick: Rc<dyn Fn(Color, bool)>) -> gtk::Grid {
    let grid = gtk::Grid::builder()
        .row_spacing(2)
        .column_spacing(2)
        .build();
    for (i, &(name, colour)) in PALETTE.iter().enumerate() {
        let button = gtk::Button::new();
        button.add_css_class("flat");
        button.add_css_class("swatch");
        button.set_child(Some(&Swatch::new(colour)));
        button.set_tooltip_text(Some(&format!("{name} (right-click: background)")));
        button.update_property(&[gtk::accessible::Property::Label(name)]);
        button.set_focusable(false);
        button.connect_clicked({
            let pick = pick.clone();
            move |_| pick(colour, false)
        });
        let right = gtk::GestureClick::new();
        right.set_button(gdk::BUTTON_SECONDARY);
        right.connect_pressed({
            let pick = pick.clone();
            move |_, _, _, _| pick(colour, true)
        });
        button.add_controller(right);
        grid.attach(&button, (i % 10) as i32, (i / 10) as i32, 1, 1);
    }
    grid
}

mod imp {
    use super::*;
    use std::cell::Cell;

    pub struct Swatch {
        pub colour: Cell<Color>,
    }

    impl Default for Swatch {
        fn default() -> Swatch {
            Swatch {
                colour: Cell::new(Color::BLACK),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Swatch {
        const NAME: &'static str = "EtchSwatch";
        type Type = super::Swatch;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Swatch {}

    impl WidgetImpl for Swatch {
        fn measure(&self, _: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            (SWATCH_PX, SWATCH_PX, -1, -1)
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let rect = graphene::Rect::new(0.0, 0.0, obj.width() as f32, obj.height() as f32);
            draw_colour(snapshot, &rect, self.colour.get());
        }
    }

    pub struct Indicator {
        pub colours: Cell<(Color, Color)>,
    }

    impl Default for Indicator {
        fn default() -> Indicator {
            Indicator {
                colours: Cell::new((Color::BLACK, Color::WHITE)),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Indicator {
        const NAME: &'static str = "EtchColourIndicator";
        type Type = super::Indicator;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_accessible_role(gtk::AccessibleRole::Img);
        }
    }

    impl ObjectImpl for Indicator {}

    impl WidgetImpl for Indicator {
        fn measure(&self, _: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            let side = (SQUARE_PX * 1.5).ceil() as i32;
            (side, side, -1, -1)
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let (fg, bg) = self.colours.get();
            let offset = SQUARE_PX / 2.0;
            draw_colour(
                snapshot,
                &graphene::Rect::new(offset, offset, SQUARE_PX, SQUARE_PX),
                bg,
            );
            draw_colour(
                snapshot,
                &graphene::Rect::new(0.0, 0.0, SQUARE_PX, SQUARE_PX),
                fg,
            );
        }
    }
}

glib::wrapper! {
    /// One colour, drawn as a square.
    pub struct Swatch(ObjectSubclass<imp::Swatch>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Swatch {
    fn new(colour: Color) -> Swatch {
        let s: Swatch = glib::Object::new();
        s.imp().colour.set(colour);
        s
    }
}

glib::wrapper! {
    /// The foreground colour's square over the background's, as Paint shows them.
    pub struct Indicator(ObjectSubclass<imp::Indicator>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Indicator {
    pub fn new() -> Indicator {
        let i: Indicator = glib::Object::new();
        i.update_property(&[gtk::accessible::Property::Label("Colours")]);
        i
    }

    pub fn set(&self, foreground: Color, background: Color) {
        self.imp().colours.set((foreground, background));
        self.update_property(&[gtk::accessible::Property::Description(&format!(
            "Foreground {}, background {}",
            hex(foreground),
            hex(background)
        ))]);
        self.set_tooltip_text(Some(&format!(
            "Foreground {}, background {} (X swaps them)",
            hex(foreground),
            hex(background)
        )));
        self.queue_draw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_shows_alpha_only_when_translucent() {
        assert_eq!(hex(Color::rgb(0xED, 0x1C, 0x24)), "#ED1C24");
        assert_eq!(hex(Color::rgba(1, 2, 3, 4)), "#01020304");
    }

    #[test]
    fn hex_reads_three_six_or_eight_digits_and_nothing_else() {
        assert_eq!(parse_hex("#3366CC"), Some(Color::rgb(0x33, 0x66, 0xCC)));
        assert_eq!(parse_hex("#3366cc"), Some(Color::rgb(0x33, 0x66, 0xCC)));
        assert_eq!(parse_hex(" 3366CC "), Some(Color::rgb(0x33, 0x66, 0xCC)));
        assert_eq!(parse_hex("#36C"), Some(Color::rgb(0x33, 0x66, 0xCC)));
        assert_eq!(
            parse_hex("#3366CC80"),
            Some(Color::rgba(0x33, 0x66, 0xCC, 0x80))
        );
        assert_eq!(parse_hex("#00FF0000"), Some(Color::rgba(0, 255, 0, 0)));
        for bad in [
            "",
            "#",
            "#12",
            "#1234",
            "#12345",
            "#1234567",
            "#123456789",
            "#GG0000",
            "red",
            "#+12345",
            "##336699",
            "#33 66CC",
            "#３３６６ＣＣ",
        ] {
            assert_eq!(parse_hex(bad), None, "{bad:?}");
        }
        // What the colour row shows reads back as the same colour.
        for c in [Color::rgb(1, 2, 3), Color::rgba(0xED, 0x1C, 0x24, 0x7F)] {
            assert_eq!(parse_hex(&hex(c)), Some(c));
        }
    }

    #[test]
    fn a_chosen_colour_keeps_its_alpha() {
        let chosen = gdk::RGBA::new(0.2, 0.4, 0.8, 0.5);
        assert_eq!(from_rgba(&chosen), Color::rgba(0x33, 0x66, 0xCC, 0x80));
        assert_eq!(
            from_rgba(&rgba(Color::rgba(1, 2, 3, 4))),
            Color::rgba(1, 2, 3, 4)
        );
        assert_eq!(
            from_rgba(&gdk::RGBA::new(1.5, -0.1, 0.0, 1.0)),
            Color::rgb(255, 0, 0)
        );
    }
}
