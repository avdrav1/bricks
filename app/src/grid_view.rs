//! The grid widget: draws the cells `grid::GridCache` holds with GSK + Pango (ADR 0001).

use commands::{SetCell, UndoStack};
use data_model::{
    CellRef, CsvTable, DelimiterChoice, RereadError, SaveJob, SaveJobError, TableSource,
};
use grid::{Bounds, GridCache, Key, Mods, Selection, Sizes, Viewport};
use gtk::{gdk, glib, graphene, pango, prelude::*, subclass::prelude::*};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt::Write as _;

/// Default row height and column width; GRID-5 lets the user resize each one.
pub const ROW_H: f64 = 22.0;
pub const COL_W: f64 = 120.0;
pub const HEADER_H: f64 = 24.0;
/// Rows loaded above and below the visible ones.
const BUFFER_ROWS: u64 = 100;
const PAD: f64 = 4.0;
/// Smallest sizes a drag can give a column or row.
const MIN_COL_W: f64 = 16.0;
const MIN_ROW_H: f64 = 8.0;

/// Row heights and column widths, all default until resized (GRID-5). Kept for the life
/// of the window: a save keeps them, a delimiter change resets the widths.
pub struct Sizing {
    pub rows: Sizes,
    pub cols: Sizes,
}

impl Default for Sizing {
    fn default() -> Self {
        Self {
            rows: Sizes::new(ROW_H),
            cols: Sizes::new(COL_W),
        }
    }
}

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
        /// The cell cursor and selected range (GRID-3, GRID-4).
        pub selection: Cell<Selection>,
        /// Table size the selection last saw, to grow whole rows/columns with indexing.
        pub bounds_seen: Cell<Option<Bounds>>,
        /// What the current mouse drag selects, and where the pointer is.
        pub drag: Cell<Option<(Drag, f64, f64)>>,
        /// Repeats the drag while the pointer is outside the body, scrolling toward it.
        pub autoscroll: RefCell<Option<glib::SourceId>>,
        /// Row heights and column widths (GRID-5).
        pub sizing: RefCell<Sizing>,
        /// Column titles from the header row (ENG-7), read once rather than every frame;
        /// `None` until read, and again after anything that could change them.
        pub titles: RefCell<Option<Vec<String>>>,
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
        pub fn layout(&self, text: &str) -> pango::Layout {
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
            obj.bounds(); // grow whole rows/columns to what indexing found
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

            let sizing = self.sizing.borrow();
            let rows = Viewport {
                scroll: vadj.value(),
                extent: body_h,
                sizes: &sizing.rows,
                count: table.row_count(),
            };
            let visible = rows.visible(0);
            cache.ensure(visible.clone(), BUFFER_ROWS, table);
            self.painted_top
                .set(cache.cell(visible.start, 0).map(|_| visible.start));
            let cols = Viewport {
                scroll: hadj.value(),
                extent: body_w,
                sizes: &sizing.cols,
                count: u64::from(cache.col_count()),
            };
            let visible_cols = cols.visible(0);

            let rect = |x: f64, y: f64, w: f64, h: f64| {
                graphene::Rect::new(x as f32, y as f32, w as f32, h as f32)
            };
            let fg = obj.color();
            let line = gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), 0.15);
            let stripe = gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), 0.035);
            let header_bg = gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), 0.07);
            // Draws `text` at (`x`, `y`) in `color`; returns its width.
            let text_in = |text: &str, x: f64, y: f64, color: &gdk::RGBA| {
                let layout = self.layout(text);
                snapshot.save();
                snapshot.translate(&graphene::Point::new(x as f32, y as f32));
                snapshot.append_layout(&layout, color);
                snapshot.restore();
                f64::from(layout.pixel_size().0)
            };
            let text_at = |text: &str, x: f64, y: f64| {
                text_in(text, x, y, &fg);
            };
            let dim = gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), 0.5);
            let mut buf = self.buf.borrow_mut();

            // Body: stripes, grid lines, cells.
            snapshot.push_clip(&rect(rh_w, HEADER_H, body_w, body_h));
            for r in visible.clone() {
                let (y, rh) = (HEADER_H + rows.pos(r), rows.size(r));
                if r % 2 == 1 {
                    snapshot.append_color(&stripe, &rect(rh_w, y, body_w, rh));
                }
                snapshot.append_color(&line, &rect(rh_w, y + rh - 1.0, body_w, 1.0));
            }
            for c in visible_cols.clone() {
                let (x, cw) = (rh_w + cols.pos(c), cols.size(c));
                snapshot.append_color(&line, &rect(x + cw - 1.0, HEADER_H, 1.0, body_h));
                snapshot.push_clip(&rect(x, HEADER_H, cw - 1.0, body_h));
                for r in visible.clone() {
                    if let Some(text) = cache.cell(r, c as u32).filter(|t| !t.is_empty()) {
                        text_at(text, x + PAD, HEADER_H + rows.pos(r) + 3.0);
                    }
                }
                snapshot.pop();
            }

            // Selection and cell cursor. Coordinates are clamped to just outside the body
            // before narrowing to f32, so far-away edges stay exact.
            let sel = self.selection.get();
            let (tl, br) = sel.range();
            let (tl_col, br_col) = (u64::from(tl.col), u64::from(br.col));
            let clamp_x = |x: f64| x.clamp(rh_w - 4.0, w + 4.0);
            let clamp_y = |y: f64| y.clamp(HEADER_H - 4.0, h + 4.0);
            let (x0, x1) = (
                clamp_x(rh_w + cols.pos(tl_col)),
                clamp_x(rh_w + cols.pos(br_col) + cols.size(br_col)),
            );
            let (y0, y1) = (
                clamp_y(HEADER_H + rows.pos(tl.row)),
                clamp_y(HEADER_H + rows.pos(br.row) + rows.size(br.row)),
            );
            let accent = gdk::RGBA::new(0.21, 0.52, 0.89, 1.0);
            if tl != br {
                let fill = gdk::RGBA::new(accent.red(), accent.green(), accent.blue(), 0.18);
                snapshot.append_color(&fill, &rect(x0, y0, x1 - x0, y1 - y0));
            }
            let cur = sel.cursor();
            let (cx, cy) = (
                rh_w + cols.pos(u64::from(cur.col)),
                HEADER_H + rows.pos(cur.row),
            );
            let (cw, ch) = (cols.size(u64::from(cur.col)), rows.size(cur.row));
            if cy > HEADER_H - ch && cy < h && cx > rh_w - cw && cx < w {
                for (x, y, bw, bh) in [
                    (cx, cy, cw, 2.0),
                    (cx, cy + ch - 2.0, cw, 2.0),
                    (cx, cy, 2.0, ch),
                    (cx + cw - 2.0, cy, 2.0, ch),
                ] {
                    snapshot.append_color(&accent, &rect(x, y, bw, bh));
                }
            }
            snapshot.pop();

            // Row numbers.
            snapshot.append_color(&header_bg, &rect(0.0, HEADER_H, rh_w, body_h));
            let mark = gdk::RGBA::new(accent.red(), accent.green(), accent.blue(), 0.25);
            snapshot.append_color(&mark, &rect(0.0, y0, rh_w, y1 - y0));
            snapshot.push_clip(&rect(0.0, HEADER_H, rh_w, body_h));
            for r in visible {
                let y = HEADER_H + rows.pos(r);
                snapshot.append_color(&line, &rect(0.0, y + rows.size(r) - 1.0, rh_w, 1.0));
                buf.clear();
                let _ = write!(buf, "{}", r + 1);
                text_at(&buf, PAD + 2.0, y + 3.0);
            }
            snapshot.pop();

            // Column letters, and the column titles when the first row is a header (ENG-7).
            snapshot.append_color(&header_bg, &rect(0.0, 0.0, w, HEADER_H));
            snapshot.append_color(&mark, &rect(x0, 0.0, x1 - x0, HEADER_H));
            snapshot.append_color(&line, &rect(0.0, HEADER_H - 1.0, w, 1.0));
            snapshot.append_color(&line, &rect(rh_w - 1.0, 0.0, 1.0, h));
            snapshot.push_clip(&rect(rh_w, 0.0, body_w, HEADER_H));
            // Row 0 is readable once any later row is, or indexing is done.
            let row0_ready = table.row_count() > 0 || table.is_complete();
            if self.titles.borrow().is_none() && row0_ready {
                let titles = (0..)
                    .map_while(|c| table.header_cell(c).map(|t| t.into_owned()))
                    .collect();
                self.titles.replace(Some(titles));
            }
            let titles = self.titles.borrow();
            let titles = titles.as_deref().unwrap_or(&[]);
            for c in visible_cols {
                let (x, cw) = (rh_w + cols.pos(c), cols.size(c));
                snapshot.append_color(&line, &rect(x + cw - 1.0, 0.0, 1.0, HEADER_H));
                snapshot.push_clip(&rect(x, 0.0, cw - 1.0, HEADER_H));
                column_name(c as u32, &mut buf);
                match titles.get(c as usize) {
                    Some(title) => {
                        let letter_w = text_in(&buf, x + PAD, 4.0, &dim);
                        text_at(title, x + PAD + letter_w + 6.0, 4.0);
                    }
                    None => text_at(&buf, x + PAD, 4.0),
                }
                snapshot.pop();
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
            move |gesture, n_press, x, y| {
                let Some(g) = g.upgrade() else { return };
                g.grab_focus();
                if n_press == 2 {
                    g.double_click(x, y);
                } else {
                    let shift = gesture
                        .current_event_state()
                        .contains(gdk::ModifierType::SHIFT_MASK);
                    g.mouse_down(x, y, shift);
                }
            }
        });
        g.add_controller(click);
        let drag = gtk::GestureDrag::new();
        drag.connect_drag_update({
            let g = g.downgrade();
            move |gesture, dx, dy| {
                let (Some(g), Some((x, y))) = (g.upgrade(), gesture.start_point()) else {
                    return;
                };
                g.drag_to(x + dx, y + dy);
            }
        });
        drag.connect_drag_end({
            let g = g.downgrade();
            move |_, _, _| {
                if let Some(g) = g.upgrade() {
                    g.imp().drag.set(None);
                    if let Some(id) = g.imp().autoscroll.take() {
                        id.remove();
                    }
                }
            }
        });
        g.add_controller(drag);
        // The resize cursor over the borders in the row and column headers.
        let motion = gtk::EventControllerMotion::new();
        motion.connect_motion({
            let g = g.downgrade();
            move |_, x, y| {
                let Some(g) = g.upgrade() else { return };
                if matches!(g.imp().drag.get(), Some((Drag::Resize { .. }, ..))) {
                    return;
                }
                g.set_cursor_from_name(match g.edge_at(x, y) {
                    Some(Edge::Col(_)) => Some("col-resize"),
                    Some(Edge::Row(_)) => Some("row-resize"),
                    None => None,
                });
            }
        });
        g.add_controller(motion);
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
    /// screen of whole rows. Whole-row and whole-column selections follow it as it grows.
    fn bounds(&self) -> Bounds {
        let imp = self.imp();
        let (_, body_h) = imp.body_size();
        let b = Bounds {
            rows: self.row_count(),
            cols: imp.cache.borrow().col_count(),
            page_rows: ((body_h / ROW_H).floor() as u64).max(1),
        };
        if let Some(old) = imp.bounds_seen.replace(Some(b)) {
            if (old.rows, old.cols) != (b.rows, b.cols) {
                let mut sel = imp.selection.get();
                sel.grow(old, b);
                imp.selection.set(sel);
            }
        }
        b
    }

    /// Both axes of the body as they are scrolled and sized now: rows, then columns.
    fn with_viewports<R>(&self, f: impl FnOnce(Viewport<'_>, Viewport<'_>) -> R) -> R {
        let imp = self.imp();
        let (body_w, body_h) = imp.body_size();
        let value = |adj: &RefCell<Option<gtk::Adjustment>>| {
            adj.borrow().as_ref().map_or(0.0, |a| a.value())
        };
        let (row_count, col_count) = (self.row_count(), imp.cache.borrow().col_count());
        let sizing = imp.sizing.borrow();
        f(
            Viewport {
                scroll: value(&imp.vadj),
                extent: body_h,
                sizes: &sizing.rows,
                count: row_count,
            },
            Viewport {
                scroll: value(&imp.hadj),
                extent: body_w,
                sizes: &sizing.cols,
                count: u64::from(col_count),
            },
        )
    }

    /// The cell under widget point (`x`, `y`), clamped into the body and the table, and
    /// the top-left visible cell. Call only with a non-empty table.
    fn cell_near(&self, x: f64, y: f64) -> (grid::Cell, grid::Cell) {
        let (x, y) = (x - self.imp().row_header_w.get(), y - HEADER_H);
        self.with_viewports(|rows, cols| {
            let at = |r: Option<u64>, c: Option<u64>| grid::Cell {
                row: r.unwrap_or(0),
                col: c.unwrap_or(0) as u32,
            };
            (
                at(rows.nearest(y), cols.nearest(x)),
                at(rows.nearest(0.0), cols.nearest(0.0)),
            )
        })
    }

    /// The row or column border under widget point (`x`, `y`) in the headers, where a
    /// drag resizes (GRID-5).
    fn edge_at(&self, x: f64, y: f64) -> Option<Edge> {
        let rh_w = self.imp().row_header_w.get();
        self.with_viewports(|rows, cols| match (x < rh_w, y < HEADER_H) {
            (false, true) => cols.edge_at(x - rh_w, 4.0).map(|c| Edge::Col(c as u32)),
            (true, false) => rows.edge_at(y - HEADER_H, 3.0).map(Edge::Row),
            _ => None,
        })
    }

    /// Double-click: on a column border, autofit it; on a row border, back to the default
    /// height (rows hold one line); on a cell, edit it.
    fn double_click(&self, x: f64, y: f64) {
        match self.edge_at(x, y) {
            Some(Edge::Col(c)) => {
                for c in self.columns_with(c) {
                    self.autofit(c);
                }
                self.update_adjustments();
            }
            Some(Edge::Row(r)) => {
                self.imp().sizing.borrow_mut().rows.set(r, ROW_H);
                self.update_adjustments();
            }
            None => self.begin_edit(x, y),
        }
    }

    /// Column `c`, or every selected column when `c` is one of several selected whole
    /// columns: resizing one of them resizes them all, as in Calc.
    fn columns_with(&self, c: u32) -> std::ops::Range<u32> {
        match self.imp().selection.get().whole_columns() {
            Some(cols) if cols.contains(&c) => cols,
            _ => c..c + 1,
        }
    }

    /// Fit column `c` to the widest text in the rows on screen. Only those are measured,
    /// so this costs the same at any row count; an empty column gets the default width.
    fn autofit(&self, c: u32) {
        let imp = self.imp();
        let (body_w, _) = imp.body_size();
        let widest = self.with_viewports(|rows, _| {
            imp.cache.borrow().widest(c, rows.visible(0), |text| {
                f64::from(imp.layout(text).pixel_size().0)
            })
        });
        // A column title sits after the dimmed letter (see `snapshot`).
        let title = imp.table.borrow().as_ref().and_then(|t| {
            let title = t.header_cell(c)?;
            let mut letter = String::new();
            column_name(c, &mut letter);
            let w = |s: &str| f64::from(imp.layout(s).pixel_size().0);
            Some(w(&letter) + 6.0 + w(&title))
        });
        let width = match (widest, title) {
            (None, None) => COL_W,
            (a, b) => {
                let w = a.unwrap_or(0.0).max(b.unwrap_or(0.0));
                (w + 2.0 * PAD + 2.0).min(body_w.max(COL_W))
            }
        };
        imp.sizing.borrow_mut().cols.set(u64::from(c), width);
    }

    /// A resize drag moved to widget point (`x`, `y`).
    fn resize_to(&self, edge: Edge, from: f64, start: f64, x: f64, y: f64) {
        let mut sizing = self.imp().sizing.borrow_mut();
        match edge {
            Edge::Col(c) => {
                let width = (from + x - start).max(MIN_COL_W);
                for c in self.columns_with(c) {
                    sizing.cols.set(u64::from(c), width);
                }
            }
            Edge::Row(r) => sizing.rows.set(r, (from + y - start).max(MIN_ROW_H)),
        }
        drop(sizing);
        self.update_adjustments();
    }

    /// A click (GRID-4): a cell, a row or column header, or the corner (everything).
    /// Shift extends the selection; the cursor stays.
    fn mouse_down(&self, x: f64, y: f64, shift: bool) {
        let imp = self.imp();
        if let Some(edge) = self.edge_at(x, y) {
            let sizing = imp.sizing.borrow();
            let (from, start) = match edge {
                Edge::Col(c) => (sizing.cols.size(u64::from(c)), x),
                Edge::Row(r) => (sizing.rows.size(r), y),
            };
            imp.drag
                .set(Some((Drag::Resize { edge, from, start }, x, y)));
            return;
        }
        let b = self.bounds();
        if b.rows == 0 || b.cols == 0 {
            return;
        }
        let (cell, top_left) = self.cell_near(x, y);
        let mut sel = imp.selection.get();
        let kind = match (x < imp.row_header_w.get(), y < HEADER_H) {
            (true, true) => {
                sel.select_all(b);
                None
            }
            (true, false) => {
                sel.click_row(cell.row, shift, top_left.col, b);
                Some(Drag::Rows)
            }
            (false, true) => {
                sel.click_col(cell.col, shift, top_left.row, b);
                Some(Drag::Cols)
            }
            (false, false) if shift => {
                sel.extend_to(cell);
                Some(Drag::Cells)
            }
            (false, false) => {
                sel = Selection::at(cell);
                Some(Drag::Cells)
            }
        };
        imp.selection.set(sel);
        imp.drag.set(kind.map(|k| (k, x, y)));
        self.queue_draw();
    }

    /// The pointer moved during a drag: stretch the selection to it, and keep scrolling
    /// toward it while it is outside the body.
    fn drag_to(&self, x: f64, y: f64) {
        let imp = self.imp();
        let Some((kind, _, _)) = imp.drag.get() else {
            return;
        };
        imp.drag.set(Some((kind, x, y)));
        if let Drag::Resize { edge, from, start } = kind {
            self.resize_to(edge, from, start, x, y);
            return;
        }
        self.drag_step();
        if imp.autoscroll.borrow().is_none() {
            let id = glib::timeout_add_local(std::time::Duration::from_millis(40), {
                let g = self.downgrade();
                move || match g.upgrade() {
                    Some(g) if g.imp().drag.get().is_some() => {
                        g.drag_step();
                        glib::ControlFlow::Continue
                    }
                    Some(g) => {
                        g.imp().autoscroll.take();
                        glib::ControlFlow::Break
                    }
                    None => glib::ControlFlow::Break,
                }
            });
            imp.autoscroll.replace(Some(id));
        }
    }

    fn drag_step(&self) {
        let imp = self.imp();
        let Some((kind, x, y)) = imp.drag.get() else {
            return;
        };
        let (body_w, body_h) = imp.body_size();
        let rh_w = imp.row_header_w.get();
        // Past an edge: scroll that way, faster the further out the pointer is.
        let push = |adj: &gtk::Adjustment, before: f64, after: f64| {
            let over = if before < 0.0 { before } else { after.max(0.0) };
            if over != 0.0 {
                adj.set_value(adj.value() + over.signum() * (8.0 + over.abs().min(400.0)));
            }
        };
        if kind != Drag::Cols {
            if let Some(v) = imp.vadj.borrow().as_ref() {
                push(v, y - HEADER_H, y - HEADER_H - body_h);
            }
        }
        if kind != Drag::Rows {
            if let Some(hz) = imp.hadj.borrow().as_ref() {
                push(hz, x - rh_w, x - rh_w - body_w);
            }
        }
        let b = self.bounds();
        if b.rows == 0 || b.cols == 0 {
            return;
        }
        let (cell, _) = self.cell_near(x, y);
        let mut sel = imp.selection.get();
        match kind {
            Drag::Cells => sel.extend_to(cell),
            Drag::Rows => sel.click_row(cell.row, true, 0, b),
            Drag::Cols => sel.click_col(cell.col, true, 0, b),
            Drag::Resize { .. } => return,
        }
        if sel != imp.selection.get() {
            imp.selection.set(sel);
            self.queue_draw();
        }
    }

    /// Apply a navigation key (GRID-3/GRID-4) and keep the moved cell in view.
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
        self.scroll_to(sel.reveal());
        self.queue_draw();
    }

    /// Scroll the least amount that shows the given row and column whole.
    fn scroll_to(&self, (row, col): (Option<u64>, Option<u32>)) {
        let imp = self.imp();
        let (body_w, body_h) = imp.body_size();
        let reveal = |adj: &gtk::Adjustment, start: f64, size: f64, view: f64| {
            if start < adj.value() {
                adj.set_value(start);
            } else if start + size > adj.value() + view {
                adj.set_value(start + size - view);
            }
        };
        let sizing = imp.sizing.borrow();
        if let (Some(v), Some(row)) = (imp.vadj.borrow().as_ref(), row) {
            reveal(v, sizing.rows.start(row), sizing.rows.size(row), body_h);
        }
        if let (Some(hz), Some(col)) = (imp.hadj.borrow().as_ref(), col) {
            let col = u64::from(col);
            reveal(hz, sizing.cols.start(col), sizing.cols.size(col), body_w);
        }
    }

    /// Open the cell editor on the cell under widget point (`x`, `y`), prefilled with the
    /// cell's full value.
    fn begin_edit(&self, x: f64, y: f64) {
        let imp = self.imp();
        if imp.saving.get() {
            return;
        }
        let rh_w = imp.row_header_w.get();
        let Some((row, col, cell)) = self.with_viewports(|rows, cols| {
            let (row, col) = (rows.at(y - HEADER_H)?, cols.at(x - rh_w)?);
            let cell = gdk::Rectangle::new(
                (rh_w + cols.pos(col)) as i32,
                (HEADER_H + rows.pos(row)) as i32,
                cols.size(col) as i32,
                rows.size(row) as i32,
            );
            Some((row, col as u32, cell))
        }) else {
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
                    imp.titles.take();
                    self.queue_draw();
                }
            }
        }
        if let Some((popover, _)) = imp.editor.borrow().as_ref() {
            popover.popdown();
        }
    }

    /// Ctrl+Z (CMD-1): revert the last command and show the cell it changed. Refused while
    /// a save runs, like edits, so nothing changes between the save's snapshot and reopen.
    pub fn undo(&self) -> bool {
        self.step_history(true)
    }

    /// Ctrl+Shift+Z / Ctrl+Y: apply the last undone command again.
    pub fn redo(&self) -> bool {
        self.step_history(false)
    }

    fn step_history(&self, back: bool) -> bool {
        let imp = self.imp();
        if imp.saving.get() {
            return false;
        }
        let focus = {
            let (mut table, mut undo) = (imp.table.borrow_mut(), imp.undo.borrow_mut());
            let (Some(table), Some(undo)) = (table.as_mut(), undo.as_mut()) else {
                return false;
            };
            let cmd = if back {
                undo.undo(table)
            } else {
                undo.redo(table)
            };
            let Some(cmd) = cmd else { return false };
            cmd.focus().and_then(|at| {
                let row = table.row_of(at.row)?;
                Some(grid::Cell { row, col: at.col })
            })
        };
        imp.cache.borrow_mut().invalidate();
        imp.titles.take();
        if let Some(cell) = focus {
            imp.selection.set(Selection::at(cell));
            self.scroll_to((Some(cell.row), Some(cell.col)));
        }
        self.queue_draw();
        true
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

    /// The first row is shown as column titles rather than data (ENG-7).
    pub fn has_header(&self) -> bool {
        self.imp()
            .table
            .borrow()
            .as_ref()
            .is_some_and(|t| t.has_header())
    }

    /// Flip "first row is header". Rows shift by one, so cached rows and row heights (which
    /// belong to the rows they were set on) start over; edits stay with their rows.
    pub fn set_header(&self, on: bool) {
        let imp = self.imp();
        if let Some(t) = imp.table.borrow_mut().as_mut() {
            t.set_header(on);
        }
        imp.cache.borrow_mut().invalidate();
        imp.titles.take();
        imp.sizing.borrow_mut().rows.clear();
        self.update_adjustments();
    }

    /// Every column back to the default width.
    pub fn reset_column_widths(&self) {
        self.imp().sizing.borrow_mut().cols.clear();
    }

    /// Show a freshly opened table (the file just saved). Its edits are on disk now, so the
    /// undo history, which refers to the old overlay, starts over.
    pub fn replace_table(&self, table: CsvTable) {
        let imp = self.imp();
        imp.table.replace(Some(table));
        imp.undo.replace(Some(UndoStack::new(1_000)));
        imp.cache.borrow_mut().reset();
        imp.titles.take();
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

    /// Share of the file indexed so far (0..=1), or `None` once indexing is complete.
    pub fn index_progress(&self) -> Option<f64> {
        let table = self.imp().table.borrow();
        let t = table.as_ref().filter(|t| !t.is_complete())?;
        let index = t.index();
        let total = index.source().bytes().len().max(1) as f64;
        Some(index.bytes_indexed() as f64 / total)
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
        let sizing = imp.sizing.borrow();
        if let Some(v) = imp.vadj.borrow().as_ref() {
            let upper = sizing.rows.start(rows).max(body_h);
            v.configure(v.value(), 0.0, upper, ROW_H, body_h * 0.9, body_h);
        }
        if let Some(hz) = imp.hadj.borrow().as_ref() {
            let cols = u64::from(imp.cache.borrow().col_count().max(1));
            let upper = sizing.cols.start(cols).max(body_w);
            hz.configure(hz.value(), 0.0, upper, COL_W / 4.0, body_w * 0.9, body_w);
        }
        drop(sizing);
        self.queue_draw();
    }
}

/// What a mouse drag stretches the selection over (GRID-4): cells, or whole rows/columns
/// when it started on a header; or a row or column border being dragged to resize it
/// (GRID-5), from size `from` with the pointer at `start` along that axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Drag {
    Cells,
    Rows,
    Cols,
    Resize { edge: Edge, from: f64, start: f64 },
}

/// The far border of a column (in the column header) or a row (in the row numbers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Col(u32),
    Row(u64),
}

/// Map a GDK key press to a navigation key (GRID-3/GRID-4). `None` lets it through (Ctrl+S,
/// plain Space, and Ctrl+arrows, which are not built yet).
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
        K::space | K::KP_Space if mods.shift || mods.ctrl => Key::Space,
        _ => return None,
    };
    if mods.ctrl && !matches!(nav, Key::Home | Key::End | Key::Space) {
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
