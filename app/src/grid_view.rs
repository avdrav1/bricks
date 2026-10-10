//! The grid widget: draws the cells `grid::GridCache` holds with GSK + Pango (ADR 0001).

use crate::editor::{self, Action, ClipCommand, Mode, ShapeCommand, Start};
use crate::status::ClipView;

use commands::{Batch, ClearCells, Reshape, SetCell, UndoStack};
use csv_engine::RowIndex as _;
use data_model::{
    parse_tsv, CellRef, ColId, ColumnTypes, Compare, Copied, CsvTable, DelimiterChoice, Edit,
    Filter, FilterError, Hit, InferredType, Matches, PasteError, Query, ReplaceError, RereadError,
    RowId, RowSet, SaveJob, SaveJobError, SearchError, SearchProgress, SortError, SortOrder, Step,
    TableSource, Test, PASTE_MAX_CELLS,
};
use grid::{Bounds, GridCache, Key, Mods, Selection, Sizes, Viewport};
use gtk::{gdk, gio, glib, graphene, pango, prelude::*, subclass::prelude::*};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Default row height and column width; GRID-5 lets the user resize each one.
pub const ROW_H: f64 = 22.0;
pub const COL_W: f64 = 120.0;
pub const HEADER_H: f64 = 24.0;
/// The filter button (FILT-1) at the right end of each column header.
const FILTER_W: f64 = 16.0;
/// Rows loaded above and below the visible ones.
const BUFFER_ROWS: u64 = 100;
const PAD: f64 = 4.0;
/// Smallest sizes a drag can give a column or row.
const MIN_COL_W: f64 = 16.0;
const MIN_ROW_H: f64 = 8.0;
/// How long the status bar says what was copied or pasted.
const CLIP_NOTE: Duration = Duration::from_secs(4);

/// A copy running on the job pool (CLIP-1, ENG-8).
pub struct CopyJob {
    job: jobs::Job<Option<Copied>>,
    done: Arc<AtomicU64>,
    rows: u64,
}

/// A sort running on the job pool (SORT-1): its result, and file rows read so far.
pub struct SortJob {
    job: jobs::Job<Result<Option<Edit>, SortError>>,
    done: Arc<AtomicU64>,
    rows: u64,
}

/// A filter evaluated on the job pool (FILT-1): the filter, its result, and file rows
/// tested so far.
pub struct FilterJob {
    filters: Vec<Filter>,
    job: jobs::Job<Result<Vec<RowSet>, FilterError>>,
    done: Arc<AtomicU64>,
    rows: u64,
}

/// A search's matches, and the match it went to (if any).
pub type Searched = (Arc<Matches>, Option<Hit>);

/// A search on the job pool (SRCH-1, SRCH-2): the matches (counted again, or the ones
/// from before) and where it went, and its progress.
pub struct SearchJob {
    job: jobs::Job<Result<Searched, SearchError>>,
    progress: Arc<SearchProgress>,
    rows: u64,
    /// It counts the matches again (not a step through the ones from before).
    counting: bool,
}

/// How a find went (SRCH-1, SRCH-2), for the find bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    /// The cursor is on a match now: the `ordinal`th of `total`.
    Hit {
        ordinal: u64,
        total: u64,
    },
    Nothing,
    /// The file is still being indexed.
    NotReady,
    /// Cancelled or replaced by a newer search: nothing changed.
    Cancelled,
}

/// A replace all on the job pool (SRCH-3): the matching cells and the edit, and the
/// progress of counting them.
pub struct ReplaceJob {
    job: jobs::Job<Result<(u64, Option<Edit>), ReplaceError>>,
    progress: Arc<SearchProgress>,
    rows: u64,
}

/// How a replace all went (SRCH-3), for the find bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Replaced {
    /// This many cells changed, as one undo step.
    Cells(u64),
    Nothing,
    /// Refused: this many cells is over [`data_model::REPLACE_MAX_CELLS`].
    TooMany(u64),
    NotReady,
    Cancelled,
}

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
        /// Every edit goes through here as a command (invariant 5).
        pub undo: RefCell<Option<UndoStack<CsvTable>>>,
        /// The in-cell editor (EDIT-2): an entry laid over its cell, made on first use.
        pub editor: RefCell<Option<gtk::Entry>>,
        /// Cell the editor is open on, and how the edit started.
        pub editing: Cell<Option<(u64, u32, Mode)>>,
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
        /// Right-click menu: insert and delete rows and columns (EDIT-3/4).
        pub menu: RefCell<Option<gtk::PopoverMenu>>,
        /// Sizes the user set, by row and column identity, so they follow their rows and
        /// columns through inserts, deletes, undo, and header flips. `sizing` is laid out
        /// from these by position (`refit_sizes`).
        pub row_heights: RefCell<HashMap<RowId, f64>>,
        pub col_widths: RefCell<HashMap<ColId, f64>>,
        /// The copy running, if any; a new one cancels it.
        pub copying: RefCell<Option<CopyJob>>,
        /// The last copy or paste result, and when, for the status bar.
        pub clip_note: Cell<Option<(ClipView, Instant)>>,
        /// Tables shown so far: `replace_table` counts up, so the status tick can tell a
        /// reopened file (whose indexing it must follow) from the one before.
        pub generation: Cell<u64>,
        /// The type inference running (TYPE-1), if any; a newer one cancels it.
        pub inferring: RefCell<Option<jobs::Job<Option<ColumnTypes>>>>,
        /// The sort running (SORT-1), if any. Edits wait for it, as for a save.
        pub sorting: RefCell<Option<SortJob>>,
        /// The filter being evaluated (FILT-1), if any. Edits wait for it.
        pub filtering: RefCell<Option<FilterJob>>,
        /// The search running (SRCH-1), if any. Edits wait for it.
        pub searching: RefCell<Option<SearchJob>>,
        /// The last search's matches, while they still describe the table, and the match
        /// it went to (SRCH-2).
        pub found: RefCell<Option<Searched>>,
        /// The replace all running (SRCH-3), if any. Edits wait for it.
        pub replacing: RefCell<Option<ReplaceJob>>,
        /// Filters to apply again, by column position, once the table is indexed: those
        /// of the table a save replaced (FILT-3).
        pub pending_filters: RefCell<Vec<(u32, Test)>>,
        /// A column's filter settings, open under its header button.
        pub filter_popover: RefCell<Option<gtk::Popover>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GridView {
        const NAME: &'static str = "BricksGridView";
        type Type = super::GridView;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for GridView {
        fn dispose(&self) {
            if let Some(entry) = self.editor.take() {
                entry.unparent();
            }
            if let Some(menu) = self.menu.take() {
                menu.unparent();
            }
            if let Some(popover) = self.filter_popover.take() {
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
            let obj = self.obj();
            obj.update_adjustments();
            obj.place_editor();
            // GTK4: a widget that parents a popover must position it on every allocation.
            if let Some(menu) = self.menu.borrow().as_ref() {
                menu.present();
            }
            if let Some(popover) = self.filter_popover.borrow().as_ref() {
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
                count: shown_rows(table),
            };
            let visible = rows.visible(0);
            cache.ensure(visible.clone(), BUFFER_ROWS, table);
            self.painted_top
                .set(cache.cell(visible.start, 0).map(|_| visible.start));
            let cols = Viewport {
                scroll: hadj.value(),
                extent: body_w,
                sizes: &sizing.cols,
                count: u64::from(cache.col_count().max(table.min_width())) + 1,
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
                // The column's type (TYPE-2) sits right of the title, left of the filter
                // button; blue when the user set it.
                let ty = table.column_type(c as u32);
                let tag_w = ty.map_or(0.0, |t| self.layout(type_tag(t)).pixel_size().0 as f64);
                let tag_x = x + cw - FILTER_W - tag_w - PAD;
                // The title stops short of the type and the filter button.
                let title_w = (tag_x - x - PAD).max(0.0);
                snapshot.push_clip(&rect(x, 0.0, title_w, HEADER_H));
                column_name(c as u32, &mut buf);
                match titles.get(c as usize) {
                    Some(title) => {
                        let letter_w = text_in(&buf, x + PAD, 4.0, &dim);
                        text_at(title, x + PAD + letter_w + 6.0, 4.0);
                    }
                    None => text_at(&buf, x + PAD, 4.0),
                }
                snapshot.pop();
                let blue = gdk::RGBA::new(0.21, 0.52, 0.89, 1.0);
                if let Some(t) = ty {
                    // Narrow columns drop the tag before the button.
                    if tag_x > x + PAD {
                        let set = table.type_override(c as u32).is_some();
                        text_in(type_tag(t), tag_x, 4.0, if set { &blue } else { &dim });
                    }
                }
                let on = table.filter_on(c as u32).is_some();
                text_in("▾", x + cw - FILTER_W, 3.0, if on { &blue } else { &dim });
            }
            snapshot.pop();

            // The in-cell editor (EDIT-2) sits over its cell, inside the body.
            let editor = self.editor.borrow();
            if let Some(entry) = editor.as_ref().filter(|e| WidgetExt::is_visible(*e)) {
                snapshot.push_clip(&rect(rh_w, HEADER_H, body_w, body_h));
                obj.snapshot_child(entry, snapshot);
                snapshot.pop();
            }
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
        imp.undo.replace(Some(UndoStack::default()));
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
        // Right-click: the row menu, on the clicked cell (selected first if it wasn't).
        let context = gtk::GestureClick::builder().button(3).build();
        context.connect_pressed({
            let g = g.downgrade();
            move |_, _, x, y| {
                if let Some(g) = g.upgrade() {
                    g.grab_focus();
                    g.row_menu(x, y);
                }
            }
        });
        g.add_controller(context);
        let actions = gio::SimpleActionGroup::new();
        for (name, run) in [
            ("insert-above", Self::insert_rows_above as fn(&Self)),
            ("insert-below", Self::insert_rows_below),
            ("delete-rows", Self::delete_rows),
            ("insert-left", Self::insert_cols_left),
            ("insert-right", Self::insert_cols_right),
            ("delete-cols", Self::delete_cols),
            ("sort-asc", Self::sort_ascending),
            ("sort-desc", Self::sort_descending),
        ] {
            let action = gio::SimpleAction::new(name, None);
            action.connect_activate({
                let g = g.downgrade();
                move |_, _| {
                    if let Some(g) = g.upgrade() {
                        run(&g);
                    }
                }
            });
            actions.add_action(&action);
        }
        g.insert_action_group("grid", Some(&actions));
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
            move |_, key, _, state| {
                let Some(g) = g.upgrade() else {
                    return glib::Propagation::Proceed;
                };
                // Keys the open editor's entry didn't use bubble up here; they aren't ours.
                if g.imp().editing.get().is_some() {
                    return glib::Propagation::Proceed;
                }
                if let Some((key, mods)) = nav_key(key, state) {
                    g.press(key, mods);
                } else if let Some(cmd) = editor::clip_command(key, state) {
                    match cmd {
                        ClipCommand::Copy => g.copy_selection(false),
                        ClipCommand::Cut => g.copy_selection(true),
                        ClipCommand::Paste => g.paste_clipboard(),
                    }
                } else if let Some(cmd) = editor::shape_command(key, state) {
                    let columns = g.imp().selection.get().whole_columns().is_some();
                    match (cmd, columns) {
                        (ShapeCommand::Insert, false) => g.insert_rows_above(),
                        (ShapeCommand::Delete, false) => g.delete_rows(),
                        (ShapeCommand::Insert, true) => g.insert_cols_left(),
                        (ShapeCommand::Delete, true) => g.delete_cols(),
                    }
                } else if editor::clears(key, state) {
                    g.clear_selected();
                } else if let Some(start) = editor::start(key, state) {
                    g.start_edit(start);
                } else {
                    return glib::Propagation::Proceed;
                }
                glib::Propagation::Stop
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
                        if g.imp().editing.get().is_some() {
                            g.queue_allocate(); // the editor moves with its cell
                        }
                        g.queue_draw();
                    }
                }
            });
        }
        let css = gtk::CssProvider::new();
        css.load_from_string(
            "entry.cell-editor { min-height: 0; padding: 0 3px; margin: 0; border-radius: 0; }",
        );
        gtk::style_context_add_provider_for_display(
            &WidgetExt::display(&g),
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        g
    }

    /// What navigation moves within: the rows indexed so far, the widest row seen, and one
    /// screen of whole rows, plus the editable edge: one empty row (once indexing is done)
    /// and one empty column past the data, where typing grows the table (SAVE-2). Whole-row
    /// and whole-column selections follow it as it grows.
    fn bounds(&self) -> Bounds {
        let imp = self.imp();
        let (_, body_h) = imp.body_size();
        let b = Bounds {
            rows: self.shown_rows(),
            cols: self.col_count() + 1,
            page_rows: ((body_h / ROW_H).floor() as u64).max(1),
        };
        if let Some(old) = imp.bounds_seen.replace(Some(b)) {
            if (old.rows, old.cols) != (b.rows, b.cols) {
                let mut sel = imp.selection.get();
                sel.grow(old, b);
                imp.selection.set(sel);
            }
        }
        // A reopened file can have fewer rows than the cursor's (a header detected where
        // there was none); once its row count is final, bring the cursor back in.
        let cur = imp.selection.get().cursor();
        if b.rows > 0 && cur.row >= b.rows && self.is_complete() {
            imp.selection.set(Selection::at(grid::Cell {
                row: b.rows - 1,
                col: cur.col,
            }));
        }
        b
    }

    /// Columns with data: the widest row loaded, and never fewer than the header row's or
    /// than reach every filtered column (FILT-2: a filter hiding every row stays clearable).
    fn col_count(&self) -> u32 {
        let imp = self.imp();
        let min = imp.table.borrow().as_ref().map_or(0, |t| t.min_width());
        imp.cache.borrow().col_count().max(min)
    }

    /// Both axes of the body as they are scrolled and sized now: rows, then columns.
    fn with_viewports<R>(&self, f: impl FnOnce(Viewport<'_>, Viewport<'_>) -> R) -> R {
        let imp = self.imp();
        let (body_w, body_h) = imp.body_size();
        let value = |adj: &RefCell<Option<gtk::Adjustment>>| {
            adj.borrow().as_ref().map_or(0.0, |a| a.value())
        };
        let (row_count, col_count) = (self.shown_rows(), self.col_count() + 1);
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
                self.set_row_height(r, ROW_H);
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
        self.set_col_width(c, width);
    }

    /// A resize drag moved to widget point (`x`, `y`).
    fn resize_to(&self, edge: Edge, from: f64, start: f64, x: f64, y: f64) {
        match edge {
            Edge::Col(c) => {
                let width = (from + x - start).max(MIN_COL_W);
                for c in self.columns_with(c) {
                    self.set_col_width(c, width);
                }
            }
            Edge::Row(r) => self.set_row_height(r, (from + y - start).max(MIN_ROW_H)),
        }
        self.update_adjustments();
    }

    /// Give column `col` a width, remembered by its identity.
    fn set_col_width(&self, col: u32, width: f64) {
        let imp = self.imp();
        let id = imp.table.borrow().as_ref().map(|t| t.col_id(col));
        if let Some(id) = id {
            imp.col_widths.borrow_mut().insert(id, width);
        }
        imp.sizing.borrow_mut().cols.set(u64::from(col), width);
    }

    /// Give row `row` a height, remembered by its identity.
    fn set_row_height(&self, row: u64, height: f64) {
        let imp = self.imp();
        let id = imp
            .table
            .borrow()
            .as_ref()
            .filter(|t| row < t.row_count())
            .map(|t| t.row_id(row));
        if let Some(id) = id {
            imp.row_heights.borrow_mut().insert(id, height);
        }
        imp.sizing.borrow_mut().rows.set(row, height);
    }

    /// Lay the remembered sizes out by where their rows and columns are now; those not
    /// shown (deleted) are kept for an undo.
    fn refit_sizes(&self) {
        let imp = self.imp();
        let table = imp.table.borrow();
        let Some(table) = table.as_ref() else { return };
        let mut sizing = imp.sizing.borrow_mut();
        sizing.rows.clear();
        sizing.cols.clear();
        for (&id, &h) in imp.row_heights.borrow().iter() {
            if let Some(r) = table.row_of(id) {
                sizing.rows.set(r, h);
            }
        }
        for (&id, &w) in imp.col_widths.borrow().iter() {
            if let Some(c) = table.col_of(id) {
                sizing.cols.set(u64::from(c), w);
            }
        }
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
        // The filter button at the right of a column header (FILT-1).
        if y < HEADER_H && x >= imp.row_header_w.get() {
            let rh_w = imp.row_header_w.get();
            let button = self.with_viewports(|_, cols| {
                let c = cols.at(x - rh_w)?;
                let right = rh_w + cols.pos(c) + cols.size(c);
                (x >= right - FILTER_W && x < right - 4.0).then_some((c as u32, right))
            });
            if let Some((col, right)) = button {
                self.filter_menu(col, right - FILTER_W / 2.0);
                return;
            }
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
        // End and Ctrl+End go to the last cell with data, as in Calc, not the empty edge.
        let within = match key {
            Key::End => {
                let (rows, cols) = (self.row_count(), self.col_count());
                Bounds {
                    rows: if rows > 0 { rows } else { bounds.rows },
                    cols: if cols > 0 { cols } else { bounds.cols },
                    ..bounds
                }
            }
            _ => bounds,
        };
        let mut sel = imp.selection.get();
        sel.press(key, mods, within);
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

    /// Double-click: edit the cell under widget point (`x`, `y`), keeping its value.
    fn begin_edit(&self, x: f64, y: f64) {
        let rh_w = self.imp().row_header_w.get();
        let Some(cell) = self.with_viewports(|rows, cols| {
            let (row, col) = (rows.at(y - HEADER_H)?, cols.at(x - rh_w)?);
            Some(grid::Cell {
                row,
                col: col as u32,
            })
        }) else {
            return;
        };
        self.imp().selection.set(Selection::at(cell));
        let value = self.cell_text(cell);
        self.open_editor(cell, &value, Mode::Editing);
    }

    /// F2 or typing on the cursor cell (EDIT-2). The selection stays, so Enter and Tab
    /// after the edit move within a selected range, as in Calc.
    fn start_edit(&self, start: Start) {
        let cell = self.imp().selection.get().cursor();
        let b = self.bounds();
        if cell.row >= b.rows || cell.col >= b.cols {
            return;
        }
        match start {
            Start::Keep => self.open_editor(cell, &self.cell_text(cell), Mode::Editing),
            Start::Replace(c) => self.open_editor(cell, &c.to_string(), Mode::Typing),
        }
    }

    fn cell_text(&self, cell: grid::Cell) -> String {
        self.imp()
            .table
            .borrow()
            .as_ref()
            .and_then(|t| t.cell_value(cell.row, cell.col).map(|v| v.into_owned()))
            .unwrap_or_default()
    }

    fn open_editor(&self, cell: grid::Cell, text: &str, mode: Mode) {
        let imp = self.imp();
        if self.busy() {
            return;
        }
        let entry = imp
            .editor
            .borrow_mut()
            .get_or_insert_with(|| self.build_editor())
            .clone();
        imp.editing.set(Some((cell.row, cell.col, mode)));
        self.scroll_to((Some(cell.row), Some(cell.col)));
        entry.set_text(text);
        entry.set_visible(true);
        entry.grab_focus();
        entry.set_position(-1);
        self.queue_allocate();
        self.queue_draw();
    }

    /// Lay the editor over its cell (at least as big as the entry needs).
    fn place_editor(&self) {
        let imp = self.imp();
        let (Some(entry), Some((row, col, _))) = (imp.editor.borrow().clone(), imp.editing.get())
        else {
            return;
        };
        let rh_w = imp.row_header_w.get();
        let (x, y, w, h) = self.with_viewports(|rows, cols| {
            let col = u64::from(col);
            (
                rh_w + cols.pos(col),
                HEADER_H + rows.pos(row),
                cols.size(col),
                rows.size(row),
            )
        });
        let (min_w, _, _, _) = entry.measure(gtk::Orientation::Horizontal, -1);
        let (min_h, _, _, _) = entry.measure(gtk::Orientation::Vertical, -1);
        // Off-screen cells park the editor just outside; the clip in `snapshot` hides it.
        let clamp = |v: f64, lo: f64, hi: f64| v.clamp(lo, hi.max(lo)) as i32;
        entry.size_allocate(
            &gtk::Allocation::new(
                clamp(x, -w - 10.0, f64::from(self.width()) + 10.0),
                clamp(y, -h - 10.0, f64::from(self.height()) + 10.0),
                (w as i32).max(min_w),
                (h as i32).max(min_h),
            ),
            -1,
        );
    }

    fn build_editor(&self) -> gtk::Entry {
        let entry = gtk::Entry::new();
        entry.add_css_class("cell-editor");
        entry.set_visible(false);
        entry.set_parent(self);
        // Capture phase: Enter, Tab and Esc are ours before the entry acts on them.
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed({
            let g = self.downgrade();
            move |_, key, _, state| {
                let Some(g) = g.upgrade() else {
                    return glib::Propagation::Proceed;
                };
                let Some((_, _, mode)) = g.imp().editing.get() else {
                    return glib::Propagation::Proceed;
                };
                match editor::while_editing(key, state, mode) {
                    Action::Commit(key, mods) => {
                        g.commit_edit();
                        g.press(key, mods);
                    }
                    Action::Cancel => g.close_editor(),
                    Action::Pass => return glib::Propagation::Proceed,
                }
                glib::Propagation::Stop
            }
        });
        entry.add_controller(keys);
        // Clicking elsewhere (another cell, the header bar) keeps what was typed.
        let focus = gtk::EventControllerFocus::new();
        focus.connect_leave({
            let g = self.downgrade();
            move |_| {
                if let Some(g) = g.upgrade() {
                    g.commit_edit();
                }
            }
        });
        entry.add_controller(focus);
        entry
    }

    /// Commit an open edit, if any; for Ctrl+S, which saves what was typed (as Calc does).
    pub fn finish_editing(&self) {
        self.commit_edit();
    }

    /// Apply the editor's text to its cell as an undoable command, then close the editor.
    fn commit_edit(&self) {
        let imp = self.imp();
        let Some((row, col, _)) = imp.editing.get() else {
            return;
        };
        let text = imp
            .editor
            .borrow()
            .as_ref()
            .map(|e| e.text().to_string())
            .unwrap_or_default();
        let text_empty = text.is_empty();
        let mut grew = false;
        {
            let mut table = imp.table.borrow_mut();
            if let (Some(table), Some(undo)) = (table.as_mut(), imp.undo.borrow_mut().as_mut()) {
                if row >= table.row_count() {
                    // The edge row (SAVE-2): add the row and set the cell, as one step.
                    match table.paste(row, col, &[vec![text]]) {
                        Ok(edits) if !edits.is_empty() => {
                            let at = CellRef {
                                row: table.row_id(row.min(table.row_count().saturating_sub(1))),
                                col: table.col_id(col),
                            };
                            undo.execute(Box::new(Batch::new("Edit cell", edits, at)), table);
                            grew = true;
                        }
                        _ => {}
                    }
                } else if table.cell_value(row, col).as_deref() != Some(text.as_str()) {
                    let at = CellRef {
                        row: table.row_id(row),
                        col: table.col_id(col),
                    };
                    undo.execute(Box::new(SetCell::new(at, text)), table);
                }
            }
        }
        let mut cache = imp.cache.borrow_mut();
        cache.invalidate();
        if !text_empty {
            // The edge column is a column now; Tab can move past it before rows reload.
            cache.widen(col + 1);
        }
        drop(cache);
        imp.titles.take();
        if grew {
            self.update_adjustments();
        }
        self.close_editor();
    }

    /// Close the editor without touching the cell (Esc), and give the grid the focus back.
    fn close_editor(&self) {
        let imp = self.imp();
        // Cleared first: taking the focus away runs the entry's focus-leave commit.
        if imp.editing.take().is_none() {
            return;
        }
        if let Some(entry) = imp.editor.borrow().as_ref() {
            entry.set_visible(false);
        }
        self.grab_focus();
        self.queue_draw();
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
        if self.busy() {
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
                let col = table.col_of(at.col)?;
                Some(grid::Cell { row, col })
            })
        };
        self.after_change(focus);
        true
    }

    /// Redraw after the data changed: cached rows and titles reload (from scratch: a
    /// column delete can narrow the widest row), the remembered sizes follow their rows
    /// and columns, the scroll range follows the row count, and the cursor goes to
    /// `focus` if given (else stays, clamped to the rows left).
    fn after_change(&self, focus: Option<grid::Cell>) {
        let imp = self.imp();
        imp.cache.borrow_mut().reset();
        imp.titles.take();
        self.refit_sizes();
        self.update_adjustments();
        let b = self.bounds();
        let cursor = focus.unwrap_or_else(|| imp.selection.get().cursor());
        if focus.is_some() || cursor.row >= b.rows {
            let cell = grid::Cell {
                row: cursor.row.min(b.rows.saturating_sub(1)),
                col: cursor.col,
            };
            imp.selection.set(Selection::at(cell));
            self.scroll_to((Some(cell.row), Some(cell.col)));
        }
        self.queue_draw();
    }

    /// Ctrl++ (EDIT-3): as many empty rows as the selection spans, above it.
    pub fn insert_rows_above(&self) {
        self.insert_rows(false);
    }

    pub fn insert_rows_below(&self) {
        self.insert_rows(true);
    }

    fn insert_rows(&self, below: bool) {
        let Some((first, count)) = self.selected_rows() else {
            return;
        };
        let at = if below { first + count } else { first };
        let edit = self
            .imp()
            .table
            .borrow_mut()
            .as_mut()
            .and_then(|t| t.insert_rows(at.min(t.row_count()), count));
        self.reshape(edit);
    }

    /// Ctrl+- (EDIT-3): delete the rows the selection spans.
    pub fn delete_rows(&self) {
        let Some((first, count)) = self.selected_rows() else {
            return;
        };
        let edit = self
            .imp()
            .table
            .borrow()
            .as_ref()
            .and_then(|t| t.delete_rows(first, count.min(t.row_count().saturating_sub(first))));
        self.reshape(edit);
    }

    /// The rows the selection spans (first, count). `None` for whole columns: Ctrl+-
    /// there deletes the columns.
    fn selected_rows(&self) -> Option<(u64, u64)> {
        let sel = self.imp().selection.get();
        if sel.whole_columns().is_some() {
            return None;
        }
        let (tl, br) = sel.range();
        Some((tl.row, br.row - tl.row + 1))
    }

    /// Ctrl++ on whole columns (EDIT-4): as many empty columns as are selected, left of
    /// them.
    pub fn insert_cols_left(&self) {
        self.insert_cols(false);
    }

    pub fn insert_cols_right(&self) {
        self.insert_cols(true);
    }

    fn insert_cols(&self, right: bool) {
        let (first, count) = self.selected_cols();
        let at = if right { first + count } else { first };
        let edit = self
            .imp()
            .table
            .borrow_mut()
            .as_mut()
            .and_then(|t| t.insert_cols(at, count));
        self.reshape(edit);
    }

    /// Ctrl+- on whole columns (EDIT-4): delete the selected columns.
    pub fn delete_cols(&self) {
        let (first, count) = self.selected_cols();
        // Not the empty edge column: there is nothing there to delete.
        let count = count.min(self.col_count().saturating_sub(first));
        let edit = self
            .imp()
            .table
            .borrow()
            .as_ref()
            .and_then(|t| t.delete_cols(first, count));
        self.reshape(edit);
    }

    /// The columns the selection spans (first, count).
    fn selected_cols(&self) -> (u32, u32) {
        let (tl, br) = self.imp().selection.get().range();
        (tl.col, br.col - tl.col + 1)
    }

    /// Run a row or column insert or delete as an undoable command. `None` (still
    /// indexing, or no such rows) and a running save or open edit leave everything as
    /// it is.
    fn reshape(&self, edit: Option<Edit>) {
        let imp = self.imp();
        let Some(edit) = edit else { return };
        if self.busy() || imp.editing.get().is_some() {
            return;
        }
        {
            let (mut table, mut undo) = (imp.table.borrow_mut(), imp.undo.borrow_mut());
            let (Some(table), Some(undo)) = (table.as_mut(), undo.as_mut()) else {
                return;
            };
            let cur = imp.selection.get().cursor();
            let cursor = CellRef {
                row: if cur.row < table.row_count() {
                    table.row_id(cur.row)
                } else {
                    RowId::source(0)
                },
                col: table.col_id(cur.col),
            };
            undo.execute(Box::new(Reshape::new(edit, cursor)), table);
        }
        self.after_change(None);
    }

    /// Delete (EDIT-5): empty the selected cells as one undoable step, however many there
    /// are. Whole rows reach every column, wider rows' included; whole columns wait for
    /// indexing to finish so they reach every row.
    pub fn clear_selected(&self) {
        let imp = self.imp();
        if self.busy() || imp.editing.get().is_some() {
            return;
        }
        let sel = imp.selection.get();
        let (tl, br) = sel.range();
        {
            let (mut table, mut undo) = (imp.table.borrow_mut(), imp.undo.borrow_mut());
            let (Some(table), Some(undo)) = (table.as_mut(), undo.as_mut()) else {
                return;
            };
            let rows = match sel.whole_columns() {
                Some(_) if !table.is_complete() => return,
                Some(_) => tl.row..table.row_count(),
                None => tl.row..(br.row + 1).min(table.row_count()),
            };
            let edit = if sel.is_whole_rows() {
                table.clear_cells(rows, tl.col..)
            } else {
                table.clear_cells(rows, tl.col..br.col + 1)
            };
            let Some(edit) = edit else { return };
            let cur = sel.cursor();
            let cursor = CellRef {
                row: table.row_id(cur.row),
                col: table.col_id(cur.col),
            };
            undo.execute(Box::new(ClearCells::new(edit, cursor)), table);
        }
        imp.cache.borrow_mut().invalidate();
        imp.titles.take();
        self.queue_draw();
    }

    /// Ctrl+C / Ctrl+X (CLIP-1): put the selection on the clipboard as TSV and an HTML
    /// table. The text is made on a worker from a snapshot of the table, so a big copy
    /// doesn't stop the window and later edits don't change it; the status bar shows its
    /// progress, and a new copy cancels it. A cut then empties the cells at once, as
    /// Calc does, as one undoable step. Whole columns wait for indexing to finish.
    pub fn copy_selection(&self, cut: bool) {
        let imp = self.imp();
        if imp.editing.get().is_some() || (cut && self.busy()) {
            return;
        }
        let sel = imp.selection.get();
        let (tl, br) = sel.range();
        let (mut snapshot, rows) = {
            let table = imp.table.borrow();
            let Some(table) = table.as_ref() else { return };
            let rows = match sel.whole_columns() {
                Some(_) if !table.is_complete() => return,
                Some(_) => tl.row..table.row_count(),
                None => tl.row..(br.row + 1).min(table.row_count()),
            };
            if rows.is_empty() {
                return;
            }
            (table.snapshot(), rows)
        };
        let done = Arc::new(AtomicU64::new(0));
        let whole_rows = sel.is_whole_rows();
        let count = rows.end - rows.start;
        let job = jobs::spawn({
            let done = done.clone();
            move |cancel| {
                if whole_rows {
                    snapshot.copy_range(rows, tl.col.., cancel.flag(), &done)
                } else {
                    snapshot.copy_range(rows, tl.col..br.col + 1, cancel.flag(), &done)
                }
            }
        });
        let running = imp.copying.replace(Some(CopyJob {
            job: job.clone(),
            done,
            rows: count,
        }));
        if let Some(old) = running {
            old.job.cancel();
        }
        let g = self.downgrade();
        glib::spawn_future_local(async move {
            let copied = job.finished().await.ok().flatten();
            let Some(g) = g.upgrade() else { return };
            let imp = g.imp();
            // Only the latest copy reaches the clipboard.
            let current = matches!(&*imp.copying.borrow(), Some(c) if c.job.is(&job));
            if !current {
                return;
            }
            imp.copying.take();
            let Some(copied) = copied else { return };
            let mut flavors = Vec::new();
            let text_only = copied.html.is_none();
            if let Some(html) = copied.html {
                let bytes = glib::Bytes::from_owned(html.into_bytes());
                flavors.push(gdk::ContentProvider::for_bytes("text/html", &bytes));
            }
            // UTF-8 bytes under both names: GTK's own `text/plain` (no charset) turns
            // non-ASCII into `\C3\AB` escapes, and many programs ask for that one.
            let text = glib::Bytes::from_owned(copied.tsv.into_bytes());
            for mime in ["text/plain;charset=utf-8", "text/plain"] {
                flavors.push(gdk::ContentProvider::for_bytes(mime, &text));
            }
            let provider = gdk::ContentProvider::new_union(&flavors);
            if g.clipboard().set_content(Some(&provider)).is_ok() {
                let note = ClipView::Copied {
                    cells: copied.cells,
                    text_only,
                };
                imp.clip_note.set(Some((note, Instant::now())));
            }
        });
        if cut {
            self.clear_selected();
        }
    }

    /// Ctrl+V / Shift+Insert (CLIP-2): paste the clipboard's text, as TSV, with its
    /// first cell at the selection's top-left, as one undoable step. Rows are added at
    /// the end as needed; the pasted range ends up selected, as in Calc.
    pub fn paste_clipboard(&self) {
        let imp = self.imp();
        if self.busy() || imp.editing.get().is_some() {
            return;
        }
        let g = self.downgrade();
        let read = self.clipboard().read_text_future();
        glib::spawn_future_local(async move {
            let Ok(Some(text)) = read.await else { return };
            if let Some(g) = g.upgrade() {
                g.paste_text(&text);
            }
        });
    }

    /// Paste `text` (TSV) with its top-left at `cell`, as one undoable step (the save
    /// benchmark's edits, SAVE-3).
    pub fn paste_at(&self, cell: grid::Cell, text: &str) {
        self.imp().selection.set(Selection::at(cell));
        self.paste_text(text);
    }

    fn paste_text(&self, text: &str) {
        let imp = self.imp();
        if self.busy() || imp.editing.get().is_some() {
            return;
        }
        let cells = parse_tsv(text);
        let width = cells.iter().map(Vec::len).max().unwrap_or(0) as u32;
        if width == 0 {
            return;
        }
        let (tl, _) = imp.selection.get().range();
        let note = {
            let (mut table, mut undo) = (imp.table.borrow_mut(), imp.undo.borrow_mut());
            let (Some(table), Some(undo)) = (table.as_mut(), undo.as_mut()) else {
                return;
            };
            let at = tl.row.min(table.row_count());
            match table.paste(at, tl.col, &cells) {
                Ok(edits) => {
                    if !edits.is_empty() {
                        let focus = CellRef {
                            row: table.row_id(at.min(table.row_count().saturating_sub(1))),
                            col: table.col_id(tl.col),
                        };
                        undo.execute(Box::new(Batch::new("Paste", edits, focus)), table);
                    }
                    ClipView::Pasted(cells.iter().map(|r| r.len() as u64).sum())
                }
                Err(PasteError::TooLarge(n)) => ClipView::PasteTooLarge {
                    cells: n,
                    max: PASTE_MAX_CELLS,
                },
                Err(PasteError::NotIndexed) => ClipView::PasteNeedsIndex,
            }
        };
        imp.clip_note.set(Some((note, Instant::now())));
        if !matches!(note, ClipView::Pasted(_)) {
            return;
        }
        let anchor = grid::Cell {
            row: tl.row.min(self.row_count().saturating_sub(1)),
            col: tl.col,
        };
        self.after_change(Some(anchor));
        let mut sel = Selection::at(anchor);
        sel.extend_to(grid::Cell {
            row: anchor.row + cells.len() as u64 - 1,
            col: anchor.col + width - 1,
        });
        imp.selection.set(sel);
        self.queue_draw();
    }

    /// The last copy or paste, as the status bar shows it.
    pub fn clip_view(&self) -> ClipView {
        let imp = self.imp();
        if let Some(job) = imp.copying.borrow().as_ref() {
            return ClipView::Copying(
                job.done.load(Ordering::Relaxed) as f64 / job.rows.max(1) as f64,
            );
        }
        match imp.clip_note.get() {
            Some((note, at)) if at.elapsed() < CLIP_NOTE => note,
            _ => ClipView::Idle,
        }
    }

    /// Open the row and column menu at widget point (`x`, `y`), selecting the cell
    /// there first unless it is already in the selection.
    fn row_menu(&self, x: f64, y: f64) {
        let imp = self.imp();
        let b = self.bounds();
        if b.rows > 0 && b.cols > 0 {
            let (cell, _) = self.cell_near(x, y);
            let (tl, br) = imp.selection.get().range();
            let inside =
                (tl.row..=br.row).contains(&cell.row) && (tl.col..=br.col).contains(&cell.col);
            if !inside {
                self.mouse_down(x, y, false);
                imp.drag.set(None);
            }
        }
        let menu = imp
            .menu
            .borrow_mut()
            .get_or_insert_with(|| {
                let rows = gio::Menu::new();
                rows.append(Some("Insert Rows Above"), Some("grid.insert-above"));
                rows.append(Some("Insert Rows Below"), Some("grid.insert-below"));
                rows.append(Some("Delete Rows"), Some("grid.delete-rows"));
                let cols = gio::Menu::new();
                cols.append(Some("Insert Columns Left"), Some("grid.insert-left"));
                cols.append(Some("Insert Columns Right"), Some("grid.insert-right"));
                cols.append(Some("Delete Columns"), Some("grid.delete-cols"));
                let sort = gio::Menu::new();
                sort.append(Some("Sort Ascending"), Some("grid.sort-asc"));
                sort.append(Some("Sort Descending"), Some("grid.sort-desc"));
                let model = gio::Menu::new();
                model.append_section(None, &rows);
                model.append_section(None, &cols);
                model.append_section(None, &sort);
                let menu = gtk::PopoverMenu::from_model(Some(&model));
                menu.set_has_arrow(false);
                menu.set_halign(gtk::Align::Start);
                menu.set_parent(self);
                menu
            })
            .clone();
        menu.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        menu.popup();
    }

    fn sort_ascending(&self) {
        self.sort(SortOrder::Ascending);
    }

    fn sort_descending(&self) {
        self.sort(SortOrder::Descending);
    }

    /// Sort every row by the cursor's column (SORT-1), on the job pool from a snapshot;
    /// the result applies as one undoable step. Waits for indexing; edits wait for it,
    /// and Esc cancels it ([`Self::cancel_sort`]).
    pub fn sort(&self, order: SortOrder) {
        let imp = self.imp();
        if self.busy() || imp.editing.get().is_some() {
            return;
        }
        let col = imp.selection.get().cursor().col;
        let (snapshot, rows) = match imp.table.borrow().as_ref() {
            Some(t) if t.is_complete() => (t.snapshot(), t.index().row_count()),
            _ => return,
        };
        let done = Arc::new(AtomicU64::new(0));
        let started = Instant::now();
        let job = jobs::spawn({
            let done = done.clone();
            move |cancel| snapshot.sort_rows(col, order, cancel.flag(), &done)
        });
        imp.sorting.replace(Some(SortJob {
            job: job.clone(),
            done,
            rows,
        }));
        let g = self.downgrade();
        glib::spawn_future_local(async move {
            let result = job.finished().await;
            let Some(g) = g.upgrade() else { return };
            let imp = g.imp();
            let current = matches!(&*imp.sorting.borrow(), Some(s) if s.job.is(&job));
            if std::env::var_os("BRICKS_TIMINGS").is_some() {
                eprintln!(
                    "sort {order:?} by column {col}: {:?} in {:?}{}",
                    result.as_ref().map(|r| r.as_ref().map(Option::is_some)),
                    started.elapsed(),
                    if current { "" } else { " (dropped)" }
                );
            }
            // A new table or a header flip since: the result is for rows as they were.
            if !current {
                return;
            }
            imp.sorting.take();
            let Ok(Ok(Some(edit))) = result else { return };
            {
                let (mut table, mut undo) = (imp.table.borrow_mut(), imp.undo.borrow_mut());
                let (Some(table), Some(undo)) = (table.as_mut(), undo.as_mut()) else {
                    return;
                };
                let focus = CellRef {
                    row: table.row_id(0),
                    col: table.col_id(col),
                };
                undo.execute(Box::new(Batch::new("Sort", vec![edit], focus)), table);
            }
            g.after_change(None);
        });
    }

    /// Esc: stop a running sort, filter, search, or replace; the rows stay as they were.
    /// Whether one was running.
    pub fn cancel_job(&self) -> bool {
        let imp = self.imp();
        let mut any = false;
        if let Some(s) = imp.sorting.borrow().as_ref() {
            s.job.cancel();
            any = true;
        }
        if let Some(f) = imp.filtering.borrow().as_ref() {
            f.job.cancel();
            any = true;
        }
        if let Some(s) = imp.searching.borrow().as_ref() {
            s.job.cancel();
            any = true;
        }
        if let Some(r) = imp.replacing.borrow().as_ref() {
            r.job.cancel();
            any = true;
        }
        any
    }

    /// A running sort, filter, search, or replace, and the share of the file it has read
    /// (0..=1), for the status bar.
    pub fn job_progress(&self) -> Option<(&'static str, f64)> {
        let imp = self.imp();
        let share =
            |done: &AtomicU64, rows: u64| done.load(Ordering::Relaxed) as f64 / rows.max(1) as f64;
        if let Some(s) = imp.sorting.borrow().as_ref() {
            return Some(("Sorting", share(&s.done, s.rows)));
        }
        if let Some(f) = imp.filtering.borrow().as_ref() {
            return Some(("Filtering", share(&f.done, f.rows)));
        }
        if let Some(r) = imp.replacing.borrow().as_ref() {
            return Some(("Replacing", share(&r.progress.rows, r.rows)));
        }
        let searching = imp.searching.borrow();
        let s = searching.as_ref()?;
        Some(("Searching", share(&s.progress.rows, s.rows)))
    }

    /// A save, sort, filter, search, or replace is running: the table must not change
    /// under it.
    pub fn busy(&self) -> bool {
        let imp = self.imp();
        imp.saving.get()
            || imp.sorting.borrow().is_some()
            || imp.filtering.borrow().is_some()
            || imp.searching.borrow().is_some()
            || imp.replacing.borrow().is_some()
    }

    /// Something a search must not run beside: an open editor, or a save, sort, filter,
    /// or replace (a newer search just replaces a running one).
    fn search_blocked(&self) -> bool {
        let imp = self.imp();
        imp.editing.get().is_some()
            || imp.saving.get()
            || imp.sorting.borrow().is_some()
            || imp.filtering.borrow().is_some()
            || imp.replacing.borrow().is_some()
    }

    /// Find (SRCH-1, SRCH-2): the match `step` leads to from the cursor, for `text` in
    /// every column or the cursor's (`column_only`), row by row and wrapping, on the job
    /// pool. The match becomes the cursor; `done` hears how it went. Matches are counted
    /// once per query and kept until the table changes, so stepping through them only
    /// walks the rows. A new search replaces a running one; Esc cancels.
    pub fn find(
        &self,
        text: &str,
        match_case: bool,
        column_only: bool,
        step: Step,
        done: impl FnOnce(Found) + 'static,
    ) {
        let imp = self.imp();
        if self.search_blocked() {
            return;
        }
        let cur = imp.selection.get().cursor();
        let Some((query, rows)) = self.query(text, match_case, column_only) else {
            return done(Found::NotReady);
        };
        let known = self.known_matches(&query).map(|(m, hit)| {
            let at_hit = hit.filter(|h| (h.row, h.col) == (cur.row, cur.col));
            (m, at_hit)
        });
        let Some(mut snapshot) = imp.table.borrow().as_ref().map(CsvTable::snapshot) else {
            return;
        };
        self.cancel_search();
        let progress = Arc::new(SearchProgress::default());
        let counting = known.is_none();
        let started = Instant::now();
        let job = jobs::spawn({
            let progress = progress.clone();
            move |cancel| {
                let (matches, hint) = match known {
                    Some(k) => k,
                    None => (
                        Arc::new(snapshot.search(&query, cancel.flag(), &progress)?),
                        None,
                    ),
                };
                let from = (cur.row, cur.col);
                let hit = matches.step(&mut snapshot, from, step, hint, cancel.flag())?;
                Ok((matches, hit))
            }
        });
        imp.searching.replace(Some(SearchJob {
            job: job.clone(),
            progress,
            rows,
            counting,
        }));
        let g = self.downgrade();
        glib::spawn_future_local(async move {
            let result = job.finished().await;
            let Some(g) = g.upgrade() else { return };
            let imp = g.imp();
            let current = matches!(&*imp.searching.borrow(), Some(s) if s.job.is(&job));
            if current {
                imp.searching.take();
            }
            if std::env::var_os("BRICKS_TIMINGS").is_some() {
                let r = result
                    .as_ref()
                    .map(|r| r.as_ref().map(|(m, hit)| (m.total(), hit)));
                eprintln!("find: {r:?} in {:?}", started.elapsed());
            }
            let (matches, hit) = match result {
                Ok(Ok(r)) if current => r,
                _ => return done(Found::Cancelled),
            };
            let total = matches.total();
            imp.found.replace(Some((matches, hit)));
            match hit {
                Some(h) => {
                    let cell = grid::Cell {
                        row: h.row,
                        col: h.col,
                    };
                    imp.selection.set(Selection::at(cell));
                    g.scroll_to((Some(h.row), Some(h.col)));
                    g.queue_draw();
                    done(Found::Hit {
                        ordinal: h.ordinal,
                        total,
                    });
                }
                None => done(Found::Nothing),
            }
        });
    }

    /// Stop the running search, if any (a new keystroke in the find bar, SRCH-2); whether
    /// one was running. Its `done` hears [`Found::Cancelled`].
    pub fn cancel_search(&self) -> bool {
        let old = self.imp().searching.take();
        if let Some(s) = &old {
            s.job.cancel();
        }
        old.is_some()
    }

    /// The query for `text` as the find bar sets it, at the cursor.
    fn query(&self, text: &str, match_case: bool, column_only: bool) -> Option<(Query, u64)> {
        let imp = self.imp();
        let cur = imp.selection.get().cursor();
        let table = imp.table.borrow();
        let t = table.as_ref().filter(|t| t.is_complete())?;
        let query = Query {
            text: text.to_owned(),
            match_case,
            col: column_only.then(|| t.col_id(cur.col)),
        };
        Some((query, t.index().row_count()))
    }

    /// The last search's matches, if they are for `query` and still describe the table.
    fn known_matches(&self, query: &Query) -> Option<Searched> {
        let imp = self.imp();
        let table = imp.table.borrow();
        let t = table.as_ref()?;
        imp.found
            .borrow()
            .as_ref()
            .filter(|(m, _)| m.query() == query && m.is_current(t))
            .cloned()
    }

    /// Replace (SRCH-3): if the cursor is on the match the last find went to, replace the
    /// text in that cell (one undo step) and go to the next match; otherwise go to the
    /// match at or after the cursor first, as Calc does.
    pub fn replace_one(
        &self,
        text: &str,
        match_case: bool,
        column_only: bool,
        with: &str,
        done: impl FnOnce(Found) + 'static,
    ) {
        if self.search_blocked() {
            return;
        }
        let imp = self.imp();
        let Some((query, _)) = self.query(text, match_case, column_only) else {
            return done(Found::NotReady);
        };
        let cur = imp.selection.get().cursor();
        let on_match = self
            .known_matches(&query)
            .filter(|(_, hit)| hit.is_some_and(|h| (h.row, h.col) == (cur.row, cur.col)));
        let Some((matches, _)) = on_match else {
            return self.find(text, match_case, column_only, Step::Here, done);
        };
        {
            let mut table = imp.table.borrow_mut();
            let mut undo = imp.undo.borrow_mut();
            if let (Some(table), Some(undo)) = (table.as_mut(), undo.as_mut()) {
                let value = table.cell_value(cur.row, cur.col).unwrap_or_default();
                if let Some(new) = matches.replace_in(&value, with) {
                    let at = CellRef {
                        row: table.row_id(cur.row),
                        col: table.col_id(cur.col),
                    };
                    undo.execute(Box::new(SetCell::new(at, new)), table);
                }
            }
        }
        self.after_change(None);
        self.find(text, match_case, column_only, Step::Next, done);
    }

    /// Replace all (SRCH-3): every matching cell shown, every match in each, as one undo
    /// step. Counting and building the edit run on the job pool; Esc or a keystroke in
    /// the find bar cancels. Refused above [`data_model::REPLACE_MAX_CELLS`] cells.
    pub fn replace_all(
        &self,
        text: &str,
        match_case: bool,
        column_only: bool,
        with: &str,
        done: impl FnOnce(Replaced) + 'static,
    ) {
        if self.search_blocked() {
            return;
        }
        let imp = self.imp();
        let Some((query, rows)) = self.query(text, match_case, column_only) else {
            return done(Replaced::NotReady);
        };
        let known = self.known_matches(&query).map(|(m, _)| m);
        self.cancel_search();
        let Some(mut snapshot) = imp.table.borrow().as_ref().map(CsvTable::snapshot) else {
            return;
        };
        let progress = Arc::new(SearchProgress::default());
        let with = with.to_owned();
        let started = Instant::now();
        let job = jobs::spawn({
            let progress = progress.clone();
            move |cancel| {
                let matches = match known {
                    Some(m) => m,
                    None => Arc::new(
                        snapshot
                            .search(&query, cancel.flag(), &progress)
                            .map_err(|_| ReplaceError::Cancelled)?,
                    ),
                };
                let done = AtomicU64::new(0);
                let edit = matches.replace_all(&snapshot, &with, cancel.flag(), &done)?;
                Ok((matches.total(), edit))
            }
        });
        imp.replacing.replace(Some(ReplaceJob {
            job: job.clone(),
            progress,
            rows,
        }));
        let g = self.downgrade();
        glib::spawn_future_local(async move {
            let result = job.finished().await;
            let Some(g) = g.upgrade() else { return };
            let imp = g.imp();
            if !matches!(&*imp.replacing.borrow(), Some(r) if r.job.is(&job)) {
                return done(Replaced::Cancelled); // a new table
            }
            imp.replacing.take();
            let (cells, edit) = match result {
                Ok(Ok(r)) => r,
                Ok(Err(ReplaceError::TooMany(n))) => return done(Replaced::TooMany(n)),
                _ => return done(Replaced::Cancelled),
            };
            let Some(edit) = edit else {
                return done(Replaced::Nothing);
            };
            let built = started.elapsed();
            {
                let mut table = imp.table.borrow_mut();
                let mut undo = imp.undo.borrow_mut();
                let (Some(table), Some(undo)) = (table.as_mut(), undo.as_mut()) else {
                    return;
                };
                let Edit::Cells(list) = &edit else { return };
                let focus = list[0].0;
                undo.execute(
                    Box::new(Batch::new("Replace all", vec![edit], focus)),
                    table,
                );
            }
            if std::env::var_os("BRICKS_TIMINGS").is_some() {
                eprintln!(
                    "replace all: {cells} cells, built in {built:?}, applied in {:?}",
                    started.elapsed() - built
                );
            }
            g.after_change(None);
            done(Replaced::Cells(cells));
        });
    }

    /// Matching cells found so far by a search that counts them, for the find bar.
    pub fn search_found(&self) -> Option<u64> {
        let searching = self.imp().searching.borrow();
        let s = searching.as_ref().filter(|s| s.counting)?;
        Some(s.progress.found.load(Ordering::Relaxed))
    }

    /// Some rows are hidden by filters (FILT-1).
    pub fn is_filtered(&self) -> bool {
        self.imp()
            .table
            .borrow()
            .as_ref()
            .is_some_and(|t| t.is_filtered())
    }

    /// While filtered, the rows there are in all: the "y" in "x of y rows".
    pub fn unfiltered_row_count(&self) -> Option<u64> {
        let table = self.imp().table.borrow();
        let t = table.as_ref().filter(|t| t.is_filtered())?;
        Some(t.unfiltered_row_count())
    }

    /// The settings of column `col` (FILT-1, TYPE-2), in a popover under its header button
    /// at `x`: the column's type (Automatic or set), then a filter condition, a value,
    /// Apply, and Clear when the column is filtered. Number conditions are offered for
    /// number columns only.
    fn filter_menu(&self, col: u32, x: f64) {
        let imp = self.imp();
        if let Some(old) = imp.filter_popover.take() {
            old.unparent();
        }
        let (current, inferred, set) = match imp.table.borrow().as_ref() {
            Some(t) => (
                t.filter_on(col).map(|f| f.test.clone()),
                t.inferred_type(col),
                t.type_override(col),
            ),
            None => (None, None, None),
        };
        // A number filter already on keeps its conditions listed whatever the type.
        let number_filter = matches!(current, Some(Test::Number(..)));
        let names = move |number: bool| -> Vec<&'static str> {
            conditions_for(number || number_filter)
                .iter()
                .map(|(name, _)| *name)
                .collect()
        };
        let conditions = gtk::DropDown::from_strings(&names(is_number(set.or(inferred))));
        let auto = format!("Automatic ({})", type_name(inferred));
        let mut type_names = vec![auto.as_str()];
        type_names.extend(TYPES.map(|(name, _)| name));
        let types = gtk::DropDown::from_strings(&type_names);
        types.set_selected(set.map_or(0, |t| 1 + type_choice(t) as u32));
        let value = gtk::Entry::builder()
            .placeholder_text("Value")
            .activates_default(true)
            .build();
        if let Some(test) = &current {
            let (i, text) = condition_of(test);
            conditions.set_selected(i as u32);
            value.set_text(&text);
        }
        let needs_value =
            |i: u32| !matches!(CONDITIONS[i as usize].1, Kind::Empty | Kind::NonEmpty);
        value.set_sensitive(needs_value(conditions.selected()));
        types.connect_selected_notify({
            let (g, conditions) = (self.downgrade(), conditions.downgrade());
            move |dd| {
                let (Some(g), Some(conditions)) = (g.upgrade(), conditions.upgrade()) else {
                    return;
                };
                let ty = (dd.selected() as usize).checked_sub(1).map(|i| TYPES[i].1);
                g.set_column_type(col, ty);
                // Number conditions come and go with the type.
                let shown = names(is_number(ty.or(inferred)));
                let Some(list) = conditions.model().and_downcast::<gtk::StringList>() else {
                    return;
                };
                if list.n_items() as usize != shown.len() {
                    let keep = conditions.selected();
                    list.splice(0, list.n_items(), &shown);
                    let keep = if (keep as usize) < shown.len() {
                        keep
                    } else {
                        0
                    };
                    conditions.set_selected(keep);
                }
            }
        });
        conditions.connect_selected_notify({
            let value = value.downgrade();
            move |dd| {
                if let Some(v) = value.upgrade() {
                    v.set_sensitive(needs_value(dd.selected()));
                }
            }
        });
        let apply = gtk::Button::with_label("Apply");
        apply.add_css_class("suggested-action");
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        buttons.set_halign(gtk::Align::End);
        if current.is_some() {
            let clear = gtk::Button::with_label("Clear");
            clear.connect_clicked({
                let g = self.downgrade();
                move |_| {
                    if let Some(g) = g.upgrade() {
                        g.clear_column_filter(col);
                    }
                }
            });
            buttons.append(&clear);
        }
        buttons.append(&apply);
        let form = gtk::Box::new(gtk::Orientation::Vertical, 8);
        form.set_margin_top(6);
        form.set_margin_bottom(6);
        form.set_margin_start(6);
        form.set_margin_end(6);
        let mut title = String::new();
        column_name(col, &mut title);
        let type_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        type_row.append(&gtk::Label::new(Some("Type")));
        types.set_hexpand(true);
        type_row.append(&types);
        form.append(&type_row);
        form.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        form.append(&gtk::Label::new(Some(&format!(
            "Show rows where column {title}"
        ))));
        form.append(&conditions);
        form.append(&value);
        form.append(&buttons);
        let popover = gtk::Popover::new();
        popover.set_child(Some(&form));
        popover.set_default_widget(Some(&apply));
        popover.set_parent(self);
        popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, HEADER_H as i32, 1, 1)));
        popover.connect_closed({
            let g = self.downgrade();
            move |p| {
                let Some(g) = g.upgrade() else { return };
                let current = matches!(&*g.imp().filter_popover.borrow(), Some(q) if q == p);
                if current {
                    g.imp().filter_popover.take();
                }
                p.unparent();
                g.grab_focus();
            }
        });
        apply.connect_clicked({
            let (g, conditions, value) =
                (self.downgrade(), conditions.downgrade(), value.downgrade());
            move |_| {
                let (Some(g), Some(conditions), Some(value)) =
                    (g.upgrade(), conditions.upgrade(), value.upgrade())
                else {
                    return;
                };
                match test_of(conditions.selected() as usize, &value.text()) {
                    Some(test) => {
                        value.remove_css_class("error");
                        g.apply_filters(vec![(col, test)]);
                    }
                    None => value.add_css_class("error"), // not a number
                }
            }
        });
        imp.filter_popover.replace(Some(popover.clone()));
        popover.popup();
        value.grab_focus();
    }

    /// Evaluate `filters` (column, test) on the job pool, one after the other, and show
    /// the rows that pass them and the other columns' filters. Waits for indexing; Esc
    /// cancels.
    fn apply_filters(&self, filters: Vec<(u32, Test)>) {
        let imp = self.imp();
        self.close_filter_menu();
        if filters.is_empty() || self.busy() || imp.editing.get().is_some() {
            return;
        }
        let (snapshot, filters, rows) = match imp.table.borrow().as_ref() {
            Some(t) if t.is_complete() => {
                let filters: Vec<Filter> = filters
                    .into_iter()
                    .map(|(col, test)| Filter {
                        col: t.col_id(col),
                        test,
                    })
                    .collect();
                let rows = t.index().row_count() * filters.len() as u64;
                (t.snapshot(), filters, rows)
            }
            _ => return,
        };
        let done = Arc::new(AtomicU64::new(0));
        let started = Instant::now();
        let job = jobs::spawn({
            let (done, filters) = (done.clone(), filters.clone());
            move |cancel| {
                filters
                    .iter()
                    .map(|f| snapshot.filter_rows(f, cancel.flag(), &done))
                    .collect()
            }
        });
        imp.filtering.replace(Some(FilterJob {
            filters,
            job: job.clone(),
            done,
            rows,
        }));
        let g = self.downgrade();
        glib::spawn_future_local(async move {
            let result = job.finished().await;
            let Some(g) = g.upgrade() else { return };
            let imp = g.imp();
            let current = matches!(&*imp.filtering.borrow(), Some(f) if f.job.is(&job));
            if std::env::var_os("BRICKS_TIMINGS").is_some() {
                eprintln!(
                    "filters: {:?} in {:?}{}",
                    result.as_ref().map(|r| r.as_ref().map(Vec::len)),
                    started.elapsed(),
                    if current { "" } else { " (dropped)" }
                );
            }
            if !current {
                return;
            }
            let Some(running) = imp.filtering.take() else {
                return;
            };
            let Ok(Ok(sets)) = result else { return };
            if let Some(t) = imp.table.borrow_mut().as_mut() {
                for (filter, rows) in running.filters.into_iter().zip(sets) {
                    t.set_filter(filter, rows);
                }
            }
            g.after_change(None);
        });
    }

    /// Set (or with `None`, drop) the user's type for column `col` (TYPE-2). Only sort and
    /// filter read it; a number filter on the column is applied again, since which cells
    /// are numbers may have changed. Not undoable, like filters.
    pub fn set_column_type(&self, col: u32, ty: Option<InferredType>) {
        let imp = self.imp();
        let refilter = {
            let mut table = imp.table.borrow_mut();
            let Some(t) = table.as_mut() else { return };
            t.set_type_override(col, ty);
            t.filter_on(col)
                .map(|f| f.test.clone())
                .filter(|test| matches!(test, Test::Number(..)))
        };
        self.queue_draw();
        if let Some(test) = refilter {
            self.apply_filters(vec![(col, test)]);
        }
    }

    /// The types the user set, by column position, to set again on the table a save
    /// reopens (TYPE-2).
    pub fn type_overrides(&self) -> Vec<(u32, InferredType)> {
        let table = self.imp().table.borrow();
        let Some(t) = table.as_ref() else {
            return Vec::new();
        };
        (0..t.min_width())
            .filter_map(|c| t.type_override(c).map(|ty| (c, ty)))
            .collect()
    }

    /// Set types from [`Self::type_overrides`] again.
    pub fn restore_type_overrides(&self, types: Vec<(u32, InferredType)>) {
        if let Some(t) = self.imp().table.borrow_mut().as_mut() {
            for (c, ty) in types {
                t.set_type_override(c, Some(ty));
            }
        }
        self.queue_draw();
    }

    /// The filters on, by column position and test, to apply again to the table a save
    /// reopens (FILT-3).
    pub fn filters(&self) -> Vec<(u32, Test)> {
        let table = self.imp().table.borrow();
        let Some(t) = table.as_ref() else {
            return Vec::new();
        };
        (0..t.min_width())
            .filter_map(|c| t.filter_on(c).map(|f| (c, f.test.clone())))
            .collect()
    }

    /// Apply `filters` (from [`Self::filters`]) once the table is indexed: now if it is,
    /// else when the status tick sees indexing finish ([`Self::apply_pending_filters`]).
    pub fn restore_filters(&self, filters: Vec<(u32, Test)>) {
        self.imp().pending_filters.replace(filters);
        self.apply_pending_filters();
    }

    /// Apply filters waiting for indexing to finish, if it has.
    pub fn apply_pending_filters(&self) {
        if !self.is_complete() || self.busy() {
            return;
        }
        let pending = self.imp().pending_filters.take();
        self.apply_filters(pending);
    }

    /// Show column `col`'s rows again, as far as the other filters allow.
    fn clear_column_filter(&self, col: u32) {
        let imp = self.imp();
        self.close_filter_menu();
        if self.busy() {
            return;
        }
        if let Some(t) = imp.table.borrow_mut().as_mut() {
            t.clear_filter(col);
        }
        self.after_change(None);
    }

    /// Close the filter popover. Its `closed` handler takes it out of `filter_popover`, so
    /// no borrow of that may be held here.
    fn close_filter_menu(&self) {
        let popover = self.imp().filter_popover.borrow().clone();
        if let Some(p) = popover {
            p.popdown();
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

    /// The first row is shown as column titles rather than data (ENG-7).
    pub fn has_header(&self) -> bool {
        self.imp()
            .table
            .borrow()
            .as_ref()
            .is_some_and(|t| t.has_header())
    }

    /// Flip "first row is header". Rows shift by one: cached rows reload, and row
    /// heights follow their rows; edits stay with their rows. Column types are inferred
    /// again, now that the first row counts differently. A running sort stops: it was
    /// keeping a different first row in place (SORT-2).
    pub fn set_header(&self, on: bool) {
        let imp = self.imp();
        if let Some(s) = imp.sorting.take() {
            s.job.cancel();
        }
        if let Some(t) = imp.table.borrow_mut().as_mut() {
            t.set_header(on);
        }
        imp.cache.borrow_mut().invalidate();
        imp.titles.take();
        self.refit_sizes();
        self.update_adjustments();
        self.infer_types();
    }

    /// Infer the column types (TYPE-1) on the job pool from a snapshot, once the table is
    /// fully indexed, and keep them on the table. A newer inference, a header flip, or a
    /// new table makes a running one moot.
    pub fn infer_types(&self) {
        let imp = self.imp();
        let mut snapshot = match imp.table.borrow().as_ref() {
            Some(t) if t.is_complete() => t.snapshot(),
            _ => return,
        };
        let started = Instant::now();
        let job = jobs::spawn(move |cancel| snapshot.infer_types(cancel.flag()));
        if let Some(old) = imp.inferring.replace(Some(job.clone())) {
            old.cancel();
        }
        let g = self.downgrade();
        glib::spawn_future_local(async move {
            let types = job.finished().await.ok().flatten();
            let Some(g) = g.upgrade() else { return };
            let imp = g.imp();
            if !matches!(&*imp.inferring.borrow(), Some(j) if j.is(&job)) {
                return;
            }
            imp.inferring.take();
            let Some(types) = types else { return };
            if std::env::var_os("BRICKS_TIMINGS").is_some() {
                eprintln!(
                    "inferred column types in {:?}: {types:?}",
                    started.elapsed()
                );
            }
            if let Some(table) = imp.table.borrow_mut().as_mut() {
                table.set_types(types);
            };
        });
    }

    /// Every column back to the default width.
    pub fn reset_column_widths(&self) {
        let imp = self.imp();
        imp.col_widths.borrow_mut().clear();
        imp.sizing.borrow_mut().cols.clear();
    }

    /// Show a freshly opened table (the file just saved). Its edits are on disk now, so the
    /// undo history, which refers to the old overlay, starts over. Rows and columns get
    /// new identities in the new file, so the remembered sizes move over by position.
    pub fn replace_table(&self, table: CsvTable) {
        let imp = self.imp();
        let (heights, widths) = {
            let sizing = imp.sizing.borrow();
            let old = imp.table.borrow();
            let heights: Vec<(u64, f64)> = (imp.row_heights.borrow().keys())
                .filter_map(|&id| old.as_ref()?.row_of(id))
                .map(|r| (r, sizing.rows.size(r)))
                .collect();
            let widths: Vec<(u32, f64)> = (imp.col_widths.borrow().keys())
                .filter_map(|&id| old.as_ref()?.col_of(id))
                .map(|c| (c, sizing.cols.size(u64::from(c))))
                .collect();
            (heights, widths)
        };
        // Unmapping a big file and freeing its index takes a while (0.2 s for 1 GB, SAVE-3):
        // a worker does it, not the UI thread.
        if let Some(old) = imp.table.replace(Some(table)) {
            let _ = std::thread::Builder::new()
                .name("drop-table".into())
                .spawn(move || drop(old));
        }
        imp.generation.set(imp.generation.get() + 1);
        // Types of the old table don't belong to the new one; the status tick infers them
        // again once it is indexed.
        if let Some(old) = imp.inferring.take() {
            old.cancel();
        }
        // A sort or filter of the old table can't apply to this one; its filters went
        // with it.
        if let Some(old) = imp.sorting.take() {
            old.job.cancel();
        }
        if let Some(old) = imp.filtering.take() {
            old.job.cancel();
        }
        self.cancel_search();
        if let Some(old) = imp.replacing.take() {
            old.job.cancel();
        }
        imp.found.take();
        imp.pending_filters.take(); // a save gives them back with `restore_filters`
        self.close_filter_menu();
        imp.undo.replace(Some(UndoStack::default()));
        imp.cache.borrow_mut().reset();
        imp.titles.take();
        imp.row_heights.borrow_mut().clear();
        imp.col_widths.borrow_mut().clear();
        // Rows of the new file appear as indexing reaches them; heights past what is
        // indexed now land on their positions' source ids all the same.
        for (r, h) in heights {
            let id = imp.table.borrow().as_ref().map(|t| {
                let first = u64::from(t.has_header());
                RowId::source(r + first)
            });
            if let Some(id) = id {
                imp.row_heights.borrow_mut().insert(id, h);
            }
        }
        for (c, w) in widths {
            imp.col_widths.borrow_mut().insert(ColId::source(c), w);
        }
        self.refit_sizes();
        self.update_adjustments();
    }

    /// How many times the table was replaced (`replace_table`) since the window opened.
    pub fn generation(&self) -> u64 {
        self.imp().generation.get()
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

    /// Unsaved changes: cell edits, plus rows inserted and deleted (EDIT-3).
    pub fn edit_count(&self) -> usize {
        self.imp()
            .table
            .borrow()
            .as_ref()
            .map_or(0, |t| t.changes())
    }

    pub fn row_count(&self) -> u64 {
        self.imp()
            .table
            .borrow()
            .as_ref()
            .map_or(0, |t| t.row_count())
    }

    /// Rows the grid shows: the table's, plus the empty edge row once indexing is done.
    fn shown_rows(&self) -> u64 {
        self.imp().table.borrow().as_ref().map_or(0, shown_rows)
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
        let rows = self.shown_rows();
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
            let cols = u64::from(self.col_count() + 1);
            let upper = sizing.cols.start(cols).max(body_w);
            hz.configure(hz.value(), 0.0, upper, COL_W / 4.0, body_w * 0.9, body_w);
        }
        drop(sizing);
        self.queue_draw();
    }
}

/// Rows shown for `table`: its rows, plus the editable edge row once indexing is done.
/// Typing there adds a row at the end of the file, filtered or not (FILT-4).
fn shown_rows(table: &CsvTable) -> u64 {
    table.row_count() + u64::from(table.is_complete())
}

/// What the popover calls an inferred type: "Automatic (whole numbers)".
fn type_name(t: Option<InferredType>) -> &'static str {
    match t {
        None => "not detected yet",
        Some(InferredType::Integer) => "whole numbers",
        Some(InferredType::Decimal) => "numbers",
        Some(InferredType::Text) => "text",
        Some(InferredType::Date) => "dates",
        Some(InferredType::DateTime) => "dates and times",
        Some(InferredType::Boolean) => "true/false",
    }
}

/// A column type's tag in the header (TYPE-2).
fn type_tag(t: InferredType) -> &'static str {
    match t {
        InferredType::Integer => "123",
        InferredType::Decimal => "1.5",
        InferredType::Text => "abc",
        InferredType::Date | InferredType::DateTime => "date",
        InferredType::Boolean => "T/F",
    }
}

/// The types the popover offers after "Automatic" (TYPE-2), and the type each sets.
const TYPES: [(&str, InferredType); 4] = [
    ("Text", InferredType::Text),
    ("Number", InferredType::Decimal),
    ("Date", InferredType::DateTime),
    ("True/false", InferredType::Boolean),
];

/// The popover's entry for type `t`: its index in [`TYPES`].
fn type_choice(t: InferredType) -> usize {
    match t {
        InferredType::Text => 0,
        InferredType::Integer | InferredType::Decimal => 1,
        InferredType::Date | InferredType::DateTime => 2,
        InferredType::Boolean => 3,
    }
}

fn is_number(t: Option<InferredType>) -> bool {
    matches!(t, Some(InferredType::Integer | InferredType::Decimal))
}

/// Conditions offered for a column: number ones only for number columns (they come
/// last in [`CONDITIONS`], so list positions stay the same).
fn conditions_for(number: bool) -> &'static [(&'static str, Kind)] {
    let text = CONDITIONS
        .iter()
        .position(|(_, k)| matches!(k, Kind::Number(_)))
        .unwrap_or(CONDITIONS.len());
    if number {
        &CONDITIONS
    } else {
        &CONDITIONS[..text]
    }
}

/// What a filter condition in the popover needs (FILT-1).
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Equals,
    Contains,
    NotContains,
    Empty,
    NonEmpty,
    Number(Compare),
}

/// The filter popover's conditions, in list order.
const CONDITIONS: [(&str, Kind); 11] = [
    ("equals", Kind::Equals),
    ("contains", Kind::Contains),
    ("does not contain", Kind::NotContains),
    ("is empty", Kind::Empty),
    ("is not empty", Kind::NonEmpty),
    ("= (number)", Kind::Number(Compare::Eq)),
    ("≠ (number)", Kind::Number(Compare::Ne)),
    ("< (number)", Kind::Number(Compare::Lt)),
    ("≤ (number)", Kind::Number(Compare::Le)),
    ("> (number)", Kind::Number(Compare::Gt)),
    ("≥ (number)", Kind::Number(Compare::Ge)),
];

/// The test for condition `i` with `text`; `None` when a number condition's text is not
/// a number.
fn test_of(i: usize, text: &str) -> Option<Test> {
    Some(match CONDITIONS[i].1 {
        Kind::Equals => Test::Equals(text.to_owned()),
        Kind::Contains => Test::Contains(text.to_owned()),
        Kind::NotContains => Test::NotContains(text.to_owned()),
        Kind::Empty => Test::Empty,
        Kind::NonEmpty => Test::NonEmpty,
        Kind::Number(c) => {
            Test::Number(c, text.trim().parse().ok().filter(|v: &f64| v.is_finite())?)
        }
    })
}

/// The popover's condition and text for a filter already on a column.
fn condition_of(test: &Test) -> (usize, String) {
    let (kind, text) = match test {
        Test::Equals(s) => (Kind::Equals, s.clone()),
        Test::Contains(s) => (Kind::Contains, s.clone()),
        Test::NotContains(s) => (Kind::NotContains, s.clone()),
        Test::Empty => (Kind::Empty, String::new()),
        Test::NonEmpty => (Kind::NonEmpty, String::new()),
        Test::Number(c, v) => (Kind::Number(*c), v.to_string()),
    };
    let i = CONDITIONS.iter().position(|(_, k)| *k == kind).unwrap_or(0);
    (i, text)
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
