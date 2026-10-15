//! Every shortcut Slate binds (MAC-3), in one table ([`BINDINGS`]), written with
//! `<Primary>` where the platform's modifier goes (`suite_ui::keys`): Ctrl on Linux, ⌘ on
//! macOS. The app takes them from it: GTK's accelerators ([`accels`]), the keys the menus
//! and tooltips show ([`accel`], [`tip`], [`label`]), the page's key handler
//! ([`page_key`]), and the find bar's ([`bar_key`]).
//!
//! On macOS the page moves the caret as Mac text does: ⌥←/⌥→ by words, ⌘←/⌘→ to the line's
//! start and end, ⌘↑/⌘↓ to the document's; ⌥⌫ and ⌥⌦ delete a word, ⌘⌫ to the line's
//! start. Redo is ⇧⌘Z there (Ctrl+Y only on Linux), and Replace ⌥⌘F, as ⌘H hides the app.
//!
//! The page's plain keys (the arrows, Home, End, Backspace, Delete, Enter, Tab, Page Up and
//! Down, Shift+Insert, Shift+Delete) are the page's own: they don't hang on the modifier.

use crate::view::Command;
use document_model::Align;
use gtk::gdk::{self, ModifierType};
use suite_ui::keys::Primary;

/// A caret move with a modifier; Shift held as well extends the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    WordLeft,
    WordRight,
    LineStart,
    LineEnd,
    DocumentStart,
    DocumentEnd,
}

/// A delete of more than a character; a selection goes instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Erase {
    /// The word before the caret.
    WordBack,
    /// The word after the caret.
    WordForward,
    /// From the start of the caret's line.
    LineStart,
}

/// What a key does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Does {
    /// Activate the action (`app.new`, `win.save`).
    Action(&'static str),
    /// The page's command, as the Edit and Format menus' items do it.
    Command(Command),
    Move(Move),
    Erase(Erase),
}

/// Who takes a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    /// GTK's accelerator, anywhere in a window. On macOS an Option+letter has to be one:
    /// Option changes the letter's keyval (⌥F is `ƒ`), and GTK matches by keycode too.
    Accel,
    /// The page's key handler, while the page has the keys.
    Page,
    /// The page's and the find bar's key handlers.
    PageAndBar,
}

/// A key, what it does, who takes it, and on which platform (`None`: both).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    /// A `suite_ui::keys` spec: `<Primary><Shift>s`.
    pub spec: &'static str,
    pub does: Does,
    pub via: Via,
    pub only: Option<Primary>,
}

const fn bind(spec: &'static str, does: Does, via: Via, only: Option<Primary>) -> Binding {
    Binding {
        spec,
        does,
        via,
        only,
    }
}

const fn app(spec: &'static str, action: &'static str) -> Binding {
    bind(spec, Does::Action(action), Via::Accel, None)
}

const fn page(spec: &'static str, command: Command) -> Binding {
    bind(spec, Does::Command(command), Via::Page, None)
}

const fn moves(spec: &'static str, to: Move, only: Option<Primary>) -> Binding {
    bind(spec, Does::Move(to), Via::Page, only)
}

const fn erase(spec: &'static str, what: Erase) -> Binding {
    bind(spec, Does::Erase(what), Via::Page, MAC)
}

const LINUX: Option<Primary> = Some(Primary::Control);
const MAC: Option<Primary> = Some(Primary::Command);

/// Every shortcut Slate binds. The first one a platform has for what it does is the one
/// its menu item or tooltip shows.
pub const BINDINGS: &[Binding] = &[
    app("<Primary>n", "app.new"),
    app("<Primary>o", "app.open"),
    app("<Primary>w", "app.close"),
    app("<Primary>q", "app.quit"),
    bind("<Primary>s", Does::Action("win.save"), Via::Page, None),
    bind(
        "<Primary><Shift>s",
        Does::Action("win.save-as"),
        Via::Page,
        None,
    ),
    bind(
        "<Primary>f",
        Does::Action("win.find"),
        Via::PageAndBar,
        None,
    ),
    bind(
        "<Primary>h",
        Does::Action("win.replace"),
        Via::PageAndBar,
        LINUX,
    ),
    // ⌘H hides the app on macOS; ⌥⌘F is Replace in Mac apps.
    bind(
        "<Alt><Primary>f",
        Does::Action("win.replace"),
        Via::Accel,
        MAC,
    ),
    page("<Primary>z", Command::Undo),
    page("<Primary><Shift>z", Command::Redo),
    bind("<Primary>y", Does::Command(Command::Redo), Via::Page, LINUX),
    page("<Primary>x", Command::Cut),
    page("<Primary>c", Command::Copy),
    page("<Primary>Insert", Command::Copy),
    page("<Primary>v", Command::Paste),
    page("<Primary>a", Command::SelectAll),
    page("<Primary>b", Command::Bold),
    page("<Primary>i", Command::Italic),
    page("<Primary>u", Command::Underline),
    page("<Primary>l", Command::Align(Align::Start)),
    page("<Primary>e", Command::Align(Align::Center)),
    page("<Primary>r", Command::Align(Align::End)),
    page("<Primary>j", Command::Align(Align::Justify)),
    page("<Primary>bracketright", Command::Size(true)),
    page("<Primary>bracketleft", Command::Size(false)),
    page("<Primary>0", Command::Style(0)),
    page("<Primary>1", Command::Style(1)),
    page("<Primary>2", Command::Style(2)),
    page("<Primary>3", Command::Style(3)),
    page("<Primary>4", Command::Style(4)),
    page("<Primary>5", Command::Style(5)),
    page("<Primary>6", Command::Style(6)),
    // Shift+8 is `*` and Shift+7 `&` on most layouts, but a key can come as either.
    page("<Primary><Shift>8", Command::Bullets),
    page("<Primary><Shift>asterisk", Command::Bullets),
    page("<Primary><Shift>7", Command::Numbers),
    page("<Primary><Shift>ampersand", Command::Numbers),
    moves("<Primary>Left", Move::WordLeft, LINUX),
    moves("<Primary>Right", Move::WordRight, LINUX),
    moves("<Alt>Left", Move::WordLeft, MAC),
    moves("<Alt>Right", Move::WordRight, MAC),
    moves("<Primary>Left", Move::LineStart, MAC),
    moves("<Primary>Right", Move::LineEnd, MAC),
    moves("<Primary>Up", Move::DocumentStart, MAC),
    moves("<Primary>Down", Move::DocumentEnd, MAC),
    moves("<Primary>Home", Move::DocumentStart, None),
    moves("<Primary>End", Move::DocumentEnd, None),
    erase("<Alt>BackSpace", Erase::WordBack),
    erase("<Alt>Delete", Erase::WordForward),
    erase("<Primary>BackSpace", Erase::LineStart),
];

/// The bindings `p`'s platform has.
pub fn bindings(p: Primary) -> impl Iterator<Item = &'static Binding> {
    BINDINGS
        .iter()
        .filter(move |b| b.only.is_none_or(|only| only == p))
}

/// The spec `p`'s platform shows for `does`: its first binding's.
pub fn shown(p: Primary, does: Does) -> Option<&'static str> {
    bindings(p).find(|b| b.does == does).map(|b| b.spec)
}

/// The accelerator a menu item for `does` shows on this platform.
pub fn accel(does: Does) -> Option<String> {
    shown(Primary::HERE, does).map(|spec| Primary::HERE.accel(spec))
}

/// The key that does `does` as `p`'s platform writes it: `Ctrl+B`, `⌘B`.
pub fn label_for(p: Primary, does: Does) -> String {
    shown(p, does).map_or_else(String::new, |spec| p.label(spec))
}

/// The key that does `does` as this platform writes it.
pub fn label(does: Does) -> String {
    label_for(Primary::HERE, does)
}

/// A tooltip: `text` and the key that does `does` on `p`'s platform, "Bold (Ctrl+B)".
pub fn tip_for(p: Primary, text: &str, does: Does) -> String {
    match shown(p, does) {
        Some(spec) => format!("{text} ({})", p.label(spec)),
        None => text.to_owned(),
    }
}

/// A tooltip: `text` and the key that does `does` on this platform.
pub fn tip(text: &str, does: Does) -> String {
    tip_for(Primary::HERE, text, does)
}

/// GTK's accelerators on `p`'s platform: each action's, in the table's order.
pub fn accels(p: Primary) -> Vec<(&'static str, Vec<String>)> {
    let mut out: Vec<(&'static str, Vec<String>)> = Vec::new();
    for b in bindings(p).filter(|b| b.via == Via::Accel) {
        let Does::Action(action) = b.does else {
            continue;
        };
        let accel = p.accel(b.spec);
        match out.iter_mut().find(|(a, _)| *a == action) {
            Some((_, list)) => list.push(accel),
            None => out.push((action, vec![accel])),
        }
    }
    out
}

/// The key the table names for a keypad key: the arrows, Home, End, Delete, and the digits
/// do what the main keys do.
fn main_key(key: gdk::Key) -> gdk::Key {
    use gdk::Key as K;
    match key {
        K::KP_Left => K::Left,
        K::KP_Right => K::Right,
        K::KP_Up => K::Up,
        K::KP_Down => K::Down,
        K::KP_Home => K::Home,
        K::KP_End => K::End,
        K::KP_Delete => K::Delete,
        K::KP_0 => K::_0,
        K::KP_1 => K::_1,
        K::KP_2 => K::_2,
        K::KP_3 => K::_3,
        K::KP_4 => K::_4,
        K::KP_5 => K::_5,
        K::KP_6 => K::_6,
        other => other,
    }
}

/// What the page does for `key` pressed with `state` on `p`'s platform, and whether Shift
/// extends the selection (for a move); `None` for a key that isn't a shortcut. A move or a
/// delete matches with Shift held too.
pub fn page_key(p: Primary, key: gdk::Key, state: ModifierType) -> Option<(Does, bool)> {
    let key = main_key(key);
    let shift = state.contains(ModifierType::SHIFT_MASK);
    let unshifted = state & !ModifierType::SHIFT_MASK;
    bindings(p)
        .filter(|b| b.via != Via::Accel)
        .find_map(|b| match b.does {
            Does::Move(_) | Does::Erase(_) => {
                p.matches(b.spec, key, unshifted).then_some((b.does, shift))
            }
            _ => p.matches(b.spec, key, state).then_some((b.does, false)),
        })
}

/// The action the find bar runs for `key` pressed with `state` on `p`'s platform.
pub fn bar_key(p: Primary, key: gdk::Key, state: ModifierType) -> Option<&'static str> {
    bindings(p)
        .filter(|b| b.via == Via::PageAndBar)
        .find_map(|b| match b.does {
            Does::Action(action) if p.matches(b.spec, key, state) => Some(action),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdk::Key as K;
    use suite_ui::keys;

    const CTRL: ModifierType = ModifierType::CONTROL_MASK;
    const SHIFT: ModifierType = ModifierType::SHIFT_MASK;
    const ALT: ModifierType = ModifierType::ALT_MASK;
    const CMD: ModifierType = ModifierType::META_MASK;

    #[test]
    fn no_key_is_bound_twice_or_taken_by_macos() {
        for p in Primary::ALL {
            let table: Vec<(String, &str)> = bindings(p)
                .map(|b| (format!("{:?}", b.does), b.spec))
                .collect();
            let problems = keys::clashes(p, table.iter().map(|(n, s)| (n.as_str(), *s)));
            assert!(problems.is_empty(), "{p:?}: {problems:?}");
            // A move or a delete with Shift held is the same move: it mustn't be another
            // binding's key either.
            let shifted: Vec<(String, String)> = bindings(p)
                .filter(|b| matches!(b.does, Does::Move(_) | Does::Erase(_)))
                .map(|b| (format!("Shift {:?}", b.does), format!("<Shift>{}", b.spec)))
                .collect();
            let all = table
                .iter()
                .map(|(n, s)| (n.as_str(), *s))
                .chain(shifted.iter().map(|(n, s)| (n.as_str(), s.as_str())));
            let problems = keys::clashes(p, all);
            assert!(problems.is_empty(), "{p:?} with Shift: {problems:?}");
        }
    }

    #[test]
    fn linux_keys_are_the_ones_slate_had() {
        let every: Vec<String> = bindings(Primary::Control)
            .map(|b| Primary::Control.accel(b.spec))
            .collect();
        assert_eq!(
            every,
            [
                "<Control>n",
                "<Control>o",
                "<Control>w",
                "<Control>q",
                "<Control>s",
                "<Control><Shift>s",
                "<Control>f",
                "<Control>h",
                "<Control>z",
                "<Control><Shift>z",
                "<Control>y",
                "<Control>x",
                "<Control>c",
                "<Control>Insert",
                "<Control>v",
                "<Control>a",
                "<Control>b",
                "<Control>i",
                "<Control>u",
                "<Control>l",
                "<Control>e",
                "<Control>r",
                "<Control>j",
                "<Control>bracketright",
                "<Control>bracketleft",
                "<Control>0",
                "<Control>1",
                "<Control>2",
                "<Control>3",
                "<Control>4",
                "<Control>5",
                "<Control>6",
                "<Control><Shift>8",
                "<Control><Shift>asterisk",
                "<Control><Shift>7",
                "<Control><Shift>ampersand",
                "<Control>Left",
                "<Control>Right",
                "<Control>Home",
                "<Control>End",
            ]
        );
        assert_eq!(
            accels(Primary::Control),
            [
                ("app.new", vec!["<Control>n".to_owned()]),
                ("app.open", vec!["<Control>o".to_owned()]),
                ("app.close", vec!["<Control>w".to_owned()]),
                ("app.quit", vec!["<Control>q".to_owned()]),
            ]
        );
        // The menus' keys and the tooltips, as they were.
        let menu = |does| shown(Primary::Control, does).map(|s| Primary::Control.accel(s));
        assert_eq!(
            menu(Does::Action("win.replace")).as_deref(),
            Some("<Control>h")
        );
        assert_eq!(
            menu(Does::Command(Command::Redo)).as_deref(),
            Some("<Control><Shift>z")
        );
        let tip = |text, does| tip_for(Primary::Control, text, does);
        assert_eq!(tip("Open…", Does::Action("app.open")), "Open… (Ctrl+O)");
        assert_eq!(tip("Bold", Does::Command(Command::Bold)), "Bold (Ctrl+B)");
        assert_eq!(
            tip("Align left", Does::Command(Command::Align(Align::Start))),
            "Align left (Ctrl+L)"
        );
        let label = |does| label_for(Primary::Control, does);
        assert_eq!(
            format!(
                "Font size ({} and {})",
                label(Does::Command(Command::Size(true))),
                label(Does::Command(Command::Size(false)))
            ),
            "Font size (Ctrl+] and Ctrl+[)"
        );
        assert_eq!(
            format!(
                "Paragraph style ({} to {})",
                label(Does::Command(Command::Style(0))),
                label(Does::Command(Command::Style(6)))
            ),
            "Paragraph style (Ctrl+0 to Ctrl+6)"
        );
    }

    #[test]
    fn macos_keys_use_command() {
        let control: Vec<&str> = bindings(Primary::Command)
            .map(|b| b.spec)
            .filter(|spec| Primary::Command.accel(spec).contains("<Control>"))
            .collect();
        assert_eq!(control, Vec::<&str>::new(), "no key stays on Control");
        assert_eq!(
            accels(Primary::Command),
            [
                ("app.new", vec!["<Meta>n".to_owned()]),
                ("app.open", vec!["<Meta>o".to_owned()]),
                ("app.close", vec!["<Meta>w".to_owned()]),
                ("app.quit", vec!["<Meta>q".to_owned()]),
                ("win.replace", vec!["<Alt><Meta>f".to_owned()]),
            ]
        );
        let mac = |does| label_for(Primary::Command, does);
        assert_eq!(mac(Does::Command(Command::Redo)), "⇧⌘Z");
        assert_eq!(mac(Does::Action("win.replace")), "⌥⌘F");
        assert_eq!(mac(Does::Action("win.save-as")), "⇧⌘S");
        assert_eq!(
            tip_for(Primary::Command, "Bold", Does::Command(Command::Bold)),
            "Bold (⌘B)"
        );
    }

    #[test]
    fn the_page_takes_the_platforms_modifier() {
        let undo = Some((Does::Command(Command::Undo), false));
        let redo = Some((Does::Command(Command::Redo), false));
        assert_eq!(page_key(Primary::Command, K::z, CMD), undo);
        assert_eq!(page_key(Primary::Command, K::z, CTRL), None);
        assert_eq!(page_key(Primary::Control, K::z, CTRL), undo);
        assert_eq!(page_key(Primary::Control, K::z, CMD), None);
        assert_eq!(page_key(Primary::Command, K::Z, CMD | SHIFT), redo);
        assert_eq!(page_key(Primary::Control, K::Z, CTRL | SHIFT), redo);
        assert_eq!(page_key(Primary::Control, K::y, CTRL), redo);
        assert_eq!(page_key(Primary::Command, K::y, CMD), None);
        assert_eq!(
            page_key(Primary::Control, K::h, CTRL),
            Some((Does::Action("win.replace"), false))
        );
        // ⌘H hides the app; Replace is GTK's accelerator, ⌥⌘F.
        assert_eq!(page_key(Primary::Command, K::h, CMD), None);
        assert_eq!(
            page_key(Primary::Control, K::KP_3, CTRL),
            Some((Does::Command(Command::Style(3)), false))
        );
        assert_eq!(
            page_key(Primary::Control, K::asterisk, CTRL | SHIFT),
            Some((Does::Command(Command::Bullets), false))
        );
        assert_eq!(page_key(Primary::Control, K::_3, CTRL | SHIFT), None);
        assert_eq!(
            page_key(Primary::Control, K::s, ModifierType::empty()),
            None
        );
    }

    #[test]
    fn caret_moves_follow_the_platform() {
        let to = |m, extend| Some((Does::Move(m), extend));
        let wipe = |e| Some((Does::Erase(e), false));
        // Linux: Ctrl+arrows by words, Ctrl+Home and Ctrl+End; Ctrl+Up and Ctrl+Backspace
        // are the plain keys' (`None` here), as before.
        let linux = |key, state| page_key(Primary::Control, key, state);
        assert_eq!(linux(K::Left, CTRL), to(Move::WordLeft, false));
        assert_eq!(linux(K::KP_Right, CTRL | SHIFT), to(Move::WordRight, true));
        assert_eq!(linux(K::Home, CTRL), to(Move::DocumentStart, false));
        assert_eq!(linux(K::End, CTRL | SHIFT), to(Move::DocumentEnd, true));
        assert_eq!(linux(K::Left, ALT), None);
        assert_eq!(linux(K::Up, CTRL), None);
        assert_eq!(linux(K::BackSpace, CTRL), None);
        assert_eq!(linux(K::BackSpace, ALT), None);
        // macOS: ⌥ by words, ⌘ to the line's and the document's ends, ⌥⌫ ⌥⌦ ⌘⌫.
        let mac = |key, state| page_key(Primary::Command, key, state);
        assert_eq!(mac(K::Left, ALT), to(Move::WordLeft, false));
        assert_eq!(mac(K::Right, ALT | SHIFT), to(Move::WordRight, true));
        assert_eq!(mac(K::Left, CMD), to(Move::LineStart, false));
        assert_eq!(mac(K::Right, CMD | SHIFT), to(Move::LineEnd, true));
        assert_eq!(mac(K::Up, CMD), to(Move::DocumentStart, false));
        assert_eq!(mac(K::Down, CMD | SHIFT), to(Move::DocumentEnd, true));
        assert_eq!(mac(K::End, CMD), to(Move::DocumentEnd, false));
        assert_eq!(mac(K::Left, CTRL), None);
        assert_eq!(mac(K::BackSpace, ALT), wipe(Erase::WordBack));
        assert_eq!(mac(K::Delete, ALT), wipe(Erase::WordForward));
        assert_eq!(mac(K::BackSpace, CMD), wipe(Erase::LineStart));
    }

    #[test]
    fn the_find_bar_takes_find_and_replace() {
        assert_eq!(bar_key(Primary::Control, K::f, CTRL), Some("win.find"));
        assert_eq!(bar_key(Primary::Control, K::H, CTRL), Some("win.replace"));
        assert_eq!(bar_key(Primary::Command, K::f, CMD), Some("win.find"));
        assert_eq!(bar_key(Primary::Command, K::f, CTRL), None);
        assert_eq!(bar_key(Primary::Command, K::h, CMD), None);
        assert_eq!(bar_key(Primary::Control, K::s, CTRL), None);
    }
}
