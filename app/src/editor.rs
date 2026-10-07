//! In-cell editing keys (EDIT-2), as in LibreOffice Calc: which keys start an edit, and
//! what keys do while one is open. Plain functions of the key, so the behavior is tested
//! without a display; `grid_view.rs` wires them to the entry drawn over the cell.

use grid::{Key, Mods};
use gtk::gdk::{self, Key as K, ModifierType};

/// How the edit started; decides what the arrow keys do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Typing on a cell replaced its content: arrows finish the edit and move.
    Typing,
    /// F2 or a double-click kept the content: arrows move the text cursor.
    Editing,
}

/// How a key press starts an edit on the cursor cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// F2: edit the current value, text cursor at the end.
    Keep,
    /// A printable character: the cell's content is replaced, starting with it.
    Replace(char),
}

/// What a key does while the editor is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Keep the text, then move the cell cursor as this navigation key would.
    Commit(Key, Mods),
    /// Drop the text; the cell keeps its value.
    Cancel,
    /// The entry handles it (text editing).
    Pass,
}

/// Whether `key` starts an edit when the grid has the focus. Navigation keys are checked
/// first by the caller; Ctrl, Alt and Super combinations never start one.
pub fn start(key: gdk::Key, state: ModifierType) -> Option<Start> {
    if key == K::F2 {
        return Some(Start::Keep);
    }
    let held = ModifierType::CONTROL_MASK | ModifierType::ALT_MASK | ModifierType::SUPER_MASK;
    if state.intersects(held) {
        return None;
    }
    key.to_unicode()
        .filter(|c| !c.is_control())
        .map(Start::Replace)
}

pub fn while_editing(key: gdk::Key, state: ModifierType, mode: Mode) -> Action {
    let shift = state.contains(ModifierType::SHIFT_MASK);
    let mods = Mods { shift, ctrl: false };
    let arrow = match key {
        K::Return | K::KP_Enter => return Action::Commit(Key::Enter, mods),
        K::Tab | K::KP_Tab => return Action::Commit(Key::Tab, mods),
        // Shift+Tab arrives as its own keysym.
        K::ISO_Left_Tab => {
            return Action::Commit(
                Key::Tab,
                Mods {
                    shift: true,
                    ..mods
                },
            )
        }
        K::Escape => return Action::Cancel,
        K::Up | K::KP_Up => Key::Up,
        K::Down | K::KP_Down => Key::Down,
        K::Left | K::KP_Left => Key::Left,
        K::Right | K::KP_Right => Key::Right,
        _ => return Action::Pass,
    };
    if mode == Mode::Typing && !shift {
        Action::Commit(arrow, Mods::default())
    } else {
        Action::Pass
    }
}

/// Row and column inserts and deletes from the keyboard (EDIT-3, EDIT-4), as in Calc:
/// whole selected columns get columns, anything else rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeCommand {
    /// Ctrl++: as many empty rows (columns) as the selection spans, above (left of) it.
    Insert,
    /// Ctrl+-: the rows (columns) the selection spans.
    Delete,
}

pub fn shape_command(key: gdk::Key, state: ModifierType) -> Option<ShapeCommand> {
    if !state.contains(ModifierType::CONTROL_MASK) {
        return None;
    }
    match key {
        K::plus | K::KP_Add => Some(ShapeCommand::Insert),
        K::minus | K::KP_Subtract => Some(ShapeCommand::Delete),
        _ => None,
    }
}

/// Delete (EDIT-5) empties the selected cells. Only on its own: Shift+Delete is cut and
/// Ctrl+Delete deletes a word in GTK, so modified presses aren't ours.
pub fn clears(key: gdk::Key, state: ModifierType) -> bool {
    let held = ModifierType::SHIFT_MASK
        | ModifierType::CONTROL_MASK
        | ModifierType::ALT_MASK
        | ModifierType::SUPER_MASK;
    matches!(key, K::Delete | K::KP_Delete) && !state.intersects(held)
}

/// Copy and cut from the keyboard (CLIP-1): Ctrl+C and Ctrl+X, and the older Ctrl+Insert
/// and Shift+Delete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipCommand {
    Copy,
    /// Copy, then empty the cells (as in Calc; undoable like Delete).
    Cut,
}

pub fn clip_command(key: gdk::Key, state: ModifierType) -> Option<ClipCommand> {
    let ctrl = state.contains(ModifierType::CONTROL_MASK);
    let shift = state.contains(ModifierType::SHIFT_MASK);
    if state.intersects(ModifierType::ALT_MASK | ModifierType::SUPER_MASK) {
        return None;
    }
    match (key, ctrl, shift) {
        (K::c | K::C | K::Insert | K::KP_Insert, true, false) => Some(ClipCommand::Copy),
        (K::x | K::X, true, false) | (K::Delete | K::KP_Delete, false, true) => {
            Some(ClipCommand::Cut)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: ModifierType = ModifierType::empty();
    const SHIFT: ModifierType = ModifierType::SHIFT_MASK;

    #[test]
    fn enter_commits_and_moves_down_esc_cancels() {
        for mode in [Mode::Typing, Mode::Editing] {
            assert_eq!(
                while_editing(K::Return, NONE, mode),
                Action::Commit(Key::Enter, Mods::default())
            );
            assert_eq!(
                while_editing(K::KP_Enter, NONE, mode),
                Action::Commit(Key::Enter, Mods::default())
            );
            assert_eq!(while_editing(K::Escape, NONE, mode), Action::Cancel);
        }
        // Shift+Enter commits and moves up; Tab and Shift+Tab move across.
        let shifted = Mods {
            shift: true,
            ctrl: false,
        };
        assert_eq!(
            while_editing(K::Return, SHIFT, Mode::Editing),
            Action::Commit(Key::Enter, shifted)
        );
        assert_eq!(
            while_editing(K::Tab, NONE, Mode::Editing),
            Action::Commit(Key::Tab, Mods::default())
        );
        assert_eq!(
            while_editing(K::ISO_Left_Tab, SHIFT, Mode::Typing),
            Action::Commit(Key::Tab, shifted)
        );
    }

    #[test]
    fn arrows_leave_a_typed_edit_but_move_the_text_cursor_in_f2() {
        assert_eq!(
            while_editing(K::Down, NONE, Mode::Typing),
            Action::Commit(Key::Down, Mods::default())
        );
        assert_eq!(
            while_editing(K::Left, NONE, Mode::Typing),
            Action::Commit(Key::Left, Mods::default())
        );
        assert_eq!(while_editing(K::Left, NONE, Mode::Editing), Action::Pass);
        assert_eq!(
            while_editing(K::Left, SHIFT, Mode::Typing),
            Action::Pass,
            "Shift+arrows select text"
        );
        assert_eq!(while_editing(K::a, NONE, Mode::Typing), Action::Pass);
    }

    #[test]
    fn f2_and_printable_keys_start_an_edit() {
        assert_eq!(start(K::F2, NONE), Some(Start::Keep));
        assert_eq!(start(K::a, NONE), Some(Start::Replace('a')));
        assert_eq!(start(K::A, SHIFT), Some(Start::Replace('A')));
        assert_eq!(start(K::_5, NONE), Some(Start::Replace('5')));
        assert_eq!(start(K::space, NONE), Some(Start::Replace(' ')));
        assert_eq!(start(K::eacute, NONE), Some(Start::Replace('é')));
        assert_eq!(
            start(K::s, ModifierType::CONTROL_MASK),
            None,
            "Ctrl+S saves"
        );
        assert_eq!(start(K::Escape, NONE), None);
        assert_eq!(start(K::Delete, NONE), None, "Delete clears instead");
        assert_eq!(start(K::Shift_L, SHIFT), None);
    }

    #[test]
    fn ctrl_plus_inserts_ctrl_minus_deletes() {
        let ctrl = ModifierType::CONTROL_MASK;
        // Ctrl++ is Ctrl+Shift+= on most layouts: the key arrives as `plus`.
        assert_eq!(
            shape_command(K::plus, ctrl | SHIFT),
            Some(ShapeCommand::Insert)
        );
        assert_eq!(shape_command(K::KP_Add, ctrl), Some(ShapeCommand::Insert));
        assert_eq!(shape_command(K::minus, ctrl), Some(ShapeCommand::Delete));
        assert_eq!(
            shape_command(K::KP_Subtract, ctrl),
            Some(ShapeCommand::Delete)
        );
        assert_eq!(
            shape_command(K::minus, NONE),
            None,
            "a plain minus is typing"
        );
    }

    #[test]
    fn delete_alone_clears() {
        assert!(clears(K::Delete, NONE));
        assert!(
            clears(K::KP_Delete, ModifierType::LOCK_MASK),
            "with Caps Lock on"
        );
        assert!(!clears(K::Delete, SHIFT), "Shift+Delete is cut");
        assert!(!clears(K::Delete, ModifierType::CONTROL_MASK));
        assert!(!clears(K::BackSpace, NONE));
    }

    #[test]
    fn ctrl_c_copies_ctrl_x_cuts() {
        let ctrl = ModifierType::CONTROL_MASK;
        assert_eq!(clip_command(K::c, ctrl), Some(ClipCommand::Copy));
        assert_eq!(
            clip_command(K::C, ctrl | ModifierType::LOCK_MASK),
            Some(ClipCommand::Copy),
            "with Caps Lock on"
        );
        assert_eq!(clip_command(K::Insert, ctrl), Some(ClipCommand::Copy));
        assert_eq!(clip_command(K::x, ctrl), Some(ClipCommand::Cut));
        assert_eq!(clip_command(K::Delete, SHIFT), Some(ClipCommand::Cut));
        assert_eq!(clip_command(K::c, NONE), None, "a plain c is typing");
        assert_eq!(clip_command(K::C, ctrl | SHIFT), None);
        assert_eq!(clip_command(K::Delete, NONE), None, "Delete clears");
    }
}
