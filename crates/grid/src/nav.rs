//! Keyboard navigation and the cell selection (GRID-3, GRID-4), matching LibreOffice Calc's
//! behavior with default settings (docs/navigation-checklist.md, replayed by
//! `tests/lo_parity.rs`).
//!
//! The selection is a rectangle between an anchor and an extent, plus the cursor (the active
//! cell). Plain movement and a click collapse all three onto one cell. Shift+movement,
//! Shift+click and dragging move only the extent: the cursor stays where the selection
//! started, as in Calc. With a range selected, Tab/Enter move the cursor inside the range and
//! wrap, leaving the range alone. Whole rows and columns run to the table's last column and
//! row, and keep doing so as indexing finds more ([`Selection::grow`]).

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
    /// With Shift: whole rows. With Ctrl: whole columns. With both: everything.
    Space,
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
    /// The range spans every column (whole rows), so it widens when the table does.
    full_width: bool,
    /// The range spans every row (whole columns), so it lengthens as indexing goes on.
    full_height: bool,
}

impl Selection {
    /// A single selected cell.
    pub fn at(cell: Cell) -> Self {
        Self {
            cursor: cell,
            anchor: cell,
            extent: cell,
            tab_start: None,
            full_width: false,
            full_height: false,
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

    /// The row and column the view should keep visible after a key: the extent while
    /// extending, else the cursor. An axis the selection spans entirely (whole rows or
    /// columns) gives `None`: the view stays put along it, as in Calc.
    pub fn reveal(&self) -> (Option<u64>, Option<u32>) {
        let end = if self.is_range() {
            self.extent
        } else {
            self.cursor
        };
        (
            (!self.full_height).then_some(end.row),
            (!self.full_width).then_some(end.col),
        )
    }

    /// The selected columns when the selection is whole columns (every row of them).
    pub fn whole_columns(&self) -> Option<std::ops::Range<u32>> {
        let (tl, br) = self.range();
        self.full_height.then(|| tl.col..br.col + 1)
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
            Key::Space => match (mods.shift, mods.ctrl) {
                (true, true) => self.select_all(b),
                (true, false) => self.whole_rows(b),
                (false, true) => self.whole_cols(b),
                (false, false) => {}
            },
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
            (Key::Tab | Key::Enter | Key::Space, _) => from,
        }
    }

    fn collapse(&mut self, to: Cell) {
        *self = Self::at(to);
    }

    /// Extend the current range to whole rows: every column of the rows it covers.
    fn whole_rows(&mut self, b: Bounds) {
        self.anchor.col = 0;
        self.extent.col = b.cols - 1;
        (self.full_width, self.tab_start) = (true, None);
    }

    /// Extend the current range to whole columns: every row of the columns it covers.
    fn whole_cols(&mut self, b: Bounds) {
        self.anchor.row = 0;
        self.extent.row = b.rows - 1;
        (self.full_height, self.tab_start) = (true, None);
    }

    /// Select the whole table; the cursor stays.
    pub fn select_all(&mut self, b: Bounds) {
        if b.rows == 0 || b.cols == 0 {
            return;
        }
        self.anchor = Cell::default();
        self.whole_rows(b);
        self.whole_cols(b);
    }

    /// Move the far corner to `cell`, keeping the cursor: Shift+click and dragging.
    pub fn extend_to(&mut self, cell: Cell) {
        self.extent = cell;
        (self.full_width, self.full_height, self.tab_start) = (false, false, None);
    }

    /// A click on a row header: select that row, with the cursor in it at `cursor_col`.
    /// `extend` (Shift+click, dragging) instead stretches the selection to whole rows
    /// through `row`, keeping the cursor.
    pub fn click_row(&mut self, row: u64, extend: bool, cursor_col: u32, b: Bounds) {
        if b.rows == 0 || b.cols == 0 {
            return;
        }
        let row = row.min(b.rows - 1);
        if !extend {
            *self = Self::at(Cell {
                row,
                col: cursor_col.min(b.cols - 1),
            });
        }
        self.extent.row = row;
        self.full_height = false;
        self.whole_rows(b);
    }

    /// A click on a column header; see [`Selection::click_row`].
    pub fn click_col(&mut self, col: u32, extend: bool, cursor_row: u64, b: Bounds) {
        if b.rows == 0 || b.cols == 0 {
            return;
        }
        let col = col.min(b.cols - 1);
        if !extend {
            *self = Self::at(Cell {
                row: cursor_row.min(b.rows - 1),
                col,
            });
        }
        self.extent.col = col;
        self.full_width = false;
        self.whole_cols(b);
    }

    /// The table grew from `old` to `new` (indexing found more rows, or a wider row): whole
    /// rows and columns that still reach the old edge move to the new one.
    pub fn grow(&mut self, old: Bounds, new: Bounds) {
        let (tl, br) = self.range();
        if self.full_height && tl.row == 0 && old.rows > 0 && br.row == old.rows - 1 {
            let far = if self.anchor.row > self.extent.row {
                &mut self.anchor.row
            } else {
                &mut self.extent.row
            };
            *far = new.rows.max(1) - 1;
        }
        if self.full_width && tl.col == 0 && old.cols > 0 && br.col == old.cols - 1 {
            let far = if self.anchor.col > self.extent.col {
                &mut self.anchor.col
            } else {
                &mut self.extent.col
            };
            *far = new.cols.max(1) - 1;
        }
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
    fn view_follows_the_extent_but_not_along_whole_rows_or_columns() {
        let mut s = Selection::at(Cell { row: 2, col: 1 });
        press(&mut s, Key::PageDown, true);
        assert_eq!(s.reveal(), (Some(5), Some(1)));
        assert_eq!(s.cursor(), Cell { row: 2, col: 1 });
        // A whole column runs to the last row; the view must not jump there.
        let ctrl = Mods {
            shift: false,
            ctrl: true,
        };
        s.press(Key::Space, ctrl, B);
        assert_eq!(s.reveal(), (None, Some(1)));
        press(&mut s, Key::Right, true);
        assert_eq!(s.reveal(), (None, Some(2)));
        // Shift+Ctrl+Space: everything, the view stays put on both axes.
        s.press(
            Key::Space,
            Mods {
                shift: true,
                ..ctrl
            },
            B,
        );
        assert_eq!(s.reveal(), (None, None));
    }

    #[test]
    fn whole_columns_keep_up_with_indexing() {
        // A column selected while the file is still indexing covers rows found later.
        let mut s = Selection::at(Cell { row: 2, col: 1 });
        s.press(
            Key::Space,
            Mods {
                shift: false,
                ctrl: true,
            },
            B,
        );
        let more = Bounds { rows: 500, ..B };
        s.grow(B, more);
        assert_eq!(
            s.range(),
            (Cell { row: 0, col: 1 }, Cell { row: 499, col: 1 })
        );
        // A range that merely touches the edge stays put.
        let mut t = Selection::at(Cell { row: 5, col: 0 });
        press(&mut t, Key::PageDown, true);
        press(&mut t, Key::PageDown, true);
        t.grow(B, more);
        assert_eq!(t.range().1, Cell { row: 9, col: 0 });
        // Shrinking the column selection off the edge stops it following.
        s.press(
            Key::Up,
            Mods {
                shift: true,
                ctrl: false,
            },
            more,
        );
        s.grow(more, Bounds { rows: 900, ..B });
        assert_eq!(s.range().1.row, 498);
    }

    #[test]
    fn header_clicks_select_whole_rows_and_columns() {
        let mut s = Selection::default();
        s.click_row(4, false, 2, B);
        assert_eq!(s.cursor(), Cell { row: 4, col: 2 });
        s.click_row(7, true, 0, B); // Shift+click or drag: rows 5-8, cursor stays
        assert_eq!(
            s.range(),
            (Cell { row: 4, col: 0 }, Cell { row: 7, col: 3 })
        );
        assert_eq!(s.cursor(), Cell { row: 4, col: 2 });
        s.click_col(3, false, 6, B);
        s.click_col(1, true, 0, B);
        assert_eq!(
            s.range(),
            (Cell { row: 0, col: 1 }, Cell { row: 9, col: 3 })
        );
        assert_eq!(s.cursor(), Cell { row: 6, col: 3 });
        // Out-of-range header positions clamp to the table.
        s.click_row(99, false, 99, B);
        assert_eq!(s.cursor(), Cell { row: 9, col: 3 });
    }

    #[test]
    fn shift_click_and_drag_keep_the_cursor() {
        let mut s = Selection::at(Cell { row: 1, col: 1 });
        s.extend_to(Cell { row: 5, col: 0 });
        assert_eq!(
            s.range(),
            (Cell { row: 1, col: 0 }, Cell { row: 5, col: 1 })
        );
        assert_eq!(s.cursor(), Cell { row: 1, col: 1 });
        // Tab then walks the dragged range, as after Shift+arrows.
        press(&mut s, Key::Tab, false);
        assert_eq!(s.cursor(), Cell { row: 2, col: 0 });
    }
}
