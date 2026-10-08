//! Toolkit-independent grid state. The UI layer draws only the cells [`GridCache`] holds:
//! the visible rows plus a buffer above and below (spec section 7). Memory depends on the
//! viewport, never on the row count.

mod nav;
mod sizes;

pub use nav::{Bounds, Cell, Key, Mods, Selection};
pub use sizes::{Sizes, Viewport, MIN_SIZE};

use data_model::{RowBlock, TableSource};
use std::ops::Range;

/// The rows the grid currently holds as display text: the visible window plus a buffer.
#[derive(Default)]
pub struct GridCache {
    block: RowBlock,
    cols: u32,
}

impl GridCache {
    /// Make sure every available row of `visible` is held. When one is missing, reload
    /// `visible` widened by `buffer` rows on each side. Returns true if it reloaded.
    pub fn ensure(
        &mut self,
        visible: Range<u64>,
        buffer: u64,
        source: &mut dyn TableSource,
    ) -> bool {
        let total = source.row_count();
        let need = visible.start.min(total)..visible.end.min(total);
        let held = self.block.rows();
        if need.is_empty() || (held.start <= need.start && need.end <= held.end) {
            return false;
        }
        let want = need.start.saturating_sub(buffer)..need.end.saturating_add(buffer).min(total);
        source.read_rows(want.clone(), &mut self.block);
        for r in self.block.rows() {
            self.cols = self.cols.max(self.block.cells_in_row(r));
        }
        true
    }

    pub fn cell(&self, row: u64, col: u32) -> Option<&str> {
        self.block.cell(row, col)
    }

    /// Widest row seen so far; the grid shows this many columns.
    pub fn col_count(&self) -> u32 {
        self.cols
    }

    /// Count at least `cols` columns: a cell was just set there, before the rows reload.
    pub fn widen(&mut self, cols: u32) {
        self.cols = self.cols.max(cols);
    }

    /// The widest text in column `col` over `rows`, by `measure` (pixels). Autofit
    /// (GRID-5) passes only the visible rows, so its cost doesn't depend on the row count.
    /// `None` when those rows hold no text in the column.
    pub fn widest(
        &self,
        col: u32,
        rows: Range<u64>,
        mut measure: impl FnMut(&str) -> f64,
    ) -> Option<f64> {
        rows.filter_map(|r| self.cell(r, col))
            .filter(|t| !t.is_empty())
            .map(&mut measure)
            .reduce(f64::max)
    }

    pub fn heap_bytes(&self) -> usize {
        self.block.heap_bytes()
    }

    /// Forget the held rows (keeping the memory) so the next `ensure` reloads them; call
    /// after an edit changes what they show.
    pub fn invalidate(&mut self) {
        self.block.reset(0);
    }

    /// Forget everything about the old table (rows and column count); call when the grid
    /// switches to a different table, e.g. the same file read with another delimiter.
    pub fn reset(&mut self) {
        self.block.reset(0);
        self.cols = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_a_window_not_the_whole_file() {
        let sizes = Sizes::new(24.0);
        let v = Viewport {
            scroll: 2_400_000.0,
            extent: 800.0,
            sizes: &sizes,
            count: 2_103_814,
        };
        let r = v.visible(10);
        assert_eq!(r.start, 99_990);
        assert!(r.end - r.start < 100);
    }

    #[test]
    fn visible_columns_clip_to_the_table() {
        let sizes = Sizes::new(100.0);
        let c = Viewport {
            scroll: 250.0,
            extent: 400.0,
            sizes: &sizes,
            count: 5,
        };
        assert_eq!(c.visible(0), 2..5);
        assert_eq!(c.pos(2), -50.0);
    }

    /// One column of the given values.
    struct Column(Vec<&'static str>);

    impl TableSource for Column {
        fn row_count(&self) -> u64 {
            self.0.len() as u64
        }
        fn is_complete(&self) -> bool {
            true
        }
        fn read_rows(&mut self, rows: Range<u64>, out: &mut RowBlock) {
            out.reset(rows.start);
            for r in rows.start..rows.end.min(self.row_count()) {
                out.push_cell(self.0[r as usize].as_bytes());
                out.end_row();
            }
        }
    }

    #[test]
    fn autofit_measures_only_the_visible_rows() {
        // Row 30 is held in the cache's buffer but is off screen, so it doesn't count.
        let mut values = vec!["abc"; 200];
        values[5] = "abcdefgh";
        values[7] = "";
        values[30] = "a value far wider than anything on screen";
        let mut src = Column(values);
        let mut cache = GridCache::default();
        let visible = 0..20;
        cache.ensure(visible.clone(), 50, &mut src);
        assert!(cache.cell(30, 0).is_some(), "the wide value is cached");
        let mut measured = 0;
        let width = cache.widest(0, visible, |t| {
            measured += 1;
            t.len() as f64 * 7.0
        });
        assert_eq!(width, Some(56.0));
        assert_eq!(measured, 19, "each non-empty visible cell, once");
        assert_eq!(cache.widest(1, 0..20, |_| 1.0), None, "no such column");
    }

    /// Fake table of any size: every row has 8 cells of fixed-width text.
    struct Synthetic {
        rows: u64,
        reads: u32,
    }

    impl TableSource for Synthetic {
        fn row_count(&self) -> u64 {
            self.rows
        }
        fn is_complete(&self) -> bool {
            true
        }
        fn read_rows(&mut self, rows: Range<u64>, out: &mut RowBlock) {
            self.reads += 1;
            out.reset(rows.start);
            for r in rows.start..rows.end.min(self.rows) {
                for c in 0..8u64 {
                    out.push_cell(format!("{:012}", r * 8 + c).as_bytes());
                }
                out.end_row();
            }
        }
    }

    /// Scroll through the table the way a user flicks and drags, returning peak cache memory.
    fn scroll_through(total_rows: u64) -> usize {
        let mut src = Synthetic {
            rows: total_rows,
            reads: 0,
        };
        let mut cache = GridCache::default();
        let mut peak = 0;
        let sizes = Sizes::new(22.0);
        let mut v = Viewport {
            scroll: 0.0,
            extent: 1000.0,
            sizes: &sizes,
            count: total_rows,
        };
        for step in 0..2_000u64 {
            v.scroll = if step % 50 == 0 {
                (step * 7_919 % total_rows) as f64 * 22.0 // a scrollbar jump
            } else {
                v.scroll + 37.0
            };
            let visible = v.visible(0);
            cache.ensure(visible.clone(), 50, &mut src);
            for r in visible {
                assert_eq!(cache.cell(r, 3).unwrap(), format!("{:012}", r * 8 + 3));
            }
            peak = peak.max(cache.heap_bytes());
        }
        peak
    }

    #[test]
    fn memory_is_the_same_at_any_row_count() {
        let small = scroll_through(10_000);
        let huge = scroll_through(10_000_000_000);
        assert_eq!(small, huge);
        assert!(
            huge < 64 * 1024,
            "cache holds about one screen plus buffer: {huge} bytes"
        );
    }

    #[test]
    fn small_scrolls_reuse_the_buffer() {
        let mut src = Synthetic {
            rows: 1_000_000,
            reads: 0,
        };
        let mut cache = GridCache::default();
        let sizes = Sizes::new(22.0);
        let mut v = Viewport {
            scroll: 22_000.0,
            extent: 1000.0,
            sizes: &sizes,
            count: 1_000_000,
        };
        for _ in 0..60 {
            cache.ensure(v.visible(0), 50, &mut src);
            v.scroll += 11.0; // 30 rows in total
        }
        assert_eq!(src.reads, 1);
        assert_eq!(cache.col_count(), 8);
    }

    #[test]
    fn picks_up_rows_that_arrive_while_indexing() {
        let mut src = Synthetic { rows: 10, reads: 0 };
        let mut cache = GridCache::default();
        assert!(cache.ensure(0..40, 5, &mut src));
        assert_eq!(cache.cell(9, 0).map(str::len), Some(12));
        assert_eq!(cache.cell(10, 0), None);
        assert!(!cache.ensure(0..40, 5, &mut src), "nothing new to load");
        src.rows = 100;
        assert!(cache.ensure(0..40, 5, &mut src));
        assert!(cache.cell(39, 0).is_some());
    }

    #[test]
    fn invalidate_makes_the_next_ensure_reload() {
        let mut src = Synthetic {
            rows: 1_000,
            reads: 0,
        };
        let mut cache = GridCache::default();
        cache.ensure(0..40, 5, &mut src);
        assert!(!cache.ensure(0..40, 5, &mut src));
        cache.invalidate();
        assert!(cache.ensure(0..40, 5, &mut src));
        assert_eq!(src.reads, 2);
    }

    #[test]
    fn hit_testing_maps_points_to_cells() {
        let rows = Sizes::new(22.0);
        let v = Viewport {
            scroll: 22.0 * 1_000_000.0 + 5.0,
            extent: 500.0,
            sizes: &rows,
            count: 2_000_000,
        };
        assert_eq!(v.at(0.0), Some(1_000_000));
        assert_eq!(v.at(16.9), Some(1_000_000));
        assert_eq!(v.at(17.0), Some(1_000_001));
        assert_eq!(v.at(-1.0), None);
        let end = Viewport {
            scroll: 0.0,
            count: 3,
            ..v
        };
        assert_eq!(end.at(70.0), None, "below the last row");

        let cols = Sizes::new(100.0);
        let c = Viewport {
            scroll: 250.0,
            extent: 400.0,
            sizes: &cols,
            count: 5,
        };
        assert_eq!(c.at(0.0), Some(2));
        assert_eq!(c.at(49.9), Some(2));
        assert_eq!(c.at(50.0), Some(3));
        assert_eq!(c.at(300.0), None, "right of the last column");
    }

    #[test]
    fn reset_forgets_the_column_count_of_the_old_table() {
        let mut wide = Synthetic {
            rows: 100,
            reads: 0,
        }; // 8 columns
        let mut cache = GridCache::default();
        cache.ensure(0..10, 5, &mut wide);
        assert_eq!(cache.col_count(), 8);
        cache.invalidate();
        assert_eq!(cache.col_count(), 8, "an edit keeps the width seen so far");
        cache.reset();
        assert_eq!(cache.col_count(), 0, "a different table starts over");
        assert!(cache.ensure(0..10, 5, &mut wide));
    }
}
