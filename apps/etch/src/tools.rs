//! The tools and the tool column at the window's left: one toggle button a tool, each
//! with an icon drawn from a path in the theme's foreground colour, so the column needs
//! no icon theme. The buttons drive the window's `tool` action, so exactly one is on.

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{glib, graphene, gsk};
use image_model::{Color, Pencil};

/// ET0's eraser width (ETCH-4 makes sizes adjustable).
const ERASER_WIDTH: u32 = 9;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Pencil,
    Eraser,
}

impl Tool {
    pub const ALL: [Tool; 2] = [Tool::Pencil, Tool::Eraser];

    /// The `tool` action's target for it.
    pub fn id(self) -> &'static str {
        match self {
            Tool::Pencil => "pencil",
            Tool::Eraser => "eraser",
        }
    }

    pub fn from_id(id: &str) -> Option<Tool> {
        Tool::ALL.into_iter().find(|t| t.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Tool::Pencil => "Pencil",
            Tool::Eraser => "Eraser",
        }
    }

    /// The key that picks it (docs/specs/etch.md).
    pub fn key(self) -> char {
        match self {
            Tool::Pencil => 'P',
            Tool::Eraser => 'E',
        }
    }

    /// What a drag with the tool draws with.
    pub fn tip(self) -> Pencil {
        match self {
            Tool::Pencil => Pencil {
                color: Color::BLACK,
                width: 1,
            },
            Tool::Eraser => Pencil::eraser(ERASER_WIDTH),
        }
    }

    /// The icon: an SVG path on a 16-unit square, stroked.
    fn icon(self) -> &'static str {
        match self {
            // A pencil leaning right: body, the tip's cut, the lead.
            Tool::Pencil => "M 2.5 13.5 L 3 10.5 L 11 2.5 L 13.5 5 L 5.5 13 Z M 9.5 4 L 12 6.5",
            // A tilted block, its band, and the line it rubs along.
            Tool::Eraser => {
                "M 2 9.5 L 8 3.5 L 13 8.5 L 8 13.5 L 6 13.5 Z M 5 6.5 L 10 11.5 M 8 13.5 L 14.5 13.5"
            }
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
}
