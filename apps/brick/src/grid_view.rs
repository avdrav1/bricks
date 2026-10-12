//! The grid widget: draws the cells `grid::GridCache` holds with GSK + Pango (ADR 0001).

use crate::editor::{self, Action, ClipCommand, Mode, ShapeCommand, Start};
use crate::looks::{Dash, Line, Look, Looks};
use crate::status::ClipView;

use commands::{Batch, ClearCells, Command, OnSheet, Reshape, SetCell, SheetEdit, UndoStack};
use csv_engine::RowIndex as _;
use data_model::{
    is_formula, parse_tsv, BookEdit, BookState, Budget, CellArea, CellRef, CellValues, ColId,
    ColumnTypes, Compare, Computed, Copied, CsvTable, DelimiterChoice, Edit, Fill, Filter,
    FilterError, Font, FormatError, FormatTarget, FormulaCells, Hit, InferredType, JournalError,
    Matches, MergeError, MergePlan, Origin, PasteError, Pattern, Query, ReplaceError, RereadError,
    RowId, RowSet, SaveJob, SaveJobError, SearchError, SearchProgress, SheetId, ShownSheet,
    SortError, SortOrder, Step, StyleId, StylePatch, TableSource, Test, Underline, VAlign,
    Workbook, Xf, MERGE_MAX_CELLS, PASTE_MAX_CELLS, REPLACE_MAX_CELLS, UI_BUDGET,
};
use data_model::{HAlign, StyleBlock};
use formula::{column_name, Formatted, NumberFormat, Value};
use grid::{Bounds, GridCache, Key, Mods, Selection, Sizes, Viewport};
use gtk::{gdk, gio, glib, graphene, pango, prelude::*, subclass::prelude::*};
use std::cell::{Cell, Ref, RefCell, RefMut};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::rc::Rc;
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
/// Most cells a centre-across-selection run reaches on each side of its text.
const ACROSS_MAX: u64 = 256;
/// Smallest sizes a drag can give a column or row.
const MIN_COL_W: f64 = 16.0;
const MIN_ROW_H: f64 = 8.0;
/// How long the status bar says what was copied or pasted.
const CLIP_NOTE: Duration = Duration::from_secs(4);

/// A copy running on the job pool (CLIP-1, ENG-8).
pub struct CopyJob {
    job: jobs::Job<Option<CopyResult>>,
    done: Arc<AtomicU64>,
    rows: u64,
}

/// What a copy gives: the clipboard text, and the formulas and styles a paste back into
/// Brick restores.
type CopyResult = (Copied, Vec<ClipFormula>, Option<StyleBlock>);

/// A formula cell in a copied block (FML-12): where it is in the block (row and column
/// offsets), its raw text, and where it was (row among all rows, view column), so a paste
/// inside Brick can shift its relative references.
pub struct ClipFormula {
    row: u64,
    col: u32,
    raw: String,
    from: (u64, u32),
}

/// The last copy's formulas (FML-12) and styles (STYLE-7): pasted back into Brick, the
/// cells get the formulas, shifted, and the styles; pasted anywhere else, the text is the
/// results.
pub struct FormulaClip {
    /// The exact text put on the clipboard.
    text: String,
    cells: Vec<ClipFormula>,
    styles: Option<StyleBlock>,
}

/// A sort's result: the new row order, and the formulas whose relative rows follow their
/// moved rows (FML-10).
type Sorted = Option<(Edit, Vec<(CellRef, Option<Box<str>>)>)>;

/// A sort running on the job pool (SORT-1): its result, and file rows read so far.
pub struct SortJob {
    job: jobs::Job<Result<Sorted, SortError>>,
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

/// Formula results computed on the job pool (FML-4) for cells the UI's budget didn't
/// reach: the cells, and how many are done.
pub struct CalcJob {
    job: jobs::Job<Option<CellValues>>,
    done: Arc<AtomicU64>,
    total: u64,
    /// The workbook revision the job's snapshot has.
    revision: u64,
}

/// The scan for a sheet's formula cells on the job pool (FML-4).
pub struct ScanJob {
    job: jobs::Job<Option<FormulaCells>>,
    done: Arc<AtomicU64>,
    rows: u64,
}

/// A sheet's formula results, for the workbook revision they were computed at (FML-11).
pub type KnownResults = (u64, SheetId, Arc<Computed>);

/// How a job gets the active table as it shows (FML-11): a snapshot, with the sheet's
/// formula results computed in the job when they aren't known yet. `None` once cancelled.
type JobTable = Box<dyn FnOnce(&jobs::CancelToken) -> Option<CsvTable> + Send>;

/// Formula rewrites a sheet or structure change needs (SHEET-2, FML-10), computed on the
/// job pool from a workbook snapshot.
pub struct RewriteJob {
    job: jobs::Job<Option<Vec<(SheetId, Edit)>>>,
}

/// A whole-row or whole-column format worked out on the job pool from a workbook
/// snapshot (STYLE-6): the edit, and the pool with the records it needs.
pub type FormatJob = jobs::Job<Result<(Edit, Arc<data_model::StylePool>), FormatError>>;

/// Style runs past which a whole-row or whole-column format is worked out in a job: on
/// the 1,048,576-row styled corpus (125,832 runs), eight whole columns took 13 ms.
const FORMAT_JOB_RUNS: usize = 16_384;

/// What a sheet looked like when another sheet was shown (XLSX-1): it comes back as it was.
#[derive(Default)]
pub struct SheetUi {
    selection: Selection,
    vscroll: f64,
    hscroll: f64,
    sizing: Sizing,
    row_heights: HashMap<RowId, f64>,
    col_widths: HashMap<ColId, f64>,
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
    /// Replace was asked on a formula cell, which it never changes (FML-11): the cursor
    /// went on to the next match, the `ordinal`th of `total`.
    Skipped {
        ordinal: u64,
        total: u64,
    },
}

/// A replace all on the job pool (SRCH-3): the matching cells and the edit, and the
/// progress of counting them.
/// What a replace all found: matching cells, and the edit (if any) with the formula cells
/// it skipped (FML-11).
type ReplaceResult = Result<(u64, (Option<Edit>, u64)), ReplaceError>;

pub struct ReplaceJob {
    job: jobs::Job<ReplaceResult>,
    progress: Arc<SearchProgress>,
    rows: u64,
}

/// How a replace all went (SRCH-3), for the find bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Replaced {
    /// This many cells changed, as one undo step; matching formula cells are never
    /// replaced (FML-11), and this many were skipped.
    Cells {
        replaced: u64,
        skipped: u64,
    },
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
        /// The sheets this window shows (ADR 0008).
        pub book: RefCell<Workbook>,
        pub cache: RefCell<GridCache>,
        pub vadj: RefCell<Option<gtk::Adjustment>>,
        pub hadj: RefCell<Option<gtk::Adjustment>>,
        /// Shaped text, kept while used: entries unused for a whole frame are dropped, so
        /// this holds at most two screens of cells.
        pub layouts: RefCell<HashMap<String, pango::Layout>>,
        pub layouts_prev: RefCell<HashMap<String, pango::Layout>>,
        /// How each cell style looks (STYLE-3), resolved once per style.
        pub looks: RefCell<Option<Looks>>,
        /// Shaped cell text in its style's font, by font key, wrap width, and text; kept
        /// like `layouts`.
        pub styled: RefCell<HashMap<(u32, i32, String), pango::Layout>>,
        pub styled_prev: RefCell<HashMap<(u32, i32, String), pango::Layout>>,
        /// Number formats by code, compiled once (`None`: a code that doesn't compile).
        pub formats: RefCell<HashMap<Box<str>, Option<Rc<NumberFormat>>>>,
        /// Cell text under a number format, by code and text; kept like `layouts`.
        pub formatted: RefCell<FormattedCache>,
        pub formatted_prev: RefCell<FormattedCache>,
        pub buf: RefCell<String>,
        pub row_header_w: Cell<f64>,
        /// Top row of the last frame drawn with its text loaded; `None` before then.
        pub painted_top: Cell<Option<u64>>,
        /// Every edit goes through here as a command (invariant 5).
        pub undo: RefCell<UndoStack<Workbook>>,
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
        /// The formulas of the last copy made here, while its text is what was copied
        /// (FML-12).
        pub formula_clip: RefCell<Option<FormulaClip>>,
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
        /// Formula results being computed (FML-4), if any.
        pub calculating: RefCell<Option<CalcJob>>,
        /// The scan for the active sheet's formula cells (FML-4), if running: the job,
        /// file rows scanned, and rows in all.
        pub scanning: RefCell<Option<ScanJob>>,
        /// A workbook is being imported (XLSX-1): nothing can change, and the body says
        /// "Opening…".
        pub loading: Cell<bool>,
        /// Each sheet's cursor, scroll, and sizes while another sheet shows (XLSX-1).
        pub sheet_ui: RefCell<HashMap<SheetId, SheetUi>>,
        /// The active sheet's formula results as the last job computed them (FML-11), by
        /// workbook revision and sheet: jobs that read shown text reuse them.
        pub results: Arc<std::sync::Mutex<Option<KnownResults>>>,
        /// Formulas being rewritten for a change that waits for them (SHEET-2, FML-10).
        /// Edits wait for it, as for a sort.
        pub rewriting: RefCell<Option<RewriteJob>>,
        /// A whole-row or whole-column format being worked out (STYLE-6). Edits wait.
        pub formatting: RefCell<Option<FormatJob>>,
        /// The sheet whose cursor, scroll, and sizes the fields above hold (SHEET-1): an
        /// undo can make another sheet active, and its own come back then.
        pub ui_sheet: Cell<Option<SheetId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GridView {
        const NAME: &'static str = "BrickGridView";
        type Type = super::GridView;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            // A grid (ARIA) can carry a description: the cursor cell and what it shows
            // (FML-4). The default generic role can't.
            klass.set_accessible_role(gtk::AccessibleRole::Grid);
        }
    }

    impl ObjectImpl for GridView {
        fn signals() -> &'static [glib::subclass::Signal] {
            static SIGNALS: std::sync::OnceLock<Vec<glib::subclass::Signal>> =
                std::sync::OnceLock::new();
            SIGNALS.get_or_init(|| {
                vec![
                    // A typed formula doesn't parse: the message says where (FML-4).
                    glib::subclass::Signal::builder("formula-error")
                        .param_types([String::static_type()])
                        .build(),
                    // The cell cursor or the selected range moved (FML-9's formula bar).
                    glib::subclass::Signal::builder("cursor-changed").build(),
                    // The data or the sheet shown changed (FML-9's formula bar).
                    glib::subclass::Signal::builder("changed").build(),
                    // A change was refused: a title and why (SHEET-2: too many formulas
                    // to rewrite).
                    glib::subclass::Signal::builder("refused")
                        .param_types([String::static_type(), String::static_type()])
                        .build(),
                ]
            })
        }

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
            let cur = std::mem::take(&mut *self.styled.borrow_mut());
            *self.styled_prev.borrow_mut() = cur;
            let cur = std::mem::take(&mut *self.formatted.borrow_mut());
            *self.formatted_prev.borrow_mut() = cur;
        }

        /// `text` shaped in `look`'s font, wrapped at `wrap` pixels (0: one line).
        pub fn styled_layout(&self, text: &str, look: &Look, wrap: i32) -> pango::Layout {
            if look.font_key == 0 && wrap == 0 {
                return self.layout(text);
            }
            let key = (look.font_key, wrap, text.to_owned());
            if let Some(l) = self.styled.borrow().get(&key) {
                return l.clone();
            }
            let l = self
                .styled_prev
                .borrow_mut()
                .remove(&key)
                .unwrap_or_else(|| {
                    let l = self.obj().create_pango_layout(Some(text));
                    if let Some(font) = &look.font {
                        l.set_font_description(Some(font));
                    }
                    if wrap > 0 {
                        // Excel breaks a word too long for the cell without a hyphen.
                        let attrs = look
                            .attrs
                            .as_ref()
                            .and_then(pango::AttrList::copy)
                            .unwrap_or_default();
                        attrs.insert(pango::AttrInt::new_insert_hyphens(false));
                        l.set_attributes(Some(&attrs));
                        l.set_width(wrap * pango::SCALE);
                        l.set_wrap(pango::WrapMode::WordChar);
                    } else {
                        l.set_attributes(look.attrs.as_ref());
                        l.set_single_paragraph_mode(true);
                    }
                    l
                });
            self.styled.borrow_mut().insert(key, l.clone());
            l
        }

        /// `text` as number format `code` shows it; `None` if the code doesn't compile.
        pub fn formatted(&self, code: &str, text: &str) -> Option<Rc<Formatted>> {
            let key = (Box::<str>::from(code), text.to_owned());
            if let Some(f) = self.formatted.borrow().get(&key) {
                return Some(f.clone());
            }
            let f = match self.formatted_prev.borrow_mut().remove(&key) {
                Some(f) => f,
                None => {
                    let format = self
                        .formats
                        .borrow_mut()
                        .entry(key.0.clone())
                        .or_insert_with(|| NumberFormat::parse(code).ok().map(Rc::new))
                        .clone()?;
                    Rc::new(format.display(&formula::literal(text)))
                }
            };
            self.formatted.borrow_mut().insert(key, f.clone());
            Some(f)
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
            // Grows whole rows/columns to what indexing found; empty cells fill the rest.
            let reach = obj.reach();
            let (Some(vadj), Some(hadj)) = (self.vadj.borrow().clone(), self.hadj.borrow().clone())
            else {
                return;
            };
            let mut book = self.book.borrow_mut();
            let mut cache = self.cache.borrow_mut();
            let (w, h) = (obj.width() as f64, obj.height() as f64);
            let rh_w = self.row_header_w.get();
            let (body_w, body_h) = self.body_size();

            let sizing = self.sizing.borrow();
            let rows = Viewport {
                scroll: vadj.value(),
                extent: body_h,
                sizes: &sizing.rows,
                count: reach.rows,
            };
            let visible = rows.visible(0);
            // Formula cells show their results (FML-4); those over the frame's budget wait
            // for a job.
            let mut shown = ShownSheet::new(&mut book);
            cache.ensure(visible.clone(), BUFFER_ROWS, &mut shown);
            let pending = shown.into_pending();
            let table = book.active();
            self.painted_top
                .set(cache.cell(visible.start, 0).map(|_| visible.start));
            let cols = Viewport {
                scroll: hadj.value(),
                extent: body_w,
                sizes: &sizing.cols,
                count: u64::from((cache.col_count().max(table.min_width()) + 1).max(reach.cols)),
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

            // Merged cells on screen (STYLE-4): each draws as one cell, the anchor's.
            let merged = visible_merges(table, visible.clone(), visible_cols.clone());
            let covered = |r: u64, c: u64| merged.iter().any(|m| m.covers(r, c as u32));
            let clamp_x = |x: f64| x.clamp(rh_w - 4.0, w + 4.0);
            let clamp_y = |y: f64| y.clamp(HEADER_H - 4.0, h + 4.0);
            let col_x = |c: u32| rh_w + cols.pos(u64::from(c));
            let col_end = |c: u32| col_x(c) + cols.size(u64::from(c));
            let row_y = |r: u64| HEADER_H + rows.pos(r);
            let row_end = |r: u64| row_y(r) + rows.size(r);

            // Body: stripes, grid lines, cells. Stripes and inner lines stop at merges; a
            // sheet saved with grid lines off shows only its cells' borders, as in Excel.
            snapshot.push_clip(&rect(rh_w, HEADER_H, body_w, body_h));
            let grid_lines = !table.layout().hide_grid_lines;
            let mut holes: Vec<(f64, f64)> = Vec::new();
            for r in visible.clone() {
                let (y, rh) = (row_y(r), rows.size(r));
                if r % 2 == 1 {
                    holes.clear();
                    holes.extend(
                        merged
                            .iter()
                            .filter(|m| m.tl.row <= r && r <= m.br.row)
                            .map(|m| (col_x(m.tl.col), col_end(m.br.col))),
                    );
                    each_gap(rh_w, rh_w + body_w, &mut holes, |x0, x1| {
                        snapshot.append_color(&stripe, &rect(x0, y, x1 - x0, rh));
                    });
                }
                if !grid_lines {
                    continue;
                }
                holes.clear();
                holes.extend(
                    merged
                        .iter()
                        .filter(|m| m.tl.row <= r && r < m.br.row)
                        .map(|m| (col_x(m.tl.col), col_end(m.br.col))),
                );
                each_gap(rh_w, rh_w + body_w, &mut holes, |x0, x1| {
                    snapshot.append_color(&line, &rect(x0, y + rh - 1.0, x1 - x0, 1.0));
                });
            }
            for c in visible_cols.clone().filter(|_| grid_lines) {
                let x = col_end(c as u32) - 1.0;
                holes.clear();
                holes.extend(
                    merged
                        .iter()
                        .filter(|m| u64::from(m.tl.col) <= c && c < u64::from(m.br.col))
                        .map(|m| (clamp_y(row_y(m.tl.row)), clamp_y(row_end(m.br.row)))),
                );
                each_gap(HEADER_H, HEADER_H + body_h, &mut holes, |y0, y1| {
                    snapshot.append_color(&line, &rect(x, y0, 1.0, y1 - y0));
                });
            }

            // Cells (STYLE-3): fills, then text, then borders, so text can overflow into
            // empty neighbours and borders sit on top. Cells a merge covers draw nothing:
            // the merge draws once over them all.
            let pool = book.styles().clone();
            let base = obj.pango_context().font_description().unwrap_or_default();
            let mut looks = self.looks.borrow_mut();
            let looks = looks.get_or_insert_with(|| Looks::new(pool.clone(), base.clone(), fg));
            looks.sync(&pool, &base, fg);
            let styled =
                !table.file_styles().is_empty() || table.has_user_styles() || pool.xfs.len() > 1;
            let plain = looks.get(StyleId(0));
            let mut cells: Vec<(u64, u64, Rc<Look>)> = Vec::new();
            for r in visible.clone().take_while(|&r| r < table.row_count()) {
                let row = table.row_id(r);
                for c in visible_cols.clone() {
                    if covered(r, c) {
                        continue;
                    }
                    let look = if styled {
                        let col = table.col_id(c as u32);
                        looks.get(table.style_of(CellRef { row, col }))
                    } else {
                        plain.clone()
                    };
                    cells.push((r, c, look));
                }
            }
            let merged_looks: Vec<Rc<Look>> = merged
                .iter()
                .map(|m| {
                    if styled {
                        looks.get(table.style_of(m.anchor))
                    } else {
                        plain.clone()
                    }
                })
                .collect();
            // A merge's rectangle, clamped to just outside the body: one can span far more
            // rows than are on screen.
            let merged_rect = |m: &Merged| {
                let (x0, y0) = (clamp_x(col_x(m.tl.col)), clamp_y(row_y(m.tl.row)));
                let (x1, y1) = (clamp_x(col_end(m.br.col)), clamp_y(row_end(m.br.row)));
                (x0, x1 - x0, y0, y1 - y0)
            };
            // Fills snap to whole pixels: neighbours sharing a fractional edge would each
            // cover part of that pixel and leave a light seam between them.
            let fill_rect = |x: f64, y: f64, w: f64, h: f64| {
                let (x0, y0) = (x.round(), y.round());
                rect(x0, y0, (x + w).round() - x0, (y + h).round() - y0)
            };
            for (r, c, look) in &cells {
                if let Some(fill) = look.fill {
                    let (x, y) = (rh_w + cols.pos(*c), HEADER_H + rows.pos(*r));
                    snapshot.append_color(&fill, &fill_rect(x, y, cols.size(*c), rows.size(*r)));
                }
            }
            for (m, look) in merged.iter().zip(&merged_looks) {
                let (x, mw, y, mh) = merged_rect(m);
                if m.tl.row % 2 == 1 {
                    snapshot.append_color(&stripe, &rect(x, y, mw, mh));
                }
                if let Some(fill) = look.fill {
                    snapshot.append_color(&fill, &fill_rect(x, y, mw, mh));
                }
            }
            let empty = |r: u64, c: u64| {
                !covered(r, c) && cache.cell(r, c as u32).is_none_or(str::is_empty)
            };
            // Centre across selection (`centerContinuous`): text centres over the run of
            // empty cells beside it that have the same alignment, as in Excel; the cells
            // keep their own fills and borders. Keyed by index into `cells`.
            let mut across: Vec<(usize, f64, f64)> = Vec::new();
            for (i, (r, c, look)) in cells.iter().enumerate() {
                if look.horizontal != HAlign::CenterContinuous || empty(*r, *c) {
                    continue;
                }
                let row = table.row_id(*r);
                let (first, last) = across_run(*c, cols.count, |k| {
                    let col = table.col_id(k as u32);
                    empty(*r, k)
                        && looks.get(table.style_of(CellRef { row, col })).horizontal
                            == HAlign::CenterContinuous
                });
                if first != last {
                    let x = rh_w + cols.pos(first);
                    across.push((i, x, rh_w + cols.pos(last) + cols.size(last) - x));
                }
            }
            let digit_w = f64::from(self.layout("0").pixel_size().0.max(1));
            // Draws `raw` in `look` in the cell at (`x`, `y`), `cw` by `rh`; `spill` (its
            // row and column) lets text that doesn't fit overflow into empty neighbours.
            let draw_text = |raw: &str,
                             look: &Look,
                             (x, cw): (f64, f64),
                             (y, rh): (f64, f64),
                             spill_from: Option<(u64, u64)>| {
                let value = formula::literal(raw);
                let formatted = look
                    .format
                    .as_deref()
                    .and_then(|code| self.formatted(code, raw));
                let text = formatted.as_ref().map_or(raw, |f| f.text.as_str());
                let color = formatted
                    .as_ref()
                    .and_then(|f| f.color)
                    .and_then(|tag| looks.tag_color(look, tag))
                    .or(look.color)
                    .unwrap_or(fg);
                let number = matches!(value, Value::Number(_) | Value::Date(_));
                let align = match look.horizontal {
                    HAlign::General if number => Align::Right,
                    HAlign::General if matches!(value, Value::Bool(_) | Value::Error(_)) => {
                        Align::Center
                    }
                    HAlign::General | HAlign::Left | HAlign::Fill | HAlign::Justify => Align::Left,
                    HAlign::Distributed => Align::Left,
                    HAlign::Center | HAlign::CenterContinuous => Align::Center,
                    HAlign::Right => Align::Right,
                };
                let indent = f64::from(look.indent) * digit_w * 1.5;
                let wrap = if look.wrap {
                    ((cw - 2.0 * PAD).max(1.0)) as i32
                } else {
                    0
                };
                // A `*x` fill: the text splits there, the parts go to the two edges, and x
                // repeats between them (Excel's accounting formats pad with spaces).
                if let Some((at, ch)) = formatted
                    .as_ref()
                    .and_then(|f| f.fill)
                    .filter(|_| wrap == 0)
                {
                    let (left, right) = text.split_at(at.min(text.len()));
                    let (lw, th) = self.styled_layout(left, look, 0).pixel_size();
                    let rw = self.styled_layout(right, look, 0).pixel_size().0;
                    let ty = text_y(look.vertical, y, rh, f64::from(th));
                    snapshot.push_clip(&rect(x, y, cw - 1.0, rh - 1.0));
                    let draw = |t: &str, tx: f64| {
                        let layout = self.styled_layout(t, look, 0);
                        snapshot.save();
                        snapshot.translate(&graphene::Point::new(tx as f32, ty as f32));
                        snapshot.append_layout(&layout, &color);
                        snapshot.restore();
                    };
                    draw(left, x + PAD);
                    let right_x = x + cw - PAD - f64::from(rw);
                    draw(right, right_x);
                    if ch != ' ' {
                        let mut one = [0u8; 4];
                        let unit = self
                            .styled_layout(ch.encode_utf8(&mut one), look, 0)
                            .pixel_size()
                            .0;
                        let gap = right_x - (x + PAD + f64::from(lw));
                        let n = (gap / f64::from(unit.max(1))).floor().max(0.0) as usize;
                        draw(&ch.to_string().repeat(n), x + PAD + f64::from(lw));
                    }
                    snapshot.pop();
                    return;
                }
                let layout = self.styled_layout(text, look, wrap);
                let (tw, th) = layout.pixel_size();
                let (tw, th) = (f64::from(tw), f64::from(th));
                // Text that doesn't fit spills into empty neighbours, as in Excel: numbers and
                // wrapped text don't.
                let (mut x0, mut x1) = (x, x + cw);
                if let Some((r, c)) =
                    spill_from.filter(|_| wrap == 0 && !number && tw + 2.0 * PAD + indent > cw)
                {
                    let spill = |mut k: u64, step: i64, room: &mut f64| {
                        let mut width = 0.0;
                        while *room > 0.0 {
                            let next = k as i64 + step;
                            if next < visible_cols.start as i64
                                || next >= visible_cols.end as i64
                                || !empty(r, next as u64)
                            {
                                break;
                            }
                            k = next as u64;
                            let w = cols.size(k);
                            width += w;
                            *room -= w;
                        }
                        width
                    };
                    let mut room = tw + 2.0 * PAD + indent - cw;
                    match align {
                        Align::Left => x1 += spill(c, 1, &mut room),
                        Align::Right => x0 -= spill(c, -1, &mut room),
                        Align::Center => {
                            room /= 2.0;
                            let mut other = room;
                            x1 += spill(c, 1, &mut room);
                            x0 -= spill(c, -1, &mut other);
                        }
                    }
                }
                let tx = match align {
                    Align::Left => x + PAD + indent,
                    Align::Right => x + cw - PAD - tw - indent,
                    Align::Center => x + (cw - tw) / 2.0,
                };
                let ty = if wrap > 0 && th > rh - 6.0 {
                    y + 1.0 // wrapped lines that fill the row start at its top
                } else {
                    text_y(look.vertical, y, rh, th)
                };
                // Most text fits its cell: a clip per cell costs the renderer, so only text
                // that reaches past its room gets one.
                let clip = tx < x0 || tx + tw > x1 - 1.0 || ty + th > y + rh - 1.0;
                if clip {
                    snapshot.push_clip(&rect(x0, y, x1 - x0 - 1.0, rh - 1.0));
                }
                snapshot.save();
                snapshot.translate(&graphene::Point::new(tx as f32, ty as f32));
                snapshot.append_layout(&layout, &color);
                snapshot.restore();
                if clip {
                    snapshot.pop();
                }
            };
            let mut across = across.into_iter().peekable();
            for (i, (r, c, look)) in cells.iter().enumerate() {
                let Some(raw) = cache.cell(*r, *c as u32).filter(|t| !t.is_empty()) else {
                    continue;
                };
                let (y, rh) = (HEADER_H + rows.pos(*r), rows.size(*r));
                if let Some((_, x, w)) = across.next_if(|a| a.0 == i) {
                    draw_text(raw, look, (x, w), (y, rh), None);
                    continue;
                }
                let (x, cw) = (rh_w + cols.pos(*c), cols.size(*c));
                draw_text(raw, look, (x, cw), (y, rh), Some((*r, *c)));
            }
            // A merge's text is its anchor's, laid out across the whole merge (a centred
            // title centres over every column it spans). An anchor scrolled out of the
            // loaded rows has no text here yet; a filtered-out one shows none.
            for (m, look) in merged.iter().zip(&merged_looks) {
                let raw = cache.cell(m.tl.row, m.tl.col).filter(|t| !t.is_empty());
                let Some(raw) = raw.filter(|_| m.anchor_shown) else {
                    continue;
                };
                let (x, y) = (col_x(m.tl.col), row_y(m.tl.row));
                let (mw, mh) = (col_end(m.br.col) - x, row_end(m.br.row) - y);
                draw_text(raw, look, (x, mw), (y, mh), None);
            }
            let borders = |look: &Look, x: f64, y: f64, cw: f64, rh: f64| {
                let [top, bottom, left, right] = look.borders;
                for (line, x, y, len, across) in [
                    (top, x - 1.0, y - 1.0, cw + 1.0, true),
                    (bottom, x - 1.0, y + rh - 1.0, cw + 1.0, true),
                    (left, x - 1.0, y - 1.0, rh + 1.0, false),
                    (right, x + cw - 1.0, y - 1.0, rh + 1.0, false),
                ] {
                    if let Some(line) = line {
                        draw_line(snapshot, &line, x, y, len, across);
                    }
                }
            };
            for (r, c, look) in cells
                .iter()
                .filter(|(_, _, l)| l.borders.iter().any(Option::is_some))
            {
                let (x, cw) = (rh_w + cols.pos(*c), cols.size(*c));
                let (y, rh) = (HEADER_H + rows.pos(*r), rows.size(*r));
                borders(look, x, y, cw, rh);
            }
            // A merge has its anchor's borders, around the whole merge.
            for (m, look) in merged.iter().zip(&merged_looks) {
                if look.borders.iter().any(Option::is_some) {
                    let (x, mw, y, mh) = merged_rect(m);
                    borders(look, x, y, mw, mh);
                }
            }
            drop(cells);

            // Selection and cell cursor. Coordinates are clamped to just outside the body
            // before narrowing to f32, so far-away edges stay exact. The selection covers
            // the merges it touches; a cursor on a merge outlines all of it. Whole rows and
            // columns cover the data, and show on through the empty cells past it (GRID-6).
            let merges = TableMerges(table);
            let sel = self.selection.get();
            let (tl, br) = sel.area(&merges);
            let (whole_cols, whole_rows) = (sel.whole_columns().is_some(), sel.is_whole_rows());
            let x1 = if whole_rows { w } else { col_end(br.col) };
            let y1 = if whole_cols { h } else { row_end(br.row) };
            let (x0, x1) = (clamp_x(col_x(tl.col)), clamp_x(x1));
            let (y0, y1) = (clamp_y(row_y(tl.row)), clamp_y(y1));
            let accent = gdk::RGBA::new(0.21, 0.52, 0.89, 1.0);
            let (range_tl, range_br) = sel.range();
            if range_tl != range_br || whole_cols || whole_rows {
                let fill = gdk::RGBA::new(accent.red(), accent.green(), accent.blue(), 0.18);
                snapshot.append_color(&fill, &rect(x0, y0, x1 - x0, y1 - y0));
            }
            let cur = sel.cursor();
            let (cur_tl, cur_br) = grid::Merges::at(&merges, cur).unwrap_or((cur, cur));
            let (cx, cx1) = (clamp_x(col_x(cur_tl.col)), clamp_x(col_end(cur_br.col)));
            let (cy, cy1) = (clamp_y(row_y(cur_tl.row)), clamp_y(row_end(cur_br.row)));
            let (cw, ch) = (cx1 - cx, cy1 - cy);
            if cy1 > HEADER_H && cy < h && cx1 > rh_w && cx < w {
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
            // File row numbers, as references count them (ADR 0006): the header row is 1.
            for r in visible {
                let y = HEADER_H + rows.pos(r);
                snapshot.append_color(&line, &rect(0.0, y + rows.size(r) - 1.0, rh_w, 1.0));
                buf.clear();
                let _ = write!(buf, "{}", row_number(table, r));
                text_at(&buf, PAD + 2.0, y + 3.0);
            }
            snapshot.pop();

            // Column letters, and the column titles when the first row is a header (ENG-7).
            snapshot.append_color(&header_bg, &rect(0.0, 0.0, w, HEADER_H));
            snapshot.append_color(&mark, &rect(x0, 0.0, x1 - x0, HEADER_H));
            snapshot.append_color(&line, &rect(0.0, HEADER_H - 1.0, w, 1.0));
            snapshot.append_color(&line, &rect(rh_w - 1.0, 0.0, 1.0, h));
            if table.has_header() {
                // The header row is row 1: its number sits in the corner, over the titles'
                // row of the gutter.
                let w = f64::from(self.layout("1").pixel_size().0);
                text_in("1", ((rh_w - w) / 2.0).max(0.0), 4.0, &dim);
            }
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

            if self.loading.get() {
                // A workbook is being imported (XLSX-1).
                let text = "Opening…";
                let (tw, _) = self.layout(text).pixel_size();
                let x = rh_w + ((body_w - f64::from(tw)) / 2.0).max(0.0);
                text_in(text, x, HEADER_H + body_h / 2.0 - 10.0, &dim);
            }

            // The in-cell editor (EDIT-2) sits over its cell, inside the body.
            let editor = self.editor.borrow();
            if let Some(entry) = editor.as_ref().filter(|e| WidgetExt::is_visible(*e)) {
                snapshot.push_clip(&rect(rh_w, HEADER_H, body_w, body_h));
                obj.snapshot_child(entry, snapshot);
                snapshot.pop();
            }
            drop(buf);
            self.end_frame();
            drop(cache);
            drop(book);
            obj.calc_pending(pending);
        }
    }
}

glib::wrapper! {
    pub struct GridView(ObjectSubclass<imp::GridView>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl GridView {
    pub fn new(book: Workbook, vadj: &gtk::Adjustment, hadj: &gtk::Adjustment) -> Self {
        let g: Self = glib::Object::new();
        let imp = g.imp();
        imp.ui_sheet.set(Some(book.active_id()));
        imp.book.replace(book);
        imp.vadj.replace(Some(vadj.clone()));
        imp.hadj.replace(Some(hadj.clone()));
        imp.row_header_w.set(64.0);
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
            ("merge", Self::merge_selection),
            ("unmerge", Self::unmerge_selection),
            ("bold", |g| g.toggle_font(FontToggle::Bold)),
            ("italic", |g| g.toggle_font(FontToggle::Italic)),
            ("underline", |g| g.toggle_font(FontToggle::Underline)),
            ("strikethrough", |g| g.toggle_font(FontToggle::Strike)),
            ("clear-formatting", |g| {
                g.format_selection(&StylePatch {
                    clear: true,
                    ..StylePatch::default()
                })
            }),
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
                        // The empty cells past the data reach on as it scrolls (GRID-6).
                        g.update_adjustments();
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

    /// The data's extent: the rows indexed so far, the widest row seen, and one screen of
    /// whole rows, plus the editable edge: one empty row (once indexing is done) and one
    /// empty column past the data, where typing grows the table (SAVE-2). Whole rows,
    /// whole columns, and select-all cover this much, and follow it as it grows; the
    /// grid shows and moves within [`GridView::reach`].
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
                self.set_selection(sel);
            }
        }
        b
    }

    /// Columns with data: the widest row loaded, and never fewer than the header row's or
    /// than reach every filtered column (FILT-2: a filter hiding every row stays clearable).
    fn col_count(&self) -> u32 {
        let imp = self.imp();
        let min = self.table().min_width();
        imp.cache.borrow().col_count().max(min)
    }

    /// What the grid shows and the cursor moves within (GRID-6): [`GridView::bounds`],
    /// then empty rows and columns past the data, as in Excel and Calc: enough to fill the
    /// window and one more screen past where it is scrolled, and to reach the selection
    /// and the cursor, up to [`reach_limit`]. Scrolling or moving on adds more, and
    /// scrolling back takes them away. Rows wait for indexing, since rows past those
    /// indexed so far aren't empty.
    fn reach(&self) -> Bounds {
        let imp = self.imp();
        let b = self.bounds();
        let (max_rows, max_cols) = reach_limit(b);
        let (body_w, body_h) = imp.body_size();
        let sel = imp.selection.get();
        let (cur, (_, far)) = (sel.cursor(), sel.range());
        let scroll = |adj: &RefCell<Option<gtk::Adjustment>>| {
            adj.borrow().as_ref().map_or(0.0, |a| a.value())
        };
        let sizing = imp.sizing.borrow();
        let rows = if self.is_complete() {
            let far = far.row.max(cur.row);
            reach_count(
                b.rows,
                &sizing.rows,
                scroll(&imp.vadj),
                body_h,
                far,
                max_rows,
            )
        } else {
            b.rows
        };
        let far = u64::from(far.col.max(cur.col));
        let (data, max) = (u64::from(b.cols), u64::from(max_cols));
        let cols = reach_count(data, &sizing.cols, scroll(&imp.hadj), body_w, far, max);
        Bounds {
            rows,
            cols: cols as u32,
            ..b
        }
    }

    /// The bounds a row header click and a column header click work in: the row or
    /// column clicked may be past the data ([`GridView::reach`]), and the whole row or
    /// column covers the data ([`GridView::bounds`]).
    fn header_bounds(&self) -> (Bounds, Bounds) {
        let (b, r) = (self.bounds(), self.reach());
        (Bounds { rows: r.rows, ..b }, Bounds { cols: r.cols, ..b })
    }

    /// Both axes of the body as they are scrolled and sized now: rows, then columns.
    fn with_viewports<R>(&self, f: impl FnOnce(Viewport<'_>, Viewport<'_>) -> R) -> R {
        let imp = self.imp();
        let (body_w, body_h) = imp.body_size();
        let value = |adj: &RefCell<Option<gtk::Adjustment>>| {
            adj.borrow().as_ref().map_or(0.0, |a| a.value())
        };
        let Bounds {
            rows: row_count,
            cols: col_count,
            ..
        } = self.reach();
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
                let h = self.imp().sizing.borrow().rows.default_size();
                self.set_row_height(r, h);
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
        let title = {
            let t = self.table();
            t.header_cell(c).map(|title| {
                let mut letter = String::new();
                column_name(c, &mut letter);
                let w = |s: &str| f64::from(imp.layout(s).pixel_size().0);
                w(&letter) + 6.0 + w(&title)
            })
        };
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
        let id = self.table().col_id(col);
        imp.col_widths.borrow_mut().insert(id, width);
        imp.sizing.borrow_mut().cols.set(u64::from(col), width);
    }

    /// Give row `row` a height, remembered by its identity.
    fn set_row_height(&self, row: u64, height: f64) {
        let imp = self.imp();
        let id = {
            let t = self.table();
            (row < t.row_count()).then(|| t.row_id(row))
        };
        if let Some(id) = id {
            imp.row_heights.borrow_mut().insert(id, height);
        }
        imp.sizing.borrow_mut().rows.set(row, height);
    }

    /// The active sheet's sizes as its file gives them (STYLE-3).
    fn file_ui(&self) -> SheetUi {
        file_ui_of(&self.table(), self.excel_scale_digit())
    }

    /// The width of a digit in the grid's font: Excel measures columns in it.
    fn excel_scale_digit(&self) -> f64 {
        f64::from(self.create_pango_layout(Some("0")).pixel_size().0.max(1))
    }

    /// Lay the remembered sizes out by where their rows and columns are now; those not
    /// shown (deleted) are kept for an undo.
    fn refit_sizes(&self) {
        let imp = self.imp();
        let table = self.table();
        let mut sizing = imp.sizing.borrow_mut();
        sizing.rows.clear();
        sizing.cols.clear();
        let heights = imp.row_heights.borrow();
        sizing.rows.set_all(
            heights
                .iter()
                .filter_map(|(&id, &h)| Some((table.row_of(id)?, h))),
        );
        let widths = imp.col_widths.borrow();
        sizing.cols.set_all(
            widths
                .iter()
                .filter_map(|(&id, &w)| Some((u64::from(table.col_of(id)?), w))),
        );
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
        let (b, (by_row, by_col)) = (self.bounds(), self.header_bounds());
        if b.rows == 0 || b.cols == 0 {
            return;
        }
        let (cell, top_left) = self.cell_near(x, y);
        let mut sel = imp.selection.get();
        let kind = self.with_merges(|m| match (x < imp.row_header_w.get(), y < HEADER_H) {
            (true, true) => {
                sel.select_all(b);
                None
            }
            (true, false) => {
                sel.click_row(cell.row, shift, top_left.col, by_row, m);
                Some(Drag::Rows)
            }
            (false, true) => {
                sel.click_col(cell.col, shift, top_left.row, by_col, m);
                Some(Drag::Cols)
            }
            (false, false) if shift => {
                sel.extend_to(cell, m);
                Some(Drag::Cells)
            }
            // A click on any cell of a merge selects the merge (`set_selection` fits it).
            (false, false) => {
                sel = Selection::at(cell);
                Some(Drag::Cells)
            }
        });
        self.set_selection(sel);
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
        let (b, (by_row, by_col)) = (self.bounds(), self.header_bounds());
        if b.rows == 0 || b.cols == 0 {
            return;
        }
        let (cell, _) = self.cell_near(x, y);
        let mut sel = imp.selection.get();
        let moved = self.with_merges(|m| {
            match kind {
                Drag::Cells => sel.extend_to(cell, m),
                Drag::Rows => sel.click_row(cell.row, true, 0, by_row, m),
                Drag::Cols => sel.click_col(cell.col, true, 0, by_col, m),
                Drag::Resize { .. } => return false,
            }
            true
        });
        if !moved {
            return;
        }
        if sel != imp.selection.get() {
            self.set_selection(sel);
            self.queue_draw();
        }
    }

    /// Apply a navigation key (GRID-3/GRID-4) and keep the moved cell in view.
    pub fn press(&self, key: Key, mods: Mods) {
        let imp = self.imp();
        let (bounds, reach) = (self.bounds(), self.reach());
        // End and Ctrl+End go to the last cell with data, as in Calc, not the empty cells
        // past it; End keeps the row. Whole rows and columns (Shift/Ctrl+Space) and
        // select-all cover the data. Everything else moves on past it (GRID-6).
        let within = match key {
            Key::End => {
                let (rows, cols) = (self.row_count(), self.col_count());
                Bounds {
                    rows: match (mods.ctrl, rows) {
                        (true, 0) => bounds.rows,
                        (true, rows) => rows,
                        (false, _) => reach.rows,
                    },
                    cols: if cols > 0 { cols } else { bounds.cols },
                    ..bounds
                }
            }
            Key::Space => bounds,
            _ => reach,
        };
        let mut sel = imp.selection.get();
        let reveal = self.with_merges(|m| {
            sel.press(key, mods, within, m);
            sel.reveal(m)
        });
        self.set_selection(sel);
        // Paging scrolls the view by a page as well, like Calc.
        if let (Key::PageUp | Key::PageDown, Some(vadj)) = (key, imp.vadj.borrow().as_ref()) {
            let step = bounds.page_rows as f64 * ROW_H;
            vadj.set_value(vadj.value() + if key == Key::PageDown { step } else { -step });
        }
        self.scroll_to(reveal);
        self.queue_draw();
    }

    /// Scroll the least amount that shows the given rows and columns (first and last,
    /// inclusive) whole, or at least their start when they don't fit.
    fn scroll_to(&self, (rows, cols): grid::Reveal) {
        // The range first reaches a selection that moved past the data (GRID-6).
        self.update_adjustments();
        let imp = self.imp();
        let (body_w, body_h) = imp.body_size();
        let reveal = |adj: &gtk::Adjustment, start: f64, end: f64, view: f64| {
            if start < adj.value() {
                adj.set_value(start);
            } else if end > adj.value() + view {
                adj.set_value((end - view).min(start));
            }
        };
        let sizing = imp.sizing.borrow();
        if let (Some(v), Some((first, last))) = (imp.vadj.borrow().as_ref(), rows) {
            let end = sizing.rows.start(last) + sizing.rows.size(last);
            reveal(v, sizing.rows.start(first), end, body_h);
        }
        if let (Some(hz), Some((first, last))) = (imp.hadj.borrow().as_ref(), cols) {
            let (first, last) = (u64::from(first), u64::from(last));
            let end = sizing.cols.start(last) + sizing.cols.size(last);
            reveal(hz, sizing.cols.start(first), end, body_w);
        }
    }

    /// Scroll to show `cell`, all of its merge if it is in one.
    fn reveal_cell(&self, cell: grid::Cell) {
        let spans = self.with_merges(|m| Selection::at(cell).reveal(m));
        self.scroll_to(spans);
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
        self.set_selection(Selection::at(cell));
        // On a merge, the anchor: the cell that holds the merge's text.
        let cell = self.imp().selection.get().cursor();
        let value = self.cell_text(cell);
        self.open_editor(cell, &value, Mode::Editing);
    }

    /// F2 or typing on the cursor cell (EDIT-2). The selection stays, so Enter and Tab
    /// after the edit move within a selected range, as in Calc.
    fn start_edit(&self, start: Start) {
        let cell = self.imp().selection.get().cursor();
        let b = self.reach();
        if cell.row >= b.rows || cell.col >= b.cols {
            return;
        }
        match start {
            Start::Keep => self.open_editor(cell, &self.cell_text(cell), Mode::Editing),
            Start::Replace(c) => self.open_editor(cell, &c.to_string(), Mode::Typing),
        }
    }

    fn cell_text(&self, cell: grid::Cell) -> String {
        self.table()
            .cell_value(cell.row, cell.col)
            .map(|v| v.into_owned())
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
        self.reveal_cell(cell);
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
        // On a merge the editor covers all of it.
        let cell = grid::Cell { row, col };
        let (tl, br) = self.with_merges(|m| m.at(cell)).unwrap_or((cell, cell));
        let (x, y, w, h) = self.with_viewports(|rows, cols| {
            let (c0, c1) = (u64::from(tl.col), u64::from(br.col));
            let (x, y) = (rh_w + cols.pos(c0), HEADER_H + rows.pos(tl.row));
            (
                x,
                y,
                rh_w + cols.pos(c1) + cols.size(c1) - x,
                HEADER_H + rows.pos(br.row) + rows.size(br.row) - y,
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
                        if g.commit_edit(true) {
                            g.press(key, mods);
                        }
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
                    // Quietly: a formula with an error stays in the editor to be fixed.
                    g.commit_edit(false);
                }
            }
        });
        entry.add_controller(focus);
        entry
    }

    /// Commit an open edit, if any; for Ctrl+S, which saves what was typed (as Calc does).
    pub fn finish_editing(&self) {
        self.commit_edit(true);
    }

    /// Apply the editor's text to its cell as an undoable command, then close the editor.
    /// A formula that doesn't parse is refused: the editor stays open and, if `report`,
    /// `formula-error` says where it fails. Whether the edit was applied (or there was
    /// none open).
    fn commit_edit(&self, report: bool) -> bool {
        let imp = self.imp();
        let Some((row, col, _)) = imp.editing.get() else {
            return true;
        };
        let text = imp
            .editor
            .borrow()
            .as_ref()
            .map(|e| e.text().to_string())
            .unwrap_or_default();
        if let Err(message) = self.apply_text(row, col, text) {
            if report {
                self.emit_by_name::<()>("formula-error", &[&message]);
            }
            return false;
        }
        self.close_editor();
        self.update_description();
        true
    }

    /// Give cell (`row`, `col`) the raw text `text` as one undoable step (EDIT-2, FML-9):
    /// a row past the data grows the table to it (SAVE-2, GRID-6); empty text there
    /// changes nothing. A formula that doesn't parse is refused,
    /// saying where: "Formula error at character N: …" (counting its `=` as 1).
    fn apply_text(&self, row: u64, col: u32, text: String) -> Result<(), String> {
        let imp = self.imp();
        if is_formula(&text) {
            if let Err(e) = formula::parse(&text[1..]) {
                return Err(format!(
                    "Formula error at character {}: {}",
                    e.at + 2,
                    e.message
                ));
            }
        }
        let text_empty = text.is_empty();
        let mut grew = false;
        let cmd: Option<Box<dyn Command<CsvTable>>> = {
            let mut table = self.table_mut();
            if row >= table.row_count() && text_empty {
                None
            } else if row >= table.row_count() {
                // Past the data: add the rows up to this one and set the cell, as one step.
                let rows = table.row_count();
                match table.paste(row, col, &[vec![text]]) {
                    Ok(edits) if !edits.is_empty() => {
                        let at = CellRef {
                            row: added_row(&edits, rows, row).unwrap_or(RowId::source(0)),
                            col: table.col_id(col),
                        };
                        grew = true;
                        Some(Box::new(Batch::new("Edit cell", edits, at)))
                    }
                    _ => None,
                }
            } else if table.cell_value(row, col).as_deref().unwrap_or("") != text {
                // A cell past its row's end shows empty: empty text leaves it so.
                let at = CellRef {
                    row: table.row_id(row),
                    col: table.col_id(col),
                };
                Some(Box::new(SetCell::new(at, text)))
            } else {
                None
            }
        };
        if let Some(cmd) = cmd {
            self.execute(cmd);
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
        self.emit_by_name::<()>("changed", &[]);
        self.queue_draw();
        Ok(())
    }

    /// The formula bar's Enter (FML-9): the cursor cell gets `text`, as typing it in the
    /// cell would. A formula that doesn't parse is refused with where it fails.
    pub fn set_cell_text(&self, text: &str) -> Result<(), String> {
        if self.busy() {
            return Err("Wait for the running job to finish".to_owned());
        }
        if !self.commit_edit(true) {
            return Err("Finish the cell being edited first".to_owned());
        }
        let cur = self.imp().selection.get().cursor();
        self.apply_text(cur.row, cur.col, text.to_owned())?;
        self.update_description();
        Ok(())
    }

    /// The cursor cell's raw text: its formula, not its result (FML-9).
    pub fn cell_raw(&self) -> String {
        let cur = self.imp().selection.get().cursor();
        self.cell_text(cur)
    }

    /// Why the cursor cell shows what it does, when that isn't its formula's result
    /// (FML-9): a result the file stored, for functions Brick doesn't compute.
    pub fn cell_note(&self) -> Option<String> {
        let cur = self.imp().selection.get().cursor();
        let book = self.imp().book.borrow();
        let t = book.active();
        if cur.row >= t.row_count() {
            return None;
        }
        let at = CellRef {
            row: t.row_id(cur.row),
            col: t.col_id(cur.col),
        };
        let names = book.uses_cached(book.active_id(), at)?;
        Some(format!(
            "Result from the file: Brick doesn't compute {} yet",
            names.join(", ")
        ))
    }

    /// The selection as the name box shows it (FML-9): `B2`, `B2:D9`, `A:C`, or `2:5`,
    /// rows as references count them (file rows, from 1).
    pub fn selection_reference(&self) -> String {
        let imp = self.imp();
        let sel = imp.selection.get();
        let (tl, br) = sel.range();
        let t = self.table();
        let row = |r: u64| row_number(&t, r) - 1;
        let cell = |r: u64, c: u32| formula::CellAddr {
            row: row(r),
            col: c,
            row_abs: false,
            col_abs: false,
        };
        let area = if let Some(cols) = sel.whole_columns() {
            formula::Area::Cols {
                first: cols.start,
                last: cols.end.saturating_sub(1).max(cols.start),
                first_abs: false,
                last_abs: false,
            }
        } else if sel.is_whole_rows() {
            formula::Area::Rows {
                first: row(tl.row),
                last: row(br.row),
                first_abs: false,
                last_abs: false,
            }
        } else if tl == br {
            formula::Area::Cell(cell(tl.row, tl.col))
        } else {
            formula::Area::Range(cell(tl.row, tl.col), cell(br.row, br.col))
        };
        formula::format_reference(&formula::Reference { sheet: None, area })
    }

    /// Go to a reference typed in the name box (FML-9): its sheet first if it names one,
    /// then the cell or area, selected and scrolled to; whole rows and columns select as
    /// clicking their headers does.
    pub fn select_reference(&self, r: &formula::Reference) -> Result<(), &'static str> {
        if let Some(name) = &r.sheet {
            let id = self
                .imp()
                .book
                .borrow()
                .sheet_named(name)
                .map(|s| s.id)
                .ok_or("No sheet named that")?;
            if !self.switch_sheet(id) {
                return Err("Wait for the running job to finish");
            }
        }
        let table_row = |position: u64| -> Result<u64, &'static str> {
            let t = self.table();
            if position >= t.total_rows() {
                // Past the data: the edge row and beyond are rows of their own.
                return Ok(t.row_count() + (position - t.total_rows()));
            }
            t.row_of(t.id_at(position)).ok_or("That row isn't shown")
        };
        let b = self.bounds();
        // Any cell past the data once indexing is done (GRID-6), up to `reach_limit`; until
        // then rows stop at those indexed. Columns are known only as far as rows have
        // loaded (none just after a sheet switch), so only the limit cuts them. Whole rows
        // and columns cover the data, as clicking their headers does.
        let (max_rows, max_cols) = reach_limit(b);
        let last = if self.is_complete() {
            max_rows - 1
        } else {
            b.rows.saturating_sub(1)
        };
        let by_row = Bounds {
            rows: last + 1,
            ..b
        };
        let by_col = Bounds {
            cols: max_cols,
            ..b
        };
        let cell = |a: &formula::CellAddr| -> Result<grid::Cell, &'static str> {
            Ok(grid::Cell {
                row: table_row(a.row)?.min(last),
                col: a.col.min(max_cols - 1),
            })
        };
        let mut sel = self.imp().selection.get();
        let reveal = self.with_merges(|m| -> Result<_, &'static str> {
            match &r.area {
                formula::Area::Cell(a) => {
                    sel = Selection::at(cell(a)?);
                    sel.fit(m);
                }
                formula::Area::Range(a, z) => {
                    sel = Selection::at(cell(a)?);
                    sel.extend_to(cell(z)?, m);
                }
                formula::Area::Cols { first, last, .. } => {
                    sel.click_col(*first, false, 0, by_col, m);
                    sel.click_col(*last, true, 0, by_col, m);
                }
                formula::Area::Rows { first, last, .. } => {
                    let (first, last) = (table_row(*first)?, table_row(*last)?);
                    sel.click_row(first, false, 0, by_row, m);
                    sel.click_row(last, true, 0, by_row, m);
                }
            }
            Ok(sel.reveal(m))
        })?;
        self.set_selection(sel);
        self.scroll_to(reveal);
        self.grab_focus();
        self.queue_draw();
        Ok(())
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
        // Undo shows the sheet the step happened on first (ADR 0008).
        let sheet = {
            let undo = imp.undo.borrow();
            let next = if back {
                undo.peek_undo()
            } else {
                undo.peek_redo()
            };
            let Some(next) = next else { return false };
            next.sheet()
        };
        let exists = |id: SheetId| imp.book.borrow().sheet(id).is_some();
        if let Some(id) = sheet.filter(|&id| exists(id)) {
            if !self.switch_sheet(id) {
                return false;
            }
        }
        let focus = {
            let (mut book, mut undo) = (imp.book.borrow_mut(), imp.undo.borrow_mut());
            let cmd = if back {
                undo.undo(&mut book)
            } else {
                undo.redo(&mut book)
            };
            let Some(cmd) = cmd else { return false };
            cmd.focus()
        };
        // A step that added or removed a sheet can leave another one active; then the
        // step's sheet shows, if it is there now (a redone Add Sheet).
        self.follow_active();
        if let Some(id) = sheet.filter(|&id| exists(id)) {
            self.switch_sheet(id);
        }
        let focus = focus.and_then(|at| {
            let table = self.table();
            let row = table.row_of(at.row)?;
            let col = table.col_of(at.col)?;
            Some(grid::Cell { row, col })
        });
        self.after_change(focus);
        true
    }

    /// Redraw after the data changed: cached rows and titles reload (from scratch: a
    /// column delete can narrow the widest row), the remembered sizes follow their rows
    /// and columns, the scroll range follows the row count, and the cursor goes to
    /// `focus` if given (else stays; while indexing, clamped to the rows indexed).
    fn after_change(&self, focus: Option<grid::Cell>) {
        let imp = self.imp();
        imp.cache.borrow_mut().reset();
        imp.titles.take();
        self.refit_sizes();
        self.update_adjustments();
        // Past the data is a place too once indexing is done (GRID-6); until then the
        // rows stop at those indexed so far.
        let last = if self.is_complete() {
            u64::MAX
        } else {
            self.bounds().rows.saturating_sub(1)
        };
        let cursor = focus.unwrap_or_else(|| imp.selection.get().cursor());
        if focus.is_some() || cursor.row > last {
            let cell = grid::Cell {
                row: cursor.row.min(last),
                col: cursor.col,
            };
            self.set_selection(Selection::at(cell));
            self.reveal_cell(cell);
        }
        self.update_description();
        self.emit_by_name::<()>("changed", &[]);
        self.queue_draw();
    }

    /// Ctrl++ (EDIT-3): as many empty rows as the selection spans, above it.
    pub fn insert_rows_above(&self) {
        self.insert_rows(false);
    }

    pub fn insert_rows_below(&self) {
        self.insert_rows(true);
    }

    /// Rows inserted past the data would only add empty lines to the file: there is
    /// nothing below them to move down (GRID-6).
    fn insert_rows(&self, below: bool) {
        let Some((first, count)) = self.selected_rows() else {
            return;
        };
        let at = if below { first + count } else { first };
        let edit = {
            let mut t = self.table_mut();
            let rows = t.row_count();
            if first >= rows {
                return;
            }
            t.insert_rows(at.min(rows), count)
        };
        self.reshape(edit);
    }

    /// Ctrl+- (EDIT-3): delete the rows the selection spans.
    pub fn delete_rows(&self) {
        let Some((first, count)) = self.selected_rows() else {
            return;
        };
        let edit = {
            let t = self.table();
            t.delete_rows(first, count.min(t.row_count().saturating_sub(first)))
        };
        self.reshape(edit);
    }

    /// The rows the selection spans (first, count), a merge's rows whole. `None` for whole
    /// columns: Ctrl+- there deletes the columns.
    fn selected_rows(&self) -> Option<(u64, u64)> {
        let sel = self.imp().selection.get();
        if sel.whole_columns().is_some() {
            return None;
        }
        let (tl, br) = self.selected_area();
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

    /// Columns past the data have nothing right of them to move, so nothing happens.
    fn insert_cols(&self, right: bool) {
        let (first, count) = self.selected_cols();
        if first >= self.col_count() {
            return;
        }
        let at = if right { first + count } else { first };
        let edit = self.table_mut().insert_cols(at, count);
        self.reshape(edit);
    }

    /// Ctrl+- on whole columns (EDIT-4): delete the selected columns.
    pub fn delete_cols(&self) {
        let (first, count) = self.selected_cols();
        // Not the empty edge column: there is nothing there to delete.
        let count = count.min(self.col_count().saturating_sub(first));
        let edit = self.table().delete_cols(first, count);
        self.reshape(edit);
    }

    /// The columns the selection spans (first, count), a merge's columns whole.
    fn selected_cols(&self) -> (u32, u32) {
        let (tl, br) = self.selected_area();
        (tl.col, br.col - tl.col + 1)
    }

    /// The selected rectangle grown over the merges it touches (STYLE-4).
    fn selected_area(&self) -> grid::Area {
        let sel = self.imp().selection.get();
        self.with_merges(|m| sel.area(m))
    }

    /// The selection (merges whole) as positions among all rows and view columns: what
    /// merges are kept in. `None` on the empty edge row alone.
    fn selected_positions(&self) -> Option<CellArea> {
        let (tl, br) = self.selected_area();
        let t = self.table();
        let n = t.row_count();
        (tl.row < n).then(|| CellArea {
            first_row: t.position(tl.row),
            first_col: tl.col,
            last_row: t.position(br.row.min(n - 1)),
            last_col: br.col,
        })
    }

    /// Merge Cells (STYLE-4): merge the selected range as one undoable step. Merges it
    /// touches are absorbed, and any text but the top-left cell's is emptied, after
    /// asking. Refused while rows are sorted or filtered: merges keep file order.
    pub fn merge_selection(&self) {
        let imp = self.imp();
        if self.busy() || imp.editing.get().is_some() {
            return;
        }
        let Some(area) = self.selected_positions() else {
            return;
        };
        let plan = self.table_mut().merge_plan(area);
        let refuse = |title: &str, why: &str| {
            self.emit_by_name::<()>("refused", &[&title, &why]);
        };
        match plan {
            Err(MergeError::OneCell) => {}
            Err(MergeError::NotInFileOrder) => refuse(
                "Can't merge cells",
                "Merge cells in file order: clear the sort and filters first.",
            ),
            Err(MergeError::TooManyCells(n)) => refuse(
                "Too many cells to merge",
                &format!(
                    "This range has {} cells; Brick merges at most {} at once.",
                    crate::status::group_digits(n),
                    crate::status::group_digits(MERGE_MAX_CELLS)
                ),
            ),
            Ok(plan) if !plan.drops_text => self.apply_merge(plan),
            Ok(plan) => {
                let dialog = gtk::AlertDialog::builder()
                    .message("Merge cells?")
                    .detail("Merging keeps only the top-left value.")
                    .buttons(["Cancel", "Merge"])
                    .cancel_button(0)
                    .default_button(1)
                    .modal(true)
                    .build();
                let window = self.root().and_downcast::<gtk::Window>();
                let revision = self.revision();
                let g = self.downgrade();
                dialog.choose(window.as_ref(), gio::Cancellable::NONE, move |answer| {
                    let Some(g) = g.upgrade() else { return };
                    // Nothing can change under a modal dialog but a job finishing; then
                    // the plan may be stale, and the merge waits for another try.
                    if answer == Ok(1) && g.revision() == revision && !g.busy() {
                        g.apply_merge(plan);
                    }
                });
            }
        }
    }

    fn apply_merge(&self, plan: MergePlan) {
        let (focus, cell) = {
            let t = self.table();
            let focus = CellRef {
                row: t.id_at(plan.area.first_row),
                col: t.col_id(plan.area.first_col),
            };
            let row = t.merge_rows(&plan.area).map_or(0, |rows| rows.start);
            let cell = grid::Cell {
                row,
                col: plan.area.first_col,
            };
            (focus, cell)
        };
        self.execute(Box::new(Batch::new("Merge cells", plan.edits, focus)));
        self.after_change(Some(cell));
    }

    /// Unmerge Cells (STYLE-4): unmerge every merge the selection touches, as one
    /// undoable step.
    pub fn unmerge_selection(&self) {
        let imp = self.imp();
        if self.busy() || imp.editing.get().is_some() {
            return;
        }
        let Some(area) = self.selected_positions() else {
            return;
        };
        let (edit, focus) = {
            let t = self.table();
            let Some(edit) = t.unmerge(area) else {
                return;
            };
            let focus = CellRef {
                row: t.id_at(area.first_row),
                col: t.col_id(area.first_col),
            };
            (edit, focus)
        };
        self.execute(Box::new(Batch::new("Unmerge cells", vec![edit], focus)));
        self.after_change(None);
    }

    /// Format the selection with `patch` as one undoable step (STYLE-6): whole columns or
    /// rows when they are selected so (their column or row styles, and the cells with
    /// their own style there), else the selected cells, merges whole. Cells and whole rows
    /// past the data get rows to hold their formats, added at the end in the same step
    /// (GRID-6). A cell range over [`REPLACE_MAX_CELLS`] cells is refused. Whole rows or
    /// columns of a sheet with many style runs are worked out on the job pool
    /// ("Formatting"; edits wait).
    pub fn format_selection(&self, patch: &StylePatch) {
        let imp = self.imp();
        if self.busy() || imp.editing.get().is_some() {
            return;
        }
        let sel = imp.selection.get();
        let (tl, br) = self.selected_area();
        let width = self.col_count();
        let (target, focus, pad) = {
            let mut t = self.table_mut();
            let n = t.row_count();
            let pad = if sel.whole_columns().is_none() && br.row >= n {
                t.insert_rows(n, br.row + 1 - n)
            } else {
                None
            };
            let rows = tl.row..br.row + 1;
            let target = match (sel.whole_columns(), sel.is_whole_rows()) {
                (Some(_), true) => FormatTarget::Cols(0..width),
                (Some(_), false) => FormatTarget::Cols(tl.col..br.col + 1),
                (None, true) => FormatTarget::Rows {
                    rows,
                    cols: 0..width,
                },
                (None, false) => FormatTarget::Cells {
                    rows,
                    cols: tl.col..br.col + 1,
                },
            };
            let cur = sel.cursor();
            let added = pad
                .as_ref()
                .and_then(|p| added_row(std::slice::from_ref(p), n, cur.row));
            let focus = CellRef {
                row: if cur.row < n {
                    t.row_id(cur.row)
                } else {
                    added.unwrap_or(RowId::source(0))
                },
                col: t.col_id(cur.col),
            };
            (target, focus, pad)
        };
        let (sheet, runs) = {
            let book = imp.book.borrow();
            (book.active_id(), book.active().style_runs())
        };
        if runs > FORMAT_JOB_RUNS && !matches!(target, FormatTarget::Cells { .. }) {
            let mut snapshot = imp.book.borrow().snapshot();
            let patch = patch.clone();
            let padding = pad.clone();
            let job = jobs::spawn(move |_| {
                if let Some(pad) = padding {
                    pad_sheet(&mut snapshot, sheet, pad);
                }
                let edit = snapshot.format_edit(sheet, target, &patch);
                edit.map(|e| (e, snapshot.styles().clone()))
            });
            imp.formatting.replace(Some(job.clone()));
            let revision = self.revision();
            let g = self.downgrade();
            glib::spawn_future_local(async move {
                let result = job.finished().await;
                let Some(g) = g.upgrade() else { return };
                let imp = g.imp();
                if !matches!(&*imp.formatting.borrow(), Some(j) if j.is(&job)) {
                    return;
                }
                imp.formatting.take();
                // Edits waited, so nothing changed; a new workbook makes it moot.
                let Ok(result) = result else { return };
                if g.revision() != revision {
                    return;
                }
                let result = result.map(|(edit, pool)| {
                    imp.book.borrow_mut().set_styles(Arc::unwrap_or_clone(pool));
                    edit
                });
                g.finish_format(result, pad, focus);
            });
            return;
        }
        let edit = match &pad {
            None => imp.book.borrow_mut().format_edit(sheet, target, patch),
            Some(pad) => {
                // Worked out on a copy that has the rows; its new style records come back.
                let mut padded = imp.book.borrow().snapshot();
                pad_sheet(&mut padded, sheet, pad.clone());
                let edit = padded.format_edit(sheet, target, patch);
                if edit.is_ok() {
                    let pool = Arc::unwrap_or_clone(padded.styles().clone());
                    imp.book.borrow_mut().set_styles(pool);
                }
                edit
            }
        };
        self.finish_format(edit, pad, focus);
    }

    /// Run a format edit as one undoable step, after `pad` (rows added for it, GRID-6), or
    /// say why there is none.
    fn finish_format(&self, edit: Result<Edit, FormatError>, pad: Option<Edit>, focus: CellRef) {
        let refuse = |title: &str, why: &str| {
            self.emit_by_name::<()>("refused", &[&title, &why]);
        };
        match edit {
            Err(FormatError::TooMany(n)) => refuse(
                "Too many cells to format",
                &format!(
                    "This range has {} cells; Brick formats at most {} at once. Select whole \
                     columns or rows to format all of them.",
                    crate::status::group_digits(n),
                    crate::status::group_digits(REPLACE_MAX_CELLS)
                ),
            ),
            Err(FormatError::BadNumberFormat) => refuse(
                "Can't use that number format",
                "Excel doesn't accept this number format code.",
            ),
            Ok(Edit::Styles(e)) if e.is_empty() => {}
            Ok(edit) => {
                let edits = pad.into_iter().chain([edit]).collect();
                self.execute(Box::new(Batch::new("Format cells", edits, focus)));
                self.after_change(None);
            }
        }
    }

    /// Ctrl+B, Ctrl+I, Ctrl+U, Ctrl+5: turn the font style on for the selection, or off
    /// when the cursor cell has it, as in Excel.
    pub fn toggle_font(&self, toggle: FontToggle) {
        let font = self.cursor_format().font;
        let patch = match toggle {
            FontToggle::Bold => StylePatch {
                bold: Some(!font.bold),
                ..StylePatch::default()
            },
            FontToggle::Italic => StylePatch {
                italic: Some(!font.italic),
                ..StylePatch::default()
            },
            FontToggle::Underline => StylePatch {
                underline: Some(if font.underline == Underline::None {
                    Underline::Single
                } else {
                    Underline::None
                }),
                ..StylePatch::default()
            },
            FontToggle::Strike => StylePatch {
                strike: Some(!font.strike),
                ..StylePatch::default()
            },
        };
        self.format_selection(&patch);
    }

    /// The cursor cell's format, for the format bar to show.
    pub fn cursor_format(&self) -> CursorFormat {
        let book = self.imp().book.borrow();
        let t = book.active();
        let cur = self.imp().selection.get().cursor();
        let col = t.col_id(cur.col);
        let style = if cur.row < t.row_count() {
            t.style_of(CellRef {
                row: t.row_id(cur.row),
                col,
            })
        } else {
            t.column_style(col)
        };
        let pool = book.styles();
        let xf = pool.xf(style).clone();
        let font = pool.font(&xf).clone();
        let fill = match pool.fill(&xf) {
            Some(Fill::Pattern {
                pattern: Pattern::Solid,
                fg,
                ..
            }) => pool.rgb(*fg),
            Some(Fill::Gradient { first }) => pool.rgb(*first),
            _ => None,
        };
        let code = pool
            .custom_format(xf.num_fmt)
            .or_else(|| formula::builtin_format(xf.num_fmt))
            .unwrap_or("General")
            .to_owned();
        CursorFormat {
            color: pool.rgb(font.color),
            font,
            fill,
            code,
            xf,
        }
    }

    /// The workbook has formatting a CSV file can't keep (STYLE-6): a CSV session then
    /// saves only as .xlsx.
    pub fn has_formatting(&self) -> bool {
        self.imp().book.borrow().has_formatting()
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
        let cursor = {
            let table = self.table();
            let cur = imp.selection.get().cursor();
            CellRef {
                row: if cur.row < table.row_count() {
                    table.row_id(cur.row)
                } else {
                    RowId::source(0)
                },
                col: table.col_id(cur.col),
            }
        };
        let (sheet, formulas) = {
            let book = imp.book.borrow();
            let any = book.sheets().iter().any(|s| s.table.has_formulas());
            (book.active_id(), any)
        };
        // Merged cells move, grow, or shrink with the rows and columns (STYLE-4), in the
        // same undo step. Edits wait while formulas are rewritten, so the list holds.
        let merges = self.table().merges_after(&edit);
        if !formulas {
            self.execute(Box::new(Reshape::new(edit, cursor).with_merges(merges)));
            self.after_change(None);
            return;
        }
        // Formulas follow the rows and columns, in the same undo step (FML-10).
        let label = Reshape::label_of(&edit);
        let structural = edit.clone();
        self.with_rewrites(
            move |book, cancel| book.structure_rewrites(sheet, &structural, cancel),
            move |g, rewrites| {
                let mut on_sheet = vec![(sheet, edit)];
                on_sheet.extend(merges.map(|m| (sheet, m)));
                let mut edits = vec![BookEdit::Sheets(on_sheet)];
                edits.extend(rewrites);
                g.execute_book(SheetEdit::new(label, edits, Some(sheet)).with_focus(cursor));
            },
        );
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
        let (edit, cursor) = {
            let table = self.table();
            let rows = match sel.whole_columns() {
                Some(_) if !table.is_complete() => return,
                Some(_) => tl.row..table.row_count(),
                None => tl.row..(br.row + 1).min(table.row_count()),
            };
            let (first, end) = (rows.start, rows.end);
            let edit = if sel.is_whole_rows() {
                table.clear_cells(rows, tl.col..)
            } else {
                table.clear_cells(rows, tl.col..br.col + 1)
            };
            let Some(edit) = edit else { return };
            // The cursor may be past the data (GRID-6); the focus is a cleared row.
            let cur = sel.cursor();
            let cursor = CellRef {
                row: table.row_id(cur.row.clamp(first, end - 1)),
                col: table.col_id(cur.col),
            };
            (edit, cursor)
        };
        self.execute(Box::new(ClearCells::new(edit, cursor)));
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
        let rows = {
            let table = self.table();
            let rows = match sel.whole_columns() {
                Some(_) if !table.is_complete() => return,
                Some(_) => tl.row..table.row_count(),
                None => tl.row..(br.row + 1).min(table.row_count()),
            };
            if rows.is_empty() {
                return;
            }
            rows
        };
        let get = self.job_table();
        let pool = imp.book.borrow().styles().clone();
        let done = Arc::new(AtomicU64::new(0));
        let whole_rows = sel.is_whole_rows();
        let count = rows.end - rows.start;
        let job = jobs::spawn({
            let done = done.clone();
            move |cancel| {
                // Formula cells copy as their results (FML-11), and their formulas are kept
                // for a paste back into Brick (FML-12), as are their styles (STYLE-7).
                let mut snapshot = get(cancel)?;
                let copied = if whole_rows {
                    snapshot.copy_range_styled(rows.clone(), tl.col.., &pool, cancel.flag(), &done)
                } else {
                    let cols = tl.col..br.col + 1;
                    snapshot.copy_range_styled(rows.clone(), cols, &pool, cancel.flag(), &done)
                }?;
                let styled = !snapshot.file_styles().is_empty()
                    || snapshot.has_user_styles()
                    || pool.xfs.len() > 1;
                let styles = (copied.cells <= PASTE_MAX_CELLS && styled).then(|| {
                    let width = if whole_rows {
                        let widest = copied.tsv.lines().map(|l| l.split('\t').count()).max();
                        (widest.unwrap_or(0) as u32).max(snapshot.styled_width())
                    } else {
                        br.col - tl.col + 1
                    };
                    snapshot.copy_styles(rows.clone(), tl.col..tl.col + width)
                });
                let last_col = if whole_rows { u32::MAX } else { br.col };
                let formulas = if copied.cells <= PASTE_MAX_CELLS && snapshot.has_formulas() {
                    snapshot.set_computed(None);
                    clip_formulas(&mut snapshot, rows, tl.col..=last_col, cancel)?
                } else {
                    Vec::new()
                };
                Some((copied, formulas, styles))
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
            let Some((copied, formulas, styles)) = copied else {
                return;
            };
            imp.formula_clip
                .replace(
                    (!formulas.is_empty() || styles.is_some()).then(|| FormulaClip {
                        text: copied.tsv.clone(),
                        cells: formulas,
                        styles,
                    }),
                );
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
        self.set_selection(Selection::at(cell));
        self.paste_text(text);
    }

    fn paste_text(&self, text: &str) {
        let imp = self.imp();
        if self.busy() || imp.editing.get().is_some() {
            return;
        }
        let mut cells = parse_tsv(text);
        let width = cells.iter().map(Vec::len).max().unwrap_or(0) as u32;
        if width == 0 {
            return;
        }
        let (tl, _) = imp.selection.get().range();
        // What this window copied pastes back as formulas, their relative references moved
        // as far as the cells did (FML-12), and with its styles (STYLE-7); text from
        // anywhere else pastes as values and keeps the cells' own formats.
        let mut styles = None;
        if let Some(clip) = imp
            .formula_clip
            .borrow()
            .as_ref()
            .filter(|c| c.text == text)
        {
            styles = clip.styles.clone();
            let t = self.table();
            let position = |row: u64| {
                let n = t.row_count();
                if row < n {
                    t.position(row)
                } else {
                    t.total_rows() + (row - n)
                }
            };
            for f in &clip.cells {
                let Some(slot) = cells
                    .get_mut(f.row as usize)
                    .and_then(|r| r.get_mut(f.col as usize))
                else {
                    continue;
                };
                let to = (position(tl.row + f.row), tl.col + f.col);
                let rows = to.0 as i64 - f.from.0 as i64;
                let cols = i64::from(to.1) - i64::from(f.from.1);
                *slot = formula::shift_formula(&f.raw, rows, cols).unwrap_or_else(|| f.raw.clone());
            }
        }
        let (note, cmd) = {
            let mut table = self.table_mut();
            // Past the data too (GRID-6): the rows up to it are added.
            let (at, rows) = (tl.row, table.row_count());
            match table.paste(at, tl.col, &cells) {
                Ok(mut edits) => {
                    if let Some(block) = &styles {
                        edits.extend(table.paste_styles(&edits, at, tl.col, block));
                    }
                    let cmd = (!edits.is_empty()).then(|| {
                        let row = if at < rows {
                            table.row_id(at)
                        } else {
                            added_row(&edits, rows, at).unwrap_or(RowId::source(0))
                        };
                        let focus = CellRef {
                            row,
                            col: table.col_id(tl.col),
                        };
                        Batch::new("Paste", edits, focus)
                    });
                    let pasted = cells.iter().map(|r| r.len() as u64).sum();
                    (ClipView::Pasted(pasted), cmd)
                }
                Err(PasteError::TooLarge(n)) => (
                    ClipView::PasteTooLarge {
                        cells: n,
                        max: PASTE_MAX_CELLS,
                    },
                    None,
                ),
                Err(PasteError::NotIndexed) => (ClipView::PasteNeedsIndex, None),
            }
        };
        if let Some(cmd) = cmd {
            self.execute(Box::new(cmd));
        }
        imp.clip_note.set(Some((note, Instant::now())));
        if !matches!(note, ClipView::Pasted(_)) {
            return;
        }
        let anchor = tl;
        self.after_change(Some(anchor));
        let mut sel = Selection::at(anchor);
        let end = grid::Cell {
            row: anchor.row + cells.len() as u64 - 1,
            col: anchor.col + width - 1,
        };
        self.with_merges(|m| sel.extend_to(end, m));
        self.set_selection(sel);
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
                let merge = gio::Menu::new();
                merge.append(Some("Merge Cells"), Some("grid.merge"));
                merge.append(Some("Unmerge Cells"), Some("grid.unmerge"));
                let format = gio::Menu::new();
                format.append(Some("Bold"), Some("grid.bold"));
                format.append(Some("Italic"), Some("grid.italic"));
                format.append(Some("Underline"), Some("grid.underline"));
                format.append(Some("Strikethrough"), Some("grid.strikethrough"));
                format.append(Some("Clear Formatting"), Some("grid.clear-formatting"));
                let model = gio::Menu::new();
                model.append_section(None, &rows);
                model.append_section(None, &cols);
                model.append_section(None, &sort);
                model.append_section(None, &merge);
                model.append_submenu(Some("Format"), &format);
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
        let (rows, merged) = {
            let t = self.table();
            if !t.is_complete() {
                return;
            }
            (t.index().row_count(), t.merges_block_sort())
        };
        // Sorting would split merged cells across rows (STYLE-4): Excel refuses too.
        if merged {
            self.emit_by_name::<()>(
                "refused",
                &[&"Can't sort these rows", &"Unmerge cells before sorting."],
            );
            return;
        }
        let get = self.job_table();
        let done = Arc::new(AtomicU64::new(0));
        let started = Instant::now();
        let job = jobs::spawn({
            let done = done.clone();
            move |cancel| -> Result<Sorted, SortError> {
                // A formula column sorts by its results (FML-11).
                let mut snapshot = get(cancel).ok_or(SortError::Cancelled)?;
                let edit = snapshot.sort_rows(col, order, cancel.flag(), &done)?;
                // A moved formula's relative references follow it, as in Excel (FML-10).
                let rewrites = match &edit {
                    Some(Edit::Reorder(new)) if snapshot.has_formulas() => {
                        snapshot.set_computed(None);
                        snapshot.sort_rewrites(new)
                    }
                    _ => Vec::new(),
                };
                Ok(edit.map(|e| (e, rewrites)))
            }
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
            if std::env::var_os("BRICK_TIMINGS").is_some() {
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
            let Ok(Ok(Some((edit, rewrites)))) = result else {
                return;
            };
            if rewrites.len() as u64 > data_model::REPLACE_MAX_CELLS {
                let why = format!(
                    "Sorting would rewrite {} formulas; Brick rewrites at most {} at once.",
                    crate::status::group_digits(rewrites.len() as u64),
                    crate::status::group_digits(data_model::REPLACE_MAX_CELLS)
                );
                g.emit_by_name::<()>("refused", &[&"Too many formulas to rewrite", &why]);
                return;
            }
            let focus = {
                let table = g.table();
                CellRef {
                    row: table.row_id(0),
                    col: table.col_id(col),
                }
            };
            let mut edits = vec![edit];
            if !rewrites.is_empty() {
                edits.push(Edit::Cells(rewrites));
            }
            g.execute(Box::new(Batch::new("Sort", edits, focus)));
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
        if imp.rewriting.borrow().is_some() {
            // No count to show: these jobs are short.
            return Some(("Updating formulas", f64::NAN));
        }
        if imp.formatting.borrow().is_some() {
            return Some(("Formatting", f64::NAN));
        }
        if let Some(f) = imp.filtering.borrow().as_ref() {
            return Some(("Filtering", share(&f.done, f.rows)));
        }
        if let Some(r) = imp.replacing.borrow().as_ref() {
            return Some(("Replacing", share(&r.progress.rows, r.rows)));
        }
        if let Some(s) = imp.searching.borrow().as_ref() {
            return Some(("Searching", share(&s.progress.rows, s.rows)));
        }
        if let Some(s) = imp.scanning.borrow().as_ref() {
            return Some(("Scanning for formulas", share(&s.done, s.rows)));
        }
        let calculating = imp.calculating.borrow();
        let c = calculating.as_ref()?;
        Some(("Calculating", share(&c.done, c.total)))
    }

    /// A save, sort, filter, search, or replace is running: the table must not change
    /// under it.
    pub fn busy(&self) -> bool {
        let imp = self.imp();
        imp.saving.get()
            || imp.loading.get()
            || imp.rewriting.borrow().is_some()
            || imp.formatting.borrow().is_some()
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
        let get = self.job_table();
        self.cancel_search();
        let progress = Arc::new(SearchProgress::default());
        let counting = known.is_none();
        let started = Instant::now();
        let job = jobs::spawn({
            let progress = progress.clone();
            move |cancel| {
                // Find looks at what cells show: formula results (FML-11).
                let mut snapshot = get(cancel).ok_or(SearchError::Cancelled)?;
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
            if std::env::var_os("BRICK_TIMINGS").is_some() {
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
                    g.set_selection(Selection::at(cell));
                    g.reveal_cell(cell);
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
        let t = self.table();
        if !t.is_complete() {
            return None;
        }
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
        let t = self.table();
        imp.found
            .borrow()
            .as_ref()
            .filter(|(m, _)| m.query() == query && m.is_current(&t))
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
        let value = self.cell_text(cur);
        if is_formula(&value) {
            // Replace never edits a formula (FML-11): on to the next match, saying so.
            return self.find(text, match_case, column_only, Step::Next, move |found| {
                done(match found {
                    Found::Hit { ordinal, total } => Found::Skipped { ordinal, total },
                    other => other,
                })
            });
        }
        let cmd = {
            let table = self.table();
            matches.replace_in(&value, with).map(|new| {
                let at = CellRef {
                    row: table.row_id(cur.row),
                    col: table.col_id(cur.col),
                };
                SetCell::new(at, new)
            })
        };
        if let Some(cmd) = cmd {
            self.execute(Box::new(cmd));
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
        let get = self.job_table();
        let progress = Arc::new(SearchProgress::default());
        let with = with.to_owned();
        let started = Instant::now();
        let job = jobs::spawn({
            let progress = progress.clone();
            move |cancel| {
                // Matches are counted on results; formula cells are skipped (FML-11).
                let mut snapshot = get(cancel).ok_or(ReplaceError::Cancelled)?;
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
            let (cells, (edit, skipped)) = match result {
                Ok(Ok(r)) => r,
                Ok(Err(ReplaceError::TooMany(n))) => return done(Replaced::TooMany(n)),
                _ => return done(Replaced::Cancelled),
            };
            let Some(edit) = edit else {
                return done(if skipped > 0 {
                    Replaced::Cells {
                        replaced: 0,
                        skipped,
                    }
                } else {
                    Replaced::Nothing
                });
            };
            let replaced = match &edit {
                Edit::Cells(list) => list.len() as u64,
                _ => 1,
            };
            let built = started.elapsed();
            {
                let Edit::Cells(list) = &edit else { return };
                let focus = list[0].0;
                g.execute(Box::new(Batch::new("Replace all", vec![edit], focus)));
            }
            if std::env::var_os("BRICK_TIMINGS").is_some() {
                eprintln!(
                    "replace all: {cells} cells, built in {built:?}, applied in {:?}",
                    started.elapsed() - built
                );
            }
            g.after_change(None);
            done(Replaced::Cells { replaced, skipped });
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
        self.table().is_filtered()
    }

    /// While filtered, the rows there are in all: the "y" in "x of y rows".
    pub fn unfiltered_row_count(&self) -> Option<u64> {
        let t = self.table();
        t.is_filtered().then(|| t.unfiltered_row_count())
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
        let (current, inferred, set) = {
            let t = self.table();
            (
                t.filter_on(col).map(|f| f.test.clone()),
                t.inferred_type(col),
                t.type_override(col),
            )
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
        let (filters, rows) = {
            let t = self.table();
            if !t.is_complete() {
                return;
            }
            let filters: Vec<Filter> = filters
                .into_iter()
                .map(|(col, test)| Filter {
                    col: t.col_id(col),
                    test,
                })
                .collect();
            let rows = t.index().row_count() * filters.len() as u64;
            (filters, rows)
        };
        let get = self.job_table();
        let done = Arc::new(AtomicU64::new(0));
        let started = Instant::now();
        let job = jobs::spawn({
            let (done, filters) = (done.clone(), filters.clone());
            move |cancel| {
                // A formula column filters on its results (FML-11).
                let snapshot = get(cancel).ok_or(FilterError::Cancelled)?;
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
            if std::env::var_os("BRICK_TIMINGS").is_some() {
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
            {
                let mut t = g.table_mut();
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
        let refilter = {
            let mut t = self.table_mut();
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
        let t = self.table();
        (0..t.min_width())
            .filter_map(|c| t.type_override(c).map(|ty| (c, ty)))
            .collect()
    }

    /// Set types from [`Self::type_overrides`] again.
    pub fn restore_type_overrides(&self, types: Vec<(u32, InferredType)>) {
        {
            let mut t = self.table_mut();
            for (c, ty) in types {
                t.set_type_override(c, Some(ty));
            }
        }
        self.queue_draw();
    }

    /// The filters on, by column position and test, to apply again to the table a save
    /// reopens (FILT-3).
    pub fn filters(&self) -> Vec<(u32, Test)> {
        let t = self.table();
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
        self.close_filter_menu();
        if self.busy() {
            return;
        }
        self.table_mut().clear_filter(col);
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
        self.table().save_job()
    }

    pub fn set_saving(&self, saving: bool) {
        self.imp().saving.set(saving);
    }

    /// The first row is shown as column titles rather than data (ENG-7).
    pub fn has_header(&self) -> bool {
        self.table().has_header()
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
        self.table_mut().set_header(on);
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
        let sheet = {
            let book = imp.book.borrow();
            let t = book.active();
            // A sheet shown before keeps its types (XLSX-1).
            if !t.is_complete() || t.has_types() {
                return;
            }
            book.active_id()
        };
        let get = self.job_table();
        let started = Instant::now();
        // A formula column's type is its results' (FML-11).
        let job = jobs::spawn(move |cancel| get(cancel)?.infer_types(cancel.flag()));
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
            if std::env::var_os("BRICK_TIMINGS").is_some() {
                eprintln!(
                    "inferred column types in {:?}: {types:?}",
                    started.elapsed()
                );
            }
            let mut book = imp.book.borrow_mut();
            if let Some(s) = book.sheet_mut(sheet) {
                s.table.set_types(types);
            }
        });
    }

    /// Find the active sheet's formula cells (FML-4) on the job pool from a snapshot, once
    /// its file is indexed; a new workbook makes a running scan moot.
    pub fn scan_formulas(&self) {
        let imp = self.imp();
        let (snapshot, rows, sheet) = {
            let book = imp.book.borrow();
            let t = book.active();
            if !t.is_complete() || t.formulas_scanned() {
                return;
            }
            (t.snapshot(), t.index().row_count(), book.active_id())
        };
        let done = Arc::new(AtomicU64::new(0));
        let job = jobs::spawn({
            let done = done.clone();
            move |cancel| snapshot.scan_formulas(cancel.flag(), &done)
        });
        let old = imp.scanning.replace(Some(ScanJob {
            job: job.clone(),
            done,
            rows,
        }));
        if let Some(old) = old {
            old.job.cancel();
        }
        let g = self.downgrade();
        glib::spawn_future_local(async move {
            let found = job.finished().await.ok().flatten();
            let Some(g) = g.upgrade() else { return };
            let imp = g.imp();
            if !matches!(&*imp.scanning.borrow(), Some(s) if s.job.is(&job)) {
                return;
            }
            imp.scanning.take();
            let mut book = imp.book.borrow_mut();
            if let (Some(found), Some(s)) = (found, book.sheet_mut(sheet)) {
                s.table.set_formula_cells(found);
            }
        });
    }

    /// Every column back to the default width.
    pub fn reset_column_widths(&self) {
        let imp = self.imp();
        imp.col_widths.borrow_mut().clear();
        imp.sizing.borrow_mut().cols.clear();
    }

    /// Show a freshly opened workbook (the file just saved, or read again). Its edits are
    /// on disk now, so the undo history, which refers to the old overlays, starts over.
    /// Rows and columns get new identities in the new file, so the active sheet's
    /// remembered sizes move over by position.
    pub fn replace_workbook(&self, book: Workbook) {
        let imp = self.imp();
        let (heights, widths) = {
            let sizing = imp.sizing.borrow();
            let old = self.table();
            let heights: Vec<(u64, f64)> = (imp.row_heights.borrow().keys())
                .filter_map(|&id| old.row_of(id))
                .map(|r| (r, sizing.rows.size(r)))
                .collect();
            let widths: Vec<(u32, f64)> = (imp.col_widths.borrow().keys())
                .filter_map(|&id| old.col_of(id))
                .map(|c| (c, sizing.cols.size(u64::from(c))))
                .collect();
            (heights, widths)
        };
        // Unmapping a big file and freeing its index takes a while (0.2 s for 1 GB, SAVE-3):
        // a worker does it, not the UI thread.
        imp.ui_sheet.set(Some(book.active_id()));
        let old = imp.book.replace(book);
        let _ = std::thread::Builder::new()
            .name("drop-table".into())
            .spawn(move || drop(old));
        imp.generation.set(imp.generation.get() + 1);
        if let Some(old) = imp.calculating.take() {
            old.job.cancel();
        }
        if let Some(old) = imp.scanning.take() {
            old.job.cancel();
        }
        if let Some(old) = imp.rewriting.take() {
            old.job.cancel();
        }
        if let Some(old) = imp.formatting.take() {
            old.cancel();
        }
        imp.sheet_ui.borrow_mut().clear();
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
        imp.undo.replace(UndoStack::default());
        imp.cache.borrow_mut().reset();
        imp.titles.take();
        // The new file's own sizes, then those the user set, by position.
        let file = self.file_ui();
        imp.sizing.replace(file.sizing);
        imp.row_heights.replace(file.row_heights);
        imp.col_widths.replace(file.col_widths);
        // Rows of the new file appear as indexing reaches them; heights past what is
        // indexed now land on their positions' source ids all the same.
        let first = u64::from(self.table().has_header());
        for (r, h) in heights {
            imp.row_heights
                .borrow_mut()
                .insert(RowId::source(r + first), h);
        }
        for (c, w) in widths {
            imp.col_widths.borrow_mut().insert(ColId::source(c), w);
        }
        self.refit_sizes();
        self.update_adjustments();
    }

    /// How many times the workbook was replaced (`replace_workbook`) since the window
    /// opened.
    pub fn generation(&self) -> u64 {
        self.imp().generation.get()
    }

    /// The open file read with another delimiter (ENG-5); refused with unsaved edits.
    pub fn reread(&self, choice: DelimiterChoice) -> Result<CsvTable, RereadError> {
        self.table().reread(choice)
    }

    pub fn encoding(&self) -> csv_engine::Encoding {
        self.table().encoding()
    }

    pub fn delimiter(&self) -> u8 {
        self.table().dialect().delimiter
    }

    /// Unsaved changes: cell edits, plus rows inserted and deleted (EDIT-3), on every
    /// sheet, plus sheet changes (ADR 0008).
    pub fn edit_count(&self) -> usize {
        self.imp().book.borrow().edit_count()
    }

    pub fn row_count(&self) -> u64 {
        self.table().row_count()
    }

    /// Rows the grid shows: the table's, plus the empty edge row once indexing is done.
    fn shown_rows(&self) -> u64 {
        shown_rows(&self.table())
    }

    pub fn is_complete(&self) -> bool {
        self.table().is_complete()
    }

    /// Changes whenever what the workbook shows changes (APP-7's journal follows it).
    pub fn revision(&self) -> u64 {
        self.imp().book.borrow().revision()
    }

    /// The active sheet's table as it is now, for a job (APP-7 writes its journal from
    /// one).
    pub fn snapshot(&self) -> CsvTable {
        self.table().snapshot()
    }

    /// The whole workbook as it is now, for a job.
    pub fn workbook_snapshot(&self) -> Workbook {
        self.imp().book.borrow().snapshot()
    }

    /// The active table for a job that reads what cells show: sort, filter, find, replace,
    /// type inference, copy (FML-11). Formula cells read as their results: known ones are
    /// reused; otherwise the job computes them first, on a workbook snapshot.
    fn job_table(&self) -> JobTable {
        let imp = self.imp();
        let book = imp.book.borrow();
        let mut table = book.active().snapshot();
        if !table.has_formulas() {
            return Box::new(move |_| Some(table));
        }
        let (revision, sheet) = (book.revision(), book.active_id());
        let known = imp
            .results
            .lock()
            .expect("no job panics holding it")
            .as_ref()
            .filter(|(r, s, _)| (*r, *s) == (revision, sheet))
            .map(|(_, _, c)| c.clone());
        if let Some(c) = known {
            table.set_computed(Some(c));
            return Box::new(move |_| Some(table));
        }
        let (mut snapshot, cache) = (book.snapshot(), imp.results.clone());
        Box::new(move |cancel| {
            let done = AtomicU64::new(0);
            let computed = Arc::new(snapshot.computed_for(sheet, cancel.shared(), &done)?);
            *cache.lock().expect("no job panics holding it") =
                Some((revision, sheet, computed.clone()));
            table.set_computed(Some(computed));
            Some(table)
        })
    }

    /// The active sheet's name.
    pub fn sheet_name(&self) -> String {
        self.imp().book.borrow().active_sheet().name.clone()
    }

    pub fn sheet_count(&self) -> usize {
        self.imp().book.borrow().sheets().len()
    }

    /// Position of the active sheet among the sheets.
    pub fn active_position(&self) -> usize {
        let book = self.imp().book.borrow();
        book.index_of(book.active_id()).unwrap_or(0)
    }

    /// Each sheet's name, rows, and columns, for an .xlsx save's limits (XLSX-1).
    pub fn sheet_sizes(&self) -> Vec<(String, u64, u32)> {
        let book = self.imp().book.borrow();
        let active = book.active_id();
        book.sheets()
            .iter()
            .map(|s| {
                let cols = if s.id == active {
                    self.col_count()
                } else {
                    s.table.min_width()
                };
                (s.name.clone(), s.table.total_rows(), cols)
            })
            .collect()
    }

    /// The sizes the user set on each sheet, in Excel's units (ADR 0010): those that
    /// differ from what the file gives, for a save to write.
    pub fn user_sizes(&self) -> HashMap<SheetId, xlsx::SheetSizes> {
        let imp = self.imp();
        let book = imp.book.borrow();
        let ui = imp.sheet_ui.borrow();
        let scale = self.excel_scale_digit();
        let mut out = HashMap::new();
        for sheet in book.sheets() {
            let (heights, widths) = if sheet.id == book.active_id() {
                (
                    imp.row_heights.borrow().clone(),
                    imp.col_widths.borrow().clone(),
                )
            } else {
                match ui.get(&sheet.id) {
                    Some(u) => (u.row_heights.clone(), u.col_widths.clone()),
                    None => continue, // never shown: as the file has it
                }
            };
            let file = file_ui_of(&sheet.table, scale);
            let scale = ExcelScale::of(&sheet.table, scale);
            let differs = |a: f64, b: f64| (a - b).abs() > 0.5;
            let mut sizes = xlsx::SheetSizes::default();
            for (&id, &px) in &widths {
                let was = file
                    .col_widths
                    .get(&id)
                    .copied()
                    .unwrap_or(file.sizing.cols.default_size());
                if differs(px, was) {
                    sizes.widths.push((id, scale.chars(px)));
                }
            }
            for (&id, &px) in &heights {
                let was = file
                    .row_heights
                    .get(&id)
                    .copied()
                    .unwrap_or(file.sizing.rows.default_size());
                if differs(px, was) {
                    sizes.heights.push((id, scale.points(px)));
                }
            }
            if !sizes.widths.is_empty() || !sizes.heights.is_empty() {
                out.insert(sheet.id, sizes);
            }
        }
        out
    }

    /// Each sheet's header row and the column types the user set, by position, to set
    /// again on the workbook a save reads back (XLSX-1).
    pub fn sheet_settings(&self) -> Vec<(bool, Vec<(u32, InferredType)>)> {
        let book = self.imp().book.borrow();
        book.sheets()
            .iter()
            .map(|s| {
                let t = &s.table;
                let types = (0..t.min_width())
                    .filter_map(|c| t.type_override(c).map(|ty| (c, ty)))
                    .collect();
                (t.has_header(), types)
            })
            .collect()
    }

    /// Put back [`Self::sheet_settings`] by position.
    pub fn restore_sheet_settings(&self, settings: Vec<(bool, Vec<(u32, InferredType)>)>) {
        let ids: Vec<SheetId> = self.sheet_list().into_iter().map(|(id, _)| id).collect();
        let active = self.active_sheet();
        for (id, (header, types)) in ids.into_iter().zip(settings) {
            if id == active {
                if self.has_header() != header {
                    self.set_header(header);
                }
                self.restore_type_overrides(types);
                continue;
            }
            let mut book = self.imp().book.borrow_mut();
            if let Some(s) = book.sheet_mut(id) {
                s.table.set_header(header);
                for (c, ty) in types {
                    s.table.set_type_override(c, Some(ty));
                }
            }
        }
    }

    /// The sheets, in order: id and name.
    pub fn sheet_list(&self) -> Vec<(SheetId, String)> {
        let book = self.imp().book.borrow();
        book.sheets()
            .iter()
            .map(|s| (s.id, s.name.clone()))
            .collect()
    }

    pub fn active_sheet(&self) -> SheetId {
        self.imp().book.borrow().active_id()
    }

    /// Where the active sheet's rows come from.
    pub fn active_origin(&self) -> Origin {
        self.imp().book.borrow().active_sheet().origin
    }

    /// Changes when sheets are added, removed, renamed, or moved.
    pub fn layout_revision(&self) -> u64 {
        self.imp().book.borrow().layout_revision()
    }

    /// A workbook is being imported (XLSX-1): edits wait and the body says "Opening…".
    pub fn set_loading(&self, on: bool) {
        self.imp().loading.set(on);
        self.queue_draw();
    }

    pub fn is_loading(&self) -> bool {
        self.imp().loading.get()
    }

    /// Show sheet `id` (XLSX-1). The sheet shown so far keeps its cursor, scroll, and
    /// sizes for when it comes back. Refused while a job runs or the open edit has a
    /// formula that doesn't parse; whether sheet `id` shows now.
    pub fn switch_sheet(&self, id: SheetId) -> bool {
        let imp = self.imp();
        let current = imp.book.borrow().active_id();
        if current == id {
            return true;
        }
        if self.busy() || imp.book.borrow().sheet(id).is_none() {
            return false;
        }
        if !self.commit_edit(true) {
            return false;
        }
        imp.book.borrow_mut().set_active(id);
        self.follow_active();
        true
    }

    /// Bring the cursor, scroll, and sizes in line with the workbook's active sheet after
    /// it changed (a switch, or an undo that added or removed a sheet): the sheet shown so
    /// far keeps its own for when it comes back.
    fn follow_active(&self) {
        let imp = self.imp();
        let active = imp.book.borrow().active_id();
        let shown = imp.ui_sheet.replace(Some(active));
        if shown == Some(active) {
            return;
        }
        let value = |adj: &RefCell<Option<gtk::Adjustment>>| {
            adj.borrow().as_ref().map_or(0.0, |a| a.value())
        };
        let ui = SheetUi {
            selection: imp.selection.get(),
            vscroll: value(&imp.vadj),
            hscroll: value(&imp.hadj),
            sizing: imp.sizing.take(),
            row_heights: imp.row_heights.take(),
            col_widths: imp.col_widths.take(),
        };
        if let Some(shown) = shown {
            // Kept even for a removed sheet: an undo brings it back as it was.
            imp.sheet_ui.borrow_mut().insert(shown, ui);
        }
        let ui = imp.sheet_ui.borrow_mut().remove(&active);
        let ui = ui.unwrap_or_else(|| self.file_ui());
        imp.sizing.replace(ui.sizing);
        imp.row_heights.replace(ui.row_heights);
        imp.col_widths.replace(ui.col_widths);
        // What showed belongs to the other sheet: its rows, titles, matches, and jobs.
        imp.cache.borrow_mut().reset();
        imp.titles.take();
        imp.found.take();
        imp.bounds_seen.set(None);
        self.cancel_search();
        if let Some(old) = imp.calculating.take() {
            old.job.cancel();
        }
        if let Some(old) = imp.scanning.take() {
            old.job.cancel();
        }
        if let Some(old) = imp.inferring.take() {
            old.cancel();
        }
        // The status tick infers this sheet's types and scans its formulas, as for a
        // newly opened table.
        imp.generation.set(imp.generation.get() + 1);
        self.refit_sizes();
        self.update_adjustments();
        for (adj, v) in [(&imp.vadj, ui.vscroll), (&imp.hadj, ui.hscroll)] {
            if let Some(a) = adj.borrow().as_ref() {
                a.set_value(v);
            }
        }
        self.set_selection(ui.selection);
        self.update_description();
        self.emit_by_name::<()>("changed", &[]);
        self.queue_draw();
    }

    /// Run a change to the sheets as one undoable step (SHEET-1), then show the sheet
    /// the workbook makes active.
    fn execute_book(&self, cmd: SheetEdit) {
        let imp = self.imp();
        {
            let mut book = imp.book.borrow_mut();
            imp.undo.borrow_mut().execute(Box::new(cmd), &mut book);
        }
        self.follow_active();
        self.after_change(None);
    }

    /// Shift+F11 (SHEET-1): a new empty sheet after the active one, named `SheetN`, shown.
    /// One undoable step; refused while a job runs.
    pub fn add_sheet(&self) -> Option<SheetId> {
        let imp = self.imp();
        if self.busy() || !self.commit_edit(true) {
            return None;
        }
        let (edit, id) = {
            let mut book = imp.book.borrow_mut();
            let name = book.new_sheet_name();
            let sheet = book.new_sheet(name, CsvTable::untitled(), Origin::New);
            let id = sheet.id;
            let at = book.index_of(book.active_id()).map_or(0, |i| i + 1);
            let sheet = Box::new(sheet);
            (BookEdit::Insert { at, sheet }, id)
        };
        self.execute_book(SheetEdit::new("Add sheet", vec![edit], Some(id)));
        self.switch_sheet(id);
        Some(id)
    }

    /// Rename sheet `id` (SHEET-1) as one undoable step. Refused, with the reason, for a
    /// name Excel wouldn't take (empty, too long, a forbidden character, another sheet's).
    pub fn rename_sheet(&self, id: SheetId, name: &str) -> Result<(), &'static str> {
        let name = name.trim();
        {
            let book = self.imp().book.borrow();
            book.check_sheet_name(name, Some(id))?;
            if book.sheet(id).is_none_or(|s| s.name == name) {
                return Ok(());
            }
        }
        if self.busy() {
            return Err("Wait for the running job to finish");
        }
        let from = self.imp().book.borrow().sheet(id).map(|s| s.name.clone());
        let Some(from) = from else { return Ok(()) };
        let to = name.to_owned();
        // References to the sheet follow its new name, in the same undo step (SHEET-2).
        self.with_rewrites(
            {
                let (from, to) = (from.clone(), to.clone());
                move |book, cancel| {
                    let change = formula::Change::RenameSheet {
                        from: &from,
                        to: &to,
                    };
                    book.sheet_rewrites(&change, cancel)
                }
            },
            move |g, mut edits| {
                edits.push(BookEdit::Rename { id, name: to });
                g.execute_book(SheetEdit::new("Rename sheet", edits, Some(id)));
            },
        );
        Ok(())
    }

    /// Compute the formula rewrites a change needs on the job pool from a workbook
    /// snapshot ("Updating formulas"; edits wait), then make the change with them as one
    /// step: `finish` gets them as a `BookEdit` (none when no formula changes). Without
    /// formulas anywhere it all happens now. More than [`data_model::REPLACE_MAX_CELLS`]
    /// rewritten cells is refused.
    fn with_rewrites(
        &self,
        compute: impl FnOnce(&mut Workbook, &std::sync::atomic::AtomicBool) -> Option<Vec<(SheetId, Edit)>>
            + Send
            + 'static,
        finish: impl FnOnce(&Self, Vec<BookEdit>) + 'static,
    ) {
        let imp = self.imp();
        let any = imp
            .book
            .borrow()
            .sheets()
            .iter()
            .any(|s| s.table.has_formulas());
        if !any {
            finish(self, Vec::new());
            return;
        }
        let mut snapshot = imp.book.borrow().snapshot();
        let job = jobs::spawn(move |cancel| compute(&mut snapshot, cancel.flag()));
        imp.rewriting.replace(Some(RewriteJob { job: job.clone() }));
        let g = self.downgrade();
        glib::spawn_future_local(async move {
            let result = job.finished().await;
            let Some(g) = g.upgrade() else { return };
            let imp = g.imp();
            if !matches!(&*imp.rewriting.borrow(), Some(r) if r.job.is(&job)) {
                return;
            }
            imp.rewriting.take();
            let Ok(Some(rewrites)) = result else { return };
            let n = data_model::rewrite_count(&rewrites);
            if n > data_model::REPLACE_MAX_CELLS {
                let why = format!(
                    "This would rewrite {} formulas; Brick rewrites at most {} at once.",
                    crate::status::group_digits(n),
                    crate::status::group_digits(data_model::REPLACE_MAX_CELLS)
                );
                g.emit_by_name::<()>("refused", &[&"Too many formulas to rewrite", &why]);
                return;
            }
            let edits = if rewrites.is_empty() {
                Vec::new()
            } else {
                vec![BookEdit::Sheets(rewrites)]
            };
            finish(&g, edits);
        });
    }

    /// Delete sheet `id` (SHEET-1) as one undoable step: undo brings it back with its
    /// edits, and references to it get back their text (SHEET-2). Refused for the only
    /// sheet and while a job runs.
    pub fn delete_sheet(&self, id: SheetId) -> bool {
        {
            let book = self.imp().book.borrow();
            if book.sheet(id).is_none() || book.sheets().len() < 2 {
                return false;
            }
        }
        if self.busy() || !self.commit_edit(true) {
            return false;
        }
        let show = self.active_sheet();
        let show = (show != id).then_some(show);
        let Some(name) = self.imp().book.borrow().sheet(id).map(|s| s.name.clone()) else {
            return false;
        };
        // References to the sheet become #REF!, in the same undo step (SHEET-2).
        self.with_rewrites(
            move |book, cancel| {
                let change = formula::Change::DeleteSheet { name: &name };
                book.sheet_rewrites(&change, cancel)
            },
            move |g, mut edits| {
                // Positions may have moved while the rewrites ran: remove by id.
                let Some(at) = g.imp().book.borrow().index_of(id) else {
                    return;
                };
                edits.push(BookEdit::Remove { at });
                g.execute_book(SheetEdit::new("Delete sheet", edits, show));
            },
        );
        true
    }

    /// Move sheet `id` to position `to` (SHEET-1), as one undoable step.
    pub fn move_sheet(&self, id: SheetId, to: usize) -> bool {
        let from = {
            let book = self.imp().book.borrow();
            match book.index_of(id) {
                Some(from) if to < book.sheets().len() && to != from => from,
                _ => return false,
            }
        };
        if self.busy() {
            return false;
        }
        self.execute_book(SheetEdit::new(
            "Move sheet",
            vec![BookEdit::Move { from, to }],
            Some(id),
        ));
        true
    }

    /// Put back a crashed session's sheets and unsaved edits (APP-7, XLSX-1), read from
    /// its journal, as one undoable step; each sheet's header row as it was. The file must
    /// be fully read; refused if the journal doesn't fit it.
    pub fn restore_journal(&self, state: BookState) -> Result<(), JournalError> {
        let headers: Vec<bool> = state.sheets.iter().map(|s| s.has_header).collect();
        let active = state.active as usize;
        let plan = self.imp().book.borrow_mut().restore_plan(state)?;
        self.restore_book(plan, active)?;
        let ids: Vec<SheetId> = self.sheet_list().into_iter().map(|(id, _)| id).collect();
        let active_id = self.active_sheet();
        for (id, on) in ids.into_iter().zip(headers) {
            if id == active_id {
                if self.has_header() != on {
                    self.set_header(on);
                }
            } else if let Some(s) = self.imp().book.borrow_mut().sheet_mut(id) {
                s.table.set_header(on);
            }
        }
        Ok(())
    }

    /// Ctrl+Page Down / Up: the next or previous sheet, if there is one.
    pub fn switch_to_neighbor(&self, step: isize) -> bool {
        let next = {
            let book = self.imp().book.borrow();
            let at = book.index_of(book.active_id()).unwrap_or(0) as isize + step;
            usize::try_from(at)
                .ok()
                .and_then(|i| book.sheets().get(i))
                .map(|s| s.id)
        };
        next.is_some_and(|id| self.switch_sheet(id))
    }

    /// Put back a crashed session's unsaved edits (APP-7, XLSX-1) as one undoable step,
    /// "Restore unsaved edits": the sheet changes and every sheet's edits, from
    /// [`Workbook::restore_plan`]; then show the sheet at position `active`.
    pub fn restore_book(&self, plan: Vec<BookEdit>, active: usize) -> Result<(), JournalError> {
        let imp = self.imp();
        if self.busy() {
            return Err(JournalError::Mismatch("another job is running"));
        }
        {
            let mut book = imp.book.borrow_mut();
            let show = book.active_id();
            imp.undo.borrow_mut().execute(
                Box::new(SheetEdit::new("Restore unsaved edits", plan, Some(show))),
                &mut book,
            );
        }
        self.follow_active();
        let id = imp.book.borrow().sheets().get(active).map(|s| s.id);
        if let Some(id) = id {
            self.switch_sheet(id);
        }
        self.after_change(None);
        Ok(())
    }

    /// Set the clock `TODAY()` and `NOW()` read; cells showing results redraw if it
    /// changed.
    pub fn set_now(&self, now: f64) {
        let imp = self.imp();
        if imp.book.borrow_mut().set_now(now) {
            imp.cache.borrow_mut().invalidate();
            self.queue_draw();
        }
    }

    /// The active table, read-only. A GridView always holds a workbook with a sheet.
    fn table(&self) -> Ref<'_, CsvTable> {
        Ref::map(self.imp().book.borrow(), Workbook::active)
    }

    fn table_mut(&self) -> RefMut<'_, CsvTable> {
        RefMut::map(self.imp().book.borrow_mut(), Workbook::active_mut)
    }

    /// Run a table command on the active sheet as one undoable step (invariant 5).
    fn execute(&self, cmd: Box<dyn Command<CsvTable>>) {
        let imp = self.imp();
        let mut book = imp.book.borrow_mut();
        let sheet = book.active_id();
        imp.undo
            .borrow_mut()
            .execute(Box::new(OnSheet::new(sheet, cmd)), &mut book);
    }

    /// Start a job for formula cells the frame's budget didn't reach (FML-4), unless one
    /// is computing for this revision already; a job for an older revision is cancelled.
    fn calc_pending(&self, pending: Vec<CellRef>) {
        let imp = self.imp();
        let revision = imp.book.borrow().revision();
        if let Some(running) = imp.calculating.borrow().as_ref() {
            if running.revision == revision {
                return;
            }
            running.job.cancel();
        }
        imp.calculating.take();
        if pending.is_empty() {
            return;
        }
        let (mut snapshot, sheet) = {
            let book = imp.book.borrow();
            (book.snapshot(), book.active_id())
        };
        let cells: Vec<(SheetId, CellRef)> = pending.into_iter().map(|c| (sheet, c)).collect();
        let done = Arc::new(AtomicU64::new(0));
        let total = cells.len() as u64;
        let job = jobs::spawn({
            let done = done.clone();
            move |cancel| snapshot.evaluate_all(&cells, cancel.shared(), &done)
        });
        imp.calculating.replace(Some(CalcJob {
            job: job.clone(),
            done,
            total,
            revision,
        }));
        let g = self.downgrade();
        glib::spawn_future_local(async move {
            let values = job.finished().await.ok().flatten();
            let Some(g) = g.upgrade() else { return };
            let imp = g.imp();
            if !matches!(&*imp.calculating.borrow(), Some(c) if c.job.is(&job)) {
                return;
            }
            imp.calculating.take();
            let Some(values) = values else { return };
            imp.book.borrow_mut().store_values(revision, values);
            imp.cache.borrow_mut().invalidate();
            g.update_description();
            g.queue_draw();
        });
    }

    /// Move the cell cursor or the selected range; `cursor-changed` says so. On merged
    /// cells the cursor goes to the merge's anchor and a range covers the merges it
    /// touches (STYLE-4), wherever the selection came from.
    fn set_selection(&self, sel: Selection) {
        let imp = self.imp();
        let mut sel = sel;
        self.with_merges(|m| sel.fit(m));
        if imp.selection.replace(sel) != sel {
            self.emit_by_name::<()>("cursor-changed", &[]);
            self.update_description();
        }
    }

    /// Run `f` with the active sheet's merged cells in grid rows and columns (STYLE-4);
    /// none while the workbook is borrowed for a change.
    fn with_merges<R>(&self, f: impl FnOnce(&dyn grid::Merges) -> R) -> R {
        match self.imp().book.try_borrow() {
            Ok(book) if !book.active().merges().is_empty() => f(&TableMerges(book.active())),
            _ => f(&grid::NoMerges),
        }
    }

    /// The cursor cell's reference (`B2`, file rows as references count them) and the
    /// text it shows (a formula's result).
    pub fn cursor_description(&self) -> String {
        let imp = self.imp();
        let cur = imp.selection.get().cursor();
        let Ok(mut book) = imp.book.try_borrow_mut() else {
            return String::new();
        };
        let mut name = String::new();
        column_name(cur.col, &mut name);
        let t = book.active();
        let number = row_number(t, cur.row);
        let mut text = format!("{name}{number}: ");
        if cur.row >= t.row_count() {
            return text;
        }
        let raw = t
            .cell_value(cur.row, cur.col)
            .map(|v| v.into_owned())
            .unwrap_or_default();
        let at = CellRef {
            row: t.row_id(cur.row),
            col: t.col_id(cur.col),
        };
        let shown = if is_formula(&raw) {
            let sheet = book.active_id();
            match book.value_of(sheet, at, &mut Budget::cells(UI_BUDGET)) {
                Ok(v) => formula::result_text(&v),
                Err(_) => return text + data_model::PENDING,
            }
        } else {
            raw
        };
        // Under its number format, as the grid draws it (STYLE-3).
        let pool = book.styles();
        let xf = pool.xf(book.active().style_of(at));
        let code = pool
            .custom_format(xf.num_fmt)
            .or_else(|| formula::builtin_format(xf.num_fmt))
            .filter(|c| !c.eq_ignore_ascii_case("general"));
        match code.and_then(|c| imp.formatted(c, &shown)) {
            Some(f) if !shown.is_empty() => text.push_str(f.text.trim()),
            _ => text.push_str(&shown),
        }
        text
    }

    /// Screen readers announce the cursor cell and what it shows (FML-4).
    fn update_description(&self) {
        let text = self.cursor_description();
        self.update_property(&[gtk::accessible::Property::Description(&text)]);
    }

    /// Put the cursor on file row `file_row` (0 is the file's first line), wherever a
    /// sort put it (APP-8: a malformed line). False if it isn't shown: the header row, or
    /// a row a filter hides.
    pub fn go_to_file_row(&self, file_row: u64) -> bool {
        let row = self.table().row_of(data_model::RowId::source(file_row));
        let Some(row) = row else {
            return false;
        };
        let cell = grid::Cell { row, col: 0 };
        self.set_selection(Selection::at(cell));
        self.reveal_cell(cell);
        self.grab_focus();
        self.queue_draw();
        true
    }

    /// Share of the file indexed so far (0..=1), or `None` once indexing is complete.
    pub fn index_progress(&self) -> Option<f64> {
        let t = self.table();
        if t.is_complete() {
            return None;
        }
        let index = t.index();
        let total = index.source().bytes().len().max(1) as f64;
        Some(index.bytes_indexed() as f64 / total)
    }

    pub fn file_changed(&self) -> bool {
        self.table().file_changed()
    }

    pub fn cache_bytes(&self) -> usize {
        self.imp().cache.borrow().heap_bytes()
    }

    /// Top row of the last painted frame, once its cells were loaded and drawn.
    pub fn painted_top_row(&self) -> Option<u64> {
        self.imp().painted_top.get()
    }

    /// Resize scroll ranges to what the grid reaches: the rows indexed so far, the widest
    /// row seen, and the empty cells past them (GRID-6).
    pub fn update_adjustments(&self) {
        let imp = self.imp();
        let reach = self.reach();
        // Room for the widest row number in the current font (digits are tabular): the
        // last row's file row number, the edge row's or one past it.
        let t = self.table();
        let widest = t.total_rows() + reach.rows.saturating_sub(t.row_count()).max(1);
        drop(t);
        let digits = widest.ilog10() as i32 + 1;
        let digit_w = self.create_pango_layout(Some("0")).pixel_size().0.max(1);
        imp.row_header_w
            .set(f64::from(digits * digit_w) + 3.0 * PAD);
        let (body_w, body_h) = imp.body_size();
        // GTK requires upper >= page size; short or narrow tables just don't scroll.
        let sizing = imp.sizing.borrow();
        if let Some(v) = imp.vadj.borrow().as_ref() {
            let upper = sizing.rows.start(reach.rows).max(body_h);
            v.configure(v.value(), 0.0, upper, ROW_H, body_h * 0.9, body_h);
        }
        if let Some(hz) = imp.hadj.borrow().as_ref() {
            let cols = u64::from(reach.cols);
            let upper = sizing.cols.start(cols).max(body_w);
            hz.configure(hz.value(), 0.0, upper, COL_W / 4.0, body_w * 0.9, body_w);
        }
        drop(sizing);
        self.queue_draw();
    }
}

/// Cell text under number formats, by format code and text.
type FormattedCache = HashMap<(Box<str>, String), Rc<Formatted>>;

/// The font styles Ctrl+B, Ctrl+I, Ctrl+U, and Ctrl+5 turn on and off (STYLE-6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontToggle {
    Bold,
    Italic,
    Underline,
    Strike,
}

/// The cursor cell's format, as the format bar shows it (STYLE-6).
#[derive(Debug, Clone)]
pub struct CursorFormat {
    pub font: Font,
    /// The font's colour as `0xRRGGBB`; `None` for automatic.
    pub color: Option<u32>,
    /// A solid fill's colour; `None` for no fill.
    pub fill: Option<u32>,
    pub xf: Xf,
    /// The number format code (`General` for none).
    pub code: String,
}

/// Excel's units on a sheet as Brick draws them (STYLE-3): a row of the sheet's default
/// height is Brick's row height; a column's characters count Brick's digit width in place
/// of Excel's 7 px (Calibri 11).
#[derive(Debug, Clone, Copy)]
struct ExcelScale {
    /// Points per default row.
    default_pt: f64,
    digit_w: f64,
}

impl ExcelScale {
    fn of(table: &CsvTable, digit_w: f64) -> Self {
        Self {
            default_pt: table.layout().default_row_height.unwrap_or(15.0).max(1.0),
            digit_w,
        }
    }

    fn row_px(self, pt: f64) -> f64 {
        (pt * ROW_H / self.default_pt).max(MIN_ROW_H)
    }

    fn points(self, px: f64) -> f64 {
        (px * self.default_pt / ROW_H * 100.0).round() / 100.0
    }

    /// Excel's pixel width for `chars`, scaled to Brick's digit.
    fn col_px(self, chars: f64) -> f64 {
        let px = ((256.0 * chars + (128.0 / 7.0f64).trunc()) / 256.0 * 7.0).trunc();
        (px * self.digit_w / 7.0).max(MIN_COL_W)
    }

    /// Excel's characters for a width of `px` (its own formula, from Excel's pixels).
    fn chars(self, px: f64) -> f64 {
        let excel_px = px * 7.0 / self.digit_w;
        ((excel_px - 5.0) / 7.0 * 100.0 + 0.5).trunc().max(0.0) / 100.0
    }
}

/// A sheet's sizes as its file gives them (STYLE-3): the most common row height and the
/// default column width become the sheet's own defaults, the rest are kept by row and
/// column identity. A CSV sheet has none.
fn file_ui_of(table: &CsvTable, digit_w: f64) -> SheetUi {
    let layout = table.layout();
    if layout.default_row_height.is_none()
        && layout.default_col_width.is_none()
        && layout.cols.is_empty()
        && layout.row_runs().is_empty()
    {
        return SheetUi::default();
    }
    let scale = ExcelScale::of(table, digit_w);
    let default_pt = scale.default_pt;
    let mut heights: HashMap<u64, u64> = HashMap::new();
    let mut covered = 0;
    for (start, end, spec) in layout.row_runs() {
        if let Some(h) = spec.height {
            *heights.entry(h.to_bits()).or_default() += end - start;
            covered += end - start;
        }
    }
    *heights.entry(default_pt.to_bits()).or_default() += table.total_rows().saturating_sub(covered);
    let common = heights
        .iter()
        .max_by_key(|&(h, n)| (*n, *h))
        .map_or(default_pt, |(&h, _)| f64::from_bits(h));
    let default_chars = layout
        .default_col_width
        .or_else(|| layout.base_col_width.map(|b| f64::from(b) + 0.43))
        .unwrap_or(8.43);
    let mut ui = SheetUi {
        sizing: Sizing {
            rows: Sizes::new(scale.row_px(common)),
            cols: Sizes::new(scale.col_px(default_chars)),
        },
        ..SheetUi::default()
    };
    for (start, end, spec) in layout.row_runs() {
        let h = spec.height.unwrap_or(default_pt);
        if h != common {
            ui.row_heights
                .extend((*start..*end).map(|r| (RowId::source(r), scale.row_px(h))));
        }
    }
    for spec in &layout.cols {
        if let Some(w) = spec.width.filter(|&w| w != default_chars) {
            let last = spec.last.min(spec.first + 16_384);
            ui.col_widths
                .extend((spec.first..=last).map(|c| (ColId::source(c), scale.col_px(w))));
        }
    }
    ui
}

/// Where a cell's text goes across it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Align {
    Left,
    Center,
    Right,
}

/// The top of a line of text `th` high in a row at `y`, `rh` high. Text keeps 3 px from
/// the row's edges where the row has room; in a row barely taller than its text the inset
/// shrinks, so the text stays whole rather than being pushed out past an edge. Text taller
/// than its row is cut where Excel cuts it: at the bottom for top alignment, evenly when
/// centred, at the top for bottom alignment.
fn text_y(vertical: VAlign, y: f64, rh: f64, th: f64) -> f64 {
    let room = rh - th;
    let inset = (room / 2.0).clamp(0.0, 3.0);
    match vertical {
        VAlign::Top => y + inset,
        VAlign::Center | VAlign::Justify | VAlign::Distributed => y + room / 2.0,
        VAlign::Bottom => y + room - inset,
    }
}

/// The first and last columns of the centre-across-selection run around text in column
/// `c` of `count`: neighbours join while `joins` says they are empty and centred across
/// too, at most `ACROSS_MAX` on each side.
fn across_run(c: u64, count: u64, mut joins: impl FnMut(u64) -> bool) -> (u64, u64) {
    let (mut first, mut last) = (c, c);
    while first > 0 && c - first < ACROSS_MAX && joins(first - 1) {
        first -= 1;
    }
    while last + 1 < count && last - c < ACROSS_MAX && joins(last + 1) {
        last += 1;
    }
    (first, last)
}

/// A merge on screen (STYLE-4): the grid cells it spans, and its anchor (top-left) cell.
struct Merged {
    tl: grid::Cell,
    br: grid::Cell,
    anchor: CellRef,
    /// The anchor's row is a table row (not filtered out), so its text shows.
    anchor_shown: bool,
}

impl Merged {
    fn covers(&self, row: u64, col: u32) -> bool {
        (self.tl.row..=self.br.row).contains(&row) && (self.tl.col..=self.br.col).contains(&col)
    }
}

/// The merges that reach into table rows `rows` and columns `cols`, in grid cells; even
/// those whose anchor is scrolled away. Cheap without merges.
fn visible_merges(
    table: &CsvTable,
    rows: std::ops::Range<u64>,
    cols: std::ops::Range<u64>,
) -> Vec<Merged> {
    let end = rows.end.min(table.row_count());
    if table.merges().is_empty() || rows.start >= end || cols.is_empty() {
        return Vec::new();
    }
    let positions = table.position(rows.start)..=table.position(end - 1);
    let cols = cols.start as u32..=(cols.end - 1) as u32;
    table
        .merges_in(positions, cols)
        .filter_map(|m| {
            let (tl, br) = TableMerges(table).area(&m)?;
            (tl != br).then(|| Merged {
                tl,
                br,
                anchor: CellRef {
                    row: table.id_at(m.first_row),
                    col: table.col_id(m.first_col),
                },
                anchor_shown: table.position(tl.row) == m.first_row,
            })
        })
        .collect()
}

/// Call `f` with each span of `start..end` outside `holes` (spans, in any order).
fn each_gap(start: f64, end: f64, holes: &mut [(f64, f64)], mut f: impl FnMut(f64, f64)) {
    holes.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut at = start;
    for &(a, b) in holes.iter() {
        if a > at {
            f(at, a.min(end));
        }
        at = at.max(b);
        if at >= end {
            return;
        }
    }
    if at < end {
        f(at, end);
    }
}

/// A table's merged cells in grid rows and columns (STYLE-4), for navigation and drawing.
/// Merges keep all-rows positions; table rows show positions in order, so a merge is a
/// rectangle of table rows too (the ones a filter leaves).
struct TableMerges<'a>(&'a CsvTable);

impl TableMerges<'_> {
    fn area(&self, m: &data_model::CellArea) -> Option<grid::Area> {
        let rows = self.0.merge_rows(m)?;
        Some((
            grid::Cell {
                row: rows.start,
                col: m.first_col,
            },
            grid::Cell {
                row: rows.end - 1,
                col: m.last_col,
            },
        ))
    }
}

impl grid::Merges for TableMerges<'_> {
    fn at(&self, cell: grid::Cell) -> Option<grid::Area> {
        let t = self.0;
        if t.merges().is_empty() || cell.row >= t.row_count() {
            return None;
        }
        self.area(&t.merge_at(t.position(cell.row), cell.col)?)
    }

    fn each_in(&self, tl: grid::Cell, br: grid::Cell, f: &mut dyn FnMut(grid::Area)) {
        let t = self.0;
        let n = t.row_count();
        if t.merges().is_empty() || tl.row >= n {
            return;
        }
        let rows = t.position(tl.row)..=t.position(br.row.min(n - 1));
        for m in t.merges_in(rows, tl.col..=br.col) {
            if let Some(area) = self.area(&m) {
                f(area);
            }
        }
    }
}

/// A border line from (`x`, `y`), `len` long, across the row (`across`) or down the column.
fn draw_line(snapshot: &gtk::Snapshot, line: &Line, x: f64, y: f64, len: f64, across: bool) {
    let w = f64::from(line.width);
    let rect = |at: f64, run: f64, offset: f64, thick: f64| {
        if across {
            graphene::Rect::new(at as f32, (y + offset) as f32, run as f32, thick as f32)
        } else {
            graphene::Rect::new((x + offset) as f32, at as f32, thick as f32, run as f32)
        }
    };
    let start = if across { x } else { y };
    match line.dash {
        Dash::Solid => snapshot.append_color(&line.color, &rect(start, len, 0.0, w)),
        Dash::Double => {
            snapshot.append_color(&line.color, &rect(start, len, -1.0, 1.0));
            snapshot.append_color(&line.color, &rect(start, len, 1.0, 1.0));
        }
        Dash::Dashed(on, off) => {
            let (on, step) = (f64::from(on), f64::from(on) + f64::from(off));
            let mut at = start;
            while at < start + len {
                let run = on.min(start + len - at);
                snapshot.append_color(&line.color, &rect(at, run, 0.0, w));
                at += step;
            }
        }
    }
}

/// The formula cells among table rows `rows` and columns `cols` of `table` (its raw text:
/// no results set), as offsets in the block (FML-12). `None` once `cancel` is set.
fn clip_formulas(
    table: &mut CsvTable,
    rows: std::ops::Range<u64>,
    cols: std::ops::RangeInclusive<u32>,
    cancel: &jobs::CancelToken,
) -> Option<Vec<ClipFormula>> {
    const CHUNK: u64 = 65_536;
    let mut out = Vec::new();
    let mut block = data_model::RowBlock::full_text();
    let mut at = rows.start;
    while at < rows.end {
        if cancel.is_cancelled() {
            return None;
        }
        let end = (at + CHUNK).min(rows.end);
        table.read_rows(at..end, &mut block);
        for r in at..end {
            let last = block.cells_in_row(r).min(cols.end().saturating_add(1));
            for c in *cols.start()..last {
                let text = block.cell(r, c).unwrap_or("");
                if is_formula(text) {
                    out.push(ClipFormula {
                        row: r - rows.start,
                        col: c - cols.start(),
                        raw: text.to_owned(),
                        from: (table.position(r), c),
                    });
                }
            }
        }
        at = end;
    }
    Some(out)
}

/// The number the row gutter shows for table row `row`: its file row (from 1, the header
/// row included), as references count rows (ADR 0006). Past the table's rows: the edge
/// row's.
fn row_number(table: &CsvTable, row: u64) -> u64 {
    let n = table.row_count();
    if row < n {
        table.position(row) + 1
    } else {
        table.total_rows() + (row - n) + 1
    }
}

/// Rows shown for `table`: its rows, plus the editable edge row once indexing is done.
/// Typing there adds a row at the end of the file, filtered or not (FILT-4).
fn shown_rows(table: &CsvTable) -> u64 {
    table.row_count() + u64::from(table.is_complete())
}

/// The identity table row `row` gets from a paste's `edits`, for a row at or past `rows`
/// (the row count before them): its place in the run they add at the end (GRID-6).
fn added_row(edits: &[Edit], rows: u64, row: u64) -> Option<RowId> {
    edits.iter().find_map(|e| match e {
        Edit::InsertRows { rows: runs, .. } => {
            runs.first().map(|r| RowId(r.first.0 + (row - rows)))
        }
        _ => None,
    })
}

/// How far the empty cells past the data go (GRID-6), as rows and columns: Excel's sheet
/// size, or the data's where that is bigger (a taller CSV keeps just its edge row). A
/// cell typed there can still save as .xlsx, and a name-box reference can't make a file
/// of a trillion empty rows.
fn reach_limit(data: Bounds) -> (u64, u32) {
    (data.rows.max(xlsx::MAX_ROWS), data.cols.max(xlsx::MAX_COLS))
}

/// Rows or columns the grid reaches along one axis (GRID-6): the data's `data`, or as
/// many as fill the `view` from `scroll` and one more `view` past it and reach index
/// `far`, at most `limit` of those.
fn reach_count(data: u64, sizes: &Sizes, scroll: f64, view: f64, far: u64, limit: u64) -> u64 {
    let fill = sizes.index_at(scroll + 2.0 * view) + 1;
    data.max(fill.max(far + 1).min(limit))
}

/// Add the rows `pad` adds to sheet `id` of `book`, a snapshot a format past the data is
/// worked out on (GRID-6).
fn pad_sheet(book: &mut Workbook, id: SheetId, pad: Edit) {
    let sheet = book.sheet_mut(id).expect("the formatted sheet exists");
    sheet.table.apply(pad);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_keeps_three_pixels_where_the_row_has_room() {
        for (v, top) in [
            (VAlign::Top, 13.0),
            (VAlign::Center, 17.0),
            (VAlign::Bottom, 21.0),
        ] {
            assert_eq!(text_y(v, 10.0, 32.0, 18.0), top, "{v:?}");
        }
    }

    #[test]
    fn text_in_a_tight_row_stays_inside_it() {
        // A 19.2 px Excel row (14.4 pt) and a 17 px line: 1.1 px each side, any alignment.
        for v in [VAlign::Top, VAlign::Center, VAlign::Bottom] {
            let top = text_y(v, 0.0, 19.2, 17.0);
            assert!((top - 1.1).abs() < 1e-9, "{v:?}: {top}");
        }
    }

    #[test]
    fn text_taller_than_its_row_is_cut_on_the_aligned_side() {
        assert_eq!(text_y(VAlign::Top, 0.0, 10.0, 14.0), 0.0);
        assert_eq!(text_y(VAlign::Center, 0.0, 10.0, 14.0), -2.0);
        assert_eq!(text_y(VAlign::Bottom, 0.0, 10.0, 14.0), -4.0);
    }

    #[test]
    fn a_centred_across_run_stops_at_cells_that_do_not_join() {
        // Columns 2..=5 join; text in 3 spans them all, but not 1 or 6.
        assert_eq!(across_run(3, 10, |k| (2..=5).contains(&k)), (2, 5));
        // Nothing joins: the text keeps its own cell.
        assert_eq!(across_run(3, 10, |_| false), (3, 3));
        // The sheet's first and last columns end it.
        assert_eq!(across_run(0, 4, |_| true), (0, 3));
    }

    #[test]
    fn a_centred_across_run_is_bounded() {
        let (first, last) = across_run(1000, 16_384, |_| true);
        assert_eq!((first, last), (1000 - ACROSS_MAX, 1000 + ACROSS_MAX));
    }

    #[test]
    fn empty_cells_reach_a_screen_past_the_view_and_the_cursor() {
        let sizes = Sizes::new(20.0);
        let max = xlsx::MAX_ROWS;
        // A blank sheet (just the edge row) fills a 700 px view and one more below it.
        assert_eq!(reach_count(1, &sizes, 0.0, 700.0, 0, max), 71);
        // Scrolled on, they follow; a cursor further down is reached too.
        assert_eq!(reach_count(1, &sizes, 2000.0, 700.0, 0, max), 171);
        assert_eq!(reach_count(1, &sizes, 0.0, 700.0, 5000, max), 5001);
        // A taller file keeps its own count until scrolled to its end, then gains a screen.
        let end = sizes.start(1000) - 700.0;
        assert_eq!(reach_count(1000, &sizes, 0.0, 700.0, 0, max), 1000);
        assert_eq!(reach_count(1000, &sizes, end, 700.0, 0, max), 1036);
        // One taller than Excel's sheet keeps just its edge row: the limit is its own.
        let tall = 19_094_580;
        let end = sizes.start(tall) - 700.0;
        assert_eq!(reach_count(tall, &sizes, end, 700.0, 0, tall), tall);
        // Excel's sheet size caps them; a cursor can't push past it either.
        assert_eq!(reach_count(1, &sizes, 0.0, 700.0, u64::MAX - 1, max), max);
        assert_eq!(reach_count(1, &sizes, 1e12, 700.0, 0, max), max);
    }
}
