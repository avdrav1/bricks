//! The grid widget: draws the cells `grid::GridCache` holds with GSK + Pango (ADR 0001).

use commands::{SetCell, UndoStack};
use data_model::{
    CellRef, CsvTable, DelimiterChoice, RereadError, SaveJob, SaveJobError, TableSource,
};
use grid::{Bounds, ColumnViewport, GridCache, Key, Mods, Selection, Viewport};
use gtk::{gdk, glib, graphene, pango, prelude::*, subclass::prelude::*};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt::Write as _;

pub const ROW_H: f64 = 22.0;
pub const COL_W: f64 = 120.0;
pub const HEADER_H: f64 = 24.0;
/// Rows loaded above and below the visible ones.
const BUFFER_ROWS: u64 = 100;
const PAD: f64 = 4.0;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct GridView {
        pub table: RefCell<Option<CsvTable>>,
        pub cache: RefCell<GridCache>,
        pub vadj: RefCell<Option<gtk::Adjustment>>,
        pub hadj: RefCell<Option<gtk::Adjustment>>,
        /// Shaped text, kept while used: entries unused for a whole frame are dropped, so
        /// this holds at most two screens of cells.
        pub layouts: RefCell<HashMap<String, pango::Layout>>,
        pub layouts_prev: RefCell<HashMap<String, pango::Layout>>,
        pub buf: RefCell<String>,
        pub row_header_w: Cell<f64>,
        /// Top row of the last frame drawn with its text loaded; `None` before then.
        pub painted_top: Cell<Option<u64>>,
        /// Every edit goes through here as a command (invariant 5). Undo keys arrive with
        /// CMD-1/CMD-2.
        pub undo: RefCell<Option<UndoStack<CsvTable>>>,
        /// Small editor shown over a cell on double-click (until the in-cell editor, EDIT-2).
        pub editor: RefCell<Option<(gtk::Popover, gtk::Entry)>>,
        /// Cell the editor is open on.
        pub editing: Cell<Option<(u64, u32)>>,
        /// A save is running; edits wait so none can be lost between snapshot and reopen.
        pub saving: Cell<bool>,
        /// The cell cursor and selected range (GRID-3).
        pub selection: Cell<Selection>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GridView {
        const NAME: &'static str = "BricksGridView";
        type Type = super::GridView;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for GridView {
        fn dispose(&self) {
            if let Some((popover, _)) = self.editor.take() {
                popover.unparent();
            }
        }
    }

    impl GridView {
        fn layout(&self, text: &str) -> pango::Layout {
            if let Some(l) = self.layouts.borrow().get(text) {
                return l.clone();
            }
            let l = self
                .layouts_prev
                .borrow_mut()
                .remove(text)
                .unwrap_or_else(|| {
                    let l = self.obj().create_pango_layout(Some(text));
                    // Embedded newlines show as a glyph instead of breaking the row.
                    l.set_single_paragraph_mode(true);
                    l
                });
            self.layouts.borrow_mut().insert(text.to_owned(), l.clone());
            l
        }

        fn end_frame(&self) {
            let cur = std::mem::take(&mut *self.layouts.borrow_mut());
            *self.layouts_prev.borrow_mut() = cur;
        }

        pub fn body_size(&self) -> (f64, f64) {
            let w = self.obj().width() as f64 - self.row_header_w.get();
            let h = self.obj().height() as f64 - HEADER_H;
            (w.max(0.0), h.max(0.0))
        }
    }

    impl WidgetImpl for GridView {
        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.parent_size_allocate(width, height, baseline);
            self.obj().update_adjustments();
            // GTK4: a widget that parents a popover must position it on every allocation.
            if let Some((popover, _)) = self.editor.borrow().as_ref() {
                popover.present();
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let (Some(vadj), Some(hadj)) = (self.vadj.borrow().clone(), self.hadj.borrow().clone())
            else {
                return;
            };
            let mut table = self.table.borrow_mut();
            let Some(table) = table.as_mut() else { return };
            let mut cache = self.cache.borrow_mut();
            let (w, h) = (obj.width() as f64, obj.height() as f64);
            let rh_w = self.row_header_w.get();
            let (body_w, body_h) = self.body_size();

            let rows = Viewport {
                scroll_y: vadj.value(),
                height: body_h,
                row_height: ROW_H,
                total_rows: table.row_count(),
            };
            let visible = rows.visible_rows(0);
            cache.ensure(visible.clone(), BUFFER_ROWS, table);
            self.painted_top
                .set(cache.cell(visible.start, 0).map(|_| visible.start));
            let cols = ColumnViewport {
                scroll_x: hadj.value(),
                width: body_w,
                col_width: COL_W,
                total_cols: cache.col_count(),
            };
            let visible_cols = cols.visible_cols();

            let rect = |x: f64, y: f64, w: f64, h: f64| {
                graphene::Rect::new(x as f32, y as f32, w as f32, h as f32)
            };
            let fg = obj.color();
            let line = gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), 0.15);
            let stripe = gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), 0.035);
            let header_bg = gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), 0.07);
            let text_at = |text: &str, x: f64, y: f64| {
                let layout = self.layout(text);
                snapshot.save();
                snapshot.translate(&graphene::Point::new(x as f32, y as f32));
                snapshot.append_layout(&layout, &fg);
                snapshot.restore();
            };
            let mut buf = self.buf.borrow_mut();

            // Body: stripes, grid lines, cells.
            snapshot.push_clip(&rect(rh_w, HEADER_H, body_w, body_h));
            for r in visible.clone() {
                let y = HEADER_H + rows.row_y(r);
                if r % 2 == 1 {
                    snapshot.append_color(&stripe, &rect(rh_w, y, body_w, ROW_H));
                }
                snapshot.append_color(&line, &rect(rh_w, y + ROW_H - 1.0, body_w, 1.0));
            }
            for c in visible_cols.clone() {
                let x = rh_w + cols.col_x(c);
                snapshot.append_color(&line, &rect(x + COL_W - 1.0, HEADER_H, 1.0, body_h));
                snapshot.push_clip(&rect(x, HEADER_H, COL_W - 1.0, body_h));
                for r in visible.clone() {
                    if let Some(text) = cache.cell(r, c).filter(|t| !t.is_empty()) {
                        text_at(text, x + PAD, HEADER_H + rows.row_y(r) + 3.0);
                    }
                }
                snapshot.pop();
            }

            // Selection and cell cursor. Coordinates are clamped to just outside the body
            // before narrowing to f32, so far-away edges stay exact.
            let sel = self.selection.get();
            let (tl, br) = sel.range();
            let clamp_x = |x: f64| x.clamp(rh_w - 4.0, w + 4.0);
            let clamp_y = |y: f64| y.clamp(HEADER_H - 4.0, h + 4.0);
            let accent = gdk::RGBA::new(0.21, 0.52, 0.89, 1.0);
            if tl != br {
                let (x0, y0) = (
                    clamp_x(rh_w + cols.col_x(tl.col)),
                    clamp_y(HEADER_H + rows.row_y(tl.row)),
                );
                let (x1, y1) = (
                    clamp_x(rh_w + cols.col_x(br.col) + COL_W),
                    clamp_y(HEADER_H + rows.row_y(br.row) + ROW_H),
                );
                let fill = gdk::RGBA::new(accent.red(), accent.green(), accent.blue(), 0.18);
                snapshot.append_color(&fill, &rect(x0, y0, x1 - x0, y1 - y0));
            }
            let cur = sel.cursor();
            let (cx, cy) = (rh_w + cols.col_x(cur.col), HEADER_H + rows.row_y(cur.row));
            if cy > HEADER_H - ROW_H && cy < h && cx > rh_w - COL_W && cx < w {
                for (x, y, bw, bh) in [
                    (cx, cy, COL_W, 2.0),
                    (cx, cy + ROW_H - 2.0, COL_W, 2.0),
                    (cx, cy, 2.0, ROW_H),
                    (cx + COL_W - 2.0, cy, 2.0, ROW_H),
                ] {
                    snapshot.append_color(&accent, &rect(x, y, bw, bh));
                }
            }
            snapshot.pop();

            // Row numbers.
            snapshot.append_color(&header_bg, &rect(0.0, HEADER_H, rh_w, body_h));
            snapshot.push_clip(&rect(0.0, HEADER_H, rh_w, body_h));
            for r in visible {
                buf.clear();
                let _ = write!(buf, "{}", r + 1);
                text_at(&buf, PAD + 2.0, HEADER_H + rows.row_y(r) + 3.0);
            }
            snapshot.pop();

            // Column letters.
            snapshot.append_color(&header_bg, &rect(0.0, 0.0, w, HEADER_H));
            snapshot.append_color(&line, &rect(0.0, HEADER_H - 1.0, w, 1.0));
            snapshot.append_color(&line, &rect(rh_w - 1.0, 0.0, 1.0, h));
            snapshot.push_clip(&rect(rh_w, 0.0, body_w, HEADER_H));
            for c in visible_cols {
                let x = rh_w + cols.col_x(c);
                column_name(c, &mut buf);
                text_at(&buf, x + PAD, 4.0);
            }
            snapshot.pop();

            drop(buf);
            self.end_frame();
        }
    }
}

glib::wrapper! {
    pub struct GridView(ObjectSubclass<imp::GridView>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl GridView {
    pub fn new(table: CsvTable, vadj: &gtk::Adjustment, hadj: &gtk::Adjustment) -> Self {
        let g: Self = glib::Object::new();
        let imp = g.imp();
        imp.table.replace(Some(table));
        imp.vadj.replace(Some(vadj.clone()));
        imp.hadj.replace(Some(hadj.clone()));
        imp.row_header_w.set(64.0);
        imp.undo.replace(Some(UndoStack::new(1_000)));
        let click = gtk::GestureClick::new();
        click.connect_pressed({
            let g = g.downgrade();
            move |_, n_press, x, y| {
                let Some(g) = g.upgrade() else { return };
                g.grab_focus();
                if n_press == 2 {
                    g.begin_edit(x, y);
                }
            }
        });
        g.add_controller(click);
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed({
            let g = g.downgrade();
            move |_, key, _, state| match (g.upgrade(), nav_key(key, state)) {
                (Some(g), Some((key, mods))) => {
                    g.press(key, mods);
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        g.add_controller(keys);
        g.set_hexpand(true);
        g.set_vexpand(true);
        g.set_focusable(true);
        g.set_overflow(gtk::Overflow::Hidden);
        // The theme's opaque content background (GTK draws it before `snapshot`).
        g.add_css_class("view");
        for adj in [vadj, hadj] {
            adj.connect_value_changed({
                let g = g.downgrade();
                move |_| {
                    if let Some(g) = g.upgrade() {
                        g.queue_draw();
                    }
                }
            });
        }
        g
    }

    /// What navigation moves within: the rows indexed so far, the widest row seen, and one
    /// screen of whole rows.
    fn bounds(&self) -> Bounds {
        let (_, body_h) = self.imp().body_size();
        Bounds {
            rows: self.row_count(),
            cols: self.imp().cache.borrow().col_count(),
            page_rows: ((body_h / ROW_H).floor() as u64).max(1),
        }
    }

    /// Apply a navigation key (GRID-3) and keep the moved cell in view.
    pub fn press(&self, key: Key, mods: Mods) {
        let imp = self.imp();
        let bounds = self.bounds();
        let mut sel = imp.selection.get();
        sel.press(key, mods, bounds);
        imp.selection.set(sel);
        // Paging scrolls the view by a page as well, like Calc.
        if let (Key::PageUp | Key::PageDown, Some(vadj)) = (key, imp.vadj.borrow().as_ref()) {
            let step = bounds.page_rows as f64 * ROW_H;
            vadj.set_value(vadj.value() + if key == Key::PageDown { step } else { -step });
        }
        self.scroll_to(sel.moving_end());
        self.queue_draw();
    }

    /// Scroll the least amount that shows `cell` whole.
    fn scroll_to(&self, cell: grid::Cell) {
        let imp = self.imp();
        let (body_w, body_h) = imp.body_size();
        let reveal = |adj: &gtk::Adjustment, start: f64, size: f64, view: f64| {
            if start < adj.value() {
                adj.set_value(start);
            } else if start + size > adj.value() + view {
                adj.set_value(start + size - view);
            }
        };
        if let Some(v) = imp.vadj.borrow().as_ref() {
            reveal(v, cell.row as f64 * ROW_H, ROW_H, body_h);
        }
        if let Some(hz) = imp.hadj.borrow().as_ref() {
            reveal(hz, f64::from(cell.col) * COL_W, COL_W, body_w);
        }
    }

    pub fn selection(&self) -> Selection {
        self.imp().selection.get()
    }

    /// Open the cell editor on the cell under widget point (`x`, `y`), prefilled with the
    /// cell's full value.
    fn begin_edit(&self, x: f64, y: f64) {
        let imp = self.imp();
        if imp.saving.get() {
            return;
        }
        let (Some(vadj), Some(hadj)) = (imp.vadj.borrow().clone(), imp.hadj.borrow().clone())
        else {
            return;
        };
        let (body_w, body_h) = imp.body_size();
        let rh_w = imp.row_header_w.get();
        let rows = Viewport {
            scroll_y: vadj.value(),
            height: body_h,
            row_height: ROW_H,
            total_rows: self.row_count(),
        };
        let cols = ColumnViewport {
            scroll_x: hadj.value(),
            width: body_w,
            col_width: COL_W,
            total_cols: imp.cache.borrow().col_count(),
        };
        let (Some(row), Some(col)) = (rows.row_at(y - HEADER_H), cols.col_at(x - rh_w)) else {
            return;
        };
        let value = imp
            .table
            .borrow()
            .as_ref()
            .and_then(|t| t.cell_value(row, col).map(|v| v.into_owned()));

        let (popover, entry) = imp
            .editor
            .borrow_mut()
            .get_or_insert_with(|| self.build_editor())
            .clone();
        let cell = gdk::Rectangle::new(
            (rh_w + cols.col_x(col)) as i32,
            (HEADER_H + rows.row_y(row)) as i32,
            COL_W as i32,
            ROW_H as i32,
        );
        imp.editing.set(Some((row, col)));
        imp.selection.set(Selection::at(grid::Cell { row, col }));
        self.queue_draw();
        popover.set_pointing_to(Some(&cell));
        entry.set_text(value.as_deref().unwrap_or(""));
        popover.popup();
        entry.grab_focus();
    }

    fn build_editor(&self) -> (gtk::Popover, gtk::Entry) {
        let entry = gtk::Entry::builder().width_chars(30).build();
        let popover = gtk::Popover::builder()
            .child(&entry)
            .position(gtk::PositionType::Bottom)
            .build();
        popover.set_parent(self);
        entry.connect_activate({
            let g = self.downgrade();
            move |entry| {
                if let Some(g) = g.upgrade() {
                    g.commit_edit(&entry.text());
                }
            }
        });
        popover.connect_closed({
            let g = self.downgrade();
            move |_| {
                if let Some(g) = g.upgrade() {
                    g.imp().editing.set(None);
                    g.grab_focus();
                }
            }
        });
        (popover, entry)
    }

    /// Apply the editor's text to its cell as an undoable command, then close the editor.
    fn commit_edit(&self, text: &str) {
        let imp = self.imp();
        if let Some((row, col)) = imp.editing.get() {
            let mut table = imp.table.borrow_mut();
            if let (Some(table), Some(undo)) = (table.as_mut(), imp.undo.borrow_mut().as_mut()) {
                if table.cell_value(row, col).as_deref() != Some(text) {
                    let at = CellRef {
                        row: table.row_id(row),
                        col,
                    };
                    undo.execute(Box::new(SetCell::new(at, text)), table);
                    imp.cache.borrow_mut().invalidate();
                    self.queue_draw();
                }
            }
        }
        if let Some((popover, _)) = imp.editor.borrow().as_ref() {
            popover.popdown();
        }
    }

    /// Snapshot for a background save (SAVE-1).
    pub fn save_job(&self) -> Result<SaveJob, SaveJobError> {
        self.imp()
            .table
            .borrow()
            .as_ref()
            .map_or(Err(SaveJobError::NotIndexed), |t| t.save_job())
    }

    pub fn set_saving(&self, saving: bool) {
        self.imp().saving.set(saving);
    }

    /// Show a freshly opened table (the file just saved). Its edits are on disk now, so the
    /// undo history, which refers to the old overlay, starts over.
    pub fn replace_table(&self, table: CsvTable) {
        let imp = self.imp();
        imp.table.replace(Some(table));
        imp.undo.replace(Some(UndoStack::new(1_000)));
        imp.cache.borrow_mut().reset();
        self.update_adjustments();
    }

    /// The open file read with another delimiter (ENG-5); refused with unsaved edits.
    pub fn reread(&self, choice: DelimiterChoice) -> Result<CsvTable, RereadError> {
        let table = self.imp().table.borrow();
        table
            .as_ref()
            .expect("a GridView always holds a table after new()")
            .reread(choice)
    }

    pub fn encoding(&self) -> csv_engine::Encoding {
        self.imp()
            .table
            .borrow()
            .as_ref()
            .map_or(csv_engine::Encoding::UTF8, |t| t.encoding())
    }

    pub fn delimiter(&self) -> u8 {
        self.imp()
            .table
            .borrow()
            .as_ref()
            .map_or(b',', |t| t.dialect().delimiter)
    }

    /// Cells edited since open.
    pub fn edit_count(&self) -> usize {
        self.imp()
            .table
            .borrow()
            .as_ref()
            .map_or(0, |t| t.overlay().len())
    }

    pub fn row_count(&self) -> u64 {
        self.imp()
            .table
            .borrow()
            .as_ref()
            .map_or(0, |t| t.row_count())
    }

    pub fn is_complete(&self) -> bool {
        self.imp()
            .table
            .borrow()
            .as_ref()
            .is_some_and(|t| t.is_complete())
    }

    pub fn file_changed(&self) -> bool {
        self.imp()
            .table
            .borrow()
            .as_ref()
            .is_some_and(|t| t.file_changed())
    }

    pub fn cache_bytes(&self) -> usize {
        self.imp().cache.borrow().heap_bytes()
    }

    /// Top row of the last painted frame, once its cells were loaded and drawn.
    pub fn painted_top_row(&self) -> Option<u64> {
        self.imp().painted_top.get()
    }

    /// Resize scroll ranges to the rows indexed so far and the widest row seen.
    pub fn update_adjustments(&self) {
        let imp = self.imp();
        let rows = self.row_count();
        // Room for the widest row number in the current font (digits are tabular).
        let digits = rows.max(1).ilog10() as i32 + 1;
        let digit_w = self.create_pango_layout(Some("0")).pixel_size().0.max(1);
        imp.row_header_w
            .set(f64::from(digits * digit_w) + 3.0 * PAD);
        let (body_w, body_h) = imp.body_size();
        // GTK requires upper >= page size; short or narrow tables just don't scroll.
        if let Some(v) = imp.vadj.borrow().as_ref() {
            let upper = (rows as f64 * ROW_H).max(body_h);
            v.configure(v.value(), 0.0, upper, ROW_H, body_h * 0.9, body_h);
        }
        if let Some(hz) = imp.hadj.borrow().as_ref() {
            let upper = (imp.cache.borrow().col_count().max(1) as f64 * COL_W).max(body_w);
            hz.configure(hz.value(), 0.0, upper, COL_W / 4.0, body_w * 0.9, body_w);
        }
        self.queue_draw();
    }
}

/// Map a GDK key press to a navigation key (GRID-3). `None` lets it through (Ctrl+S, and
/// Ctrl+arrows, which are not built yet).
fn nav_key(key: gdk::Key, state: gdk::ModifierType) -> Option<(Key, Mods)> {
    use gdk::Key as K;
    let mut mods = Mods {
        shift: state.contains(gdk::ModifierType::SHIFT_MASK),
        ctrl: state.contains(gdk::ModifierType::CONTROL_MASK),
    };
    let nav = match key {
        K::Up | K::KP_Up => Key::Up,
        K::Down | K::KP_Down => Key::Down,
        K::Left | K::KP_Left => Key::Left,
        K::Right | K::KP_Right => Key::Right,
        K::Tab | K::KP_Tab => Key::Tab,
        K::ISO_Left_Tab => {
            mods.shift = true; // Shift+Tab arrives as its own keysym
            Key::Tab
        }
        K::Return | K::KP_Enter => Key::Enter,
        K::Home | K::KP_Home => Key::Home,
        K::End | K::KP_End => Key::End,
        K::Page_Up | K::KP_Page_Up => Key::PageUp,
        K::Page_Down | K::KP_Page_Down => Key::PageDown,
        _ => return None,
    };
    if mods.ctrl && !matches!(nav, Key::Home | Key::End) {
        return None;
    }
    Some((nav, mods))
}

/// Spreadsheet column name: 0 -> A, 25 -> Z, 26 -> AA.
pub fn column_name(mut col: u32, out: &mut String) {
    out.clear();
    let mut letters = [0u8; 7];
    let mut i = letters.len();
    loop {
        i -= 1;
        letters[i] = b'A' + (col % 26) as u8;
        if col < 26 {
            break;
        }
        col = col / 26 - 1;
    }
    out.push_str(std::str::from_utf8(&letters[i..]).unwrap());
}

#[cfg(test)]
mod tests {
    #[test]
    fn column_names() {
        let mut s = String::new();
        for (c, want) in [
            (0, "A"),
            (25, "Z"),
            (26, "AA"),
            (51, "AZ"),
            (52, "BA"),
            (701, "ZZ"),
            (702, "AAA"),
        ] {
            super::column_name(c, &mut s);
            assert_eq!(s, want);
        }
    }
}
