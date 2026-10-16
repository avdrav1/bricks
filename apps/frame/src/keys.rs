//! Every shortcut Frame binds, in one table ([`BINDINGS`]), written with `<Primary>` where
//! the platform's modifier goes (MAC-3, through `suite_ui::keys`): Ctrl on Linux, ⌘ on
//! macOS. The app takes them from it: GTK's accelerators ([`accels`]), the tooltips'
//! keys ([`label`]), and what the window's, the canvas's, and the outline's key handlers
//! do ([`find`]). Moving the caret and erasing in slide text on the canvas go by
//! [`caret`] and [`erase`]: as before on Linux, and as macOS's text has them there.

use gtk::gdk::{self, ModifierType};
use suite_ui::keys::Primary;

/// What a shortcut does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// File → New: a window with a new deck.
    New,
    /// File → Open.
    Open,
    /// The slideshow from the first slide.
    Slideshow,
    /// The slideshow from the current slide.
    SlideshowHere,
    Undo,
    Redo,
    /// New Slide after the current one.
    NewSlide,
    Save,
    SaveAs,
    /// The outline view, or the canvas again.
    Outline,
    /// A copy of the current slide after it.
    Duplicate,
    /// The current slide a place up the list.
    MoveUp,
    /// The current slide a place down the list.
    MoveDown,
    /// A picture from the clipboard onto the slide.
    Paste,
    /// On the canvas: the slide's next text placeholder, or a new slide after the last.
    NextPlaceholder,
    /// On the canvas: all the text being edited.
    SelectAll,
    /// In the outline: an empty slide after the caret's.
    OutlineSlide,
    /// macOS's File → Close Window.
    Close,
    /// macOS's Quit (the app menu's).
    Quit,
}

/// Who takes a shortcut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    /// A GTK accelerator of the action named, live in every window.
    Accel(&'static str),
    /// The window's key handler, before anything else sees the key.
    Window,
    /// The canvas's key handler (crate::editing).
    Canvas,
    /// The outline's key handler (crate::outline).
    Outline,
}

/// A shortcut: what it does, who takes it, and its key on Linux and on macOS (`None`: not
/// bound there).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub action: Action,
    pub via: Via,
    control: Option<&'static str>,
    command: Option<&'static str>,
    /// Shift held or not, as these keys always were on Linux.
    shift_too: bool,
}

impl Binding {
    /// Its key under `p`.
    pub fn spec(&self, p: Primary) -> Option<&'static str> {
        match p {
            Primary::Control => self.control,
            Primary::Command => self.command,
        }
    }
}

const fn both(action: Action, via: Via, spec: &'static str) -> Binding {
    Binding {
        action,
        via,
        control: Some(spec),
        command: Some(spec),
        shift_too: false,
    }
}

const fn shift_too(b: Binding) -> Binding {
    Binding {
        shift_too: true,
        ..b
    }
}

/// A key bound on Linux only.
const fn linux(action: Action, via: Via, spec: &'static str) -> Binding {
    Binding {
        command: None,
        ..both(action, via, spec)
    }
}

/// A key bound on macOS only.
const fn mac(action: Action, via: Via, spec: &'static str) -> Binding {
    Binding {
        control: None,
        ..both(action, via, spec)
    }
}

/// A key that's another on macOS.
const fn pick(action: Action, via: Via, control: &'static str, command: &'static str) -> Binding {
    Binding {
        command: Some(command),
        ..both(action, via, control)
    }
}

use Action as A;

/// Every shortcut Frame binds. An action's first binding is the one its tooltip shows.
pub const BINDINGS: &[Binding] = &[
    both(A::New, Via::Accel("app.new"), "<Primary>n"),
    both(A::Open, Via::Accel("app.open"), "<Primary>o"),
    both(A::Slideshow, Via::Accel("win.slideshow"), "F5"),
    both(
        A::SlideshowHere,
        Via::Accel("win.slideshow-here"),
        "<Shift>F5",
    ),
    both(A::Undo, Via::Window, "<Primary>z"),
    both(A::Redo, Via::Window, "<Primary><Shift>z"),
    // Ctrl+Y is Linux's other Redo, Shift or not; macOS's is ⇧⌘Z only.
    shift_too(linux(A::Redo, Via::Window, "<Primary>y")),
    // ⌘M is macOS's Minimize: New Slide is ⇧⌘N there, as in PowerPoint for Mac.
    pick(A::NewSlide, Via::Window, "<Primary>m", "<Primary><Shift>n"),
    both(A::Save, Via::Window, "<Primary>s"),
    both(A::SaveAs, Via::Window, "<Primary><Shift>s"),
    both(A::Outline, Via::Window, "<Primary><Shift>o"),
    both(A::Duplicate, Via::Window, "<Primary>d"),
    both(A::MoveUp, Via::Window, "<Primary>Up"),
    both(A::MoveDown, Via::Window, "<Primary>Down"),
    both(A::Paste, Via::Window, "<Primary>v"),
    shift_too(both(A::NextPlaceholder, Via::Canvas, "<Primary>Return")),
    shift_too(both(A::SelectAll, Via::Canvas, "<Primary>a")),
    both(A::OutlineSlide, Via::Outline, "<Primary>Return"),
    // The menu bar's Close Window and Quit (MAC-4); Linux has neither.
    mac(A::Close, Via::Accel("app.close"), "<Primary>w"),
    mac(A::Quit, Via::Accel("app.quit"), "<Primary>q"),
];

/// The actions' accelerators under `p`, each action's in the table's order.
pub fn accels_for(p: Primary) -> Vec<(&'static str, Vec<String>)> {
    let mut out: Vec<(&'static str, Vec<String>)> = Vec::new();
    for b in BINDINGS {
        let (Via::Accel(name), Some(spec)) = (b.via, b.spec(p)) else {
            continue;
        };
        let accel = p.accel(spec);
        match out.iter_mut().find(|(n, _)| *n == name) {
            Some((_, list)) => list.push(accel),
            None => out.push((name, vec![accel])),
        }
    }
    out
}

/// The actions' accelerators on this platform.
pub fn accels() -> Vec<(&'static str, Vec<String>)> {
    accels_for(Primary::HERE)
}

/// The key a tooltip shows for `action` under `p`: its first binding's (`Ctrl+M`, `⇧⌘N`).
pub fn label_for(p: Primary, action: Action) -> String {
    BINDINGS
        .iter()
        .filter(|b| b.action == action)
        .find_map(|b| b.spec(p))
        .map(|spec| p.label(spec))
        .unwrap_or_default()
}

/// The key a tooltip shows for `action` on this platform.
pub fn label(action: Action) -> String {
    label_for(Primary::HERE, action)
}

/// The accelerator a menu item for `action` shows under `p`: its first binding's.
pub fn accel_for(p: Primary, action: Action) -> Option<String> {
    BINDINGS
        .iter()
        .filter(|b| b.action == action)
        .find_map(|b| b.spec(p))
        .map(|spec| p.accel(spec))
}

/// The keypad's keys as the main ones: a binding names only those.
fn main_key(key: gdk::Key) -> gdk::Key {
    use gdk::Key;
    match key {
        Key::KP_Up => Key::Up,
        Key::KP_Down => Key::Down,
        Key::KP_Left => Key::Left,
        Key::KP_Right => Key::Right,
        Key::KP_Home => Key::Home,
        Key::KP_End => Key::End,
        Key::KP_Delete => Key::Delete,
        Key::KP_Enter | Key::ISO_Enter => Key::Return,
        k => k,
    }
}

/// The action `via` takes for a press of `key` with `mods` held, under `p`.
pub fn find(p: Primary, via: Via, key: gdk::Key, mods: ModifierType) -> Option<Action> {
    let key = main_key(key);
    BINDINGS
        .iter()
        .filter(|b| b.via == via)
        .find(|b| {
            let held = if b.shift_too {
                mods - ModifierType::SHIFT_MASK
            } else {
                mods
            };
            b.spec(p).is_some_and(|spec| p.matches(spec, key, held))
        })
        .map(|b| b.action)
}

/// Whether `mods` holds Ctrl, Alt, or the primary modifier: a key then isn't text, nor
/// the slide's own keys.
pub fn modified(p: Primary, mods: ModifierType) -> bool {
    p.held(mods) || mods.intersects(ModifierType::CONTROL_MASK | ModifierType::ALT_MASK)
}

/// Where a key moves the caret in slide text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Caret {
    /// A character left or right.
    Char { forward: bool },
    /// The start of the word before, or the end of the word after.
    Word { forward: bool },
    /// The line above or below.
    Vertical { up: bool },
    /// The line's start or end.
    Line { end: bool },
    /// The text box's start or end.
    Text { end: bool },
}

/// The caret move for `key` with `mods` held under `p` (Shift extends the selection; the
/// caller sees to that). Linux: the arrows, Home, and End, Ctrl for the text's start and
/// end. macOS: ⌥←/⌥→ a word, ⌘←/⌘→ the line's start and end, ⌘↑/⌘↓ (and ⌘Home/⌘End) the
/// text's.
pub fn caret(p: Primary, key: gdk::Key, mods: ModifierType) -> Option<Caret> {
    use gdk::Key;
    let mac = p == Primary::Command;
    let primary = p.held(mods);
    let alt = mods.contains(ModifierType::ALT_MASK);
    let key = main_key(key);
    Some(match key {
        Key::Left | Key::Right => {
            let forward = key == Key::Right;
            if mac && primary {
                Caret::Line { end: forward }
            } else if mac && alt {
                Caret::Word { forward }
            } else {
                Caret::Char { forward }
            }
        }
        Key::Up | Key::Down if mac && primary => Caret::Text {
            end: key == Key::Down,
        },
        Key::Up | Key::Down => Caret::Vertical { up: key == Key::Up },
        Key::Home | Key::End if primary => Caret::Text {
            end: key == Key::End,
        },
        Key::Home | Key::End => Caret::Line {
            end: key == Key::End,
        },
        _ => return None,
    })
}

/// What Backspace or Delete erases when nothing is selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Erase {
    /// The character before or after the caret.
    Char { forward: bool },
    /// To the start of the word before, or the end of the word after.
    Word { forward: bool },
    /// To the line's start.
    LineStart,
}

/// What `key` with `mods` held erases under `p`. Linux: a character, whatever's held.
/// macOS: ⌥⌫ and ⌥⌦ a word, ⌘⌫ to the line's start.
pub fn erase(p: Primary, key: gdk::Key, mods: ModifierType) -> Option<Erase> {
    use gdk::Key;
    let mac = p == Primary::Command;
    let alt = mods.contains(ModifierType::ALT_MASK);
    let forward = match main_key(key) {
        Key::BackSpace => false,
        Key::Delete => true,
        _ => return None,
    };
    Some(if mac && !forward && p.held(mods) {
        Erase::LineStart
    } else if mac && alt {
        Erase::Word { forward }
    } else {
        Erase::Char { forward }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdk::Key;
    use suite_ui::keys;

    const CTRL: ModifierType = ModifierType::CONTROL_MASK;
    const CMD: ModifierType = ModifierType::META_MASK;
    const SHIFT: ModifierType = ModifierType::SHIFT_MASK;
    const ALT: ModifierType = ModifierType::ALT_MASK;
    const NONE: ModifierType = ModifierType::empty();

    /// The (name, spec) pairs live together where `via` takes keys: its own and the
    /// window's, before it, and the accelerators. Shift-or-not keys count both ways.
    fn live(p: Primary, via: Via) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for b in BINDINGS {
            let here = b.via == via || b.via == Via::Window || matches!(b.via, Via::Accel(_));
            let Some(spec) = b.spec(p).filter(|_| here) else {
                continue;
            };
            let name = format!("{:?}", b.action);
            if b.shift_too {
                out.push((format!("{name} (Shift)"), format!("<Shift>{spec}")));
            }
            out.push((name, spec.to_owned()));
        }
        out
    }

    #[test]
    fn no_shortcut_clashes_on_either_platform() {
        for p in Primary::ALL {
            for via in [Via::Window, Via::Canvas, Via::Outline] {
                let table = live(p, via);
                let pairs = table.iter().map(|(n, s)| (n.as_str(), s.as_str()));
                assert_eq!(
                    keys::clashes(p, pairs),
                    Vec::<String>::new(),
                    "{p:?} {via:?}"
                );
            }
        }
    }

    #[test]
    fn every_control_key_is_command_on_macos() {
        for b in BINDINGS {
            if let Some(spec) = b.spec(Primary::Command) {
                let accel = Primary::Command.accel(spec);
                assert!(!accel.contains("<Control>"), "{:?}: {accel}", b.action);
            }
        }
        // The only keys not bound on macOS: Ctrl+Y (Redo is ⇧⌘Z there).
        let linux_only: Vec<_> = BINDINGS
            .iter()
            .filter(|b| b.spec(Primary::Command).is_none())
            .map(|b| (b.action, b.spec(Primary::Control)))
            .collect();
        assert_eq!(linux_only, [(Action::Redo, Some("<Primary>y"))]);
    }

    #[test]
    fn linux_keys_are_as_before() {
        let p = Primary::Control;
        assert_eq!(
            accels_for(p),
            [
                ("app.new", vec!["<Control>n".to_owned()]),
                ("app.open", vec!["<Control>o".to_owned()]),
                ("win.slideshow", vec!["F5".to_owned()]),
                ("win.slideshow-here", vec!["<Shift>F5".to_owned()]),
            ]
        );
        let accel = |action| {
            BINDINGS
                .iter()
                .filter(|b| b.action == action)
                .filter_map(|b| b.spec(p).map(|s| p.accel(s)))
                .collect::<Vec<_>>()
        };
        assert_eq!(accel(A::Undo), ["<Control>z"]);
        assert_eq!(accel(A::Redo), ["<Control><Shift>z", "<Control>y"]);
        assert_eq!(accel(A::NewSlide), ["<Control>m"]);
        assert_eq!(accel(A::Save), ["<Control>s"]);
        assert_eq!(accel(A::SaveAs), ["<Control><Shift>s"]);
        assert_eq!(accel(A::Outline), ["<Control><Shift>o"]);
        assert_eq!(accel(A::Duplicate), ["<Control>d"]);
        assert_eq!(accel(A::MoveUp), ["<Control>Up"]);
        assert_eq!(accel(A::MoveDown), ["<Control>Down"]);
        assert_eq!(accel(A::Paste), ["<Control>v"]);
        assert_eq!(accel(A::NextPlaceholder), ["<Control>Return"]);
        assert_eq!(accel(A::SelectAll), ["<Control>a"]);
        assert_eq!(accel(A::OutlineSlide), ["<Control>Return"]);
        // The tooltips' text, byte for byte.
        assert_eq!(format!("New ({})", label_for(p, A::New)), "New (Ctrl+N)");
        assert_eq!(
            format!("Open… ({})", label_for(p, A::Open)),
            "Open… (Ctrl+O)"
        );
        assert_eq!(format!("Save ({})", label_for(p, A::Save)), "Save (Ctrl+S)");
        assert_eq!(
            format!("New Slide ({})", label_for(p, A::NewSlide)),
            "New Slide (Ctrl+M)"
        );
        assert_eq!(
            format!("Outline ({})", label_for(p, A::Outline)),
            "Outline (Ctrl+Shift+O)"
        );
        assert_eq!(label_for(p, A::Slideshow), "F5");
    }

    #[test]
    fn macos_keys_and_labels() {
        let p = Primary::Command;
        assert_eq!(accels_for(p)[0], ("app.new", vec!["<Meta>n".to_owned()]));
        assert_eq!(label_for(p, A::NewSlide), "⇧⌘N");
        assert_eq!(label_for(p, A::Outline), "⇧⌘O");
        assert_eq!(label_for(p, A::Open), "⌘O");
        assert_eq!(find(p, Via::Window, Key::N, CMD | SHIFT), Some(A::NewSlide));
        assert_eq!(find(p, Via::Window, Key::m, CMD), None);
        assert_eq!(find(p, Via::Window, Key::y, CMD), None);
        assert_eq!(find(p, Via::Window, Key::Z, CMD | SHIFT), Some(A::Redo));
        // The menu bar's Close Window and Quit, macOS's alone.
        let accels = accels_for(p);
        assert!(accels.contains(&("app.close", vec!["<Meta>w".to_owned()])));
        assert!(accels.contains(&("app.quit", vec!["<Meta>q".to_owned()])));
        assert_eq!(accel_for(p, A::Close).as_deref(), Some("<Meta>w"));
        assert_eq!(accel_for(Primary::Control, A::Quit), None);
        // A menu item shows an action's first key.
        assert_eq!(accel_for(p, A::NewSlide).as_deref(), Some("<Meta><Shift>n"));
        assert_eq!(
            accel_for(Primary::Control, A::Redo).as_deref(),
            Some("<Control><Shift>z")
        );
    }

    #[test]
    fn handlers_match_the_platforms_modifier() {
        let (ctrl, cmd) = (Primary::Control, Primary::Command);
        // ⌘Z undoes under Command, ⌃Z doesn't; Ctrl+Z does under Control.
        assert_eq!(find(cmd, Via::Window, Key::z, CMD), Some(A::Undo));
        assert_eq!(find(cmd, Via::Window, Key::z, CTRL), None);
        assert_eq!(find(ctrl, Via::Window, Key::z, CTRL), Some(A::Undo));
        assert_eq!(find(ctrl, Via::Window, Key::z, CMD), None);
        // Shifted, z arrives as Z; Ctrl+Y redoes, Shift or not.
        assert_eq!(find(ctrl, Via::Window, Key::Z, CTRL | SHIFT), Some(A::Redo));
        assert_eq!(find(ctrl, Via::Window, Key::Y, CTRL | SHIFT), Some(A::Redo));
        assert_eq!(find(ctrl, Via::Window, Key::z, CTRL | ALT), None);
        assert_eq!(find(ctrl, Via::Window, Key::KP_Up, CTRL), Some(A::MoveUp));
        assert_eq!(find(ctrl, Via::Window, Key::Up, CTRL | SHIFT), None);
        // Ctrl+Enter, Shift or not, on the canvas; only without it in the outline.
        for p in Primary::ALL {
            let m = p.mask();
            assert_eq!(
                find(p, Via::Canvas, Key::KP_Enter, m | SHIFT),
                Some(A::NextPlaceholder)
            );
            assert_eq!(find(p, Via::Outline, Key::Return, m), Some(A::OutlineSlide));
            assert_eq!(find(p, Via::Outline, Key::Return, m | SHIFT), None);
            assert_eq!(find(p, Via::Canvas, Key::a, m), Some(A::SelectAll));
            assert!(modified(p, m) && modified(p, ALT) && !modified(p, SHIFT));
        }
        assert!(modified(cmd, CTRL));
    }

    #[test]
    fn caret_keys_on_linux_are_as_before() {
        let p = Primary::Control;
        for mods in [NONE, CTRL, ALT, SHIFT | CTRL] {
            assert_eq!(
                caret(p, Key::Left, mods),
                Some(Caret::Char { forward: false })
            );
            assert_eq!(
                caret(p, Key::KP_Right, mods),
                Some(Caret::Char { forward: true })
            );
            assert_eq!(caret(p, Key::Up, mods), Some(Caret::Vertical { up: true }));
            assert_eq!(
                erase(p, Key::BackSpace, mods),
                Some(Erase::Char { forward: false })
            );
            assert_eq!(
                erase(p, Key::KP_Delete, mods),
                Some(Erase::Char { forward: true })
            );
        }
        assert_eq!(caret(p, Key::Home, NONE), Some(Caret::Line { end: false }));
        assert_eq!(caret(p, Key::End, SHIFT), Some(Caret::Line { end: true }));
        assert_eq!(caret(p, Key::Home, CTRL), Some(Caret::Text { end: false }));
        assert_eq!(
            caret(p, Key::KP_End, CTRL | SHIFT),
            Some(Caret::Text { end: true })
        );
        assert_eq!(caret(p, Key::a, NONE), None);
    }

    #[test]
    fn caret_keys_on_macos() {
        let p = Primary::Command;
        assert_eq!(
            caret(p, Key::Left, ALT),
            Some(Caret::Word { forward: false })
        );
        assert_eq!(
            caret(p, Key::Right, ALT | SHIFT),
            Some(Caret::Word { forward: true })
        );
        assert_eq!(caret(p, Key::Left, CMD), Some(Caret::Line { end: false }));
        assert_eq!(
            caret(p, Key::Right, CMD | SHIFT),
            Some(Caret::Line { end: true })
        );
        assert_eq!(caret(p, Key::Up, CMD), Some(Caret::Text { end: false }));
        assert_eq!(
            caret(p, Key::Down, CMD | SHIFT),
            Some(Caret::Text { end: true })
        );
        assert_eq!(caret(p, Key::Up, NONE), Some(Caret::Vertical { up: true }));
        assert_eq!(
            caret(p, Key::Left, CTRL),
            Some(Caret::Char { forward: false })
        );
        assert_eq!(caret(p, Key::Home, CMD), Some(Caret::Text { end: false }));
        assert_eq!(caret(p, Key::End, NONE), Some(Caret::Line { end: true }));
        assert_eq!(
            erase(p, Key::BackSpace, ALT),
            Some(Erase::Word { forward: false })
        );
        assert_eq!(
            erase(p, Key::Delete, ALT),
            Some(Erase::Word { forward: true })
        );
        assert_eq!(erase(p, Key::BackSpace, CMD), Some(Erase::LineStart));
        assert_eq!(
            erase(p, Key::BackSpace, NONE),
            Some(Erase::Char { forward: false })
        );
        assert_eq!(
            erase(p, Key::Delete, CMD),
            Some(Erase::Char { forward: true })
        );
    }
}
