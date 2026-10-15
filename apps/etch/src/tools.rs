//! The tools and the tool column at the window's left: one toggle button a tool, each
//! with an icon drawn from a path in the theme's foreground colour, so the column needs
//! no icon theme. The buttons drive the window's `tool` action, so exactly one is on.

use crate::text::{TextLook, DEFAULT_FAMILY};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{glib, graphene, gsk};
use image_model::{Brush, Color, Kind, Pencil, ShapeTool, Style, Tip};

/// The widths a tool's size can take, in image pixels.
pub const SIZES: std::ops::RangeInclusive<u32> = 1..=100;
/// The sizes text can take: its font's size in image pixels (the em, as Pango's absolute
/// size).
pub const TEXT_SIZES: std::ops::RangeInclusive<u32> = 4..=400;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Pencil,
    Brush,
    Eraser,
    Fill,
    Picker,
    Text,
    Line,
    Rectangle,
    Ellipse,
    Selection,
}

/// What the canvas does with the pointer's buttons under the current tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// A primary-button drag draws with the tip.
    Draw(Tip),
    /// A click fills the region under it with the foreground, a right-click with the
    /// background, taking colours within `tolerance` of the clicked pixel's.
    Fill {
        foreground: Color,
        background: Color,
        tolerance: u8,
    },
    /// A click makes the pixel under it the foreground colour, a right-click the
    /// background.
    Pick,
    /// A primary-button drag previews the shape from the press to the pointer, and the
    /// release draws it.
    Shape(ShapeTool),
    /// A click starts typing text there in this look; while it's typed, a click elsewhere
    /// puts it on the image.
    Text(TextLook),
    /// A primary-button drag selects a rectangle; one inside the selection moves its
    /// pixels.
    Select,
}

impl Tool {
    pub const ALL: [Tool; 10] = [
        Tool::Pencil,
        Tool::Brush,
        Tool::Eraser,
        Tool::Fill,
        Tool::Picker,
        Tool::Text,
        Tool::Line,
        Tool::Rectangle,
        Tool::Ellipse,
        Tool::Selection,
    ];

    /// The `tool` action's target for it.
    pub fn id(self) -> &'static str {
        match self {
            Tool::Pencil => "pencil",
            Tool::Brush => "brush",
            Tool::Eraser => "eraser",
            Tool::Fill => "fill",
            Tool::Picker => "picker",
            Tool::Text => "text",
            Tool::Line => "line",
            Tool::Rectangle => "rectangle",
            Tool::Ellipse => "ellipse",
            Tool::Selection => "selection",
        }
    }

    pub fn from_id(id: &str) -> Option<Tool> {
        Tool::ALL.into_iter().find(|t| t.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Tool::Pencil => "Pencil",
            Tool::Brush => "Brush",
            Tool::Eraser => "Eraser",
            Tool::Fill => "Fill",
            Tool::Picker => "Colour Picker",
            Tool::Text => "Text",
            Tool::Line => "Line",
            Tool::Rectangle => "Rectangle",
            Tool::Ellipse => "Ellipse",
            Tool::Selection => "Selection",
        }
    }

    /// The key that picks it, from the key table ([`crate::keys::BINDINGS`]).
    pub fn key(self) -> char {
        crate::keys::tool_key(self).expect("every tool has a key (keys::tests)")
    }

    /// The icon: an SVG path on a 16-unit square, stroked.
    fn icon(self) -> &'static str {
        match self {
            // A pencil leaning right: body, the tip's cut, the lead.
            Tool::Pencil => "M 2.5 13.5 L 3 10.5 L 11 2.5 L 13.5 5 L 5.5 13 Z M 9.5 4 L 12 6.5",
            // A brush leaning right: its handle, the ferrule, and the bristles' rounded tip.
            Tool::Brush => {
                "M 14 2 L 9 7 M 7 6.5 L 9.5 9 M 8 8 C 5.5 7 3.5 9 3.5 11.5 C 3.5 12.8 3 13.5 2 14 C 5.5 14.5 9 12 8 8 Z"
            }
            // A tilted block, its band, and the line it rubs along.
            Tool::Eraser => {
                "M 2 9.5 L 8 3.5 L 13 8.5 L 8 13.5 L 6 13.5 Z M 5 6.5 L 10 11.5 M 8 13.5 L 14.5 13.5"
            }
            // A paint bucket tipped right: its body, rim, and handle, and a drop falling.
            Tool::Fill => {
                "M 2.5 8 L 7.5 3 L 12.5 8 L 8 12.5 L 7 12.5 Z M 2.5 8 L 12.5 8 M 7.5 3 L 5.5 1 M 13.5 10 C 12.5 11.5 12.3 13.5 13.5 13.5 C 14.7 13.5 14.5 11.5 13.5 10 Z"
            }
            // An eyedropper leaning right: its glass tube, the bulb's collar, the bulb.
            Tool::Picker => {
                "M 2 14 L 2.5 11.5 L 8.5 5.5 L 10.5 7.5 L 4.5 13.5 Z M 7.5 4.5 L 11.5 8.5 M 9.5 6.5 L 12 4 C 13 3 14.5 3 13.5 2.5 C 13 1.5 13 3 12 4"
            }
            // A capital T with serifs at its arms' ends and its foot.
            Tool::Text => "M 3 5 L 3 2.5 L 13 2.5 L 13 5 M 8 2.5 L 8 13.5 M 5.5 13.5 L 10.5 13.5",
            // A line rising right.
            Tool::Line => "M 2.5 13.5 L 13.5 2.5",
            // A rectangle, wider than tall.
            Tool::Rectangle => "M 2 4 L 14 4 L 14 12 L 2 12 Z",
            // An ellipse, wider than tall.
            Tool::Ellipse => {
                "M 1.5 8 C 1.5 5.5 4.4 3.5 8 3.5 C 11.6 3.5 14.5 5.5 14.5 8 C 14.5 10.5 11.6 12.5 8 12.5 C 4.4 12.5 1.5 10.5 1.5 8 Z"
            }
            // A dashed rectangle: its corners, and a dash in each side's middle.
            Tool::Selection => {
                "M 2 5.5 L 2 3.5 L 4 3.5 M 7 3.5 L 9 3.5 M 12 3.5 L 14 3.5 L 14 5.5 M 14 7 L 14 9 M 14 10.5 L 14 12.5 L 12 12.5 M 9 12.5 L 7 12.5 M 4 12.5 L 2 12.5 L 2 10.5 M 2 9 L 2 7"
            }
        }
    }
}

/// What drawing uses: the tool, the foreground and background colours, each tool's own
/// size (switching tools keeps each one's), the brush's edge, the rectangle's and
/// ellipse's style, and the text's font.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub tool: Tool,
    pub foreground: Color,
    pub background: Color,
    /// The brush's edge: soft (falling off to transparent) or hard.
    pub soft: bool,
    /// Rectangles and ellipses outlined, filled, or both (one style for the two, as in
    /// Paint).
    pub style: Style,
    /// The text's font family, bold, and italic; its size is the text tool's.
    pub family: &'static str,
    pub bold: bool,
    pub italic: bool,
    /// The fill's tolerance: how far, 0 to 255, a pixel's colour may be from the clicked
    /// one's and still fill (`image_model::Document::fill`); 0, exact, at first.
    pub tolerance: u8,
    /// The tool before the colour picker, which a pick goes back to.
    before_picker: Tool,
    pencil_width: u32,
    brush_width: u32,
    eraser_width: u32,
    text_size: u32,
    line_width: u32,
    rectangle_width: u32,
    ellipse_width: u32,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            tool: Tool::Pencil,
            foreground: Color::BLACK,
            background: Color::WHITE,
            soft: true,
            style: Style::Outline,
            family: DEFAULT_FAMILY,
            bold: false,
            italic: false,
            tolerance: 0,
            before_picker: Tool::Pencil,
            pencil_width: 1,
            brush_width: 8,
            eraser_width: 9,
            text_size: 24,
            line_width: 1,
            rectangle_width: 1,
            ellipse_width: 1,
        }
    }
}

impl Settings {
    /// Makes `tool` the tool, remembering the one before the colour picker.
    pub fn set_tool(&mut self, tool: Tool) {
        if tool == Tool::Picker && self.tool != Tool::Picker {
            self.before_picker = self.tool;
        }
        self.tool = tool;
    }

    /// A pick: `colour` becomes the background (a right-click) or the foreground. Returns
    /// the tool to go back to, the one used before the picker, as Paint does.
    pub fn pick(&mut self, colour: Color, background: bool) -> Tool {
        match background {
            true => self.background = colour,
            false => self.foreground = colour,
        }
        self.before_picker
    }

    /// The current tool's size (text's is its font's); the fill, the picker, and the
    /// selection have none.
    pub fn width(&self) -> Option<u32> {
        match self.tool {
            Tool::Pencil => Some(self.pencil_width),
            Tool::Brush => Some(self.brush_width),
            Tool::Eraser => Some(self.eraser_width),
            Tool::Text => Some(self.text_size),
            Tool::Line => Some(self.line_width),
            Tool::Rectangle => Some(self.rectangle_width),
            Tool::Ellipse => Some(self.ellipse_width),
            Tool::Fill | Tool::Picker | Tool::Selection => None,
        }
    }

    /// The sizes the current tool takes: [`TEXT_SIZES`] for text, else [`SIZES`].
    pub fn sizes(&self) -> std::ops::RangeInclusive<u32> {
        match self.tool {
            Tool::Text => TEXT_SIZES,
            _ => SIZES,
        }
    }

    /// Sets the current tool's size, kept within [`Settings::sizes`]; nothing for a tool
    /// without one.
    pub fn set_width(&mut self, width: u32) {
        let sizes = self.sizes();
        let w = width.clamp(*sizes.start(), *sizes.end());
        match self.tool {
            Tool::Pencil => self.pencil_width = w,
            Tool::Brush => self.brush_width = w,
            Tool::Eraser => self.eraser_width = w,
            Tool::Text => self.text_size = w,
            Tool::Line => self.line_width = w,
            Tool::Rectangle => self.rectangle_width = w,
            Tool::Ellipse => self.ellipse_width = w,
            Tool::Fill | Tool::Picker | Tool::Selection => {}
        }
    }

    /// Whether the tool has a style: outlined, filled, or both.
    pub fn has_style(&self) -> bool {
        matches!(self.tool, Tool::Rectangle | Tool::Ellipse)
    }

    /// X: the foreground becomes the background and the background the foreground.
    pub fn swap(&mut self) {
        std::mem::swap(&mut self.foreground, &mut self.background);
    }

    /// What the canvas does with the current tool: the pencil and the brush draw in the
    /// foreground, the eraser to transparency, each at its size; the fill fills; the
    /// picker picks; text is typed in the foreground at its size in the font chosen; the
    /// shapes draw at their size, outlined in the foreground and filled with the
    /// background, or, filled without an outline, with the foreground (as Paint and
    /// Paint.NET do); the selection selects.
    pub fn action(&self) -> Action {
        let shape = |kind, width| {
            let fill = match self.style {
                Style::Filled => self.foreground,
                Style::Outline | Style::Both => self.background,
            };
            Action::Shape(ShapeTool {
                kind,
                width,
                outline: self.foreground,
                fill,
            })
        };
        match self.tool {
            Tool::Pencil => Action::Draw(Tip::Pencil(Pencil {
                color: self.foreground,
                width: self.pencil_width,
            })),
            Tool::Brush => Action::Draw(Tip::Brush(Brush {
                color: self.foreground,
                width: self.brush_width,
                soft: self.soft,
            })),
            Tool::Eraser => Action::Draw(Tip::Pencil(Pencil::eraser(self.eraser_width))),
            Tool::Fill => Action::Fill {
                foreground: self.foreground,
                background: self.background,
                tolerance: self.tolerance,
            },
            Tool::Picker => Action::Pick,
            Tool::Text => Action::Text(TextLook {
                family: self.family,
                size: self.text_size,
                bold: self.bold,
                italic: self.italic,
                color: self.foreground,
            }),
            Tool::Line => shape(Kind::Line, self.line_width),
            Tool::Rectangle => shape(Kind::Rectangle(self.style), self.rectangle_width),
            Tool::Ellipse => shape(Kind::Ellipse(self.style), self.ellipse_width),
            Tool::Selection => Action::Select,
        }
    }
}

/// The tool column: a button a tool, bound to `win.tool`.
pub fn column() -> gtk::Box {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 4);
    column.set_margin_start(6);
    column.set_margin_end(6);
    column.set_margin_top(6);
    for tool in Tool::ALL {
        let button = gtk::ToggleButton::new();
        button.set_child(Some(&ToolIcon::new(tool)));
        button.set_action_name(Some("win.tool"));
        button.set_action_target_value(Some(&tool.id().to_variant()));
        button.set_tooltip_text(Some(&format!("{} ({})", tool.label(), tool.key())));
        button.update_property(&[gtk::accessible::Property::Label(tool.label())]);
        button.add_css_class("flat");
        column.append(&button);
    }
    column
}

/// The size an icon draws at.
const ICON_PX: f32 = 20.0;

mod imp {
    use super::*;
    use std::cell::Cell;

    #[derive(Default)]
    pub struct ToolIcon {
        pub tool: Cell<Tool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ToolIcon {
        const NAME: &'static str = "EtchToolIcon";
        type Type = super::ToolIcon;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for ToolIcon {}

    impl WidgetImpl for ToolIcon {
        fn measure(&self, _: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            (ICON_PX as i32, ICON_PX as i32, -1, -1)
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let Ok(path) = gsk::Path::parse(self.tool.get().icon()) else {
                return;
            };
            let (w, h) = (obj.width() as f32, obj.height() as f32);
            snapshot.save();
            snapshot.translate(&graphene::Point::new(
                (w - ICON_PX) / 2.0,
                (h - ICON_PX) / 2.0,
            ));
            snapshot.scale(ICON_PX / 16.0, ICON_PX / 16.0);
            let stroke = gsk::Stroke::new(1.3);
            stroke.set_line_join(gsk::LineJoin::Round);
            stroke.set_line_cap(gsk::LineCap::Round);
            snapshot.append_stroke(&path, &stroke, &obj.color());
            snapshot.restore();
        }
    }
}

glib::wrapper! {
    pub struct ToolIcon(ObjectSubclass<imp::ToolIcon>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl ToolIcon {
    fn new(tool: Tool) -> ToolIcon {
        let icon: ToolIcon = glib::Object::new();
        icon.imp().tool.set(tool);
        icon
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_its_own_id_and_key() {
        let ids: std::collections::HashSet<_> = Tool::ALL.iter().map(|t| t.id()).collect();
        let keys: std::collections::HashSet<_> = Tool::ALL.iter().map(|t| t.key()).collect();
        assert_eq!((ids.len(), keys.len()), (Tool::ALL.len(), Tool::ALL.len()));
        for t in Tool::ALL {
            assert_eq!(Tool::from_id(t.id()), Some(t));
            assert!(gsk::Path::parse(t.icon()).is_ok(), "{t:?}'s icon parses");
        }
    }

    #[test]
    fn each_tool_keeps_its_own_size() {
        let mut s = Settings::default();
        assert_eq!(s.width(), Some(1));
        s.set_width(7);
        s.tool = Tool::Eraser;
        assert_eq!(s.width(), Some(9));
        s.set_width(500);
        assert_eq!(s.width(), Some(100));
        s.set_width(0);
        assert_eq!(s.width(), Some(1));
        s.tool = Tool::Brush;
        assert_eq!(s.width(), Some(8));
        s.tool = Tool::Fill;
        assert_eq!(s.width(), None);
        s.set_width(30);
        s.tool = Tool::Pencil;
        assert_eq!(s.width(), Some(7));
    }

    #[test]
    fn each_tool_acts_with_the_colours() {
        let red = Color::rgb(237, 28, 36);
        let mut s = Settings {
            foreground: red,
            ..Settings::default()
        };
        let pencil = |c| Action::Draw(Tip::Pencil(Pencil { color: c, width: 1 }));
        assert_eq!(s.action(), pencil(red));
        s.swap();
        assert_eq!((s.foreground, s.background), (Color::WHITE, red));
        assert_eq!(s.action(), pencil(Color::WHITE));
        s.tool = Tool::Brush;
        let brush = |soft| {
            Action::Draw(Tip::Brush(Brush {
                color: Color::WHITE,
                width: 8,
                soft,
            }))
        };
        assert_eq!(s.action(), brush(true));
        s.soft = false;
        assert_eq!(s.action(), brush(false));
        s.tool = Tool::Eraser;
        assert_eq!(s.action(), Action::Draw(Tip::Pencil(Pencil::eraser(9))));
        s.tool = Tool::Fill;
        assert_eq!(
            s.action(),
            Action::Fill {
                foreground: Color::WHITE,
                background: red,
                tolerance: 0,
            }
        );
        // The tolerance set is the fill's; switching tools keeps it.
        s.tolerance = 32;
        s.set_tool(Tool::Pencil);
        s.set_tool(Tool::Fill);
        assert_eq!(
            s.action(),
            Action::Fill {
                foreground: Color::WHITE,
                background: red,
                tolerance: 32,
            }
        );
    }

    #[test]
    fn shapes_outline_in_the_foreground_and_fill_with_the_background() {
        let red = Color::rgb(237, 28, 36);
        let mut s = Settings {
            foreground: red,
            ..Settings::default()
        };
        let shape = |kind, width, fill| {
            Action::Shape(ShapeTool {
                kind,
                width,
                outline: red,
                fill,
            })
        };
        s.set_tool(Tool::Line);
        assert!(!s.has_style());
        assert_eq!(s.action(), shape(Kind::Line, 1, Color::WHITE));
        s.set_width(5);
        s.set_tool(Tool::Rectangle);
        assert!(s.has_style());
        assert_eq!(s.width(), Some(1));
        let outline = Kind::Rectangle(Style::Outline);
        assert_eq!(s.action(), shape(outline, 1, Color::WHITE));
        s.style = Style::Both;
        assert_eq!(
            s.action(),
            shape(Kind::Rectangle(Style::Both), 1, Color::WHITE)
        );
        // Filled without an outline: the foreground, as in Paint.
        s.style = Style::Filled;
        assert_eq!(s.action(), shape(Kind::Rectangle(Style::Filled), 1, red));
        // The style is the ellipse's too; each keeps its own size.
        s.set_tool(Tool::Ellipse);
        s.set_width(3);
        assert_eq!(s.action(), shape(Kind::Ellipse(Style::Filled), 3, red));
        s.set_tool(Tool::Line);
        assert_eq!(s.width(), Some(5));
    }

    #[test]
    fn a_pick_sets_a_colour_alpha_included_and_goes_back_to_the_tool_before() {
        let mut s = Settings::default();
        s.set_tool(Tool::Brush);
        s.set_tool(Tool::Picker);
        assert_eq!((s.action(), s.width()), (Action::Pick, None));
        let clear = Color::rgba(0, 0, 255, 128);
        assert_eq!(s.pick(clear, false), Tool::Brush);
        assert_eq!(s.foreground, clear);
        // Picking the picker again keeps the tool to go back to.
        s.set_tool(Tool::Fill);
        s.set_tool(Tool::Picker);
        s.set_tool(Tool::Picker);
        assert_eq!(s.pick(Color::TRANSPARENT, true), Tool::Fill);
        assert_eq!((s.foreground, s.background), (clear, Color::TRANSPARENT));
    }

    #[test]
    fn text_is_typed_in_the_foreground_at_its_own_size_in_the_font_chosen() {
        let red = Color::rgb(237, 28, 36);
        let mut s = Settings {
            foreground: red,
            ..Settings::default()
        };
        s.set_tool(Tool::Text);
        assert_eq!((s.width(), s.sizes()), (Some(24), TEXT_SIZES));
        let look = |size, bold, italic| {
            Action::Text(TextLook {
                family: DEFAULT_FAMILY,
                size,
                bold,
                italic,
                color: red,
            })
        };
        assert_eq!(s.action(), look(24, false, false));
        // Text takes sizes beyond the pencil's 100, and keeps its own.
        s.set_width(250);
        (s.bold, s.italic) = (true, true);
        assert_eq!(s.action(), look(250, true, true));
        s.set_width(1);
        assert_eq!(s.width(), Some(*TEXT_SIZES.start()));
        s.set_tool(Tool::Pencil);
        assert_eq!((s.width(), s.sizes()), (Some(1), SIZES));
        s.set_width(250);
        assert_eq!(s.width(), Some(100));
        s.set_tool(Tool::Text);
        s.family = "DejaVu Serif";
        let Action::Text(t) = s.action() else {
            panic!("text's action")
        };
        assert_eq!((t.family, t.size), ("DejaVu Serif", 4));
    }

    #[test]
    fn the_selection_selects_and_has_no_size() {
        let mut s = Settings::default();
        s.set_tool(Tool::Selection);
        assert_eq!(
            (s.action(), s.width(), s.has_style()),
            (Action::Select, None, false)
        );
        s.set_width(30);
        s.set_tool(Tool::Pencil);
        assert_eq!(s.width(), Some(1));
        assert_eq!(Tool::from_id("selection"), Some(Tool::Selection));
        assert_eq!(Tool::Selection.key(), 'S');
    }
}
