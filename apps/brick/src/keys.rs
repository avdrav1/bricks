//! Every shortcut Brick binds (MAC-3), in two tables: the window's ([`WINDOW`]: GTK's
//! accelerators and the window's key handler), and the grid's (`GRID`, checked against
//! [`grid_key`]: what `editor.rs` and the grid's navigation take while the grid has the
//! keys; the window's keys are live there too, as the grid passes on what isn't its own).
//!
//! A spec says `<Primary>` where the platform's modifier goes, so each key is Ctrl on Linux
//! and ⌘ on macOS (`suite_ui::keys`). A literal `<Control>` stays Control on macOS too:
//! the few keys macOS takes for itself with ⌘ and Excel for Mac puts on ⌃ instead.

use crate::editor::{self, ClipCommand, ShapeCommand, Start};
use crate::grid_view::{nav_key, FontToggle};
use gtk::gdk::{self, ModifierType};
use suite_ui::keys::Primary;

/// What a window key does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Open,
    New,
    Save,
    SaveAs,
    Undo,
    Redo,
    Find,
    Replace,
    /// The name box (FML-9).
    GoTo,
    /// A new sheet (SHEET-1).
    NewSheet,
    /// Bold, italic, underline, strikethrough (STYLE-6).
    Font(FontToggle),
    /// The custom number format popover.
    CustomFormat,
    /// A number format code for the selection (STYLE-6).
    Format(&'static str),
    /// The next (1) or previous (-1) sheet (XLSX-1).
    Sheet(isize),
    /// The menu bar's Close Window (macOS).
    Close,
    /// The app menu's Quit (macOS).
    Quit,
}

/// Who takes a window key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    /// A GTK accelerator, through [`action`]: the start window's too, or (Replace on
    /// macOS) Option+letter, which a key handler can't match by keyval.
    Accel,
    /// The window's key handler, through [`window_command`].
    Handler,
}

/// A window key: what it's called, its spec, what it does, who takes it, and the
/// platform it's bound on (`None`: both).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub name: &'static str,
    pub spec: &'static str,
    pub command: Command,
    pub via: Via,
    pub only: Option<Primary>,
}

const fn key(name: &'static str, spec: &'static str, command: Command) -> Binding {
    Binding {
        name,
        spec,
        command,
        via: Via::Handler,
        only: None,
    }
}

const fn accel(name: &'static str, spec: &'static str, command: Command) -> Binding {
    Binding {
        via: Via::Accel,
        ..key(name, spec, command)
    }
}

const fn only(primary: Primary, binding: Binding) -> Binding {
    Binding {
        only: Some(primary),
        ..binding
    }
}

const NUMBER: Command = Command::Format("#,##0.00");
const DATE: Command = Command::Format("yyyy-mm-dd");
const CURRENCY: Command = Command::Format("[$$-409]#,##0.00");
const PERCENT: Command = Command::Format("0.00%");
const GENERAL: Command = Command::Format("General");

/// Every window key. A command's first binding is the one tooltips show.
pub const WINDOW: &[Binding] = &[
    accel("Open", "<Primary>o", Command::Open),
    accel("New", "<Primary>n", Command::New),
    key("Save", "<Primary>s", Command::Save),
    key("Save As", "<Primary><Shift>s", Command::SaveAs),
    key("Undo", "<Primary>z", Command::Undo),
    key("Redo", "<Primary><Shift>z", Command::Redo),
    only(Primary::Control, key("Redo", "<Primary>y", Command::Redo)),
    key("Find", "<Primary>f", Command::Find),
    // ⌘H hides the app on macOS; ⌥⌘F is Replace there, and an accelerator, as Option
    // changes the letter's keyval.
    only(
        Primary::Control,
        accel("Replace", "<Primary>h", Command::Replace),
    ),
    only(
        Primary::Command,
        accel("Replace", "<Alt><Primary>f", Command::Replace),
    ),
    key("Go To", "<Primary>g", Command::GoTo),
    key("Go To", "F5", Command::GoTo),
    key("New Sheet", "<Shift>F11", Command::NewSheet),
    key("Bold", "<Primary>b", Command::Font(FontToggle::Bold)),
    key("Italic", "<Primary>i", Command::Font(FontToggle::Italic)),
    key(
        "Underline",
        "<Primary>u",
        Command::Font(FontToggle::Underline),
    ),
    key(
        "Strikethrough",
        "<Primary>5",
        Command::Font(FontToggle::Strike),
    ),
    key("Custom Format", "<Primary>1", Command::CustomFormat),
    // The number formats stay on Control on macOS: ⇧⌘3, ⇧⌘4 and ⇧⌘5 take screenshots,
    // and Excel for Mac has them on ⌃⇧digit. Shifted digits arrive as their symbols, or
    // as the digit where it's the shifted one (AZERTY).
    key("Number", "<Control><Shift>exclam", NUMBER),
    key("Number", "<Control>exclam", NUMBER),
    key("Number", "<Control><Shift>1", NUMBER),
    key("Date", "<Control><Shift>numbersign", DATE),
    key("Date", "<Control><Shift>3", DATE),
    key("Currency", "<Control><Shift>dollar", CURRENCY),
    key("Currency", "<Control><Shift>4", CURRENCY),
    key("Percent", "<Control><Shift>percent", PERCENT),
    key("Percent", "<Control>percent", PERCENT),
    key("Percent", "<Control><Shift>5", PERCENT),
    key("General", "<Control><Shift>asciitilde", GENERAL),
    key("General", "<Control><Shift>grave", GENERAL),
    key("General", "<Control><Shift>dead_tilde", GENERAL),
    key("General", "<Control><Shift>dead_grave", GENERAL),
    // Sheets stay on Control on macOS too, as Excel for Mac's ⌃Page Down / ⌃Page Up.
    key("Next Sheet", "<Control>Page_Down", Command::Sheet(1)),
    key("Next Sheet", "<Control>KP_Page_Down", Command::Sheet(1)),
    key("Previous Sheet", "<Control>Page_Up", Command::Sheet(-1)),
    key("Previous Sheet", "<Control>KP_Page_Up", Command::Sheet(-1)),
    // The menu bar's Close Window and Quit (MAC-4); Linux has neither.
    only(
        Primary::Command,
        accel("Close Window", "<Primary>w", Command::Close),
    ),
    only(Primary::Command, accel("Quit", "<Primary>q", Command::Quit)),
];

/// The window keys bound under `primary`.
pub fn window(primary: Primary) -> impl Iterator<Item = &'static Binding> {
    WINDOW
        .iter()
        .filter(move |b| b.only.is_none_or(|p| p == primary))
}

/// What the window's key handler does with `key` pressed with `state` held.
pub fn window_command(primary: Primary, key: gdk::Key, state: ModifierType) -> Option<Command> {
    window(primary)
        .filter(|b| b.via == Via::Handler)
        .find(|b| primary.matches(b.spec, key, state))
        .map(|b| b.command)
}

/// The action an accelerator runs.
pub fn action(command: Command) -> Option<&'static str> {
    match command {
        Command::Open => Some("app.open"),
        Command::New => Some("app.new"),
        Command::Replace => Some("win.replace"),
        Command::Close => Some("app.close"),
        Command::Quit => Some("app.quit"),
        _ => None,
    }
}

/// The accelerators under `primary`: each action's, in the table's order.
pub fn accels(primary: Primary) -> Vec<(&'static str, Vec<String>)> {
    let mut out: Vec<(&'static str, Vec<String>)> = Vec::new();
    for b in window(primary).filter(|b| b.via == Via::Accel) {
        let Some(name) = action(b.command) else {
            continue;
        };
        let accel = primary.accel(b.spec);
        match out.iter_mut().find(|(n, _)| *n == name) {
            Some((_, list)) => list.push(accel),
            None => out.push((name, vec![accel])),
        }
    }
    out
}

/// `command`'s key under `primary` as a tooltip shows it: its first binding's.
pub fn label_for(primary: Primary, command: Command) -> String {
    window(primary)
        .find(|b| b.command == command)
        .map(|b| primary.label(b.spec))
        .unwrap_or_default()
}

/// The accelerator a menu item for `command` shows under `primary`: its first binding's.
pub fn accel_for(primary: Primary, command: Command) -> Option<String> {
    window(primary)
        .find(|b| b.command == command)
        .map(|b| primary.accel(b.spec))
}

/// `command`'s key on this platform, as a tooltip shows it.
pub fn label(command: Command) -> String {
    label_for(Primary::HERE, command)
}

/// What a grid key does: `editor.rs`'s commands and the grid's moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridKey {
    /// A move (GRID-3), Mods.ctrl set to the data's corner or for whole rows and columns.
    Nav(grid::Key, grid::Mods),
    Clip(ClipCommand),
    Shape(ShapeCommand),
    Clear,
    Edit(Start),
}

/// What the grid does with `key` pressed with `state` held, while no editor is open: each
/// of these takes the primary modifier as `primary`, so the tests run `GRID` under both.
pub fn grid_key(primary: Primary, key: gdk::Key, state: ModifierType) -> Option<GridKey> {
    if let Some((key, mods)) = nav_key(primary, key, state) {
        Some(GridKey::Nav(key, mods))
    } else if let Some(c) = editor::clip_command(primary, key, state) {
        Some(GridKey::Clip(c))
    } else if let Some(c) = editor::shape_command(primary, key, state) {
        Some(GridKey::Shape(c))
    } else if editor::clears(primary, key, state) {
        Some(GridKey::Clear)
    } else {
        editor::start(primary, key, state).map(GridKey::Edit)
    }
}

const CTRL: grid::Mods = grid::Mods {
    shift: false,
    ctrl: true,
};
const SHIFT: grid::Mods = grid::Mods {
    shift: true,
    ctrl: false,
};
const BOTH: grid::Mods = grid::Mods {
    shift: true,
    ctrl: true,
};

/// The grid's modified keys (plain arrows, Tab, Enter and typing aside), each checked
/// against [`grid_key`] for both platforms; the menu bar's Cut, Copy, and Paste show
/// theirs.
pub const GRID: &[(&str, &str, GridKey)] = &[
    ("Copy", "<Primary>c", GridKey::Clip(ClipCommand::Copy)),
    ("Copy", "<Primary>Insert", GridKey::Clip(ClipCommand::Copy)),
    ("Cut", "<Primary>x", GridKey::Clip(ClipCommand::Cut)),
    ("Cut", "<Shift>Delete", GridKey::Clip(ClipCommand::Cut)),
    ("Paste", "<Primary>v", GridKey::Clip(ClipCommand::Paste)),
    ("Paste", "<Shift>Insert", GridKey::Clip(ClipCommand::Paste)),
    // Ctrl++ is Ctrl+Shift+= on most layouts: the key arrives as `plus`.
    (
        "Insert",
        "<Primary><Shift>plus",
        GridKey::Shape(ShapeCommand::Insert),
    ),
    (
        "Insert",
        "<Primary>plus",
        GridKey::Shape(ShapeCommand::Insert),
    ),
    (
        "Insert",
        "<Primary>KP_Add",
        GridKey::Shape(ShapeCommand::Insert),
    ),
    (
        "Delete",
        "<Primary>minus",
        GridKey::Shape(ShapeCommand::Delete),
    ),
    (
        "Delete",
        "<Primary>KP_Subtract",
        GridKey::Shape(ShapeCommand::Delete),
    ),
    ("Clear", "Delete", GridKey::Clear),
    ("Edit", "F2", GridKey::Edit(Start::Keep)),
    (
        "First Cell",
        "<Primary>Home",
        GridKey::Nav(grid::Key::Home, CTRL),
    ),
    (
        "Last Cell",
        "<Primary>End",
        GridKey::Nav(grid::Key::End, CTRL),
    ),
    (
        "Select to First Cell",
        "<Primary><Shift>Home",
        GridKey::Nav(grid::Key::Home, BOTH),
    ),
    (
        "Select to Last Cell",
        "<Primary><Shift>End",
        GridKey::Nav(grid::Key::End, BOTH),
    ),
    // The selections stay on Control on macOS: ⌘Space is Spotlight, and Excel for Mac
    // selects columns with ⌃Space.
    (
        "Select Column",
        "<Control>space",
        GridKey::Nav(grid::Key::Space, CTRL),
    ),
    (
        "Select Row",
        "<Shift>space",
        GridKey::Nav(grid::Key::Space, SHIFT),
    ),
    (
        "Select All",
        "<Control><Shift>space",
        GridKey::Nav(grid::Key::Space, BOTH),
    ),
];

/// The accelerator a menu item for the grid's `name` (`"Copy"`) shows under `primary`:
/// its first key's.
pub fn grid_accel(primary: Primary, name: &str) -> Option<String> {
    GRID.iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, spec, _)| primary.accel(spec))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk::gdk::Key;
    use suite_ui::keys;

    const CONTROL: ModifierType = ModifierType::CONTROL_MASK;
    const META: ModifierType = ModifierType::META_MASK;

    /// Every key live while the grid has the keys: the window's and the grid's.
    fn live(p: Primary) -> Vec<(&'static str, &'static str)> {
        window(p)
            .map(|b| (b.name, b.spec))
            .chain(GRID.iter().map(|(name, spec, _)| (*name, *spec)))
            .collect()
    }

    #[test]
    fn no_key_is_bound_twice_or_taken_by_macos() {
        for p in Primary::ALL {
            assert_eq!(keys::clashes(p, live(p)), Vec::<String>::new(), "{p:?}");
        }
    }

    #[test]
    fn linux_keeps_its_keys() {
        let p = Primary::Control;
        let window: Vec<_> = window(p).map(|b| (b.name, p.accel(b.spec))).collect();
        let want = [
            ("Open", "<Control>o"),
            ("New", "<Control>n"),
            ("Save", "<Control>s"),
            ("Save As", "<Control><Shift>s"),
            ("Undo", "<Control>z"),
            ("Redo", "<Control><Shift>z"),
            ("Redo", "<Control>y"),
            ("Find", "<Control>f"),
            ("Replace", "<Control>h"),
            ("Go To", "<Control>g"),
            ("Go To", "F5"),
            ("New Sheet", "<Shift>F11"),
            ("Bold", "<Control>b"),
            ("Italic", "<Control>i"),
            ("Underline", "<Control>u"),
            ("Strikethrough", "<Control>5"),
            ("Custom Format", "<Control>1"),
            ("Number", "<Control><Shift>exclam"),
            ("Number", "<Control>exclam"),
            ("Number", "<Control><Shift>1"),
            ("Date", "<Control><Shift>numbersign"),
            ("Date", "<Control><Shift>3"),
            ("Currency", "<Control><Shift>dollar"),
            ("Currency", "<Control><Shift>4"),
            ("Percent", "<Control><Shift>percent"),
            ("Percent", "<Control>percent"),
            ("Percent", "<Control><Shift>5"),
            ("General", "<Control><Shift>asciitilde"),
            ("General", "<Control><Shift>grave"),
            ("General", "<Control><Shift>dead_tilde"),
            ("General", "<Control><Shift>dead_grave"),
            ("Next Sheet", "<Control>Page_Down"),
            ("Next Sheet", "<Control>KP_Page_Down"),
            ("Previous Sheet", "<Control>Page_Up"),
            ("Previous Sheet", "<Control>KP_Page_Up"),
        ];
        let want: Vec<_> = want.iter().map(|(n, a)| (*n, a.to_string())).collect();
        assert_eq!(window, want);
        assert_eq!(
            accels(p),
            [
                ("app.open", vec!["<Control>o".to_string()]),
                ("app.new", vec!["<Control>n".to_string()]),
                ("win.replace", vec!["<Control>h".to_string()]),
            ]
        );
        let grid: Vec<_> = GRID.iter().map(|(_, spec, _)| p.accel(spec)).collect();
        assert_eq!(
            grid,
            [
                "<Control>c",
                "<Control>Insert",
                "<Control>x",
                "<Shift>Delete",
                "<Control>v",
                "<Shift>Insert",
                "<Control><Shift>plus",
                "<Control>plus",
                "<Control>KP_Add",
                "<Control>minus",
                "<Control>KP_Subtract",
                "Delete",
                "F2",
                "<Control>Home",
                "<Control>End",
                "<Control><Shift>Home",
                "<Control><Shift>End",
                "<Control>space",
                "<Shift>space",
                "<Control><Shift>space",
            ]
        );
        // The tooltips' text, as before MAC-3.
        for (command, text) in [
            (Command::Open, "Ctrl+O"),
            (Command::New, "Ctrl+N"),
            (Command::SaveAs, "Ctrl+Shift+S"),
            (Command::Replace, "Ctrl+H"),
            (Command::GoTo, "Ctrl+G"),
            (Command::NewSheet, "Shift+F11"),
            (Command::Font(FontToggle::Bold), "Ctrl+B"),
            (Command::Font(FontToggle::Italic), "Ctrl+I"),
            (Command::Font(FontToggle::Underline), "Ctrl+U"),
            (Command::Font(FontToggle::Strike), "Ctrl+5"),
            (Command::CustomFormat, "Ctrl+1"),
        ] {
            assert_eq!(label_for(p, command), text, "{command:?}");
        }
    }

    #[test]
    fn macos_uses_command_but_for_the_named_exceptions() {
        let p = Primary::Command;
        let control: Vec<_> = live(p)
            .into_iter()
            .map(|(_, spec)| p.accel(spec))
            .filter(|a| a.contains("<Control>"))
            .collect();
        assert_eq!(
            control,
            [
                "<Control><Shift>exclam",
                "<Control>exclam",
                "<Control><Shift>1",
                "<Control><Shift>numbersign",
                "<Control><Shift>3",
                "<Control><Shift>dollar",
                "<Control><Shift>4",
                "<Control><Shift>percent",
                "<Control>percent",
                "<Control><Shift>5",
                "<Control><Shift>asciitilde",
                "<Control><Shift>grave",
                "<Control><Shift>dead_tilde",
                "<Control><Shift>dead_grave",
                "<Control>Page_Down",
                "<Control>KP_Page_Down",
                "<Control>Page_Up",
                "<Control>KP_Page_Up",
                "<Control>space",
                "<Control><Shift>space",
            ]
        );
        assert_eq!(
            accels(p),
            [
                ("app.open", vec!["<Meta>o".to_string()]),
                ("app.new", vec!["<Meta>n".to_string()]),
                ("win.replace", vec!["<Alt><Meta>f".to_string()]),
                ("app.close", vec!["<Meta>w".to_string()]),
                ("app.quit", vec!["<Meta>q".to_string()]),
            ]
        );
        // A menu item shows a command's first key; Close Window's is macOS's alone.
        assert_eq!(
            accel_for(p, Command::Redo).as_deref(),
            Some("<Meta><Shift>z")
        );
        assert_eq!(accel_for(p, Command::Close).as_deref(), Some("<Meta>w"));
        assert_eq!(accel_for(Primary::Control, Command::Close), None);
        assert_eq!(label_for(p, Command::Replace), "⌥⌘F");
        assert_eq!(label_for(p, Command::SaveAs), "⇧⌘S");
        assert!(
            window(p).all(|b| b.spec != "<Primary>y"),
            "redo is ⇧⌘Z alone"
        );
    }

    #[test]
    fn the_window_handler_takes_the_platforms_modifier() {
        let mac = Primary::Command;
        let linux = Primary::Control;
        assert_eq!(window_command(mac, Key::z, META), Some(Command::Undo));
        assert_eq!(window_command(mac, Key::z, CONTROL), None, "⌃Z isn't undo");
        assert_eq!(window_command(linux, Key::z, CONTROL), Some(Command::Undo));
        assert_eq!(window_command(linux, Key::z, META), None);
        let shift = ModifierType::SHIFT_MASK;
        assert_eq!(
            window_command(mac, Key::Z, META | shift),
            Some(Command::Redo)
        );
        assert_eq!(
            window_command(linux, Key::Z, CONTROL | shift),
            Some(Command::Redo)
        );
        assert_eq!(window_command(linux, Key::y, CONTROL), Some(Command::Redo));
        assert_eq!(window_command(mac, Key::y, META), None);
        assert_eq!(
            window_command(mac, Key::S, META | shift),
            Some(Command::SaveAs)
        );
        assert_eq!(
            window_command(linux, Key::F5, ModifierType::empty()),
            Some(Command::GoTo)
        );
        // Replace is an accelerator on both, never the handler's.
        assert_eq!(window_command(linux, Key::h, CONTROL), None);
        // The exceptions: Control on both.
        for p in Primary::ALL {
            assert_eq!(
                window_command(p, Key::dollar, CONTROL | shift),
                Some(CURRENCY),
                "{p:?}"
            );
            assert_eq!(
                window_command(p, Key::Page_Down, CONTROL),
                Some(Command::Sheet(1)),
                "{p:?}"
            );
        }
        assert_eq!(window_command(mac, Key::dollar, META | shift), None);
        assert_eq!(
            window_command(mac, Key::_5, META),
            Some(Command::Font(FontToggle::Strike))
        );
    }

    /// The grid takes each of `GRID`'s keys, under either platform.
    #[test]
    fn the_grid_takes_its_table() {
        for p in Primary::ALL {
            for (name, spec, want) in GRID {
                let (key, state) = p.parse(spec).expect(spec);
                assert_eq!(grid_key(p, key, state), Some(*want), "{p:?} {name} {spec}");
            }
        }
    }

    #[test]
    fn the_grid_leaves_the_other_modifiers_keys_alone() {
        let mac = Primary::Command;
        assert_eq!(
            grid_key(mac, Key::c, META),
            Some(GridKey::Clip(ClipCommand::Copy))
        );
        for (key, state, why) in [
            (Key::c, CONTROL, "⌃C"),
            (Key::Home, CONTROL, "⌃Home"),
            (Key::space, META, "⌘Space is Spotlight"),
            (Key::Down, META, "⌘↓ isn't built yet"),
            (
                Key::Page_Down,
                CONTROL,
                "⌃Page Down is the window's: sheets",
            ),
            (Key::s, META, "⌘S saves"),
            (Key::Delete, META, "⌘Delete"),
        ] {
            assert_eq!(grid_key(mac, key, state), None, "{why}");
        }
        // Linux: Ctrl+Page Down passes on to the window too, Ctrl+Home is the grid's.
        let linux = Primary::Control;
        assert_eq!(grid_key(linux, Key::Page_Down, CONTROL), None);
        assert_eq!(
            grid_key(linux, Key::Home, CONTROL),
            Some(GridKey::Nav(grid::Key::Home, CTRL))
        );
    }
}
