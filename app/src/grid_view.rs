//! The grid widget: draws the cells `grid::GridCache` holds with GSK + Pango (ADR 0001).

use data_model::{CsvTable, TableSource};
use grid::{ColumnViewport, GridCache, Viewport};
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
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GridView {
        const NAME: &'static str = "BricksGridView";
        type Type = super::GridView;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for GridView {}

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
