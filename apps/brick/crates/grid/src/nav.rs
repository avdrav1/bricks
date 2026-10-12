//! Keyboard navigation and the cell selection (GRID-3, GRID-4), matching LibreOffice Calc's
//! behavior with default settings (apps/brick/docs/navigation-checklist.md, replayed by
//! `tests/lo_parity.rs`).
//!
//! The selection is a rectangle between an anchor and an extent, plus the cursor (the active
//! cell). Plain movement and a click collapse all three onto one cell. Shift+movement,
//! Shift+click and dragging move only the extent: the cursor stays where the selection
//! started, as in Calc. With a range selected, Tab/Enter move the cursor inside the range and
//! wrap, leaving the range alone. Whole rows and columns run to the table's last column and
//! row, and keep doing so as indexing finds more ([`Selection::grow`]).
//!
//! Merged cells (STYLE-4) act as one cell: the cursor on a merge sits on its top-left
//! cell (the anchor), keys step out of it from its far edge, and a range that touches a
//! merge grows to cover it whole, as in Calc and Excel.

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

/// A merged area: its top-left and bottom-right cells, inclusive.
pub type Area = (Cell, Cell);

/// The rows and columns to keep in view, each as (first, last) inclusive; `None` leaves
/// that axis where it is.
pub type Reveal = (Option<(u64, u64)>, Option<(u32, u32)>);

/// The merged cells navigation treats as one (STYLE-4), in grid rows and columns.
pub trait Merges {
    /// The merged area covering `cell`, if any.
    fn at(&self, cell: Cell) -> Option<Area>;
    /// Call `f` with each merged area that intersects the rectangle `tl`..=`br`.
    fn each_in(&self, tl: Cell, br: Cell, f: &mut dyn FnMut(Area));
}

/// No merged cells: a CSV sheet.
pub struct NoMerges;

impl Merges for NoMerges {
    fn at(&self, _: Cell) -> Option<Area> {
        None
    }

    fn each_in(&self, _: Cell, _: Cell, _: &mut dyn FnMut(Area)) {}
}

/// The area `cell` covers: its merge's, or just itself.
fn area_of(cell: Cell, m: &dyn Merges) -> Area {
    m.at(cell).unwrap_or((cell, cell))
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

    /// The selected rectangle grown over the merges it touches: for a cursor on a merge
    /// with nothing else selected, the merge. What is drawn as selected, and what row,
    /// column, merge, and unmerge actions act on.
    pub fn area(&self, m: &dyn Merges) -> Area {
        cover(self.range(), m)
    }

    /// The rows and columns the view should keep visible after a key (first and last,
    /// inclusive): the extent while extending, else the cursor, with its whole merge. An
    /// axis the selection spans entirely (whole rows or columns) gives `None`: the view
    /// stays put along it, as in Calc.
    pub fn reveal(&self, m: &dyn Merges) -> Reveal {
        let end = if self.is_range() {
            self.extent
        } else {
            self.cursor
        };
        let (tl, br) = area_of(end, m);
        (
            (!self.full_height).then_some((tl.row, br.row)),
            (!self.full_width).then_some((tl.col, br.col)),
        )
    }

    /// The selected columns when the selection is whole columns (every row of them).
    pub fn whole_columns(&self) -> Option<std::ops::Range<u32>> {
        let (tl, br) = self.range();
        self.full_height.then(|| tl.col..br.col + 1)
    }

    /// The selection is whole rows (every column of them).
    pub fn is_whole_rows(&self) -> bool {
        self.full_width
    }

    /// More than the cursor's cell (or merge) is selected. [`Self::fit`] keeps a range
    /// that is just the cursor's merge collapsed, so anchor and extent differ only then.
    fn is_range(&self) -> bool {
        self.anchor != self.extent
    }

    /// Apply one key press.
    pub fn press(&mut self, key: Key, mods: Mods, b: Bounds, m: &dyn Merges) {
        if b.rows == 0 || b.cols == 0 {
            return;
        }
        let clamp = |c: Cell| Cell {
            row: c.row.min(b.rows - 1),
            col: c.col.min(b.cols - 1),
        };
        match key {
            Key::Space => {
                match (mods.shift, mods.ctrl) {
                    (true, true) => self.select_all(b),
                    (true, false) => self.whole_rows(b),
                    (false, true) => self.whole_cols(b),
                    (false, false) => {}
                }
                self.fit(m);
            }
            Key::Tab | Key::Enter if self.is_range() => self.cycle_in_range(key, mods.shift, m),
            Key::Tab => {
                // A run of Tabs remembers where it began, for the Enter that ends it.
                let start = self.tab_start.unwrap_or(self.cursor.col);
                let (tl, br) = area_of(self.cursor, m);
                let col = if mods.shift {
                    tl.col.saturating_sub(1)
                } else {
                    br.col + 1
                };
                self.collapse(clamp(Cell { col, ..self.cursor }), m);
                self.tab_start = Some(start);
            }
            Key::Enter => {
                let col = self.tab_start.take().unwrap_or(self.cursor.col);
                let (tl, br) = area_of(self.cursor, m);
                let row = if mods.shift {
                    tl.row.saturating_sub(1)
                } else {
                    br.row + 1
                };
                self.collapse(clamp(Cell { row, col }), m);
            }
            _ => {
                let from = if mods.shift { self.extent } else { self.cursor };
                let to = clamp(Self::target(key, mods.ctrl, from, b, m));
                if mods.shift {
                    self.extent = to;
                    self.tab_start = None;
                    self.fit(m);
                } else {
                    self.collapse(to, m);
                }
            }
        }
    }

    /// Where a movement key goes from `from`: arrows and paging step out of the merge
    /// `from` is in from its far edge.
    fn target(key: Key, ctrl: bool, from: Cell, b: Bounds, m: &dyn Merges) -> Cell {
        let Cell { row, col } = from;
        let (tl, br) = area_of(from, m);
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
                row: tl.row.saturating_sub(1),
                col,
            },
            (Key::Down, _) => Cell {
                row: br.row + 1,
                col,
            },
            (Key::Left, _) => Cell {
                row,
                col: tl.col.saturating_sub(1),
            },
            (Key::Right, _) => Cell {
                row,
                col: br.col + 1,
            },
            (Key::PageUp, _) => Cell {
                row: tl.row.saturating_sub(b.page_rows),
                col,
            },
            (Key::PageDown, _) => Cell {
                row: br.row.saturating_add(b.page_rows),
                col,
            },
            (Key::Tab | Key::Enter | Key::Space, _) => from,
        }
    }

    /// Just `to`, or the anchor of the merge it is in.
    fn collapse(&mut self, to: Cell, m: &dyn Merges) {
        *self = Self::at(area_of(to, m).0);
    }

    /// Make the selection respect merged cells: a cursor on a merge moves to its anchor,
    /// and a range grows to cover every merge it touches (anchor and extent move to the
    /// grown corners, on their sides). A range that is just the cursor's merge collapses
    /// to the cursor, so it acts as one cell. Changes nothing without merges.
    pub fn fit(&mut self, m: &dyn Merges) {
        if !self.is_range() {
            let (tl, br) = area_of(self.cursor, m);
            if tl != br || self.anchor != self.cursor {
                (self.cursor, self.anchor, self.extent) = (tl, tl, tl);
            }
            return;
        }
        let (tl, br) = cover(self.range(), m);
        let (rows_down, cols_right) = (
            self.anchor.row <= self.extent.row,
            self.anchor.col <= self.extent.col,
        );
        (self.anchor.row, self.extent.row) = if rows_down {
            (tl.row, br.row)
        } else {
            (br.row, tl.row)
        };
        (self.anchor.col, self.extent.col) = if cols_right {
            (tl.col, br.col)
        } else {
            (br.col, tl.col)
        };
        let own = area_of(self.cursor, m);
        if (tl, br) == own {
            (self.anchor, self.extent) = (own.0, own.0);
        }
        self.cursor = own.0;
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
    pub fn extend_to(&mut self, cell: Cell, m: &dyn Merges) {
        self.extent = cell;
        (self.full_width, self.full_height, self.tab_start) = (false, false, None);
        self.fit(m);
    }

    /// A click on a row header: select that row, with the cursor in it at `cursor_col`.
    /// `extend` (Shift+click, dragging) instead stretches the selection to whole rows
    /// through `row`, keeping the cursor. Rows a merge spans come along whole.
    pub fn click_row(
        &mut self,
        row: u64,
        extend: bool,
        cursor_col: u32,
        b: Bounds,
        m: &dyn Merges,
    ) {
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
        self.fit(m);
    }

    /// A click on a column header; see [`Selection::click_row`].
    pub fn click_col(
        &mut self,
        col: u32,
        extend: bool,
        cursor_row: u64,
        b: Bounds,
        m: &dyn Merges,
    ) {
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
        self.fit(m);
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
    /// or column by column (Enter), wrapping at the edges; Shift goes backwards. Cells a
    /// merge covers are passed over: the cursor stops only on a merge's anchor.
    fn cycle_in_range(&mut self, key: Key, back: bool, m: &dyn Merges) {
        let (tl, br) = self.range();
        let (row_major, forward) = (key == Key::Tab, !back);
        // (inner, outer) axis positions of a cell, and the range's bounds on them.
        let split = |c: Cell| {
            if row_major {
                (u64::from(c.col), c.row)
            } else {
                (c.row, u64::from(c.col))
            }
        };
        let join = |inner: u64, outer: u64| {
            if row_major {
                Cell {
                    row: outer,
                    col: inner as u32,
                }
            } else {
                Cell {
                    row: inner,
                    col: outer as u32,
                }
            }
        };
        let ((inner_lo, outer_lo), (inner_hi, outer_hi)) = (split(tl), split(br));
        let (mut inner, mut outer) = split(self.cursor);
        // Each pass lands, or skips a merge's run along the line: bounded by the cells.
        let limit = (inner_hi - inner_lo + 1).saturating_mul(outer_hi - outer_lo + 1);
        for _ in 0..limit {
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
            let cell = join(inner, outer);
            match m.at(cell) {
                None => break,
                Some((anchor, _)) if anchor == cell => break,
                // A covered cell: jump to the end of the merge's run along this line, so
                // the next pass steps past it (backwards, the anchor may be on this line).
                Some((a, z)) => {
                    let ((a_inner, a_outer), (z_inner, _)) = (split(a), split(z));
                    if forward {
                        inner = z_inner.min(inner_hi);
                    } else if a_outer == outer {
                        inner = a_inner;
                        break;
                    } else {
                        inner = a_inner.max(inner_lo);
                    }
                }
            }
        }
        self.cursor = join(inner, outer);
    }
}

/// `range` grown until every merge it touches lies inside it.
fn cover((mut tl, mut br): Area, m: &dyn Merges) -> Area {
    loop {
        let before = (tl, br);
        m.each_in(tl, br, &mut |(a, z)| {
            tl.row = tl.row.min(a.row);
            tl.col = tl.col.min(a.col);
            br.row = br.row.max(z.row);
            br.col = br.col.max(z.col);
        });
        if (tl, br) == before {
            return (tl, br);
        }
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
        sel.press(key, Mods { shift, ctrl: false }, B, &NoMerges);
    }

    /// Merged areas as a list, as the app's lookup reads them from the table.
    struct List(Vec<Area>);

    impl Merges for List {
        fn at(&self, cell: Cell) -> Option<Area> {
            self.0.iter().copied().find(|&(a, z)| {
                (a.row..=z.row).contains(&cell.row) && (a.col..=z.col).contains(&cell.col)
            })
        }

        fn each_in(&self, tl: Cell, br: Cell, f: &mut dyn FnMut(Area)) {
            for &(a, z) in &self.0 {
                if a.row <= br.row && tl.row <= z.row && a.col <= br.col && tl.col <= z.col {
                    f((a, z));
                }
            }
        }
    }

    fn c(row: u64, col: u32) -> Cell {
        Cell { row, col }
    }

    /// A1:C1 (a title across three columns) and B4:C5 (two by two).
    fn merges() -> List {
        List(vec![(c(0, 0), c(0, 2)), (c(3, 1), c(4, 2))])
    }

    fn go(sel: &mut Selection, key: Key, shift: bool, m: &List) {
        sel.press(key, Mods { shift, ctrl: false }, B, m);
    }

    #[test]
    fn arrows_step_over_a_merge_and_land_on_its_anchor() {
        let m = merges();
        let mut s = Selection::at(c(0, 0));
        go(&mut s, Key::Right, false, &m);
        assert_eq!(s.cursor(), c(0, 3), "Right from A1:C1 steps to D1");
        go(&mut s, Key::Left, false, &m);
        assert_eq!(s.cursor(), c(0, 0), "into the merge: its anchor");
        // Entering B4:C5 anywhere puts the cursor on B4.
        let mut s = Selection::at(c(2, 2));
        go(&mut s, Key::Down, false, &m);
        assert_eq!(s.cursor(), c(3, 1));
        assert_eq!(s.range(), (c(3, 1), c(3, 1)), "one cell, not a range");
        go(&mut s, Key::Down, false, &m);
        assert_eq!(s.cursor(), c(5, 1), "out from its bottom edge");
        let mut s = Selection::at(c(4, 3));
        go(&mut s, Key::Left, false, &m);
        assert_eq!(s.cursor(), c(3, 1));
        go(&mut s, Key::Left, false, &m);
        assert_eq!(s.cursor(), c(3, 0));
        // At the table's edge a merge doesn't move: A1:C1 has nothing above.
        let mut s = Selection::at(c(0, 0));
        go(&mut s, Key::Up, false, &m);
        assert_eq!(s.cursor(), c(0, 0));
    }

    #[test]
    fn tab_and_enter_step_over_merges_too() {
        let m = merges();
        let mut s = Selection::at(c(3, 0));
        go(&mut s, Key::Tab, false, &m);
        assert_eq!(s.cursor(), c(3, 1));
        go(&mut s, Key::Tab, false, &m);
        assert_eq!(s.cursor(), c(3, 3), "past C4");
        go(&mut s, Key::Tab, true, &m);
        assert_eq!(s.cursor(), c(3, 1), "Shift+Tab lands on the anchor");
        go(&mut s, Key::Enter, false, &m);
        assert_eq!(
            s.cursor(),
            c(5, 0),
            "below the merge, back in the column Tab began"
        );
    }

    #[test]
    fn a_range_grows_to_cover_the_merges_it_touches() {
        let m = merges();
        // Shift+Down from A3 reaches B4:C5's row: the range takes the whole merge.
        let mut s = Selection::at(c(2, 0));
        go(&mut s, Key::Right, true, &m);
        go(&mut s, Key::Down, true, &m);
        assert_eq!(s.range(), (c(2, 0), c(4, 2)));
        assert_eq!(s.cursor(), c(2, 0));
        // Shift+Right from the title selects A1:D1, not A1:B1.
        let mut s = Selection::at(c(0, 0));
        go(&mut s, Key::Right, true, &m);
        assert_eq!(s.range(), (c(0, 0), c(0, 3)));
        // and Shift+Left twice shrinks back to the title, then it is one cell again.
        go(&mut s, Key::Left, true, &m);
        assert_eq!(s.range(), (c(0, 0), c(0, 0)));
        // Dragging from D6 up to C5 takes B4:C5 in.
        let mut s = Selection::at(c(5, 3));
        s.extend_to(c(4, 2), &m);
        assert_eq!(s.range(), (c(3, 1), c(5, 3)));
        assert_eq!(s.cursor(), c(5, 3));
        // A row header click on row 5 takes row 4 too: B4:C5 spans both.
        s.click_row(4, false, 0, B, &m);
        assert_eq!(s.range(), (c(3, 0), c(4, 3)));
    }

    #[test]
    fn fit_puts_the_cursor_on_the_anchor_and_reveal_covers_the_merge() {
        let m = merges();
        // A click (or Go To) on a covered cell selects the merge.
        let mut s = Selection::at(c(4, 2));
        s.fit(&m);
        assert_eq!(s.cursor(), c(3, 1));
        assert_eq!(s.area(&m), (c(3, 1), c(4, 2)));
        assert_eq!(s.reveal(&m), (Some((3, 4)), Some((1, 2))));
        // Without merges nothing moves.
        let mut t = Selection::at(c(4, 2));
        t.fit(&NoMerges);
        assert_eq!(t, Selection::at(c(4, 2)));
    }

    #[test]
    fn tab_in_a_range_stops_only_on_anchors() {
        let m = merges();
        // A3:D5 holds B4:C5: Tab walks A3 B3 C3 D3 A4 B4 D4 A5 D5.
        let mut s = Selection::at(c(2, 0));
        s.extend_to(c(4, 3), &m);
        let mut seen = vec![s.cursor()];
        for _ in 0..9 {
            go(&mut s, Key::Tab, false, &m);
            seen.push(s.cursor());
        }
        let want = [
            c(2, 0),
            c(2, 1),
            c(2, 2),
            c(2, 3),
            c(3, 0),
            c(3, 1),
            c(3, 3),
            c(4, 0),
            c(4, 3),
            c(2, 0),
        ];
        assert_eq!(seen, want);
        // Backwards from D4: B4, the anchor, then A4.
        let mut s = Selection::at(c(2, 0));
        s.extend_to(c(4, 3), &m);
        for _ in 0..6 {
            go(&mut s, Key::Tab, false, &m);
        }
        assert_eq!(s.cursor(), c(3, 3));
        go(&mut s, Key::Tab, true, &m);
        assert_eq!(s.cursor(), c(3, 1));
        go(&mut s, Key::Tab, true, &m);
        assert_eq!(s.cursor(), c(3, 0));
        // From D5 backwards: C5 and B5 are covered, B4's anchor is a row up: A5.
        let mut s = Selection::at(c(2, 0));
        s.extend_to(c(4, 3), &m);
        for _ in 0..8 {
            go(&mut s, Key::Tab, false, &m);
        }
        assert_eq!(s.cursor(), c(4, 3));
        go(&mut s, Key::Tab, true, &m);
        assert_eq!(s.cursor(), c(4, 0));
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
            &NoMerges,
        );
        assert_eq!(s.cursor(), Cell::default());
    }

    #[test]
    fn view_follows_the_extent_but_not_along_whole_rows_or_columns() {
        let mut s = Selection::at(Cell { row: 2, col: 1 });
        press(&mut s, Key::PageDown, true);
        assert_eq!(s.reveal(&NoMerges), (Some((5, 5)), Some((1, 1))));
        assert_eq!(s.cursor(), Cell { row: 2, col: 1 });
        // A whole column runs to the last row; the view must not jump there.
        let ctrl = Mods {
            shift: false,
            ctrl: true,
        };
        s.press(Key::Space, ctrl, B, &NoMerges);
        assert_eq!(s.reveal(&NoMerges), (None, Some((1, 1))));
        press(&mut s, Key::Right, true);
        assert_eq!(s.reveal(&NoMerges), (None, Some((2, 2))));
        // Shift+Ctrl+Space: everything, the view stays put on both axes.
        s.press(
            Key::Space,
            Mods {
                shift: true,
                ..ctrl
            },
            B,
            &NoMerges,
        );
        assert_eq!(s.reveal(&NoMerges), (None, None));
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
            &NoMerges,
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
            &NoMerges,
        );
        s.grow(more, Bounds { rows: 900, ..B });
        assert_eq!(s.range().1.row, 498);
    }

    #[test]
    fn header_clicks_select_whole_rows_and_columns() {
        let mut s = Selection::default();
        s.click_row(4, false, 2, B, &NoMerges);
        assert_eq!(s.cursor(), Cell { row: 4, col: 2 });
        s.click_row(7, true, 0, B, &NoMerges); // Shift+click or drag: rows 5-8, cursor stays
        assert_eq!(
            s.range(),
            (Cell { row: 4, col: 0 }, Cell { row: 7, col: 3 })
        );
        assert_eq!(s.cursor(), Cell { row: 4, col: 2 });
        s.click_col(3, false, 6, B, &NoMerges);
        s.click_col(1, true, 0, B, &NoMerges);
        assert_eq!(
            s.range(),
            (Cell { row: 0, col: 1 }, Cell { row: 9, col: 3 })
        );
        assert_eq!(s.cursor(), Cell { row: 6, col: 3 });
        // Out-of-range header positions clamp to the table.
        s.click_row(99, false, 99, B, &NoMerges);
        assert_eq!(s.cursor(), Cell { row: 9, col: 3 });
    }

    #[test]
    fn shift_click_and_drag_keep_the_cursor() {
        let mut s = Selection::at(Cell { row: 1, col: 1 });
        s.extend_to(Cell { row: 5, col: 0 }, &NoMerges);
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
