//! The primary modifier (ADR 0011, MAC-3): Control on Linux, Command on macOS. Every
//! shortcut in the suite is written once, with `<Primary>` where the platform's own
//! modifier goes (`<Primary><Shift>s`), and comes from here as a GTK accelerator
//! ([`accel`]), as the text a tooltip shows ([`label`]), and as what a key handler matches
//! ([`matches`], [`is_primary`]).
//!
//! GTK 4 doesn't do this itself: its own `<Primary>` parses as Control on macOS too, and
//! its macOS backend reports ⌘ as `META_MASK`. So `<Primary>` becomes `<Control>` or
//! `<Meta>` here, and the mask is `CONTROL_MASK` or `META_MASK`.
//!
//! Each [`Primary`] does it for one platform, so the apps' tests check their shortcut
//! tables for both on either: no binding twice ([`clashes`]), and none of the keys macOS
//! keeps for itself ([`MAC_RESERVED`]).

use gtk::gdk::{self, ModifierType};

/// The platform's primary modifier: what Ctrl+S is on Linux and ⌘S on macOS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Primary {
    /// Linux (and Windows): Ctrl.
    Control,
    /// macOS: ⌘.
    Command,
}

/// The modifiers a shortcut is told apart by. Caps Lock and the mouse buttons aren't
/// among them. Linux's Meta isn't either: on X11 Alt can report it too.
const SHORTCUT_MODS: ModifierType = ModifierType::CONTROL_MASK
    .union(ModifierType::SHIFT_MASK)
    .union(ModifierType::ALT_MASK)
    .union(ModifierType::SUPER_MASK)
    .union(ModifierType::HYPER_MASK);

impl Primary {
    /// This build's: Command on macOS, Control elsewhere.
    pub const HERE: Primary = if cfg!(target_os = "macos") {
        Primary::Command
    } else {
        Primary::Control
    };

    /// Both, for tests over a shortcut table.
    pub const ALL: [Primary; 2] = [Primary::Control, Primary::Command];

    /// The modifier GDK reports for it.
    pub const fn mask(self) -> ModifierType {
        match self {
            Primary::Control => ModifierType::CONTROL_MASK,
            Primary::Command => ModifierType::META_MASK,
        }
    }

    /// The accelerator name GTK parses for it.
    const fn accel_name(self) -> &'static str {
        match self {
            Primary::Control => "<Control>",
            Primary::Command => "<Meta>",
        }
    }

    /// `control` on Linux, `command` on macOS: for a shortcut macOS has another key for
    /// (Replace is Ctrl+H and ⌥⌘F, as ⌘H hides the app).
    pub const fn pick<'a>(self, control: &'a str, command: &'a str) -> &'a str {
        match self {
            Primary::Control => control,
            Primary::Command => command,
        }
    }

    /// Whether `state` holds the primary modifier.
    pub fn held(self, state: ModifierType) -> bool {
        state.contains(self.mask())
    }

    /// `spec` as a GTK accelerator: each `<Primary>` becomes `<Control>` or `<Meta>`.
    pub fn accel(self, spec: &str) -> String {
        spec.replace("<Primary>", self.accel_name())
    }

    /// The key and modifiers `spec` names, or `None` for a key GDK doesn't know. The key
    /// is lower case: Shift is among the modifiers.
    pub fn parse(self, spec: &str) -> Option<(gdk::Key, ModifierType)> {
        let (mods, name) = split(spec)?;
        let mut state = ModifierType::empty();
        for m in mods {
            state |= match m {
                Mod::Primary => self.mask(),
                Mod::Control => ModifierType::CONTROL_MASK,
                Mod::Meta => ModifierType::META_MASK,
                Mod::Shift => ModifierType::SHIFT_MASK,
                Mod::Alt => ModifierType::ALT_MASK,
                Mod::Super => ModifierType::SUPER_MASK,
            };
        }
        let key = gdk::Key::from_name(name)?;
        Some((key.to_lower(), state))
    }

    /// Whether a key press of `key` with `state` held is the shortcut `spec`: the same
    /// key, either case, and the same modifiers, no more.
    pub fn matches(self, spec: &str, key: gdk::Key, state: ModifierType) -> bool {
        let Some((want, mods)) = self.parse(spec) else {
            return false;
        };
        let relevant = SHORTCUT_MODS | self.mask();
        key.to_lower() == want && state & relevant == mods
    }

    /// `spec` as a tooltip or a menu shows it: `Ctrl+Shift+S` on Linux, `⇧⌘S` on macOS.
    pub fn label(self, spec: &str) -> String {
        let Some((mods, name)) = split(spec) else {
            return spec.to_owned();
        };
        let has = |m: Mod| mods.contains(&m);
        let key = key_label(self, name);
        match self {
            Primary::Control => {
                let mut out = String::new();
                for (on, text) in [
                    (has(Mod::Primary) || has(Mod::Control), "Ctrl+"),
                    (has(Mod::Super) || has(Mod::Meta), "Super+"),
                    (has(Mod::Shift), "Shift+"),
                    (has(Mod::Alt), "Alt+"),
                ] {
                    if on {
                        out.push_str(text);
                    }
                }
                out + &key
            }
            // Apple's order: Control, Option, Shift, Command.
            Primary::Command => {
                let mut out = String::new();
                for (on, text) in [
                    (has(Mod::Control), "⌃"),
                    (has(Mod::Alt), "⌥"),
                    (has(Mod::Shift), "⇧"),
                    (has(Mod::Primary) || has(Mod::Meta) || has(Mod::Super), "⌘"),
                ] {
                    if on {
                        out.push_str(text);
                    }
                }
                out + &key
            }
        }
    }
}

/// The primary modifier's mask on this platform: `CONTROL_MASK` or `META_MASK`.
pub fn primary() -> ModifierType {
    Primary::HERE.mask()
}

/// Whether `state` holds this platform's primary modifier.
pub fn is_primary(state: ModifierType) -> bool {
    Primary::HERE.held(state)
}

/// `spec` as this platform's GTK accelerator: `<Primary>s` is `<Control>s` or `<Meta>s`.
pub fn accel(spec: &str) -> String {
    Primary::HERE.accel(spec)
}

/// `spec` as this platform shows it: `Ctrl+S` or `⌘S`.
pub fn label(spec: &str) -> String {
    Primary::HERE.label(spec)
}

/// Whether a press of `key` with `state` is the shortcut `spec` on this platform.
pub fn matches(spec: &str, key: gdk::Key, state: ModifierType) -> bool {
    Primary::HERE.matches(spec, key, state)
}

/// The shortcuts macOS keeps for itself or for the app menu GTK makes there, so a window
/// never sees them: Hide (⌘H), Hide Others (⌥⌘H), Minimize (⌘M), Settings (⌘,), the next
/// window (⌘`), Spotlight (⌘Space, ⌥⌘Space), the app switcher (⌘Tab), screenshots (⇧⌘3,
/// ⇧⌘4, ⇧⌘5), full screen (⌃⌘F), lock screen (⌃⌘Q), log out (⇧⌘Q), Force Quit (⌥⌘Esc),
/// the Dock (⌥⌘D), and Help's search (⇧⌘/).
pub const MAC_RESERVED: &[&str] = &[
    "<Meta>h",
    "<Alt><Meta>h",
    "<Meta>m",
    "<Meta>comma",
    "<Meta>grave",
    "<Meta>space",
    "<Alt><Meta>space",
    "<Meta>Tab",
    "<Shift><Meta>3",
    "<Shift><Meta>4",
    "<Shift><Meta>5",
    "<Control><Meta>f",
    "<Control><Meta>q",
    "<Shift><Meta>q",
    "<Alt><Meta>Escape",
    "<Alt><Meta>d",
    "<Shift><Meta>slash",
];

/// What's wrong with a table of shortcuts for `primary`: a spec GDK can't parse, one bound
/// twice (by `specs`' names, the first item of each pair), and, for Command, one macOS
/// keeps for itself. Empty when it's fine.
pub fn clashes<'a>(
    primary: Primary,
    specs: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Vec<String> {
    let mut problems = Vec::new();
    let mut seen: Vec<((gdk::Key, ModifierType), &str, &str)> = Vec::new();
    let reserved: Vec<_> = MAC_RESERVED
        .iter()
        .filter_map(|r| Primary::Command.parse(r).map(|p| (p, *r)))
        .collect();
    for (name, spec) in specs {
        let Some(parsed) = primary.parse(spec) else {
            problems.push(format!("{name}: {spec} isn't a key GDK knows"));
            continue;
        };
        if let Some((_, other, other_spec)) = seen.iter().find(|(p, _, _)| *p == parsed) {
            problems.push(format!(
                "{primary:?}: {name} ({spec}) and {other} ({other_spec}) are the same key"
            ));
        }
        if primary == Primary::Command {
            if let Some((_, r)) = reserved.iter().find(|(p, _)| *p == parsed) {
                problems.push(format!("{name}: {spec} is macOS's {r}"));
            }
        }
        seen.push((parsed, name, spec));
    }
    problems
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mod {
    Primary,
    Control,
    Meta,
    Shift,
    Alt,
    Super,
}

/// `spec`'s modifiers and its key's GDK name.
fn split(spec: &str) -> Option<(Vec<Mod>, &str)> {
    let mut mods = Vec::new();
    let mut rest = spec;
    while let Some(after) = rest.strip_prefix('<') {
        let (name, tail) = after.split_once('>')?;
        mods.push(match name.to_ascii_lowercase().as_str() {
            "primary" => Mod::Primary,
            "control" | "ctrl" | "ctl" => Mod::Control,
            "meta" => Mod::Meta,
            "shift" | "shft" => Mod::Shift,
            "alt" | "mod1" => Mod::Alt,
            "super" => Mod::Super,
            _ => return None,
        });
        rest = tail;
    }
    (!rest.is_empty()).then_some((mods, rest))
}

/// A key's name as a label shows it: `S`, `Page Down`, `+`; on macOS its symbol (`↩`).
fn key_label(primary: Primary, name: &str) -> String {
    let mac = primary == Primary::Command;
    let symbol = match name {
        "plus" | "KP_Add" => "+",
        "minus" | "KP_Subtract" => "-",
        "equal" => "=",
        "bracketleft" => "[",
        "bracketright" => "]",
        "comma" => ",",
        "period" => ".",
        "slash" => "/",
        "backslash" => "\\",
        "grave" => "`",
        "semicolon" => ";",
        "apostrophe" => "'",
        "space" => "Space",
        "Return" | "KP_Enter" if mac => "↩",
        "Return" | "KP_Enter" => "Enter",
        "BackSpace" if mac => "⌫",
        "BackSpace" => "Backspace",
        "Delete" if mac => "⌦",
        "Escape" if mac => "⎋",
        "Escape" => "Esc",
        "Tab" if mac => "⇥",
        "Left" if mac => "←",
        "Right" if mac => "→",
        "Up" if mac => "↑",
        "Down" if mac => "↓",
        "Home" if mac => "↖",
        "End" if mac => "↘",
        "Page_Up" if mac => "⇞",
        "Page_Down" if mac => "⇟",
        _ => {
            let mut chars = name.chars();
            return match (chars.next(), chars.next()) {
                (Some(c), None) => c.to_uppercase().collect(),
                _ => name.replace('_', " "),
            };
        }
    };
    symbol.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdk::Key as K;
    use ModifierType as M;

    #[test]
    fn primary_is_control_on_linux_and_command_on_macos() {
        let here = if cfg!(target_os = "macos") {
            Primary::Command
        } else {
            Primary::Control
        };
        assert_eq!(Primary::HERE, here);
        assert_eq!(Primary::Control.mask(), M::CONTROL_MASK);
        // GTK's macOS backend reports ⌘ as Meta (gdkmacosdisplay-translate.c).
        assert_eq!(Primary::Command.mask(), M::META_MASK);
        assert_eq!(primary(), here.mask());
        assert_eq!(accel("<Primary>z"), here.accel("<Primary>z"));
        assert_eq!(label("<Primary>z"), here.label("<Primary>z"));
    }

    #[test]
    fn accelerators_get_the_platforms_modifier() {
        assert_eq!(Primary::Control.accel("<Primary>o"), "<Control>o");
        assert_eq!(Primary::Command.accel("<Primary>o"), "<Meta>o");
        assert_eq!(
            Primary::Command.accel("<Primary><Shift>s"),
            "<Meta><Shift>s"
        );
        assert_eq!(Primary::Command.accel("F5"), "F5");
        assert_eq!(Primary::Command.accel("<Control>space"), "<Control>space");
        assert_eq!(
            Primary::Control.pick("<Primary>h", "<Alt><Primary>f"),
            "<Primary>h"
        );
        assert_eq!(
            Primary::Command.pick("<Primary>h", "<Alt><Primary>f"),
            "<Alt><Primary>f"
        );
    }

    #[test]
    fn labels_read_as_each_platform_writes_them() {
        let (c, m) = (Primary::Control, Primary::Command);
        assert_eq!(c.label("<Primary>o"), "Ctrl+O");
        assert_eq!(m.label("<Primary>o"), "⌘O");
        assert_eq!(c.label("<Primary><Shift>s"), "Ctrl+Shift+S");
        assert_eq!(m.label("<Primary><Shift>s"), "⇧⌘S");
        assert_eq!(m.label("<Shift><Alt><Primary>f"), "⌥⇧⌘F");
        assert_eq!(c.label("<Primary>bracketright"), "Ctrl+]");
        assert_eq!(m.label("<Primary>5"), "⌘5");
        assert_eq!(c.label("<Primary>Return"), "Ctrl+Enter");
        assert_eq!(m.label("<Primary>Return"), "⌘↩");
        assert_eq!(c.label("<Primary>Page_Down"), "Ctrl+Page Down");
        assert_eq!(m.label("<Control>space"), "⌃Space");
        assert_eq!(c.label("<Shift>F11"), "Shift+F11");
    }

    #[test]
    fn handlers_match_the_platforms_modifier_only() {
        let (c, m) = (Primary::Control, Primary::Command);
        let (ctrl, meta, shift) = (M::CONTROL_MASK, M::META_MASK, M::SHIFT_MASK);
        assert!(c.matches("<Primary>z", K::z, ctrl));
        assert!(!c.matches("<Primary>z", K::z, meta));
        assert!(m.matches("<Primary>z", K::z, meta));
        assert!(!m.matches("<Primary>z", K::z, ctrl), "⌃Z isn't ⌘Z");
        // Shifted, a letter may arrive upper case.
        assert!(c.matches("<Primary><Shift>z", K::Z, ctrl | shift));
        assert!(m.matches("<Primary><Shift>z", K::Z, meta | shift));
        assert!(
            !c.matches("<Primary>z", K::Z, ctrl | shift),
            "no more modifiers"
        );
        // Caps Lock and the mouse buttons don't count; on Linux, neither does Meta.
        assert!(c.matches("<Primary>z", K::Z, ctrl | M::LOCK_MASK | M::BUTTON1_MASK));
        assert!(c.matches("<Alt>Left", K::Left, M::ALT_MASK | meta));
        assert!(!m.matches("<Alt>Left", K::Left, M::ALT_MASK | meta));
        assert!(m.matches("<Control>space", K::space, ctrl));
        assert!(!c.matches("<Primary>nosuchkey", K::z, ctrl));
        assert!(Primary::Command.held(meta | shift));
        assert!(!Primary::Command.held(ctrl));
    }

    #[test]
    fn clashes_finds_twice_bound_keys_unknown_keys_and_macos_own() {
        let table = [
            ("undo", "<Primary>z"),
            ("redo", "<Primary><Shift>z"),
            ("redo", "<Shift><Primary>Z"),
        ];
        for p in Primary::ALL {
            let found = clashes(p, table);
            assert_eq!(found.len(), 1, "{found:?}");
            assert!(found[0].contains("redo (<Shift><Primary>Z)"), "{found:?}");
        }
        // Ctrl+Z and ⌘Z are different keys on a Mac, the same one on Linux.
        let both = [("undo", "<Primary>z"), ("other", "<Control>z")];
        assert_eq!(clashes(Primary::Command, both), Vec::<String>::new());
        assert_eq!(clashes(Primary::Control, both).len(), 1);
        let hide = [("replace", "<Primary>h"), ("bogus", "<Primary>nosuchkey")];
        assert_eq!(clashes(Primary::Control, hide).len(), 1, "unknown key");
        let mac = clashes(Primary::Command, hide);
        assert_eq!(mac.len(), 2, "{mac:?}");
        assert!(mac.iter().any(|p| p.contains("macOS's <Meta>h")));
        for spec in MAC_RESERVED {
            assert!(Primary::Command.parse(spec).is_some(), "{spec}");
        }
    }
}
