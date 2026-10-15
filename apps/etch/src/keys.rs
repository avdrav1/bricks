//! Every key Etch binds, in one table ([`BINDINGS`]): the tool keys and X, the menus'
//! shortcuts, the window's own keys (the selection's, Space), and the mnemonics of the
//! menu bar and the colour row. The app takes them from it: GTK's accelerators
//! ([`accels`]), the keys the menus show ([`menu_item`]), the window's key handler
//! ([`window_binding`]), the labels' underlined letters ([`mnemonic`]), and the tools' keys
//! ([`tool_key`]). A menu's own items and a dialog's mnemonics work only while it's open,
//! so they aren't here.
//!
//! Where a key goes depends on what has the keys ([`Focus`], [`takes`]): a field (the
//! size, the tolerance, the hex field, the font list's search) keeps every key, and text
//! being typed on the canvas every key but the Ctrl+Shift ones, so tool keys and X are
//! letters there (docs/specs/etch.md).

use crate::tools::Tool;
use gtk::gio;
use gtk::prelude::*;

/// The modifiers held with a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Mods {
    pub const NONE: Mods = Mods::new(false, false, false);
    pub const SHIFT: Mods = Mods::new(false, true, false);
    pub const CTRL: Mods = Mods::new(true, false, false);
    pub const CTRL_SHIFT: Mods = Mods::new(true, true, false);
    pub const ALT: Mods = Mods::new(false, false, true);

    const fn new(ctrl: bool, shift: bool, alt: bool) -> Mods {
        Mods { ctrl, shift, alt }
    }

    /// The modifiers of a key event's `state` (Caps Lock and Num Lock aside); `None` with
    /// Super, Hyper, or Meta held, which nothing binds.
    pub fn of(state: gtk::gdk::ModifierType) -> Option<Mods> {
        use gtk::gdk::ModifierType as M;
        let others = M::SUPER_MASK | M::HYPER_MASK | M::META_MASK;
        (state & others).is_empty().then(|| Mods {
            ctrl: state.contains(M::CONTROL_MASK),
            shift: state.contains(M::SHIFT_MASK),
            alt: state.contains(M::ALT_MASK),
        })
    }
}

/// A key and the modifiers held with it. The key is its GDK name, a letter in lower case:
/// Shift is a modifier, so X and Shift+X are different combinations, as are X, Ctrl+X,
/// Ctrl+Shift+X, and Alt+X.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Combo {
    pub mods: Mods,
    pub key: &'static str,
}

impl Combo {
    /// As a GTK accelerator: `<Control><Shift>s`.
    pub fn accel(self) -> String {
        let mut accel = String::new();
        for (on, name) in [
            (self.mods.ctrl, "<Control>"),
            (self.mods.shift, "<Shift>"),
            (self.mods.alt, "<Alt>"),
        ] {
            if on {
                accel.push_str(name);
            }
        }
        accel.push_str(self.key);
        accel
    }
}

/// What a key does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// Picks the tool.
    Tool(Tool),
    /// Swaps the foreground and background colours.
    Swap,
    /// Held, a drag pans the image.
    Pan,
    /// Moves the selection's pixels so many image pixels across and down.
    Nudge(i64, i64),
    /// Drops the shape being dragged (Escape does too, as Deselect).
    DropShape,
    /// Runs the action of this full name (`win.save-as`).
    Action(&'static str),
    /// A label's mnemonic: puts the keys in its field or opens its menu (the label's
    /// text, without the underline).
    Label(&'static str),
}

/// Who takes a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    /// GTK's accelerator for the action: it works wherever the keys are, a field
    /// included.
    Accel,
    /// The window's key handler, so a field and the text being typed keep the key
    /// ([`takes`]).
    Window,
    /// The window's key handler, which takes a Ctrl+Shift letter however its case arrives
    /// (an accelerator can miss an upper case one), and GTK's accelerator while a field
    /// has the keys.
    Both,
    /// GTK's mnemonic: the label's underlined letter, with Alt.
    Mnemonic,
}

/// A key combination, what it does, and who takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub combo: Combo,
    pub command: Command,
    pub via: Via,
}

const fn bind(mods: Mods, key: &'static str, command: Command, via: Via) -> Binding {
    Binding {
        combo: Combo { mods, key },
        command,
        via,
    }
}

const fn action(mods: Mods, key: &'static str, name: &'static str, via: Via) -> Binding {
    bind(mods, key, Command::Action(name), via)
}

const fn tool(key: &'static str, tool: Tool) -> Binding {
    bind(Mods::NONE, key, Command::Tool(tool), Via::Window)
}

const fn nudge(mods: Mods, key: &'static str, dx: i64, dy: i64) -> Binding {
    bind(mods, key, Command::Nudge(dx, dy), Via::Window)
}

const fn label(key: &'static str, text: &'static str) -> Binding {
    bind(Mods::ALT, key, Command::Label(text), Via::Mnemonic)
}

use Mods as M;
use Via::{Accel, Both, Window};

/// Every key Etch binds. An action's first binding is the one its menu item shows.
pub const BINDINGS: &[Binding] = &[
    // File.
    action(M::CTRL, "n", "app.new", Accel),
    action(M::CTRL, "o", "app.open", Accel),
    action(M::CTRL, "s", "win.save", Accel),
    action(M::CTRL_SHIFT, "s", "win.save-as", Both),
    action(M::CTRL_SHIFT, "e", "win.export", Both),
    action(M::CTRL, "w", "window.close", Accel),
    action(M::CTRL, "q", "app.quit", Accel),
    // Edit. Ctrl+Shift+Z is the window's alone: a field keeps its own redo. Cut, copy,
    // paste, select all, deselect, and delete are the window's so a field and the text
    // being typed keep theirs.
    action(M::CTRL, "z", "win.undo", Accel),
    action(M::CTRL, "y", "win.redo", Accel),
    action(M::CTRL_SHIFT, "z", "win.redo", Window),
    action(M::CTRL, "x", "win.cut", Window),
    action(M::CTRL, "c", "win.copy", Window),
    action(M::CTRL, "v", "win.paste", Window),
    action(M::CTRL, "a", "win.select-all", Window),
    action(M::NONE, "Escape", "win.deselect", Window),
    action(M::NONE, "Delete", "win.delete", Window),
    action(M::NONE, "KP_Delete", "win.delete", Window),
    // The selection's pixels move a pixel (Shift: ten).
    nudge(M::NONE, "Left", -1, 0),
    nudge(M::NONE, "Right", 1, 0),
    nudge(M::NONE, "Up", 0, -1),
    nudge(M::NONE, "Down", 0, 1),
    nudge(M::NONE, "KP_Left", -1, 0),
    nudge(M::NONE, "KP_Right", 1, 0),
    nudge(M::NONE, "KP_Up", 0, -1),
    nudge(M::NONE, "KP_Down", 0, 1),
    nudge(M::SHIFT, "Left", -10, 0),
    nudge(M::SHIFT, "Right", 10, 0),
    nudge(M::SHIFT, "Up", 0, -10),
    nudge(M::SHIFT, "Down", 0, 10),
    nudge(M::SHIFT, "KP_Left", -10, 0),
    nudge(M::SHIFT, "KP_Right", 10, 0),
    nudge(M::SHIFT, "KP_Up", 0, -10),
    nudge(M::SHIFT, "KP_Down", 0, 10),
    // A shape's drag held with Shift (to constrain it) drops with Shift+Escape too.
    bind(M::SHIFT, "Escape", Command::DropShape, Window),
    // View.
    action(M::CTRL, "plus", "win.zoom-in", Accel),
    action(M::CTRL, "equal", "win.zoom-in", Accel),
    action(M::CTRL, "KP_Add", "win.zoom-in", Accel),
    action(M::CTRL, "minus", "win.zoom-out", Accel),
    action(M::CTRL, "KP_Subtract", "win.zoom-out", Accel),
    action(M::CTRL, "0", "win.actual-size", Accel),
    action(M::CTRL, "KP_0", "win.actual-size", Accel),
    action(M::CTRL, "9", "win.fit", Accel),
    // Image (the spec's; Paint.NET's). The flips have none.
    action(M::CTRL_SHIFT, "x", "win.crop", Both),
    action(M::CTRL, "r", "win.resize-image", Accel),
    action(M::CTRL_SHIFT, "r", "win.resize-canvas", Both),
    action(M::CTRL, "h", "win.rotate-right", Accel),
    action(M::CTRL, "g", "win.rotate-left", Accel),
    action(M::CTRL, "j", "win.rotate-half", Accel),
    // The tools, the colours, and panning.
    tool("p", Tool::Pencil),
    tool("b", Tool::Brush),
    tool("e", Tool::Eraser),
    tool("f", Tool::Fill),
    tool("i", Tool::Picker),
    tool("t", Tool::Text),
    tool("r", Tool::Rectangle),
    tool("l", Tool::Line),
    tool("s", Tool::Selection),
    tool("o", Tool::Ellipse),
    bind(M::NONE, "x", Command::Swap, Window),
    bind(M::NONE, "space", Command::Pan, Window),
    // The menu bar's menus and the colour row's fields.
    label("f", "File"),
    label("e", "Edit"),
    label("v", "View"),
    label("i", "Image"),
    label("h", "Hex"),
    label("s", "Size"),
    label("t", "Tolerance"),
];

/// Where the window's keys go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    /// The image: the canvas (or a button) has them, and no text is typed.
    Image,
    /// Text is typed on the canvas: the keys are its letters, Space, the arrows, Delete,
    /// and Escape.
    Typing,
    /// A text field: the size, the tolerance, the hex field, or the font list's search.
    Field,
}

/// Whether the window's key handler takes `binding` with the keys at `focus`: none while
/// a field has them, only the Ctrl+Shift ones while text is typed.
pub fn takes(binding: &Binding, focus: Focus) -> bool {
    matches!(binding.via, Via::Window | Via::Both)
        && match focus {
            Focus::Image => true,
            Focus::Typing => binding.combo.mods == Mods::CTRL_SHIFT,
            Focus::Field => false,
        }
}

/// The binding of the key named `key` (GDK's name, a letter in lower case) with `mods`
/// held that the window's key handler takes with the keys at `focus`.
pub fn window_binding(key: &str, mods: Mods, focus: Focus) -> Option<&'static Binding> {
    BINDINGS
        .iter()
        .find(|b| b.combo.mods == mods && b.combo.key == key)
        .filter(|b| takes(b, focus))
}

/// The actions' accelerators, each action's in the table's order.
pub fn accels() -> Vec<(&'static str, Vec<String>)> {
    let mut accels: Vec<(&'static str, Vec<String>)> = Vec::new();
    for b in BINDINGS {
        let (Command::Action(name), Via::Accel | Via::Both) = (b.command, b.via) else {
            continue;
        };
        match accels.iter_mut().find(|(n, _)| *n == name) {
            Some((_, keys)) => keys.push(b.combo.accel()),
            None => accels.push((name, vec![b.combo.accel()])),
        }
    }
    accels
}

/// The key the menu shows for the action `name`: its first binding's.
pub fn accel_of(name: &str) -> Option<String> {
    BINDINGS
        .iter()
        .find(|b| matches!(b.command, Command::Action(n) if n == name))
        .map(|b| b.combo.accel())
}

/// A menu item `label` running the action `name`, showing its key: the window's key
/// handler's too, which GTK's accelerators don't list.
pub fn menu_item(label: &str, name: &str) -> gio::MenuItem {
    let item = gio::MenuItem::new(Some(label), Some(name));
    if let Some(accel) = accel_of(name) {
        item.set_attribute_value("accel", Some(&accel.to_variant()));
    }
    item
}

/// `text` with its mnemonic underlined (`_Hex`): the letter its binding has, the first one
/// that is it in either case.
pub fn mnemonic(text: &'static str) -> String {
    let letter = BINDINGS
        .iter()
        .find(|b| b.command == Command::Label(text))
        .and_then(|b| b.combo.key.chars().next());
    match letter.and_then(|l| text.find(|c: char| c.to_ascii_lowercase() == l)) {
        Some(at) => format!("{}_{}", &text[..at], &text[at..]),
        None => text.to_owned(),
    }
}

/// The key that picks `tool`, as its tooltip shows it (upper case).
pub fn tool_key(tool: Tool) -> Option<char> {
    BINDINGS
        .iter()
        .find(|b| b.command == Command::Tool(tool))
        .and_then(|b| b.combo.key.chars().next())
        .map(|c| c.to_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk::gdk;
    use std::collections::HashMap;

    const SPEC: &str = include_str!("../../../docs/specs/etch.md");

    #[test]
    fn no_key_combination_is_bound_twice() {
        let mut seen: HashMap<Combo, Command> = HashMap::new();
        let mut twice = Vec::new();
        for b in BINDINGS {
            if let Some(first) = seen.insert(b.combo, b.command) {
                twice.push(format!(
                    "{} ({first:?} and {:?})",
                    b.combo.accel(),
                    b.command
                ));
            }
        }
        assert!(twice.is_empty(), "bound twice: {twice:?}");
    }

    #[test]
    fn every_key_is_a_gdk_key_by_its_own_name_a_letter_in_lower_case() {
        for b in BINDINGS {
            let key = gdk::Key::from_name(b.combo.key)
                .unwrap_or_else(|| panic!("{:?} is no GDK key", b.combo.key));
            assert_eq!(key.name().as_deref(), Some(b.combo.key));
            assert_eq!(key.to_lower(), key, "{:?} isn't lower case", b.combo.key);
        }
    }

    #[test]
    fn every_tool_has_its_own_key_unmodified() {
        for t in Tool::ALL {
            let keys: Vec<_> = BINDINGS
                .iter()
                .filter(|b| b.command == Command::Tool(t))
                .collect();
            assert_eq!(keys.len(), 1, "{t:?}'s keys");
            assert_eq!(keys[0].combo.mods, Mods::NONE);
            assert_eq!(keys[0].via, Via::Window);
            assert_eq!(
                tool_key(t),
                Some(
                    keys[0]
                        .combo
                        .key
                        .to_ascii_uppercase()
                        .chars()
                        .next()
                        .unwrap()
                )
            );
        }
    }

    #[test]
    fn no_tool_key_nor_x_while_text_is_typed_or_a_field_has_the_keys() {
        let picks = BINDINGS
            .iter()
            .filter(|b| matches!(b.command, Command::Tool(_) | Command::Swap));
        assert_eq!(picks.clone().count(), Tool::ALL.len() + 1);
        for b in picks {
            let key = b.combo.key;
            assert!(takes(b, Focus::Image), "{key} on the image");
            assert!(!takes(b, Focus::Typing), "{key} while text is typed");
            assert!(!takes(b, Focus::Field), "{key} in a field");
            assert_eq!(window_binding(key, Mods::NONE, Focus::Typing), None);
            assert_eq!(window_binding(key, Mods::NONE, Focus::Field), None);
            assert_eq!(window_binding(key, Mods::NONE, Focus::Image), Some(b));
        }
        // Space, the arrows, Delete, Escape, and Ctrl+A, X, C, V are the text's too; a
        // field keeps every key.
        for (key, mods) in [
            ("space", Mods::NONE),
            ("Left", Mods::NONE),
            ("Left", Mods::SHIFT),
            ("Delete", Mods::NONE),
            ("Escape", Mods::NONE),
            ("a", Mods::CTRL),
            ("x", Mods::CTRL),
            ("c", Mods::CTRL),
            ("v", Mods::CTRL),
        ] {
            assert!(window_binding(key, mods, Focus::Image).is_some(), "{key}");
            assert_eq!(window_binding(key, mods, Focus::Typing), None, "{key}");
        }
        for b in BINDINGS {
            assert!(!takes(b, Focus::Field), "{} in a field", b.combo.accel());
        }
        // While text is typed, the Ctrl+Shift ones still work.
        let redo = window_binding("z", Mods::CTRL_SHIFT, Focus::Typing);
        assert_eq!(redo.map(|b| b.command), Some(Command::Action("win.redo")));
    }

    #[test]
    fn accelerators_are_the_actions_keys_in_order() {
        let accels = accels();
        let of = |name: &str| {
            accels
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, k)| k.clone())
        };
        assert_eq!(
            of("win.zoom-in"),
            Some(vec![
                "<Control>plus".into(),
                "<Control>equal".into(),
                "<Control>KP_Add".into()
            ])
        );
        assert_eq!(of("win.save-as"), Some(vec!["<Control><Shift>s".into()]));
        assert_eq!(of("win.redo"), Some(vec!["<Control>y".into()]));
        assert_eq!(of("win.cut"), None);
        assert_eq!(accel_of("win.cut").as_deref(), Some("<Control>x"));
        assert_eq!(accel_of("win.deselect").as_deref(), Some("Escape"));
        assert_eq!(accel_of("win.flip-vertical"), None);
    }

    #[test]
    fn mnemonics_underline_their_letter() {
        for b in BINDINGS {
            if let Command::Label(text) = b.command {
                assert_eq!(b.combo.mods, Mods::ALT);
                let underlined = mnemonic(text);
                let letter = underlined.split('_').nth(1).and_then(|s| s.chars().next());
                assert_eq!(
                    letter.map(|c| c.to_ascii_lowercase().to_string()),
                    Some(b.combo.key.to_owned()),
                    "{text}"
                );
            }
        }
        assert_eq!(mnemonic("Hex"), "_Hex");
        assert_eq!(mnemonic("Tolerance"), "_Tolerance");
    }

    /// The key named in the spec's table (`Ctrl+Shift+S`, `Ctrl++`, `Escape`, `P`).
    fn spec_combo(text: &str) -> (Mods, String) {
        let (mods, key) = match text.strip_suffix("++") {
            Some(mods) => (mods, "+"),
            None => text.rsplit_once('+').unwrap_or(("", text)),
        };
        let mut held = Mods::NONE;
        for m in mods.split('+').filter(|m| !m.is_empty()) {
            match m {
                "Ctrl" => held.ctrl = true,
                "Shift" => held.shift = true,
                "Alt" => held.alt = true,
                other => panic!("the spec names a modifier {other:?}"),
            }
        }
        let key = match key {
            "+" => "plus".to_owned(),
            "-" => "minus".to_owned(),
            k if k.chars().count() == 1 => k.to_ascii_lowercase(),
            k => k.to_owned(),
        };
        (held, key)
    }

    /// The command the spec's table names (`Save As`, `Colour picker`).
    fn spec_command(name: &str) -> Command {
        if let Some(t) = Tool::ALL
            .into_iter()
            .find(|t| t.label().eq_ignore_ascii_case(name))
        {
            return Command::Tool(t);
        }
        Command::Action(match name {
            "New" => "app.new",
            "Open" => "app.open",
            "Save" => "win.save",
            "Save As" => "win.save-as",
            "Export" => "win.export",
            "Undo" => "win.undo",
            "Redo" => "win.redo",
            "Cut" => "win.cut",
            "Copy" => "win.copy",
            "Paste" => "win.paste",
            "Select All" => "win.select-all",
            "Deselect" => "win.deselect",
            "Delete selection" => "win.delete",
            "Zoom In" => "win.zoom-in",
            "Zoom Out" => "win.zoom-out",
            "Actual Size" => "win.actual-size",
            "Fit to Window" => "win.fit",
            "Crop to Selection" => "win.crop",
            "Resize Image" => "win.resize-image",
            "Resize Canvas" => "win.resize-canvas",
            "Rotate 90° Right" => "win.rotate-right",
            "Rotate 90° Left" => "win.rotate-left",
            "Rotate 180°" => "win.rotate-half",
            "Swap the foreground and background colours" => return Command::Swap,
            other => {
                panic!("the spec's shortcut table names {other:?}, which this test doesn't know")
            }
        })
    }

    /// The spec's shortcut table as (keys, command) pairs: a row's keys and commands pair
    /// up by `;` and then `,` (`Ctrl+Z; Ctrl+Shift+Z or Ctrl+Y` | `Undo; Redo`), each of a
    /// key's `or` alternatives with its command, and a parenthesis `(Ellipse: O)` is a pair
    /// of its own (`(100%)` a remark).
    fn spec_shortcuts() -> Vec<(String, String)> {
        let table = SPEC
            .split("\n## Shortcuts\n")
            .nth(1)
            .expect("the spec has a Shortcuts section");
        let mut pairs = Vec::new();
        for row in table
            .lines()
            .skip_while(|l| !l.starts_with('|'))
            .take_while(|l| l.starts_with('|'))
            .skip(2)
        {
            let cells: Vec<&str> = row.trim_matches('|').split('|').map(str::trim).collect();
            let [keys, mut commands] = cells[..] else {
                panic!("a shortcut row of two cells: {row}");
            };
            if let Some((rest, aside)) = commands.split_once(" (") {
                let aside = aside.trim_end_matches(')');
                if let Some((name, key)) = aside.split_once(": ") {
                    pairs.push((key.to_owned(), name.to_owned()));
                }
                commands = rest;
            }
            let keys: Vec<&str> = keys.split("; ").collect();
            let commands: Vec<&str> = commands.split("; ").collect();
            assert_eq!(keys.len(), commands.len(), "{row}");
            for (keys, commands) in keys.into_iter().zip(commands) {
                let keys: Vec<&str> = keys.split(", ").collect();
                let commands: Vec<&str> = commands.split(", ").collect();
                assert_eq!(keys.len(), commands.len(), "{row}");
                for (key, command) in keys.into_iter().zip(commands) {
                    for key in key.split(" or ") {
                        pairs.push((key.to_owned(), command.to_owned()));
                    }
                }
            }
        }
        pairs
    }

    #[test]
    fn every_shortcut_in_the_spec_is_bound_to_its_command() {
        let pairs = spec_shortcuts();
        assert!(pairs.len() >= 30, "the spec's table read: {pairs:?}");
        for (keys, name) in &pairs {
            let (mods, key) = spec_combo(keys);
            let bound = BINDINGS
                .iter()
                .find(|b| b.combo.mods == mods && b.combo.key == key);
            assert_eq!(
                bound.map(|b| b.command),
                Some(spec_command(name)),
                "the spec's {keys} ({name})"
            );
        }
        // Every tool is in the spec's table, and X.
        for t in Tool::ALL {
            assert!(
                pairs
                    .iter()
                    .any(|(_, n)| spec_command(n) == Command::Tool(t)),
                "{t:?}"
            );
        }
        assert!(pairs
            .iter()
            .any(|(k, n)| k == "X" && spec_command(n) == Command::Swap));
    }

    #[test]
    fn modifiers_beyond_ctrl_shift_and_alt_bind_nothing() {
        use gdk::ModifierType as G;
        assert_eq!(Mods::of(G::empty()), Some(Mods::NONE));
        assert_eq!(
            Mods::of(G::CONTROL_MASK | G::SHIFT_MASK),
            Some(Mods::CTRL_SHIFT)
        );
        // Caps Lock and Num Lock aren't modifiers a key is bound with.
        assert_eq!(Mods::of(G::LOCK_MASK), Some(Mods::NONE));
        assert_eq!(Mods::of(G::SUPER_MASK), None);
    }
}
