//! Every key Etch binds, in one table ([`BINDINGS`]): the tool keys and X, the menus'
//! shortcuts, the window's own keys (the selection's, Space), and the mnemonics of the
//! menu bar and the colour row. The app takes them from it: GTK's accelerators
//! ([`accels`]), the keys the menus show ([`menu_item`]), the window's key handler
//! ([`window_key`]), the labels' underlined letters ([`mnemonic`]), and the tools' keys
//! ([`tool_key`]). A menu's own items and a dialog's mnemonics work only while it's open,
//! so they aren't here.
//!
//! The table's shortcuts hold the platform's primary modifier (MAC-3, `suite_ui::keys`):
//! Ctrl on Linux, ⌘ on macOS. A binding can be one platform's alone ([`Binding::on`]):
//! Ctrl+Y redoes on Linux only, and Rotate 90° Right is Ctrl+H on Linux and ⌘] on macOS,
//! where ⌘H hides the app.
//!
//! Where a key goes depends on what has the keys ([`Focus`], [`takes`]): a field (the
//! size, the tolerance, the hex field, the font list's search) keeps every key, and text
//! being typed on the canvas every key but the primary+Shift ones (Ctrl+Shift, ⇧⌘), so
//! tool keys and X are letters there (docs/specs/etch.md).

use crate::tools::Tool;
use gtk::gdk;
use gtk::gio;
use gtk::prelude::*;
use suite_ui::keys::Primary;

/// The modifiers held with a key. `primary` is the platform's primary modifier: Ctrl on
/// Linux, ⌘ on macOS.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Mods {
    pub primary: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Mods {
    pub const NONE: Mods = Mods::new(false, false, false);
    pub const SHIFT: Mods = Mods::new(false, true, false);
    pub const PRIMARY: Mods = Mods::new(true, false, false);
    pub const PRIMARY_SHIFT: Mods = Mods::new(true, true, false);
    pub const ALT: Mods = Mods::new(false, false, true);

    const fn new(primary: bool, shift: bool, alt: bool) -> Mods {
        Mods {
            primary,
            shift,
            alt,
        }
    }

    /// The modifiers of a key event's `state` with `p` the primary modifier (Caps Lock and
    /// Num Lock aside); `None` with any other held, which nothing binds: Super, Hyper, and
    /// Meta on Linux, Super, Hyper, and Control (⌃) on macOS, where ⌘ is Meta.
    pub fn of(p: Primary, state: gdk::ModifierType) -> Option<Mods> {
        use gdk::ModifierType as M;
        let others =
            (M::SUPER_MASK | M::HYPER_MASK | M::META_MASK | M::CONTROL_MASK).difference(p.mask());
        (state & others).is_empty().then(|| Mods {
            primary: p.held(state),
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
    /// As the suite writes a shortcut (`suite_ui::keys`): `<Primary><Shift>s`.
    pub fn spec(self) -> String {
        let mut spec = String::new();
        for (on, name) in [
            (self.mods.primary, "<Primary>"),
            (self.mods.shift, "<Shift>"),
            (self.mods.alt, "<Alt>"),
        ] {
            if on {
                spec.push_str(name);
            }
        }
        spec.push_str(self.key);
        spec
    }

    /// As a GTK accelerator with `p` the primary modifier: `<Control><Shift>s` on Linux,
    /// `<Meta><Shift>s` on macOS.
    pub fn accel(self, p: Primary) -> String {
        p.accel(&self.spec())
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
    /// The window's key handler, which takes a primary+Shift letter (Ctrl+Shift, ⇧⌘)
    /// however its case arrives (an accelerator can miss an upper case one), and GTK's
    /// accelerator while a field has the keys.
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
    /// The primary modifier it's bound with alone, if it's one platform's.
    pub only: Option<Primary>,
}

impl Binding {
    /// Whether it's bound with `p` the primary modifier.
    pub fn on(&self, p: Primary) -> bool {
        self.only.is_none_or(|o| o == p)
    }

    /// It, bound with `p` the primary modifier alone.
    const fn only(self, p: Primary) -> Binding {
        Binding {
            only: Some(p),
            ..self
        }
    }
}

const fn bind(mods: Mods, key: &'static str, command: Command, via: Via) -> Binding {
    Binding {
        combo: Combo { mods, key },
        command,
        via,
        only: None,
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

/// Every key Etch binds. An action's first binding on a platform is the one its menu item
/// shows there. `M::PRIMARY` is Ctrl on Linux and ⌘ on macOS.
pub const BINDINGS: &[Binding] = &[
    // File.
    action(M::PRIMARY, "n", "app.new", Accel),
    action(M::PRIMARY, "o", "app.open", Accel),
    action(M::PRIMARY, "s", "win.save", Accel),
    action(M::PRIMARY_SHIFT, "s", "win.save-as", Both),
    action(M::PRIMARY_SHIFT, "e", "win.export", Both),
    action(M::PRIMARY, "w", "window.close", Accel),
    action(M::PRIMARY, "q", "app.quit", Accel),
    // Edit. Ctrl+Shift+Z is the window's alone: a field keeps its own redo. Cut, copy,
    // paste, select all, deselect, and delete are the window's so a field and the text
    // being typed keep theirs. Ctrl+Y is Linux's alone: macOS redoes with ⇧⌘Z.
    action(M::PRIMARY, "z", "win.undo", Accel),
    action(M::PRIMARY, "y", "win.redo", Accel).only(Primary::Control),
    action(M::PRIMARY_SHIFT, "z", "win.redo", Window),
    action(M::PRIMARY, "x", "win.cut", Window),
    action(M::PRIMARY, "c", "win.copy", Window),
    action(M::PRIMARY, "v", "win.paste", Window),
    action(M::PRIMARY, "a", "win.select-all", Window),
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
    action(M::PRIMARY, "plus", "win.zoom-in", Accel),
    action(M::PRIMARY, "equal", "win.zoom-in", Accel),
    action(M::PRIMARY, "KP_Add", "win.zoom-in", Accel),
    action(M::PRIMARY, "minus", "win.zoom-out", Accel),
    action(M::PRIMARY, "KP_Subtract", "win.zoom-out", Accel),
    action(M::PRIMARY, "0", "win.actual-size", Accel),
    action(M::PRIMARY, "KP_0", "win.actual-size", Accel),
    action(M::PRIMARY, "9", "win.fit", Accel),
    // Image (the spec's; Paint.NET's). The flips have none. ⌘H hides the app on macOS, so
    // Rotate 90° Right is ⌘] there (Pixelmator's; Preview's ⌘R is Resize Image here).
    action(M::PRIMARY_SHIFT, "x", "win.crop", Both),
    action(M::PRIMARY, "r", "win.resize-image", Accel),
    action(M::PRIMARY_SHIFT, "r", "win.resize-canvas", Both),
    action(M::PRIMARY, "h", "win.rotate-right", Accel).only(Primary::Control),
    action(M::PRIMARY, "bracketright", "win.rotate-right", Accel).only(Primary::Command),
    action(M::PRIMARY, "g", "win.rotate-left", Accel),
    action(M::PRIMARY, "j", "win.rotate-half", Accel),
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
/// a field has them, only the primary+Shift ones while text is typed.
pub fn takes(binding: &Binding, focus: Focus) -> bool {
    matches!(binding.via, Via::Window | Via::Both)
        && match focus {
            Focus::Image => true,
            Focus::Typing => binding.combo.mods == Mods::PRIMARY_SHIFT,
            Focus::Field => false,
        }
}

/// The bindings with `p` the primary modifier.
pub fn bindings(p: Primary) -> impl Iterator<Item = &'static Binding> {
    BINDINGS.iter().filter(move |b| b.on(p))
}

/// The binding of the key named `key` (GDK's name, a letter in lower case) with `mods`
/// held that the window's key handler takes with the keys at `focus`, `p` the primary
/// modifier.
pub fn window_binding(p: Primary, key: &str, mods: Mods, focus: Focus) -> Option<&'static Binding> {
    bindings(p)
        .find(|b| b.combo.mods == mods && b.combo.key == key)
        .filter(|b| takes(b, focus))
}

/// The binding the window's key handler takes for a press of `key` with `state` held at
/// `focus`, `p` the primary modifier: a letter in either case.
pub fn window_key(
    p: Primary,
    key: gdk::Key,
    state: gdk::ModifierType,
    focus: Focus,
) -> Option<&'static Binding> {
    Mods::of(p, state)
        .zip(key.to_lower().name())
        .and_then(|(mods, name)| window_binding(p, &name, mods, focus))
}

/// The actions' accelerators with `p` the primary modifier, each action's in the table's
/// order.
pub fn accels(p: Primary) -> Vec<(&'static str, Vec<String>)> {
    let mut accels: Vec<(&'static str, Vec<String>)> = Vec::new();
    for b in bindings(p) {
        let (Command::Action(name), Via::Accel | Via::Both) = (b.command, b.via) else {
            continue;
        };
        match accels.iter_mut().find(|(n, _)| *n == name) {
            Some((_, keys)) => keys.push(b.combo.accel(p)),
            None => accels.push((name, vec![b.combo.accel(p)])),
        }
    }
    accels
}

/// The key the menu shows for the action `name` with `p` the primary modifier: its first
/// binding's.
pub fn accel_of(p: Primary, name: &str) -> Option<String> {
    bindings(p)
        .find(|b| matches!(b.command, Command::Action(n) if n == name))
        .map(|b| b.combo.accel(p))
}

/// A menu item `label` running the action `name`, showing its key on this platform: the
/// window's key handler's too, which GTK's accelerators don't list.
pub fn menu_item(label: &str, name: &str) -> gio::MenuItem {
    let item = gio::MenuItem::new(Some(label), Some(name));
    if let Some(accel) = accel_of(Primary::HERE, name) {
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
    use std::collections::HashMap;
    use suite_ui::keys;

    const SPEC: &str = include_str!("../../../docs/specs/etch.md");

    #[test]
    fn no_key_combination_is_bound_twice_nor_one_macos_keeps() {
        for p in Primary::ALL {
            let mut seen: HashMap<Combo, Command> = HashMap::new();
            let mut twice = Vec::new();
            for b in bindings(p) {
                if let Some(first) = seen.insert(b.combo, b.command) {
                    twice.push(format!(
                        "{} ({first:?} and {:?})",
                        b.combo.accel(p),
                        b.command
                    ));
                }
            }
            assert!(twice.is_empty(), "{p:?}: bound twice: {twice:?}");
            let specs: Vec<(String, String)> = bindings(p)
                .map(|b| (format!("{:?}", b.command), b.combo.spec()))
                .collect();
            let clashes = keys::clashes(p, specs.iter().map(|(n, s)| (n.as_str(), s.as_str())));
            assert_eq!(clashes, Vec::<String>::new(), "{p:?}");
        }
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
        for p in Primary::ALL {
            for b in picks.clone() {
                let key = b.combo.key;
                assert!(takes(b, Focus::Image), "{key} on the image");
                assert!(!takes(b, Focus::Typing), "{key} while text is typed");
                assert!(!takes(b, Focus::Field), "{key} in a field");
                assert_eq!(window_binding(p, key, Mods::NONE, Focus::Typing), None);
                assert_eq!(window_binding(p, key, Mods::NONE, Focus::Field), None);
                assert_eq!(window_binding(p, key, Mods::NONE, Focus::Image), Some(b));
            }
            // Space, the arrows, Delete, Escape, and Ctrl+A, X, C, V (⌘ on macOS) are the
            // text's too; a field keeps every key.
            for (key, mods) in [
                ("space", Mods::NONE),
                ("Left", Mods::NONE),
                ("Left", Mods::SHIFT),
                ("Delete", Mods::NONE),
                ("Escape", Mods::NONE),
                ("a", Mods::PRIMARY),
                ("x", Mods::PRIMARY),
                ("c", Mods::PRIMARY),
                ("v", Mods::PRIMARY),
            ] {
                assert!(
                    window_binding(p, key, mods, Focus::Image).is_some(),
                    "{key}"
                );
                assert_eq!(window_binding(p, key, mods, Focus::Typing), None, "{key}");
            }
            // While text is typed, the primary+Shift ones still work.
            let redo = window_binding(p, "z", Mods::PRIMARY_SHIFT, Focus::Typing);
            assert_eq!(redo.map(|b| b.command), Some(Command::Action("win.redo")));
        }
        for b in BINDINGS {
            assert!(!takes(b, Focus::Field), "{} in a field", b.combo.spec());
        }
    }

    /// Etch's accelerators on Linux before MAC-3, written out: they stay as they were.
    const LINUX_ACCELS: &[(&str, &[&str])] = &[
        ("app.new", &["<Control>n"]),
        ("app.open", &["<Control>o"]),
        ("win.save", &["<Control>s"]),
        ("win.save-as", &["<Control><Shift>s"]),
        ("win.export", &["<Control><Shift>e"]),
        ("window.close", &["<Control>w"]),
        ("app.quit", &["<Control>q"]),
        ("win.undo", &["<Control>z"]),
        ("win.redo", &["<Control>y"]),
        (
            "win.zoom-in",
            &["<Control>plus", "<Control>equal", "<Control>KP_Add"],
        ),
        ("win.zoom-out", &["<Control>minus", "<Control>KP_Subtract"]),
        ("win.actual-size", &["<Control>0", "<Control>KP_0"]),
        ("win.fit", &["<Control>9"]),
        ("win.crop", &["<Control><Shift>x"]),
        ("win.resize-image", &["<Control>r"]),
        ("win.resize-canvas", &["<Control><Shift>r"]),
        ("win.rotate-right", &["<Control>h"]),
        ("win.rotate-left", &["<Control>g"]),
        ("win.rotate-half", &["<Control>j"]),
    ];

    /// The keys Linux's menus showed before MAC-3 for the window's own actions.
    const LINUX_MENU_KEYS: &[(&str, &str)] = &[
        ("win.redo", "<Control>y"),
        ("win.cut", "<Control>x"),
        ("win.copy", "<Control>c"),
        ("win.paste", "<Control>v"),
        ("win.select-all", "<Control>a"),
        ("win.deselect", "Escape"),
        ("win.delete", "Delete"),
    ];

    fn owned(list: &[(&'static str, &[&str])]) -> Vec<(&'static str, Vec<String>)> {
        list.iter()
            .map(|(n, keys)| (*n, keys.iter().map(|k| k.to_string()).collect()))
            .collect()
    }

    #[test]
    fn linux_accelerators_are_the_actions_keys_in_order_as_before() {
        let p = Primary::Control;
        assert_eq!(accels(p), owned(LINUX_ACCELS));
        for (name, keys) in LINUX_ACCELS {
            assert_eq!(accel_of(p, name).as_deref(), Some(keys[0]), "{name}");
        }
        for (name, key) in LINUX_MENU_KEYS {
            assert_eq!(accel_of(p, name).as_deref(), Some(*key), "{name}");
        }
        let accels = accels(p);
        assert!(!accels.iter().any(|(n, _)| *n == "win.cut"));
        assert_eq!(accel_of(p, "win.flip-vertical"), None);
    }

    #[test]
    fn macos_accelerators_are_command_where_linux_has_ctrl() {
        let p = Primary::Command;
        // Linux's, with ⌘ for Ctrl; but Ctrl+Y (Linux's alone: macOS redoes with ⇧⌘Z) and
        // Ctrl+H (⌘H hides the app: ⌘] rotates right).
        let mac: Vec<(&str, Vec<String>)> = owned(LINUX_ACCELS)
            .into_iter()
            .filter(|(n, _)| *n != "win.redo")
            .map(|(n, keys)| match n {
                "win.rotate-right" => (n, vec!["<Meta>bracketright".to_owned()]),
                _ => (
                    n,
                    keys.iter()
                        .map(|k| k.replace("<Control>", "<Meta>"))
                        .collect(),
                ),
            })
            .collect();
        assert_eq!(accels(p), mac);
        assert_eq!(accel_of(p, "win.redo").as_deref(), Some("<Meta><Shift>z"));
        assert_eq!(accel_of(p, "win.cut").as_deref(), Some("<Meta>x"));
        assert_eq!(accel_of(p, "win.deselect").as_deref(), Some("Escape"));
        // No shortcut stays on Control on macOS: Etch has no exceptions.
        let on_control: Vec<String> = bindings(p)
            .map(|b| b.combo.accel(p))
            .filter(|a| a.contains("<Control>"))
            .collect();
        assert_eq!(on_control, Vec::<String>::new());
    }

    #[test]
    fn rotate_right_and_ctrl_y_are_one_platforms() {
        // ⌘H is macOS's Hide, so Ctrl+H can't be Rotate 90° Right there.
        assert!(!keys::clashes(Primary::Command, [("rotate-right", "<Primary>h")]).is_empty());
        let only: Vec<(String, Option<Primary>)> = BINDINGS
            .iter()
            .filter(|b| b.only.is_some())
            .map(|b| (b.combo.spec(), b.only))
            .collect();
        assert_eq!(
            only,
            [
                ("<Primary>y".to_owned(), Some(Primary::Control)),
                ("<Primary>h".to_owned(), Some(Primary::Control)),
                ("<Primary>bracketright".to_owned(), Some(Primary::Command)),
            ]
        );
        let rotate = |p: Primary| -> Vec<String> {
            bindings(p)
                .filter(|b| b.command == Command::Action("win.rotate-right"))
                .map(|b| b.combo.accel(p))
                .collect()
        };
        assert_eq!(rotate(Primary::Control), ["<Control>h"]);
        assert_eq!(rotate(Primary::Command), ["<Meta>bracketright"]);
    }

    #[test]
    fn the_window_takes_its_keys_with_the_primary_modifier() {
        use gdk::ModifierType as G;
        use Primary::{Command as Cmd, Control as Ctl};
        let run = |p, key, state, focus| window_key(p, key, state, focus).map(|b| b.command);
        let cut = Some(Command::Action("win.cut"));
        let redo = Some(Command::Action("win.redo"));
        // ⌘X cuts on macOS and ⌃X doesn't; Ctrl+X cuts on Linux and Meta+X doesn't.
        assert_eq!(run(Cmd, gdk::Key::x, G::META_MASK, Focus::Image), cut);
        assert_eq!(run(Cmd, gdk::Key::x, G::CONTROL_MASK, Focus::Image), None);
        assert_eq!(run(Ctl, gdk::Key::x, G::CONTROL_MASK, Focus::Image), cut);
        assert_eq!(run(Ctl, gdk::Key::x, G::META_MASK, Focus::Image), None);
        // ⇧⌘Z and Ctrl+Shift+Z redo, the letter in either case, while text is typed too.
        let shifted = G::SHIFT_MASK | G::LOCK_MASK;
        assert_eq!(
            run(Cmd, gdk::Key::Z, G::META_MASK | shifted, Focus::Typing),
            redo
        );
        assert_eq!(
            run(Ctl, gdk::Key::Z, G::CONTROL_MASK | shifted, Focus::Typing),
            redo
        );
        assert_eq!(
            run(Cmd, gdk::Key::Z, G::CONTROL_MASK | shifted, Focus::Typing),
            None
        );
        // ⌘Z undoes on macOS (an accelerator) and ⌃Z doesn't; Ctrl+Z undoes on Linux.
        let undo = BINDINGS
            .iter()
            .find(|b| b.command == Command::Action("win.undo"))
            .map(|b| b.combo.spec())
            .unwrap();
        assert_eq!(undo, "<Primary>z");
        assert!(Cmd.matches(&undo, gdk::Key::z, G::META_MASK));
        assert!(!Cmd.matches(&undo, gdk::Key::z, G::CONTROL_MASK));
        assert!(Ctl.matches(&undo, gdk::Key::z, G::CONTROL_MASK));
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
                "Ctrl" => held.primary = true,
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

    /// The spec's table names Linux's keys (Ctrl); macOS's are in the paragraph after it.
    #[test]
    fn every_shortcut_in_the_spec_is_bound_to_its_command() {
        let pairs = spec_shortcuts();
        assert!(pairs.len() >= 30, "the spec's table read: {pairs:?}");
        for (keys, name) in &pairs {
            let (mods, key) = spec_combo(keys);
            let bound =
                bindings(Primary::Control).find(|b| b.combo.mods == mods && b.combo.key == key);
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
    fn modifiers_beyond_the_primary_shift_and_alt_bind_nothing() {
        use gdk::ModifierType as G;
        for p in Primary::ALL {
            assert_eq!(Mods::of(p, G::empty()), Some(Mods::NONE));
            assert_eq!(
                Mods::of(p, p.mask() | G::SHIFT_MASK),
                Some(Mods::PRIMARY_SHIFT)
            );
            // Caps Lock and Num Lock aren't modifiers a key is bound with.
            assert_eq!(Mods::of(p, G::LOCK_MASK), Some(Mods::NONE));
            assert_eq!(Mods::of(p, G::SUPER_MASK), None);
            assert_eq!(Mods::of(p, G::HYPER_MASK), None);
        }
        // Linux: Ctrl is the primary modifier, and Meta binds nothing.
        let linux = Primary::Control;
        assert_eq!(Mods::of(linux, G::CONTROL_MASK), Some(Mods::PRIMARY));
        assert_eq!(Mods::of(linux, G::META_MASK), None);
        // macOS: ⌘ (GDK's Meta) is, and Control (⌃) binds nothing.
        let mac = Primary::Command;
        assert_eq!(Mods::of(mac, G::META_MASK), Some(Mods::PRIMARY));
        assert_eq!(Mods::of(mac, G::CONTROL_MASK), None);
        assert_eq!(Mods::of(mac, G::META_MASK | G::CONTROL_MASK), None);
    }
}
