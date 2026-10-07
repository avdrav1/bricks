//! Keyboard navigation and the cell selection (GRID-3), matching LibreOffice Calc's behavior
//! with default settings (docs/navigation-checklist.md, replayed by `tests/lo_parity.rs`).
//!
//! The selection is a rectangle between an anchor and an extent, plus the cursor (the active
//! cell). Plain movement collapses all three onto one cell. Shift+movement moves only the
//! extent: the cursor stays where the selection started, as in Calc. With a range selected,
//! Tab/Enter move the cursor inside the range and wrap, leaving the range alone.

/// A cell by row and column (0-based).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cell {
    pub row: u64,
    pub col: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Tab,
    Enter,
    Home,
    End,
    PageUp,
    PageDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
}

/// What the keys move within: the table's size and how many rows one screen holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub rows: u64,
    pub cols: u32,
    pub page_rows: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Selection {
    cursor: Cell,
    anchor: Cell,
    extent: Cell,
    /// Column where a run of Tab presses began; Enter returns there.
    tab_start: Option<u32>,
}

impl Selection {
    /// A single selected cell.
    pub fn at(cell: Cell) -> Self {
        Self {
            cursor: cell,
            anchor: cell,
            extent: cell,
            tab_start: None,
        }
    }

    /// The active cell.
    pub fn cursor(&self) -> Cell {
        self.cursor
    }

    /// The selected rectangle as (top-left, bottom-right).
    pub fn range(&self) -> (Cell, Cell) {
        let (a, e) = (self.anchor, self.extent);
        (
            Cell {
                row: a.row.min(e.row),
                col: a.col.min(e.col),
            },
            Cell {
                row: a.row.max(e.row),
                col: a.col.max(e.col),
            },
        )
    }

    /// The cell the last key moved: the extent while extending, else the cursor. The view
    /// scrolls to keep it visible.
    pub fn moving_end(&self) -> Cell {
        if self.is_range() {
            self.extent
        } else {
            self.cursor
        }
    }

    fn is_range(&self) -> bool {
        self.anchor != self.extent
    }

    /// Apply one key press.
    pub fn press(&mut self, key: Key, mods: Mods, b: Bounds) {
        if b.rows == 0 || b.cols == 0 {
            return;
        }
        let clamp = |c: Cell| Cell {
            row: c.row.min(b.rows - 1),
            col: c.col.min(b.cols - 1),
        };
        match key {
            Key::Tab | Key::Enter if self.is_range() => self.cycle_in_range(key, mods.shift),
            Key::Tab => {
                // A run of Tabs remembers where it began, for the Enter that ends it.
                let start = self.tab_start.unwrap_or(self.cursor.col);
                let col = if mods.shift {
                    self.cursor.col.saturating_sub(1)
                } else {
                    self.cursor.col + 1
                };
                self.collapse(clamp(Cell { col, ..self.cursor }));
                self.tab_start = Some(start);
            }
            Key::Enter => {
                let col = self.tab_start.take().unwrap_or(self.cursor.col);
                let row = if mods.shift {
                    self.cursor.row.saturating_sub(1)
                } else {
                    self.cursor.row + 1
                };
                self.collapse(clamp(Cell { row, col }));
            }
            _ => {
                let from = if mods.shift { self.extent } else { self.cursor };
                let to = clamp(Self::target(key, mods.ctrl, from, b));
                if mods.shift {
                    self.extent = to;
                    self.tab_start = None;
                } else {
                    self.collapse(to);
                }
            }
        }
    }

    /// Where a movement key goes from `from`.
    fn target(key: Key, ctrl: bool, from: Cell, b: Bounds) -> Cell {
        let Cell { row, col } = from;
        match (key, ctrl) {
            (Key::Home, true) => Cell { row: 0, col: 0 },
            (Key::End, true) => Cell {
                row: b.rows - 1,
                col: b.cols - 1,
            },
            (Key::Home, false) => Cell { row, col: 0 },
            (Key::End, false) => Cell {
                row,
                col: b.cols - 1,
            },
            (Key::Up, _) => Cell {
                row: row.saturating_sub(1),
                col,
            },
            (Key::Down, _) => Cell { row: row + 1, col },
            (Key::Left, _) => Cell {
                row,
                col: col.saturating_sub(1),
            },
            (Key::Right, _) => Cell { row, col: col + 1 },
            (Key::PageUp, _) => Cell {
                row: row.saturating_sub(b.page_rows),
                col,
            },
            (Key::PageDown, _) => Cell {
                row: row.saturating_add(b.page_rows),
                col,
            },
            (Key::Tab | Key::Enter, _) => from,
        }
    }

    fn collapse(&mut self, to: Cell) {
        *self = Self::at(to);
    }

    /// Tab/Enter with a range selected: move the cursor through the range, row by row (Tab)
    /// or column by column (Enter), wrapping at the edges; Shift goes backwards.
    fn cycle_in_range(&mut self, key: Key, back: bool) {
        let (tl, br) = self.range();
        let Cell { mut row, mut col } = self.cursor;
        let (row_major, forward) = (key == Key::Tab, !back);
        // (inner, outer) axis positions and bounds.
        let (mut inner, mut outer) = if row_major {
            (u64::from(col), row)
        } else {
            (row, u64::from(col))
        };
        let (inner_lo, inner_hi, outer_lo, outer_hi) = if row_major {
            (u64::from(tl.col), u64::from(br.col), tl.row, br.row)
        } else {
            (tl.row, br.row, u64::from(tl.col), u64::from(br.col))
        };
        if forward {
            if inner < inner_hi {
                inner += 1;
            } else {
                inner = inner_lo;
                outer = if outer < outer_hi {
                    outer + 1
                } else {
                    outer_lo
                };
            }
        } else if inner > inner_lo {
            inner -= 1;
        } else {
            inner = inner_hi;
            outer = if outer > outer_lo {
                outer - 1
            } else {
                outer_hi
            };
        }
        if row_major {
            (col, row) = (inner as u32, outer);
        } else {
            (row, col) = (inner, outer as u32);
        }
        self.cursor = Cell { row, col };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const B: Bounds = Bounds {
        rows: 10,
        cols: 4,
        page_rows: 3,
    };

    fn press(sel: &mut Selection, key: Key, shift: bool) {
        sel.press(key, Mods { shift, ctrl: false }, B);
    }

    #[test]
    fn stays_inside_the_table_at_the_far_edges() {
        // Past the last row/column Calc has empty cells; this grid has none, so it stops.
        let mut s = Selection::at(Cell { row: 9, col: 3 });
        press(&mut s, Key::Down, false);
        press(&mut s, Key::Right, false);
        press(&mut s, Key::Tab, false);
        press(&mut s, Key::Enter, false);
        press(&mut s, Key::PageDown, false);
        assert_eq!(s.cursor(), Cell { row: 9, col: 3 });
    }

    #[test]
    fn empty_table_ignores_keys() {
        let mut s = Selection::default();
        s.press(
            Key::Down,
            Mods::default(),
            Bounds {
                rows: 0,
                cols: 0,
                page_rows: 10,
            },
        );
        assert_eq!(s.cursor(), Cell::default());
    }

    #[test]
    fn moving_end_follows_the_extent_while_extending() {
        let mut s = Selection::at(Cell { row: 2, col: 1 });
        press(&mut s, Key::PageDown, true);
        assert_eq!(s.moving_end(), Cell { row: 5, col: 1 });
        assert_eq!(s.cursor(), Cell { row: 2, col: 1 });
    }
}
